use std::collections::{BTreeMap, BTreeSet};

use loom_codec::Value;
use loom_types::{Code, LoomError, Result, WorkspaceId};

pub const ANNOTATION_EVENT_SCHEMA: &str = "loom.substrate.annotation-event.v1";
pub const EMOJI_REGISTRY_SCHEMA: &str = "loom.substrate.emoji-registry.v1";
pub const EMOJI_REGISTRY_DIR: &str = ".loom/substrate/emoji";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EmojiRegistry {
    custom: BTreeSet<String>,
}

impl EmojiRegistry {
    pub fn new(custom: Vec<String>) -> Result<Self> {
        let mut registry = Self {
            custom: BTreeSet::new(),
        };
        for kind in custom {
            registry.register(kind)?;
        }
        Ok(registry)
    }

    pub fn register(&mut self, kind: impl Into<String>) -> Result<bool> {
        let kind = kind.into();
        validate_text("emoji kind", &kind)?;
        if is_implicit_unicode_emoji(&kind) {
            return Ok(false);
        }
        Ok(self.custom.insert(kind))
    }

    pub fn unregister(&mut self, kind: &str) -> bool {
        self.custom.remove(kind)
    }

    pub fn contains(&self, kind: &str) -> bool {
        is_implicit_unicode_emoji(kind) || self.custom.contains(kind)
    }

    pub fn custom(&self) -> impl Iterator<Item = &str> {
        self.custom.iter().map(String::as_str)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(EMOJI_REGISTRY_SCHEMA.to_string()),
            Value::Array(self.custom.iter().cloned().map(Value::Text).collect()),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "emoji registry")?;
        outer.expect_text(EMOJI_REGISTRY_SCHEMA)?;
        let custom = string_list(outer.next("custom emoji kinds")?, "custom emoji kinds")?;
        outer.end("emoji registry")?;
        Self::new(custom)
    }
}

