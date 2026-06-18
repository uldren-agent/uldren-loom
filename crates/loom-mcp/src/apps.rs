//! MCP App metadata and URI helpers.
//!
//! The first Loom Apps profile is intentionally file-backed: an app lives under
//! `/.loom/facets/mcp/apps/{app-name}/` and is exposed only when both `index.html` and `_meta.md` are
//! present.
//!
//! Licensed under BUSL-1.1.

use loom_core::error::{LoomError, Result};

pub const APP_ROOT: &str = ".loom/facets/mcp/apps";
pub const INDEX_FILE: &str = "index.html";
pub const META_FILE: &str = "_meta.md";
pub const APP_MIME: &str = "text/html;profile=mcp-app";
pub const INTERNAL_VCS_APP: &str = "internal/vcs";
pub const INTERNAL_DECISIONS_APP: &str = "internal/decisions";
pub const DOCUMENT_VIEWER_APP: &str = "document-viewer";
pub const DIRECTED_GRAPH_APP: &str = "directed-graph";
pub const MIND_MAP_APP: &str = "mind-map";
pub const CANVAS_APP: &str = "canvas";
pub const DIAGRAM_EDITOR_APP: &str = "diagram-editor";
pub const TICKET_DETAILS_APP: &str = "ticket-details";
pub const BOARD_APP: &str = "board";
pub const ROADMAP_APP: &str = "roadmap";
pub const SPRINT_PLANNER_APP: &str = "sprint-planner";
pub const BACKLOG_TRIAGE_APP: &str = "backlog-triage";
pub const DASHBOARDS_APP: &str = "dashboards";
pub const CHAT_CHANNEL_APP: &str = "chat-channel";
pub const CHAT_THREAD_APP: &str = "chat-thread";
pub const CHAT_TASKS_APP: &str = "chat-tasks";
pub const CHAT_PRESENCE_APP: &str = "chat-presence";
pub const CHAT_HANDOFFS_APP: &str = "chat-handoffs";
pub const DRIVE_BROWSER_APP: &str = "drive-browser";
pub const DRIVE_PREVIEW_APP: &str = "drive-preview";
pub const DRIVE_SHARING_APP: &str = "drive-sharing";
pub const DRIVE_CONFLICTS_APP: &str = "drive-conflicts";
pub const DRIVE_RETENTION_APP: &str = "drive-retention";
pub const MEETING_DETAILS_APP: &str = "meeting-details";
pub const MEMORY_GRAPH_APP: &str = "memory-graph";
pub const EXTRACTION_REVIEW_APP: &str = "extraction-review";
pub const MEETING_SEARCH_APP: &str = "meeting-search";
pub const IMPORT_COVERAGE_APP: &str = "import-coverage";
pub const ACCESS_AUDIT_APP: &str = "access-audit";
/// Every binary-sourced internal app, in listing order.
pub const INTERNAL_APPS: &[&str] = &[
    INTERNAL_VCS_APP,
    INTERNAL_DECISIONS_APP,
    TICKET_DETAILS_APP,
    BOARD_APP,
    ROADMAP_APP,
    SPRINT_PLANNER_APP,
    BACKLOG_TRIAGE_APP,
    DASHBOARDS_APP,
    CHAT_CHANNEL_APP,
    CHAT_THREAD_APP,
    CHAT_TASKS_APP,
    CHAT_PRESENCE_APP,
    CHAT_HANDOFFS_APP,
    DRIVE_BROWSER_APP,
    DRIVE_PREVIEW_APP,
    DRIVE_SHARING_APP,
    DRIVE_CONFLICTS_APP,
    DRIVE_RETENTION_APP,
    MEETING_DETAILS_APP,
    MEMORY_GRAPH_APP,
    EXTRACTION_REVIEW_APP,
    MEETING_SEARCH_APP,
    IMPORT_COVERAGE_APP,
    ACCESS_AUDIT_APP,
    DOCUMENT_VIEWER_APP,
    DIRECTED_GRAPH_APP,
    MIND_MAP_APP,
    CANVAS_APP,
    DIAGRAM_EDITOR_APP,
];
pub const INTERNAL_APP_ROOT: &str = ".loom/facets/mcp/apps/internal";
pub const INTERNAL_VCS_APP_ROOT: &str = ".loom/facets/mcp/apps/internal/vcs";
pub const INTERNAL_VCS_INDEX_PATH: &str = ".loom/facets/mcp/apps/internal/vcs/index.html";
pub const INTERNAL_VCS_META_PATH: &str = ".loom/facets/mcp/apps/internal/vcs/_meta.md";
pub const INTERNAL_DECISIONS_APP_ROOT: &str = ".loom/facets/mcp/apps/internal/decisions";
pub const INTERNAL_DECISIONS_INDEX_PATH: &str =
    ".loom/facets/mcp/apps/internal/decisions/index.html";
