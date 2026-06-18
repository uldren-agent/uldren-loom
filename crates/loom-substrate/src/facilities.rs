use std::collections::BTreeSet;

use chrono::{DateTime, Datelike, NaiveDate, TimeZone, Utc};
use loom_codec::Value;
use loom_types::{Digest, LoomError, Result, WorkspaceId};
use serde_json::Value as JsonValue;

use crate::predicate::Predicate;
use crate::{canonical_labels, codec_error, string_array, validate_text};

pub const ATTACHMENT_SCHEMA: &str = "loom.substrate.attachment.v1";
pub const WATCH_SCHEMA: &str = "loom.substrate.watch.v1";
pub const SCOPE_MODE_SCHEMA: &str = "loom.substrate.scope-mode.v1";
pub const SAVED_QUERY_SCHEMA: &str = "loom.substrate.saved-query.v1";
pub const MENTION_SCHEMA: &str = "loom.substrate.mention.v1";
pub const FIELD_DEFINITION_SCHEMA: &str = "loom.substrate.field-definition.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanStatus {
    Pending,
    Clean,
    Blocked,
    Failed,
}

impl ScanStatus {
    const fn tag(self) -> u64 {
        match self {
            ScanStatus::Pending => 0,
            ScanStatus::Clean => 1,
            ScanStatus::Blocked => 2,
            ScanStatus::Failed => 3,
        }
    }

