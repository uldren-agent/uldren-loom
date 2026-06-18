use std::collections::{BTreeMap, BTreeSet};

use chrono::{TimeZone, Utc};
use loom_codec::Value;
use loom_types::{Code, Digest, LoomError, Result, WorkspaceId};
use unicode_normalization::UnicodeNormalization;

use crate::{Fields, codec_error, validate_text};

pub const APP_ID: &str = "drive";
pub const PROFILE_SNAPSHOT_SCHEMA: &str = "loom.studio.drive.profile-snapshot.v1";
pub const FOLDER_INDEX_SCHEMA: &str = "loom.studio.drive.folder-index.v1";
pub const FILE_VERSION_INDEX_SCHEMA: &str = "loom.studio.drive.file-version-index.v1";
pub const CHUNK_MANIFEST_SCHEMA: &str = "loom.studio.drive.chunk-manifest.v1";
pub const OPERATION_LOG_SCHEMA: &str = "loom.studio.drive.operation-log.v1";
pub const UPLOAD_SESSION_SCHEMA: &str = "loom.studio.drive.upload-session.v1";
pub const CONFLICT_INDEX_SCHEMA: &str = "loom.studio.drive.conflict-index.v1";
pub const SHARE_INDEX_SCHEMA: &str = "loom.studio.drive.share-index.v1";
pub const RETENTION_INDEX_SCHEMA: &str = "loom.studio.drive.retention-index.v1";
pub const POLICY_REGISTRY_SCHEMA: &str = "loom.studio.drive.policy-registry.v1";
pub const DEHYDRATED_FILE_MARKER_VERSION: u64 = 1;
pub const DEHYDRATED_FILE_MARKER_MAGIC: &[u8] = b"LOOM-DRIVE-DEHYDRATED\0";
pub const PROFILE_CONTROL_PREFIX: &str = "profile/drive/v1";
pub const CHUNK_MIN_SIZE: u64 = 512 * 1024;
pub const CHUNK_AVG_SIZE: u64 = 1024 * 1024;
pub const CHUNK_MAX_SIZE: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriveConcurrentOperation {
    CreateFile {
        folder_id: String,
        name: String,
        content_digest: Digest,
        actor_display: String,
        timestamp_ms: u64,
    },
    Rename {
        node_id: String,
        new_name: String,
    },
    Move {
        node_id: String,
        target_folder_id: String,
        creates_cycle: bool,
    },
    Delete {
        node_id: String,
        folder_delete: bool,
    },
    ContentEdit {
        file_id: String,
        content_digest: Digest,
    },
    DescendantEdit {
        ancestor_folder_id: String,
        node_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriveMergeOutcome {
    Merge,
    Deduplicate,
    ConflictCopy { name: String },
    ConflictRecord { field_or_region: String },
    Reject { rule: String },
}

pub fn drive_merge_outcome(
    first: &DriveConcurrentOperation,
    second: &DriveConcurrentOperation,
) -> Result<DriveMergeOutcome> {
    use DriveConcurrentOperation as Op;
    match (first, second) {
        (
            Op::CreateFile {
                folder_id: left_folder,
                name: left_name,
                content_digest: left_digest,
                ..
            },
            Op::CreateFile {
                folder_id: right_folder,
                name: right_name,
                content_digest: right_digest,
                actor_display,
                timestamp_ms,
            },
        ) if left_folder == right_folder => {
            let left_key = drive_fold_key(left_name)?;
            let right_key = drive_fold_key(right_name)?;
            if left_key != right_key {
                Ok(DriveMergeOutcome::Merge)
            } else if left_digest == right_digest {
                Ok(DriveMergeOutcome::Deduplicate)
            } else {
                Ok(DriveMergeOutcome::ConflictCopy {
                    name: conflict_copy_name(right_name, actor_display, *timestamp_ms, 1)?,
                })
            }
        }
        (
            Op::Rename {
                node_id: left_node,
                new_name: left_name,
            },
            Op::Rename {
                node_id: right_node,
                new_name: right_name,
            },
        ) if left_node == right_node
            && drive_fold_key(left_name)? != drive_fold_key(right_name)? =>
        {
            Ok(DriveMergeOutcome::ConflictRecord {
                field_or_region: "name".to_string(),
            })
        }
        (
            Op::Move {
                node_id: left_node,
                target_folder_id: left_folder,
                creates_cycle,
            },
            Op::Move {
                node_id: right_node,
                target_folder_id: right_folder,
                ..
            },
        ) if *creates_cycle && left_node == right_node && left_folder != right_folder => {
            Ok(DriveMergeOutcome::Reject {
                rule: "path_cycle".to_string(),
            })
        }
        (
            Op::Move {
                node_id: left_node,
                target_folder_id: left_folder,
                ..
            },
            Op::Move {
                node_id: right_node,
                target_folder_id: right_folder,
                creates_cycle,
            },
        ) if *creates_cycle && left_node == right_node && left_folder != right_folder => {
            Ok(DriveMergeOutcome::Reject {
                rule: "path_cycle".to_string(),
            })
        }
        (
            Op::Move {
                node_id: left_node,
                target_folder_id: left_folder,
                ..
            },
            Op::Move {
                node_id: right_node,
                target_folder_id: right_folder,
                ..
            },
        ) if left_node == right_node && left_folder != right_folder => {
            Ok(DriveMergeOutcome::ConflictRecord {
                field_or_region: "folder".to_string(),
            })
        }
        (Op::Move { node_id, .. }, Op::ContentEdit { file_id, .. })
        | (Op::ContentEdit { file_id, .. }, Op::Move { node_id, .. })
            if node_id == file_id =>
        {
            Ok(DriveMergeOutcome::Merge)
        }
        (Op::Delete { node_id, .. }, Op::ContentEdit { file_id, .. })
        | (Op::ContentEdit { file_id, .. }, Op::Delete { node_id, .. })
            if node_id == file_id =>
        {
            Ok(DriveMergeOutcome::ConflictRecord {
                field_or_region: "delete".to_string(),
            })
        }
        (
            Op::Delete {
                node_id,
                folder_delete: true,
            },
            Op::DescendantEdit {
                ancestor_folder_id, ..
            },
        )
        | (
            Op::DescendantEdit {
                ancestor_folder_id, ..
            },
            Op::Delete {
                node_id,
                folder_delete: true,
            },
        ) if node_id == ancestor_folder_id => Ok(DriveMergeOutcome::ConflictRecord {
            field_or_region: "descendant".to_string(),
        }),
        _ => Ok(DriveMergeOutcome::Merge),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriveShareTargetKind {
    File,
    Folder,
    Comment,
    Link,
    Artifact,
}

impl DriveShareTargetKind {
    const fn tag(self) -> u64 {
        match self {
            Self::File => 0,
            Self::Folder => 1,
            Self::Comment => 2,
            Self::Link => 3,
            Self::Artifact => 4,
        }
    }

    fn from_tag(tag: u64) -> Result<Self> {
        match tag {
            0 => Ok(Self::File),
            1 => Ok(Self::Folder),
            2 => Ok(Self::Comment),
            3 => Ok(Self::Link),
            4 => Ok(Self::Artifact),
            other => Err(LoomError::corrupt(format!(
                "unknown drive share target kind tag {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriveShareRole {
    Viewer,
    Commenter,
    Editor,
    Owner,
    AgentReader,
    AgentEditor,
}

impl DriveShareRole {
    const fn tag(self) -> u64 {
        match self {
            Self::Viewer => 0,
            Self::Commenter => 1,
            Self::Editor => 2,
            Self::Owner => 3,
            Self::AgentReader => 4,
            Self::AgentEditor => 5,
        }
    }

    fn from_tag(tag: u64) -> Result<Self> {
        match tag {
            0 => Ok(Self::Viewer),
            1 => Ok(Self::Commenter),
            2 => Ok(Self::Editor),
            3 => Ok(Self::Owner),
            4 => Ok(Self::AgentReader),
            5 => Ok(Self::AgentEditor),
            other => Err(LoomError::corrupt(format!(
                "unknown drive share role tag {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveShareGrant {
    pub grant_id: String,
    pub target_kind: DriveShareTargetKind,
    pub target_id: String,
    pub principal: WorkspaceId,
    pub role: DriveShareRole,
    pub granted_by: WorkspaceId,
    pub granted_at_ms: u64,
    pub expires_at_ms: Option<u64>,
}

impl DriveShareGrant {
    pub fn new(input: DriveShareGrantInput) -> Result<Self> {
        let grant = Self {
            grant_id: input.grant_id,
            target_kind: input.target_kind,
            target_id: input.target_id,
            principal: input.principal,
            role: input.role,
            granted_by: input.granted_by,
            granted_at_ms: input.granted_at_ms,
            expires_at_ms: input.expires_at_ms,
        };
        grant.validate()?;
        Ok(grant)
    }

    fn validate(&self) -> Result<()> {
        validate_text("drive share grant_id", &self.grant_id)?;
        validate_text("drive share target_id", &self.target_id)?;
        if let Some(expires_at_ms) = self.expires_at_ms
            && expires_at_ms <= self.granted_at_ms
        {
            return Err(LoomError::invalid(
                "drive share expiration must be after grant time",
            ));
        }
        Ok(())
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(self.grant_id.clone()),
            Value::Uint(self.target_kind.tag()),
            Value::Text(self.target_id.clone()),
            Value::Text(self.principal.to_string()),
            Value::Uint(self.role.tag()),
            Value::Text(self.granted_by.to_string()),
            Value::Uint(self.granted_at_ms),
            optional_u64_value(self.expires_at_ms),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut fields = Fields::array(value, "drive share grant")?;
        let grant_id = fields.text("grant_id")?;
        let target_kind = DriveShareTargetKind::from_tag(fields.uint("target_kind")?)?;
        let target_id = fields.text("target_id")?;
        let principal = WorkspaceId::parse(&fields.text("principal")?)?;
        let role = DriveShareRole::from_tag(fields.uint("role")?)?;
        let granted_by = WorkspaceId::parse(&fields.text("granted_by")?)?;
        let granted_at_ms = fields.uint("granted_at_ms")?;
        let expires_at_ms = read_optional_u64(&mut fields, "expires_at_ms")?;
        fields.end("drive share grant")?;
        Self::new(DriveShareGrantInput {
            grant_id,
            target_kind,
            target_id,
            principal,
            role,
            granted_by,
            granted_at_ms,
            expires_at_ms,
        })
    }
}

pub struct DriveShareGrantInput {
    pub grant_id: String,
    pub target_kind: DriveShareTargetKind,
    pub target_id: String,
    pub principal: WorkspaceId,
    pub role: DriveShareRole,
    pub granted_by: WorkspaceId,
    pub granted_at_ms: u64,
    pub expires_at_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveShareIndex {
    pub workspace_id: String,
    pub grants: Vec<DriveShareGrant>,
}

impl DriveShareIndex {
    pub fn new(workspace_id: impl Into<String>, grants: Vec<DriveShareGrant>) -> Result<Self> {
        let index = Self {
            workspace_id: workspace_id.into(),
            grants,
        };
        index.validate()?;
        Ok(index)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    fn validate(&self) -> Result<()> {
        validate_text("workspace_id", &self.workspace_id)?;
        let mut ids = BTreeSet::new();
        for grant in &self.grants {
            grant.validate()?;
            if !ids.insert(grant.grant_id.clone()) {
                return Err(LoomError::invalid("drive share grant ids must be unique"));
            }
        }
        Ok(())
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(SHARE_INDEX_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.workspace_id.clone()),
                Value::Array(self.grants.iter().map(DriveShareGrant::to_value).collect()),
            ]),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "drive share index")?;
        outer.expect_text(SHARE_INDEX_SCHEMA)?;
        let mut fields =
            Fields::array(outer.next("drive share index fields")?, "drive share index")?;
        outer.end("drive share index")?;
        let workspace_id = fields.text("workspace_id")?;
        let grants = share_grant_list(fields.next("grants")?)?;
        fields.end("drive share index")?;
        Self::new(workspace_id, grants)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriveRetentionPinKind {
    CurrentRoot,
    TrashSubtree,
    LegalHold,
    RevisionRetention,
}

impl DriveRetentionPinKind {
    const fn tag(self) -> u64 {
        match self {
            Self::CurrentRoot => 0,
            Self::TrashSubtree => 1,
            Self::LegalHold => 2,
            Self::RevisionRetention => 3,
        }
    }

    fn from_tag(tag: u64) -> Result<Self> {
        match tag {
            0 => Ok(Self::CurrentRoot),
            1 => Ok(Self::TrashSubtree),
            2 => Ok(Self::LegalHold),
            3 => Ok(Self::RevisionRetention),
            other => Err(LoomError::corrupt(format!(
                "unknown drive retention pin kind tag {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveRetentionPin {
    pub pin_id: String,
    pub kind: DriveRetentionPinKind,
    pub root: Digest,
    pub target_entity_id: Option<String>,
    pub added_by: WorkspaceId,
    pub added_at_ms: u64,
    pub expires_at_ms: Option<u64>,
}

impl DriveRetentionPin {
    pub fn new(input: DriveRetentionPinInput) -> Result<Self> {
        let pin = Self {
            pin_id: input.pin_id,
            kind: input.kind,
            root: input.root,
            target_entity_id: input.target_entity_id,
            added_by: input.added_by,
            added_at_ms: input.added_at_ms,
            expires_at_ms: input.expires_at_ms,
        };
        pin.validate()?;
        Ok(pin)
    }

    fn validate(&self) -> Result<()> {
        validate_text("drive retention pin_id", &self.pin_id)?;
        if let Some(target) = &self.target_entity_id {
            validate_text("drive retention target", target)?;
        }
        if self.kind == DriveRetentionPinKind::LegalHold && self.expires_at_ms.is_some() {
            return Err(LoomError::invalid("drive legal-hold pins must not expire"));
        }
        if let Some(expires_at_ms) = self.expires_at_ms
            && expires_at_ms <= self.added_at_ms
        {
            return Err(LoomError::invalid(
                "drive retention expiration must be after pin time",
            ));
        }
        Ok(())
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(self.pin_id.clone()),
            Value::Uint(self.kind.tag()),
            digest_value(self.root),
            optional_text_value(self.target_entity_id.as_deref()),
            Value::Text(self.added_by.to_string()),
            Value::Uint(self.added_at_ms),
            optional_u64_value(self.expires_at_ms),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut fields = Fields::array(value, "drive retention pin")?;
        let pin_id = fields.text("pin_id")?;
        let kind = DriveRetentionPinKind::from_tag(fields.uint("kind")?)?;
        let root = fields.digest("root")?;
        let target_entity_id = read_optional_text(&mut fields, "target_entity_id")?;
        let added_by = WorkspaceId::parse(&fields.text("added_by")?)?;
        let added_at_ms = fields.uint("added_at_ms")?;
        let expires_at_ms = read_optional_u64(&mut fields, "expires_at_ms")?;
        fields.end("drive retention pin")?;
        Self::new(DriveRetentionPinInput {
            pin_id,
            kind,
            root,
            target_entity_id,
            added_by,
            added_at_ms,
            expires_at_ms,
        })
    }
}

pub struct DriveRetentionPinInput {
    pub pin_id: String,
    pub kind: DriveRetentionPinKind,
    pub root: Digest,
    pub target_entity_id: Option<String>,
    pub added_by: WorkspaceId,
    pub added_at_ms: u64,
    pub expires_at_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveRetentionIndex {
    pub workspace_id: String,
    pub pins: Vec<DriveRetentionPin>,
}

impl DriveRetentionIndex {
    pub fn new(workspace_id: impl Into<String>, pins: Vec<DriveRetentionPin>) -> Result<Self> {
        let index = Self {
            workspace_id: workspace_id.into(),
            pins,
        };
        index.validate()?;
        Ok(index)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    pub fn live_roots(&self) -> BTreeSet<Digest> {
        self.pins.iter().map(|pin| pin.root).collect()
    }

    fn validate(&self) -> Result<()> {
        validate_text("workspace_id", &self.workspace_id)?;
        let mut ids = BTreeSet::new();
        for pin in &self.pins {
            pin.validate()?;
            if !ids.insert(pin.pin_id.clone()) {
                return Err(LoomError::invalid("drive retention pin ids must be unique"));
            }
        }
        Ok(())
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(RETENTION_INDEX_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.workspace_id.clone()),
                Value::Array(self.pins.iter().map(DriveRetentionPin::to_value).collect()),
            ]),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "drive retention index")?;
        outer.expect_text(RETENTION_INDEX_SCHEMA)?;
        let mut fields = Fields::array(
            outer.next("drive retention index fields")?,
            "drive retention index",
        )?;
        outer.end("drive retention index")?;
        let workspace_id = fields.text("workspace_id")?;
        let pins = retention_pin_list(fields.next("pins")?)?;
        fields.end("drive retention index")?;
        Self::new(workspace_id, pins)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrivePolicyTarget {
    pub workspace: WorkspaceId,
    pub workspace_id: String,
    pub enabled: bool,
}

impl DrivePolicyTarget {
    pub fn new(
        workspace: WorkspaceId,
        workspace_id: impl Into<String>,
        enabled: bool,
    ) -> Result<Self> {
        let target = Self {
            workspace,
            workspace_id: workspace_id.into(),
            enabled,
        };
        target.validate()?;
        Ok(target)
    }

    fn validate(&self) -> Result<()> {
        validate_text("workspace_id", &self.workspace_id)
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(self.workspace.to_string()),
            Value::Text(self.workspace_id.clone()),
            Value::Bool(self.enabled),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut fields = Fields::array(value, "drive policy target")?;
        let workspace = WorkspaceId::parse(&fields.text("workspace")?)?;
        let workspace_id = fields.text("workspace_id")?;
        let enabled = fields.bool("enabled")?;
        fields.end("drive policy target")?;
        Self::new(workspace, workspace_id, enabled)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrivePolicyRegistry {
    pub targets: Vec<DrivePolicyTarget>,
}

impl DrivePolicyRegistry {
    pub fn new(targets: Vec<DrivePolicyTarget>) -> Result<Self> {
        let registry = Self { targets };
        registry.validate()?;
        Ok(registry)
    }

    pub fn empty() -> Self {
        Self {
            targets: Vec::new(),
        }
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    pub fn upsert_enabled(&mut self, target: DrivePolicyTarget) -> Result<()> {
        target.validate()?;
        if let Some(existing) = self.targets.iter_mut().find(|existing| {
            existing.workspace == target.workspace && existing.workspace_id == target.workspace_id
        }) {
            existing.enabled = target.enabled;
        } else {
            self.targets.push(target);
        }
        self.targets.sort_by(|a, b| {
            (a.workspace, a.workspace_id.as_str()).cmp(&(b.workspace, b.workspace_id.as_str()))
        });
        self.validate()
    }

    pub fn enabled_targets(&self) -> impl Iterator<Item = &DrivePolicyTarget> {
        self.targets.iter().filter(|target| target.enabled)
    }

    fn validate(&self) -> Result<()> {
        let mut keys = BTreeSet::new();
        for target in &self.targets {
            target.validate()?;
            if !keys.insert((target.workspace, target.workspace_id.clone())) {
                return Err(LoomError::invalid("drive policy targets must be unique"));
            }
        }
        Ok(())
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(POLICY_REGISTRY_SCHEMA.to_string()),
            Value::Array(
                self.targets
                    .iter()
                    .map(DrivePolicyTarget::to_value)
                    .collect(),
            ),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "drive policy registry")?;
        outer.expect_text(POLICY_REGISTRY_SCHEMA)?;
        let targets = policy_target_list(outer.next("targets")?)?;
        outer.end("drive policy registry")?;
        Self::new(targets)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriveUploadTargetKind {
    NewFile,
    ReplaceFile,
}

impl DriveUploadTargetKind {
    const fn tag(self) -> u64 {
        match self {
            Self::NewFile => 0,
            Self::ReplaceFile => 1,
        }
    }

    fn from_tag(tag: u64) -> Result<Self> {
        match tag {
            0 => Ok(Self::NewFile),
            1 => Ok(Self::ReplaceFile),
            other => Err(LoomError::corrupt(format!(
                "unknown drive upload target kind tag {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveUploadChunk {
    pub sequence: u64,
    pub digest: Digest,
    pub size: u64,
}

impl DriveUploadChunk {
    pub fn new(sequence: u64, digest: Digest, size: u64) -> Result<Self> {
        if size == 0 || size > CHUNK_MAX_SIZE {
            return Err(LoomError::invalid(
                "drive upload chunk size is out of range",
            ));
        }
        Ok(Self {
            sequence,
            digest,
            size,
        })
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Uint(self.sequence),
            digest_value(self.digest),
            Value::Uint(self.size),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut fields = Fields::array(value, "drive upload chunk")?;
        let sequence = fields.uint("sequence")?;
        let digest = fields.digest("digest")?;
        let size = fields.uint("size")?;
        fields.end("drive upload chunk")?;
        Self::new(sequence, digest, size)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveUploadSession {
    pub workspace_id: String,
    pub upload_id: String,
    pub target_kind: DriveUploadTargetKind,
    pub parent_folder_id: String,
    pub name: String,
    pub file_id: String,
    pub expected_root: Digest,
    pub author_principal: WorkspaceId,
    pub created_at_ms: u64,
    pub chunks: Vec<DriveUploadChunk>,
}

impl DriveUploadSession {
    pub fn new(input: DriveUploadSessionInput) -> Result<Self> {
        let session = Self {
            workspace_id: input.workspace_id,
            upload_id: input.upload_id,
            target_kind: input.target_kind,
            parent_folder_id: input.parent_folder_id,
            name: input.name,
            file_id: input.file_id,
            expected_root: input.expected_root,
            author_principal: input.author_principal,
            created_at_ms: input.created_at_ms,
            chunks: input.chunks,
        };
        session.validate()?;
        Ok(session)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    pub fn append_chunk(&mut self, chunk: DriveUploadChunk) -> Result<()> {
        let expected = u64::try_from(self.chunks.len())
            .map_err(|_| LoomError::invalid("drive upload chunk count overflow"))?;
        if chunk.sequence != expected {
            return Err(LoomError::invalid(
                "drive upload chunk sequence is not next",
            ));
        }
        self.chunks.push(chunk);
        self.validate()
    }

    pub fn total_size(&self) -> Result<u64> {
        self.chunks.iter().try_fold(0u64, |total, chunk| {
            total
                .checked_add(chunk.size)
                .ok_or_else(|| LoomError::invalid("drive upload size overflow"))
        })
    }

    fn validate(&self) -> Result<()> {
        validate_text("workspace_id", &self.workspace_id)?;
        validate_text("drive upload_id", &self.upload_id)?;
        validate_text("drive parent_folder_id", &self.parent_folder_id)?;
        validate_drive_name(&self.name)?;
        validate_text("drive file_id", &self.file_id)?;
        for (expected, chunk) in self.chunks.iter().enumerate() {
            let expected = u64::try_from(expected)
                .map_err(|_| LoomError::invalid("drive upload chunk count overflow"))?;
            if chunk.sequence != expected {
                return Err(LoomError::invalid("drive upload chunks must be contiguous"));
            }
        }
        Ok(())
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(UPLOAD_SESSION_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.workspace_id.clone()),
                Value::Text(self.upload_id.clone()),
                Value::Uint(self.target_kind.tag()),
                Value::Text(self.parent_folder_id.clone()),
                Value::Text(self.name.clone()),
                Value::Text(self.file_id.clone()),
                digest_value(self.expected_root),
                Value::Text(self.author_principal.to_string()),
                Value::Uint(self.created_at_ms),
                Value::Array(self.chunks.iter().map(DriveUploadChunk::to_value).collect()),
            ]),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "drive upload session")?;
        outer.expect_text(UPLOAD_SESSION_SCHEMA)?;
        let mut fields = Fields::array(
            outer.next("drive upload session fields")?,
            "drive upload session",
        )?;
        outer.end("drive upload session")?;
        let workspace_id = fields.text("workspace_id")?;
        let upload_id = fields.text("upload_id")?;
        let target_kind = DriveUploadTargetKind::from_tag(fields.uint("target_kind")?)?;
        let parent_folder_id = fields.text("parent_folder_id")?;
        let name = fields.text("name")?;
        let file_id = fields.text("file_id")?;
        let expected_root = fields.digest("expected_root")?;
        let author_principal = fields.id("author_principal")?;
        let created_at_ms = fields.uint("created_at_ms")?;
        let chunks = upload_chunk_list(fields.next("chunks")?)?;
        fields.end("drive upload session")?;
        Self::new(DriveUploadSessionInput {
            workspace_id,
            upload_id,
            target_kind,
            parent_folder_id,
            name,
            file_id,
            expected_root,
            author_principal,
            created_at_ms,
            chunks,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveUploadSessionInput {
    pub workspace_id: String,
    pub upload_id: String,
    pub target_kind: DriveUploadTargetKind,
    pub parent_folder_id: String,
    pub name: String,
    pub file_id: String,
    pub expected_root: Digest,
    pub author_principal: WorkspaceId,
    pub created_at_ms: u64,
    pub chunks: Vec<DriveUploadChunk>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveOperationRecord {
    pub sequence: u64,
    pub operation_id: String,
    pub operation_kind: String,
    pub target_entity_id: Option<String>,
    pub root_after: Digest,
    pub envelope: Vec<u8>,
}

impl DriveOperationRecord {
    pub fn new(
        sequence: u64,
        operation_id: impl Into<String>,
        operation_kind: impl Into<String>,
        target_entity_id: Option<String>,
        root_after: Digest,
        envelope: Vec<u8>,
    ) -> Result<Self> {
        let record = Self {
            sequence,
            operation_id: operation_id.into(),
            operation_kind: operation_kind.into(),
            target_entity_id,
            root_after,
            envelope,
        };
        record.validate()?;
        Ok(record)
    }

    fn validate(&self) -> Result<()> {
        validate_text("drive operation_id", &self.operation_id)?;
        validate_text("drive operation_kind", &self.operation_kind)?;
        if let Some(target) = &self.target_entity_id {
            validate_text("drive operation target", target)?;
        }
        if self.envelope.is_empty() {
            return Err(LoomError::invalid(
                "drive operation envelope must not be empty",
            ));
        }
        Ok(())
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Uint(self.sequence),
            Value::Text(self.operation_id.clone()),
            Value::Text(self.operation_kind.clone()),
            optional_text_value(self.target_entity_id.as_deref()),
            digest_value(self.root_after),
            Value::Bytes(self.envelope.clone()),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut fields = Fields::array(value, "drive operation record")?;
        let sequence = fields.uint("sequence")?;
        let operation_id = fields.text("operation_id")?;
        let operation_kind = fields.text("operation_kind")?;
        let target_entity_id = read_optional_text(&mut fields, "target_entity_id")?;
        let root_after = fields.digest("root_after")?;
        let envelope = fields.bytes("envelope")?;
        fields.end("drive operation record")?;
        Self::new(
            sequence,
            operation_id,
            operation_kind,
            target_entity_id,
            root_after,
            envelope,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveOperationLog {
    pub workspace_id: String,
    pub records: Vec<DriveOperationRecord>,
}

impl DriveOperationLog {
    pub fn new(
        workspace_id: impl Into<String>,
        records: Vec<DriveOperationRecord>,
    ) -> Result<Self> {
        let log = Self {
            workspace_id: workspace_id.into(),
            records,
        };
        log.validate()?;
        Ok(log)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    pub fn append(&mut self, record: DriveOperationRecord) -> Result<()> {
        self.records.push(record);
        self.validate()
    }

    pub fn next_sequence(&self) -> u64 {
        self.records.last().map_or(1, |record| record.sequence + 1)
    }

    fn validate(&self) -> Result<()> {
        validate_text("workspace_id", &self.workspace_id)?;
        let mut previous = None;
        let mut ids = BTreeSet::new();
        for record in &self.records {
            record.validate()?;
            if let Some(prev) = previous
                && record.sequence <= prev
            {
                return Err(LoomError::invalid(
                    "drive operation sequences must increase",
                ));
            }
            if !ids.insert(record.operation_id.clone()) {
                return Err(LoomError::invalid("drive operation ids must be unique"));
            }
            previous = Some(record.sequence);
        }
        Ok(())
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(OPERATION_LOG_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.workspace_id.clone()),
                Value::Array(
                    self.records
                        .iter()
                        .map(DriveOperationRecord::to_value)
                        .collect(),
                ),
            ]),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "drive operation log")?;
        outer.expect_text(OPERATION_LOG_SCHEMA)?;
        let mut fields = Fields::array(
            outer.next("drive operation log fields")?,
            "drive operation log",
        )?;
        outer.end("drive operation log")?;
        let workspace_id = fields.text("workspace_id")?;
        let records = operation_record_list(fields.next("records")?)?;
        fields.end("drive operation log")?;
        Self::new(workspace_id, records)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriveConflictResolution {
    Open,
    KeepCurrent,
    KeepConflict,
    KeepBoth,
}

impl DriveConflictResolution {
    const fn tag(&self) -> u64 {
        match self {
            Self::Open => 0,
            Self::KeepCurrent => 1,
            Self::KeepConflict => 2,
            Self::KeepBoth => 3,
        }
    }

    fn from_tag(tag: u64) -> Result<Self> {
        match tag {
            0 => Ok(Self::Open),
            1 => Ok(Self::KeepCurrent),
            2 => Ok(Self::KeepConflict),
            3 => Ok(Self::KeepBoth),
            other => Err(LoomError::corrupt(format!(
                "unknown drive conflict resolution tag {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveConflictRecord {
    pub conflict_id: String,
    pub folder_id: String,
    pub visible_node_id: String,
    pub conflict_node_id: String,
    pub conflict_name: String,
    pub base_root: Digest,
    pub resolution: DriveConflictResolution,
}

impl DriveConflictRecord {
    pub fn new(
        conflict_id: impl Into<String>,
        folder_id: impl Into<String>,
        visible_node_id: impl Into<String>,
        conflict_node_id: impl Into<String>,
        conflict_name: impl Into<String>,
        base_root: Digest,
        resolution: DriveConflictResolution,
    ) -> Result<Self> {
        let record = Self {
            conflict_id: conflict_id.into(),
            folder_id: folder_id.into(),
            visible_node_id: visible_node_id.into(),
            conflict_node_id: conflict_node_id.into(),
            conflict_name: conflict_name.into(),
            base_root,
            resolution,
        };
        record.validate()?;
        Ok(record)
    }

    fn validate(&self) -> Result<()> {
        validate_text("drive conflict_id", &self.conflict_id)?;
        validate_text("drive folder_id", &self.folder_id)?;
        validate_text("drive visible_node_id", &self.visible_node_id)?;
        validate_text("drive conflict_node_id", &self.conflict_node_id)?;
        validate_drive_name(&self.conflict_name)
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(self.conflict_id.clone()),
            Value::Text(self.folder_id.clone()),
            Value::Text(self.visible_node_id.clone()),
            Value::Text(self.conflict_node_id.clone()),
            Value::Text(self.conflict_name.clone()),
            digest_value(self.base_root),
            Value::Uint(self.resolution.tag()),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut fields = Fields::array(value, "drive conflict record")?;
        let conflict_id = fields.text("conflict_id")?;
        let folder_id = fields.text("folder_id")?;
        let visible_node_id = fields.text("visible_node_id")?;
        let conflict_node_id = fields.text("conflict_node_id")?;
        let conflict_name = fields.text("conflict_name")?;
        let base_root = fields.digest("base_root")?;
        let resolution = DriveConflictResolution::from_tag(fields.uint("resolution")?)?;
        fields.end("drive conflict record")?;
        Self::new(
            conflict_id,
            folder_id,
            visible_node_id,
            conflict_node_id,
            conflict_name,
            base_root,
            resolution,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveConflictIndex {
    pub workspace_id: String,
    pub conflicts: Vec<DriveConflictRecord>,
}

impl DriveConflictIndex {
    pub fn new(
        workspace_id: impl Into<String>,
        conflicts: Vec<DriveConflictRecord>,
    ) -> Result<Self> {
        let index = Self {
            workspace_id: workspace_id.into(),
            conflicts,
        };
        index.validate()?;
        Ok(index)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    pub fn append(&mut self, record: DriveConflictRecord) -> Result<()> {
        self.conflicts.push(record);
        self.validate()
    }

    fn validate(&self) -> Result<()> {
        validate_text("workspace_id", &self.workspace_id)?;
        let mut ids = BTreeSet::new();
        for conflict in &self.conflicts {
            conflict.validate()?;
            if !ids.insert(conflict.conflict_id.clone()) {
                return Err(LoomError::invalid("drive conflict ids must be unique"));
            }
        }
        Ok(())
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(CONFLICT_INDEX_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.workspace_id.clone()),
                Value::Array(
                    self.conflicts
                        .iter()
                        .map(DriveConflictRecord::to_value)
                        .collect(),
                ),
            ]),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "drive conflict index")?;
        outer.expect_text(CONFLICT_INDEX_SCHEMA)?;
        let mut fields = Fields::array(
            outer.next("drive conflict index fields")?,
            "drive conflict index",
        )?;
        outer.end("drive conflict index")?;
        let workspace_id = fields.text("workspace_id")?;
        let conflicts = conflict_record_list(fields.next("conflicts")?)?;
        fields.end("drive conflict index")?;
        Self::new(workspace_id, conflicts)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriveNodeKind {
    File,
    Folder,
    Shortcut,
}

impl DriveNodeKind {
    const fn tag(self) -> u64 {
        match self {
            Self::File => 0,
            Self::Folder => 1,
            Self::Shortcut => 2,
        }
    }

    fn from_tag(tag: u64) -> Result<Self> {
        match tag {
            0 => Ok(Self::File),
            1 => Ok(Self::Folder),
            2 => Ok(Self::Shortcut),
            other => Err(LoomError::corrupt(format!(
                "unknown drive node kind tag {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveFolderEntry {
    pub name: String,
    pub fold_key: String,
    pub node_id: String,
    pub kind: DriveNodeKind,
}

impl DriveFolderEntry {
    pub fn new(
        name: impl Into<String>,
        node_id: impl Into<String>,
        kind: DriveNodeKind,
    ) -> Result<Self> {
        let name = name.into();
        let node_id = node_id.into();
        validate_drive_name(&name)?;
        validate_text("drive node_id", &node_id)?;
        Ok(Self {
            fold_key: drive_fold_key(&name)?,
            name,
            node_id,
            kind,
        })
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(self.name.clone()),
            Value::Text(self.fold_key.clone()),
            Value::Text(self.node_id.clone()),
            Value::Uint(self.kind.tag()),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut fields = Fields::array(value, "drive folder entry")?;
        let name = fields.text("name")?;
        let fold_key = fields.text("fold_key")?;
        let node_id = fields.text("node_id")?;
        let kind = DriveNodeKind::from_tag(fields.uint("kind")?)?;
        fields.end("drive folder entry")?;
        let entry = Self::new(name, node_id, kind)?;
        if entry.fold_key != fold_key {
            return Err(LoomError::corrupt("drive folder entry fold key mismatch"));
        }
        Ok(entry)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveFolderChildren {
    pub folder_id: String,
    pub entries: Vec<DriveFolderEntry>,
}

impl DriveFolderChildren {
    pub fn new(folder_id: impl Into<String>, entries: Vec<DriveFolderEntry>) -> Result<Self> {
        let children = Self {
            folder_id: folder_id.into(),
            entries,
        };
        children.validate()?;
        Ok(children)
    }

    pub fn entry_by_name(&self, name: &str) -> Result<Option<&DriveFolderEntry>> {
        let fold_key = drive_fold_key(name)?;
        Ok(self.entries.iter().find(|entry| entry.fold_key == fold_key))
    }

    fn validate(&self) -> Result<()> {
        validate_text("drive folder_id", &self.folder_id)?;
        let mut keys = BTreeSet::new();
        let mut node_ids = BTreeSet::new();
        for entry in &self.entries {
            validate_drive_name(&entry.name)?;
            validate_text("drive node_id", &entry.node_id)?;
            if entry.fold_key != drive_fold_key(&entry.name)? {
                return Err(LoomError::invalid("drive folder entry fold key mismatch"));
            }
            if !keys.insert(entry.fold_key.clone()) {
                return Err(LoomError::new(
                    Code::AlreadyExists,
                    "drive folder entry name collision",
                ));
            }
            if !node_ids.insert(entry.node_id.clone()) {
                return Err(LoomError::invalid(
                    "drive folder children node ids must be unique",
                ));
            }
        }
        Ok(())
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(self.folder_id.clone()),
            Value::Array(
                self.entries
                    .iter()
                    .map(DriveFolderEntry::to_value)
                    .collect(),
            ),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut fields = Fields::array(value, "drive folder children")?;
        let folder_id = fields.text("folder_id")?;
        let entries = folder_entry_list(fields.next("entries")?)?;
        fields.end("drive folder children")?;
        Self::new(folder_id, entries)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveFolderIndex {
    pub workspace_id: String,
    pub folders: Vec<DriveFolderChildren>,
}

impl DriveFolderIndex {
    pub fn new(workspace_id: impl Into<String>, folders: Vec<DriveFolderChildren>) -> Result<Self> {
        let index = Self {
            workspace_id: workspace_id.into(),
            folders,
        };
        index.validate()?;
        Ok(index)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    pub fn children(&self, folder_id: &str) -> Option<&DriveFolderChildren> {
        self.folders
            .iter()
            .find(|children| children.folder_id == folder_id)
    }

    fn validate(&self) -> Result<()> {
        validate_text("workspace_id", &self.workspace_id)?;
        let mut folder_ids = BTreeSet::new();
        for folder in &self.folders {
            folder.validate()?;
            if !folder_ids.insert(folder.folder_id.clone()) {
                return Err(LoomError::invalid("drive folder ids must be unique"));
            }
        }
        Ok(())
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(FOLDER_INDEX_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.workspace_id.clone()),
                Value::Array(
                    self.folders
                        .iter()
                        .map(DriveFolderChildren::to_value)
                        .collect(),
                ),
            ]),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "drive folder index")?;
        outer.expect_text(FOLDER_INDEX_SCHEMA)?;
        let mut fields = Fields::array(
            outer.next("drive folder index fields")?,
            "drive folder index",
        )?;
        outer.end("drive folder index")?;
        let workspace_id = fields.text("workspace_id")?;
        let folders = folder_children_list(fields.next("folders")?)?;
        fields.end("drive folder index")?;
        Self::new(workspace_id, folders)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveChunkRef {
    pub digest: Digest,
    pub size: u64,
}

impl DriveChunkRef {
    pub fn new(digest: Digest, size: u64) -> Result<Self> {
        if size == 0 || size > CHUNK_MAX_SIZE {
            return Err(LoomError::invalid("drive chunk size is out of range"));
        }
        Ok(Self { digest, size })
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(self.digest.to_string()),
            Value::Uint(self.size),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut fields = Fields::array(value, "drive chunk ref")?;
        let digest = fields.digest("digest")?;
        let size = fields.uint("size")?;
        fields.end("drive chunk ref")?;
        Self::new(digest, size)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveChunkManifest {
    pub content_digest: Digest,
    pub total_size: u64,
    pub chunks: Vec<DriveChunkRef>,
}

impl DriveChunkManifest {
    pub fn new(
        content_digest: Digest,
        total_size: u64,
        chunks: Vec<DriveChunkRef>,
    ) -> Result<Self> {
        let manifest = Self {
            content_digest,
            total_size,
            chunks,
        };
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    fn validate(&self) -> Result<()> {
        if self.total_size <= CHUNK_MIN_SIZE {
            return Err(LoomError::invalid(
                "drive chunk manifest is only for chunked content",
            ));
        }
        if self.chunks.is_empty() {
            return Err(LoomError::invalid("drive chunk manifest must not be empty"));
        }
        let mut total = 0u64;
        for (idx, chunk) in self.chunks.iter().enumerate() {
            if idx + 1 < self.chunks.len() && chunk.size < CHUNK_MIN_SIZE {
                return Err(LoomError::invalid("drive non-final chunk is too small"));
            }
            total = total
                .checked_add(chunk.size)
                .ok_or_else(|| LoomError::invalid("drive chunk manifest size overflow"))?;
        }
        if total != self.total_size {
            return Err(LoomError::invalid(
                "drive chunk manifest total size mismatch",
            ));
        }
        Ok(())
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(CHUNK_MANIFEST_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.content_digest.to_string()),
                Value::Uint(self.total_size),
                Value::Array(self.chunks.iter().map(DriveChunkRef::to_value).collect()),
            ]),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "drive chunk manifest")?;
        outer.expect_text(CHUNK_MANIFEST_SCHEMA)?;
        let mut fields = Fields::array(
            outer.next("drive chunk manifest fields")?,
            "drive chunk manifest",
        )?;
        outer.end("drive chunk manifest")?;
        let content_digest = fields.digest("content_digest")?;
        let total_size = fields.uint("total_size")?;
        let chunks = chunk_ref_list(fields.next("chunks")?)?;
        fields.end("drive chunk manifest")?;
        Self::new(content_digest, total_size, chunks)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriveContentRef {
    Blob {
        digest: Digest,
        size: u64,
    },
    Manifest {
        manifest_digest: Digest,
        content_digest: Digest,
        size: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveDehydratedFileMarker {
    pub file_id: String,
    pub size: u64,
    pub content_digest: Digest,
    pub uri: String,
}

impl DriveDehydratedFileMarker {
    pub fn new(
        file_id: impl Into<String>,
        size: u64,
        content_digest: Digest,
        uri: impl Into<String>,
    ) -> Result<Self> {
        let marker = Self {
            file_id: file_id.into(),
            size,
            content_digest,
            uri: uri.into(),
        };
        marker.validate()?;
        Ok(marker)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        let mut out = DEHYDRATED_FILE_MARKER_MAGIC.to_vec();
        out.extend(
            loom_codec::encode(&Value::Array(vec![
                Value::Uint(DEHYDRATED_FILE_MARKER_VERSION),
                Value::Text(self.file_id.clone()),
                Value::Uint(self.size),
                digest_value(self.content_digest),
                Value::Text(self.uri.clone()),
            ]))
            .map_err(codec_error)?,
        );
        Ok(out)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        if !bytes.starts_with(DEHYDRATED_FILE_MARKER_MAGIC) {
            return Err(LoomError::invalid(
                "drive dehydrated marker magic is missing",
            ));
        }
        let payload = &bytes[DEHYDRATED_FILE_MARKER_MAGIC.len()..];
        let mut fields = Fields::array(
            loom_codec::decode(payload).map_err(codec_error)?,
            "drive dehydrated marker",
        )?;
        let version = fields.uint("version")?;
        if version != DEHYDRATED_FILE_MARKER_VERSION {
            return Err(LoomError::invalid(
                "unsupported drive dehydrated marker version",
            ));
        }
        let file_id = fields.text("file_id")?;
        let size = fields.uint("size")?;
        let content_digest = fields.digest("content_digest")?;
        let uri = fields.text("uri")?;
        fields.end("drive dehydrated marker")?;
        Self::new(file_id, size, content_digest, uri)
    }

    fn validate(&self) -> Result<()> {
        validate_text("drive file_id", &self.file_id)?;
        if self.size == 0 {
            return Err(LoomError::invalid(
                "drive dehydrated marker size must be positive",
            ));
        }
        if !self.uri.starts_with("loom://") {
            return Err(LoomError::invalid(
                "drive dehydrated marker uri must be a loom URI",
            ));
        }
        if self.uri.chars().any(|ch| ch == '\0' || ch.is_control()) {
            return Err(LoomError::invalid(
                "drive dehydrated marker uri must not contain NUL or control characters",
            ));
        }
        Ok(())
    }
}

pub fn is_drive_dehydrated_file_marker(bytes: &[u8]) -> bool {
    bytes.starts_with(DEHYDRATED_FILE_MARKER_MAGIC)
}

impl DriveContentRef {
    fn to_value(&self) -> Value {
        match self {
            Self::Blob { digest, size } => Value::Array(vec![
                Value::Uint(0),
                Value::Text(digest.to_string()),
                Value::Uint(*size),
            ]),
            Self::Manifest {
                manifest_digest,
                content_digest,
                size,
            } => Value::Array(vec![
                Value::Uint(1),
                Value::Text(manifest_digest.to_string()),
                Value::Text(content_digest.to_string()),
                Value::Uint(*size),
            ]),
        }
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut fields = Fields::array(value, "drive content ref")?;
        let content = match fields.uint("content ref tag")? {
            0 => Self::Blob {
                digest: fields.digest("digest")?,
                size: fields.uint("size")?,
            },
            1 => Self::Manifest {
                manifest_digest: fields.digest("manifest_digest")?,
                content_digest: fields.digest("content_digest")?,
                size: fields.uint("size")?,
            },
            other => {
                return Err(LoomError::corrupt(format!(
                    "unknown drive content ref tag {other}"
                )));
            }
        };
        fields.end("drive content ref")?;
        content.validate()?;
        Ok(content)
    }

    fn validate(&self) -> Result<()> {
        match self {
            Self::Blob { size, .. } if *size > CHUNK_MIN_SIZE => Err(LoomError::invalid(
                "drive blob content exceeds single-blob threshold",
            )),
            Self::Blob { size: 0, .. } => Err(LoomError::invalid("drive blob size is zero")),
            Self::Blob { .. } => Ok(()),
            Self::Manifest { size, .. } if *size <= CHUNK_MIN_SIZE => Err(LoomError::invalid(
                "drive manifest content is below chunk threshold",
            )),
            Self::Manifest { .. } => Ok(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveFileVersion {
    pub file_id: String,
    pub version: u64,
    pub operation_id: String,
    pub author_principal: WorkspaceId,
    pub timestamp_ms: u64,
    pub content: DriveContentRef,
}

impl DriveFileVersion {
    pub fn new(
        file_id: impl Into<String>,
        version: u64,
        operation_id: impl Into<String>,
        author_principal: WorkspaceId,
        timestamp_ms: u64,
        content: DriveContentRef,
    ) -> Result<Self> {
        let record = Self {
            file_id: file_id.into(),
            version,
            operation_id: operation_id.into(),
            author_principal,
            timestamp_ms,
            content,
        };
        record.validate()?;
        Ok(record)
    }

    fn validate(&self) -> Result<()> {
        validate_text("drive file_id", &self.file_id)?;
        validate_text("drive operation_id", &self.operation_id)?;
        if self.version == 0 {
            return Err(LoomError::invalid("drive file version must be positive"));
        }
        self.content.validate()
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(self.file_id.clone()),
            Value::Uint(self.version),
            Value::Text(self.operation_id.clone()),
            Value::Text(self.author_principal.to_string()),
            Value::Uint(self.timestamp_ms),
            self.content.to_value(),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut fields = Fields::array(value, "drive file version")?;
        let file_id = fields.text("file_id")?;
        let version = fields.uint("version")?;
        let operation_id = fields.text("operation_id")?;
        let author_principal = fields.id("author_principal")?;
        let timestamp_ms = fields.uint("timestamp_ms")?;
        let content = DriveContentRef::from_value(fields.next("content")?)?;
        fields.end("drive file version")?;
        Self::new(
            file_id,
            version,
            operation_id,
            author_principal,
            timestamp_ms,
            content,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveFileVersionIndex {
    pub workspace_id: String,
    pub versions: Vec<DriveFileVersion>,
}

impl DriveFileVersionIndex {
    pub fn new(workspace_id: impl Into<String>, versions: Vec<DriveFileVersion>) -> Result<Self> {
        let index = Self {
            workspace_id: workspace_id.into(),
            versions,
        };
        index.validate()?;
        Ok(index)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    pub fn latest(&self, file_id: &str) -> Option<&DriveFileVersion> {
        self.versions
            .iter()
            .filter(|version| version.file_id == file_id)
            .max_by_key(|version| version.version)
    }

    fn validate(&self) -> Result<()> {
        validate_text("workspace_id", &self.workspace_id)?;
        let mut seen = BTreeSet::new();
        let mut latest_by_file = BTreeMap::new();
        for version in &self.versions {
            version.validate()?;
            if !seen.insert((version.file_id.clone(), version.version)) {
                return Err(LoomError::invalid(
                    "drive file versions must be unique per file",
                ));
            }
            let latest = latest_by_file
                .entry(version.file_id.clone())
                .or_insert(version.version);
            if version.version > *latest {
                *latest = version.version;
            }
        }
        Ok(())
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(FILE_VERSION_INDEX_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.workspace_id.clone()),
                Value::Array(
                    self.versions
                        .iter()
                        .map(DriveFileVersion::to_value)
                        .collect(),
                ),
            ]),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "drive file version index")?;
        outer.expect_text(FILE_VERSION_INDEX_SCHEMA)?;
        let mut fields = Fields::array(
            outer.next("drive file version index fields")?,
            "drive file version index",
        )?;
        outer.end("drive file version index")?;
        let workspace_id = fields.text("workspace_id")?;
        let versions = file_version_list(fields.next("versions")?)?;
        fields.end("drive file version index")?;
        Self::new(workspace_id, versions)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveProfileSnapshot {
    pub workspace_id: String,
    pub folders: DriveFolderIndex,
    pub versions: DriveFileVersionIndex,
}

impl DriveProfileSnapshot {
    pub fn new(
        workspace_id: impl Into<String>,
        folders: DriveFolderIndex,
        versions: DriveFileVersionIndex,
    ) -> Result<Self> {
        let snapshot = Self {
            workspace_id: workspace_id.into(),
            folders,
            versions,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    fn validate(&self) -> Result<()> {
        validate_text("workspace_id", &self.workspace_id)?;
        if self.folders.workspace_id != self.workspace_id
            || self.versions.workspace_id != self.workspace_id
        {
            return Err(LoomError::invalid(
                "drive snapshot component drive ids must match",
            ));
        }
        Ok(())
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(PROFILE_SNAPSHOT_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.workspace_id.clone()),
                self.folders.to_value(),
                self.versions.to_value(),
            ]),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "drive profile snapshot")?;
        outer.expect_text(PROFILE_SNAPSHOT_SCHEMA)?;
        let mut fields = Fields::array(
            outer.next("drive profile snapshot fields")?,
            "drive profile snapshot",
        )?;
        outer.end("drive profile snapshot")?;
        let workspace_id = fields.text("workspace_id")?;
        let folders = DriveFolderIndex::from_value(fields.next("folders")?)?;
        let versions = DriveFileVersionIndex::from_value(fields.next("versions")?)?;
        fields.end("drive profile snapshot")?;
        Self::new(workspace_id, folders, versions)
    }
}

pub fn drive_fold_key(name: &str) -> Result<String> {
    validate_drive_name(name)?;
    Ok(name.nfc().flat_map(char::to_lowercase).collect())
}

pub fn drive_profile_key(workspace_id: &str) -> Result<Vec<u8>> {
    validate_text("workspace_id", workspace_id)?;
    Ok(format!("{PROFILE_CONTROL_PREFIX}/{workspace_id}/snapshot").into_bytes())
}

pub fn drive_operation_log_key(workspace_id: &str) -> Result<Vec<u8>> {
    validate_text("workspace_id", workspace_id)?;
    Ok(format!("{PROFILE_CONTROL_PREFIX}/{workspace_id}/operations").into_bytes())
}

pub fn drive_upload_session_key(workspace_id: &str, upload_id: &str) -> Result<Vec<u8>> {
    validate_text("workspace_id", workspace_id)?;
    validate_text("drive upload_id", upload_id)?;
    Ok(format!("{PROFILE_CONTROL_PREFIX}/{workspace_id}/uploads/{upload_id}").into_bytes())
}

pub fn drive_conflict_index_key(workspace_id: &str) -> Result<Vec<u8>> {
    validate_text("workspace_id", workspace_id)?;
    Ok(format!("{PROFILE_CONTROL_PREFIX}/{workspace_id}/conflicts").into_bytes())
}

pub fn drive_share_index_key(workspace_id: &str) -> Result<Vec<u8>> {
    validate_text("workspace_id", workspace_id)?;
    Ok(format!("{PROFILE_CONTROL_PREFIX}/{workspace_id}/shares").into_bytes())
}

pub fn drive_retention_index_key(workspace_id: &str) -> Result<Vec<u8>> {
    validate_text("workspace_id", workspace_id)?;
    Ok(format!("{PROFILE_CONTROL_PREFIX}/{workspace_id}/retention").into_bytes())
}

pub fn drive_policy_registry_key() -> Vec<u8> {
    format!("{PROFILE_CONTROL_PREFIX}/registry").into_bytes()
}

pub fn conflict_copy_name(
    original_name: &str,
    actor_display: &str,
    timestamp_ms: u64,
    collision_ordinal: u64,
) -> Result<String> {
    validate_drive_name(original_name)?;
    validate_text("actor_display", actor_display)?;
    if collision_ordinal == 0 {
        return Err(LoomError::invalid(
            "drive conflict collision ordinal must be positive",
        ));
    }
    let timestamp = i64::try_from(timestamp_ms)
        .map_err(|_| LoomError::invalid("drive conflict timestamp exceeds i64 range"))?;
    let date = Utc
        .timestamp_millis_opt(timestamp)
        .single()
        .ok_or_else(|| LoomError::invalid("drive conflict timestamp is out of range"))?
        .format("%Y-%m-%d");
    let actor = actor_display
        .chars()
        .map(|ch| if ch == '/' || ch == '\0' { '_' } else { ch })
        .collect::<String>();
    let (stem, ext) = split_extension(original_name);
    let ordinal = if collision_ordinal == 1 {
        String::new()
    } else {
        format!(" - {collision_ordinal}")
    };
    let name = format!("{stem} (conflicted copy of {actor}, {date}){ordinal}{ext}");
    validate_drive_name(&name)?;
    Ok(name)
}

fn validate_drive_name(name: &str) -> Result<()> {
    validate_text("drive name", name)?;
    if name.contains('/') || name.contains('\0') {
        return Err(LoomError::invalid(
            "drive name contains a forbidden character",
        ));
    }
    Ok(())
}

fn split_extension(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(0) | None => (name, ""),
        Some(index) => (&name[..index], &name[index..]),
    }
}

fn folder_entry_list(value: Value) -> Result<Vec<DriveFolderEntry>> {
    match value {
        Value::Array(items) => items
            .into_iter()
            .map(DriveFolderEntry::from_value)
            .collect(),
        _ => Err(LoomError::corrupt("drive folder entries must be an array")),
    }
}

fn folder_children_list(value: Value) -> Result<Vec<DriveFolderChildren>> {
    match value {
        Value::Array(items) => items
            .into_iter()
            .map(DriveFolderChildren::from_value)
            .collect(),
        _ => Err(LoomError::corrupt("drive folders must be an array")),
    }
}

fn chunk_ref_list(value: Value) -> Result<Vec<DriveChunkRef>> {
    match value {
        Value::Array(items) => items.into_iter().map(DriveChunkRef::from_value).collect(),
        _ => Err(LoomError::corrupt("drive chunks must be an array")),
    }
}

fn upload_chunk_list(value: Value) -> Result<Vec<DriveUploadChunk>> {
    match value {
        Value::Array(items) => items
            .into_iter()
            .map(DriveUploadChunk::from_value)
            .collect(),
        _ => Err(LoomError::corrupt("drive upload chunks must be an array")),
    }
}

fn operation_record_list(value: Value) -> Result<Vec<DriveOperationRecord>> {
    match value {
        Value::Array(items) => items
            .into_iter()
            .map(DriveOperationRecord::from_value)
            .collect(),
        _ => Err(LoomError::corrupt(
            "drive operation records must be an array",
        )),
    }
}

fn conflict_record_list(value: Value) -> Result<Vec<DriveConflictRecord>> {
    match value {
        Value::Array(items) => items
            .into_iter()
            .map(DriveConflictRecord::from_value)
            .collect(),
        _ => Err(LoomError::corrupt(
            "drive conflict records must be an array",
        )),
    }
}

fn share_grant_list(value: Value) -> Result<Vec<DriveShareGrant>> {
    match value {
        Value::Array(items) => items.into_iter().map(DriveShareGrant::from_value).collect(),
        _ => Err(LoomError::corrupt("drive share grants must be an array")),
    }
}

fn retention_pin_list(value: Value) -> Result<Vec<DriveRetentionPin>> {
    match value {
        Value::Array(items) => items
            .into_iter()
            .map(DriveRetentionPin::from_value)
            .collect(),
        _ => Err(LoomError::corrupt("drive retention pins must be an array")),
    }
}

fn policy_target_list(value: Value) -> Result<Vec<DrivePolicyTarget>> {
    match value {
        Value::Array(items) => items
            .into_iter()
            .map(DrivePolicyTarget::from_value)
            .collect(),
        _ => Err(LoomError::corrupt("drive policy targets must be an array")),
    }
}

fn file_version_list(value: Value) -> Result<Vec<DriveFileVersion>> {
    match value {
        Value::Array(items) => items
            .into_iter()
            .map(DriveFileVersion::from_value)
            .collect(),
        _ => Err(LoomError::corrupt("drive file versions must be an array")),
    }
}

fn optional_u64_value(value: Option<u64>) -> Value {
    match value {
        Some(value) => Value::Array(vec![Value::Bool(true), Value::Uint(value)]),
        None => Value::Array(vec![Value::Bool(false)]),
    }
}

fn read_optional_u64(fields: &mut Fields, name: &str) -> Result<Option<u64>> {
    let mut optional = Fields::array(fields.next(name)?, name)?;
    let present = optional.bool("present")?;
    let value = if present {
        Some(optional.uint("value")?)
    } else {
        None
    };
    optional.end(name)?;
    Ok(value)
}

fn digest_value(digest: Digest) -> Value {
    Value::Text(digest.to_string())
}

fn optional_text_value(value: Option<&str>) -> Value {
    match value {
        Some(value) => Value::Array(vec![Value::Bool(true), Value::Text(value.to_string())]),
        None => Value::Array(vec![Value::Bool(false)]),
    }
}

fn read_optional_text(fields: &mut Fields, name: &str) -> Result<Option<String>> {
    let mut optional = Fields::array(fields.next(name)?, name)?;
    let present = optional.bool("present")?;
    let value = if present {
        Some(optional.text("value")?)
    } else {
        None
    };
    optional.end(name)?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use loom_types::Algo;

    use super::*;

    fn digest(byte: u8) -> Digest {
        Digest::hash(Algo::Blake3, &[byte])
    }

    fn principal(byte: u8) -> WorkspaceId {
        WorkspaceId::v4_from_bytes([byte; 16])
    }

    #[test]
    fn folder_index_rejects_fold_equal_siblings() {
        let cafe_nfc =
            DriveFolderEntry::new("CAF\u{00C9}.txt", "file-1", DriveNodeKind::File).unwrap();
        let cafe_nfd =
            DriveFolderEntry::new("cafe\u{0301}.txt", "file-2", DriveNodeKind::File).unwrap();

        assert_eq!(cafe_nfc.fold_key, cafe_nfd.fold_key);
        assert_eq!(
            DriveFolderChildren::new("root", vec![cafe_nfc, cafe_nfd])
                .unwrap_err()
                .code,
            loom_types::Code::AlreadyExists
        );
    }

    #[test]
    fn folder_index_round_trips_and_resolves_by_fold_key() {
        let index = DriveFolderIndex::new(
            "main",
            vec![
                DriveFolderChildren::new(
                    "root",
                    vec![
                        DriveFolderEntry::new("Specs", "folder-1", DriveNodeKind::Folder).unwrap(),
                        DriveFolderEntry::new("Budget.xlsx", "file-1", DriveNodeKind::File)
                            .unwrap(),
                    ],
                )
                .unwrap(),
            ],
        )
        .unwrap();

        let decoded = DriveFolderIndex::decode(&index.encode().unwrap()).unwrap();
        assert_eq!(decoded, index);
        let root = decoded.children("root").unwrap();
        assert_eq!(
            root.entry_by_name("budget.xlsx").unwrap().unwrap().node_id,
            "file-1"
        );
    }

    #[test]
    fn chunk_manifest_enforces_drive_thresholds_and_round_trips() {
        let manifest = DriveChunkManifest::new(
            digest(9),
            CHUNK_MAX_SIZE + CHUNK_MIN_SIZE,
            vec![
                DriveChunkRef::new(digest(1), CHUNK_MAX_SIZE).unwrap(),
                DriveChunkRef::new(digest(2), CHUNK_MIN_SIZE).unwrap(),
            ],
        )
        .unwrap();

        assert_eq!(
            DriveChunkManifest::decode(&manifest.encode().unwrap()).unwrap(),
            manifest
        );
        assert!(DriveChunkManifest::new(digest(9), CHUNK_MIN_SIZE, vec![]).is_err());
    }

    #[test]
    fn version_index_tracks_latest_content_and_snapshot_round_trips() {
        let versions = DriveFileVersionIndex::new(
            "main",
            vec![
                DriveFileVersion::new(
                    "file-1",
                    1,
                    "op-1",
                    principal(1),
                    100,
                    DriveContentRef::Blob {
                        digest: digest(1),
                        size: CHUNK_MIN_SIZE,
                    },
                )
                .unwrap(),
                DriveFileVersion::new(
                    "file-1",
                    2,
                    "op-2",
                    principal(2),
                    200,
                    DriveContentRef::Manifest {
                        manifest_digest: digest(3),
                        content_digest: digest(4),
                        size: CHUNK_MIN_SIZE + 1,
                    },
                )
                .unwrap(),
            ],
        )
        .unwrap();
        assert_eq!(versions.latest("file-1").unwrap().version, 2);
        let folders = DriveFolderIndex::new(
            "main",
            vec![
                DriveFolderChildren::new(
                    "root",
                    vec![
                        DriveFolderEntry::new("Budget.xlsx", "file-1", DriveNodeKind::File)
                            .unwrap(),
                    ],
                )
                .unwrap(),
            ],
        )
        .unwrap();
        let snapshot = DriveProfileSnapshot::new("main", folders, versions).unwrap();

        assert_eq!(
            DriveProfileSnapshot::decode(&snapshot.encode().unwrap()).unwrap(),
            snapshot
        );
        assert_eq!(
            drive_profile_key("main").unwrap(),
            b"profile/drive/v1/main/snapshot".to_vec()
        );
        assert_eq!(
            drive_operation_log_key("main").unwrap(),
            b"profile/drive/v1/main/operations".to_vec()
        );
        assert_eq!(
            drive_upload_session_key("main", "upload-1").unwrap(),
            b"profile/drive/v1/main/uploads/upload-1".to_vec()
        );
        assert_eq!(
            drive_conflict_index_key("main").unwrap(),
            b"profile/drive/v1/main/conflicts".to_vec()
        );
    }

    #[test]
    fn upload_sessions_operation_logs_and_conflicts_round_trip() {
        let mut session = DriveUploadSession::new(DriveUploadSessionInput {
            workspace_id: "main".to_string(),
            upload_id: "upload-1".to_string(),
            target_kind: DriveUploadTargetKind::NewFile,
            parent_folder_id: "root".to_string(),
            name: "Budget.xlsx".to_string(),
            file_id: "file-1".to_string(),
            expected_root: digest(9),
            author_principal: principal(1),
            created_at_ms: 100,
            chunks: Vec::new(),
        })
        .unwrap();
        session
            .append_chunk(DriveUploadChunk::new(0, digest(1), 100).unwrap())
            .unwrap();
        session
            .append_chunk(DriveUploadChunk::new(1, digest(2), 200).unwrap())
            .unwrap();
        assert_eq!(session.total_size().unwrap(), 300);
        assert_eq!(
            DriveUploadSession::decode(&session.encode().unwrap()).unwrap(),
            session
        );
        assert!(
            session
                .append_chunk(DriveUploadChunk::new(3, digest(3), 1).unwrap())
                .is_err()
        );

        let mut log = DriveOperationLog::new("main", Vec::new()).unwrap();
        log.append(
            DriveOperationRecord::new(
                log.next_sequence(),
                "op-1",
                "folder.created",
                Some("folder-1".to_string()),
                digest(4),
                b"envelope".to_vec(),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            DriveOperationLog::decode(&log.encode().unwrap()).unwrap(),
            log
        );

        let mut conflicts = DriveConflictIndex::new("main", Vec::new()).unwrap();
        conflicts
            .append(
                DriveConflictRecord::new(
                    "conflict-1",
                    "root",
                    "file-1",
                    "file-2",
                    "Budget (conflicted copy of Dana, 2024-01-01).xlsx",
                    digest(5),
                    DriveConflictResolution::Open,
                )
                .unwrap(),
            )
            .unwrap();
        assert_eq!(
            DriveConflictIndex::decode(&conflicts.encode().unwrap()).unwrap(),
            conflicts
        );
    }

    #[test]
    fn share_and_retention_indexes_round_trip() {
        let share = DriveShareGrant::new(DriveShareGrantInput {
            grant_id: "grant-1".to_string(),
            target_kind: DriveShareTargetKind::Folder,
            target_id: "folder-1".to_string(),
            principal: principal(2),
            role: DriveShareRole::Editor,
            granted_by: principal(1),
            granted_at_ms: 100,
            expires_at_ms: Some(200),
        })
        .unwrap();
        let shares = DriveShareIndex::new("main", vec![share.clone()]).unwrap();
        assert_eq!(
            DriveShareIndex::decode(&shares.encode().unwrap()).unwrap(),
            shares
        );
        assert_eq!(
            drive_share_index_key("main").unwrap(),
            b"profile/drive/v1/main/shares".to_vec()
        );
        assert!(
            DriveShareGrant::new(DriveShareGrantInput {
                expires_at_ms: Some(100),
                ..DriveShareGrantInput {
                    grant_id: "bad".to_string(),
                    target_kind: DriveShareTargetKind::File,
                    target_id: "file-1".to_string(),
                    principal: principal(2),
                    role: DriveShareRole::Viewer,
                    granted_by: principal(1),
                    granted_at_ms: 100,
                    expires_at_ms: None,
                }
            })
            .is_err()
        );
        assert!(DriveShareIndex::new("main", vec![share.clone(), share]).is_err());

        let current = DriveRetentionPin::new(DriveRetentionPinInput {
            pin_id: "current".to_string(),
            kind: DriveRetentionPinKind::CurrentRoot,
            root: digest(1),
            target_entity_id: None,
            added_by: principal(1),
            added_at_ms: 100,
            expires_at_ms: None,
        })
        .unwrap();
        let trash = DriveRetentionPin::new(DriveRetentionPinInput {
            pin_id: "trash-1".to_string(),
            kind: DriveRetentionPinKind::TrashSubtree,
            root: digest(2),
            target_entity_id: Some("file:file-1".to_string()),
            added_by: principal(1),
            added_at_ms: 100,
            expires_at_ms: Some(1_000),
        })
        .unwrap();
        let retention = DriveRetentionIndex::new("main", vec![current, trash]).unwrap();
        assert_eq!(
            DriveRetentionIndex::decode(&retention.encode().unwrap()).unwrap(),
            retention
        );
        assert!(retention.live_roots().contains(&digest(1)));
        assert_eq!(
            drive_retention_index_key("main").unwrap(),
            b"profile/drive/v1/main/retention".to_vec()
        );
        let mut registry = DrivePolicyRegistry::empty();
        registry
            .upsert_enabled(DrivePolicyTarget::new(principal(3), "main", true).unwrap())
            .unwrap();
        assert_eq!(
            DrivePolicyRegistry::decode(&registry.encode().unwrap()).unwrap(),
            registry
        );
        assert_eq!(
            registry
                .enabled_targets()
                .next()
                .unwrap()
                .workspace_id
                .as_str(),
            "main"
        );
        assert_eq!(
            drive_policy_registry_key(),
            b"profile/drive/v1/registry".to_vec()
        );
        assert!(
            DrivePolicyRegistry::new(vec![
                DrivePolicyTarget::new(principal(3), "main", true).unwrap(),
                DrivePolicyTarget::new(principal(3), "main", false).unwrap(),
            ])
            .is_err()
        );
        assert!(
            DriveRetentionPin::new(DriveRetentionPinInput {
                pin_id: "hold".to_string(),
                kind: DriveRetentionPinKind::LegalHold,
                root: digest(3),
                target_entity_id: Some("file:file-1".to_string()),
                added_by: principal(1),
                added_at_ms: 100,
                expires_at_ms: Some(1_000),
            })
            .is_err()
        );
    }

    #[test]
    fn dehydrated_file_marker_has_pinned_magic_and_round_trips() {
        let marker = DriveDehydratedFileMarker::new(
            "file-1",
            42,
            digest(9),
            "loom://main/drive/main/files/file-1",
        )
        .unwrap();
        let bytes = marker.encode().unwrap();
        assert!(bytes.starts_with(DEHYDRATED_FILE_MARKER_MAGIC));
        assert!(is_drive_dehydrated_file_marker(&bytes));
        assert_eq!(DriveDehydratedFileMarker::decode(&bytes).unwrap(), marker);
        assert!(DriveDehydratedFileMarker::decode(b"not-marker").is_err());
    }

    #[test]
    fn drive_merge_matrix_handles_create_collisions_and_conflict_names() {
        let different_names = drive_merge_outcome(
            &DriveConcurrentOperation::CreateFile {
                folder_id: "root".to_string(),
                name: "A.txt".to_string(),
                content_digest: digest(1),
                actor_display: "Dana".to_string(),
                timestamp_ms: 1_704_067_200_000,
            },
            &DriveConcurrentOperation::CreateFile {
                folder_id: "root".to_string(),
                name: "B.txt".to_string(),
                content_digest: digest(2),
                actor_display: "Dana".to_string(),
                timestamp_ms: 1_704_067_200_000,
            },
        )
        .unwrap();
        assert_eq!(different_names, DriveMergeOutcome::Merge);

        let same_content = drive_merge_outcome(
            &DriveConcurrentOperation::CreateFile {
                folder_id: "root".to_string(),
                name: "Budget.xlsx".to_string(),
                content_digest: digest(1),
                actor_display: "Dana".to_string(),
                timestamp_ms: 1_704_067_200_000,
            },
            &DriveConcurrentOperation::CreateFile {
                folder_id: "root".to_string(),
                name: "budget.xlsx".to_string(),
                content_digest: digest(1),
                actor_display: "Dana".to_string(),
                timestamp_ms: 1_704_067_200_000,
            },
        )
        .unwrap();
        assert_eq!(same_content, DriveMergeOutcome::Deduplicate);

        let conflicting = drive_merge_outcome(
            &DriveConcurrentOperation::CreateFile {
                folder_id: "root".to_string(),
                name: "Budget.xlsx".to_string(),
                content_digest: digest(1),
                actor_display: "Dana".to_string(),
                timestamp_ms: 1_704_067_200_000,
            },
            &DriveConcurrentOperation::CreateFile {
                folder_id: "root".to_string(),
                name: "budget.xlsx".to_string(),
                content_digest: digest(2),
                actor_display: "Dana".to_string(),
                timestamp_ms: 1_704_067_200_000,
            },
        )
        .unwrap();
        assert_eq!(
            conflicting,
            DriveMergeOutcome::ConflictCopy {
                name: "budget (conflicted copy of Dana, 2024-01-01).xlsx".to_string()
            }
        );
    }

    #[test]
    fn drive_merge_matrix_handles_metadata_conflicts_and_rejections() {
        assert_eq!(
            drive_merge_outcome(
                &DriveConcurrentOperation::Rename {
                    node_id: "file-1".to_string(),
                    new_name: "A.txt".to_string(),
                },
                &DriveConcurrentOperation::Rename {
                    node_id: "file-1".to_string(),
                    new_name: "B.txt".to_string(),
                },
            )
            .unwrap(),
            DriveMergeOutcome::ConflictRecord {
                field_or_region: "name".to_string()
            }
        );
        assert_eq!(
            drive_merge_outcome(
                &DriveConcurrentOperation::Move {
                    node_id: "file-1".to_string(),
                    target_folder_id: "folder-a".to_string(),
                    creates_cycle: false,
                },
                &DriveConcurrentOperation::ContentEdit {
                    file_id: "file-1".to_string(),
                    content_digest: digest(1),
                },
            )
            .unwrap(),
            DriveMergeOutcome::Merge
        );
        assert_eq!(
            drive_merge_outcome(
                &DriveConcurrentOperation::Delete {
                    node_id: "file-1".to_string(),
                    folder_delete: false,
                },
                &DriveConcurrentOperation::ContentEdit {
                    file_id: "file-1".to_string(),
                    content_digest: digest(1),
                },
            )
            .unwrap(),
            DriveMergeOutcome::ConflictRecord {
                field_or_region: "delete".to_string()
            }
        );
        assert_eq!(
            drive_merge_outcome(
                &DriveConcurrentOperation::Move {
                    node_id: "folder-a".to_string(),
                    target_folder_id: "folder-b".to_string(),
                    creates_cycle: true,
                },
                &DriveConcurrentOperation::Move {
                    node_id: "folder-a".to_string(),
                    target_folder_id: "folder-c".to_string(),
                    creates_cycle: false,
                },
            )
            .unwrap(),
            DriveMergeOutcome::Reject {
                rule: "path_cycle".to_string()
            }
        );
    }

    #[test]
    fn conflict_copy_name_preserves_extension_and_adds_collision_suffix() {
        assert_eq!(
            conflict_copy_name("Budget.xlsx", "Dana/Mac", 1_704_067_200_000, 2).unwrap(),
            "Budget (conflicted copy of Dana_Mac, 2024-01-01) - 2.xlsx"
        );
    }
}