pub const INTERNAL_DECISIONS_META_PATH: &str = ".loom/facets/mcp/apps/internal/decisions/_meta.md";
pub const DOCUMENT_VIEWER_INDEX_PATH: &str = ".loom/facets/mcp/apps/document-viewer/index.html";
pub const DOCUMENT_VIEWER_META_PATH: &str = ".loom/facets/mcp/apps/document-viewer/_meta.md";
pub const DIRECTED_GRAPH_INDEX_PATH: &str = ".loom/facets/mcp/apps/directed-graph/index.html";
pub const DIRECTED_GRAPH_META_PATH: &str = ".loom/facets/mcp/apps/directed-graph/_meta.md";
pub const MIND_MAP_INDEX_PATH: &str = ".loom/facets/mcp/apps/mind-map/index.html";
pub const MIND_MAP_META_PATH: &str = ".loom/facets/mcp/apps/mind-map/_meta.md";
pub const CANVAS_INDEX_PATH: &str = ".loom/facets/mcp/apps/canvas/index.html";
pub const CANVAS_META_PATH: &str = ".loom/facets/mcp/apps/canvas/_meta.md";
pub const DIAGRAM_EDITOR_INDEX_PATH: &str = ".loom/facets/mcp/apps/diagram-editor/index.html";
pub const DIAGRAM_EDITOR_META_PATH: &str = ".loom/facets/mcp/apps/diagram-editor/_meta.md";
pub const TICKET_DETAILS_INDEX_PATH: &str = ".loom/facets/mcp/apps/ticket-details/index.html";
pub const TICKET_DETAILS_META_PATH: &str = ".loom/facets/mcp/apps/ticket-details/_meta.md";
pub const BOARD_INDEX_PATH: &str = ".loom/facets/mcp/apps/board/index.html";
pub const BOARD_META_PATH: &str = ".loom/facets/mcp/apps/board/_meta.md";
pub const ROADMAP_INDEX_PATH: &str = ".loom/facets/mcp/apps/roadmap/index.html";
pub const ROADMAP_META_PATH: &str = ".loom/facets/mcp/apps/roadmap/_meta.md";
pub const SPRINT_PLANNER_INDEX_PATH: &str = ".loom/facets/mcp/apps/sprint-planner/index.html";
pub const SPRINT_PLANNER_META_PATH: &str = ".loom/facets/mcp/apps/sprint-planner/_meta.md";
pub const BACKLOG_TRIAGE_INDEX_PATH: &str = ".loom/facets/mcp/apps/backlog-triage/index.html";
pub const BACKLOG_TRIAGE_META_PATH: &str = ".loom/facets/mcp/apps/backlog-triage/_meta.md";
pub const DASHBOARDS_INDEX_PATH: &str = ".loom/facets/mcp/apps/dashboards/index.html";
pub const DASHBOARDS_META_PATH: &str = ".loom/facets/mcp/apps/dashboards/_meta.md";
pub const CHAT_CHANNEL_INDEX_PATH: &str = ".loom/facets/mcp/apps/chat-channel/index.html";
pub const CHAT_CHANNEL_META_PATH: &str = ".loom/facets/mcp/apps/chat-channel/_meta.md";
pub const CHAT_THREAD_INDEX_PATH: &str = ".loom/facets/mcp/apps/chat-thread/index.html";
pub const CHAT_THREAD_META_PATH: &str = ".loom/facets/mcp/apps/chat-thread/_meta.md";
pub const CHAT_TASKS_INDEX_PATH: &str = ".loom/facets/mcp/apps/chat-tasks/index.html";
pub const CHAT_TASKS_META_PATH: &str = ".loom/facets/mcp/apps/chat-tasks/_meta.md";
pub const CHAT_PRESENCE_INDEX_PATH: &str = ".loom/facets/mcp/apps/chat-presence/index.html";
pub const CHAT_PRESENCE_META_PATH: &str = ".loom/facets/mcp/apps/chat-presence/_meta.md";
pub const CHAT_HANDOFFS_INDEX_PATH: &str = ".loom/facets/mcp/apps/chat-handoffs/index.html";
pub const CHAT_HANDOFFS_META_PATH: &str = ".loom/facets/mcp/apps/chat-handoffs/_meta.md";
pub const DRIVE_BROWSER_INDEX_PATH: &str = ".loom/facets/mcp/apps/drive-browser/index.html";
pub const DRIVE_BROWSER_META_PATH: &str = ".loom/facets/mcp/apps/drive-browser/_meta.md";
pub const DRIVE_PREVIEW_INDEX_PATH: &str = ".loom/facets/mcp/apps/drive-preview/index.html";
pub const DRIVE_PREVIEW_META_PATH: &str = ".loom/facets/mcp/apps/drive-preview/_meta.md";
pub const DRIVE_SHARING_INDEX_PATH: &str = ".loom/facets/mcp/apps/drive-sharing/index.html";
pub const DRIVE_SHARING_META_PATH: &str = ".loom/facets/mcp/apps/drive-sharing/_meta.md";
pub const DRIVE_CONFLICTS_INDEX_PATH: &str = ".loom/facets/mcp/apps/drive-conflicts/index.html";
pub const DRIVE_CONFLICTS_META_PATH: &str = ".loom/facets/mcp/apps/drive-conflicts/_meta.md";
pub const DRIVE_RETENTION_INDEX_PATH: &str = ".loom/facets/mcp/apps/drive-retention/index.html";
pub const DRIVE_RETENTION_META_PATH: &str = ".loom/facets/mcp/apps/drive-retention/_meta.md";
pub const MEETING_DETAILS_INDEX_PATH: &str = ".loom/facets/mcp/apps/meeting-details/index.html";
pub const MEETING_DETAILS_META_PATH: &str = ".loom/facets/mcp/apps/meeting-details/_meta.md";
pub const MEMORY_GRAPH_INDEX_PATH: &str = ".loom/facets/mcp/apps/memory-graph/index.html";
pub const MEMORY_GRAPH_META_PATH: &str = ".loom/facets/mcp/apps/memory-graph/_meta.md";
pub const EXTRACTION_REVIEW_INDEX_PATH: &str = ".loom/facets/mcp/apps/extraction-review/index.html";
pub const EXTRACTION_REVIEW_META_PATH: &str = ".loom/facets/mcp/apps/extraction-review/_meta.md";
pub const MEETING_SEARCH_INDEX_PATH: &str = ".loom/facets/mcp/apps/meeting-search/index.html";
pub const MEETING_SEARCH_META_PATH: &str = ".loom/facets/mcp/apps/meeting-search/_meta.md";
pub const IMPORT_COVERAGE_INDEX_PATH: &str = ".loom/facets/mcp/apps/import-coverage/index.html";
pub const IMPORT_COVERAGE_META_PATH: &str = ".loom/facets/mcp/apps/import-coverage/_meta.md";
pub const ACCESS_AUDIT_INDEX_PATH: &str = ".loom/facets/mcp/apps/access-audit/index.html";
pub const ACCESS_AUDIT_META_PATH: &str = ".loom/facets/mcp/apps/access-audit/_meta.md";

const INTERNAL_VCS_INDEX_HTML: &str = include_str!("internal_apps/vcs/index.html");
const INTERNAL_VCS_META_MD: &str = include_str!("internal_apps/vcs/_meta.md");
const INTERNAL_DECISIONS_INDEX_HTML: &str = include_str!("internal_apps/decisions/index.html");
const INTERNAL_DECISIONS_META_MD: &str = include_str!("internal_apps/decisions/_meta.md");
const TICKET_PLANNING_INDEX_HTML: &str = include_str!("internal_apps/ticket_planning/index.html");
const TICKET_DETAILS_META_MD: &str = include_str!("internal_apps/ticket_details/_meta.md");
const BOARD_META_MD: &str = include_str!("internal_apps/board/_meta.md");
const ROADMAP_META_MD: &str = include_str!("internal_apps/roadmap/_meta.md");
const SPRINT_PLANNER_META_MD: &str = include_str!("internal_apps/sprint_planner/_meta.md");
const BACKLOG_TRIAGE_META_MD: &str = include_str!("internal_apps/backlog_triage/_meta.md");
const DASHBOARDS_META_MD: &str = include_str!("internal_apps/dashboards/_meta.md");
const CHAT_INDEX_HTML: &str = include_str!("internal_apps/chat/index.html");
const CHAT_CHANNEL_META_MD: &str = include_str!("internal_apps/chat_channel/_meta.md");
const CHAT_THREAD_META_MD: &str = include_str!("internal_apps/chat_thread/_meta.md");
const CHAT_TASKS_META_MD: &str = include_str!("internal_apps/chat_tasks/_meta.md");
const CHAT_PRESENCE_META_MD: &str = include_str!("internal_apps/chat_presence/_meta.md");
const CHAT_HANDOFFS_META_MD: &str = include_str!("internal_apps/chat_handoffs/_meta.md");
const DRIVE_INDEX_HTML: &str = include_str!("internal_apps/drive/index.html");
const DRIVE_BROWSER_META_MD: &str = include_str!("internal_apps/drive_browser/_meta.md");
const DRIVE_PREVIEW_META_MD: &str = include_str!("internal_apps/drive_preview/_meta.md");
const DRIVE_SHARING_META_MD: &str = include_str!("internal_apps/drive_sharing/_meta.md");
const DRIVE_CONFLICTS_META_MD: &str = include_str!("internal_apps/drive_conflicts/_meta.md");
const DRIVE_RETENTION_META_MD: &str = include_str!("internal_apps/drive_retention/_meta.md");
const MEETINGS_INDEX_HTML: &str = include_str!("internal_apps/meetings/index.html");
const MEETING_DETAILS_META_MD: &str = include_str!("internal_apps/meeting_details/_meta.md");
const MEMORY_GRAPH_META_MD: &str = include_str!("internal_apps/memory_graph/_meta.md");
const EXTRACTION_REVIEW_META_MD: &str = include_str!("internal_apps/extraction_review/_meta.md");
const MEETING_SEARCH_META_MD: &str = include_str!("internal_apps/meeting_search/_meta.md");
const IMPORT_COVERAGE_META_MD: &str = include_str!("internal_apps/import_coverage/_meta.md");
const ACCESS_AUDIT_META_MD: &str = include_str!("internal_apps/access_audit/_meta.md");
const DOCUMENT_VIEWER_INDEX_HTML: &str = include_str!("internal_apps/document_viewer/index.html");
const DOCUMENT_VIEWER_META_MD: &str = include_str!("internal_apps/document_viewer/_meta.md");
const DIRECTED_GRAPH_INDEX_HTML: &str = include_str!("internal_apps/directed_graph/index.html");
const DIRECTED_GRAPH_META_MD: &str = include_str!("internal_apps/directed_graph/_meta.md");
const MIND_MAP_INDEX_HTML: &str = include_str!("internal_apps/mind_map/index.html");
const MIND_MAP_META_MD: &str = include_str!("internal_apps/mind_map/_meta.md");
const CANVAS_INDEX_HTML: &str = include_str!("internal_apps/canvas/index.html");
const CANVAS_META_MD: &str = include_str!("internal_apps/canvas/_meta.md");
const DIAGRAM_EDITOR_INDEX_HTML: &str = include_str!("internal_apps/diagram_editor/index.html");
const DIAGRAM_EDITOR_META_MD: &str = include_str!("internal_apps/diagram_editor/_meta.md");
const APP_SHELL_CSS: &str = include_str!("internal_apps/app_shell.css");

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppConformanceVector {
    pub app: &'static str,
    pub meta_md: Option<&'static [u8]>,
    pub index_html: Option<&'static [u8]>,
    pub expect_valid: bool,
    pub expect_status: &'static str,
}

