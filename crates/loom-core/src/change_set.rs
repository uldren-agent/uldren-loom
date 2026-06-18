//! Shared change cursor and change-set primitives for facets with replayable history.

use crate::cbor::{self, Value};
use crate::digest::Digest;
use crate::error::{LoomError, Result};

const CHANGE_CURSOR_V1: &str = "loom.change_cursor.v1";
const CHANGE_SET_V1: &str = "loom.change_set.v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeCursor {
    pub scope: String,
    pub position: ChangeCursorPosition,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeCursorPosition {
    Sequence { next: u64 },
    Commit { anchor: Digest, index: u64 },
}

impl ChangeCursor {
    pub fn sequence(scope: impl Into<String>, next: u64) -> Self {
        Self {
            scope: scope.into(),
            position: ChangeCursorPosition::Sequence { next },
        }
    }

    pub fn commit(scope: impl Into<String>, anchor: Digest, index: u64) -> Self {
        Self {
            scope: scope.into(),
            position: ChangeCursorPosition::Commit { anchor, index },
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        cbor::encode(&Value::Array(vec![
            Value::Text(CHANGE_CURSOR_V1.to_string()),
            Value::Uint(1),
            Value::Text(self.scope.clone()),
            cursor_position_value(&self.position),
        ]))
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let mut fields = cbor::Fields::new(cbor::decode_array(bytes)?);
        let tag = fields.text()?;
        if tag != CHANGE_CURSOR_V1 {
            return Err(LoomError::corrupt("change cursor tag mismatch"));
        }
        if fields.uint()? != 1 {
            return Err(LoomError::unsupported("unsupported change cursor version"));
        }
        let scope = fields.text()?;
        let position = decode_cursor_position(fields.next_field()?)?;
        fields.end()?;
        Ok(Self { scope, position })
    }

    pub fn encode_text(&self) -> String {
        format!("loom-change-cursor-v1:{}", hex::encode(self.encode()))
    }

    pub fn decode_text(text: &str) -> Result<Self> {
        let hex = text
            .strip_prefix("loom-change-cursor-v1:")
            .ok_or_else(|| LoomError::cursor_invalid("invalid change cursor prefix"))?;
        let bytes =
            hex::decode(hex).map_err(|_| LoomError::cursor_invalid("invalid change cursor hex"))?;
        Self::decode(&bytes).map_err(|error| LoomError::cursor_invalid(error.message))
    }

    pub fn require_not_before_low_water(&self, retained_low_water_mark: u64) -> Result<()> {
        if let ChangeCursorPosition::Sequence { next } = self.position
            && next < retained_low_water_mark
        {
            return Err(LoomError::retained_gap(format!(
                "change cursor sequence {next} predates retained low-water mark {retained_low_water_mark}"
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeGapState {
    Retained,
    PlannedPrune,
    Gap,
}

impl ChangeGapState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Retained => "retained",
            Self::PlannedPrune => "planned_prune",
            Self::Gap => "gap",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "retained" => Ok(Self::Retained),
            "planned_prune" => Ok(Self::PlannedPrune),
            "gap" => Ok(Self::Gap),
            other => Err(LoomError::corrupt(format!(
                "unsupported change gap state {other:?}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeItemKind {
    Added,
    Updated,
    Removed,
    Sequence,
}

impl ChangeItemKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Added => "added",
            Self::Updated => "updated",
            Self::Removed => "removed",
            Self::Sequence => "sequence",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "added" => Ok(Self::Added),
            "updated" => Ok(Self::Updated),
            "removed" => Ok(Self::Removed),
            "sequence" => Ok(Self::Sequence),
            other => Err(LoomError::corrupt(format!(
                "unsupported change item kind {other:?}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeItem {
    pub id: String,
    pub kind: ChangeItemKind,
    pub etag: Option<Digest>,
    pub payload: Option<Vec<u8>>,
    pub sequence: Option<u64>,
}

impl ChangeItem {
    pub fn item_diff(id: impl Into<String>, kind: ChangeItemKind, etag: Option<Digest>) -> Self {
        Self {
            id: id.into(),
            kind,
            etag,
            payload: None,
            sequence: None,
        }
    }

    pub fn sequence_record(sequence: u64, payload: Vec<u8>) -> Self {
        Self {
            id: sequence.to_string(),
            kind: ChangeItemKind::Sequence,
            etag: None,
            payload: Some(payload),
            sequence: Some(sequence),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeSet {
    pub scope: String,
    pub gap_state: ChangeGapState,
    pub retained_low_water_mark: Option<u64>,
    pub next_cursor: ChangeCursor,
    pub items: Vec<ChangeItem>,
}

impl ChangeSet {
    pub fn new(
        scope: impl Into<String>,
        gap_state: ChangeGapState,
        retained_low_water_mark: Option<u64>,
        next_cursor: ChangeCursor,
        items: Vec<ChangeItem>,
    ) -> Result<Self> {
        let set = Self {
            scope: scope.into(),
            gap_state,
            retained_low_water_mark,
            next_cursor,
            items,
        };
        set.validate()?;
        Ok(set)
    }

    pub fn validate(&self) -> Result<()> {
        if self.scope.is_empty() {
            return Err(LoomError::invalid("change set scope is empty"));
        }
        if self.next_cursor.scope != self.scope {
            return Err(LoomError::invalid("change set cursor scope mismatch"));
        }
        if let Some(mark) = self.retained_low_water_mark {
            self.next_cursor.require_not_before_low_water(mark)?;
        }
        for item in &self.items {
            match item.kind {
                ChangeItemKind::Added | ChangeItemKind::Updated => {
                    if item.etag.is_none() || item.sequence.is_some() {
                        return Err(LoomError::invalid("item diff change requires etag only"));
                    }
                }
                ChangeItemKind::Removed => {
                    if item.payload.is_some() || item.sequence.is_some() {
                        return Err(LoomError::invalid(
                            "removed change cannot carry payload or sequence",
                        ));
                    }
                }
                ChangeItemKind::Sequence => {
                    if item.sequence.is_none() || item.payload.is_none() || item.etag.is_some() {
                        return Err(LoomError::invalid(
                            "sequence change requires sequence and payload only",
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    pub fn encode(&self) -> Vec<u8> {
        cbor::encode(&Value::Array(vec![
            Value::Text(CHANGE_SET_V1.to_string()),
            Value::Uint(1),
            Value::Text(self.scope.clone()),
            Value::Text(self.gap_state.as_str().to_string()),
            optional_u64_value(self.retained_low_water_mark),
            Value::Bytes(self.next_cursor.encode()),
            Value::Array(self.items.iter().map(change_item_value).collect()),
        ]))
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let mut fields = cbor::Fields::new(cbor::decode_array(bytes)?);
        let tag = fields.text()?;
        if tag != CHANGE_SET_V1 {
            return Err(LoomError::corrupt("change set tag mismatch"));
        }
        if fields.uint()? != 1 {
            return Err(LoomError::unsupported("unsupported change set version"));
        }
        let scope = fields.text()?;
        let gap_state = ChangeGapState::parse(&fields.text()?)?;
        let retained_low_water_mark = optional_u64_from_value(fields.next_field()?)?;
        let next_cursor = ChangeCursor::decode(&fields.bytes()?)?;
        let items = fields
            .array()?
            .into_iter()
            .map(decode_change_item)
            .collect::<Result<Vec<_>>>()?;
        fields.end()?;
        Self::new(
            scope,
            gap_state,
            retained_low_water_mark,
            next_cursor,
            items,
        )
    }
}

fn cursor_position_value(position: &ChangeCursorPosition) -> Value {
    match position {
        ChangeCursorPosition::Sequence { next } => Value::Array(vec![
            Value::Text("sequence".to_string()),
            Value::Uint(*next),
        ]),
        ChangeCursorPosition::Commit { anchor, index } => Value::Array(vec![
            Value::Text("commit".to_string()),
            cbor::digest_value(anchor),
            Value::Uint(*index),
        ]),
    }
}

fn decode_cursor_position(value: Value) -> Result<ChangeCursorPosition> {
    let mut fields = cbor::Fields::new(cbor::as_array(value)?);
    let kind = fields.text()?;
    let position = match kind.as_str() {
        "sequence" => ChangeCursorPosition::Sequence {
            next: fields.uint()?,
        },
        "commit" => ChangeCursorPosition::Commit {
            anchor: fields.digest()?,
            index: fields.uint()?,
        },
        _ => return Err(LoomError::corrupt("unsupported change cursor position")),
    };
    fields.end()?;
    Ok(position)
}

fn change_item_value(item: &ChangeItem) -> Value {
    Value::Array(vec![
        Value::Text(item.id.clone()),
        Value::Text(item.kind.as_str().to_string()),
        optional_digest_value(item.etag),
        optional_bytes_value(item.payload.as_deref()),
        optional_u64_value(item.sequence),
    ])
}

fn decode_change_item(value: Value) -> Result<ChangeItem> {
    let mut fields = cbor::Fields::new(cbor::as_array(value)?);
    let id = fields.text()?;
    let kind = ChangeItemKind::parse(&fields.text()?)?;
    let etag = optional_digest_from_value(fields.next_field()?)?;
    let payload = optional_bytes_from_value(fields.next_field()?)?;
    let sequence = optional_u64_from_value(fields.next_field()?)?;
    fields.end()?;
    Ok(ChangeItem {
        id,
        kind,
        etag,
        payload,
        sequence,
    })
}

fn optional_u64_value(value: Option<u64>) -> Value {
    value.map_or(Value::Null, Value::Uint)
}

fn optional_u64_from_value(value: Value) -> Result<Option<u64>> {
    match value {
        Value::Null => Ok(None),
        Value::Uint(value) => Ok(Some(value)),
        _ => Err(LoomError::corrupt("expected optional uint")),
    }
}

fn optional_digest_value(value: Option<Digest>) -> Value {
    value.map_or(Value::Null, |digest| cbor::digest_value(&digest))
}

fn optional_digest_from_value(value: Value) -> Result<Option<Digest>> {
    match value {
        Value::Null => Ok(None),
        value => Ok(Some(cbor::as_digest(value)?)),
    }
}

fn optional_bytes_value(value: Option<&[u8]>) -> Value {
    value.map_or(Value::Null, |bytes| Value::Bytes(bytes.to_vec()))
}

fn optional_bytes_from_value(value: Value) -> Result<Option<Vec<u8>>> {
    match value {
        Value::Null => Ok(None),
        value => Ok(Some(cbor::as_bytes(value)?)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Algo;

    #[test]
    fn change_cursor_round_trips_sequence_and_text() {
        let cursor = ChangeCursor::sequence("mail:alice/inbox", 7);
        let encoded = cursor.encode();
        assert_eq!(ChangeCursor::decode(&encoded).unwrap(), cursor);
        assert_eq!(
            ChangeCursor::decode_text(&cursor.encode_text()).unwrap(),
            cursor
        );
    }

    #[test]
    fn change_cursor_round_trips_commit_anchor() {
        let digest = Digest::hash(Algo::Blake3, b"commit");
        let cursor = ChangeCursor::commit("watch:workspace/main", digest, 3);
        assert_eq!(ChangeCursor::decode(&cursor.encode()).unwrap(), cursor);
    }

    #[test]
    fn low_water_mark_reports_retained_gap() {
        let err = ChangeCursor::sequence("queue:events", 2)
            .require_not_before_low_water(3)
            .unwrap_err();
        assert_eq!(err.code, crate::Code::RetainedGap);
    }

    #[test]
    fn change_set_round_trips_diff_and_sequence_records() {
        let etag = Digest::hash(Algo::Blake3, b"etag");
        let cursor = ChangeCursor::sequence("calendar:team", 4);
        let set = ChangeSet::new(
            "calendar:team",
            ChangeGapState::Retained,
            Some(4),
            cursor,
            vec![
                ChangeItem::item_diff("event-1", ChangeItemKind::Updated, Some(etag)),
                ChangeItem::sequence_record(3, b"payload".to_vec()),
            ],
        )
        .unwrap();
        assert_eq!(ChangeSet::decode(&set.encode()).unwrap(), set);
    }
}
