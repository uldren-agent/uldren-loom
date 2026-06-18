use loom_codec::Value;
use loom_types::{Code, Digest, LoomError, Result};

use crate::{codec_error, validate_text, view::validate_view_id};

pub const BODY_REF_SCHEMA: &str = "loom.substrate.body-ref.v1";
pub const ENTITY_REVISION_SCHEMA: &str = "loom.substrate.entity-revision.v1";
pub const REVISION_LOG_SCHEMA: &str = "loom.substrate.revision-log.v1";
pub const CHECKPOINT_SCHEMA: &str = "loom.substrate.checkpoint.v1";
pub const REVISION_INDEX_SCHEMA: &str = "loom.substrate.revision-index.v1";
pub const REVISION_INDEX_DIR: &str = ".loom/substrate/revisions";

pub fn revision_index_path(scope_id: &str) -> Result<String> {
    validate_view_id(scope_id)?;
    Ok(format!("{REVISION_INDEX_DIR}/{scope_id}.lri"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileRevisionUpdate {
    pub entity_id: String,
    pub operation_id: String,
    pub body: BodyRef,
    pub timestamp_ms: u64,
    pub checkpoint_id: String,
    pub expected_latest_revision: Option<u64>,
}

impl ProfileRevisionUpdate {
    pub fn new(
        entity_id: impl Into<String>,
        operation_id: impl Into<String>,
        body: BodyRef,
        timestamp_ms: u64,
        checkpoint_id: impl Into<String>,
        expected_latest_revision: Option<u64>,
    ) -> Result<Self> {
        let update = Self {
            entity_id: entity_id.into(),
            operation_id: operation_id.into(),
            body,
            timestamp_ms,
            checkpoint_id: checkpoint_id.into(),
            expected_latest_revision,
        };
        validate_text("entity_id", &update.entity_id)?;
        validate_text("operation_id", &update.operation_id)?;
        validate_text("checkpoint_id", &update.checkpoint_id)?;
        Ok(update)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileTransaction {
    pub scope_id: String,
    pub expected_root: Option<Digest>,
    pub root_after: Digest,
    pub revisions: Vec<ProfileRevisionUpdate>,
}

impl ProfileTransaction {
    pub fn new(
        scope_id: impl Into<String>,
        expected_root: Option<Digest>,
        root_after: Digest,
        revisions: Vec<ProfileRevisionUpdate>,
    ) -> Result<Self> {
        let transaction = Self {
            scope_id: scope_id.into(),
            expected_root,
            root_after,
            revisions,
        };
        validate_text("scope_id", &transaction.scope_id)?;
        if transaction.revisions.is_empty() {
            return Err(LoomError::invalid(
                "profile transaction must include at least one revision",
            ));
        }
        Ok(transaction)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileRevisionReceipt {
    pub entity_id: String,
    pub revision: u64,
    pub operation_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileTransactionReceipt {
    pub root_before: Digest,
    pub root_after: Digest,
    pub revisions: Vec<ProfileRevisionReceipt>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevisionBackfillUpdate {
    pub entity_id: String,
    pub operation_id: String,
    pub body: BodyRef,
    pub root: Digest,
    pub timestamp_ms: u64,
    pub checkpoint_id: String,
}

impl RevisionBackfillUpdate {
    pub fn new(
        entity_id: impl Into<String>,
        operation_id: impl Into<String>,
        body: BodyRef,
        root: Digest,
        timestamp_ms: u64,
        checkpoint_id: impl Into<String>,
    ) -> Result<Self> {
        let update = Self {
            entity_id: entity_id.into(),
            operation_id: operation_id.into(),
            body,
            root,
            timestamp_ms,
            checkpoint_id: checkpoint_id.into(),
        };
        validate_text("entity_id", &update.entity_id)?;
        validate_text("operation_id", &update.operation_id)?;
        validate_text("checkpoint_id", &update.checkpoint_id)?;
        Ok(update)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevisionBackfillReport {
    pub inserted: u64,
    pub skipped_existing: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileTransactionState {
    root: Digest,
    revision_index: RevisionIndex,
}

impl ProfileTransactionState {
    pub fn new(root: Digest, revision_index: RevisionIndex) -> Self {
        Self {
            root,
            revision_index,
        }
    }

    pub fn root(&self) -> Digest {
        self.root
    }

    pub fn revision_index(&self) -> &RevisionIndex {
        &self.revision_index
    }

    pub fn into_revision_index(self) -> RevisionIndex {
        self.revision_index
    }

    pub fn apply(&mut self, transaction: ProfileTransaction) -> Result<ProfileTransactionReceipt> {
        if let Some(expected_root) = transaction.expected_root
            && expected_root != self.root
        {
            return Err(LoomError::new(
                Code::Conflict,
                "profile transaction root does not match current root",
            ));
        }
        let mut next_index = self.revision_index.clone();
        let mut receipts = Vec::with_capacity(transaction.revisions.len());
        for update in transaction.revisions {
            let current_revision = next_index
                .latest(&update.entity_id)
                .map(|entry| entry.revision)
                .unwrap_or(0);
            if let Some(expected_latest_revision) = update.expected_latest_revision
                && expected_latest_revision != current_revision
            {
                return Err(LoomError::new(
                    Code::Conflict,
                    "profile entity revision does not match expected revision",
                ));
            }
            let revision = current_revision
                .checked_add(1)
                .ok_or_else(|| LoomError::invalid("profile entity revision overflow"))?;
            next_index.append_revision(EntityRevision::new(
                update.entity_id.clone(),
                revision,
                update.operation_id.clone(),
                update.body,
                transaction.root_after,
                update.timestamp_ms,
            )?)?;
            next_index.add_checkpoint(Checkpoint::new(
                transaction.scope_id.clone(),
                update.checkpoint_id,
                transaction.root_after,
                revision,
                update.operation_id.clone(),
                update.timestamp_ms,
            )?)?;
            receipts.push(ProfileRevisionReceipt {
                entity_id: update.entity_id,
                revision,
                operation_id: update.operation_id,
            });
        }
        let root_before = self.root;
        self.root = transaction.root_after;
        self.revision_index = next_index;
        Ok(ProfileTransactionReceipt {
            root_before,
            root_after: self.root,
            revisions: receipts,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodyRef {
    pub digest: Digest,
    pub len: u64,
    pub media_type: String,
}

impl BodyRef {
    pub fn new(digest: Digest, len: u64, media_type: impl Into<String>) -> Result<Self> {
        let media_type = media_type.into();
        validate_text("media_type", &media_type)?;
        Ok(Self {
            digest,
            len,
            media_type,
        })
    }

    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(BODY_REF_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.digest.to_string()),
                Value::Uint(self.len),
                Value::Text(self.media_type.clone()),
            ]),
        ])
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = ArrayFields::new(value, "body ref")?;
        outer.expect_schema(BODY_REF_SCHEMA)?;
        let mut fields = ArrayFields::new(outer.next("body ref fields")?, "body ref fields")?;
        outer.end("body ref")?;
        let digest = Digest::parse(&fields.text("digest")?)?;
        let len = fields.uint("len")?;
        let media_type = fields.text("media_type")?;
        fields.end("body ref fields")?;
        BodyRef::new(digest, len, media_type)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityRevision {
    pub entity_id: String,
    pub revision: u64,
    pub operation_id: String,
    pub body: BodyRef,
    pub root: Digest,
    pub timestamp_ms: u64,
}

impl EntityRevision {
    pub fn new(
        entity_id: impl Into<String>,
        revision: u64,
        operation_id: impl Into<String>,
        body: BodyRef,
        root: Digest,
        timestamp_ms: u64,
    ) -> Result<Self> {
        let entity_id = entity_id.into();
        let operation_id = operation_id.into();
        validate_text("entity_id", &entity_id)?;
        validate_text("operation_id", &operation_id)?;
        Ok(Self {
            entity_id,
            revision,
            operation_id,
            body,
            root,
            timestamp_ms,
        })
    }

    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(ENTITY_REVISION_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.entity_id.clone()),
                Value::Uint(self.revision),
                Value::Text(self.operation_id.clone()),
                self.body.to_value(),
                Value::Text(self.root.to_string()),
                Value::Uint(self.timestamp_ms),
            ]),
        ])
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = ArrayFields::new(value, "entity revision")?;
        outer.expect_schema(ENTITY_REVISION_SCHEMA)?;
        let mut fields = ArrayFields::new(
            outer.next("entity revision fields")?,
            "entity revision fields",
        )?;
        outer.end("entity revision")?;
        let entity_id = fields.text("entity_id")?;
        let revision = fields.uint("revision")?;
        let operation_id = fields.text("operation_id")?;
        let body = BodyRef::from_value(fields.next("body")?)?;
        let root = Digest::parse(&fields.text("root")?)?;
        let timestamp_ms = fields.uint("timestamp_ms")?;
        fields.end("entity revision fields")?;
        EntityRevision::new(entity_id, revision, operation_id, body, root, timestamp_ms)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RevisionLog {
    revisions: Vec<EntityRevision>,
}

impl RevisionLog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn append(&mut self, revision: EntityRevision) -> Result<()> {
        let expected = self
            .revisions
            .iter()
            .filter(|entry| entry.entity_id == revision.entity_id)
            .map(|entry| entry.revision)
            .max()
            .unwrap_or(0)
            + 1;
        if revision.revision != expected {
            return Err(LoomError::new(
                Code::Conflict,
                format!(
                    "entity revision must be {expected}, got {}",
                    revision.revision
                ),
            ));
        }
        self.revisions.push(revision);
        self.revisions.sort_by(|left, right| {
            left.entity_id
                .cmp(&right.entity_id)
                .then_with(|| left.revision.cmp(&right.revision))
        });
        Ok(())
    }

    pub fn latest(&self, entity_id: &str) -> Option<&EntityRevision> {
        self.revisions
            .iter()
            .filter(|entry| entry.entity_id == entity_id)
            .max_by_key(|entry| entry.revision)
    }

    pub fn at_revision(&self, entity_id: &str, revision: u64) -> Option<&EntityRevision> {
        self.revisions
            .iter()
            .find(|entry| entry.entity_id == entity_id && entry.revision == revision)
    }

    pub fn as_of_root(&self, entity_id: &str, root: &Digest) -> Option<&EntityRevision> {
        self.revisions
            .iter()
            .filter(|entry| entry.entity_id == entity_id && &entry.root == root)
            .max_by_key(|entry| entry.revision)
    }

    pub fn revisions(&self) -> &[EntityRevision] {
        &self.revisions
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(REVISION_LOG_SCHEMA.to_string()),
            Value::Array(
                self.revisions
                    .iter()
                    .map(EntityRevision::to_value)
                    .collect(),
            ),
        ])
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = ArrayFields::new(value, "revision log")?;
        outer.expect_schema(REVISION_LOG_SCHEMA)?;
        let revisions = array_items(outer.next("revisions")?, "revisions")?
            .into_iter()
            .map(EntityRevision::from_value)
            .collect::<Result<Vec<_>>>()?;
        outer.end("revision log")?;
        let mut log = RevisionLog::new();
        for revision in revisions {
            log.append(revision)?;
        }
        Ok(log)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkpoint {
    pub scope_id: String,
    pub checkpoint_id: String,
    pub root: Digest,
    pub max_revision: u64,
    pub operation_id: String,
    pub created_at_ms: u64,
}

impl Checkpoint {
    pub fn new(
        scope_id: impl Into<String>,
        checkpoint_id: impl Into<String>,
        root: Digest,
        max_revision: u64,
        operation_id: impl Into<String>,
        created_at_ms: u64,
    ) -> Result<Self> {
        let scope_id = scope_id.into();
        let checkpoint_id = checkpoint_id.into();
        let operation_id = operation_id.into();
        validate_text("scope_id", &scope_id)?;
        validate_text("checkpoint_id", &checkpoint_id)?;
        validate_text("operation_id", &operation_id)?;
        Ok(Self {
            scope_id,
            checkpoint_id,
            root,
            max_revision,
            operation_id,
            created_at_ms,
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
            Value::Text(CHECKPOINT_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.scope_id.clone()),
                Value::Text(self.checkpoint_id.clone()),
                Value::Text(self.root.to_string()),
                Value::Uint(self.max_revision),
                Value::Text(self.operation_id.clone()),
                Value::Uint(self.created_at_ms),
            ]),
        ])
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = ArrayFields::new(value, "checkpoint")?;
        outer.expect_schema(CHECKPOINT_SCHEMA)?;
        let mut fields = ArrayFields::new(outer.next("checkpoint fields")?, "checkpoint fields")?;
        outer.end("checkpoint")?;
        let scope_id = fields.text("scope_id")?;
        let checkpoint_id = fields.text("checkpoint_id")?;
        let root = Digest::parse(&fields.text("root")?)?;
        let max_revision = fields.uint("max_revision")?;
        let operation_id = fields.text("operation_id")?;
        let created_at_ms = fields.uint("created_at_ms")?;
        fields.end("checkpoint fields")?;
        Checkpoint::new(
            scope_id,
            checkpoint_id,
            root,
            max_revision,
            operation_id,
            created_at_ms,
        )
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RevisionIndex {
    log: RevisionLog,
    checkpoints: Vec<Checkpoint>,
}

impl RevisionIndex {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn append_revision(&mut self, revision: EntityRevision) -> Result<()> {
        self.log.append(revision)
    }

    pub fn add_checkpoint(&mut self, checkpoint: Checkpoint) -> Result<()> {
        if self.checkpoints.iter().any(|existing| {
            existing.scope_id == checkpoint.scope_id
                && existing.checkpoint_id == checkpoint.checkpoint_id
        }) {
            return Err(LoomError::new(
                Code::AlreadyExists,
                "checkpoint already exists in scope",
            ));
        }
        self.checkpoints.push(checkpoint);
        self.checkpoints.sort_by(|left, right| {
            left.scope_id
                .cmp(&right.scope_id)
                .then_with(|| left.max_revision.cmp(&right.max_revision))
                .then_with(|| left.checkpoint_id.cmp(&right.checkpoint_id))
        });
        Ok(())
    }

    pub fn history(&self, entity_id: &str) -> Vec<&EntityRevision> {
        self.log
            .revisions()
            .iter()
            .filter(|entry| entry.entity_id == entity_id)
            .collect()
    }

    pub fn latest(&self, entity_id: &str) -> Option<&EntityRevision> {
        self.log.latest(entity_id)
    }

    pub fn at_revision(&self, entity_id: &str, revision: u64) -> Option<&EntityRevision> {
        self.log.at_revision(entity_id, revision)
    }

    pub fn as_of_root(&self, entity_id: &str, root: &Digest) -> Option<&EntityRevision> {
        self.log.as_of_root(entity_id, root)
    }

    pub fn checkpoint_before_or_at(&self, scope_id: &str, revision: u64) -> Option<&Checkpoint> {
        self.checkpoints
            .iter()
            .filter(|entry| entry.scope_id == scope_id && entry.max_revision <= revision)
            .max_by_key(|entry| entry.max_revision)
    }

    pub fn checkpoints(&self) -> &[Checkpoint] {
        &self.checkpoints
    }

    pub fn backfill_missing_current(
        &mut self,
        scope_id: &str,
        updates: impl IntoIterator<Item = RevisionBackfillUpdate>,
    ) -> Result<RevisionBackfillReport> {
        validate_text("scope_id", scope_id)?;
        let mut inserted = 0u64;
        let mut skipped_existing = 0u64;
        for update in updates {
            if self.latest(&update.entity_id).is_some() {
                skipped_existing = skipped_existing.saturating_add(1);
                continue;
            }
            self.append_revision(EntityRevision::new(
                update.entity_id,
                1,
                update.operation_id.clone(),
                update.body,
                update.root,
                update.timestamp_ms,
            )?)?;
            self.add_checkpoint(Checkpoint::new(
                scope_id,
                update.checkpoint_id,
                update.root,
                1,
                update.operation_id,
                update.timestamp_ms,
            )?)?;
            inserted = inserted.saturating_add(1);
        }
        Ok(RevisionBackfillReport {
            inserted,
            skipped_existing,
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
            Value::Text(REVISION_INDEX_SCHEMA.to_string()),
            Value::Array(vec![
                self.log.to_value(),
                Value::Array(self.checkpoints.iter().map(Checkpoint::to_value).collect()),
            ]),
        ])
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = ArrayFields::new(value, "revision index")?;
        outer.expect_schema(REVISION_INDEX_SCHEMA)?;
        let mut fields = ArrayFields::new(
            outer.next("revision index fields")?,
            "revision index fields",
        )?;
        outer.end("revision index")?;
        let log = RevisionLog::from_value(fields.next("revision log")?)?;
        let checkpoints = array_items(fields.next("checkpoints")?, "checkpoints")?
            .into_iter()
            .map(Checkpoint::from_value)
            .collect::<Result<Vec<_>>>()?;
        fields.end("revision index fields")?;
        let mut index = RevisionIndex::new();
        for revision in log.revisions() {
            index.append_revision(revision.clone())?;
        }
        for checkpoint in checkpoints {
            index.add_checkpoint(checkpoint)?;
        }
        Ok(index)
    }
}

fn array_items(value: Value, name: &str) -> Result<Vec<Value>> {
    match value {
        Value::Array(values) => Ok(values),
        _ => Err(LoomError::corrupt(format!("{name} must be an array"))),
    }
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

    fn end(&mut self, name: &str) -> Result<()> {
        if self.values.next().is_some() {
            return Err(LoomError::corrupt(format!("{name} has trailing fields")));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_types::Algo;

    fn digest(value: &[u8]) -> Digest {
        Digest::hash(Algo::Blake3, value)
    }

    #[test]
    fn revision_log_assigns_monotonic_entity_revisions() {
        let mut log = RevisionLog::new();
        let body = BodyRef::new(digest(b"v1"), 2, "text/plain").unwrap();
        log.append(
            EntityRevision::new("ISSUE-1", 1, "op-1", body.clone(), digest(b"root-1"), 10).unwrap(),
        )
        .unwrap();
        assert_eq!(
            log.append(
                EntityRevision::new("ISSUE-1", 3, "op-3", body, digest(b"root-3"), 30).unwrap()
            )
            .unwrap_err()
            .code,
            loom_types::Code::Conflict
        );
    }

    #[test]
    fn revision_log_supports_latest_revision_and_root_lookup() {
        let mut log = RevisionLog::new();
        let root_1 = digest(b"root-1");
        let root_2 = digest(b"root-2");
        log.append(
            EntityRevision::new(
                "ISSUE-1",
                1,
                "op-1",
                BodyRef::new(digest(b"v1"), 2, "text/plain").unwrap(),
                root_1,
                10,
            )
            .unwrap(),
        )
        .unwrap();
        log.append(
            EntityRevision::new(
                "ISSUE-1",
                2,
                "op-2",
                BodyRef::new(digest(b"v2"), 2, "text/plain").unwrap(),
                root_2,
                20,
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(log.latest("ISSUE-1").unwrap().revision, 2);
        assert_eq!(log.at_revision("ISSUE-1", 1).unwrap().operation_id, "op-1");
        assert_eq!(log.as_of_root("ISSUE-1", &root_2).unwrap().revision, 2);
        let bytes = log.encode().unwrap();
        assert_eq!(RevisionLog::decode(&bytes).unwrap(), log);
    }

    #[test]
    fn checkpoint_encodes_scope_root_and_revision_boundary() {
        let checkpoint =
            Checkpoint::new("PROJ", "ready", digest(b"root"), 7, "op-ready", 40).unwrap();
        let bytes = checkpoint.encode().unwrap();
        assert_eq!(Checkpoint::decode(&bytes).unwrap(), checkpoint);
    }

    #[test]
    fn revision_index_projects_history_and_checkpoints() {
        let root_1 = digest(b"root-1");
        let root_2 = digest(b"root-2");
        let root_3 = digest(b"root-3");
        let mut index = RevisionIndex::new();
        for (revision, root, timestamp_ms) in [(1, root_1, 10), (2, root_2, 20), (3, root_3, 30)] {
            index
                .append_revision(
                    EntityRevision::new(
                        "ISSUE-1",
                        revision,
                        format!("op-{revision}"),
                        BodyRef::new(digest(format!("v{revision}").as_bytes()), 2, "text/plain")
                            .unwrap(),
                        root,
                        timestamp_ms,
                    )
                    .unwrap(),
                )
                .unwrap();
        }
        index
            .add_checkpoint(Checkpoint::new("PROJ", "cp-1", root_1, 1, "op-1", 11).unwrap())
            .unwrap();
        index
            .add_checkpoint(Checkpoint::new("PROJ", "cp-3", root_3, 3, "op-3", 31).unwrap())
            .unwrap();

        assert_eq!(index.history("ISSUE-1").len(), 3);
        assert_eq!(index.latest("ISSUE-1").unwrap().revision, 3);
        assert_eq!(index.at_revision("ISSUE-1", 2).unwrap().root, root_2);
        assert_eq!(index.as_of_root("ISSUE-1", &root_2).unwrap().revision, 2);
        assert_eq!(
            index
                .checkpoint_before_or_at("PROJ", 2)
                .unwrap()
                .checkpoint_id,
            "cp-1"
        );
        assert_eq!(
            index
                .checkpoint_before_or_at("PROJ", 3)
                .unwrap()
                .checkpoint_id,
            "cp-3"
        );
        assert!(index.checkpoint_before_or_at("PROJ", 0).is_none());
        assert_eq!(
            RevisionIndex::decode(&index.encode().unwrap()).unwrap(),
            index
        );
    }

    #[test]
    fn revision_index_rejects_duplicate_checkpoint_ids_per_scope() {
        let root = digest(b"root");
        let mut index = RevisionIndex::new();
        index
            .add_checkpoint(Checkpoint::new("PROJ", "cp", root, 1, "op-1", 10).unwrap())
            .unwrap();
        assert_eq!(
            index
                .add_checkpoint(Checkpoint::new("PROJ", "cp", root, 2, "op-2", 20).unwrap())
                .unwrap_err()
                .code,
            Code::AlreadyExists
        );
    }

    #[test]
    fn revision_index_backfills_missing_current_rows_once() {
        let root = digest(b"root");
        let mut index = RevisionIndex::new();
        index
            .append_revision(
                EntityRevision::new(
                    "page:existing",
                    1,
                    "op-existing",
                    BodyRef::new(digest(b"existing"), 8, "text/plain").unwrap(),
                    root,
                    10,
                )
                .unwrap(),
            )
            .unwrap();
        index
            .add_checkpoint(
                Checkpoint::new("studio", "page:existing:1", root, 1, "op-existing", 10).unwrap(),
            )
            .unwrap();

        let report = index
            .backfill_missing_current(
                "studio",
                vec![
                    RevisionBackfillUpdate::new(
                        "page:existing",
                        "op-existing-backfill",
                        BodyRef::new(digest(b"existing"), 8, "text/plain").unwrap(),
                        root,
                        20,
                        "page:existing:backfill:1",
                    )
                    .unwrap(),
                    RevisionBackfillUpdate::new(
                        "page:new",
                        "op-new-backfill",
                        BodyRef::new(digest(b"new"), 3, "text/plain").unwrap(),
                        root,
                        20,
                        "page:new:backfill:1",
                    )
                    .unwrap(),
                ],
            )
            .unwrap();

        assert_eq!(report.inserted, 1);
        assert_eq!(report.skipped_existing, 1);
        assert_eq!(index.history("page:existing").len(), 1);
        assert_eq!(index.history("page:new").len(), 1);
        assert_eq!(
            index.latest("page:new").unwrap().operation_id,
            "op-new-backfill"
        );
        assert!(
            index
                .checkpoints()
                .iter()
                .any(|checkpoint| checkpoint.checkpoint_id == "page:new:backfill:1")
        );
    }

    #[test]
    fn profile_transaction_compares_root_and_advances_revision_index() {
        let root_1 = digest(b"root-1");
        let root_2 = digest(b"root-2");
        let mut state = ProfileTransactionState::new(root_1, RevisionIndex::new());

        let receipt = state
            .apply(
                ProfileTransaction::new(
                    "studio",
                    Some(root_1),
                    root_2,
                    vec![
                        ProfileRevisionUpdate::new(
                            "page:one",
                            "op-1",
                            BodyRef::new(digest(b"body-1"), 6, "text/plain").unwrap(),
                            10,
                            "page:one:1",
                            Some(0),
                        )
                        .unwrap(),
                    ],
                )
                .unwrap(),
            )
            .unwrap();

        assert_eq!(receipt.root_before, root_1);
        assert_eq!(receipt.root_after, root_2);
        assert_eq!(receipt.revisions[0].revision, 1);
        assert_eq!(state.root(), root_2);
        assert_eq!(
            state
                .revision_index()
                .latest("page:one")
                .unwrap()
                .operation_id,
            "op-1"
        );
        assert_eq!(
            state
                .revision_index()
                .checkpoint_before_or_at("studio", 1)
                .unwrap()
                .checkpoint_id,
            "page:one:1"
        );
    }

    #[test]
    fn profile_transaction_conflict_leaves_state_unchanged() {
        let root_1 = digest(b"root-1");
        let root_2 = digest(b"root-2");
        let root_3 = digest(b"root-3");
        let mut state = ProfileTransactionState::new(root_1, RevisionIndex::new());
        state
            .apply(
                ProfileTransaction::new(
                    "studio",
                    Some(root_1),
                    root_2,
                    vec![
                        ProfileRevisionUpdate::new(
                            "ticket:one",
                            "op-1",
                            BodyRef::new(digest(b"body-1"), 6, "text/plain").unwrap(),
                            10,
                            "ticket:one:1",
                            Some(0),
                        )
                        .unwrap(),
                    ],
                )
                .unwrap(),
            )
            .unwrap();

        let conflict = state
            .apply(
                ProfileTransaction::new(
                    "studio",
                    Some(root_1),
                    root_3,
                    vec![
                        ProfileRevisionUpdate::new(
                            "ticket:one",
                            "op-2",
                            BodyRef::new(digest(b"body-2"), 6, "text/plain").unwrap(),
                            20,
                            "ticket:one:2",
                            Some(1),
                        )
                        .unwrap(),
                    ],
                )
                .unwrap(),
            )
            .unwrap_err();

        assert_eq!(conflict.code, Code::Conflict);
        assert_eq!(state.root(), root_2);
        assert_eq!(
            state
                .revision_index()
                .latest("ticket:one")
                .unwrap()
                .revision,
            1
        );
        assert_eq!(state.revision_index().checkpoints().len(), 1);
    }

    #[test]
    fn profile_transaction_checks_expected_entity_revision_atomically() {
        let root_1 = digest(b"root-1");
        let root_2 = digest(b"root-2");
        let mut state = ProfileTransactionState::new(root_1, RevisionIndex::new());
        let conflict = state
            .apply(
                ProfileTransaction::new(
                    "studio",
                    Some(root_1),
                    root_2,
                    vec![
                        ProfileRevisionUpdate::new(
                            "meeting:one",
                            "op-1",
                            BodyRef::new(digest(b"body-1"), 6, "text/plain").unwrap(),
                            10,
                            "meeting:one:1",
                            Some(1),
                        )
                        .unwrap(),
                    ],
                )
                .unwrap(),
            )
            .unwrap_err();

        assert_eq!(conflict.code, Code::Conflict);
        assert_eq!(state.root(), root_1);
        assert!(state.revision_index().latest("meeting:one").is_none());
        assert!(state.revision_index().checkpoints().is_empty());
    }
}