pub const APP_CONFORMANCE_VECTORS: &[AppConformanceVector] = &[
    AppConformanceVector {
        app: "map",
        meta_md: Some(
            br#"---
name: Map
description: Interactive map app
mimeType: text/html;profile=mcp-app
ui.prefersBorder: true
ui.csp.resourceDomains:
  - https://cdn.example.com
ui.permissions.geolocation: true
loom.processing: static
---
"#,
        ),
        index_html: Some(b"<!doctype html><html><body>Map</body></html>"),
        expect_valid: true,
        expect_status: "valid",
    },
    AppConformanceVector {
        app: "dashboard",
        meta_md: Some(
            br#"---
name: Dashboard
description: Template-backed dashboard app
mimeType: text/html;profile=mcp-app
loom.processing: templates
---
"#,
        ),
        index_html: Some(br#"<!doctype html><html><body>{{ loom.program(name="dashboard/load") }}</body></html>"#),
        expect_valid: true,
        expect_status: "valid",
    },
    AppConformanceVector {
        app: "broken",
        meta_md: Some(b"not front matter"),
        index_html: Some(b"<!doctype html><html><body>Broken</body></html>"),
        expect_valid: false,
        expect_status: "malformed_meta",
    },
    AppConformanceVector {
        app: "missing-index",
        meta_md: Some(b"---\nname: Missing\n---\n"),
        index_html: None,
        expect_valid: false,
        expect_status: "missing_index",
    },
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppTarget {
    pub workspace: String,
    pub app: String,
    pub instance: Option<String>,
    pub internal: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct AppCsp {
    pub connect_domains: Vec<String>,
    pub resource_domains: Vec<String>,
    pub frame_domains: Vec<String>,
    pub base_uri_domains: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct AppPermissions {
    pub camera: bool,
    pub microphone: bool,
    pub geolocation: bool,
    pub clipboard_write: bool,
}

/// Who may invoke the app's launcher tool: the MCP Apps `_meta.ui.visibility` surfaces, declared in
/// `_meta.md` as a list (`ui.visibility: [model, app]`). `model` = the agent may see and call it;
/// `app` = the embedded view may call it over the bridge. Kept as an open list so new surfaces can be
/// introduced without a shape change. Absent/empty defaults to [`DEFAULT_VISIBILITY`].
pub const DEFAULT_VISIBILITY: &[&str] = &["model", "app"];
/// Recognized visibility surfaces, validated on parse.
pub const VISIBILITY_SURFACES: &[&str] = &["model", "app"];

/// The display modes an app supports, declared in `_meta.md` as `ui.availableDisplayModes`
/// (a list of `inline` | `fullscreen` | `pip`). Surfaced to the app so its `ui/initialize`
/// handshake can advertise them. Defaults to `["inline"]` when absent.
pub const DEFAULT_DISPLAY_MODES: &[&str] = &["inline"];

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct AppMeta {
    pub name: String,
    pub description: Option<String>,
    #[serde(rename = "mimeType")]
    pub mime_type: String,
    pub csp: AppCsp,
    pub permissions: AppPermissions,
    pub domain: Option<String>,
    pub prefers_border: Option<bool>,
    pub processing: String,
    /// Raw declared visibility surfaces; empty means "use the default". Read via
    /// [`AppMeta::visibility_surfaces`].
    pub visibility: Vec<String>,
    /// Raw declared display modes; empty means "use the default". Read via [`AppMeta::display_modes`].
    #[serde(rename = "availableDisplayModes")]
    pub available_display_modes: Vec<String>,
}

impl AppMeta {
    /// The effective visibility surfaces, applying the [`DEFAULT_VISIBILITY`] default when none are
    /// declared.
    pub fn visibility_surfaces(&self) -> Vec<String> {
        if self.visibility.is_empty() {
            DEFAULT_VISIBILITY.iter().map(|s| s.to_string()).collect()
        } else {
            self.visibility.clone()
        }
    }

    /// Whether the agent (model) may see and call the app's launcher tool.
    pub fn model_visible(&self) -> bool {
        self.visibility_surfaces().iter().any(|s| s == "model")
    }

    /// The effective display modes, applying the `["inline"]` default when none are declared.
    pub fn display_modes(&self) -> Vec<String> {
        if self.available_display_modes.is_empty() {
            DEFAULT_DISPLAY_MODES
                .iter()
                .map(|s| s.to_string())
                .collect()
        } else {
            self.available_display_modes.clone()
        }
    }
}

impl Default for AppMeta {
    fn default() -> Self {
        Self {
            name: String::new(),
            description: None,
            mime_type: APP_MIME.to_string(),
            csp: AppCsp::default(),
            permissions: AppPermissions::default(),
            domain: None,
            prefers_border: None,
            processing: "static".to_string(),
            visibility: Vec::new(),
            available_display_modes: Vec::new(),
        }
    }
}

pub fn app_dir(app: &str) -> String {
    format!("{APP_ROOT}/{app}")
}

pub fn index_path(app: &str) -> String {
    format!("{}/{INDEX_FILE}", app_dir(app))
}

pub fn meta_path(app: &str) -> String {
    format!("{}/{META_FILE}", app_dir(app))
}

pub fn app_file_path(app: &str, path: &str) -> Result<String> {
    validate_app_name(app)?;
    validate_app_file_path(path)?;
    Ok(format!("{}/{path}", app_dir(app)))
}

pub fn app_parent_dir(app: &str, path: &str) -> Result<String> {
    let file = app_file_path(app, path)?;
    Ok(file
        .rsplit_once('/')
        .map_or_else(|| app_dir(app), |(parent, _)| parent.to_string()))
}

pub fn app_uri(workspace: &str, app: &str, workspace_bound: bool) -> String {
    if workspace_bound {
        format!("ui://mcp/apps/{app}")
    } else {
        format!("ui://{workspace}/mcp/apps/{app}")
    }
}

pub fn app_uri_with_instance(
    workspace: &str,
    app: &str,
    instance: Option<&str>,
    workspace_bound: bool,
) -> String {
    let base = app_uri(workspace, app, workspace_bound);
    match instance {
        Some(instance) => format!("{base}/{instance}"),
        None => base,
    }
}

pub fn is_internal_app(app: &str) -> bool {
    INTERNAL_APPS.contains(&app)
}

pub fn split_internal_app_instance(tail: &str) -> Option<(&'static str, Option<String>)> {
    if let Some(app) = INTERNAL_APPS.iter().find(|app| **app == tail) {
        return Some((app, None));
    }
    if let Some(instance) = tail.strip_prefix("internal/decisions/") {
        validate_app_instance_segment(instance).ok()?;
        return Some((INTERNAL_DECISIONS_APP, Some(instance.to_string())));
    }
    if let Some(ticket_id) = tail.strip_prefix("ticket-details/ticket/") {
        validate_app_instance_segment(ticket_id).ok()?;
        return Some((TICKET_DETAILS_APP, Some(format!("ticket/{ticket_id}"))));
    }
    if let Some(channel_id) = tail.strip_prefix("chat-channel/channel/") {
        validate_app_instance_segment(channel_id).ok()?;
        return Some((CHAT_CHANNEL_APP, Some(format!("channel/{channel_id}"))));
    }
    if let Some(channel_id) = tail.strip_prefix("chat-tasks/channel/") {
        validate_app_instance_segment(channel_id).ok()?;
        return Some((CHAT_TASKS_APP, Some(format!("channel/{channel_id}"))));
    }
    if let Some(channel_id) = tail.strip_prefix("chat-presence/channel/") {
        validate_app_instance_segment(channel_id).ok()?;
        return Some((CHAT_PRESENCE_APP, Some(format!("channel/{channel_id}"))));
    }
    if let Some(channel_id) = tail.strip_prefix("chat-handoffs/channel/") {
        validate_app_instance_segment(channel_id).ok()?;
        return Some((CHAT_HANDOFFS_APP, Some(format!("channel/{channel_id}"))));
    }
    if let Some(rest) = tail.strip_prefix("chat-thread/channel/") {
        let (channel_id, thread_id) = rest.split_once("/thread/")?;
        validate_app_instance_segment(channel_id).ok()?;
        validate_app_instance_segment(thread_id).ok()?;
        return Some((
            CHAT_THREAD_APP,
            Some(format!("channel/{channel_id}/thread/{thread_id}")),
        ));
    }
    if let Some(folder_id) = tail.strip_prefix("drive-browser/folder/") {
        validate_app_instance_segment(folder_id).ok()?;
        return Some((DRIVE_BROWSER_APP, Some(format!("folder/{folder_id}"))));
    }
    if let Some(file_id) = tail.strip_prefix("drive-preview/file/") {
        validate_app_instance_segment(file_id).ok()?;
        return Some((DRIVE_PREVIEW_APP, Some(format!("file/{file_id}"))));
    }
    if let Some(meeting_id) = tail.strip_prefix("meeting-details/meeting/") {
        validate_app_instance_path(meeting_id).ok()?;
        return Some((MEETING_DETAILS_APP, Some(format!("meeting/{meeting_id}"))));
    }
    if let Some(page_id) = tail.strip_prefix("document-viewer/page/") {
        validate_app_instance_segment(page_id).ok()?;
        return Some((DOCUMENT_VIEWER_APP, Some(format!("page/{page_id}"))));
    }
    for app in [MIND_MAP_APP, CANVAS_APP, DIAGRAM_EDITOR_APP] {
        if let Some(structure_id) = tail.strip_prefix(&format!("{app}/structure/")) {
            validate_app_instance_segment(structure_id).ok()?;
            return Some((app, Some(format!("structure/{structure_id}"))));
        }
    }
    None
}

pub fn validate_app_instance_segment(instance: &str) -> Result<()> {
    if instance.is_empty()
        || instance.starts_with('.')
        || !instance
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    {
        return Err(LoomError::invalid(format!(
            "invalid MCP app instance {instance:?}"
        )));
    }
    Ok(())
}

fn validate_app_instance_path(instance: &str) -> Result<()> {
    if instance.is_empty() {
        return Err(LoomError::invalid(format!(
            "invalid MCP app instance {instance:?}"
        )));
    }
    for segment in instance.split('/') {
        validate_app_instance_segment(segment)?;
    }
    Ok(())
}

pub fn internal_app_meta(app: &str) -> Option<Result<AppMeta>> {
    match app {
        INTERNAL_VCS_APP => Some(parse_meta("vcs", INTERNAL_VCS_META_MD)),
        INTERNAL_DECISIONS_APP => Some(parse_meta("decisions", INTERNAL_DECISIONS_META_MD)),
        TICKET_DETAILS_APP => Some(parse_meta(TICKET_DETAILS_APP, TICKET_DETAILS_META_MD)),
        BOARD_APP => Some(parse_meta(BOARD_APP, BOARD_META_MD)),
        ROADMAP_APP => Some(parse_meta(ROADMAP_APP, ROADMAP_META_MD)),
        SPRINT_PLANNER_APP => Some(parse_meta(SPRINT_PLANNER_APP, SPRINT_PLANNER_META_MD)),
        BACKLOG_TRIAGE_APP => Some(parse_meta(BACKLOG_TRIAGE_APP, BACKLOG_TRIAGE_META_MD)),
        DASHBOARDS_APP => Some(parse_meta(DASHBOARDS_APP, DASHBOARDS_META_MD)),
        CHAT_CHANNEL_APP => Some(parse_meta(CHAT_CHANNEL_APP, CHAT_CHANNEL_META_MD)),
        CHAT_THREAD_APP => Some(parse_meta(CHAT_THREAD_APP, CHAT_THREAD_META_MD)),
        CHAT_TASKS_APP => Some(parse_meta(CHAT_TASKS_APP, CHAT_TASKS_META_MD)),
        CHAT_PRESENCE_APP => Some(parse_meta(CHAT_PRESENCE_APP, CHAT_PRESENCE_META_MD)),
        CHAT_HANDOFFS_APP => Some(parse_meta(CHAT_HANDOFFS_APP, CHAT_HANDOFFS_META_MD)),
        DRIVE_BROWSER_APP => Some(parse_meta(DRIVE_BROWSER_APP, DRIVE_BROWSER_META_MD)),
        DRIVE_PREVIEW_APP => Some(parse_meta(DRIVE_PREVIEW_APP, DRIVE_PREVIEW_META_MD)),
        DRIVE_SHARING_APP => Some(parse_meta(DRIVE_SHARING_APP, DRIVE_SHARING_META_MD)),
        DRIVE_CONFLICTS_APP => Some(parse_meta(DRIVE_CONFLICTS_APP, DRIVE_CONFLICTS_META_MD)),
        DRIVE_RETENTION_APP => Some(parse_meta(DRIVE_RETENTION_APP, DRIVE_RETENTION_META_MD)),
        MEETING_DETAILS_APP => Some(parse_meta(MEETING_DETAILS_APP, MEETING_DETAILS_META_MD)),
        MEMORY_GRAPH_APP => Some(parse_meta(MEMORY_GRAPH_APP, MEMORY_GRAPH_META_MD)),
        EXTRACTION_REVIEW_APP => Some(parse_meta(EXTRACTION_REVIEW_APP, EXTRACTION_REVIEW_META_MD)),
        MEETING_SEARCH_APP => Some(parse_meta(MEETING_SEARCH_APP, MEETING_SEARCH_META_MD)),
        IMPORT_COVERAGE_APP => Some(parse_meta(IMPORT_COVERAGE_APP, IMPORT_COVERAGE_META_MD)),
        ACCESS_AUDIT_APP => Some(parse_meta(ACCESS_AUDIT_APP, ACCESS_AUDIT_META_MD)),
        DOCUMENT_VIEWER_APP => Some(parse_meta(DOCUMENT_VIEWER_APP, DOCUMENT_VIEWER_META_MD)),
        DIRECTED_GRAPH_APP => Some(parse_meta(DIRECTED_GRAPH_APP, DIRECTED_GRAPH_META_MD)),
        MIND_MAP_APP => Some(parse_meta(MIND_MAP_APP, MIND_MAP_META_MD)),
        CANVAS_APP => Some(parse_meta(CANVAS_APP, CANVAS_META_MD)),
        DIAGRAM_EDITOR_APP => Some(parse_meta(DIAGRAM_EDITOR_APP, DIAGRAM_EDITOR_META_MD)),
        _ => None,
    }
}

pub fn internal_app_html(app: &str) -> Option<(&'static str, Result<AppMeta>)> {
    match app {
        INTERNAL_VCS_APP => Some((
            INTERNAL_VCS_INDEX_HTML,
            parse_meta("vcs", INTERNAL_VCS_META_MD),
        )),
        INTERNAL_DECISIONS_APP => Some((
            INTERNAL_DECISIONS_INDEX_HTML,
            parse_meta("decisions", INTERNAL_DECISIONS_META_MD),
        )),
        TICKET_DETAILS_APP => Some((
            TICKET_PLANNING_INDEX_HTML,
            parse_meta(TICKET_DETAILS_APP, TICKET_DETAILS_META_MD),
        )),
        BOARD_APP => Some((
            TICKET_PLANNING_INDEX_HTML,
            parse_meta(BOARD_APP, BOARD_META_MD),
        )),
        ROADMAP_APP => Some((
            TICKET_PLANNING_INDEX_HTML,
            parse_meta(ROADMAP_APP, ROADMAP_META_MD),
        )),
        SPRINT_PLANNER_APP => Some((
            TICKET_PLANNING_INDEX_HTML,
            parse_meta(SPRINT_PLANNER_APP, SPRINT_PLANNER_META_MD),
        )),
        BACKLOG_TRIAGE_APP => Some((
            TICKET_PLANNING_INDEX_HTML,
            parse_meta(BACKLOG_TRIAGE_APP, BACKLOG_TRIAGE_META_MD),
        )),
        DASHBOARDS_APP => Some((
            TICKET_PLANNING_INDEX_HTML,
            parse_meta(DASHBOARDS_APP, DASHBOARDS_META_MD),
        )),
        CHAT_CHANNEL_APP => Some((
            CHAT_INDEX_HTML,
            parse_meta(CHAT_CHANNEL_APP, CHAT_CHANNEL_META_MD),
        )),
        CHAT_THREAD_APP => Some((
            CHAT_INDEX_HTML,
            parse_meta(CHAT_THREAD_APP, CHAT_THREAD_META_MD),
        )),
        CHAT_TASKS_APP => Some((
            CHAT_INDEX_HTML,
            parse_meta(CHAT_TASKS_APP, CHAT_TASKS_META_MD),
        )),
        CHAT_PRESENCE_APP => Some((
            CHAT_INDEX_HTML,
            parse_meta(CHAT_PRESENCE_APP, CHAT_PRESENCE_META_MD),
        )),
        CHAT_HANDOFFS_APP => Some((
            CHAT_INDEX_HTML,
            parse_meta(CHAT_HANDOFFS_APP, CHAT_HANDOFFS_META_MD),
        )),
        DRIVE_BROWSER_APP => Some((
            DRIVE_INDEX_HTML,
            parse_meta(DRIVE_BROWSER_APP, DRIVE_BROWSER_META_MD),
        )),
        DRIVE_PREVIEW_APP => Some((
            DRIVE_INDEX_HTML,
            parse_meta(DRIVE_PREVIEW_APP, DRIVE_PREVIEW_META_MD),
        )),
        DRIVE_SHARING_APP => Some((
            DRIVE_INDEX_HTML,
            parse_meta(DRIVE_SHARING_APP, DRIVE_SHARING_META_MD),
        )),
        DRIVE_CONFLICTS_APP => Some((
            DRIVE_INDEX_HTML,
            parse_meta(DRIVE_CONFLICTS_APP, DRIVE_CONFLICTS_META_MD),
        )),
        DRIVE_RETENTION_APP => Some((
            DRIVE_INDEX_HTML,
            parse_meta(DRIVE_RETENTION_APP, DRIVE_RETENTION_META_MD),
        )),
        MEETING_DETAILS_APP => Some((
            MEETINGS_INDEX_HTML,
            parse_meta(MEETING_DETAILS_APP, MEETING_DETAILS_META_MD),
        )),
        MEMORY_GRAPH_APP => Some((
            MEETINGS_INDEX_HTML,
            parse_meta(MEMORY_GRAPH_APP, MEMORY_GRAPH_META_MD),
        )),
        EXTRACTION_REVIEW_APP => Some((
            MEETINGS_INDEX_HTML,
            parse_meta(EXTRACTION_REVIEW_APP, EXTRACTION_REVIEW_META_MD),
        )),
        MEETING_SEARCH_APP => Some((
            MEETINGS_INDEX_HTML,
            parse_meta(MEETING_SEARCH_APP, MEETING_SEARCH_META_MD),
        )),
        IMPORT_COVERAGE_APP => Some((
            MEETINGS_INDEX_HTML,
            parse_meta(IMPORT_COVERAGE_APP, IMPORT_COVERAGE_META_MD),
        )),
        ACCESS_AUDIT_APP => Some((
            MEETINGS_INDEX_HTML,
            parse_meta(ACCESS_AUDIT_APP, ACCESS_AUDIT_META_MD),
        )),
        DOCUMENT_VIEWER_APP => Some((
            DOCUMENT_VIEWER_INDEX_HTML,
            parse_meta(DOCUMENT_VIEWER_APP, DOCUMENT_VIEWER_META_MD),
        )),
        DIRECTED_GRAPH_APP => Some((
            DIRECTED_GRAPH_INDEX_HTML,
            parse_meta(DIRECTED_GRAPH_APP, DIRECTED_GRAPH_META_MD),
        )),
        MIND_MAP_APP => Some((
            MIND_MAP_INDEX_HTML,
            parse_meta(MIND_MAP_APP, MIND_MAP_META_MD),
        )),
        CANVAS_APP => Some((CANVAS_INDEX_HTML, parse_meta(CANVAS_APP, CANVAS_META_MD))),
        DIAGRAM_EDITOR_APP => Some((
            DIAGRAM_EDITOR_INDEX_HTML,
            parse_meta(DIAGRAM_EDITOR_APP, DIAGRAM_EDITOR_META_MD),
        )),
        _ => None,
    }
}

pub fn internal_app_file(app: &str, path: &str) -> Option<&'static [u8]> {
    match (app, path) {
        (INTERNAL_VCS_APP, INDEX_FILE) => Some(INTERNAL_VCS_INDEX_HTML.as_bytes()),
        (INTERNAL_VCS_APP, META_FILE) => Some(INTERNAL_VCS_META_MD.as_bytes()),
        (INTERNAL_DECISIONS_APP, INDEX_FILE) => Some(INTERNAL_DECISIONS_INDEX_HTML.as_bytes()),
        (INTERNAL_DECISIONS_APP, META_FILE) => Some(INTERNAL_DECISIONS_META_MD.as_bytes()),
        (TICKET_DETAILS_APP, INDEX_FILE) => Some(TICKET_PLANNING_INDEX_HTML.as_bytes()),
        (TICKET_DETAILS_APP, META_FILE) => Some(TICKET_DETAILS_META_MD.as_bytes()),
        (BOARD_APP, INDEX_FILE) => Some(TICKET_PLANNING_INDEX_HTML.as_bytes()),
        (BOARD_APP, META_FILE) => Some(BOARD_META_MD.as_bytes()),
        (ROADMAP_APP, INDEX_FILE) => Some(TICKET_PLANNING_INDEX_HTML.as_bytes()),
        (ROADMAP_APP, META_FILE) => Some(ROADMAP_META_MD.as_bytes()),
        (SPRINT_PLANNER_APP, INDEX_FILE) => Some(TICKET_PLANNING_INDEX_HTML.as_bytes()),
        (SPRINT_PLANNER_APP, META_FILE) => Some(SPRINT_PLANNER_META_MD.as_bytes()),
        (BACKLOG_TRIAGE_APP, INDEX_FILE) => Some(TICKET_PLANNING_INDEX_HTML.as_bytes()),
        (BACKLOG_TRIAGE_APP, META_FILE) => Some(BACKLOG_TRIAGE_META_MD.as_bytes()),
        (DASHBOARDS_APP, INDEX_FILE) => Some(TICKET_PLANNING_INDEX_HTML.as_bytes()),
        (DASHBOARDS_APP, META_FILE) => Some(DASHBOARDS_META_MD.as_bytes()),
        (CHAT_CHANNEL_APP, INDEX_FILE) => Some(CHAT_INDEX_HTML.as_bytes()),
        (CHAT_CHANNEL_APP, META_FILE) => Some(CHAT_CHANNEL_META_MD.as_bytes()),
        (CHAT_THREAD_APP, INDEX_FILE) => Some(CHAT_INDEX_HTML.as_bytes()),
        (CHAT_THREAD_APP, META_FILE) => Some(CHAT_THREAD_META_MD.as_bytes()),
        (CHAT_TASKS_APP, INDEX_FILE) => Some(CHAT_INDEX_HTML.as_bytes()),
        (CHAT_TASKS_APP, META_FILE) => Some(CHAT_TASKS_META_MD.as_bytes()),
        (CHAT_PRESENCE_APP, INDEX_FILE) => Some(CHAT_INDEX_HTML.as_bytes()),
        (CHAT_PRESENCE_APP, META_FILE) => Some(CHAT_PRESENCE_META_MD.as_bytes()),
        (CHAT_HANDOFFS_APP, INDEX_FILE) => Some(CHAT_INDEX_HTML.as_bytes()),
        (CHAT_HANDOFFS_APP, META_FILE) => Some(CHAT_HANDOFFS_META_MD.as_bytes()),
        (DRIVE_BROWSER_APP, INDEX_FILE) => Some(DRIVE_INDEX_HTML.as_bytes()),
        (DRIVE_BROWSER_APP, META_FILE) => Some(DRIVE_BROWSER_META_MD.as_bytes()),
        (DRIVE_PREVIEW_APP, INDEX_FILE) => Some(DRIVE_INDEX_HTML.as_bytes()),
        (DRIVE_PREVIEW_APP, META_FILE) => Some(DRIVE_PREVIEW_META_MD.as_bytes()),
        (DRIVE_SHARING_APP, INDEX_FILE) => Some(DRIVE_INDEX_HTML.as_bytes()),
        (DRIVE_SHARING_APP, META_FILE) => Some(DRIVE_SHARING_META_MD.as_bytes()),
        (DRIVE_CONFLICTS_APP, INDEX_FILE) => Some(DRIVE_INDEX_HTML.as_bytes()),
        (DRIVE_CONFLICTS_APP, META_FILE) => Some(DRIVE_CONFLICTS_META_MD.as_bytes()),
        (DRIVE_RETENTION_APP, INDEX_FILE) => Some(DRIVE_INDEX_HTML.as_bytes()),
        (DRIVE_RETENTION_APP, META_FILE) => Some(DRIVE_RETENTION_META_MD.as_bytes()),
        (MEETING_DETAILS_APP, INDEX_FILE) => Some(MEETINGS_INDEX_HTML.as_bytes()),
        (MEETING_DETAILS_APP, META_FILE) => Some(MEETING_DETAILS_META_MD.as_bytes()),
        (MEMORY_GRAPH_APP, INDEX_FILE) => Some(MEETINGS_INDEX_HTML.as_bytes()),
        (MEMORY_GRAPH_APP, META_FILE) => Some(MEMORY_GRAPH_META_MD.as_bytes()),
        (EXTRACTION_REVIEW_APP, INDEX_FILE) => Some(MEETINGS_INDEX_HTML.as_bytes()),
        (EXTRACTION_REVIEW_APP, META_FILE) => Some(EXTRACTION_REVIEW_META_MD.as_bytes()),
        (MEETING_SEARCH_APP, INDEX_FILE) => Some(MEETINGS_INDEX_HTML.as_bytes()),
        (MEETING_SEARCH_APP, META_FILE) => Some(MEETING_SEARCH_META_MD.as_bytes()),
        (IMPORT_COVERAGE_APP, INDEX_FILE) => Some(MEETINGS_INDEX_HTML.as_bytes()),
        (IMPORT_COVERAGE_APP, META_FILE) => Some(IMPORT_COVERAGE_META_MD.as_bytes()),
        (ACCESS_AUDIT_APP, INDEX_FILE) => Some(MEETINGS_INDEX_HTML.as_bytes()),
        (ACCESS_AUDIT_APP, META_FILE) => Some(ACCESS_AUDIT_META_MD.as_bytes()),
        (DOCUMENT_VIEWER_APP, INDEX_FILE) => Some(DOCUMENT_VIEWER_INDEX_HTML.as_bytes()),
        (DOCUMENT_VIEWER_APP, META_FILE) => Some(DOCUMENT_VIEWER_META_MD.as_bytes()),
        (DIRECTED_GRAPH_APP, INDEX_FILE) => Some(DIRECTED_GRAPH_INDEX_HTML.as_bytes()),
        (DIRECTED_GRAPH_APP, META_FILE) => Some(DIRECTED_GRAPH_META_MD.as_bytes()),
        (MIND_MAP_APP, INDEX_FILE) => Some(MIND_MAP_INDEX_HTML.as_bytes()),
        (MIND_MAP_APP, META_FILE) => Some(MIND_MAP_META_MD.as_bytes()),
        (CANVAS_APP, INDEX_FILE) => Some(CANVAS_INDEX_HTML.as_bytes()),
        (CANVAS_APP, META_FILE) => Some(CANVAS_META_MD.as_bytes()),
        (DIAGRAM_EDITOR_APP, INDEX_FILE) => Some(DIAGRAM_EDITOR_INDEX_HTML.as_bytes()),
        (DIAGRAM_EDITOR_APP, META_FILE) => Some(DIAGRAM_EDITOR_META_MD.as_bytes()),
        _ => None,
    }
}

pub fn internal_app_source_path(app: &str) -> Option<&'static str> {
    match app {
        INTERNAL_VCS_APP => Some(INTERNAL_VCS_INDEX_PATH),
        INTERNAL_DECISIONS_APP => Some(INTERNAL_DECISIONS_INDEX_PATH),
        TICKET_DETAILS_APP => Some(TICKET_DETAILS_INDEX_PATH),
        BOARD_APP => Some(BOARD_INDEX_PATH),
        ROADMAP_APP => Some(ROADMAP_INDEX_PATH),
        SPRINT_PLANNER_APP => Some(SPRINT_PLANNER_INDEX_PATH),
        BACKLOG_TRIAGE_APP => Some(BACKLOG_TRIAGE_INDEX_PATH),
        DASHBOARDS_APP => Some(DASHBOARDS_INDEX_PATH),
        CHAT_CHANNEL_APP => Some(CHAT_CHANNEL_INDEX_PATH),
        CHAT_THREAD_APP => Some(CHAT_THREAD_INDEX_PATH),
        CHAT_TASKS_APP => Some(CHAT_TASKS_INDEX_PATH),
        CHAT_PRESENCE_APP => Some(CHAT_PRESENCE_INDEX_PATH),
        CHAT_HANDOFFS_APP => Some(CHAT_HANDOFFS_INDEX_PATH),
        DRIVE_BROWSER_APP => Some(DRIVE_BROWSER_INDEX_PATH),
        DRIVE_PREVIEW_APP => Some(DRIVE_PREVIEW_INDEX_PATH),
        DRIVE_SHARING_APP => Some(DRIVE_SHARING_INDEX_PATH),
        DRIVE_CONFLICTS_APP => Some(DRIVE_CONFLICTS_INDEX_PATH),
        DRIVE_RETENTION_APP => Some(DRIVE_RETENTION_INDEX_PATH),
        MEETING_DETAILS_APP => Some(MEETING_DETAILS_INDEX_PATH),
        MEMORY_GRAPH_APP => Some(MEMORY_GRAPH_INDEX_PATH),
        EXTRACTION_REVIEW_APP => Some(EXTRACTION_REVIEW_INDEX_PATH),
        MEETING_SEARCH_APP => Some(MEETING_SEARCH_INDEX_PATH),
        IMPORT_COVERAGE_APP => Some(IMPORT_COVERAGE_INDEX_PATH),
        ACCESS_AUDIT_APP => Some(ACCESS_AUDIT_INDEX_PATH),
        DOCUMENT_VIEWER_APP => Some(DOCUMENT_VIEWER_INDEX_PATH),
        DIRECTED_GRAPH_APP => Some(DIRECTED_GRAPH_INDEX_PATH),
        MIND_MAP_APP => Some(MIND_MAP_INDEX_PATH),
        CANVAS_APP => Some(CANVAS_INDEX_PATH),
        DIAGRAM_EDITOR_APP => Some(DIAGRAM_EDITOR_INDEX_PATH),
        _ => None,
    }
}

pub fn app_shell_css() -> &'static str {
    APP_SHELL_CSS
}

pub fn validate_app_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.starts_with('.')
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    {
        return Err(LoomError::invalid(format!("invalid MCP app name {name:?}")));
    }
    Ok(())
}

