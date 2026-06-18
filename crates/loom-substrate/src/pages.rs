use std::collections::{BTreeMap, BTreeSet};

use loom_codec::Value;
use loom_types::{Algo, Code, Digest, LoomError, Result, WorkspaceId};

use crate::changes::{OperationChangeBatch, OperationChangeCursor, OperationChangeRecord};
use crate::{Fields, OperationEnvelope, codec_error, optional_text_value, validate_text};

pub const APP_ID: &str = "pages";
pub const SPACE_SCHEMA: &str = "loom.studio.pages.space.v1";
pub const PAGE_SCHEMA: &str = "loom.studio.pages.page.v1";
pub const PAGE_REVISION_SCHEMA: &str = "loom.studio.pages.page-revision.v1";
pub const PAGE_DRAFT_SCHEMA: &str = "loom.studio.pages.page-draft.v1";
pub const PAGE_CONFLICT_SCHEMA: &str = "loom.studio.pages.page-conflict.v1";
pub const STRUCTURE_SCHEMA: &str = "loom.studio.pages.structure.v1";
pub const STRUCTURE_NODE_SCHEMA: &str = "loom.studio.pages.structure-node.v1";
pub const STRUCTURE_EDGE_SCHEMA: &str = "loom.studio.pages.structure-edge.v1";
pub const PROFILE_SNAPSHOT_SCHEMA: &str = "loom.studio.pages.profile-snapshot.v1";
pub const PROFILE_OPERATION_LOG_SCHEMA: &str = "loom.studio.pages.operation-log.v1";
const PROFILE_CONTROL_PREFIX: &str = "profile/pages/v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageSpace {
    pub space_id: String,
    pub title: String,
    pub archived: bool,
}

impl PageSpace {
    pub fn new(space_id: impl Into<String>, title: impl Into<String>) -> Result<Self> {
        let space = Self {
            space_id: space_id.into(),
            title: title.into(),
            archived: false,
        };
        space.validate()?;
        Ok(space)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    fn validate(&self) -> Result<()> {
        validate_text("space_id", &self.space_id)?;
        validate_text("space title", &self.title)
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(SPACE_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.space_id.clone()),
                Value::Text(self.title.clone()),
                Value::Bool(self.archived),
            ]),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "page space")?;
        outer.expect_text(SPACE_SCHEMA)?;
        let mut fields = Fields::array(outer.next("page space fields")?, "page space")?;
        outer.end("page space")?;
        let space = Self {
            space_id: fields.text("space_id")?,
            title: fields.text("title")?,
            archived: read_bool(&mut fields, "archived")?,
        };
        fields.end("page space")?;
        space.validate()?;
        Ok(space)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspacePage {
    pub page_id: String,
    pub space_id: String,
    pub parent_page_id: Option<String>,
    pub title: String,
    pub current_revision: Option<u64>,
    pub deleted: bool,
}

impl WorkspacePage {
    pub fn new(
        page_id: impl Into<String>,
        space_id: impl Into<String>,
        parent_page_id: Option<String>,
        title: impl Into<String>,
    ) -> Result<Self> {
        let page = Self {
            page_id: page_id.into(),
            space_id: space_id.into(),
            parent_page_id,
            title: title.into(),
            current_revision: None,
            deleted: false,
        };
        page.validate()?;
        Ok(page)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    fn validate(&self) -> Result<()> {
        validate_text("page_id", &self.page_id)?;
        validate_text("page space_id", &self.space_id)?;
        if let Some(parent) = &self.parent_page_id {
            validate_text("page parent_page_id", parent)?;
            if parent == &self.page_id {
                return Err(LoomError::invalid("page must not parent itself"));
            }
        }
        validate_text("page title", &self.title)
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(PAGE_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.page_id.clone()),
                Value::Text(self.space_id.clone()),
                optional_text_value(self.parent_page_id.as_deref()),
                Value::Text(self.title.clone()),
                optional_u64_value(self.current_revision),
                Value::Bool(self.deleted),
            ]),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "organization page")?;
        outer.expect_text(PAGE_SCHEMA)?;
        let mut fields =
            Fields::array(outer.next("organization page fields")?, "organization page")?;
        outer.end("organization page")?;
        let page = Self {
            page_id: fields.text("page_id")?,
            space_id: fields.text("space_id")?,
            parent_page_id: fields.optional_text("parent_page_id")?,
            title: fields.text("title")?,
            current_revision: read_optional_u64(&mut fields, "current_revision")?,
            deleted: read_bool(&mut fields, "deleted")?,
        };
        fields.end("organization page")?;
        page.validate()?;
        Ok(page)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageRevision {
    pub page_id: String,
    pub revision: u64,
    pub body_digest: Digest,
    pub body: Vec<u8>,
    pub author: WorkspaceId,
    pub published_at_ms: u64,
}

impl PageRevision {
    pub fn new(
        algo: Algo,
        page_id: impl Into<String>,
        revision: u64,
        body: Vec<u8>,
        author: WorkspaceId,
        published_at_ms: u64,
    ) -> Result<Self> {
        if revision == 0 {
            return Err(LoomError::invalid("page revision must be positive"));
        }
        let revision = Self {
            page_id: page_id.into(),
            revision,
            body_digest: Digest::hash(algo, &body),
            body,
            author,
            published_at_ms,
        };
        revision.validate(algo)?;
        Ok(revision)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    pub fn validate(&self, algo: Algo) -> Result<()> {
        validate_text("revision page_id", &self.page_id)?;
        if self.revision == 0 {
            return Err(LoomError::invalid("page revision must be positive"));
        }
        let digest = Digest::hash(algo, &self.body);
        if digest != self.body_digest {
            return Err(LoomError::integrity_failure(
                "page revision body digest mismatch",
            ));
        }
        Ok(())
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(PAGE_REVISION_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.page_id.clone()),
                Value::Uint(self.revision),
                Value::Text(self.body_digest.to_string()),
                Value::Bytes(self.body.clone()),
                Value::Text(self.author.to_string()),
                Value::Uint(self.published_at_ms),
            ]),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "page revision")?;
        outer.expect_text(PAGE_REVISION_SCHEMA)?;
        let mut fields = Fields::array(outer.next("page revision fields")?, "page revision")?;
        outer.end("page revision")?;
        let revision = Self {
            page_id: fields.text("page_id")?,
            revision: fields.uint("revision")?,
            body_digest: fields.digest("body_digest")?,
            body: fields.bytes("body")?,
            author: fields.id("author")?,
            published_at_ms: fields.uint("published_at_ms")?,
        };
        fields.end("page revision")?;
        validate_text("revision page_id", &revision.page_id)?;
        if revision.revision == 0 {
            return Err(LoomError::invalid("page revision must be positive"));
        }
        Ok(revision)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageDraft {
    pub draft_id: String,
    pub page_id: String,
    pub principal: WorkspaceId,
    pub base_revision: Option<u64>,
    pub body: Vec<u8>,
    pub updated_at_ms: u64,
}

