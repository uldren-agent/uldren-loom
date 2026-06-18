use crate::cbor::{self, Value};
use loom_types::error::{LoomError, Result};
use vcard4::Vcard;
use vcard4::parameter::{Parameters, TypeParameter};
use vcard4::property::{
    AnyProperty, ExtensionProperty, TextListProperty, TextOrUriProperty, TextProperty,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypedValue {
    pub value: String,
    pub kind: Option<String>,
}

impl TypedValue {
    pub fn new(value: impl Into<String>) -> Self {
        TypedValue {
            value: value.into(),
            kind: None,
        }
    }

    pub fn typed(value: impl Into<String>, kind: impl Into<String>) -> Self {
        TypedValue {
            value: value.into(),
            kind: Some(kind.into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ContactEntry {
    pub uid: String,
    pub full_name: String,
    pub n: Option<String>,
    pub emails: Vec<TypedValue>,
    pub tels: Vec<TypedValue>,
    pub org: Option<String>,
    pub title: Option<String>,
    pub extra: Vec<(String, String)>,
    pub vcard3_properties: Vec<VcardProperty>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VcardProperty {
    pub group: Option<String>,
    pub name: String,
    pub params: Vec<(String, Vec<String>)>,
    pub value: String,
}

impl ContactEntry {
    pub fn new(uid: impl Into<String>, full_name: impl Into<String>) -> Self {
        ContactEntry {
            uid: uid.into(),
            full_name: full_name.into(),
            ..Default::default()
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut map: Vec<(Value, Value)> = Vec::new();
        let put = |map: &mut Vec<(Value, Value)>, key: &str, value: Value| {
            map.push((Value::Text(key.into()), value));
        };
        put(&mut map, "uid", Value::Text(self.uid.clone()));
        put(&mut map, "fn", Value::Text(self.full_name.clone()));
        if let Some(value) = &self.n {
            put(&mut map, "n", Value::Text(value.clone()));
        }
        if !self.emails.is_empty() {
            put(&mut map, "emails", typed_array(&self.emails));
        }
        if !self.tels.is_empty() {
            put(&mut map, "tels", typed_array(&self.tels));
        }
        if let Some(value) = &self.org {
            put(&mut map, "org", Value::Text(value.clone()));
        }
        if let Some(value) = &self.title {
            put(&mut map, "title", Value::Text(value.clone()));
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
        if !self.vcard3_properties.is_empty() {
            let items = self
                .vcard3_properties
                .iter()
                .map(|property| {
                    Value::Array(vec![
                        property
                            .group
                            .as_ref()
                            .map_or(Value::Null, |group| Value::Text(group.clone())),
                        Value::Text(property.name.clone()),
                        Value::Array(
                            property
                                .params
                                .iter()
                                .map(|(name, values)| {
                                    Value::Array(vec![
                                        Value::Text(name.clone()),
                                        Value::Array(
                                            values
                                                .iter()
                                                .map(|value| Value::Text(value.clone()))
                                                .collect(),
                                        ),
                                    ])
                                })
                                .collect(),
                        ),
                        Value::Text(property.value.clone()),
                    ])
                })
                .collect();
            put(&mut map, "vcard3_properties", Value::Array(items));
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
                get(key).ok_or_else(|| LoomError::corrupt(format!("contacts: missing {key}")))?,
            )
        };
        let opt_text =
            |key: &str| -> Result<Option<String>> { get(key).map(cbor::as_text).transpose() };
        let typed = |key: &str| -> Result<Vec<TypedValue>> {
            match get(key) {
                Some(value) => cbor::as_array(value)?
                    .into_iter()
                    .map(|item| {
                        let mut fields = cbor::Fields::new(cbor::as_array(item)?);
                        let value = fields.text()?;
                        let kind = match fields.next_field() {
                            Ok(Value::Text(text)) => Some(text),
                            Ok(_) | Err(_) => None,
                        };
                        Ok(TypedValue { value, kind })
                    })
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
        let vcard3_properties = match get("vcard3_properties") {
            Some(value) => {
                let mut out = Vec::new();
                for item in cbor::as_array(value)? {
                    let mut fields = cbor::Fields::new(cbor::as_array(item)?);
                    let group = match fields.next_field()? {
                        Value::Null => None,
                        Value::Text(group) => Some(group),
                        _ => return Err(LoomError::corrupt("contacts: invalid vCard group")),
                    };
                    let name = fields.text()?;
                    let mut params = Vec::new();
                    for param in cbor::as_array(fields.next_field()?)? {
                        let mut param_fields = cbor::Fields::new(cbor::as_array(param)?);
                        let param_name = param_fields.text()?;
                        let values = cbor::as_array(param_fields.next_field()?)?
                            .into_iter()
                            .map(cbor::as_text)
                            .collect::<Result<Vec<_>>>()?;
                        param_fields.end()?;
                        params.push((param_name, values));
                    }
                    let value = fields.text()?;
                    fields.end()?;
                    out.push(VcardProperty {
                        group,
                        name,
                        params,
                        value,
                    });
                }
                out
            }
            None => Vec::new(),
        };
        Ok(ContactEntry {
            uid: text("uid")?,
            full_name: text("fn")?,
            n: opt_text("n")?,
            emails: typed("emails")?,
            tels: typed("tels")?,
            org: opt_text("org")?,
            title: opt_text("title")?,
            extra,
            vcard3_properties,
        })
    }

    pub fn to_vcard(&self) -> String {
        let mut card = Vcard::new(self.full_name.clone());
        card.uid = Some(TextOrUriProperty::Text(TextProperty {
            group: None,
            value: self.uid.clone(),
            parameters: None,
        }));
        if let Some(name) = &self.n {
            let parts = name.split(';').map(str::to_string).collect();
            card.name = Some(TextListProperty::new_semi_colon(parts));
        }
        for email in &self.emails {
            card.email.push(typed_text(&email.value, &email.kind));
        }
        for tel in &self.tels {
            card.tel
                .push(TextOrUriProperty::Text(typed_text(&tel.value, &tel.kind)));
        }
        if let Some(org) = &self.org {
            card.org
                .push(TextListProperty::new_semi_colon(vec![org.clone()]));
        }
        if let Some(title) = &self.title {
            card.title.push(TextProperty {
                group: None,
                value: title.clone(),
                parameters: None,
            });
        }
        for (key, value) in &self.extra {
            card.extensions.push(ExtensionProperty {
                name: key.clone(),
                group: None,
                value: AnyProperty::Text(value.clone()),
                parameters: None,
            });
        }
        card.to_string()
    }

    pub fn to_vcard3(&self) -> String {
        let mut lines = vec![
            "BEGIN:VCARD".to_string(),
            "VERSION:3.0".to_string(),
            format!("FN:{}", vcard_text(&self.full_name)),
        ];
        if let Some(name) = &self.n {
            let value = name
                .split(';')
                .map(vcard_text)
                .collect::<Vec<_>>()
                .join(";");
            lines.push(format!("N:{value}"));
        }
        if let Some(title) = &self.title {
            lines.push(format!("TITLE:{}", vcard_text(title)));
        }
        if let Some(org) = &self.org {
            lines.push(format!("ORG:{}", vcard_text(org)));
        }
        for tel in &self.tels {
            lines.push(typed_vcard3_line("TEL", tel, false));
        }
        for email in &self.emails {
            lines.push(typed_vcard3_line("EMAIL", email, true));
        }
        lines.push(format!("UID:{}", vcard_text(&self.uid)));
        for property in &self.vcard3_properties {
            lines.push(property.to_vcard_line());
        }
        for (key, value) in &self.extra {
            if key.starts_with("X-") {
                lines.push(format!(
                    "{}:{}",
                    key.to_ascii_uppercase(),
                    vcard_text(value)
                ));
            }
        }
        lines.push("END:VCARD".to_string());
        lines
            .into_iter()
            .flat_map(|line| fold_vcard_line(&line))
            .collect::<Vec<_>>()
            .join("\r\n")
            + "\r\n"
    }

    pub fn from_vcard(input: &str) -> Result<Self> {
        if vcard_version(input)?.as_deref() == Some("3.0") {
            return ContactEntry::from_vcard3(input);
        }
        let cards = vcard4::parse(input)
            .map_err(|e| LoomError::invalid(format!("contacts: vCard parse: {e}")))?;
        let card = cards
            .into_iter()
            .next()
            .ok_or_else(|| LoomError::invalid("contacts: no VCARD component"))?;

        let mut entry = ContactEntry::default();
        if let Some(full_name) = card.formatted_name.first() {
            entry.full_name = full_name.value.clone();
        }
        if let Some(uid) = &card.uid {
            entry.uid = text_or_uri_value(uid);
        }
        if let Some(name) = &card.name {
            entry.n = Some(name.value.join(";"));
        }
        for email in &card.email {
            entry.emails.push(TypedValue {
                value: email.value.clone(),
                kind: param_to_kind(&email.parameters),
            });
        }
        for tel in &card.tel {
            let (value, params) = match tel {
                TextOrUriProperty::Text(text) => (text.value.clone(), &text.parameters),
                TextOrUriProperty::Uri(uri) => (uri.value.to_string(), &uri.parameters),
            };
            entry.tels.push(TypedValue {
                value,
                kind: param_to_kind(params),
            });
        }
        if let Some(org) = card.org.first() {
            entry.org = Some(org.value.join(";"));
        }
        if let Some(title) = card.title.first() {
            entry.title = Some(title.value.clone());
        }
        for ext in &card.extensions {
            match ext.name.as_str() {
                "VERSION" | "PRODID" | "REV" => {}
                _ => {
                    let value = match &ext.value {
                        AnyProperty::Text(text) => text.clone(),
                        other => other.to_string(),
                    };
                    entry.extra.push((ext.name.clone(), value));
                }
            }
        }

        if entry.uid.is_empty() {
            return Err(LoomError::invalid("contacts: vCard missing UID"));
        }
        if entry.full_name.is_empty() {
            return Err(LoomError::invalid("contacts: vCard missing FN"));
        }
        Ok(entry)
    }

    fn from_vcard3(input: &str) -> Result<Self> {
        let mut entry = ContactEntry::default();
        let mut saw_begin = false;
        let mut saw_end = false;
        let mut saw_version = false;
        for line in vcard_content_lines(input)? {
            let property = parse_vcard_property(&line)?;
            match property.name.as_str() {
                "BEGIN" => {
                    if !property.value.eq_ignore_ascii_case("VCARD") {
                        return Err(LoomError::invalid("contacts: vCard BEGIN must be VCARD"));
                    }
                    saw_begin = true;
                }
                "END" => {
                    if !property.value.eq_ignore_ascii_case("VCARD") {
                        return Err(LoomError::invalid("contacts: vCard END must be VCARD"));
                    }
                    saw_end = true;
                }
                "VERSION" => {
                    if property.value.trim() != "3.0" {
                        return Err(LoomError::invalid("contacts: vCard version must be 3.0"));
                    }
                    saw_version = true;
                }
                "FN" => entry.full_name = vcard_unescape_text(&property.value),
                "N" => entry.n = Some(vcard_unescape_structured_text(&property.value, ';')),
                "UID" => entry.uid = vcard_unescape_text(&property.value),
                "EMAIL" => entry.emails.push(TypedValue {
                    value: vcard_unescape_text(&property.value),
                    kind: vcard_type_param(&property.params, &["internet", "pref"]),
                }),
                "TEL" => entry.tels.push(TypedValue {
                    value: vcard_unescape_text(&property.value),
                    kind: vcard_type_param(&property.params, &["voice", "pref"]),
                }),
                "ORG" => entry.org = Some(vcard_unescape_structured_text(&property.value, ';')),
                "TITLE" => entry.title = Some(vcard_unescape_text(&property.value)),
                "PROFILE" | "NAME" | "SOURCE" | "NICKNAME" | "PHOTO" | "BDAY" | "ADR" | "LABEL"
                | "MAILER" | "TZ" | "GEO" | "ROLE" | "LOGO" | "AGENT" | "CATEGORIES" | "NOTE"
                | "PRODID" | "REV" | "SORT-STRING" | "SOUND" | "URL" | "CLASS" | "KEY" => {
                    entry.vcard3_properties.push(property)
                }
                name if name.starts_with("X-") => entry.vcard3_properties.push(property),
                _ => entry.vcard3_properties.push(property),
            }
        }
        if !saw_begin {
            return Err(LoomError::invalid("contacts: vCard missing BEGIN"));
        }
        if !saw_end {
            return Err(LoomError::invalid("contacts: vCard missing END"));
        }
        if !saw_version {
            return Err(LoomError::invalid("contacts: vCard missing VERSION"));
        }
        if entry.uid.is_empty() {
            return Err(LoomError::invalid("contacts: vCard missing UID"));
        }
        if entry.full_name.is_empty() {
            return Err(LoomError::invalid("contacts: vCard missing FN"));
        }
        if entry.n.is_none() {
            entry.n = Some(format!("{};;;;", entry.full_name));
        }
        Ok(entry)
    }
}

impl VcardProperty {
    fn to_vcard_line(&self) -> String {
        let mut line = String::new();
        if let Some(group) = &self.group {
            line.push_str(group);
            line.push('.');
        }
        line.push_str(&self.name);
        for (name, values) in &self.params {
            line.push(';');
            line.push_str(name);
            line.push('=');
            line.push_str(&values.join(","));
        }
        line.push(':');
        line.push_str(&self.value);
        line
    }
}

fn typed_vcard3_line(name: &str, value: &TypedValue, include_internet: bool) -> String {
    let mut line = name.to_string();
    if include_internet {
        line.push_str(";TYPE=INTERNET");
    }
    if let Some(kind) = &value.kind {
        let kind = vcard3_param_value(kind);
        if !kind.is_empty() {
            line.push_str(";TYPE=");
            line.push_str(&kind);
        }
    }
    line.push(':');
    line.push_str(&vcard_text(&value.value));
    line
}

fn vcard_version(input: &str) -> Result<Option<String>> {
    for line in vcard_content_lines(input)? {
        let property = parse_vcard_property(&line)?;
        if property.name == "VERSION" {
            return Ok(Some(property.value.trim().to_string()));
        }
    }
    Ok(None)
}

fn vcard_content_lines(input: &str) -> Result<Vec<String>> {
    let normalized = input.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines: Vec<String> = Vec::new();
    for raw in normalized.split('\n') {
        if raw.is_empty() {
            continue;
        }
        if raw.starts_with(' ') || raw.starts_with('\t') {
            let Some(line) = lines.last_mut() else {
                return Err(LoomError::invalid("contacts: invalid folded vCard line"));
            };
            line.push_str(&raw[1..]);
        } else {
            lines.push(raw.to_string());
        }
    }
    Ok(lines)
}

fn parse_vcard_property(line: &str) -> Result<VcardProperty> {
    let Some((left, value)) = line.split_once(':') else {
        return Err(LoomError::invalid("contacts: invalid vCard content line"));
    };
    let parts = split_unquoted(left, ';');
    let Some(name_part) = parts.first() else {
        return Err(LoomError::invalid("contacts: missing vCard property name"));
    };
    let (group, name) = match name_part.rsplit_once('.') {
        Some((group, name)) => (Some(group.to_string()), name.to_string()),
        None => (None, name_part.to_string()),
    };
    let name = name.to_ascii_uppercase();
    if !vcard_token_is_valid(&name) {
        return Err(LoomError::invalid("contacts: invalid vCard property name"));
    }
    let group = group
        .filter(|group| !group.is_empty())
        .map(|group| {
            if vcard_group_is_valid(&group) {
                Ok(group)
            } else {
                Err(LoomError::invalid("contacts: invalid vCard group"))
            }
        })
        .transpose()?;
    let mut params = Vec::new();
    for param in parts.iter().skip(1) {
        let (param_name, values) = match param.split_once('=') {
            Some((name, values)) => (
                name.to_ascii_uppercase(),
                split_unquoted(values, ',')
                    .into_iter()
                    .map(|value| unquote_param_value(value.trim()).to_string())
                    .collect::<Vec<_>>(),
            ),
            None => ("TYPE".to_string(), vec![param.trim().to_string()]),
        };
        if !vcard_token_is_valid(&param_name) || values.iter().any(|value| value.is_empty()) {
            return Err(LoomError::invalid("contacts: invalid vCard parameter"));
        }
        params.push((param_name, values));
    }
    Ok(VcardProperty {
        group,
        name,
        params,
        value: value.to_string(),
    })
}

fn split_unquoted(value: &str, delimiter: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut in_quote = false;
    let mut start = 0;
    for (index, ch) in value.char_indices() {
        if ch == '"' {
            in_quote = !in_quote;
        } else if ch == delimiter && !in_quote {
            parts.push(&value[start..index]);
            start = index + ch.len_utf8();
        }
    }
    parts.push(&value[start..]);
    parts
}

fn unquote_param_value(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(value)
}

fn vcard_token_is_valid(value: &str) -> bool {
    value.starts_with("X-")
        || value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
}

fn vcard_group_is_valid(value: &str) -> bool {
    value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
}

fn vcard_type_param(params: &[(String, Vec<String>)], ignored: &[&str]) -> Option<String> {
    params
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("TYPE"))
        .flat_map(|(_, values)| values)
        .map(|value| value.trim())
        .find(|value| {
            !value.is_empty()
                && !ignored
                    .iter()
                    .any(|ignored| value.eq_ignore_ascii_case(ignored))
        })
        .map(|value| value.to_ascii_lowercase())
}

fn vcard_unescape_structured_text(value: &str, delimiter: char) -> String {
    split_escaped(value, delimiter)
        .into_iter()
        .map(vcard_unescape_text)
        .collect::<Vec<_>>()
        .join(&delimiter.to_string())
}

fn split_escaped(value: &str, delimiter: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut escaped = false;
    let mut start = 0;
    for (index, ch) in value.char_indices() {
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == delimiter {
            parts.push(&value[start..index]);
            start = index + ch.len_utf8();
        }
    }
    parts.push(&value[start..]);
    parts
}

fn vcard_unescape_text(value: &str) -> String {
    let mut out = String::new();
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('n' | 'N') => out.push('\n'),
                Some(next) => out.push(next),
                None => out.push('\\'),
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn vcard3_param_value(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
        .collect::<String>()
        .to_ascii_uppercase()
}

fn vcard_text(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            ',' => out.push_str("\\,"),
            ';' => out.push_str("\\;"),
            _ => out.push(ch),
        }
    }
    out
}

fn fold_vcard_line(line: &str) -> Vec<String> {
    const LIMIT: usize = 75;
    let mut lines = Vec::new();
    let mut current = String::new();
    for ch in line.chars() {
        let limit = if lines.is_empty() { LIMIT } else { LIMIT - 1 };
        if !current.is_empty() && current.len() + ch.len_utf8() > limit {
            lines.push(current);
            current = " ".to_string();
        }
        current.push(ch);
    }
    lines.push(current);
    lines
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BookMeta {
    pub display_name: String,
}

impl BookMeta {
    pub fn encode(&self) -> Vec<u8> {
        cbor::encode(&Value::Map(vec![(
            Value::Text("display_name".into()),
            Value::Text(self.display_name.clone()),
        )]))
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let pairs = cbor::as_map(cbor::decode(bytes)?)?;
        let display_name = pairs
            .into_iter()
            .find(|(key, _)| matches!(key, Value::Text(text) if text == "display_name"))
            .map(|(_, value)| cbor::as_text(value))
            .transpose()?
            .unwrap_or_default();
        Ok(BookMeta { display_name })
    }
}

fn typed_array(items: &[TypedValue]) -> Value {
    Value::Array(
        items
            .iter()
            .map(|item| {
                let mut array = vec![Value::Text(item.value.clone())];
                if let Some(kind) = &item.kind {
                    array.push(Value::Text(kind.clone()));
                }
                Value::Array(array)
            })
            .collect(),
    )
}

fn kind_to_param(kind: &str) -> TypeParameter {
    kind.parse()
        .unwrap_or_else(|_| TypeParameter::Extension(kind.to_string()))
}

fn param_to_kind(params: &Option<Parameters>) -> Option<String> {
    params
        .as_ref()
        .and_then(|params| params.types.as_ref())
        .and_then(|types| types.first())
        .map(ToString::to_string)
}

fn typed_text(value: &str, kind: &Option<String>) -> TextProperty {
    TextProperty {
        group: None,
        value: value.to_string(),
        parameters: kind.as_ref().map(|kind| {
            let mut params = Parameters::default();
            params.types = Some(vec![kind_to_param(kind)]);
            params
        }),
    }
}

fn text_or_uri_value(prop: &TextOrUriProperty) -> String {
    match prop {
        TextOrUriProperty::Text(text) => text.value.clone(),
        TextOrUriProperty::Uri(uri) => uri.value.to_string(),
    }
}