pub fn validate_app_file_path(path: &str) -> Result<()> {
    if path.is_empty() || path.starts_with('/') || path.ends_with('/') {
        return Err(LoomError::invalid(format!(
            "invalid MCP app file path {path:?}"
        )));
    }
    for segment in path.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." || segment.starts_with('.') {
            return Err(LoomError::invalid(format!(
                "invalid MCP app file path {path:?}"
            )));
        }
    }
    Ok(())
}

pub fn parse_meta(app: &str, text: &str) -> Result<AppMeta> {
    validate_app_name(app)?;
    let front = front_matter(text)?;
    let mut meta = AppMeta::default();
    let mut current_list: Option<String> = None;
    for raw in front.lines() {
        let line = raw.trim_end();
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let trimmed = line.trim_start();
        if let Some(item) = trimmed.strip_prefix("- ") {
            let key = current_list
                .as_deref()
                .ok_or_else(|| LoomError::invalid("metadata list item without key"))?;
            push_list_value(&mut meta, key, item.trim())?;
            continue;
        }
        let (key, value) = line
            .split_once(':')
            .ok_or_else(|| LoomError::invalid(format!("invalid metadata line {line:?}")))?;
        let key = key.trim();
        let value = value.trim();
        if value.is_empty() {
            current_list = Some(key.to_string());
            continue;
        }
        current_list = None;
        set_value(&mut meta, key, value)?;
    }
    if meta.name.trim().is_empty() {
        return Err(LoomError::invalid("app metadata requires name"));
    }
    Ok(meta)
}

