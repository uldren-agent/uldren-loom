use std::collections::BTreeSet;

use loom_codec::Value;
use loom_types::{Digest, LoomError, Result, WorkspaceId};

use crate::{
    Fields, codec_error, optional_digest, optional_text_value, string_array, validate_text,
};

pub const APP_ID: &str = "surfaces";
pub const APP_DEFINITION_SCHEMA: &str = "loom.studio.surfaces.app-definition.v1";
pub const ELICITATION_REQUEST_SCHEMA: &str = "loom.studio.surfaces.elicitation-request.v1";
pub const ELICITATION_RESPONSE_SCHEMA: &str = "loom.studio.surfaces.elicitation-response.v1";
pub const PROMPT_HANDOFF_SCHEMA: &str = "loom.studio.surfaces.prompt-handoff.v1";
pub const RENDER_FRAME_SCHEMA: &str = "loom.studio.surfaces.render-frame.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreSurfaceAppKind {
    TicketDetails,
    Board,
    DocumentViewer,
    DirectedGraph,
}

impl CoreSurfaceAppKind {
    pub const fn app_id(self) -> &'static str {
        match self {
            Self::TicketDetails => "ticket-details",
            Self::Board => "board",
            Self::DocumentViewer => "document-viewer",
            Self::DirectedGraph => "directed-graph",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogSurfaceAppKind {
    Roadmap,
    SearchPalette,
    ChangesInbox,
    SprintPlanner,
    BacklogTriage,
    DecisionLog,
    AuditTimeline,
    Dashboards,
    RevisionDiff,
    MindMap,
    Canvas,
    DiagramEditor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeetingMemorySurfaceAppKind {
    MeetingDetails,
    MemoryGraph,
    ExtractionReview,
    MeetingSearch,
    ImportCoverage,
    AccessAudit,
}

impl MeetingMemorySurfaceAppKind {
    pub const fn app_id(self) -> &'static str {
        match self {
            Self::MeetingDetails => "meeting-details",
            Self::MemoryGraph => "memory-graph",
            Self::ExtractionReview => "extraction-review",
            Self::MeetingSearch => "meeting-search",
            Self::ImportCoverage => "import-coverage",
            Self::AccessAudit => "access-audit",
        }
    }
}

impl CatalogSurfaceAppKind {
    pub const fn app_id(self) -> &'static str {
        match self {
            Self::Roadmap => "roadmap",
            Self::SearchPalette => "search-palette",
            Self::ChangesInbox => "changes-inbox",
            Self::SprintPlanner => "sprint-planner",
            Self::BacklogTriage => "backlog-triage",
            Self::DecisionLog => "decision-log",
            Self::AuditTimeline => "audit-timeline",
            Self::Dashboards => "dashboards",
            Self::RevisionDiff => "revision-diff",
            Self::MindMap => "mind-map",
            Self::Canvas => "canvas",
            Self::DiagramEditor => "diagram-editor",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElicitationStatus {
    Pending,
    Submitted,
    Cancelled,
    Expired,
}

impl ElicitationStatus {
    const fn tag(self) -> u64 {
        match self {
            Self::Pending => 0,
            Self::Submitted => 1,
            Self::Cancelled => 2,
            Self::Expired => 3,
        }
    }

    fn from_tag(tag: u64) -> Result<Self> {
        match tag {
            0 => Ok(Self::Pending),
            1 => Ok(Self::Submitted),
            2 => Ok(Self::Cancelled),
            3 => Ok(Self::Expired),
            other => Err(LoomError::corrupt(format!(
                "unknown surfaces elicitation status tag {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StalenessPolicy {
    Hide,
    ShowAsOf,
    RequireRefresh,
}

impl StalenessPolicy {
    const fn tag(self) -> u64 {
        match self {
            Self::Hide => 0,
            Self::ShowAsOf => 1,
            Self::RequireRefresh => 2,
        }
    }

    fn from_tag(tag: u64) -> Result<Self> {
        match tag {
            0 => Ok(Self::Hide),
            1 => Ok(Self::ShowAsOf),
            2 => Ok(Self::RequireRefresh),
            other => Err(LoomError::corrupt(format!(
                "unknown surfaces staleness policy tag {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceAppDefinition {
    pub app_id: String,
    pub display_name: String,
    pub resource_uri: String,
    pub projection_refs: Vec<String>,
    pub read_tools: Vec<String>,
    pub write_tools: Vec<String>,
    pub elicitation_schema_refs: Vec<String>,
    pub prompt_handoff_refs: Vec<String>,
    pub subscription_refs: Vec<String>,
    pub staleness_policy: StalenessPolicy,
}

#[derive(Debug, Clone)]
pub struct SurfaceAppDefinitionInput<'a> {
    pub app_id: &'a str,
    pub display_name: &'a str,
    pub resource_uri: &'a str,
    pub projection_refs: &'a [&'a str],
    pub read_tools: &'a [&'a str],
    pub write_tools: &'a [&'a str],
    pub elicitation_schema_refs: &'a [&'a str],
    pub prompt_handoff_refs: &'a [&'a str],
    pub subscription_refs: &'a [&'a str],
    pub staleness_policy: StalenessPolicy,
}

impl SurfaceAppDefinition {
    pub fn new(input: SurfaceAppDefinitionInput<'_>) -> Result<Self> {
        let app = Self {
            app_id: input.app_id.to_string(),
            display_name: input.display_name.to_string(),
            resource_uri: input.resource_uri.to_string(),
            projection_refs: text_list(input.projection_refs)?,
            read_tools: text_list(input.read_tools)?,
            write_tools: text_list(input.write_tools)?,
            elicitation_schema_refs: text_list(input.elicitation_schema_refs)?,
            prompt_handoff_refs: text_list(input.prompt_handoff_refs)?,
            subscription_refs: text_list(input.subscription_refs)?,
            staleness_policy: input.staleness_policy,
        };
        app.validate()?;
        Ok(app)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(APP_DEFINITION_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.app_id.clone()),
                Value::Text(self.display_name.clone()),
                Value::Text(self.resource_uri.clone()),
                string_array(&self.projection_refs),
                string_array(&self.read_tools),
                string_array(&self.write_tools),
                string_array(&self.elicitation_schema_refs),
                string_array(&self.prompt_handoff_refs),
                string_array(&self.subscription_refs),
                Value::Uint(self.staleness_policy.tag()),
            ]),
        ])
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "surface app definition")?;
        outer.expect_text(APP_DEFINITION_SCHEMA)?;
        let mut fields = Fields::array(
            outer.next("surface app definition fields")?,
            "surface app definition",
        )?;
        outer.end("surface app definition")?;
        let app = Self {
            app_id: fields.text("app_id")?,
            display_name: fields.text("display_name")?,
            resource_uri: fields.text("resource_uri")?,
            projection_refs: fields.string_array("projection_refs")?,
            read_tools: fields.string_array("read_tools")?,
            write_tools: fields.string_array("write_tools")?,
            elicitation_schema_refs: fields.string_array("elicitation_schema_refs")?,
            prompt_handoff_refs: fields.string_array("prompt_handoff_refs")?,
            subscription_refs: fields.string_array("subscription_refs")?,
            staleness_policy: StalenessPolicy::from_tag(fields.uint("staleness_policy")?)?,
        };
        fields.end("surface app definition")?;
        app.validate()?;
        Ok(app)
    }

    fn validate(&self) -> Result<()> {
        validate_text("surface app_id", &self.app_id)?;
        validate_text("surface display_name", &self.display_name)?;
        validate_ui_resource_uri(&self.resource_uri)?;
        validate_text_list("surface projection_ref", &self.projection_refs)?;
        validate_text_list("surface read tool", &self.read_tools)?;
        validate_text_list("surface write tool", &self.write_tools)?;
        validate_text_list(
            "surface elicitation schema_ref",
            &self.elicitation_schema_refs,
        )?;
        validate_text_list("surface prompt_handoff_ref", &self.prompt_handoff_refs)?;
        validate_text_list("surface subscription_ref", &self.subscription_refs)?;
        require_unique("surface projection refs", &self.projection_refs)?;
        require_unique("surface read tools", &self.read_tools)?;
        require_unique("surface write tools", &self.write_tools)?;
        require_unique(
            "surface elicitation schema refs",
            &self.elicitation_schema_refs,
        )?;
        require_unique("surface prompt handoff refs", &self.prompt_handoff_refs)?;
        require_unique("surface subscription refs", &self.subscription_refs)
    }
}

pub fn core_surface_app(kind: CoreSurfaceAppKind, workspace: &str) -> Result<SurfaceAppDefinition> {
    validate_text("surface workspace", workspace)?;
    let app_id = kind.app_id();
    let resource_uri = format!("ui://{workspace}/mcp/apps/{app_id}");
    match kind {
        CoreSurfaceAppKind::TicketDetails => SurfaceAppDefinition::new(SurfaceAppDefinitionInput {
            app_id,
            display_name: "Ticket Details",
            resource_uri: &resource_uri,
            projection_refs: &["view:tickets.detail", "view:substrate.revisions"],
            read_tools: &["tickets_get", "substrate_history", "substrate_refs"],
            write_tools: &["tickets_update", "annotation.added"],
            elicitation_schema_refs: &["schema:tickets.update", "schema:tickets.comment"],
            prompt_handoff_refs: &["prompt:tickets.split", "prompt:tickets.explain"],
            subscription_refs: &["changes:tickets", "changes:annotations"],
            staleness_policy: StalenessPolicy::ShowAsOf,
        }),
        CoreSurfaceAppKind::Board => SurfaceAppDefinition::new(SurfaceAppDefinitionInput {
            app_id,
            display_name: "Board",
            resource_uri: &resource_uri,
            projection_refs: &["view:tickets.board", "view:lifecycle.stage"],
            read_tools: &["tickets_board_get", "tickets_board_list", "tickets_list"],
            write_tools: &[
                "tickets_board_create",
                "tickets_board_update",
                "tickets_board_delete",
                "tickets_board_configure_columns",
                "tickets_board_move_card",
                "tickets_update",
            ],
            elicitation_schema_refs: &["schema:tickets.update", "schema:tickets.rank"],
            prompt_handoff_refs: &["prompt:tickets.plan-sprint"],
            subscription_refs: &["changes:tickets", "changes:lifecycle"],
            staleness_policy: StalenessPolicy::ShowAsOf,
        }),
        CoreSurfaceAppKind::DocumentViewer => {
            SurfaceAppDefinition::new(SurfaceAppDefinitionInput {
                app_id,
                display_name: "Spec Document Viewer",
                resource_uri: &resource_uri,
                projection_refs: &["view:pages.document", "view:substrate.revisions"],
                read_tools: &["pages_get", "substrate_history", "substrate_refs"],
                write_tools: &["pages_update", "pages_publish", "annotation.added"],
                elicitation_schema_refs: &["schema:pages.publish", "schema:pages.restore"],
                prompt_handoff_refs: &["prompt:pages.rewrite", "prompt:pages.extract-tasks"],
                subscription_refs: &["changes:pages", "changes:annotations"],
                staleness_policy: StalenessPolicy::ShowAsOf,
            })
        }
        CoreSurfaceAppKind::DirectedGraph => SurfaceAppDefinition::new(SurfaceAppDefinitionInput {
            app_id,
            display_name: "Directed Graph",
            resource_uri: &resource_uri,
            projection_refs: &["view:substrate.refs", "view:graph.neighborhood"],
            read_tools: &["substrate_refs", "substrate_search"],
            write_tools: &["graph.edge_add", "graph.edge_remove"],
            elicitation_schema_refs: &["schema:graph.edge-confirm"],
            prompt_handoff_refs: &["prompt:graph.explain-cluster"],
            subscription_refs: &["changes:refs", "changes:graph"],
            staleness_policy: StalenessPolicy::ShowAsOf,
        }),
    }
}

pub fn core_surface_catalog(workspace: &str) -> Result<Vec<SurfaceAppDefinition>> {
    [
        CoreSurfaceAppKind::TicketDetails,
        CoreSurfaceAppKind::Board,
        CoreSurfaceAppKind::DocumentViewer,
        CoreSurfaceAppKind::DirectedGraph,
    ]
    .into_iter()
    .map(|kind| core_surface_app(kind, workspace))
    .collect()
}

pub fn catalog_surface_app(
    kind: CatalogSurfaceAppKind,
    workspace: &str,
) -> Result<SurfaceAppDefinition> {
    validate_text("surface workspace", workspace)?;
    let app_id = kind.app_id();
    let resource_uri = format!("ui://{workspace}/mcp/apps/{app_id}");
    match kind {
        CatalogSurfaceAppKind::Roadmap => SurfaceAppDefinition::new(SurfaceAppDefinitionInput {
            app_id,
            display_name: "Roadmap",
            resource_uri: &resource_uri,
            projection_refs: &["view:tickets.planning", "view:graph.dependencies"],
            read_tools: &["tickets.search", "substrate_refs"],
            write_tools: &["tickets_update", "graph.edge_add"],
            elicitation_schema_refs: &["schema:planning.reschedule"],
            prompt_handoff_refs: &["prompt:planning.explain-critical-path"],
            subscription_refs: &["changes:tickets", "changes:graph"],
            staleness_policy: StalenessPolicy::ShowAsOf,
        }),
        CatalogSurfaceAppKind::SearchPalette => {
            SurfaceAppDefinition::new(SurfaceAppDefinitionInput {
                app_id,
                display_name: "Search Palette",
                resource_uri: &resource_uri,
                projection_refs: &["view:substrate.search"],
                read_tools: &["substrate_search"],
                write_tools: &[],
                elicitation_schema_refs: &[],
                prompt_handoff_refs: &["prompt:search.refine"],
                subscription_refs: &["changes:search"],
                staleness_policy: StalenessPolicy::ShowAsOf,
            })
        }
        CatalogSurfaceAppKind::ChangesInbox => {
            SurfaceAppDefinition::new(SurfaceAppDefinitionInput {
                app_id,
                display_name: "Changes Inbox",
                resource_uri: &resource_uri,
                projection_refs: &["view:substrate.changes", "view:substrate.conflicts"],
                read_tools: &["substrate_changes"],
                write_tools: &["changes.acknowledge", "conflicts.resolve"],
                elicitation_schema_refs: &["schema:conflicts.resolve"],
                prompt_handoff_refs: &["prompt:changes.summarize"],
                subscription_refs: &["changes:substrate"],
                staleness_policy: StalenessPolicy::RequireRefresh,
            })
        }
        CatalogSurfaceAppKind::SprintPlanner => {
            SurfaceAppDefinition::new(SurfaceAppDefinitionInput {
                app_id,
                display_name: "Sprint Planner",
                resource_uri: &resource_uri,
                projection_refs: &["view:tickets.sprint", "view:tickets.capacity"],
                read_tools: &["tickets.search", "substrate_view_get"],
                write_tools: &["tickets.rank", "tickets_update"],
                elicitation_schema_refs: &["schema:sprint.over-capacity"],
                prompt_handoff_refs: &["prompt:sprint.plan"],
                subscription_refs: &["changes:tickets"],
                staleness_policy: StalenessPolicy::ShowAsOf,
            })
        }
        CatalogSurfaceAppKind::BacklogTriage => {
            SurfaceAppDefinition::new(SurfaceAppDefinitionInput {
                app_id,
                display_name: "Backlog Triage",
                resource_uri: &resource_uri,
                projection_refs: &["view:tickets.untriaged"],
                read_tools: &["tickets.search"],
                write_tools: &["tickets_update"],
                elicitation_schema_refs: &["schema:tickets.triage"],
                prompt_handoff_refs: &["prompt:tickets.classify"],
                subscription_refs: &["changes:tickets"],
                staleness_policy: StalenessPolicy::ShowAsOf,
            })
        }
        CatalogSurfaceAppKind::DecisionLog => {
            SurfaceAppDefinition::new(SurfaceAppDefinitionInput {
                app_id,
                display_name: "Decision Log",
                resource_uri: &resource_uri,
                projection_refs: &["view:ledger.decisions"],
                read_tools: &["ledger.read"],
                write_tools: &["ledger.append_decision"],
                elicitation_schema_refs: &["schema:decision.record"],
                prompt_handoff_refs: &["prompt:decision.explain"],
                subscription_refs: &["changes:ledger"],
                staleness_policy: StalenessPolicy::ShowAsOf,
            })
        }
        CatalogSurfaceAppKind::AuditTimeline => {
            SurfaceAppDefinition::new(SurfaceAppDefinitionInput {
                app_id,
                display_name: "Audit Timeline",
                resource_uri: &resource_uri,
                projection_refs: &["view:audit.timeline"],
                read_tools: &["audit.list", "audit.view"],
                write_tools: &[],
                elicitation_schema_refs: &["schema:audit.export"],
                prompt_handoff_refs: &["prompt:audit.summarize"],
                subscription_refs: &["changes:audit"],
                staleness_policy: StalenessPolicy::ShowAsOf,
            })
        }
        CatalogSurfaceAppKind::Dashboards => SurfaceAppDefinition::new(SurfaceAppDefinitionInput {
            app_id,
            display_name: "Dashboards",
            resource_uri: &resource_uri,
            projection_refs: &["view:dataframe.metrics", "view:tickets.history"],
            read_tools: &["dataframe.query", "substrate_view_get"],
            write_tools: &[],
            elicitation_schema_refs: &[],
            prompt_handoff_refs: &["prompt:dashboard.explain"],
            subscription_refs: &["changes:tickets", "changes:dataframe"],
            staleness_policy: StalenessPolicy::ShowAsOf,
        }),
        CatalogSurfaceAppKind::RevisionDiff => {
            SurfaceAppDefinition::new(SurfaceAppDefinitionInput {
                app_id,
                display_name: "Revision Diff",
                resource_uri: &resource_uri,
                projection_refs: &["view:substrate.revisions", "view:substrate.diff"],
                read_tools: &["substrate_history", "vcs.diff_commits"],
                write_tools: &["substrate.restore_revision"],
                elicitation_schema_refs: &["schema:revision.restore"],
                prompt_handoff_refs: &["prompt:revision.explain"],
                subscription_refs: &["changes:substrate"],
                staleness_policy: StalenessPolicy::ShowAsOf,
            })
        }
        CatalogSurfaceAppKind::MindMap => SurfaceAppDefinition::new(SurfaceAppDefinitionInput {
            app_id,
            display_name: "Mind Map",
            resource_uri: &resource_uri,
            projection_refs: &["view:pages.structure", "view:substrate.refs"],
            read_tools: &["structures_get", "substrate_refs"],
            write_tools: &[
                "structures_add_node",
                "structures_move_node",
                "structures_link_node",
            ],
            elicitation_schema_refs: &["schema:structure.node"],
            prompt_handoff_refs: &["prompt:structure.decompose"],
            subscription_refs: &["changes:pages", "changes:refs"],
            staleness_policy: StalenessPolicy::ShowAsOf,
        }),
        CatalogSurfaceAppKind::Canvas => SurfaceAppDefinition::new(SurfaceAppDefinitionInput {
            app_id,
            display_name: "Canvas",
            resource_uri: &resource_uri,
            projection_refs: &["view:structure.canvas", "view:substrate.refs"],
            read_tools: &["structures_get", "substrate_refs"],
            write_tools: &["structures_update_node", "structures_link_node"],
            elicitation_schema_refs: &["schema:canvas.bind"],
            prompt_handoff_refs: &["prompt:canvas.organize"],
            subscription_refs: &["changes:pages", "changes:refs"],
            staleness_policy: StalenessPolicy::ShowAsOf,
        }),
        CatalogSurfaceAppKind::DiagramEditor => {
            SurfaceAppDefinition::new(SurfaceAppDefinitionInput {
                app_id,
                display_name: "Diagram Editor",
                resource_uri: &resource_uri,
                projection_refs: &["view:structure.diagram", "view:graph.neighborhood"],
                read_tools: &["structures_get", "substrate_refs"],
                write_tools: &[
                    "structures_update_node",
                    "graph.edge_add",
                    "graph.edge_remove",
                ],
                elicitation_schema_refs: &["schema:diagram.bind"],
                prompt_handoff_refs: &["prompt:diagram.edit"],
                subscription_refs: &["changes:pages", "changes:graph"],
                staleness_policy: StalenessPolicy::ShowAsOf,
            })
        }
    }
}

pub fn surface_app_catalog(workspace: &str) -> Result<Vec<SurfaceAppDefinition>> {
    let mut catalog = core_surface_catalog(workspace)?;
    catalog.extend(
        [
            CatalogSurfaceAppKind::Roadmap,
            CatalogSurfaceAppKind::SearchPalette,
            CatalogSurfaceAppKind::ChangesInbox,
            CatalogSurfaceAppKind::SprintPlanner,
            CatalogSurfaceAppKind::BacklogTriage,
            CatalogSurfaceAppKind::DecisionLog,
            CatalogSurfaceAppKind::AuditTimeline,
            CatalogSurfaceAppKind::Dashboards,
            CatalogSurfaceAppKind::RevisionDiff,
            CatalogSurfaceAppKind::MindMap,
            CatalogSurfaceAppKind::Canvas,
            CatalogSurfaceAppKind::DiagramEditor,
        ]
        .into_iter()
        .map(|kind| catalog_surface_app(kind, workspace))
        .collect::<Result<Vec<_>>>()?,
    );
    let app_ids = catalog
        .iter()
        .map(|app| app.app_id.clone())
        .collect::<Vec<_>>();
    require_unique("surface catalog app ids", &app_ids)?;
    Ok(catalog)
}

pub fn meeting_memory_surface_app(
    kind: MeetingMemorySurfaceAppKind,
    workspace: &str,
) -> Result<SurfaceAppDefinition> {
    validate_text("surface workspace", workspace)?;
    let app_id = kind.app_id();
    let resource_uri = format!("ui://{workspace}/mcp/apps/{app_id}");
    match kind {
        MeetingMemorySurfaceAppKind::MeetingDetails => {
            SurfaceAppDefinition::new(SurfaceAppDefinitionInput {
                app_id,
                display_name: "Meeting Details",
                resource_uri: &resource_uri,
                projection_refs: &[
                    "view:meetings.detail",
                    "view:meetings.spans",
                    "view:substrate.revisions",
                ],
                read_tools: &["meetings.get", "meetings.spans", "substrate_history"],
                write_tools: &[
                    "meetings.annotation_accept",
                    "meetings.annotation_reject",
                    "meetings.redact_span",
                ],
                elicitation_schema_refs: &["schema:meetings.redaction"],
                prompt_handoff_refs: &["prompt:meetings.summarize"],
                subscription_refs: &["changes:meetings"],
                staleness_policy: StalenessPolicy::ShowAsOf,
            })
        }
        MeetingMemorySurfaceAppKind::MemoryGraph => {
            SurfaceAppDefinition::new(SurfaceAppDefinitionInput {
                app_id,
                display_name: "Memory Graph",
                resource_uri: &resource_uri,
                projection_refs: &["view:meetings.graph", "view:substrate.refs"],
                read_tools: &["meetings.graph", "substrate_refs"],
                write_tools: &["meetings.edge_accept"],
                elicitation_schema_refs: &["schema:meetings.edge-accept"],
                prompt_handoff_refs: &["prompt:meetings.explain-cluster"],
                subscription_refs: &["changes:meetings", "changes:refs"],
                staleness_policy: StalenessPolicy::ShowAsOf,
            })
        }
        MeetingMemorySurfaceAppKind::ExtractionReview => {
            SurfaceAppDefinition::new(SurfaceAppDefinitionInput {
                app_id,
                display_name: "Extraction Review",
                resource_uri: &resource_uri,
                projection_refs: &["view:meetings.extraction-review"],
                read_tools: &["meetings.extraction_review"],
                write_tools: &[
                    "meetings.annotation_accept",
                    "meetings.annotation_reject",
                    "meetings.entity_merge",
                    "meetings.vocabulary_promote",
                ],
                elicitation_schema_refs: &[
                    "schema:meetings.entity-merge",
                    "schema:meetings.vocabulary-promote",
                ],
                prompt_handoff_refs: &["prompt:meetings.review"],
                subscription_refs: &["changes:meetings"],
                staleness_policy: StalenessPolicy::RequireRefresh,
            })
        }
        MeetingMemorySurfaceAppKind::MeetingSearch => {
            SurfaceAppDefinition::new(SurfaceAppDefinitionInput {
                app_id,
                display_name: "Meeting Search",
                resource_uri: &resource_uri,
                projection_refs: &["view:meetings.search"],
                read_tools: &["search"],
                write_tools: &[],
                elicitation_schema_refs: &[],
                prompt_handoff_refs: &[],
                subscription_refs: &["changes:meetings", "changes:search"],
                staleness_policy: StalenessPolicy::ShowAsOf,
            })
        }
        MeetingMemorySurfaceAppKind::ImportCoverage => {
            SurfaceAppDefinition::new(SurfaceAppDefinitionInput {
                app_id,
                display_name: "Import Coverage",
                resource_uri: &resource_uri,
                projection_refs: &["view:meetings.import-coverage"],
                read_tools: &["meetings.import_runs", "meetings.fidelity_report"],
                write_tools: &[
                    "meetings.import_rerun",
                    "meetings.import_scope_change",
                    "meetings.bridge_stop",
                ],
                elicitation_schema_refs: &["schema:meetings.import-scope-change"],
                prompt_handoff_refs: &["prompt:meetings.explain-coverage"],
                subscription_refs: &["changes:meetings.import"],
                staleness_policy: StalenessPolicy::ShowAsOf,
            })
        }
        MeetingMemorySurfaceAppKind::AccessAudit => {
            SurfaceAppDefinition::new(SurfaceAppDefinitionInput {
                app_id,
                display_name: "Access Audit",
                resource_uri: &resource_uri,
                projection_refs: &["view:meetings.access-audit"],
                read_tools: &["meetings.access_audit", "audit.list"],
                write_tools: &[],
                elicitation_schema_refs: &["schema:meetings.access-export"],
                prompt_handoff_refs: &["prompt:meetings.audit-summary"],
                subscription_refs: &["changes:meetings", "changes:audit"],
                staleness_policy: StalenessPolicy::ShowAsOf,
            })
        }
    }
}

pub fn meeting_memory_surface_catalog(workspace: &str) -> Result<Vec<SurfaceAppDefinition>> {
    [
        MeetingMemorySurfaceAppKind::MeetingDetails,
        MeetingMemorySurfaceAppKind::MemoryGraph,
        MeetingMemorySurfaceAppKind::ExtractionReview,
        MeetingMemorySurfaceAppKind::MeetingSearch,
        MeetingMemorySurfaceAppKind::ImportCoverage,
        MeetingMemorySurfaceAppKind::AccessAudit,
    ]
    .into_iter()
    .map(|kind| meeting_memory_surface_app(kind, workspace))
    .collect()
}

pub fn surface_catalog_json(workspace: &str, set: &str) -> Result<String> {
    let apps = match set {
        "core" => core_surface_catalog(workspace)?,
        "all" => surface_app_catalog(workspace)?,
        "meeting-memory" => meeting_memory_surface_catalog(workspace)?,
        other => {
            return Err(LoomError::invalid(format!(
                "unsupported Studio surface catalog set {other:?}; supported sets: core, all, meeting-memory"
            )));
        }
    };
    let apps = apps
        .iter()
        .map(surface_app_json)
        .collect::<Vec<serde_json::Value>>();
    Ok(serde_json::json!({
        "workspace": workspace,
        "set": set,
        "apps": apps,
    })
    .to_string())
}

fn surface_app_json(app: &SurfaceAppDefinition) -> serde_json::Value {
    serde_json::json!({
        "app_id": &app.app_id,
        "display_name": &app.display_name,
        "resource_uri": &app.resource_uri,
        "projection_refs": &app.projection_refs,
        "read_tools": &app.read_tools,
        "write_tools": &app.write_tools,
        "elicitation_schema_refs": &app.elicitation_schema_refs,
        "prompt_handoff_refs": &app.prompt_handoff_refs,
        "subscription_refs": &app.subscription_refs,
        "staleness_policy": staleness_policy_name(app.staleness_policy),
    })
}

fn staleness_policy_name(policy: StalenessPolicy) -> &'static str {
    match policy {
        StalenessPolicy::Hide => "hide",
        StalenessPolicy::ShowAsOf => "show_as_of",
        StalenessPolicy::RequireRefresh => "require_refresh",
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElicitationRequest {
    pub request_id: String,
    pub app_id: String,
    pub principal_id: WorkspaceId,
    pub operation_kind: String,
    pub message: String,
    pub schema_ref: String,
    pub schema_digest: Digest,
    pub context_digest: Option<Digest>,
    pub requested_at_ms: u64,
    pub expires_at_ms: Option<u64>,
    pub status: ElicitationStatus,
}

#[derive(Debug, Clone)]
pub struct ElicitationRequestInput<'a> {
    pub request_id: &'a str,
    pub app_id: &'a str,
    pub principal_id: WorkspaceId,
    pub operation_kind: &'a str,
    pub message: &'a str,
    pub schema_ref: &'a str,
    pub schema_digest: Digest,
    pub context_digest: Option<Digest>,
    pub requested_at_ms: u64,
    pub expires_at_ms: Option<u64>,
    pub status: ElicitationStatus,
}

impl ElicitationRequest {
    pub fn new(input: ElicitationRequestInput<'_>) -> Result<Self> {
        let request = Self {
            request_id: input.request_id.to_string(),
            app_id: input.app_id.to_string(),
            principal_id: input.principal_id,
            operation_kind: input.operation_kind.to_string(),
            message: input.message.to_string(),
            schema_ref: input.schema_ref.to_string(),
            schema_digest: input.schema_digest,
            context_digest: input.context_digest,
            requested_at_ms: input.requested_at_ms,
            expires_at_ms: input.expires_at_ms,
            status: input.status,
        };
        request.validate()?;
        Ok(request)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(ELICITATION_REQUEST_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.request_id.clone()),
                Value::Text(self.app_id.clone()),
                Value::Text(self.principal_id.to_string()),
                Value::Text(self.operation_kind.clone()),
                Value::Text(self.message.clone()),
                Value::Text(self.schema_ref.clone()),
                Value::Text(self.schema_digest.to_string()),
                optional_digest(self.context_digest),
                Value::Uint(self.requested_at_ms),
                optional_u64(self.expires_at_ms),
                Value::Uint(self.status.tag()),
            ]),
        ])
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "surface elicitation request")?;
        outer.expect_text(ELICITATION_REQUEST_SCHEMA)?;
        let mut fields = Fields::array(
            outer.next("surface elicitation request fields")?,
            "surface elicitation request",
        )?;
        outer.end("surface elicitation request")?;
        let request = Self {
            request_id: fields.text("request_id")?,
            app_id: fields.text("app_id")?,
            principal_id: fields.id("principal_id")?,
            operation_kind: fields.text("operation_kind")?,
            message: fields.text("message")?,
            schema_ref: fields.text("schema_ref")?,
            schema_digest: fields.digest("schema_digest")?,
            context_digest: fields.optional_digest("context_digest")?,
            requested_at_ms: fields.uint("requested_at_ms")?,
            expires_at_ms: optional_u64_field(&mut fields, "expires_at_ms")?,
            status: ElicitationStatus::from_tag(fields.uint("status")?)?,
        };
        fields.end("surface elicitation request")?;
        request.validate()?;
        Ok(request)
    }

    fn validate(&self) -> Result<()> {
        validate_text("surface request_id", &self.request_id)?;
        validate_text("surface app_id", &self.app_id)?;
        validate_text("surface operation_kind", &self.operation_kind)?;
        validate_text("surface message", &self.message)?;
        validate_text("surface schema_ref", &self.schema_ref)?;
        if let Some(expires_at_ms) = self.expires_at_ms
            && expires_at_ms <= self.requested_at_ms
        {
            return Err(LoomError::invalid(
                "surface elicitation expiry must be after request time",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElicitationResponse {
    pub request_id: String,
    pub responder_principal_id: WorkspaceId,
    pub response_digest: Digest,
    pub response_len: u64,
    pub responded_at_ms: u64,
    pub status: ElicitationStatus,
}

impl ElicitationResponse {
    pub fn new(
        request_id: impl Into<String>,
        responder_principal_id: WorkspaceId,
        response_digest: Digest,
        response_len: u64,
        responded_at_ms: u64,
        status: ElicitationStatus,
    ) -> Result<Self> {
        let response = Self {
            request_id: request_id.into(),
            responder_principal_id,
            response_digest,
            response_len,
            responded_at_ms,
            status,
        };
        response.validate()?;
        Ok(response)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(ELICITATION_RESPONSE_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.request_id.clone()),
                Value::Text(self.responder_principal_id.to_string()),
                Value::Text(self.response_digest.to_string()),
                Value::Uint(self.response_len),
                Value::Uint(self.responded_at_ms),
                Value::Uint(self.status.tag()),
            ]),
        ])
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "surface elicitation response")?;
        outer.expect_text(ELICITATION_RESPONSE_SCHEMA)?;
        let mut fields = Fields::array(
            outer.next("surface elicitation response fields")?,
            "surface elicitation response",
        )?;
        outer.end("surface elicitation response")?;
        let response = Self {
            request_id: fields.text("request_id")?,
            responder_principal_id: fields.id("responder_principal_id")?,
            response_digest: fields.digest("response_digest")?,
            response_len: fields.uint("response_len")?,
            responded_at_ms: fields.uint("responded_at_ms")?,
            status: ElicitationStatus::from_tag(fields.uint("status")?)?,
        };
        fields.end("surface elicitation response")?;
        response.validate()?;
        Ok(response)
    }

    fn validate(&self) -> Result<()> {
        validate_text("surface request_id", &self.request_id)?;
        if self.status == ElicitationStatus::Pending {
            return Err(LoomError::invalid(
                "surface elicitation response cannot be pending",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptHandoff {
    pub handoff_id: String,
    pub app_id: String,
    pub principal_id: WorkspaceId,
    pub prompt_digest: Digest,
    pub prompt_len: u64,
    pub source_entity_refs: Vec<String>,
    pub target_prompt_ref: Option<String>,
    pub created_at_ms: u64,
}

impl PromptHandoff {
    pub fn new(input: PromptHandoffInput<'_>) -> Result<Self> {
        let handoff = Self {
            handoff_id: input.handoff_id.to_string(),
            app_id: input.app_id.to_string(),
            principal_id: input.principal_id,
            prompt_digest: input.prompt_digest,
            prompt_len: input.prompt_len,
            source_entity_refs: text_list(input.source_entity_refs)?,
            target_prompt_ref: input.target_prompt_ref.map(str::to_string),
            created_at_ms: input.created_at_ms,
        };
        handoff.validate()?;
        Ok(handoff)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(PROMPT_HANDOFF_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.handoff_id.clone()),
                Value::Text(self.app_id.clone()),
                Value::Text(self.principal_id.to_string()),
                Value::Text(self.prompt_digest.to_string()),
                Value::Uint(self.prompt_len),
                string_array(&self.source_entity_refs),
                optional_text_value(self.target_prompt_ref.as_deref()),
                Value::Uint(self.created_at_ms),
            ]),
        ])
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "surface prompt handoff")?;
        outer.expect_text(PROMPT_HANDOFF_SCHEMA)?;
        let mut fields = Fields::array(
            outer.next("surface prompt handoff fields")?,
            "surface prompt handoff",
        )?;
        outer.end("surface prompt handoff")?;
        let handoff = Self {
            handoff_id: fields.text("handoff_id")?,
            app_id: fields.text("app_id")?,
            principal_id: fields.id("principal_id")?,
            prompt_digest: fields.digest("prompt_digest")?,
            prompt_len: fields.uint("prompt_len")?,
            source_entity_refs: fields.string_array("source_entity_refs")?,
            target_prompt_ref: fields.optional_text("target_prompt_ref")?,
            created_at_ms: fields.uint("created_at_ms")?,
        };
        fields.end("surface prompt handoff")?;
        handoff.validate()?;
        Ok(handoff)
    }

    fn validate(&self) -> Result<()> {
        validate_text("surface handoff_id", &self.handoff_id)?;
        validate_text("surface app_id", &self.app_id)?;
        if self.prompt_len == 0 {
            return Err(LoomError::invalid("surface prompt must not be empty"));
        }
        validate_text_list("surface source entity ref", &self.source_entity_refs)?;
        if let Some(target_prompt_ref) = &self.target_prompt_ref {
            validate_text("surface target_prompt_ref", target_prompt_ref)?;
        }
        require_unique("surface source entity refs", &self.source_entity_refs)
    }
}