pub fn emoji_registry_path(scope_id: &str) -> Result<String> {
    validate_text("emoji registry scope", scope_id)?;
    if scope_id == "." || scope_id == ".." || scope_id.contains('/') {
        return Err(LoomError::invalid("emoji registry scope is invalid"));
    }
    Ok(format!("{EMOJI_REGISTRY_DIR}/{scope_id}.ler"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnnotationAnchor {
    Entity {
        entity_id: String,
    },
    Thread {
        thread_id: String,
    },
    Range {
        entity_id: String,
        start: u64,
        end: u64,
        stale: bool,
    },
}

impl AnnotationAnchor {
    fn to_value(&self) -> Value {
        match self {
            AnnotationAnchor::Entity { entity_id } => {
                Value::Array(vec![Value::Uint(0), Value::Text(entity_id.clone())])
            }
            AnnotationAnchor::Thread { thread_id } => {
                Value::Array(vec![Value::Uint(1), Value::Text(thread_id.clone())])
            }
            AnnotationAnchor::Range {
                entity_id,
                start,
                end,
                stale,
            } => Value::Array(vec![
                Value::Uint(2),
                Value::Text(entity_id.clone()),
                Value::Uint(*start),
                Value::Uint(*end),
                Value::Bool(*stale),
            ]),
        }
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut fields = Fields::array(value, "annotation anchor")?;
        match fields.uint("anchor kind")? {
            0 => {
                let entity_id = fields.text("entity_id")?;
                fields.end("annotation anchor")?;
                validate_text("entity_id", &entity_id)?;
                Ok(AnnotationAnchor::Entity { entity_id })
            }
            1 => {
                let thread_id = fields.text("thread_id")?;
                fields.end("annotation anchor")?;
                validate_text("thread_id", &thread_id)?;
                Ok(AnnotationAnchor::Thread { thread_id })
            }
            2 => {
                let entity_id = fields.text("entity_id")?;
                let start = fields.uint("start")?;
                let end = fields.uint("end")?;
                let stale = fields.bool("stale")?;
                fields.end("annotation anchor")?;
                validate_text("entity_id", &entity_id)?;
                if start > end {
                    return Err(LoomError::invalid("range anchor start exceeds end"));
                }
                Ok(AnnotationAnchor::Range {
                    entity_id,
                    start,
                    end,
                    stale,
                })
            }
            other => Err(LoomError::corrupt(format!(
                "unknown annotation anchor tag {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnnotationAction {
    Add {
        anchor: AnnotationAnchor,
        body: String,
    },
    Edit {
        body: String,
    },
    Redact {
        reason: Option<String>,
    },
    Resolve,
    ReactionAdd {
        kind: String,
    },
    ReactionRemove {
        kind: String,
    },
    Pin,
    Unpin,
}

impl AnnotationAction {
    fn to_value(&self) -> Value {
        match self {
            AnnotationAction::Add { anchor, body } => Value::Array(vec![
                Value::Uint(0),
                anchor.to_value(),
                Value::Text(body.clone()),
            ]),
            AnnotationAction::Edit { body } => {
                Value::Array(vec![Value::Uint(1), Value::Text(body.clone())])
            }
            AnnotationAction::Redact { reason } => {
                Value::Array(vec![Value::Uint(2), optional_text_value(reason.as_deref())])
            }
            AnnotationAction::Resolve => Value::Array(vec![Value::Uint(3)]),
            AnnotationAction::ReactionAdd { kind } => {
                Value::Array(vec![Value::Uint(4), Value::Text(kind.clone())])
            }
            AnnotationAction::ReactionRemove { kind } => {
                Value::Array(vec![Value::Uint(5), Value::Text(kind.clone())])
            }
            AnnotationAction::Pin => Value::Array(vec![Value::Uint(6)]),
            AnnotationAction::Unpin => Value::Array(vec![Value::Uint(7)]),
        }
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut fields = Fields::array(value, "annotation action")?;
        match fields.uint("action kind")? {
            0 => {
                let anchor = AnnotationAnchor::from_value(fields.next("anchor")?)?;
                let body = fields.text("body")?;
                fields.end("annotation action")?;
                validate_text("body", &body)?;
                Ok(AnnotationAction::Add { anchor, body })
            }
            1 => {
                let body = fields.text("body")?;
                fields.end("annotation action")?;
                validate_text("body", &body)?;
                Ok(AnnotationAction::Edit { body })
            }
            2 => {
                let reason = fields.optional_text("reason")?;
                fields.end("annotation action")?;
                if let Some(reason) = reason.as_deref() {
                    validate_text("reason", reason)?;
                }
                Ok(AnnotationAction::Redact { reason })
            }
            3 => {
                fields.end("annotation action")?;
                Ok(AnnotationAction::Resolve)
            }
            4 => {
                let kind = fields.text("reaction kind")?;
                fields.end("annotation action")?;
                validate_text("reaction kind", &kind)?;
                Ok(AnnotationAction::ReactionAdd { kind })
            }
            5 => {
                let kind = fields.text("reaction kind")?;
                fields.end("annotation action")?;
                validate_text("reaction kind", &kind)?;
                Ok(AnnotationAction::ReactionRemove { kind })
            }
            6 => {
                fields.end("annotation action")?;
                Ok(AnnotationAction::Pin)
            }
            7 => {
                fields.end("annotation action")?;
                Ok(AnnotationAction::Unpin)
            }
            other => Err(LoomError::corrupt(format!(
                "unknown annotation action tag {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnnotationEvent {
    pub event_id: String,
    pub annotation_id: String,
    pub actor_principal: WorkspaceId,
    pub timestamp_ms: u64,
    pub action: AnnotationAction,
}

impl AnnotationEvent {
    pub fn new(
        event_id: impl Into<String>,
        annotation_id: impl Into<String>,
        actor_principal: WorkspaceId,
        timestamp_ms: u64,
        action: AnnotationAction,
    ) -> Result<Self> {
        let event_id = event_id.into();
        let annotation_id = annotation_id.into();
        validate_text("event_id", &event_id)?;
        validate_text("annotation_id", &annotation_id)?;
        validate_action(&action)?;
        Ok(Self {
            event_id,
            annotation_id,
            actor_principal,
            timestamp_ms,
            action,
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
            Value::Text(ANNOTATION_EVENT_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.event_id.clone()),
                Value::Text(self.annotation_id.clone()),
                Value::Text(self.actor_principal.to_string()),
                Value::Uint(self.timestamp_ms),
                self.action.to_value(),
            ]),
        ])
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "annotation event")?;
        outer.expect_text(ANNOTATION_EVENT_SCHEMA)?;
        let mut fields = Fields::array(outer.next("annotation event fields")?, "annotation event")?;
        outer.end("annotation event")?;
        let event_id = fields.text("event_id")?;
        let annotation_id = fields.text("annotation_id")?;
        let actor_principal = fields.id("actor_principal")?;
        let timestamp_ms = fields.uint("timestamp_ms")?;
        let action = AnnotationAction::from_value(fields.next("action")?)?;
        fields.end("annotation event")?;
        Self::new(
            event_id,
            annotation_id,
            actor_principal,
            timestamp_ms,
            action,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ReactionKey {
    pub kind: String,
    pub principal: WorkspaceId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redaction {
    pub actor_principal: WorkspaceId,
    pub timestamp_ms: u64,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnnotationRecord {
    pub annotation_id: String,
    pub anchor: AnnotationAnchor,
    pub author_principal: WorkspaceId,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub body: Option<String>,
    pub redaction: Option<Redaction>,
    pub resolved_at_ms: Option<u64>,
    pub reactions: BTreeSet<ReactionKey>,
    pub pinned_by: BTreeSet<WorkspaceId>,
    pub history_event_ids: Vec<String>,
}

#[derive(Debug, Default, Clone)]
pub struct AnnotationStore {
    records: BTreeMap<String, AnnotationRecord>,
}

impl AnnotationStore {
    pub fn new() -> Self {
        Self {
            records: BTreeMap::new(),
        }
    }

    pub fn get(&self, annotation_id: &str) -> Option<&AnnotationRecord> {
        self.records.get(annotation_id)
    }

    pub fn apply(&mut self, event: AnnotationEvent) -> Result<()> {
        match &event.action {
            AnnotationAction::Add { anchor, body } => {
                if self.records.contains_key(&event.annotation_id) {
                    return Err(LoomError::new(
                        Code::AlreadyExists,
                        "annotation already exists",
                    ));
                }
                self.records.insert(
                    event.annotation_id.clone(),
                    AnnotationRecord {
                        annotation_id: event.annotation_id.clone(),
                        anchor: anchor.clone(),
                        author_principal: event.actor_principal,
                        created_at_ms: event.timestamp_ms,
                        updated_at_ms: event.timestamp_ms,
                        body: Some(body.clone()),
                        redaction: None,
                        resolved_at_ms: None,
                        reactions: BTreeSet::new(),
                        pinned_by: BTreeSet::new(),
                        history_event_ids: vec![event.event_id],
                    },
                );
                Ok(())
            }
            AnnotationAction::Edit { body } => {
                let record = self.record_mut(&event.annotation_id)?;
                reject_if_redacted(record)?;
                record.body = Some(body.clone());
                record.updated_at_ms = event.timestamp_ms;
                record.history_event_ids.push(event.event_id);
                Ok(())
            }
            AnnotationAction::Redact { reason } => {
                let record = self.record_mut(&event.annotation_id)?;
                record.body = None;
                record.redaction = Some(Redaction {
                    actor_principal: event.actor_principal,
                    timestamp_ms: event.timestamp_ms,
                    reason: reason.clone(),
                });
                record.updated_at_ms = event.timestamp_ms;
                record.history_event_ids.push(event.event_id);
                Ok(())
            }
            AnnotationAction::Resolve => {
                let record = self.record_mut(&event.annotation_id)?;
                record.resolved_at_ms = Some(event.timestamp_ms);
                record.updated_at_ms = event.timestamp_ms;
                record.history_event_ids.push(event.event_id);
                Ok(())
            }
            AnnotationAction::ReactionAdd { kind } => {
                let record = self.record_mut(&event.annotation_id)?;
                record.reactions.insert(ReactionKey {
                    kind: kind.clone(),
                    principal: event.actor_principal,
                });
                record.updated_at_ms = event.timestamp_ms;
                record.history_event_ids.push(event.event_id);
                Ok(())
            }
            AnnotationAction::ReactionRemove { kind } => {
                let record = self.record_mut(&event.annotation_id)?;
                record.reactions.remove(&ReactionKey {
                    kind: kind.clone(),
                    principal: event.actor_principal,
                });
                record.updated_at_ms = event.timestamp_ms;
                record.history_event_ids.push(event.event_id);
                Ok(())
            }
            AnnotationAction::Pin => {
                let record = self.record_mut(&event.annotation_id)?;
                record.pinned_by.insert(event.actor_principal);
                record.updated_at_ms = event.timestamp_ms;
                record.history_event_ids.push(event.event_id);
                Ok(())
            }
            AnnotationAction::Unpin => {
                let record = self.record_mut(&event.annotation_id)?;
                record.pinned_by.remove(&event.actor_principal);
                record.updated_at_ms = event.timestamp_ms;
                record.history_event_ids.push(event.event_id);
                Ok(())
            }
        }
    }

    fn record_mut(&mut self, annotation_id: &str) -> Result<&mut AnnotationRecord> {
        self.records
            .get_mut(annotation_id)
            .ok_or_else(|| LoomError::not_found("annotation not found"))
    }
}

fn reject_if_redacted(record: &AnnotationRecord) -> Result<()> {
    if record.redaction.is_some() {
        Err(LoomError::new(
            Code::Conflict,
            "redacted annotation cannot be edited",
        ))
    } else {
        Ok(())
    }
}

fn validate_action(action: &AnnotationAction) -> Result<()> {
    match action {
        AnnotationAction::Add { anchor, body } => {
            validate_anchor(anchor)?;
            validate_text("body", body)
        }
        AnnotationAction::Edit { body } => validate_text("body", body),
        AnnotationAction::Redact { reason } => {
            if let Some(reason) = reason.as_deref() {
                validate_text("reason", reason)?;
            }
            Ok(())
        }
        AnnotationAction::ReactionAdd { kind } | AnnotationAction::ReactionRemove { kind } => {
            validate_text("reaction kind", kind)
        }
        AnnotationAction::Resolve | AnnotationAction::Pin | AnnotationAction::Unpin => Ok(()),
    }
}

fn validate_anchor(anchor: &AnnotationAnchor) -> Result<()> {
    match anchor {
        AnnotationAnchor::Entity { entity_id } => validate_text("entity_id", entity_id),
        AnnotationAnchor::Thread { thread_id } => validate_text("thread_id", thread_id),
        AnnotationAnchor::Range {
            entity_id,
            start,
            end,
            stale: _,
        } => {
            validate_text("entity_id", entity_id)?;
            if start > end {
                Err(LoomError::invalid("range anchor start exceeds end"))
            } else {
                Ok(())
            }
        }
    }
}

fn optional_text_value(value: Option<&str>) -> Value {
    value
        .map(|value| Value::Text(value.to_string()))
        .unwrap_or(Value::Null)
}

fn validate_text(name: &str, value: &str) -> Result<()> {
    if value.is_empty() {
        return Err(LoomError::invalid(format!("{name} must not be empty")));
    }
    if value.len() > 4096 {
        return Err(LoomError::invalid(format!("{name} is too long")));
    }
    Ok(())
}

fn string_list(value: Value, name: &str) -> Result<Vec<String>> {
    match value {
        Value::Array(items) => items
            .into_iter()
            .map(|item| match item {
                Value::Text(value) => {
                    validate_text(name, &value)?;
                    Ok(value)
                }
                _ => Err(LoomError::corrupt(format!("{name} item must be text"))),
            })
            .collect(),
        _ => Err(LoomError::corrupt(format!("{name} must be an array"))),
    }
}

fn is_implicit_unicode_emoji(kind: &str) -> bool {
    let mut has_emoji = false;
    for ch in kind.chars() {
        if is_emoji_scalar(ch) {
            has_emoji = true;
            continue;
        }
        if matches!(
            ch,
            '\u{200D}'
                | '\u{20E3}'
                | '\u{FE0E}'
                | '\u{FE0F}'
                | '\u{1F3FB}'..='\u{1F3FF}'
                | '\u{E0020}'..='\u{E007F}'
        ) {
            continue;
        }
        return false;
    }
    has_emoji
}

fn is_emoji_scalar(ch: char) -> bool {
    matches!(
        ch,
        '\u{00A9}'
            | '\u{00AE}'
            | '\u{203C}'
            | '\u{2049}'
            | '\u{2122}'
            | '\u{2139}'
            | '\u{2194}'..='\u{21AA}'
            | '\u{231A}'..='\u{231B}'
            | '\u{2328}'
            | '\u{23CF}'
            | '\u{23E9}'..='\u{23F3}'
            | '\u{23F8}'..='\u{23FA}'
            | '\u{24C2}'
            | '\u{25AA}'..='\u{25AB}'
            | '\u{25B6}'
            | '\u{25C0}'
            | '\u{25FB}'..='\u{25FE}'
            | '\u{2600}'..='\u{27BF}'
            | '\u{2934}'..='\u{2935}'
            | '\u{2B05}'..='\u{2B55}'
            | '\u{3030}'
            | '\u{303D}'
            | '\u{3297}'
            | '\u{3299}'
            | '\u{1F000}'..='\u{1FAFF}'
    )
}

fn codec_error(error: loom_codec::CodecError) -> LoomError {
    LoomError::corrupt(format!("annotation event cbor: {error}"))
}

struct Fields {
    items: Vec<Value>,
}

impl Fields {
    fn array(value: Value, name: &str) -> Result<Self> {
        match value {
            Value::Array(items) => Ok(Self { items }),
            _ => Err(LoomError::corrupt(format!("{name} must be an array"))),
        }
    }

    fn next(&mut self, name: &str) -> Result<Value> {
        if self.items.is_empty() {
            return Err(LoomError::corrupt(format!("missing {name}")));
        }
        Ok(self.items.remove(0))
    }

    fn end(&self, name: &str) -> Result<()> {
        if self.items.is_empty() {
            Ok(())
        } else {
            Err(LoomError::corrupt(format!("{name} has trailing fields")))
        }
    }

    fn expect_text(&mut self, expected: &str) -> Result<()> {
        let actual = self.text("schema")?;
        if actual == expected {
            Ok(())
        } else {
            Err(LoomError::corrupt(format!("expected schema {expected}")))
        }
    }

    fn text(&mut self, name: &str) -> Result<String> {
        match self.next(name)? {
            Value::Text(value) => Ok(value),
            _ => Err(LoomError::corrupt(format!("{name} must be text"))),
        }
    }

    fn optional_text(&mut self, name: &str) -> Result<Option<String>> {
        match self.next(name)? {
            Value::Null => Ok(None),
            Value::Text(value) => Ok(Some(value)),
            _ => Err(LoomError::corrupt(format!("{name} must be text or null"))),
        }
    }

    fn uint(&mut self, name: &str) -> Result<u64> {
        match self.next(name)? {
            Value::Uint(value) => Ok(value),
            _ => Err(LoomError::corrupt(format!(
                "{name} must be unsigned integer"
            ))),
        }
    }

    fn bool(&mut self, name: &str) -> Result<bool> {
        match self.next(name)? {
            Value::Bool(value) => Ok(value),
            _ => Err(LoomError::corrupt(format!("{name} must be bool"))),
        }
    }

    fn id(&mut self, name: &str) -> Result<WorkspaceId> {
        let value = self.text(name)?;
        WorkspaceId::parse(&value)
            .map_err(|_| LoomError::corrupt(format!("{name} must be a workspace id")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(byte: u8) -> WorkspaceId {
        WorkspaceId::from_bytes([byte; 16])
    }

    fn add_event() -> AnnotationEvent {
        AnnotationEvent::new(
            "e1",
            "a1",
            id(1),
            100,
            AnnotationAction::Add {
                anchor: AnnotationAnchor::Range {
                    entity_id: "page-1".to_string(),
                    start: 5,
                    end: 9,
                    stale: false,
                },
                body: "hello".to_string(),
            },
        )
        .unwrap()
    }

    #[test]
    fn annotation_event_round_trips_canonical_bytes() {
        let event = add_event();
        let encoded = event.encode().unwrap();
        let decoded = AnnotationEvent::decode(&encoded).unwrap();
        assert_eq!(decoded, event);
        assert_eq!(decoded.encode().unwrap(), encoded);
    }

    #[test]
    fn emoji_registry_round_trips_custom_kinds_and_implicit_unicode() {
        let registry = EmojiRegistry::new(vec![
            "approved".to_string(),
            "blocked".to_string(),
            "approved".to_string(),
            "\u{1F44D}".to_string(),
        ])
        .unwrap();
        assert_eq!(
            registry.custom().collect::<Vec<_>>(),
            vec!["approved", "blocked"]
        );
        assert!(registry.contains("approved"));
        assert!(registry.contains("\u{1F44D}"));
        assert!(registry.contains("\u{1F469}\u{200D}\u{1F4BB}"));
        assert!(!registry.contains("missing"));
        let encoded = registry.encode().unwrap();
        assert_eq!(EmojiRegistry::decode(&encoded).unwrap(), registry);
    }

    #[test]
    fn annotation_store_applies_edit_reaction_pin_and_resolve() {
        let mut store = AnnotationStore::new();
        store.apply(add_event()).unwrap();
        store
            .apply(
                AnnotationEvent::new(
                    "e2",
                    "a1",
                    id(1),
                    110,
                    AnnotationAction::Edit {
                        body: "updated".to_string(),
                    },
                )
                .unwrap(),
            )
            .unwrap();
        store
            .apply(
                AnnotationEvent::new(
                    "e3",
                    "a1",
                    id(2),
                    120,
                    AnnotationAction::ReactionAdd {
                        kind: "thumbs-up".to_string(),
                    },
                )
                .unwrap(),
            )
            .unwrap();
        store
            .apply(AnnotationEvent::new("e4", "a1", id(2), 130, AnnotationAction::Pin).unwrap())
            .unwrap();
        store
            .apply(AnnotationEvent::new("e5", "a1", id(1), 140, AnnotationAction::Resolve).unwrap())
            .unwrap();
        let record = store.get("a1").unwrap();
        assert_eq!(record.body.as_deref(), Some("updated"));
        assert_eq!(record.reactions.len(), 1);
        assert!(record.pinned_by.contains(&id(2)));
        assert_eq!(record.resolved_at_ms, Some(140));
        assert_eq!(record.history_event_ids, vec!["e1", "e2", "e3", "e4", "e5"]);
    }

    #[test]
    fn redaction_hides_body_and_preserves_history() {
        let mut store = AnnotationStore::new();
        store.apply(add_event()).unwrap();
        store
            .apply(
                AnnotationEvent::new(
                    "e2",
                    "a1",
                    id(2),
                    200,
                    AnnotationAction::Redact {
                        reason: Some("policy".to_string()),
                    },
                )
                .unwrap(),
            )
            .unwrap();
        let record = store.get("a1").unwrap();
        assert_eq!(record.body, None);
        assert_eq!(
            record
                .redaction
                .as_ref()
                .and_then(|redaction| redaction.reason.as_deref()),
            Some("policy")
        );
        assert_eq!(record.history_event_ids, vec!["e1", "e2"]);
    }

    #[test]
    fn redacted_annotation_cannot_be_edited() {
        let mut store = AnnotationStore::new();
        store.apply(add_event()).unwrap();
        store
            .apply(
                AnnotationEvent::new(
                    "e2",
                    "a1",
                    id(2),
                    200,
                    AnnotationAction::Redact { reason: None },
                )
                .unwrap(),
            )
            .unwrap();
        let err = store
            .apply(
                AnnotationEvent::new(
                    "e3",
                    "a1",
                    id(1),
                    210,
                    AnnotationAction::Edit {
                        body: "new".to_string(),
                    },
                )
                .unwrap(),
            )
            .unwrap_err();
        assert_eq!(err.code, Code::Conflict);
        assert_eq!(store.get("a1").unwrap().history_event_ids, vec!["e1", "e2"]);
    }

    #[test]
    fn reaction_remove_and_unpin_are_idempotent_state_changes() {
        let mut store = AnnotationStore::new();
        store.apply(add_event()).unwrap();
        store
            .apply(
                AnnotationEvent::new(
                    "e2",
                    "a1",
                    id(2),
                    120,
                    AnnotationAction::ReactionRemove {
                        kind: "thumbs-up".to_string(),
                    },
                )
                .unwrap(),
            )
            .unwrap();
        store
            .apply(AnnotationEvent::new("e3", "a1", id(2), 130, AnnotationAction::Unpin).unwrap())
            .unwrap();
        let record = store.get("a1").unwrap();
        assert!(record.reactions.is_empty());
        assert!(record.pinned_by.is_empty());
        assert_eq!(record.history_event_ids, vec!["e1", "e2", "e3"]);
    }
}