fn front_matter(text: &str) -> Result<&str> {
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some("---") {
        return Err(LoomError::invalid(
            "app metadata must start with YAML front matter",
        ));
    }
    let start = text
        .find('\n')
        .ok_or_else(|| LoomError::invalid("app metadata front matter is not closed"))?
        + 1;
    let rest = &text[start..];
    let end = rest
        .lines()
        .scan(0usize, |offset, line| {
            let cur = *offset;
            *offset += line.len() + 1;
            Some((cur, line))
        })
        .find_map(|(offset, line)| (line.trim() == "---").then_some(offset))
        .ok_or_else(|| LoomError::invalid("app metadata front matter is not closed"))?;
    Ok(&rest[..end])
}

fn set_value(meta: &mut AppMeta, key: &str, value: &str) -> Result<()> {
    match key {
        "name" => meta.name = unquote(value).to_string(),
        "description" => meta.description = Some(unquote(value).to_string()),
        "mimeType" => {
            let mime = unquote(value);
            if mime != APP_MIME {
                return Err(LoomError::invalid(format!(
                    "unsupported app mimeType {mime:?}"
                )));
            }
            meta.mime_type = mime.to_string();
        }
        "ui.domain" => meta.domain = Some(unquote(value).to_string()),
        "ui.prefersBorder" => meta.prefers_border = Some(parse_bool(value)?),
        "loom.processing" => {
            let processing = unquote(value);
            match processing {
                "static" | "templates" => meta.processing = processing.to_string(),
                other => {
                    return Err(LoomError::invalid(format!(
                        "unsupported loom.processing {other:?}"
                    )));
                }
            }
        }
        "ui.permissions.camera" => meta.permissions.camera = parse_bool(value)?,
        "ui.permissions.microphone" => meta.permissions.microphone = parse_bool(value)?,
        "ui.permissions.geolocation" => meta.permissions.geolocation = parse_bool(value)?,
        "ui.permissions.clipboardWrite" => meta.permissions.clipboard_write = parse_bool(value)?,
        "ui.visibility"
        | "ui.csp.connectDomains"
        | "ui.csp.resourceDomains"
        | "ui.csp.frameDomains"
        | "ui.csp.baseUriDomains"
        | "ui.availableDisplayModes" => {
            for item in parse_list(value) {
                push_list_value(meta, key, &item)?;
            }
        }
        _ => {
            return Err(LoomError::invalid(format!(
                "unknown app metadata key {key:?}"
            )));
        }
    }
    Ok(())
}