    fn from_tag(tag: u64) -> Result<Self> {
        match tag {
            0 => Ok(ScanStatus::Pending),
            1 => Ok(ScanStatus::Clean),
            2 => Ok(ScanStatus::Blocked),
            3 => Ok(ScanStatus::Failed),
            other => Err(LoomError::corrupt(format!(
                "unknown scan status tag {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentMeta {
    pub attachment_id: String,
    pub digest: Digest,
    pub name: String,
    pub media_type: String,
    pub size: u64,
    pub uploaded_by: WorkspaceId,
    pub created_at_ms: u64,
    pub scan_status: ScanStatus,
    pub retention_class: String,
}

impl AttachmentMeta {
    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(ATTACHMENT_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.attachment_id.clone()),
                Value::Text(self.digest.to_string()),
                Value::Text(self.name.clone()),
                Value::Text(self.media_type.clone()),
                Value::Uint(self.size),
                Value::Text(self.uploaded_by.to_string()),
                Value::Uint(self.created_at_ms),
                Value::Uint(self.scan_status.tag()),
                Value::Text(self.retention_class.clone()),
            ]),
        ])
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = ArrayFields::new(value, "attachment")?;
        outer.expect_schema(ATTACHMENT_SCHEMA)?;
        let mut fields = ArrayFields::new(outer.next("attachment fields")?, "attachment fields")?;
        outer.end("attachment")?;
        let attachment_id = fields.text("attachment_id")?;
        let digest = Digest::parse(&fields.text("digest")?)?;
        let name = fields.text("name")?;
        let media_type = fields.text("media_type")?;
        let size = fields.uint("size")?;
        let uploaded_by = WorkspaceId::parse(&fields.text("uploaded_by")?)?;
        let created_at_ms = fields.uint("created_at_ms")?;
        let scan_status = ScanStatus::from_tag(fields.uint("scan_status")?)?;
        let retention_class = fields.text("retention_class")?;
        fields.end("attachment fields")?;
        validate_text("attachment_id", &attachment_id)?;
        validate_text("name", &name)?;
        validate_text("media_type", &media_type)?;
        validate_text("retention_class", &retention_class)?;
        Ok(Self {
            attachment_id,
            digest,
            name,
            media_type,
            size,
            uploaded_by,
            created_at_ms,
            scan_status,
            retention_class,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum WatchTarget {
    Entity(String),
    Scope(String),
    Label(String),
    Query(String),
}

impl WatchTarget {
    fn to_value(&self) -> Value {
        let (tag, value) = match self {
            WatchTarget::Entity(value) => (0, value),
            WatchTarget::Scope(value) => (1, value),
            WatchTarget::Label(value) => (2, value),
            WatchTarget::Query(value) => (3, value),
        };
        Value::Array(vec![Value::Uint(tag), Value::Text(value.clone())])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut fields = ArrayFields::new(value, "watch target")?;
        let tag = fields.uint("target tag")?;
        let value = fields.text("target value")?;
        fields.end("watch target")?;
        validate_text("watch target", &value)?;
        match tag {
            0 => Ok(WatchTarget::Entity(value)),
            1 => Ok(WatchTarget::Scope(value)),
            2 => Ok(WatchTarget::Label(value)),
            3 => Ok(WatchTarget::Query(value)),
            other => Err(LoomError::corrupt(format!(
                "unknown watch target tag {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchEntry {
    pub principal: WorkspaceId,
    pub target: WatchTarget,
    pub created_at_ms: u64,
}

impl WatchEntry {
    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(self.principal.to_string()),
            self.target.to_value(),
            Value::Uint(self.created_at_ms),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut fields = ArrayFields::new(value, "watch entry")?;
        let principal = WorkspaceId::parse(&fields.text("principal")?)?;
        let target = WatchTarget::from_value(fields.next("target")?)?;
        let created_at_ms = fields.uint("created_at_ms")?;
        fields.end("watch entry")?;
        Ok(Self {
            principal,
            target,
            created_at_ms,
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WatchRegistry {
    entries: Vec<WatchEntry>,
}

impl WatchRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, principal: WorkspaceId, target: WatchTarget, created_at_ms: u64) {
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.principal == principal && entry.target == target)
        {
            entry.created_at_ms = created_at_ms;
            return;
        }
        self.entries.push(WatchEntry {
            principal,
            target,
            created_at_ms,
        });
        self.entries.sort_by(|left, right| {
            left.principal
                .to_string()
                .cmp(&right.principal.to_string())
                .then_with(|| left.target.cmp(&right.target))
        });
    }

    pub fn remove(&mut self, principal: WorkspaceId, target: &WatchTarget) -> bool {
        let before = self.entries.len();
        self.entries
            .retain(|entry| !(entry.principal == principal && &entry.target == target));
        self.entries.len() != before
    }

    pub fn entries(&self) -> &[WatchEntry] {
        &self.entries
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(WATCH_SCHEMA.to_string()),
            Value::Array(self.entries.iter().map(WatchEntry::to_value).collect()),
        ])
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = ArrayFields::new(value, "watch registry")?;
        outer.expect_schema(WATCH_SCHEMA)?;
        let entries = array_items(outer.next("watch entries")?, "watch entries")?
            .into_iter()
            .map(WatchEntry::from_value)
            .collect::<Result<Vec<_>>>()?;
        outer.end("watch registry")?;
        let mut registry = WatchRegistry::new();
        for entry in entries {
            registry.add(entry.principal, entry.target, entry.created_at_ms);
        }
        Ok(registry)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LabelSet {
    labels: BTreeSet<String>,
}

impl LabelSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, label: impl Into<String>) -> Result<bool> {
        let label = label.into();
        validate_text("label", &label)?;
        Ok(self.labels.insert(label))
    }

    pub fn remove(&mut self, label: &str) -> bool {
        self.labels.remove(label)
    }

    pub fn labels(&self) -> Vec<String> {
        self.labels.iter().cloned().collect()
    }

    pub fn to_value(&self) -> Value {
        let labels = self.labels();
        string_array(&labels)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScopeMode {
    ReadWrite,
    ReadOnly {
        reason: String,
    },
    Mirror {
        source: String,
        last_synced_root: Option<Digest>,
    },
}

impl ScopeMode {
    pub fn to_value(&self) -> Value {
        match self {
            ScopeMode::ReadWrite => Value::Array(vec![Value::Uint(0)]),
            ScopeMode::ReadOnly { reason } => {
                Value::Array(vec![Value::Uint(1), Value::Text(reason.clone())])
            }
            ScopeMode::Mirror {
                source,
                last_synced_root,
            } => Value::Array(vec![
                Value::Uint(2),
                Value::Text(source.clone()),
                optional_digest(*last_synced_root),
            ]),
        }
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut fields = ArrayFields::new(value, "scope mode")?;
        let tag = fields.uint("scope mode tag")?;
        let mode = match tag {
            0 => ScopeMode::ReadWrite,
            1 => {
                let reason = fields.text("read_only reason")?;
                validate_text("read_only reason", &reason)?;
                ScopeMode::ReadOnly { reason }
            }
            2 => {
                let source = fields.text("mirror source")?;
                validate_text("mirror source", &source)?;
                ScopeMode::Mirror {
                    source,
                    last_synced_root: fields.optional_digest("last_synced_root")?,
                }
            }
            other => {
                return Err(LoomError::corrupt(format!(
                    "unknown scope mode tag {other}"
                )));
            }
        };
        fields.end("scope mode")?;
        Ok(mode)
    }

    pub fn allows_write(&self) -> bool {
        matches!(self, ScopeMode::ReadWrite)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeModeRecord {
    pub scope_id: String,
    pub mode: ScopeMode,
    pub changed_by: WorkspaceId,
    pub changed_at_ms: u64,
}

impl ScopeModeRecord {
    pub fn new(
        scope_id: impl Into<String>,
        mode: ScopeMode,
        changed_by: WorkspaceId,
        changed_at_ms: u64,
    ) -> Result<Self> {
        let scope_id = scope_id.into();
        validate_text("scope_id", &scope_id)?;
        Ok(Self {
            scope_id,
            mode,
            changed_by,
            changed_at_ms,
        })
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(SCOPE_MODE_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.scope_id.clone()),
                self.mode.to_value(),
                Value::Text(self.changed_by.to_string()),
                Value::Uint(self.changed_at_ms),
            ]),
        ])
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = ArrayFields::new(value, "scope mode record")?;
        outer.expect_schema(SCOPE_MODE_SCHEMA)?;
        let mut fields = ArrayFields::new(
            outer.next("scope mode record fields")?,
            "scope mode record fields",
        )?;
        outer.end("scope mode record")?;
        let scope_id = fields.text("scope_id")?;
        validate_text("scope_id", &scope_id)?;
        let mode = ScopeMode::from_value(fields.next("mode")?)?;
        let changed_by = WorkspaceId::parse(&fields.text("changed_by")?)?;
        let changed_at_ms = fields.uint("changed_at_ms")?;
        fields.end("scope mode record fields")?;
        Ok(Self {
            scope_id,
            mode,
            changed_by,
            changed_at_ms,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SavedQuery {
    pub query_id: String,
    pub name: String,
    pub predicate: Predicate,
    pub created_by: WorkspaceId,
    pub created_at_ms: u64,
    pub labels: Vec<String>,
}

impl SavedQuery {
    pub fn new(
        query_id: impl Into<String>,
        name: impl Into<String>,
        predicate: Predicate,
        created_by: WorkspaceId,
        created_at_ms: u64,
        labels: Vec<String>,
    ) -> Result<Self> {
        let query_id = query_id.into();
        let name = name.into();
        validate_text("query_id", &query_id)?;
        validate_text("name", &name)?;
        let labels = canonical_labels(labels)?;
        Ok(Self {
            query_id,
            name,
            predicate,
            created_by,
            created_at_ms,
            labels,
        })
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(SAVED_QUERY_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.query_id.clone()),
                Value::Text(self.name.clone()),
                self.predicate.to_value(),
                Value::Text(self.created_by.to_string()),
                Value::Uint(self.created_at_ms),
                string_array(&self.labels),
            ]),
        ])
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MentionTarget {
    Principal(WorkspaceId),
    Entity(String),
}

impl MentionTarget {
    fn to_value(&self) -> Value {
        match self {
            MentionTarget::Principal(id) => {
                Value::Array(vec![Value::Uint(0), Value::Text(id.to_string())])
            }
            MentionTarget::Entity(entity_id) => {
                Value::Array(vec![Value::Uint(1), Value::Text(entity_id.clone())])
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mention {
    pub source_entity_id: String,
    pub target: MentionTarget,
    pub byte_start: u64,
    pub byte_end: u64,
}

impl Mention {
    pub fn new(
        source_entity_id: impl Into<String>,
        target: MentionTarget,
        byte_start: u64,
        byte_end: u64,
    ) -> Result<Self> {
        let source_entity_id = source_entity_id.into();
        validate_text("source_entity_id", &source_entity_id)?;
        if byte_start > byte_end {
            return Err(LoomError::invalid("mention byte range is inverted"));
        }
        Ok(Self {
            source_entity_id,
            target,
            byte_start,
            byte_end,
        })
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(MENTION_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.source_entity_id.clone()),
                self.target.to_value(),
                Value::Uint(self.byte_start),
                Value::Uint(self.byte_end),
            ]),
        ])
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldType {
    String,
    Integer,
    Number,
    Boolean,
    Date,
    DateTime,
    DateRange,
    Duration,
    Principal,
    EntityRef { kind: Option<String> },
    Enum { option_set: String },
    Url,
    List(Box<FieldType>),
    OpaqueJson,
}

impl FieldType {
    pub fn string() -> Self {
        Self::String
    }

    pub fn text() -> Self {
        Self::String
    }

    pub fn integer() -> Self {
        Self::Integer
    }

    pub fn number() -> Self {
        Self::Number
    }

    pub fn boolean() -> Self {
        Self::Boolean
    }

    pub fn date() -> Self {
        Self::Date
    }

    pub fn datetime() -> Self {
        Self::DateTime
    }

    pub fn date_range() -> Self {
        Self::DateRange
    }

    pub fn duration() -> Self {
        Self::Duration
    }

    pub fn principal() -> Self {
        Self::Principal
    }

    pub fn entity_ref(kind: Option<String>) -> Result<Self> {
        if let Some(kind) = &kind {
            validate_text("entity_ref kind", kind)?;
        }
        Ok(Self::EntityRef { kind })
    }

    pub fn enum_options(option_set: impl Into<String>) -> Result<Self> {
        let option_set = option_set.into();
        validate_text("option_set", &option_set)?;
        Ok(Self::Enum { option_set })
    }

    pub fn url() -> Self {
        Self::Url
    }

    pub fn list(inner: FieldType) -> Self {
        Self::List(Box::new(inner))
    }

    pub fn opaque_json() -> Self {
        Self::OpaqueJson
    }
}

impl FieldType {
    pub fn to_value(&self) -> Value {
        match self {
            FieldType::String => Value::Array(vec![Value::Uint(0)]),
            FieldType::Integer => Value::Array(vec![Value::Uint(2)]),
            FieldType::Number => Value::Array(vec![Value::Uint(3)]),
            FieldType::Boolean => Value::Array(vec![Value::Uint(4)]),
            FieldType::Date => Value::Array(vec![Value::Uint(5)]),
            FieldType::DateTime => Value::Array(vec![Value::Uint(6)]),
            FieldType::DateRange => Value::Array(vec![Value::Uint(7)]),
            FieldType::Duration => Value::Array(vec![Value::Uint(8)]),
            FieldType::Principal => Value::Array(vec![Value::Uint(9)]),
            FieldType::EntityRef { kind } => {
                Value::Array(vec![Value::Uint(10), optional_text(kind)])
            }
            FieldType::Enum { option_set } => {
                Value::Array(vec![Value::Uint(11), Value::Text(option_set.clone())])
            }
            FieldType::Url => Value::Array(vec![Value::Uint(12)]),
            FieldType::List(inner) => Value::Array(vec![Value::Uint(13), inner.to_value()]),
            FieldType::OpaqueJson => Value::Array(vec![Value::Uint(14)]),
        }
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut fields = ArrayFields::new(value, "field type")?;
        let tag = fields.uint("field type tag")?;
        let field_type = match tag {
            0 => FieldType::String,
            1 => FieldType::String,
            2 => FieldType::Integer,
            3 => FieldType::Number,
            4 => FieldType::Boolean,
            5 => FieldType::Date,
            6 => FieldType::DateTime,
            7 => FieldType::DateRange,
            8 => FieldType::Duration,
            9 => FieldType::Principal,
            10 => FieldType::EntityRef {
                kind: fields.optional_text("entity_ref kind")?,
            },
            11 => {
                let option_set = fields.text("option_set")?;
                validate_text("option_set", &option_set)?;
                FieldType::Enum { option_set }
            }
            12 => FieldType::Url,
            13 => FieldType::List(Box::new(FieldType::from_value(
                fields.next("list item type")?,
            )?)),
            14 => FieldType::OpaqueJson,
            other => {
                return Err(LoomError::corrupt(format!(
                    "unknown field type tag {other}"
                )));
            }
        };
        fields.end("field type")?;
        Ok(field_type)
    }

    pub fn validate_value(&self, value: &FieldValue) -> Result<()> {
        value.validate()?;
        match (self, value) {
            (_, FieldValue::Null) => Ok(()),
            (FieldType::String, FieldValue::String(_))
            | (FieldType::Integer, FieldValue::Integer(_))
            | (FieldType::Number, FieldValue::Number(_))
            | (FieldType::Boolean, FieldValue::Boolean(_))
            | (FieldType::Date, FieldValue::Date(_))
            | (FieldType::DateTime, FieldValue::DateTime(_))
            | (FieldType::DateRange, FieldValue::DateRange { .. })
            | (FieldType::Duration, FieldValue::DurationMillis(_))
            | (FieldType::Principal, FieldValue::Principal(_))
            | (FieldType::Url, FieldValue::Url(_))
            | (FieldType::OpaqueJson, FieldValue::OpaqueJson(_)) => Ok(()),
            (FieldType::Number, FieldValue::Integer(_)) => Ok(()),
            (
                FieldType::EntityRef { kind },
                FieldValue::EntityRef {
                    kind: value_kind, ..
                },
            ) => {
                if kind.as_deref().is_none_or(|kind| kind == value_kind) {
                    Ok(())
                } else {
                    Err(LoomError::invalid("field value entity kind mismatch"))
                }
            }
            (FieldType::Enum { .. }, FieldValue::EnumOption(_)) => Ok(()),
            (FieldType::List(inner), FieldValue::List(values)) => {
                for value in values {
                    inner.validate_value(value)?;
                }
                Ok(())
            }
            _ => Err(LoomError::invalid("field value does not match field type")),
        }
    }

    pub fn can_widen_to(&self, target: &FieldType) -> bool {
        match (self, target) {
            (left, right) if left == right => true,
            (FieldType::Integer, FieldType::Number) => true,
            (FieldType::Enum { option_set: left }, FieldType::List(inner)) => {
                matches!(&**inner, FieldType::Enum { option_set: right } if left == right)
            }
            (FieldType::Principal, FieldType::List(inner)) => {
                matches!(&**inner, FieldType::Principal)
            }
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum FieldValue {
    String(String),
    Integer(i64),
    Number(f64),
    Boolean(bool),
    Date(DateValue),
    DateTime(DateTimeValue),
    DateRange {
        start: DateValue,
        end: Option<DateValue>,
    },
    DurationMillis(i64),
    Principal(String),
    EntityRef {
        kind: String,
        id: String,
    },
    EnumOption(String),
    Url(String),
    List(Vec<FieldValue>),
    OpaqueJson(String),
    Null,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct DateValue {
    days_since_epoch: i32,
}

impl DateValue {
    pub fn from_days_since_epoch(days_since_epoch: i32) -> Self {
        Self { days_since_epoch }
    }

    pub fn parse(value: &str) -> Result<Self> {
        let date = NaiveDate::parse_from_str(value, "%Y-%m-%d")
            .map_err(|_| LoomError::invalid("date must be YYYY-MM-DD"))?;
        let epoch = epoch_date()?;
        let days = date
            .signed_duration_since(epoch)
            .num_days()
            .try_into()
            .map_err(|_| LoomError::invalid("date is outside supported range"))?;
        Ok(Self {
            days_since_epoch: days,
        })
    }

    pub fn days_since_epoch(self) -> i32 {
        self.days_since_epoch
    }

    pub fn to_iso8601(self) -> Result<String> {
        let epoch = epoch_date()?;
        let date = epoch
            .checked_add_signed(chrono::Duration::days(i64::from(self.days_since_epoch)))
            .ok_or_else(|| LoomError::invalid("date is outside supported range"))?;
        Ok(format!(
            "{:04}-{:02}-{:02}",
            date.year(),
            date.month(),
            date.day()
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct DateTimeValue {
    unix_millis_utc: i64,
}

impl DateTimeValue {
    pub fn from_unix_millis_utc(unix_millis_utc: i64) -> Self {
        Self { unix_millis_utc }
    }

    pub fn parse(value: &str) -> Result<Self> {
        let datetime = DateTime::parse_from_rfc3339(value)
            .map_err(|_| LoomError::invalid("datetime must be RFC3339"))?;
        if datetime.offset().local_minus_utc() != 0 {
            return Err(LoomError::invalid("datetime must use UTC offset"));
        }
        Ok(Self {
            unix_millis_utc: datetime.timestamp_millis(),
        })
    }

    pub fn unix_millis_utc(self) -> i64 {
        self.unix_millis_utc
    }

    pub fn to_iso8601(self) -> Result<String> {
        let datetime = Utc
            .timestamp_millis_opt(self.unix_millis_utc)
            .single()
            .ok_or_else(|| LoomError::invalid("datetime is outside supported range"))?;
        Ok(datetime.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
    }
}

impl FieldValue {
    pub fn to_value(&self) -> Value {
        match self {
            Self::String(value) => tagged(0, vec![Value::Text(value.clone())]),
            Self::Integer(value) => tagged(2, vec![Value::int(*value)]),
            Self::Number(value) => tagged(3, vec![Value::Float(*value)]),
            Self::Boolean(value) => tagged(4, vec![Value::Bool(*value)]),
            Self::Date(value) => tagged(5, vec![Value::int(i64::from(value.days_since_epoch()))]),
            Self::DateTime(value) => tagged(6, vec![Value::int(value.unix_millis_utc())]),
            Self::DateRange { start, end } => tagged(
                7,
                vec![
                    Value::int(i64::from(start.days_since_epoch())),
                    optional_i32_value(end.map(DateValue::days_since_epoch)),
                ],
            ),
            Self::DurationMillis(value) => tagged(8, vec![Value::int(*value)]),
            Self::Principal(value) => tagged(9, vec![Value::Text(value.clone())]),
            Self::EntityRef { kind, id } => {
                tagged(10, vec![Value::Text(kind.clone()), Value::Text(id.clone())])
            }
            Self::EnumOption(value) => tagged(11, vec![Value::Text(value.clone())]),
            Self::Url(value) => tagged(12, vec![Value::Text(value.clone())]),
            Self::List(values) => tagged(
                13,
                vec![Value::Array(
                    values.iter().map(FieldValue::to_value).collect(),
                )],
            ),
            Self::OpaqueJson(value) => tagged(14, vec![Value::Text(value.clone())]),
            Self::Null => tagged(15, Vec::new()),
        }
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut fields = ArrayFields::new(value, "field value")?;
        let tag = fields.uint("field value tag")?;
        let result = match tag {
            0 => Self::String(fields.text("string")?),
            1 => Self::String(fields.text("text")?),
            2 => Self::Integer(fields.i64("integer")?),
            3 => Self::Number(fields.float("number")?),
            4 => Self::Boolean(fields.bool("boolean")?),
            5 => Self::Date(read_date_value(&mut fields, "date")?),
            6 => Self::DateTime(read_datetime_value(&mut fields, "datetime")?),
            7 => Self::DateRange {
                start: read_date_value(&mut fields, "date_range start")?,
                end: read_optional_date_value(&mut fields, "date_range end")?,
            },
            8 => Self::DurationMillis(fields.i64("duration")?),
            9 => Self::Principal(fields.text("principal")?),
            10 => Self::EntityRef {
                kind: fields.text("entity kind")?,
                id: fields.text("entity id")?,
            },
            11 => Self::EnumOption(fields.text("enum option")?),
            12 => Self::Url(fields.text("url")?),
            13 => Self::List(read_field_value_list(fields.next("field value list")?)?),
            14 => Self::OpaqueJson(fields.text("opaque_json")?),
            15 => Self::Null,
            other => {
                return Err(LoomError::corrupt(format!(
                    "unknown field value tag {other}"
                )));
            }
        };
        fields.end("field value")?;
        result.validate()?;
        Ok(result)
    }

    pub fn validate(&self) -> Result<()> {
        match self {
            Self::String(value) | Self::OpaqueJson(value) => {
                validate_field_string("field value", value)
            }
            Self::Principal(value) | Self::EnumOption(value) | Self::Url(value) => {
                validate_text("field value", value)
            }
            Self::Date(_) | Self::DateTime(_) | Self::DateRange { .. } => Ok(()),
            Self::EntityRef { kind, id } => {
                validate_text("field entity ref kind", kind)?;
                validate_text("field entity ref id", id)
            }
            Self::List(values) => {
                for value in values {
                    value.validate()?;
                }
                Ok(())
            }
            Self::Integer(_)
            | Self::Number(_)
            | Self::Boolean(_)
            | Self::DurationMillis(_)
            | Self::Null => Ok(()),
        }
    }

    pub fn from_json(value: &JsonValue) -> Result<Self> {
        match value {
            JsonValue::Null => Ok(FieldValue::Null),
            JsonValue::Bool(value) => Ok(FieldValue::Boolean(*value)),
            JsonValue::Number(value) => {
                if let Some(value) = value.as_i64() {
                    Ok(FieldValue::Integer(value))
                } else if let Some(value) = value.as_u64() {
                    if let Ok(value) = i64::try_from(value) {
                        Ok(FieldValue::Integer(value))
                    } else {
                        Ok(FieldValue::OpaqueJson(value.to_string()))
                    }
                } else if let Some(value) = value.as_f64() {
                    Ok(FieldValue::Number(value))
                } else {
                    Err(LoomError::invalid("field number is not representable"))
                }
            }
            JsonValue::String(value) => Ok(FieldValue::String(value.clone())),
            JsonValue::Array(values) => Ok(FieldValue::List(
                values
                    .iter()
                    .map(FieldValue::from_json)
                    .collect::<Result<Vec<_>>>()?,
            )),
            JsonValue::Object(_) => serde_json::to_string(value)
                .map(FieldValue::OpaqueJson)
                .map_err(|e| LoomError::invalid(format!("field JSON encoding failed: {e}"))),
        }
    }

    pub fn to_json(&self) -> JsonValue {
        match self {
            FieldValue::String(value)
            | FieldValue::Principal(value)
            | FieldValue::EnumOption(value)
            | FieldValue::Url(value)
            | FieldValue::OpaqueJson(value) => JsonValue::String(value.clone()),
            FieldValue::Date(value) => JsonValue::String(value.to_iso8601().unwrap_or_default()),
            FieldValue::DateTime(value) => {
                JsonValue::String(value.to_iso8601().unwrap_or_default())
            }
            FieldValue::Integer(value) | FieldValue::DurationMillis(value) => {
                serde_json::json!(value)
            }
            FieldValue::Number(value) => serde_json::json!(value),
            FieldValue::Boolean(value) => serde_json::json!(value),
            FieldValue::DateRange { start, end } => {
                serde_json::json!({
                    "start": start.to_iso8601().unwrap_or_default(),
                    "end": end.map(|value| value.to_iso8601().unwrap_or_default())
                })
            }
            FieldValue::EntityRef { kind, id } => serde_json::json!({ "kind": kind, "id": id }),
            FieldValue::List(values) => {
                JsonValue::Array(values.iter().map(FieldValue::to_json).collect())
            }
            FieldValue::Null => JsonValue::Null,
        }
    }
}

fn validate_field_string(name: &str, value: &str) -> Result<()> {
    if value.is_empty() {
        return Err(LoomError::invalid(format!("{name} must not be empty")));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldDefinition {
    pub field_id: String,
    pub key: String,
    pub name: String,
    pub description: Option<String>,
    pub field_type: FieldType,
    pub contexts: Vec<String>,
    pub required: bool,
    pub imported_computed: bool,
}

impl FieldDefinition {
    pub fn new(
        field_id: impl Into<String>,
        key: impl Into<String>,
        name: impl Into<String>,
        field_type: FieldType,
        contexts: Vec<String>,
        required: bool,
    ) -> Result<Self> {
        let field_id = field_id.into();
        let key = key.into();
        let name = name.into();
        validate_text("field_id", &field_id)?;
        validate_field_key("field key", &key)?;
        validate_text("name", &name)?;
        let contexts = canonical_labels(contexts)?;
        Ok(Self {
            field_id,
            key,
            name,
            description: None,
            field_type,
            contexts,
            required,
            imported_computed: false,
        })
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Result<Self> {
        let description = description.into();
        validate_text("field description", &description)?;
        self.description = Some(description);
        Ok(self)
    }

    pub fn imported_computed(mut self, imported_computed: bool) -> Self {
        self.imported_computed = imported_computed;
        self
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(FIELD_DEFINITION_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.field_id.clone()),
                Value::Text(self.key.clone()),
                Value::Text(self.name.clone()),
                optional_text(&self.description),
                self.field_type.to_value(),
                string_array(&self.contexts),
                Value::Bool(self.required),
                Value::Bool(self.imported_computed),
            ]),
        ])
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = ArrayFields::new(value, "field definition")?;
        outer.expect_schema(FIELD_DEFINITION_SCHEMA)?;
        let mut fields = ArrayFields::new(
            outer.next("field definition fields")?,
            "field definition fields",
        )?;
        outer.end("field definition")?;
        let field_id = fields.text("field_id")?;
        validate_text("field_id", &field_id)?;
        let key = fields.text("key")?;
        validate_field_key("field key", &key)?;
        let name = fields.text("name")?;
        validate_text("name", &name)?;
        let description = fields.optional_text("description")?;
        if let Some(description) = &description {
            validate_text("field description", description)?;
        }
        let field_type = FieldType::from_value(fields.next("field_type")?)?;
        let contexts = canonical_labels(read_string_array(fields.next("contexts")?, "contexts")?)?;
        let required = fields.bool("required")?;
        let imported_computed = fields.bool("imported_computed")?;
        fields.end("field definition fields")?;
        Ok(Self {
            field_id,
            key,
            name,
            description,
            field_type,
            contexts,
            required,
            imported_computed,
        })
    }

    pub fn validate_value(&self, value: &FieldValue) -> Result<()> {
        self.field_type.validate_value(value)
    }
}

fn array_items(value: Value, name: &str) -> Result<Vec<Value>> {
    match value {
        Value::Array(values) => Ok(values),
        _ => Err(LoomError::corrupt(format!("{name} must be an array"))),
    }
}

fn optional_digest(value: Option<Digest>) -> Value {
    value
        .map(|digest| Value::Text(digest.to_string()))
        .unwrap_or(Value::Null)
}

struct ArrayFields {
    values: std::vec::IntoIter<Value>,
}

impl ArrayFields {
    fn new(value: Value, name: &str) -> Result<Self> {
        Ok(Self {
            values: array_items(value, name)?.into_iter(),
        })
    }

    fn next(&mut self, name: &str) -> Result<Value> {
        self.values
            .next()
            .ok_or_else(|| LoomError::corrupt(format!("{name} is missing")))
    }

    fn expect_schema(&mut self, schema: &str) -> Result<()> {
        match self.next("schema")? {
            Value::Text(value) if value == schema => Ok(()),
            _ => Err(LoomError::corrupt(format!("expected schema {schema}"))),
        }
    }

    fn text(&mut self, name: &str) -> Result<String> {
        match self.next(name)? {
            Value::Text(value) => Ok(value),
            _ => Err(LoomError::corrupt(format!("{name} must be text"))),
        }
    }

    fn uint(&mut self, name: &str) -> Result<u64> {
        match self.next(name)? {
            Value::Uint(value) => Ok(value),
            _ => Err(LoomError::corrupt(format!("{name} must be uint"))),
        }
    }

    fn i64(&mut self, name: &str) -> Result<i64> {
        value_to_i64(self.next(name)?, name)
    }

    fn float(&mut self, name: &str) -> Result<f64> {
        match self.next(name)? {
            Value::Float(value) => Ok(value),
            _ => Err(LoomError::corrupt(format!("{name} must be float"))),
        }
    }

    fn bool(&mut self, name: &str) -> Result<bool> {
        match self.next(name)? {
            Value::Bool(value) => Ok(value),
            _ => Err(LoomError::corrupt(format!("{name} must be bool"))),
        }
    }

    fn optional_text(&mut self, name: &str) -> Result<Option<String>> {
        match self.next(name)? {
            Value::Array(values) => {
                let mut fields = ArrayFields {
                    values: values.into_iter(),
                };
                let tag = fields.uint(name)?;
                let value = match tag {
                    0 => None,
                    1 => Some(fields.text(name)?),
                    other => {
                        return Err(LoomError::corrupt(format!(
                            "{name} has unknown optional tag {other}"
                        )));
                    }
                };
                fields.end(name)?;
                Ok(value)
            }
            _ => Err(LoomError::corrupt(format!("{name} must be optional text"))),
        }
    }

    fn optional_digest(&mut self, name: &str) -> Result<Option<Digest>> {
        match self.next(name)? {
            Value::Null => Ok(None),
            Value::Text(value) => Digest::parse(&value).map(Some),
            _ => Err(LoomError::corrupt(format!("{name} must be null or digest"))),
        }
    }

    fn end(&mut self, name: &str) -> Result<()> {
        if self.values.next().is_some() {
            return Err(LoomError::corrupt(format!("{name} has trailing fields")));
        }
        Ok(())
    }
}

fn tagged(tag: u64, values: Vec<Value>) -> Value {
    let mut fields = Vec::with_capacity(values.len() + 1);
    fields.push(Value::Uint(tag));
    fields.extend(values);
    Value::Array(fields)
}

fn optional_text(value: &Option<String>) -> Value {
    match value {
        Some(value) => Value::Array(vec![Value::Uint(1), Value::Text(value.clone())]),
        None => Value::Array(vec![Value::Uint(0)]),
    }
}

fn optional_i32_value(value: Option<i32>) -> Value {
    match value {
        Some(value) => Value::Array(vec![Value::Uint(1), Value::int(i64::from(value))]),
        None => Value::Array(vec![Value::Uint(0)]),
    }
}

fn read_date_value(fields: &mut ArrayFields, name: &str) -> Result<DateValue> {
    match fields.next(name)? {
        value @ (Value::Uint(_) | Value::Nint(_)) => i32::try_from(value_to_i64(value, name)?)
            .map(DateValue::from_days_since_epoch)
            .map_err(|_| LoomError::invalid("date is outside supported range")),
        Value::Text(value) => DateValue::parse(&value),
        _ => Err(LoomError::corrupt(format!("{name} must be date"))),
    }
}

fn read_datetime_value(fields: &mut ArrayFields, name: &str) -> Result<DateTimeValue> {
    match fields.next(name)? {
        value @ (Value::Uint(_) | Value::Nint(_)) => Ok(DateTimeValue::from_unix_millis_utc(
            value_to_i64(value, name)?,
        )),
        Value::Text(value) => DateTimeValue::parse(&value),
        _ => Err(LoomError::corrupt(format!("{name} must be datetime"))),
    }
}

fn read_optional_date_value(fields: &mut ArrayFields, name: &str) -> Result<Option<DateValue>> {
    let mut fields = ArrayFields::new(fields.next(name)?, name)?;
    let tag = fields.uint("optional tag")?;
    let value = match tag {
        0 => None,
        1 => Some(match fields.next(name)? {
            value @ (Value::Uint(_) | Value::Nint(_)) => i32::try_from(value_to_i64(value, name)?)
                .map(DateValue::from_days_since_epoch)
                .map_err(|_| LoomError::invalid("date is outside supported range"))?,
            Value::Text(value) => DateValue::parse(&value)?,
            _ => return Err(LoomError::corrupt(format!("{name} must be date"))),
        }),
        other => {
            return Err(LoomError::corrupt(format!(
                "{name} has unknown optional tag {other}"
            )));
        }
    };
    fields.end(name)?;
    Ok(value)
}

fn epoch_date() -> Result<NaiveDate> {
    NaiveDate::from_ymd_opt(1970, 1, 1).ok_or_else(|| LoomError::corrupt("epoch date is invalid"))
}

fn read_field_value_list(value: Value) -> Result<Vec<FieldValue>> {
    match value {
        Value::Array(values) => values.into_iter().map(FieldValue::from_value).collect(),
        _ => Err(LoomError::corrupt("field value list must be an array")),
    }
}

fn read_string_array(value: Value, name: &str) -> Result<Vec<String>> {
    match value {
        Value::Array(values) => values
            .into_iter()
            .map(|value| match value {
                Value::Text(value) => {
                    validate_text(name, &value)?;
                    Ok(value)
                }
                _ => Err(LoomError::corrupt(format!("{name} entry must be text"))),
            })
            .collect(),
        _ => Err(LoomError::corrupt(format!("{name} must be an array"))),
    }
}

fn value_to_i64(value: Value, name: &str) -> Result<i64> {
    match value {
        Value::Uint(value) => i64::try_from(value)
            .map_err(|_| LoomError::corrupt(format!("{name} exceeds i64 range"))),
        Value::Nint(value) => {
            let value = i64::try_from(value)
                .map_err(|_| LoomError::corrupt(format!("{name} exceeds i64 range")))?;
            Ok(-1 - value)
        }
        _ => Err(LoomError::corrupt(format!("{name} must be int"))),
    }
}

fn validate_field_key(name: &str, value: &str) -> Result<()> {
    validate_text(name, value)?;
    if value
        .chars()
        .any(|ch| !(ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_'))
    {
        return Err(LoomError::invalid(format!(
            "{name} must contain only a-z, 0-9, or underscore"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_types::Algo;

    fn principal(byte: u8) -> WorkspaceId {
        WorkspaceId::v4_from_bytes([byte; 16])
    }

    #[test]
    fn attachment_meta_round_trips_canonical_bytes() {
        let meta = AttachmentMeta {
            attachment_id: "att-1".to_string(),
            digest: Digest::hash(Algo::Blake3, b"attachment"),
            name: "design.txt".to_string(),
            media_type: "text/plain".to_string(),
            size: 10,
            uploaded_by: principal(1),
            created_at_ms: 11,
            scan_status: ScanStatus::Clean,
            retention_class: "standard".to_string(),
        };

        let bytes = meta.encode().unwrap();
        assert_eq!(AttachmentMeta::decode(&bytes).unwrap(), meta);
        assert_eq!(
            AttachmentMeta::decode(&bytes).unwrap().encode().unwrap(),
            bytes
        );
    }

    #[test]
    fn watch_registry_add_remove_is_idempotent_and_ordered() {
        let mut registry = WatchRegistry::new();
        registry.add(principal(2), WatchTarget::Label("p0".to_string()), 10);
        registry.add(principal(1), WatchTarget::Entity("ISSUE-1".to_string()), 11);
        registry.add(principal(2), WatchTarget::Label("p0".to_string()), 12);

        assert_eq!(registry.entries().len(), 2);
        assert_eq!(registry.entries()[0].principal, principal(1));
        assert_eq!(registry.entries()[1].created_at_ms, 12);
        assert!(registry.remove(principal(2), &WatchTarget::Label("p0".to_string())));
        assert!(!registry.remove(principal(2), &WatchTarget::Label("p0".to_string())));
        let bytes = registry.encode().unwrap();
        assert_eq!(WatchRegistry::decode(&bytes).unwrap(), registry);
    }

    #[test]
    fn labels_are_canonical_and_unique() {
        let mut labels = LabelSet::new();
        assert!(labels.add("b").unwrap());
        assert!(labels.add("a").unwrap());
        assert!(!labels.add("a").unwrap());
        assert_eq!(labels.labels(), vec!["a".to_string(), "b".to_string()]);
        assert!(labels.remove("a"));
        assert_eq!(labels.labels(), vec!["b".to_string()]);
    }

    #[test]
    fn scope_modes_round_trip_and_gate_writes() {
        let read_only = ScopeModeRecord::new(
            "PROJ",
            ScopeMode::ReadOnly {
                reason: "import".into(),
            },
            principal(7),
            1,
        )
        .unwrap();
        assert!(!read_only.mode.allows_write());
        assert_eq!(
            ScopeModeRecord::decode(&read_only.encode().unwrap()).unwrap(),
            read_only
        );
        let mirror = ScopeModeRecord::new(
            "PROJ",
            ScopeMode::Mirror {
                source: "jira://project/PROJ".to_string(),
                last_synced_root: Some(Digest::hash(Algo::Blake3, b"root")),
            },
            principal(7),
            2,
        )
        .unwrap();
        assert!(!mirror.mode.allows_write());
        assert_eq!(
            ScopeModeRecord::decode(&mirror.encode().unwrap()).unwrap(),
            mirror
        );
        let read_write =
            ScopeModeRecord::new("PROJ", ScopeMode::ReadWrite, principal(7), 3).unwrap();
        assert!(read_write.mode.allows_write());
    }

    #[test]
    fn saved_query_embeds_predicate_identity() {
        let predicate = Predicate::from_json_value(&serde_json::json!({
            "version": 1,
            "expr": {
                "op": "eq",
                "path": ["status"],
                "value": { "type": "text", "value": "open" }
            }
        }))
        .unwrap();
        let query = SavedQuery::new(
            "q-1",
            "Open",
            predicate,
            principal(3),
            44,
            vec!["team".to_string(), "team".to_string()],
        )
        .unwrap();
        assert_eq!(query.labels, vec!["team".to_string()]);
        assert!(!query.encode().unwrap().is_empty());
    }

    #[test]
    fn mention_and_field_definition_validate_boundaries() {
        assert!(Mention::new("ISSUE-1", MentionTarget::Principal(principal(4)), 4, 2).is_err());
        let mention =
            Mention::new("ISSUE-1", MentionTarget::Entity("PAGE-1".to_string()), 2, 4).unwrap();
        assert!(!mention.encode().unwrap().is_empty());
        let field = FieldDefinition::new(
            "field-priority",
            "priority",
            "Priority",
            FieldType::list(FieldType::enum_options("priority-options").unwrap()),
            vec!["project-b".to_string(), "project-a".to_string()],
            true,
        )
        .unwrap()
        .with_description("Priority option")
        .unwrap()
        .imported_computed(true);
        assert_eq!(
            FieldDefinition::decode(&field.encode().unwrap()).unwrap(),
            field
        );
        assert_eq!(
            field.contexts,
            vec!["project-a".to_string(), "project-b".to_string()]
        );
        field
            .validate_value(&FieldValue::List(vec![FieldValue::EnumOption(
                "high".to_string(),
            )]))
            .unwrap();
        assert!(
            field
                .validate_value(&FieldValue::String("high".to_string()))
                .is_err()
        );
        assert!(FieldType::integer().can_widen_to(&FieldType::number()));
        assert_eq!(FieldType::text(), FieldType::String);
        assert_eq!(
            FieldType::from_value(Value::Array(vec![Value::Uint(1)])).unwrap(),
            FieldType::String
        );
        assert!(FieldType::principal().can_widen_to(&FieldType::list(FieldType::principal())));
    }

    #[test]
    fn field_values_preserve_ticket_canonical_tags_and_json_projection() {
        let value = FieldValue::List(vec![
            FieldValue::String("alpha".to_string()),
            FieldValue::Integer(3),
            FieldValue::EntityRef {
                kind: "page".to_string(),
                id: "page:plan".to_string(),
            },
        ]);
        let encoded = loom_codec::encode(&value.to_value()).unwrap();
        assert_eq!(
            FieldValue::from_value(loom_codec::decode(&encoded).unwrap()).unwrap(),
            value
        );
        assert_eq!(
            FieldValue::from_json(&serde_json::json!(["alpha", 3])).unwrap(),
            FieldValue::List(vec![
                FieldValue::String("alpha".to_string()),
                FieldValue::Integer(3),
            ])
        );
        assert_eq!(
            FieldValue::from_value(tagged(1, vec![Value::Text("legacy text".to_string())]))
                .unwrap(),
            FieldValue::String("legacy text".to_string())
        );
        assert_eq!(
            FieldValue::String("legacy text".to_string()).to_value(),
            tagged(0, vec![Value::Text("legacy text".to_string())])
        );
        assert!(matches!(
            FieldValue::from_json(&value.to_json()).unwrap(),
            FieldValue::List(_)
        ));
        let typed_ref = FieldType::entity_ref(Some("page".to_string())).unwrap();
        typed_ref
            .validate_value(&FieldValue::EntityRef {
                kind: "page".to_string(),
                id: "page:plan".to_string(),
            })
            .unwrap();
        assert!(
            typed_ref
                .validate_value(&FieldValue::EntityRef {
                    kind: "ticket".to_string(),
                    id: "ticket:LOOM-1".to_string(),
                })
                .is_err()
        );
    }

    #[test]
    fn field_date_and_datetime_values_are_native_and_project_iso8601() {
        let date = DateValue::parse("2026-07-15").unwrap();
        assert_eq!(date.to_iso8601().unwrap(), "2026-07-15");
        assert_eq!(
            FieldValue::from_value(FieldValue::Date(date).to_value()).unwrap(),
            FieldValue::Date(date)
        );
        assert_eq!(
            FieldValue::from_value(tagged(5, vec![Value::Text("2026-07-15".to_string())])).unwrap(),
            FieldValue::Date(date)
        );

        let datetime = DateTimeValue::parse("2026-07-15T12:34:56.789Z").unwrap();
        assert_eq!(datetime.to_iso8601().unwrap(), "2026-07-15T12:34:56.789Z");
        assert_eq!(
            FieldValue::from_value(FieldValue::DateTime(datetime).to_value()).unwrap(),
            FieldValue::DateTime(datetime)
        );
        assert!(DateTimeValue::parse("2026-07-15T12:34:56.789+01:00").is_err());
        assert_eq!(
            FieldValue::DateTime(datetime).to_json(),
            serde_json::json!("2026-07-15T12:34:56.789Z")
        );

        let range = FieldValue::DateRange {
            start: DateValue::parse("2026-07-15").unwrap(),
            end: Some(DateValue::parse("2026-07-16").unwrap()),
        };
        assert_eq!(FieldValue::from_value(range.to_value()).unwrap(), range);
    }
}
