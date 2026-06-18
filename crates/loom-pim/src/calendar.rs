use crate::cbor::{self, Value};
use icalendar::parser as ical_parser;
use icalendar::{
    Component as IcalComponent, Event as IcalEvent, Property as IcalProperty, Todo as IcalTodo,
};
use loom_rrule::{RRule, RecurrenceSet};
use loom_types::error::{LoomError, Result};
use time::{Date, Month, PrimitiveDateTime, Time};

pub use time::{
    Date as IcalDate, Month as IcalMonth, PrimitiveDateTime as DateTime, Time as IcalTime,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Component {
    Event,
    Todo,
}

impl Component {
    pub const fn as_str(self) -> &'static str {
        match self {
            Component::Event => "event",
            Component::Todo => "todo",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "event" => Ok(Component::Event),
            "todo" => Ok(Component::Todo),
            other => Err(LoomError::corrupt(format!(
                "calendar: bad component {other:?}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComponentField(pub Component);

impl Default for ComponentField {
    fn default() -> Self {
        ComponentField(Component::Event)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CalendarEntry {
    pub uid: String,
    pub component: ComponentField,
    pub summary: String,
    pub dtstart: String,
    pub dtend: Option<String>,
    pub tzid: Option<String>,
    pub rrule: Option<String>,
    pub rdate: Vec<String>,
    pub exdate: Vec<String>,
    pub status: Option<String>,
    pub extra: Vec<(String, String)>,
}

impl CalendarEntry {
    pub fn event(
        uid: impl Into<String>,
        summary: impl Into<String>,
        dtstart: impl Into<String>,
    ) -> Self {
        CalendarEntry {
            uid: uid.into(),
            component: ComponentField(Component::Event),
            summary: summary.into(),
            dtstart: dtstart.into(),
            ..Default::default()
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut map: Vec<(Value, Value)> = Vec::new();
        let put = |map: &mut Vec<(Value, Value)>, key: &str, value: Value| {
            map.push((Value::Text(key.into()), value));
        };
        put(&mut map, "uid", Value::Text(self.uid.clone()));
        put(
            &mut map,
            "component",
            Value::Text(self.component.0.as_str().into()),
        );
        put(&mut map, "summary", Value::Text(self.summary.clone()));
        put(&mut map, "dtstart", Value::Text(self.dtstart.clone()));
        if let Some(value) = &self.dtend {
            put(&mut map, "dtend", Value::Text(value.clone()));
        }
        if let Some(value) = &self.tzid {
            put(&mut map, "tzid", Value::Text(value.clone()));
        }
        if let Some(value) = &self.rrule {
            put(&mut map, "rrule", Value::Text(value.clone()));
        }
        if !self.rdate.is_empty() {
            put(&mut map, "rdate", text_array(&self.rdate));
        }
        if !self.exdate.is_empty() {
            put(&mut map, "exdate", text_array(&self.exdate));
        }
        if let Some(value) = &self.status {
            put(&mut map, "status", Value::Text(value.clone()));
        }
        if !self.extra.is_empty() {
            let items = self
                .extra
                .iter()
                .map(|(key, value)| {
                    Value::Array(vec![Value::Text(key.clone()), Value::Text(value.clone())])
                })
                .collect();
            put(&mut map, "extra", Value::Array(items));
        }
        cbor::encode(&Value::Map(map))
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let pairs = cbor::as_map(cbor::decode(bytes)?)?;
        let get = |key: &str| {
            pairs
                .iter()
                .find(|(field, _)| matches!(field, Value::Text(text) if text == key))
                .map(|(_, value)| value.clone())
        };
        let text = |key: &str| -> Result<String> {
            cbor::as_text(
                get(key).ok_or_else(|| LoomError::corrupt(format!("calendar: missing {key}")))?,
            )
        };
        let opt_text =
            |key: &str| -> Result<Option<String>> { get(key).map(cbor::as_text).transpose() };
        let opt_list = |key: &str| -> Result<Vec<String>> {
            match get(key) {
                Some(value) => cbor::as_array(value)?
                    .into_iter()
                    .map(cbor::as_text)
                    .collect(),
                None => Ok(Vec::new()),
            }
        };
        let extra = match get("extra") {
            Some(value) => {
                let mut out = Vec::new();
                for item in cbor::as_array(value)? {
                    let mut fields = cbor::Fields::new(cbor::as_array(item)?);
                    let key = fields.text()?;
                    let value = fields.text()?;
                    fields.end()?;
                    out.push((key, value));
                }
                out
            }
            None => Vec::new(),
        };
        Ok(CalendarEntry {
            uid: text("uid")?,
            component: ComponentField(Component::parse(&text("component")?)?),
            summary: text("summary")?,
            dtstart: text("dtstart")?,
            dtend: opt_text("dtend")?,
            tzid: opt_text("tzid")?,
            rrule: opt_text("rrule")?,
            rdate: opt_list("rdate")?,
            exdate: opt_list("exdate")?,
            status: opt_text("status")?,
            extra,
        })
    }

    pub fn occurrence_starts(
        &self,
        from: PrimitiveDateTime,
        to: PrimitiveDateTime,
    ) -> Result<Vec<PrimitiveDateTime>> {
        if self.dtstart.is_empty() {
            return Ok(Vec::new());
        }
        let set = self.recurrence_set()?;
        Ok(set.expand(from, to))
    }

    pub fn has_valid_dtstart(&self) -> bool {
        if self.dtstart.is_empty() {
            return self.component.0 == Component::Todo;
        }
        parse_ical_dt(&self.dtstart).is_some()
    }

    fn recurrence_set(&self) -> Result<RecurrenceSet> {
        let dtstart = parse_ical_dt(&self.dtstart).ok_or_else(|| {
            LoomError::invalid(format!("calendar: bad DTSTART {:?}", self.dtstart))
        })?;
        let rrules = match &self.rrule {
            Some(rule) => vec![
                RRule::parse(rule)
                    .map_err(|e| LoomError::invalid(format!("calendar: bad RRULE: {e}")))?,
            ],
            None => Vec::new(),
        };
        let rdates = self
            .rdate
            .iter()
            .filter_map(|item| parse_ical_dt(item))
            .collect();
        let exdates = self
            .exdate
            .iter()
            .filter_map(|item| parse_ical_dt(item))
            .collect();
        Ok(RecurrenceSet {
            dtstart,
            rrules,
            rdates,
            exdates,
        })
    }

    pub fn to_ics(&self) -> String {
        fn fill<C: IcalComponent>(component: &mut C, entry: &CalendarEntry) {
            component.add_property("UID", entry.uid.as_str());
            component.add_property("SUMMARY", entry.summary.as_str());
            if !entry.dtstart.is_empty() {
                component.append_property(dt_prop(
                    "DTSTART",
                    &entry.dtstart,
                    entry.tzid.as_deref(),
                ));
            }
            if let Some(end) = &entry.dtend {
                component.append_property(dt_prop("DTEND", end, entry.tzid.as_deref()));
            }
            if let Some(rule) = &entry.rrule {
                component.add_property("RRULE", rule.as_str());
            }
            if !entry.rdate.is_empty() {
                component.add_property("RDATE", entry.rdate.join(",").as_str());
            }
            if !entry.exdate.is_empty() {
                component.add_property("EXDATE", entry.exdate.join(",").as_str());
            }
            if let Some(status) = &entry.status {
                component.add_property("STATUS", status.as_str());
            }
            for (key, value) in &entry.extra {
                component.add_property(key.as_str(), value.as_str());
            }
        }
        let mut document = icalendar::Calendar::new();
        match self.component.0 {
            Component::Event => {
                let mut event = IcalEvent::new();
                fill(&mut event, self);
                document.push(event.done());
            }
            Component::Todo => {
                let mut todo = IcalTodo::new();
                fill(&mut todo, self);
                document.push(todo.done());
            }
        }
        document.to_string()
    }

    pub fn from_ics(input: &str) -> Result<Self> {
        let unfolded = ical_parser::unfold(input);
        let document = ical_parser::read_calendar(&unfolded)
            .map_err(|e| LoomError::invalid(format!("calendar: {e}")))?;
        let component = document
            .components
            .iter()
            .find(|component| matches!(component.name.as_str(), "VEVENT" | "VTODO"))
            .ok_or_else(|| LoomError::invalid("calendar: no VEVENT/VTODO component"))?;
        let mut entry = CalendarEntry {
            component: ComponentField(if component.name.as_str() == "VTODO" {
                Component::Todo
            } else {
                Component::Event
            }),
            ..Default::default()
        };
        for property in &component.properties {
            let value = property.val.as_str().to_string();
            match property.name.as_str() {
                "UID" => entry.uid = value,
                "SUMMARY" => entry.summary = value,
                "DTSTART" => {
                    entry.dtstart = value;
                    entry.tzid = ical_tzid(property);
                }
                "DTEND" => {
                    entry.dtend = Some(value);
                    if entry.tzid.is_none() {
                        entry.tzid = ical_tzid(property);
                    }
                }
                "RRULE" => entry.rrule = Some(value),
                "RDATE" => entry.rdate = value.split(',').map(str::to_string).collect(),
                "EXDATE" => entry.exdate = value.split(',').map(str::to_string).collect(),
                "STATUS" => entry.status = Some(value),
                "DTSTAMP" => {}
                other => entry.extra.push((other.to_string(), value)),
            }
        }
        if entry.uid.is_empty() {
            return Err(LoomError::invalid("calendar: iCalendar missing UID"));
        }
        if entry.dtstart.is_empty() && entry.component.0 == Component::Event {
            return Err(LoomError::invalid("calendar: iCalendar missing DTSTART"));
        }
        Ok(entry)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CollectionMeta {
    pub display_name: String,
    pub component_set: Vec<Component>,
}

impl CollectionMeta {
    pub fn encode(&self) -> Vec<u8> {
        let components = self
            .component_set
            .iter()
            .map(|component| Value::Text(component.as_str().into()))
            .collect();
        cbor::encode(&Value::Map(vec![
            (
                Value::Text("display_name".into()),
                Value::Text(self.display_name.clone()),
            ),
            (
                Value::Text("component_set".into()),
                Value::Array(components),
            ),
        ]))
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let pairs = cbor::as_map(cbor::decode(bytes)?)?;
        let get = |key: &str| {
            pairs
                .iter()
                .find(|(field, _)| matches!(field, Value::Text(text) if text == key))
                .map(|(_, value)| value.clone())
        };
        let display_name = match get("display_name") {
            Some(value) => cbor::as_text(value)?,
            None => String::new(),
        };
        let component_set = match get("component_set") {
            Some(value) => cbor::as_array(value)?
                .into_iter()
                .map(|component| Component::parse(&cbor::as_text(component)?))
                .collect::<Result<Vec<_>>>()?,
            None => Vec::new(),
        };
        Ok(CollectionMeta {
            display_name,
            component_set,
        })
    }
}

fn text_array(items: &[String]) -> Value {
    Value::Array(items.iter().map(|item| Value::Text(item.clone())).collect())
}

fn dt_prop(name: &str, value: &str, tzid: Option<&str>) -> IcalProperty {
    let mut property = IcalProperty::new(name, value);
    if let Some(tzid) = tzid.filter(|_| value.contains('T') && !value.ends_with('Z')) {
        property.add_parameter("TZID", tzid);
    }
    property.done()
}

fn ical_tzid(property: &ical_parser::Property) -> Option<String> {
    property
        .params
        .iter()
        .find(|param| param.key.as_str() == "TZID")
        .and_then(|param| param.val.as_ref())
        .map(|value| value.as_str().to_string())
}

fn parse_ical_dt(value: &str) -> Option<PrimitiveDateTime> {
    let core = value.strip_suffix('Z').unwrap_or(value);
    let (date_part, time_part) = match core.split_once('T') {
        Some((date_part, time_part)) => (date_part, Some(time_part)),
        None => (core, None),
    };
    if date_part.len() != 8 || !date_part.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let date = Date::from_calendar_date(
        date_part[0..4].parse().ok()?,
        Month::try_from(date_part[4..6].parse::<u8>().ok()?).ok()?,
        date_part[6..8].parse().ok()?,
    )
    .ok()?;
    let time = match time_part {
        Some(time_part)
            if time_part.len() == 6 && time_part.bytes().all(|b| b.is_ascii_digit()) =>
        {
            Time::from_hms(
                time_part[0..2].parse().ok()?,
                time_part[2..4].parse().ok()?,
                time_part[4..6].parse().ok()?,
            )
            .ok()?
        }
        Some(_) => return None,
        None => Time::MIDNIGHT,
    };
    Some(PrimitiveDateTime::new(date, time))
}