fn push_list_value(meta: &mut AppMeta, key: &str, value: &str) -> Result<()> {
    let value = unquote(value);
    if value.is_empty() {
        return Err(LoomError::invalid(format!("empty value for {key}")));
    }
    let target = match key {
        "ui.csp.connectDomains" => &mut meta.csp.connect_domains,
        "ui.csp.resourceDomains" => &mut meta.csp.resource_domains,
        "ui.csp.frameDomains" => &mut meta.csp.frame_domains,
        "ui.csp.baseUriDomains" => &mut meta.csp.base_uri_domains,
        "ui.visibility" => {
            if !VISIBILITY_SURFACES.contains(&value) {
                return Err(LoomError::invalid(format!(
                    "invalid ui.visibility value {value:?} (expected model|app)"
                )));
            }
            &mut meta.visibility
        }
        "ui.availableDisplayModes" => {
            match value {
                "inline" | "fullscreen" | "pip" => {}
                other => {
                    return Err(LoomError::invalid(format!(
                        "invalid ui.availableDisplayModes value {other:?} (expected inline|fullscreen|pip)"
                    )));
                }
            }
            &mut meta.available_display_modes
        }
        _ => return Err(LoomError::invalid(format!("{key:?} is not a list key"))),
    };
    target.push(value.to_string());
    Ok(())
}