#[derive(Debug, Clone)]
pub struct PromptHandoffInput<'a> {
    pub handoff_id: &'a str,
    pub app_id: &'a str,
    pub principal_id: WorkspaceId,
    pub prompt_digest: Digest,
    pub prompt_len: u64,
    pub source_entity_refs: &'a [&'a str],
    pub target_prompt_ref: Option<&'a str>,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderFrame {
    pub app_id: String,
    pub view_ref: String,
    pub as_of_root: Digest,
    pub cursor: Option<String>,
    pub stale: bool,
}

impl RenderFrame {
    pub fn new(
        app_id: impl Into<String>,
        view_ref: impl Into<String>,
        as_of_root: Digest,
        cursor: Option<&str>,
        stale: bool,
    ) -> Result<Self> {
        let frame = Self {
            app_id: app_id.into(),
            view_ref: view_ref.into(),
            as_of_root,
            cursor: cursor.map(str::to_string),
            stale,
        };
        frame.validate()?;
        Ok(frame)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(RENDER_FRAME_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.app_id.clone()),
                Value::Text(self.view_ref.clone()),
                Value::Text(self.as_of_root.to_string()),
                optional_text_value(self.cursor.as_deref()),
                Value::Bool(self.stale),
            ]),
        ])
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "surface render frame")?;
        outer.expect_text(RENDER_FRAME_SCHEMA)?;
        let mut fields = Fields::array(
            outer.next("surface render frame fields")?,
            "surface render frame",
        )?;
        outer.end("surface render frame")?;
        let frame = Self {
            app_id: fields.text("app_id")?,
            view_ref: fields.text("view_ref")?,
            as_of_root: fields.digest("as_of_root")?,
            cursor: fields.optional_text("cursor")?,
            stale: fields.bool("stale")?,
        };
        fields.end("surface render frame")?;
        frame.validate()?;
        Ok(frame)
    }

    fn validate(&self) -> Result<()> {
        validate_text("surface app_id", &self.app_id)?;
        validate_text("surface view_ref", &self.view_ref)?;
        if let Some(cursor) = &self.cursor {
            validate_text("surface cursor", cursor)?;
        }
        Ok(())
    }
}