impl PageDraft {
    pub fn new(
        draft_id: impl Into<String>,
        page_id: impl Into<String>,
        principal: WorkspaceId,
        base_revision: Option<u64>,
        body: Vec<u8>,
        updated_at_ms: u64,
    ) -> Result<Self> {
        let draft = Self {
            draft_id: draft_id.into(),
            page_id: page_id.into(),
            principal,
            base_revision,
            body,
            updated_at_ms,
        };
        draft.validate()?;
        Ok(draft)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    fn validate(&self) -> Result<()> {
        validate_text("draft_id", &self.draft_id)?;
        validate_text("draft page_id", &self.page_id)?;
        if self.base_revision == Some(0) {
            return Err(LoomError::invalid("draft base revision must be positive"));
        }
        Ok(())
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(PAGE_DRAFT_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.draft_id.clone()),
                Value::Text(self.page_id.clone()),
                Value::Text(self.principal.to_string()),
                optional_u64_value(self.base_revision),
                Value::Bytes(self.body.clone()),
                Value::Uint(self.updated_at_ms),
            ]),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "page draft")?;
        outer.expect_text(PAGE_DRAFT_SCHEMA)?;
        let mut fields = Fields::array(outer.next("page draft fields")?, "page draft")?;
        outer.end("page draft")?;
        let draft = Self {
            draft_id: fields.text("draft_id")?,
            page_id: fields.text("page_id")?,
            principal: fields.id("principal")?,
            base_revision: read_optional_u64(&mut fields, "base_revision")?,
            body: fields.bytes("body")?,
            updated_at_ms: fields.uint("updated_at_ms")?,
        };
        fields.end("page draft")?;
        draft.validate()?;
        Ok(draft)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageConflictState {
    Open,
    Resolved,
    Superseded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageConflict {
    pub conflict_id: String,
    pub page_id: String,
    pub base_revision: Option<u64>,
    pub current_revision: Option<u64>,
    pub candidate_digest: Digest,
    pub state: PageConflictState,
}

impl PageConflict {
    pub fn new(
        conflict_id: impl Into<String>,
        page_id: impl Into<String>,
        base_revision: Option<u64>,
        current_revision: Option<u64>,
        candidate_digest: Digest,
    ) -> Result<Self> {
        let conflict = Self {
            conflict_id: conflict_id.into(),
            page_id: page_id.into(),
            base_revision,
            current_revision,
            candidate_digest,
            state: PageConflictState::Open,
        };
        conflict.validate()?;
        Ok(conflict)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    fn validate(&self) -> Result<()> {
        validate_text("conflict_id", &self.conflict_id)?;
        validate_text("conflict page_id", &self.page_id)
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(PAGE_CONFLICT_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.conflict_id.clone()),
                Value::Text(self.page_id.clone()),
                optional_u64_value(self.base_revision),
                optional_u64_value(self.current_revision),
                Value::Text(self.candidate_digest.to_string()),
                Value::Uint(match self.state {
                    PageConflictState::Open => 0,
                    PageConflictState::Resolved => 1,
                    PageConflictState::Superseded => 2,
                }),
            ]),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "page conflict")?;
        outer.expect_text(PAGE_CONFLICT_SCHEMA)?;
        let mut fields = Fields::array(outer.next("page conflict fields")?, "page conflict")?;
        outer.end("page conflict")?;
        let state = match fields.uint("conflict state")? {
            0 => PageConflictState::Open,
            1 => PageConflictState::Resolved,
            2 => PageConflictState::Superseded,
            other => {
                return Err(LoomError::corrupt(format!(
                    "unknown page conflict state tag {other}"
                )));
            }
        };
        let conflict = Self {
            conflict_id: fields.text("conflict_id")?,
            page_id: fields.text("page_id")?,
            base_revision: read_optional_u64(&mut fields, "base_revision")?,
            current_revision: read_optional_u64(&mut fields, "current_revision")?,
            candidate_digest: fields.digest("candidate_digest")?,
            state,
        };
        fields.end("page conflict")?;
        conflict.validate()?;
        Ok(conflict)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublishOutcome {
    Published(PageRevision),
    ConflictRecorded(PageConflict),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageStructure {
    pub structure_id: String,
    pub space_id: String,
    pub kind: String,
    pub title: String,
    pub root_node_id: Option<String>,
    pub field_ids: Vec<String>,
}

impl PageStructure {
    pub fn new(
        structure_id: impl Into<String>,
        space_id: impl Into<String>,
        kind: impl Into<String>,
        title: impl Into<String>,
    ) -> Result<Self> {
        let structure = Self {
            structure_id: structure_id.into(),
            space_id: space_id.into(),
            kind: kind.into(),
            title: title.into(),
            root_node_id: None,
            field_ids: Vec::new(),
        };
        structure.validate()?;
        Ok(structure)
    }

    pub fn with_field_ids(mut self, field_ids: Vec<String>) -> Result<Self> {
        self.set_field_ids(field_ids)?;
        Ok(self)
    }

    pub fn set_field_ids(&mut self, field_ids: Vec<String>) -> Result<()> {
        if self.kind != "database" && !field_ids.is_empty() {
            return Err(LoomError::invalid(
                "only database structures can carry field ids",
            ));
        }
        let field_ids = canonical_texts("structure field_id", field_ids)?;
        self.field_ids = field_ids;
        self.validate()
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    fn validate(&self) -> Result<()> {
        validate_text("structure_id", &self.structure_id)?;
        validate_text("structure space_id", &self.space_id)?;
        validate_structure_kind(&self.kind)?;
        validate_text("structure title", &self.title)?;
        if let Some(root) = &self.root_node_id {
            validate_text("structure root_node_id", root)?;
        }
        if self.kind != "database" && !self.field_ids.is_empty() {
            return Err(LoomError::invalid(
                "only database structures can carry field ids",
            ));
        }
        for field_id in &self.field_ids {
            validate_text("structure field_id", field_id)?;
        }
        Ok(())
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(STRUCTURE_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.structure_id.clone()),
                Value::Text(self.space_id.clone()),
                Value::Text(self.kind.clone()),
                Value::Text(self.title.clone()),
                optional_text_value(self.root_node_id.as_deref()),
                string_array(&self.field_ids),
            ]),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "page structure")?;
        outer.expect_text(STRUCTURE_SCHEMA)?;
        let mut fields = Fields::array(outer.next("page structure fields")?, "page structure")?;
        outer.end("page structure")?;
        let structure = Self {
            structure_id: fields.text("structure_id")?,
            space_id: fields.text("space_id")?,
            kind: fields.text("kind")?,
            title: fields.text("title")?,
            root_node_id: fields.optional_text("root_node_id")?,
            field_ids: read_string_array(&mut fields, "field_ids")?,
        };
        fields.end("page structure")?;
        structure.validate()?;
        Ok(structure)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructureNode {
    pub node_id: String,
    pub structure_id: String,
    pub kind: String,
    pub label: String,
    pub body_digest: Option<Digest>,
    pub entity_ref: Option<String>,
}

impl StructureNode {
    pub fn new(
        node_id: impl Into<String>,
        structure_id: impl Into<String>,
        kind: impl Into<String>,
        label: impl Into<String>,
        body_digest: Option<Digest>,
        entity_ref: Option<String>,
    ) -> Result<Self> {
        let node = Self {
            node_id: node_id.into(),
            structure_id: structure_id.into(),
            kind: kind.into(),
            label: label.into(),
            body_digest,
            entity_ref,
        };
        node.validate()?;
        Ok(node)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    fn validate(&self) -> Result<()> {
        validate_text("structure node_id", &self.node_id)?;
        validate_text("structure node structure_id", &self.structure_id)?;
        validate_text("structure node kind", &self.kind)?;
        validate_text("structure node label", &self.label)?;
        if let Some(entity_ref) = &self.entity_ref {
            validate_text("structure node entity_ref", entity_ref)?;
        }
        Ok(())
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(STRUCTURE_NODE_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.node_id.clone()),
                Value::Text(self.structure_id.clone()),
                Value::Text(self.kind.clone()),
                Value::Text(self.label.clone()),
                optional_digest_value(self.body_digest),
                optional_text_value(self.entity_ref.as_deref()),
            ]),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "structure node")?;
        outer.expect_text(STRUCTURE_NODE_SCHEMA)?;
        let mut fields = Fields::array(outer.next("structure node fields")?, "structure node")?;
        outer.end("structure node")?;
        let node = Self {
            node_id: fields.text("node_id")?,
            structure_id: fields.text("structure_id")?,
            kind: fields.text("kind")?,
            label: fields.text("label")?,
            body_digest: read_optional_digest(&mut fields, "body_digest")?,
            entity_ref: fields.optional_text("entity_ref")?,
        };
        fields.end("structure node")?;
        node.validate()?;
        Ok(node)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructureEdge {
    pub edge_id: String,
    pub structure_id: String,
    pub src_node_id: String,
    pub dst_node_id: String,
    pub label: String,
    pub target_ref: Option<String>,
}

impl StructureEdge {
    pub fn new(
        edge_id: impl Into<String>,
        structure_id: impl Into<String>,
        src_node_id: impl Into<String>,
        dst_node_id: impl Into<String>,
        label: impl Into<String>,
        target_ref: Option<String>,
    ) -> Result<Self> {
        let edge = Self {
            edge_id: edge_id.into(),
            structure_id: structure_id.into(),
            src_node_id: src_node_id.into(),
            dst_node_id: dst_node_id.into(),
            label: label.into(),
            target_ref,
        };
        edge.validate()?;
        Ok(edge)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    fn validate(&self) -> Result<()> {
        validate_text("structure edge_id", &self.edge_id)?;
        validate_text("structure edge structure_id", &self.structure_id)?;
        validate_text("structure edge src_node_id", &self.src_node_id)?;
        validate_text("structure edge dst_node_id", &self.dst_node_id)?;
        validate_text("structure edge label", &self.label)?;
        if let Some(target_ref) = &self.target_ref {
            validate_text("structure edge target_ref", target_ref)?;
        }
        Ok(())
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(STRUCTURE_EDGE_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.edge_id.clone()),
                Value::Text(self.structure_id.clone()),
                Value::Text(self.src_node_id.clone()),
                Value::Text(self.dst_node_id.clone()),
                Value::Text(self.label.clone()),
                optional_text_value(self.target_ref.as_deref()),
            ]),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "structure edge")?;
        outer.expect_text(STRUCTURE_EDGE_SCHEMA)?;
        let mut fields = Fields::array(outer.next("structure edge fields")?, "structure edge")?;
        outer.end("structure edge")?;
        let edge = Self {
            edge_id: fields.text("edge_id")?,
            structure_id: fields.text("structure_id")?,
            src_node_id: fields.text("src_node_id")?,
            dst_node_id: fields.text("dst_node_id")?,
            label: fields.text("label")?,
            target_ref: fields.optional_text("target_ref")?,
        };
        fields.end("structure edge")?;
        edge.validate()?;
        Ok(edge)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageWorkspaceSnapshot {
    pub workspace_id: String,
    pub spaces: Vec<PageSpace>,
    pub pages: Vec<WorkspacePage>,
    pub revisions: Vec<PageRevision>,
    pub drafts: Vec<PageDraft>,
    pub conflicts: Vec<PageConflict>,
    pub structures: Vec<PageStructure>,
    pub structure_nodes: Vec<StructureNode>,
    pub structure_edges: Vec<StructureEdge>,
}

pub struct PageWorkspaceSnapshotInput {
    pub workspace_id: String,
    pub spaces: Vec<PageSpace>,
    pub pages: Vec<WorkspacePage>,
    pub revisions: Vec<PageRevision>,
    pub drafts: Vec<PageDraft>,
    pub conflicts: Vec<PageConflict>,
    pub structures: Vec<PageStructure>,
    pub structure_nodes: Vec<StructureNode>,
    pub structure_edges: Vec<StructureEdge>,
}

impl PageWorkspaceSnapshot {
    pub fn new(input: PageWorkspaceSnapshotInput) -> Result<Self> {
        let snapshot = Self {
            workspace_id: input.workspace_id,
            spaces: input.spaces,
            pages: input.pages,
            revisions: input.revisions,
            drafts: input.drafts,
            conflicts: input.conflicts,
            structures: input.structures,
            structure_nodes: input.structure_nodes,
            structure_edges: input.structure_edges,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn from_workspace(
        workspace_id: impl Into<String>,
        organization: &PageWorkspace,
    ) -> Result<Self> {
        Self::new(PageWorkspaceSnapshotInput {
            workspace_id: workspace_id.into(),
            spaces: organization.spaces.values().cloned().collect(),
            pages: organization.pages.values().cloned().collect(),
            revisions: organization.revisions.values().cloned().collect(),
            drafts: organization.drafts.values().cloned().collect(),
            conflicts: organization.conflicts.values().cloned().collect(),
            structures: organization.structures.values().cloned().collect(),
            structure_nodes: organization.structure_nodes.values().cloned().collect(),
            structure_edges: organization.structure_edges.values().cloned().collect(),
        })
    }

    pub fn organization(&self, algo: Algo) -> Result<PageWorkspace> {
        let mut organization = PageWorkspace::default();
        for space in &self.spaces {
            organization
                .spaces
                .insert(space.space_id.clone(), space.clone());
        }
        for page in &self.pages {
            organization
                .pages
                .insert(page.page_id.clone(), page.clone());
        }
        for revision in &self.revisions {
            revision.validate(algo)?;
            organization.revisions.insert(
                (revision.page_id.clone(), revision.revision),
                revision.clone(),
            );
        }
        for draft in &self.drafts {
            organization
                .drafts
                .insert(draft.draft_id.clone(), draft.clone());
        }
        for conflict in &self.conflicts {
            organization
                .conflicts
                .insert(conflict.conflict_id.clone(), conflict.clone());
        }
        for structure in &self.structures {
            organization
                .structures
                .insert(structure.structure_id.clone(), structure.clone());
        }
        for node in &self.structure_nodes {
            organization
                .structure_nodes
                .insert(node.node_id.clone(), node.clone());
        }
        for edge in &self.structure_edges {
            organization
                .structure_edges
                .insert(edge.edge_id.clone(), edge.clone());
        }
        organization.validate()?;
        Ok(organization)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    fn validate(&self) -> Result<()> {
        validate_text("workspace_id", &self.workspace_id)?;
        let mut organization = PageWorkspace::default();
        for space in &self.spaces {
            if organization
                .spaces
                .insert(space.space_id.clone(), space.clone())
                .is_some()
            {
                return Err(LoomError::new(Code::AlreadyExists, "space already exists"));
            }
        }
        for page in &self.pages {
            if organization
                .pages
                .insert(page.page_id.clone(), page.clone())
                .is_some()
            {
                return Err(LoomError::new(Code::AlreadyExists, "page already exists"));
            }
        }
        for revision in &self.revisions {
            if organization
                .revisions
                .insert(
                    (revision.page_id.clone(), revision.revision),
                    revision.clone(),
                )
                .is_some()
            {
                return Err(LoomError::new(
                    Code::AlreadyExists,
                    "page revision already exists",
                ));
            }
        }
        for draft in &self.drafts {
            if organization
                .drafts
                .insert(draft.draft_id.clone(), draft.clone())
                .is_some()
            {
                return Err(LoomError::new(Code::AlreadyExists, "draft already exists"));
            }
        }
        for conflict in &self.conflicts {
            if organization
                .conflicts
                .insert(conflict.conflict_id.clone(), conflict.clone())
                .is_some()
            {
                return Err(LoomError::new(
                    Code::AlreadyExists,
                    "page conflict already exists",
                ));
            }
        }
        for structure in &self.structures {
            if organization
                .structures
                .insert(structure.structure_id.clone(), structure.clone())
                .is_some()
            {
                return Err(LoomError::new(
                    Code::AlreadyExists,
                    "structure already exists",
                ));
            }
        }
        for node in &self.structure_nodes {
            if organization
                .structure_nodes
                .insert(node.node_id.clone(), node.clone())
                .is_some()
            {
                return Err(LoomError::new(
                    Code::AlreadyExists,
                    "structure node already exists",
                ));
            }
        }
        for edge in &self.structure_edges {
            if organization
                .structure_edges
                .insert(edge.edge_id.clone(), edge.clone())
                .is_some()
            {
                return Err(LoomError::new(
                    Code::AlreadyExists,
                    "structure edge already exists",
                ));
            }
        }
        organization.validate()
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(PROFILE_SNAPSHOT_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.workspace_id.clone()),
                Value::Array(self.spaces.iter().map(PageSpace::to_value).collect()),
                Value::Array(self.pages.iter().map(WorkspacePage::to_value).collect()),
                Value::Array(self.revisions.iter().map(PageRevision::to_value).collect()),
                Value::Array(self.drafts.iter().map(PageDraft::to_value).collect()),
                Value::Array(self.conflicts.iter().map(PageConflict::to_value).collect()),
                Value::Array(
                    self.structures
                        .iter()
                        .map(PageStructure::to_value)
                        .collect(),
                ),
                Value::Array(
                    self.structure_nodes
                        .iter()
                        .map(StructureNode::to_value)
                        .collect(),
                ),
                Value::Array(
                    self.structure_edges
                        .iter()
                        .map(StructureEdge::to_value)
                        .collect(),
                ),
            ]),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "page organization snapshot")?;
        outer.expect_text(PROFILE_SNAPSHOT_SCHEMA)?;
        let mut fields = Fields::array(
            outer.next("page organization snapshot fields")?,
            "page organization snapshot",
        )?;
        outer.end("page organization snapshot")?;
        let snapshot = Self::new(PageWorkspaceSnapshotInput {
            workspace_id: fields.text("workspace_id")?,
            spaces: decode_values(fields.next("spaces")?, "spaces", PageSpace::from_value)?,
            pages: decode_values(fields.next("pages")?, "pages", WorkspacePage::from_value)?,
            revisions: decode_values(
                fields.next("revisions")?,
                "revisions",
                PageRevision::from_value,
            )?,
            drafts: decode_values(fields.next("drafts")?, "drafts", PageDraft::from_value)?,
            conflicts: decode_values(
                fields.next("conflicts")?,
                "conflicts",
                PageConflict::from_value,
            )?,
            structures: decode_values(
                fields.next("structures")?,
                "structures",
                PageStructure::from_value,
            )?,
            structure_nodes: decode_values(
                fields.next("structure_nodes")?,
                "structure_nodes",
                StructureNode::from_value,
            )?,
            structure_edges: decode_values(
                fields.next("structure_edges")?,
                "structure_edges",
                StructureEdge::from_value,
            )?,
        })?;
        fields.end("page organization snapshot")?;
        Ok(snapshot)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageOperationRecord {
    pub sequence: u64,
    pub operation_id: String,
    pub operation_kind: String,
    pub target_entity_id: Option<String>,
    pub root_after: Digest,
    pub envelope: Vec<u8>,
}

impl PageOperationRecord {
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
        validate_text("page operation_id", &self.operation_id)?;
        validate_text("page operation_kind", &self.operation_kind)?;
        if let Some(target) = &self.target_entity_id {
            validate_text("page operation target", target)?;
        }
        if self.envelope.is_empty() {
            return Err(LoomError::invalid(
                "page operation envelope must not be empty",
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
        let mut fields = Fields::array(value, "page operation record")?;
        let sequence = fields.uint("sequence")?;
        let operation_id = fields.text("operation_id")?;
        let operation_kind = fields.text("operation_kind")?;
        let target_entity_id = read_optional_text(&mut fields, "target_entity_id")?;
        let root_after = fields.digest("root_after")?;
        let envelope = fields.bytes("envelope")?;
        fields.end("page operation record")?;
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
pub struct PageOperationLog {
    pub workspace_id: String,
    pub records: Vec<PageOperationRecord>,
}

impl PageOperationLog {
    pub fn new(workspace_id: impl Into<String>, records: Vec<PageOperationRecord>) -> Result<Self> {
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

    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(PROFILE_OPERATION_LOG_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.workspace_id.clone()),
                Value::Array(
                    self.records
                        .iter()
                        .map(PageOperationRecord::to_value)
                        .collect(),
                ),
            ]),
        ])
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "page operation log")?;
        outer.expect_text(PROFILE_OPERATION_LOG_SCHEMA)?;
        let mut fields = Fields::array(
            outer.next("page operation log fields")?,
            "page operation log",
        )?;
        outer.end("page operation log")?;
        let workspace_id = fields.text("workspace_id")?;
        let records = page_operation_record_list(fields.next("records")?)?;
        fields.end("page operation log")?;
        Self::new(workspace_id, records)
    }

    fn validate(&self) -> Result<()> {
        validate_text("workspace_id", &self.workspace_id)?;
        let mut previous = None;
        let mut ids = BTreeSet::new();
        for record in &self.records {
            record.validate()?;
            if !ids.insert(record.operation_id.clone()) {
                return Err(LoomError::invalid("page operation ids must be unique"));
            }
            if let Some(previous) = previous
                && record.sequence <= previous
            {
                return Err(LoomError::invalid(
                    "page operation records must be ordered by increasing sequence",
                ));
            }
            previous = Some(record.sequence);
        }
        Ok(())
    }

    pub fn changes(
        &self,
        cursor: &OperationChangeCursor,
        max: usize,
    ) -> Result<OperationChangeBatch> {
        let expected_scope = page_operation_cursor_scope(&self.workspace_id);
        if cursor.scope_id != expected_scope {
            return Err(LoomError::invalid(
                "operation change cursor scope does not match page operation log",
            ));
        }
        let mut events = Vec::new();
        let mut next_sequence = cursor.next_sequence;
        for record in &self.records {
            if record.sequence < cursor.next_sequence {
                continue;
            }
            if events.len() == max {
                break;
            }
            let envelope = OperationEnvelope::decode(&record.envelope)?;
            let change = OperationChangeRecord {
                workspace_id: envelope.workspace_id,
                app_id: envelope.app_id,
                scope_id: envelope.scope_id,
                operation_id: record.operation_id.clone(),
                operation_kind: record.operation_kind.clone(),
                sequence: record.sequence,
                actor_principal: envelope.actor_principal.to_string(),
                timestamp_ms: envelope.timestamp_ms,
                root_after: record.root_after,
                target_entity_id: envelope.target_entity_id,
                payload_digest: envelope.payload_digest,
                policy_labels: envelope.policy_labels,
            };
            change.validate()?;
            next_sequence = change.sequence + 1;
            events.push(change);
        }
        Ok(OperationChangeBatch {
            events,
            next: OperationChangeCursor::new(expected_scope, next_sequence)?,
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PageWorkspace {
    pub spaces: BTreeMap<String, PageSpace>,
    pub pages: BTreeMap<String, WorkspacePage>,
    pub revisions: BTreeMap<(String, u64), PageRevision>,
    pub drafts: BTreeMap<String, PageDraft>,
    pub conflicts: BTreeMap<String, PageConflict>,
    pub structures: BTreeMap<String, PageStructure>,
    pub structure_nodes: BTreeMap<String, StructureNode>,
    pub structure_edges: BTreeMap<String, StructureEdge>,
}

impl PageWorkspace {
    pub fn create_space(
        &mut self,
        space_id: impl Into<String>,
        title: impl Into<String>,
    ) -> Result<&PageSpace> {
        let space = PageSpace::new(space_id, title)?;
        if self.spaces.contains_key(&space.space_id) {
            return Err(LoomError::new(Code::AlreadyExists, "space already exists"));
        }
        let space_id = space.space_id.clone();
        self.spaces.insert(space_id.clone(), space);
        Ok(self.spaces.get(&space_id).unwrap())
    }

    pub fn create_page(
        &mut self,
        page_id: impl Into<String>,
        space_id: &str,
        parent_page_id: Option<String>,
        title: impl Into<String>,
    ) -> Result<&WorkspacePage> {
        if !self.spaces.contains_key(space_id) {
            return Err(LoomError::not_found("space not found"));
        }
        let page = WorkspacePage::new(page_id, space_id, parent_page_id, title)?;
        if self.pages.contains_key(&page.page_id) {
            return Err(LoomError::new(Code::AlreadyExists, "page already exists"));
        }
        if let Some(parent) = &page.parent_page_id
            && !self.pages.contains_key(parent)
        {
            return Err(LoomError::not_found("parent page not found"));
        }
        let page_id = page.page_id.clone();
        self.pages.insert(page_id.clone(), page);
        Ok(self.pages.get(&page_id).unwrap())
    }

    pub fn create_draft(
        &mut self,
        draft_id: impl Into<String>,
        page_id: &str,
        principal: WorkspaceId,
        body: Vec<u8>,
        updated_at_ms: u64,
    ) -> Result<&PageDraft> {
        let page = self
            .pages
            .get(page_id)
            .ok_or_else(|| LoomError::not_found("page not found"))?;
        let draft = PageDraft::new(
            draft_id,
            page_id,
            principal,
            page.current_revision,
            body,
            updated_at_ms,
        )?;
        if self.drafts.contains_key(&draft.draft_id) {
            return Err(LoomError::new(Code::AlreadyExists, "draft already exists"));
        }
        let draft_id = draft.draft_id.clone();
        self.drafts.insert(draft_id.clone(), draft);
        Ok(self.drafts.get(&draft_id).unwrap())
    }

    pub fn update_draft(
        &mut self,
        draft_id: &str,
        body: Vec<u8>,
        updated_at_ms: u64,
    ) -> Result<&PageDraft> {
        let draft = self
            .drafts
            .get_mut(draft_id)
            .ok_or_else(|| LoomError::not_found("draft not found"))?;
        draft.body = body;
        draft.updated_at_ms = updated_at_ms;
        draft.validate()?;
        Ok(draft)
    }

    pub fn publish_draft(
        &mut self,
        algo: Algo,
        draft_id: &str,
        published_at_ms: u64,
    ) -> Result<PublishOutcome> {
        let draft = self
            .drafts
            .remove(draft_id)
            .ok_or_else(|| LoomError::not_found("draft not found"))?;
        let page = self
            .pages
            .get_mut(&draft.page_id)
            .ok_or_else(|| LoomError::not_found("page not found"))?;
        if page.current_revision != draft.base_revision {
            let candidate_digest = Digest::hash(algo, &draft.body);
            let conflict_id = format!("{}:{}:{}", draft.page_id, draft.draft_id, candidate_digest);
            let conflict = PageConflict::new(
                conflict_id,
                draft.page_id,
                draft.base_revision,
                page.current_revision,
                candidate_digest,
            )?;
            self.conflicts
                .insert(conflict.conflict_id.clone(), conflict.clone());
            return Ok(PublishOutcome::ConflictRecorded(conflict));
        }
        let next_revision = page.current_revision.unwrap_or(0) + 1;
        let revision = PageRevision::new(
            algo,
            &page.page_id,
            next_revision,
            draft.body,
            draft.principal,
            published_at_ms,
        )?;
        page.current_revision = Some(next_revision);
        self.revisions
            .insert((page.page_id.clone(), next_revision), revision.clone());
        Ok(PublishOutcome::Published(revision))
    }

    pub fn create_structure(
        &mut self,
        structure_id: impl Into<String>,
        space_id: &str,
        kind: impl Into<String>,
        title: impl Into<String>,
    ) -> Result<&PageStructure> {
        if !self.spaces.contains_key(space_id) {
            return Err(LoomError::not_found("space not found"));
        }
        let structure = PageStructure::new(structure_id, space_id, kind, title)?;
        if self.structures.contains_key(&structure.structure_id) {
            return Err(LoomError::new(
                Code::AlreadyExists,
                "structure already exists",
            ));
        }
        let structure_id = structure.structure_id.clone();
        self.structures.insert(structure_id.clone(), structure);
        Ok(self.structures.get(&structure_id).unwrap())
    }

    pub fn set_structure_field_ids(
        &mut self,
        structure_id: &str,
        field_ids: Vec<String>,
    ) -> Result<&PageStructure> {
        let structure = self
            .structures
            .get_mut(structure_id)
            .ok_or_else(|| LoomError::not_found("structure not found"))?;
        structure.set_field_ids(field_ids)?;
        Ok(structure)
    }

    pub fn add_structure_node(
        &mut self,
        node_id: impl Into<String>,
        structure_id: &str,
        kind: impl Into<String>,
        label: impl Into<String>,
        body_digest: Option<Digest>,
        entity_ref: Option<String>,
    ) -> Result<&StructureNode> {
        let structure = self
            .structures
            .get_mut(structure_id)
            .ok_or_else(|| LoomError::not_found("structure not found"))?;
        let node = StructureNode::new(node_id, structure_id, kind, label, body_digest, entity_ref)?;
        if self.structure_nodes.contains_key(&node.node_id) {
            return Err(LoomError::new(
                Code::AlreadyExists,
                "structure node already exists",
            ));
        }
        let node_id = node.node_id.clone();
        if structure.root_node_id.is_none() {
            structure.root_node_id = Some(node_id.clone());
        }
        self.structure_nodes.insert(node_id.clone(), node);
        Ok(self.structure_nodes.get(&node_id).unwrap())
    }

    pub fn update_structure_node(
        &mut self,
        structure_id: &str,
        node_id: &str,
        kind: impl Into<String>,
        label: impl Into<String>,
        body_digest: Option<Digest>,
        entity_ref: Option<String>,
    ) -> Result<&StructureNode> {
        let node = self
            .structure_nodes
            .get_mut(node_id)
            .ok_or_else(|| LoomError::not_found("structure node not found"))?;
        if node.structure_id != structure_id {
            return Err(LoomError::not_found("structure node not found"));
        }
        node.kind = kind.into();
        node.label = label.into();
        node.body_digest = body_digest;
        node.entity_ref = entity_ref;
        node.validate()?;
        Ok(node)
    }

    pub fn bind_structure_node(
        &mut self,
        structure_id: &str,
        node_id: &str,
        entity_ref: Option<String>,
    ) -> Result<&StructureNode> {
        let node = self
            .structure_nodes
            .get_mut(node_id)
            .ok_or_else(|| LoomError::not_found("structure node not found"))?;
        if node.structure_id != structure_id {
            return Err(LoomError::not_found("structure node not found"));
        }
        node.entity_ref = entity_ref;
        node.validate()?;
        Ok(node)
    }

    pub fn move_structure_node(
        &mut self,
        structure_id: &str,
        node_id: &str,
        parent_node_id: Option<&str>,
        label: &str,
    ) -> Result<Option<StructureEdge>> {
        if !self.structures.contains_key(structure_id) {
            return Err(LoomError::not_found("structure not found"));
        }
        let node = self
            .structure_nodes
            .get(node_id)
            .ok_or_else(|| LoomError::not_found("structure node not found"))?;
        if node.structure_id != structure_id {
            return Err(LoomError::not_found("structure node not found"));
        }
        if let Some(parent_node_id) = parent_node_id {
            if parent_node_id == node_id {
                return Err(LoomError::invalid("structure node cannot parent itself"));
            }
            let parent = self
                .structure_nodes
                .get(parent_node_id)
                .ok_or_else(|| LoomError::not_found("parent structure node not found"))?;
            if parent.structure_id != structure_id {
                return Err(LoomError::not_found("parent structure node not found"));
            }
            if self.structure_reaches(structure_id, node_id, parent_node_id, label) {
                return Err(LoomError::invalid("structure move would create a cycle"));
            }
        }
        let removed = self
            .structure_edges
            .iter()
            .filter(|(_, edge)| {
                edge.structure_id == structure_id
                    && edge.dst_node_id == node_id
                    && edge.label == label
            })
            .map(|(edge_id, _)| edge_id.clone())
            .collect::<Vec<_>>();
        for edge_id in removed {
            self.structure_edges.remove(&edge_id);
        }
        let Some(parent_node_id) = parent_node_id else {
            self.structures
                .get_mut(structure_id)
                .expect("structure exists")
                .root_node_id = Some(node_id.to_string());
            return Ok(None);
        };
        if self.structures[structure_id].root_node_id.as_deref() == Some(node_id) {
            return Err(LoomError::invalid(
                "structure root cannot be moved under a parent",
            ));
        }
        let edge_id = structure_parent_edge_id(structure_id, parent_node_id, node_id, label);
        let edge = StructureEdge::new(edge_id, structure_id, parent_node_id, node_id, label, None)?;
        self.structure_edges
            .insert(edge.edge_id.clone(), edge.clone());
        Ok(Some(edge))
    }

    pub fn link_structure_node(
        &mut self,
        edge_id: impl Into<String>,
        structure_id: &str,
        src_node_id: &str,
        dst_node_id: &str,
        label: impl Into<String>,
        target_ref: Option<String>,
    ) -> Result<&StructureEdge> {
        if !self.structures.contains_key(structure_id) {
            return Err(LoomError::not_found("structure not found"));
        }
        let src = self
            .structure_nodes
            .get(src_node_id)
            .ok_or_else(|| LoomError::not_found("source structure node not found"))?;
        let dst = self
            .structure_nodes
            .get(dst_node_id)
            .ok_or_else(|| LoomError::not_found("destination structure node not found"))?;
        if src.structure_id != structure_id || dst.structure_id != structure_id {
            return Err(LoomError::invalid(
                "structure edge endpoints must belong to the structure",
            ));
        }
        let edge = StructureEdge::new(
            edge_id,
            structure_id,
            src_node_id,
            dst_node_id,
            label,
            target_ref,
        )?;
        if self.structure_edges.contains_key(&edge.edge_id) {
            return Err(LoomError::new(
                Code::AlreadyExists,
                "structure edge already exists",
            ));
        }
        let edge_id = edge.edge_id.clone();
        self.structure_edges.insert(edge_id.clone(), edge);
        Ok(self.structure_edges.get(&edge_id).unwrap())
    }

    fn structure_reaches(
        &self,
        structure_id: &str,
        start: &str,
        target: &str,
        label: &str,
    ) -> bool {
        if start == target {
            return true;
        }
        let mut stack = vec![start];
        let mut seen = BTreeSet::new();
        while let Some(node_id) = stack.pop() {
            if !seen.insert(node_id) {
                continue;
            }
            for edge in self.structure_edges.values().filter(|edge| {
                edge.structure_id == structure_id
                    && edge.src_node_id == node_id
                    && edge.label == label
            }) {
                if edge.dst_node_id == target {
                    return true;
                }
                stack.push(&edge.dst_node_id);
            }
        }
        false
    }

    fn validate(&self) -> Result<()> {
        for page in self.pages.values() {
            if !self.spaces.contains_key(&page.space_id) {
                return Err(LoomError::not_found("space not found"));
            }
            if let Some(parent) = &page.parent_page_id
                && !self.pages.contains_key(parent)
            {
                return Err(LoomError::not_found("parent page not found"));
            }
            if let Some(current_revision) = page.current_revision
                && !self
                    .revisions
                    .contains_key(&(page.page_id.clone(), current_revision))
            {
                return Err(LoomError::not_found("current page revision not found"));
            }
        }
        for ((page_id, _), revision) in &self.revisions {
            if !self.pages.contains_key(page_id) {
                return Err(LoomError::not_found("revision page not found"));
            }
            if revision.page_id != *page_id {
                return Err(LoomError::corrupt("revision key page mismatch"));
            }
        }
        for draft in self.drafts.values() {
            if !self.pages.contains_key(&draft.page_id) {
                return Err(LoomError::not_found("draft page not found"));
            }
        }
        for conflict in self.conflicts.values() {
            if !self.pages.contains_key(&conflict.page_id) {
                return Err(LoomError::not_found("conflict page not found"));
            }
        }
        for structure in self.structures.values() {
            if !self.spaces.contains_key(&structure.space_id) {
                return Err(LoomError::not_found("structure space not found"));
            }
            if let Some(root) = &structure.root_node_id {
                let Some(node) = self.structure_nodes.get(root) else {
                    return Err(LoomError::not_found("structure root node not found"));
                };
                if node.structure_id != structure.structure_id {
                    return Err(LoomError::corrupt("structure root node mismatch"));
                }
            }
        }
        for node in self.structure_nodes.values() {
            if !self.structures.contains_key(&node.structure_id) {
                return Err(LoomError::not_found("structure node parent not found"));
            }
        }
        for edge in self.structure_edges.values() {
            if !self.structures.contains_key(&edge.structure_id) {
                return Err(LoomError::not_found("structure edge parent not found"));
            }
            let src = self
                .structure_nodes
                .get(&edge.src_node_id)
                .ok_or_else(|| LoomError::not_found("structure edge source not found"))?;
            let dst = self
                .structure_nodes
                .get(&edge.dst_node_id)
                .ok_or_else(|| LoomError::not_found("structure edge destination not found"))?;
            if src.structure_id != edge.structure_id || dst.structure_id != edge.structure_id {
                return Err(LoomError::corrupt("structure edge endpoint mismatch"));
            }
        }
        Ok(())
    }
}

pub fn structure_parent_edge_id(
    structure_id: &str,
    parent_node_id: &str,
    node_id: &str,
    label: &str,
) -> String {
    format!("{structure_id}:{label}:{parent_node_id}:{node_id}")
}

pub fn page_workspace_snapshot_key(workspace_id: &str) -> Result<Vec<u8>> {
    validate_text("workspace_id", workspace_id)?;
    Ok(format!("{PROFILE_CONTROL_PREFIX}/{workspace_id}/snapshot").into_bytes())
}

pub fn page_profile_operation_log_key(workspace_id: &str) -> Result<Vec<u8>> {
    validate_text("workspace_id", workspace_id)?;
    Ok(format!("{PROFILE_CONTROL_PREFIX}/{workspace_id}/operations").into_bytes())
}

pub fn page_operation_cursor_scope(workspace_id: &str) -> String {
    format!("pages:{workspace_id}")
}

fn digest_value(value: Digest) -> Value {
    Value::Text(value.to_string())
}

fn optional_u64_value(value: Option<u64>) -> Value {
    match value {
        Some(value) => Value::Array(vec![Value::Uint(1), Value::Uint(value)]),
        None => Value::Array(vec![Value::Uint(0)]),
    }
}

fn optional_digest_value(value: Option<Digest>) -> Value {
    optional_text_value(value.map(|digest| digest.to_string()).as_deref())
}

fn read_optional_digest(fields: &mut Fields, name: &str) -> Result<Option<Digest>> {
    match fields.next(name)? {
        Value::Array(mut values) if values.len() == 1 => match values.pop().unwrap() {
            Value::Uint(0) => Ok(None),
            _ => Err(LoomError::corrupt(format!(
                "{name} has invalid optional tag"
            ))),
        },
        Value::Array(mut values) if values.len() == 2 => {
            let value = values.pop().unwrap();
            let tag = values.pop().unwrap();
            match (tag, value) {
                (Value::Uint(1), Value::Text(value)) => Digest::parse(&value).map(Some),
                _ => Err(LoomError::corrupt(format!(
                    "{name} has invalid optional value"
                ))),
            }
        }
        _ => Err(LoomError::corrupt(format!(
            "{name} must be optional digest"
        ))),
    }
}

fn read_optional_text(fields: &mut Fields, name: &str) -> Result<Option<String>> {
    match fields.next(name)? {
        Value::Array(mut values) if values.len() == 1 => match values.pop().unwrap() {
            Value::Uint(0) => Ok(None),
            _ => Err(LoomError::corrupt(format!(
                "{name} has invalid optional tag"
            ))),
        },
        Value::Array(mut values) if values.len() == 2 => {
            let value = values.pop().unwrap();
            let tag = values.pop().unwrap();
            match (tag, value) {
                (Value::Uint(1), Value::Text(value)) => Ok(Some(value)),
                _ => Err(LoomError::corrupt(format!(
                    "{name} has invalid optional value"
                ))),
            }
        }
        _ => Err(LoomError::corrupt(format!("{name} must be optional text"))),
    }
}

fn read_optional_u64(fields: &mut Fields, name: &str) -> Result<Option<u64>> {
    match fields.next(name)? {
        Value::Array(mut values) if values.len() == 1 => match values.pop().unwrap() {
            Value::Uint(0) => Ok(None),
            _ => Err(LoomError::corrupt(format!(
                "{name} has invalid optional tag"
            ))),
        },
        Value::Array(mut values) if values.len() == 2 => {
            let value = values.pop().unwrap();
            let tag = values.pop().unwrap();
            match (tag, value) {
                (Value::Uint(1), Value::Uint(value)) if value > 0 => Ok(Some(value)),
                (Value::Uint(1), Value::Uint(_)) => {
                    Err(LoomError::invalid(format!("{name} value must be positive")))
                }
                _ => Err(LoomError::corrupt(format!(
                    "{name} has invalid optional value"
                ))),
            }
        }
        _ => Err(LoomError::corrupt(format!("{name} must be optional uint"))),
    }
}

fn read_bool(fields: &mut Fields, name: &str) -> Result<bool> {
    match fields.next(name)? {
        Value::Bool(value) => Ok(value),
        _ => Err(LoomError::corrupt(format!("{name} must be bool"))),
    }
}

fn decode_values<T>(
    value: Value,
    name: &str,
    decode: impl Fn(Value) -> Result<T>,
) -> Result<Vec<T>> {
    match value {
        Value::Array(values) => values.into_iter().map(decode).collect(),
        _ => Err(LoomError::corrupt(format!("{name} must be an array"))),
    }
}

fn validate_structure_kind(kind: &str) -> Result<()> {
    match kind {
        "mindmap"
        | "outline"
        | "decision_tree"
        | "canvas"
        | "database"
        | "diagram.flowchart"
        | "diagram.sequence"
        | "diagram.architecture" => Ok(()),
        _ => Err(LoomError::invalid("unknown structure kind")),
    }
}

fn canonical_texts(name: &str, values: Vec<String>) -> Result<Vec<String>> {
    let mut out = BTreeSet::new();
    for value in values {
        validate_text(name, &value)?;
        out.insert(value);
    }
    Ok(out.into_iter().collect())
}

fn string_array(values: &[String]) -> Value {
    Value::Array(
        values
            .iter()
            .map(|value| Value::Text(value.clone()))
            .collect(),
    )
}

fn read_string_array(fields: &mut Fields, name: &str) -> Result<Vec<String>> {
    match fields.next(name)? {
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

fn page_operation_record_list(value: Value) -> Result<Vec<PageOperationRecord>> {
    match value {
        Value::Array(values) => values
            .into_iter()
            .map(PageOperationRecord::from_value)
            .collect(),
        _ => Err(LoomError::corrupt(
            "page operation records must be an array",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ActorKind, OperationEnvelopeInput};

    fn principal(byte: u8) -> WorkspaceId {
        WorkspaceId::v4_from_bytes([byte; 16])
    }

    fn page_operation_record(sequence: u64, kind: &str) -> PageOperationRecord {
        let operation_id = format!("studio:{sequence}");
        let envelope = OperationEnvelope::new(
            Algo::Blake3,
            OperationEnvelopeInput {
                workspace_id: "studio",
                app_id: APP_ID,
                scope_id: "page-1",
                operation_id: &operation_id,
                operation_kind: kind,
                sequence,
                actor_principal: principal(7),
                actor_kind: ActorKind::User,
                timestamp_ms: sequence * 10,
                idempotency_key: &operation_id,
                base_root: Digest::hash(Algo::Blake3, b"base"),
                base_entity_version: None,
                target_entity_id: Some("page-1"),
                payload: kind.as_bytes(),
                policy_labels: &[],
                signature: None,
                agent: None,
            },
        )
        .unwrap();
        PageOperationRecord::new(
            sequence,
            operation_id,
            kind,
            Some("page-1".to_string()),
            Digest::hash(Algo::Blake3, format!("root-{sequence}").as_bytes()),
            envelope.encode().unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn page_space_revision_and_draft_round_trip() {
        let space = PageSpace::new("eng", "Engineering").unwrap();
        assert_eq!(PageSpace::decode(&space.encode().unwrap()).unwrap(), space);
        let page = WorkspacePage::new("page-1", "eng", None, "Roadmap").unwrap();
        assert_eq!(
            WorkspacePage::decode(&page.encode().unwrap()).unwrap(),
            page
        );
        let draft = PageDraft::new(
            "draft-1",
            "page-1",
            principal(1),
            None,
            b"body".to_vec(),
            10,
        )
        .unwrap();
        assert_eq!(PageDraft::decode(&draft.encode().unwrap()).unwrap(), draft);
        let revision = PageRevision::new(
            Algo::Blake3,
            "page-1",
            1,
            b"published".to_vec(),
            principal(2),
            20,
        )
        .unwrap();
        assert_eq!(
            PageRevision::decode(&revision.encode().unwrap()).unwrap(),
            revision
        );
    }

    #[test]
    fn page_operation_log_round_trips_and_pages_changes() {
        let log = PageOperationLog::new(
            "studio",
            vec![
                page_operation_record(1, "page.updated"),
                page_operation_record(2, "page.published"),
            ],
        )
        .unwrap();
        let decoded = PageOperationLog::decode(&log.encode().unwrap()).unwrap();
        assert_eq!(decoded, log);

        let changes = decoded
            .changes(
                &OperationChangeCursor::new(page_operation_cursor_scope("studio"), 2).unwrap(),
                10,
            )
            .unwrap();

        assert_eq!(changes.events.len(), 1);
        assert_eq!(changes.events[0].operation_kind, "page.published");
        assert_eq!(changes.events[0].app_id, "pages");
        assert_eq!(changes.next.encode(), "oplog:3:pages:studio");
    }

    #[test]
    fn publish_draft_advances_revision_and_removes_draft() {
        let mut organization = PageWorkspace::default();
        organization.create_space("eng", "Engineering").unwrap();
        organization
            .create_page("page-1", "eng", None, "Roadmap")
            .unwrap();
        organization
            .create_draft("draft-1", "page-1", principal(1), b"v1".to_vec(), 10)
            .unwrap();
        let outcome = organization
            .publish_draft(Algo::Blake3, "draft-1", 20)
            .unwrap();
        let PublishOutcome::Published(revision) = outcome else {
            panic!("draft should publish");
        };
        assert_eq!(revision.revision, 1);
        assert_eq!(revision.body, b"v1".to_vec());
        assert_eq!(
            organization.pages["page-1"].current_revision,
            Some(revision.revision)
        );
        assert!(!organization.drafts.contains_key("draft-1"));
    }

    #[test]
    fn stale_publish_records_conflict_without_overwriting_current_revision() {
        let mut organization = PageWorkspace::default();
        organization.create_space("eng", "Engineering").unwrap();
        organization
            .create_page("page-1", "eng", None, "Roadmap")
            .unwrap();
        organization
            .create_draft("a", "page-1", principal(1), b"a".to_vec(), 10)
            .unwrap();
        organization
            .create_draft("b", "page-1", principal(2), b"b".to_vec(), 11)
            .unwrap();
        assert!(matches!(
            organization.publish_draft(Algo::Blake3, "a", 20).unwrap(),
            PublishOutcome::Published(_)
        ));
        let outcome = organization.publish_draft(Algo::Blake3, "b", 21).unwrap();
        let PublishOutcome::ConflictRecorded(conflict) = outcome else {
            panic!("stale draft should record conflict");
        };
        assert_eq!(conflict.base_revision, None);
        assert_eq!(conflict.current_revision, Some(1));
        assert_eq!(organization.pages["page-1"].current_revision, Some(1));
        assert_eq!(organization.conflicts.len(), 1);
    }

    #[test]
    fn profile_snapshot_round_trips_workspace() {
        let mut organization = PageWorkspace::default();
        organization.create_space("eng", "Engineering").unwrap();
        organization
            .create_page("page-1", "eng", None, "Roadmap")
            .unwrap();
        organization
            .create_draft("draft-1", "page-1", principal(1), b"v1".to_vec(), 10)
            .unwrap();
        assert!(matches!(
            organization
                .publish_draft(Algo::Blake3, "draft-1", 20)
                .unwrap(),
            PublishOutcome::Published(_)
        ));
        let snapshot = PageWorkspaceSnapshot::from_workspace("studio", &organization).unwrap();
        assert_eq!(
            PageWorkspaceSnapshot::decode(&snapshot.encode().unwrap()).unwrap(),
            snapshot
        );
        assert_eq!(
            snapshot.organization(Algo::Blake3).unwrap().pages["page-1"].current_revision,
            Some(1)
        );
        assert_eq!(
            page_workspace_snapshot_key("studio").unwrap(),
            b"profile/pages/v1/studio/snapshot".to_vec()
        );
    }

    #[test]
    fn structures_validate_as_graph_containers() {
        let mut organization = PageWorkspace::default();
        organization.create_space("eng", "Engineering").unwrap();
        let structure = organization
            .create_structure("roadmap", "eng", "mindmap", "Roadmap")
            .unwrap()
            .clone();
        assert_eq!(structure.kind, "mindmap");
        organization
            .add_structure_node("root", "roadmap", "topic", "Root", None, None)
            .unwrap();
        organization
            .add_structure_node(
                "feature",
                "roadmap",
                "topic",
                "Feature",
                None,
                Some("page:page-1".to_string()),
            )
            .unwrap();
        let edge = organization
            .link_structure_node("edge-1", "roadmap", "root", "feature", "child_of", None)
            .unwrap()
            .clone();
        assert_eq!(edge.label, "child_of");
        assert_eq!(
            organization.structures["roadmap"].root_node_id,
            Some("root".to_string())
        );
        assert!(
            organization
                .set_structure_field_ids("roadmap", vec!["field-a".to_string()])
                .is_err()
        );
        organization
            .create_structure("db", "eng", "database", "Plans")
            .unwrap();
        organization
            .set_structure_field_ids(
                "db",
                vec![
                    "field-status".to_string(),
                    "field-priority".to_string(),
                    "field-status".to_string(),
                ],
            )
            .unwrap();
        assert_eq!(
            organization.structures["db"].field_ids,
            vec!["field-priority".to_string(), "field-status".to_string()]
        );
        let snapshot = PageWorkspaceSnapshot::from_workspace("studio", &organization).unwrap();
        assert_eq!(
            snapshot
                .organization(Algo::Blake3)
                .unwrap()
                .structure_edges
                .len(),
            1
        );
        assert_eq!(
            snapshot.organization(Algo::Blake3).unwrap().structures["db"].field_ids,
            vec!["field-priority".to_string(), "field-status".to_string()]
        );
    }

    #[test]
    fn revision_detects_body_digest_mismatch() {
        let mut revision = PageRevision::new(
            Algo::Blake3,
            "page-1",
            1,
            b"published".to_vec(),
            principal(3),
            30,
        )
        .unwrap();
        revision.body = b"tampered".to_vec();
        assert_eq!(
            revision.validate(Algo::Blake3).unwrap_err().code,
            Code::IntegrityFailure
        );
    }
}