fn parse_bool(value: &str) -> Result<bool> {
    match unquote(value) {
        "true" => Ok(true),
        "false" => Ok(false),
        other => Err(LoomError::invalid(format!("invalid boolean {other:?}"))),
    }
}

fn parse_list(value: &str) -> Vec<String> {
    let value = unquote(value).trim();
    let inner = value
        .strip_prefix('[')
        .and_then(|v| v.strip_suffix(']'))
        .unwrap_or(value);
    if inner.trim().is_empty() {
        return Vec::new();
    }
    inner
        .split(',')
        .map(|item| unquote(item.trim()).to_string())
        .collect()
}

fn unquote(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
        .unwrap_or(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_front_matter_subset() {
        let meta = parse_meta(
            "dashboard",
            r#"---
name: Dashboard
description: "A useful app"
mimeType: text/html;profile=mcp-app
ui.prefersBorder: true
ui.csp.connectDomains:
  - https://api.example.com
ui.csp.resourceDomains: [https://cdn.example.com]
ui.permissions.geolocation: true
loom.processing: templates
---

Body text.
"#,
        )
        .unwrap();
        assert_eq!(meta.name, "Dashboard");
        assert_eq!(meta.description.as_deref(), Some("A useful app"));
        assert_eq!(meta.mime_type, APP_MIME);
        assert_eq!(meta.prefers_border, Some(true));
        assert_eq!(meta.csp.connect_domains, vec!["https://api.example.com"]);
        assert_eq!(meta.csp.resource_domains, vec!["https://cdn.example.com"]);
        assert!(meta.permissions.geolocation);
        assert_eq!(meta.processing, "templates");
        // Defaults when the fields are absent.
        assert!(meta.visibility.is_empty());
        assert_eq!(
            meta.visibility_surfaces(),
            vec!["model".to_string(), "app".to_string()]
        );
        assert!(meta.model_visible());
        assert!(meta.available_display_modes.is_empty());
        assert_eq!(meta.display_modes(), vec!["inline".to_string()]);
    }

    #[test]
    fn parses_visibility_and_display_modes() {
        // Block list form, app-only visibility.
        let meta = parse_meta(
            "panel",
            "---\nname: Panel\nui.visibility:\n  - app\nui.availableDisplayModes:\n  - inline\n  - fullscreen\n---\n",
        )
        .unwrap();
        assert_eq!(meta.visibility_surfaces(), vec!["app".to_string()]);
        assert!(!meta.model_visible());
        assert_eq!(
            meta.display_modes(),
            vec!["inline".to_string(), "fullscreen".to_string()]
        );

        // Inline list form with both surfaces, plus display modes.
        let meta = parse_meta(
            "panel",
            "---\nname: Panel\nui.visibility: [model, app]\nui.availableDisplayModes: [fullscreen, pip]\n---\n",
        )
        .unwrap();
        assert_eq!(
            meta.visibility_surfaces(),
            vec!["model".to_string(), "app".to_string()]
        );
        assert!(meta.model_visible());
        assert_eq!(
            meta.display_modes(),
            vec!["fullscreen".to_string(), "pip".to_string()]
        );
    }

    #[test]
    fn ticket_planning_app_uses_app_bridge_and_normalized_statuses() {
        assert!(TICKET_PLANNING_INDEX_HTML.contains("apps_call_tool"));
        assert!(TICKET_PLANNING_INDEX_HTML.contains("waiting_for_review"));
        assert!(TICKET_PLANNING_INDEX_HTML.contains("feedback_available"));
        assert!(!TICKET_PLANNING_INDEX_HTML.contains("<option value=\"todo\">"));
        assert!(!TICKET_PLANNING_INDEX_HTML.contains("<option value=\"done\">"));
        assert!(!TICKET_PLANNING_INDEX_HTML.contains("Update requested"));
    }

    #[test]
    fn rejects_invalid_app_names_and_unknown_keys() {
        assert!(parse_meta("../bad", "---\nname: Bad\n---\n").is_err());
        assert!(parse_meta("bad", "---\nunknown: value\n---\n").is_err());
        assert!(parse_meta("bad", "---\nname: Bad\nloom.processing: raw\n---\n").is_err());
        // Invalid enum / display-mode values are rejected.
        assert!(parse_meta("bad", "---\nname: Bad\nui.visibility: agent\n---\n").is_err());
        assert!(
            parse_meta(
                "bad",
                "---\nname: Bad\nui.availableDisplayModes: [inline, giant]\n---\n"
            )
            .is_err()
        );
    }
}