fn text_list(values: &[&str]) -> Result<Vec<String>> {
    values
        .iter()
        .map(|value| {
            validate_text("surface text list item", value)?;
            Ok((*value).to_string())
        })
        .collect()
}

fn validate_text_list(name: &str, values: &[String]) -> Result<()> {
    for value in values {
        validate_text(name, value)?;
    }
    Ok(())
}

fn require_unique(name: &str, values: &[String]) -> Result<()> {
    let mut seen = BTreeSet::new();
    for value in values {
        if !seen.insert(value.as_str()) {
            return Err(LoomError::invalid(format!("{name} must be unique")));
        }
    }
    Ok(())
}

fn validate_ui_resource_uri(value: &str) -> Result<()> {
    validate_text("surface resource_uri", value)?;
    if !value.starts_with("ui://") {
        return Err(LoomError::invalid(
            "surface resource_uri must use the ui scheme",
        ));
    }
    Ok(())
}

fn optional_u64(value: Option<u64>) -> Value {
    match value {
        Some(value) => Value::Array(vec![Value::Uint(1), Value::Uint(value)]),
        None => Value::Array(vec![Value::Uint(0)]),
    }
}

fn optional_u64_field(fields: &mut Fields, name: &str) -> Result<Option<u64>> {
    let mut optional = Fields::array(fields.next(name)?, name)?;
    let tag = optional.uint(name)?;
    let value = match tag {
        0 => None,
        1 => Some(optional.uint(name)?),
        other => {
            return Err(LoomError::corrupt(format!(
                "{name} has unknown optional tag {other}"
            )));
        }
    };
    optional.end(name)?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use loom_types::{Algo, Digest, WorkspaceId};

    use super::*;

    fn digest(label: &[u8]) -> Digest {
        Digest::hash(Algo::Sha256, label)
    }

    fn principal() -> WorkspaceId {
        WorkspaceId::parse("11111111-1111-4111-8111-111111111111").unwrap()
    }

    #[test]
    fn app_definition_round_trips_and_validates_uri() {
        let app = SurfaceAppDefinition::new(SurfaceAppDefinitionInput {
            app_id: "board",
            display_name: "Board",
            resource_uri: "ui://main/mcp/apps/board",
            projection_refs: &["view:board"],
            read_tools: &["tickets_get"],
            write_tools: &["tickets_update"],
            elicitation_schema_refs: &["schema:transition"],
            prompt_handoff_refs: &["prompt:split-ticket"],
            subscription_refs: &["changes:tickets"],
            staleness_policy: StalenessPolicy::ShowAsOf,
        })
        .unwrap();

        assert_eq!(
            SurfaceAppDefinition::decode(&app.encode().unwrap()).unwrap(),
            app
        );
        assert!(
            SurfaceAppDefinition::new(SurfaceAppDefinitionInput {
                resource_uri: "https://example.invalid/app",
                ..SurfaceAppDefinitionInput {
                    app_id: "bad",
                    display_name: "Bad",
                    resource_uri: "ui://main/mcp/apps/bad",
                    projection_refs: &[],
                    read_tools: &[],
                    write_tools: &[],
                    elicitation_schema_refs: &[],
                    prompt_handoff_refs: &[],
                    subscription_refs: &[],
                    staleness_policy: StalenessPolicy::Hide,
                }
            })
            .is_err()
        );
    }

    #[test]
    fn elicitation_request_requires_schema_and_time_order() {
        let request = ElicitationRequest::new(ElicitationRequestInput {
            request_id: "req-1",
            app_id: "board",
            principal_id: principal(),
            operation_kind: "tickets_update",
            message: "Missing resolution",
            schema_ref: "schema:transition-resolution",
            schema_digest: digest(b"schema"),
            context_digest: Some(digest(b"context")),
            requested_at_ms: 10,
            expires_at_ms: Some(20),
            status: ElicitationStatus::Pending,
        })
        .unwrap();

        assert_eq!(
            ElicitationRequest::decode(&request.encode().unwrap()).unwrap(),
            request
        );
        assert!(
            ElicitationRequest::new(ElicitationRequestInput {
                expires_at_ms: Some(9),
                ..ElicitationRequestInput {
                    request_id: "req-2",
                    app_id: "board",
                    principal_id: principal(),
                    operation_kind: "tickets_update",
                    message: "Missing resolution",
                    schema_ref: "schema:transition-resolution",
                    schema_digest: digest(b"schema"),
                    context_digest: None,
                    requested_at_ms: 10,
                    expires_at_ms: None,
                    status: ElicitationStatus::Pending,
                }
            })
            .is_err()
        );
    }

    #[test]
    fn response_prompt_handoff_and_render_frame_round_trip() {
        let response = ElicitationResponse::new(
            "req-1",
            principal(),
            digest(b"response"),
            17,
            30,
            ElicitationStatus::Submitted,
        )
        .unwrap();
        assert_eq!(
            ElicitationResponse::decode(&response.encode().unwrap()).unwrap(),
            response
        );
        assert!(
            ElicitationResponse::new(
                "req-1",
                principal(),
                digest(b"response"),
                17,
                30,
                ElicitationStatus::Pending,
            )
            .is_err()
        );

        let handoff = PromptHandoff::new(PromptHandoffInput {
            handoff_id: "handoff-1",
            app_id: "graph",
            principal_id: principal(),
            prompt_digest: digest(b"prompt"),
            prompt_len: 42,
            source_entity_refs: &["ticket:LOOM-9", "page:design"],
            target_prompt_ref: Some("prompt:explain-cluster"),
            created_at_ms: 40,
        })
        .unwrap();
        assert_eq!(
            PromptHandoff::decode(&handoff.encode().unwrap()).unwrap(),
            handoff
        );

        let frame = RenderFrame::new(
            "board",
            "view:board",
            digest(b"root"),
            Some("changes:10"),
            true,
        )
        .unwrap();
        assert_eq!(
            RenderFrame::decode(&frame.encode().unwrap()).unwrap(),
            frame
        );
    }

    #[test]
    fn core_surface_catalog_defines_the_first_four_apps() {
        let catalog = core_surface_catalog("main").unwrap();
        assert_eq!(catalog.len(), 4);
        assert_eq!(
            catalog
                .iter()
                .map(|app| app.app_id.as_str())
                .collect::<Vec<_>>(),
            vec![
                "ticket-details",
                "board",
                "document-viewer",
                "directed-graph"
            ]
        );
        assert!(
            catalog
                .iter()
                .all(|app| app.resource_uri.starts_with("ui://main/mcp/apps/"))
        );
        assert!(catalog.iter().all(|app| !app.projection_refs.is_empty()));
        assert!(catalog.iter().all(|app| !app.read_tools.is_empty()));

        let board = catalog.iter().find(|app| app.app_id == "board").unwrap();
        assert!(board.write_tools.contains(&"tickets_update".to_string()));
        assert!(
            board
                .elicitation_schema_refs
                .contains(&"schema:tickets.update".to_string())
        );
    }

    #[test]
    fn surface_app_catalog_includes_remaining_apps() {
        let catalog = surface_app_catalog("main").unwrap();
        assert_eq!(catalog.len(), 16);
        assert_eq!(
            catalog.last().map(|app| app.app_id.as_str()),
            Some("diagram-editor")
        );

        let changes = catalog
            .iter()
            .find(|app| app.app_id == "changes-inbox")
            .unwrap();
        assert_eq!(changes.staleness_policy, StalenessPolicy::RequireRefresh);
        assert!(
            changes
                .write_tools
                .contains(&"conflicts.resolve".to_string())
        );

        let search = catalog
            .iter()
            .find(|app| app.app_id == "search-palette")
            .unwrap();
        assert!(search.write_tools.is_empty());
        assert_eq!(search.read_tools, vec!["substrate_search".to_string()]);
    }

    #[test]
    fn meeting_memory_surface_catalog_defines_profile_apps() {
        let catalog = meeting_memory_surface_catalog("main").unwrap();
        assert_eq!(catalog.len(), 6);
        assert_eq!(
            catalog
                .iter()
                .map(|app| app.app_id.as_str())
                .collect::<Vec<_>>(),
            vec![
                "meeting-details",
                "memory-graph",
                "extraction-review",
                "meeting-search",
                "import-coverage",
                "access-audit"
            ]
        );

        let review = catalog
            .iter()
            .find(|app| app.app_id == "extraction-review")
            .unwrap();
        assert_eq!(review.staleness_policy, StalenessPolicy::RequireRefresh);
        assert!(
            review
                .write_tools
                .contains(&"meetings.entity_merge".to_string())
        );

        let audit = catalog
            .iter()
            .find(|app| app.app_id == "access-audit")
            .unwrap();
        assert!(audit.write_tools.is_empty());
        assert!(
            audit
                .elicitation_schema_refs
                .contains(&"schema:meetings.access-export".to_string())
        );
    }
}
