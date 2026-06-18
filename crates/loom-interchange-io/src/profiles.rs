use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};

use loom_core::{Algo, Code, Digest, Loom};
use loom_interchange::{FidelityIssue, FidelitySeverity, ImportReport, ImportReportInput};
use loom_store::FileStore;
use loom_substrate::body::{Block, BlockKind, Body, TextRun};
use loom_substrate::order_token::{first_token, insert_between};
use loom_types::{Result, WorkspaceId};
use quick_xml::Reader as XmlReader;
use quick_xml::XmlVersion;
use quick_xml::events::Event as XmlEvent;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TicketImportFieldPolicy {
    Strict,
    Infer,
}

impl TicketImportFieldPolicy {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "strict" => Ok(Self::Strict),
            "infer" => Ok(Self::Infer),
            other => Err(loom_types::LoomError::invalid(format!(
                "unsupported ticket import field policy {other:?}; supported values: strict, infer"
            ))),
        }
    }
}

#[derive(Debug, serde::Deserialize)]
pub struct RedmineImportSnapshot {
    #[serde(default)]
    pub source_scope: Option<String>,
    #[serde(default)]
    pub projects: Vec<RedmineProject>,
    #[serde(default)]
    pub issues: Vec<RedmineIssue>,
    #[serde(default)]
    pub wiki_pages: Vec<RedmineWikiPage>,
    #[serde(default)]
    pub time_entries: Vec<RedmineTimeEntry>,
}

#[derive(Debug, serde::Deserialize)]
pub struct RedmineProject {
    pub id: serde_json::Value,
    #[serde(default)]
    pub identifier: Option<String>,
    pub name: String,
    #[serde(default)]
    pub key_prefix: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub status: Option<serde_json::Value>,
    #[serde(default)]
    pub is_public: Option<serde_json::Value>,
    #[serde(default)]
    pub parent: Option<serde_json::Value>,
    #[serde(default)]
    pub default_version: Option<serde_json::Value>,
    #[serde(default)]
    pub default_assignee: Option<serde_json::Value>,
    #[serde(default)]
    pub created_on: Option<String>,
    #[serde(default)]
    pub updated_on: Option<String>,
    #[serde(default)]
    pub trackers: Vec<serde_json::Value>,
    #[serde(default)]
    pub issue_categories: Vec<serde_json::Value>,
    #[serde(default)]
    pub enabled_modules: Vec<serde_json::Value>,
    #[serde(default)]
    pub time_entry_activities: Vec<serde_json::Value>,
    #[serde(default)]
    pub issue_custom_fields: Vec<serde_json::Value>,
}

#[derive(Debug, serde::Deserialize)]
pub struct RedmineIssue {
    pub id: serde_json::Value,
    #[serde(default)]
    pub project_id: Option<serde_json::Value>,
    #[serde(default)]
    pub project_identifier: Option<String>,
    #[serde(default)]
    pub tracker: Option<String>,
    pub subject: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub priority: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub assigned_to: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub created_on: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub updated_on: Option<String>,
    #[serde(default)]
    pub start_date: Option<String>,
    #[serde(default)]
    pub due_date: Option<String>,
    #[serde(default)]
    pub closed_on: Option<String>,
    #[serde(default)]
    pub done_ratio: Option<serde_json::Value>,
    #[serde(default)]
    pub estimated_hours: Option<serde_json::Value>,
    #[serde(default)]
    pub fixed_version: Option<String>,
    #[serde(default)]
    pub affected_version: Option<String>,
    #[serde(default)]
    pub affected_versions: Vec<serde_json::Value>,
    #[serde(default)]
    pub parent_issue_id: Option<serde_json::Value>,
    #[serde(default)]
    pub is_private: Option<serde_json::Value>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub custom_fields: Option<serde_json::Value>,
    #[serde(default)]
    pub policy_labels: Vec<String>,
    #[serde(default)]
    pub journals: Vec<serde_json::Value>,
    #[serde(default)]
    pub comments: Vec<serde_json::Value>,
    #[serde(default)]
    pub watchers: Vec<serde_json::Value>,
    #[serde(default)]
    pub attachments: Vec<serde_json::Value>,
    #[serde(default)]
    pub time_entries: Vec<serde_json::Value>,
    #[serde(default)]
    pub relations: Vec<serde_json::Value>,
    #[serde(default)]
    pub children: Vec<serde_json::Value>,
    #[serde(default)]
    pub changesets: Vec<serde_json::Value>,
    #[serde(default)]
    pub allowed_statuses: Vec<serde_json::Value>,
}

#[derive(Debug, serde::Deserialize)]
pub struct RedmineWikiPage {
    pub id: serde_json::Value,
    pub title: String,
    #[serde(default)]
    pub project_id: Option<serde_json::Value>,
    #[serde(default)]
    pub project_identifier: Option<String>,
    #[serde(default)]
    pub space_id: Option<String>,
    #[serde(default)]
    pub page_id: Option<String>,
    #[serde(default)]
    pub parent_page_id: Option<String>,
    #[serde(default)]
    pub parent_title: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub markdown: Option<String>,
    #[serde(default)]
    pub version: Option<serde_json::Value>,
    #[serde(default)]
    pub author: Option<serde_json::Value>,
    #[serde(default)]
    pub comments: Option<String>,
    #[serde(default)]
    pub created_on: Option<String>,
    #[serde(default)]
    pub updated_on: Option<String>,
    #[serde(default)]
    pub attachments: Vec<serde_json::Value>,
}

#[derive(Debug, serde::Deserialize)]
pub struct RedmineTimeEntry {
    pub id: serde_json::Value,
    #[serde(default)]
    pub issue_id: Option<serde_json::Value>,
    #[serde(default)]
    pub project_id: Option<serde_json::Value>,
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default)]
    pub activity: Option<String>,
    #[serde(default)]
    pub hours: Option<serde_json::Value>,
    #[serde(default)]
    pub comments: Option<String>,
    #[serde(default)]
    pub spent_on: Option<String>,
    #[serde(default)]
    pub created_on: Option<String>,
    #[serde(default)]
    pub updated_on: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
pub struct AsanaImportSnapshot {
    #[serde(default)]
    pub source_scope: Option<String>,
    #[serde(default)]
    pub projects: Vec<AsanaProject>,
    #[serde(default)]
    pub tasks: Vec<AsanaTask>,
}

#[derive(Debug, serde::Deserialize)]
pub struct AsanaProject {
    pub gid: serde_json::Value,
    pub name: String,
    #[serde(default)]
    pub key_prefix: Option<String>,
    #[serde(default)]
    pub archived: Option<bool>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub modified_at: Option<String>,
    #[serde(default)]
    pub start_on: Option<String>,
    #[serde(default)]
    pub due_on: Option<String>,
    #[serde(default)]
    pub default_view: Option<String>,
    #[serde(default)]
    pub permalink_url: Option<String>,
    #[serde(default)]
    pub workspace: Option<serde_json::Value>,
    #[serde(default)]
    pub team: Option<serde_json::Value>,
    #[serde(default)]
    pub owner: Option<serde_json::Value>,
    #[serde(default)]
    pub members: Vec<serde_json::Value>,
    #[serde(default)]
    pub followers: Vec<serde_json::Value>,
    #[serde(default)]
    pub current_status: Option<serde_json::Value>,
    #[serde(default)]
    pub custom_field_settings: Vec<serde_json::Value>,
}

#[derive(Debug, serde::Deserialize)]
pub struct AsanaTask {
    pub gid: serde_json::Value,
    pub name: String,
    #[serde(default)]
    pub project_gid: Option<serde_json::Value>,
    #[serde(default)]
    pub project_id: Option<serde_json::Value>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub html_notes: Option<String>,
    #[serde(default)]
    pub resource_subtype: Option<String>,
    #[serde(default)]
    pub approval_status: Option<String>,
    #[serde(default)]
    pub assignee_status: Option<String>,
    #[serde(default)]
    pub completed: Option<bool>,
    #[serde(default)]
    pub completed_at: Option<String>,
    #[serde(default)]
    pub completed_by: Option<serde_json::Value>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub created_by: Option<serde_json::Value>,
    #[serde(default)]
    pub modified_at: Option<String>,
    #[serde(default)]
    pub assigned_by: Option<serde_json::Value>,
    #[serde(default)]
    pub assignee: Option<serde_json::Value>,
    #[serde(default)]
    pub assignee_section: Option<serde_json::Value>,
    #[serde(default)]
    pub workspace: Option<serde_json::Value>,
    #[serde(default)]
    pub parent: Option<serde_json::Value>,
    #[serde(default)]
    pub external: Option<serde_json::Value>,
    #[serde(default)]
    pub due_on: Option<String>,
    #[serde(default)]
    pub due_at: Option<String>,
    #[serde(default)]
    pub start_on: Option<String>,
    #[serde(default)]
    pub start_at: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub custom_fields: Option<serde_json::Value>,
    #[serde(default)]
    pub dependencies: Vec<serde_json::Value>,
    #[serde(default)]
    pub dependents: Vec<serde_json::Value>,
    #[serde(default)]
    pub memberships: Vec<serde_json::Value>,
    #[serde(default)]
    pub followers: Vec<serde_json::Value>,
    #[serde(default)]
    pub likes: Vec<serde_json::Value>,
    #[serde(default)]
    pub liked: Option<bool>,
    #[serde(default)]
    pub num_likes: Option<i64>,
    #[serde(default)]
    pub num_subtasks: Option<i64>,
    #[serde(default)]
    pub actual_time_minutes: Option<i64>,
    #[serde(default)]
    pub is_rendered_as_separator: Option<bool>,
    #[serde(default)]
    pub subtasks: Vec<serde_json::Value>,
    #[serde(default)]
    pub stories: Vec<serde_json::Value>,
    #[serde(default)]
    pub attachments: Vec<serde_json::Value>,
    #[serde(default)]
    pub portfolios: Vec<serde_json::Value>,
    #[serde(default)]
    pub goals: Vec<serde_json::Value>,
}

#[derive(Debug, serde::Deserialize)]
pub struct JiraImportSnapshot {
    #[serde(default)]
    pub source_scope: Option<String>,
    #[serde(default)]
    pub projects: Vec<JiraProject>,
    #[serde(default)]
    pub issues: Vec<JiraIssue>,
}

#[derive(Debug, serde::Deserialize)]
pub struct JiraProject {
    pub id: serde_json::Value,
    pub key: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default, alias = "projectTypeKey")]
    pub project_type_key: Option<String>,
    #[serde(default)]
    pub style: Option<String>,
    #[serde(default)]
    pub simplified: Option<bool>,
    #[serde(default)]
    pub archived: Option<bool>,
    #[serde(default)]
    pub deleted: Option<bool>,
    #[serde(default)]
    pub private: Option<bool>,
    #[serde(default)]
    pub self_url: Option<String>,
    #[serde(default, alias = "avatarUrls")]
    pub avatar_urls: Option<serde_json::Value>,
    #[serde(default)]
    pub lead: Option<serde_json::Value>,
    #[serde(default, alias = "projectCategory")]
    pub project_category: Option<serde_json::Value>,
    #[serde(default)]
    pub insight: Option<serde_json::Value>,
    #[serde(default, alias = "issueTypes")]
    pub issue_types: Vec<serde_json::Value>,
    #[serde(default)]
    pub components: Vec<serde_json::Value>,
    #[serde(default)]
    pub versions: Vec<serde_json::Value>,
    #[serde(default)]
    pub roles: Option<serde_json::Value>,
}

#[derive(Debug, serde::Deserialize)]
pub struct JiraIssue {
    pub id: serde_json::Value,
    pub key: String,
    #[serde(default)]
    pub project_id: Option<serde_json::Value>,
    #[serde(default)]
    pub project_key: Option<String>,
    #[serde(default)]
    pub issue_type: Option<String>,
    pub summary: String,
    #[serde(default)]
    pub description: Option<serde_json::Value>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub status_category: Option<String>,
    #[serde(default)]
    pub priority: Option<String>,
    #[serde(default)]
    pub resolution: Option<serde_json::Value>,
    #[serde(default)]
    pub resolution_date: Option<String>,
    #[serde(default)]
    pub assignee: Option<serde_json::Value>,
    #[serde(default)]
    pub reporter: Option<serde_json::Value>,
    #[serde(default)]
    pub creator: Option<serde_json::Value>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub due_date: Option<String>,
    #[serde(default)]
    pub environment: Option<serde_json::Value>,
    #[serde(default)]
    pub parent: Option<serde_json::Value>,
    #[serde(default)]
    pub security: Option<serde_json::Value>,
    #[serde(default)]
    pub votes: Option<serde_json::Value>,
    #[serde(default)]
    pub watches: Option<serde_json::Value>,
    #[serde(default)]
    pub sprint: Option<serde_json::Value>,
    #[serde(default)]
    pub transitions: Vec<serde_json::Value>,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub custom_fields: Option<serde_json::Value>,
    #[serde(default)]
    pub components: Vec<serde_json::Value>,
    #[serde(default, alias = "fixVersions")]
    pub fix_versions: Vec<serde_json::Value>,
    #[serde(default, alias = "versions")]
    pub affected_versions: Vec<serde_json::Value>,
    #[serde(default, alias = "issuelinks")]
    pub issue_links: Vec<serde_json::Value>,
    #[serde(default)]
    pub subtasks: Vec<serde_json::Value>,
    #[serde(default)]
    pub properties: Option<serde_json::Value>,
    #[serde(default)]
    pub development: Option<serde_json::Value>,
    #[serde(default)]
    pub changelog: Option<serde_json::Value>,
    #[serde(default)]
    pub comments: Vec<serde_json::Value>,
    #[serde(default)]
    pub attachments: Vec<serde_json::Value>,
    #[serde(default)]
    pub worklog: Vec<serde_json::Value>,
}

#[derive(Debug, serde::Deserialize)]
pub struct ConfluenceImportSnapshot {
    #[serde(default)]
    pub source_scope: Option<String>,
    #[serde(default)]
    pub spaces: Vec<ConfluenceSpace>,
    #[serde(default)]
    pub pages: Vec<ConfluencePage>,
}

#[derive(Debug, serde::Deserialize)]
pub struct ConfluenceSpace {
    pub id: String,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<serde_json::Value>,
    #[serde(default)]
    pub r#type: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub homepage_id: Option<String>,
    #[serde(default)]
    pub author_id: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub links: Option<serde_json::Value>,
    #[serde(default)]
    pub labels: Vec<serde_json::Value>,
    #[serde(default)]
    pub properties: Vec<serde_json::Value>,
    #[serde(default)]
    pub permissions: Vec<serde_json::Value>,
}

#[derive(Debug, serde::Deserialize)]
pub struct ConfluencePage {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub space_id: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub version: Option<serde_json::Value>,
    #[serde(default)]
    pub author_id: Option<String>,
    #[serde(default)]
    pub owner_id: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub links: Option<serde_json::Value>,
    #[serde(default)]
    pub ancestors: Vec<serde_json::Value>,
    #[serde(default)]
    pub descendants: Vec<serde_json::Value>,
    #[serde(default)]
    pub labels: Vec<serde_json::Value>,
    #[serde(default)]
    pub properties: Vec<serde_json::Value>,
    #[serde(default)]
    pub restrictions: Vec<serde_json::Value>,
    #[serde(default)]
    pub parent_page_id: Option<String>,
    #[serde(default)]
    pub storage_xhtml: Option<String>,
    #[serde(default)]
    pub adf_json: Option<serde_json::Value>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub markdown: Option<String>,
    #[serde(default)]
    pub attachments: Vec<serde_json::Value>,
    #[serde(default)]
    pub comments: Vec<serde_json::Value>,
}

struct MarkdownImportItem {
    path: PathBuf,
    relative_path: String,
    page_id: String,
    title: String,
    unsupported_fields: Vec<&'static str>,
}

pub fn import_redmine_bytes(
    loom: &mut Loom<FileStore>,
    ns: WorkspaceId,
    workspace_id: &str,
    source_path: &str,
    bytes: &[u8],
    dry_run: bool,
) -> Result<ImportReport> {
    import_redmine_bytes_with_field_policy(
        loom,
        ns,
        workspace_id,
        source_path,
        bytes,
        dry_run,
        TicketImportFieldPolicy::Strict,
    )
}

pub fn import_redmine_bytes_with_field_policy(
    loom: &mut Loom<FileStore>,
    ns: WorkspaceId,
    workspace_id: &str,
    source_path: &str,
    bytes: &[u8],
    dry_run: bool,
    field_policy: TicketImportFieldPolicy,
) -> Result<ImportReport> {
    let parsed = parse_redmine_import_snapshot(bytes)?;
    import_redmine_snapshot(
        loom,
        ns,
        workspace_id,
        source_path,
        bytes.len() as u64,
        parsed,
        dry_run,
        field_policy,
    )
}

pub fn import_redmine_snapshot(
    loom: &mut Loom<FileStore>,
    ns: WorkspaceId,
    workspace_id: &str,
    fallback_source_scope: &str,
    bytes_in: u64,
    parsed: RedmineImportSnapshot,
    dry_run: bool,
    field_policy: TicketImportFieldPolicy,
) -> Result<ImportReport> {
    let source_scope = parsed
        .source_scope
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(fallback_source_scope);
    let wiki_spaces = redmine_wiki_space_ids(&parsed.wiki_pages)?;
    let time_entries_by_issue = redmine_time_entries_by_issue(&parsed.time_entries)?;
    let mut report = ImportReport::new(ImportReportInput {
        profile: "redmine",
        source_scope,
        commit: None,
        objects_added: 0,
        bytes_in,
        bytes_stored: 0,
        rows_imported: 0,
        skipped: 0,
        operations_planned: (parsed.projects.len()
            + parsed.issues.len()
            + parsed.wiki_pages.len()
            + wiki_spaces.len()) as u64,
        operations_applied: 0,
        dry_run,
    })?;
    for project in &parsed.projects {
        add_redmine_project_fidelity_issues(&mut report, project)?;
    }
    for page in &parsed.wiki_pages {
        add_redmine_wiki_page_fidelity_issues(&mut report, page)?;
    }
    if dry_run {
        return Ok(report);
    }

    for project in &parsed.projects {
        match loom_tickets::create_project(
            loom,
            ns,
            workspace_id,
            &redmine_project_id(project)?,
            &redmine_project_key_prefix(project)?,
            &project.name,
            None,
        ) {
            Ok(_) => {
                report.operations_applied += 1;
                report.rows_imported += 1;
            }
            Err(error) if error.code == Code::AlreadyExists => {
                report.skipped += 1;
            }
            Err(error) => return Err(error),
        }
    }
    for issue in &parsed.issues {
        let issue_source_id = redmine_source_id("issue", &issue.id)?;
        let fields = redmine_issue_fields(
            issue,
            time_entries_by_issue
                .get(&issue_source_id)
                .map(Vec::as_slice)
                .unwrap_or(&[]),
        )?;
        match import_ticket_with_field_policy(
            loom,
            ns,
            field_policy,
            loom_tickets::TicketCreateRequest {
                workspace_id,
                project_id: &redmine_issue_project_id(issue)?,
                ticket_type: redmine_ticket_type(issue.tracker.as_deref()),
                external_source: Some("redmine"),
                external_id: Some(&issue_source_id),
                fields: &fields,
                policy_labels: &issue.policy_labels,
                expected_root: None,
            },
        ) {
            Ok(_) => {
                report.operations_applied += 1;
                report.rows_imported += 1;
            }
            Err(error) if error.code == Code::AlreadyExists => {
                report.skipped += 1;
            }
            Err(error) => return Err(error),
        }
    }
    for space in &wiki_spaces {
        match loom_pages::create_space(loom, ns, workspace_id, space, space, None) {
            Ok(_) => {
                report.operations_applied += 1;
            }
            Err(error) if error.code == Code::AlreadyExists => {
                report.skipped += 1;
            }
            Err(error) => return Err(error),
        }
    }
    for page in &parsed.wiki_pages {
        let markdown = redmine_wiki_page_markdown(page);
        let body = markdown_body(&markdown)?;
        let body_bytes = body.encode()?;
        apply_page_import(
            loom,
            ns,
            workspace_id,
            &redmine_wiki_page_space_id(page)?,
            &redmine_wiki_page_id(page)?,
            &page.title,
            page.parent_page_id.as_deref(),
            body_bytes,
            &mut report,
        )?;
        report.bytes_stored += markdown.len() as u64;
    }
    Ok(report)
}

#[derive(Debug, Clone, Default)]
struct RedmineXmlNode {
    name: String,
    attrs: BTreeMap<String, String>,
    text: String,
    children: Vec<RedmineXmlNode>,
}

fn parse_redmine_import_snapshot(bytes: &[u8]) -> Result<RedmineImportSnapshot> {
    if bytes
        .iter()
        .copied()
        .find(|byte| !byte.is_ascii_whitespace())
        .is_some_and(|byte| byte == b'<')
    {
        parse_redmine_xml_snapshot(bytes)
    } else {
        serde_json::from_slice(bytes).map_err(|e| {
            loom_types::LoomError::invalid(format!("parse Redmine snapshot JSON: {e}"))
        })
    }
}

fn parse_redmine_xml_snapshot(bytes: &[u8]) -> Result<RedmineImportSnapshot> {
    let xml = std::str::from_utf8(bytes)
        .map_err(|e| loom_types::LoomError::invalid(format!("Redmine XML is not UTF-8: {e}")))?;
    let root = parse_redmine_xml_tree(xml)?;
    let projects = redmine_xml_collection_items(&root, "projects", "project")
        .into_iter()
        .filter(|node| redmine_xml_has_meaningful_project(node))
        .map(redmine_project_from_xml)
        .collect::<Result<Vec<_>>>()?;
    let issues = redmine_xml_collection_items(&root, "issues", "issue")
        .into_iter()
        .map(redmine_issue_from_xml)
        .collect::<Result<Vec<_>>>()?;
    let wiki_pages = redmine_xml_collection_items(&root, "wiki_pages", "wiki_page")
        .into_iter()
        .chain(redmine_xml_collection_items(
            &root,
            "wiki-pages",
            "wiki-page",
        ))
        .chain(redmine_xml_collection_items(&root, "pages", "page"))
        .filter(|node| redmine_xml_has_any(node, &["title", "text", "content", "markdown"]))
        .map(redmine_wiki_page_from_xml)
        .collect::<Result<Vec<_>>>()?;
    let time_entries = redmine_xml_collection_items(&root, "time_entries", "time_entry")
        .into_iter()
        .map(redmine_time_entry_from_xml)
        .collect::<Result<Vec<_>>>()?;
    let source_scope = redmine_xml_child_text(&root, "source_scope")
        .or_else(|| redmine_xml_child_text(&root, "source-scope"))
        .or_else(|| root.attrs.get("source_scope").cloned())
        .or_else(|| root.attrs.get("source-scope").cloned());
    Ok(RedmineImportSnapshot {
        source_scope,
        projects,
        issues,
        wiki_pages,
        time_entries,
    })
}

fn parse_redmine_xml_tree(xml: &str) -> Result<RedmineXmlNode> {
    let mut reader = XmlReader::from_str(xml);
    reader.config_mut().trim_text(true);
    let decoder = reader.decoder();
    let mut stack = Vec::<RedmineXmlNode>::new();
    let mut root = None;
    loop {
        match reader.read_event() {
            Ok(XmlEvent::Start(element)) => stack.push(RedmineXmlNode {
                name: redmine_xml_name(element.name().as_ref()),
                attrs: redmine_xml_attrs(&element, decoder)?,
                text: String::new(),
                children: Vec::new(),
            }),
            Ok(XmlEvent::Empty(element)) => {
                let node = RedmineXmlNode {
                    name: redmine_xml_name(element.name().as_ref()),
                    attrs: redmine_xml_attrs(&element, decoder)?,
                    text: String::new(),
                    children: Vec::new(),
                };
                redmine_xml_push_node(&mut stack, &mut root, node)?;
            }
            Ok(XmlEvent::End(_)) => {
                let node = stack.pop().ok_or_else(|| {
                    loom_types::LoomError::invalid("Redmine XML end without start")
                })?;
                redmine_xml_push_node(&mut stack, &mut root, node)?;
            }
            Ok(XmlEvent::Text(text)) => {
                if let Some(node) = stack.last_mut() {
                    node.text.push_str(
                        &text
                            .decode()
                            .map_err(|e| loom_types::LoomError::invalid(e.to_string()))?,
                    );
                }
            }
            Ok(XmlEvent::CData(text)) => {
                if let Some(node) = stack.last_mut() {
                    node.text.push_str(
                        &text
                            .decode()
                            .map_err(|e| loom_types::LoomError::invalid(e.to_string()))?,
                    );
                }
            }
            Ok(XmlEvent::DocType(_) | XmlEvent::GeneralRef(_)) => {
                return Err(loom_types::LoomError::invalid(
                    "Redmine XML must not contain DTD or entity declarations",
                ));
            }
            Ok(XmlEvent::Eof) => break,
            Err(err) => {
                return Err(loom_types::LoomError::invalid(format!(
                    "parse Redmine XML: {err}"
                )));
            }
            _ => {}
        }
    }
    if !stack.is_empty() {
        return Err(loom_types::LoomError::invalid(
            "Redmine XML has unclosed elements",
        ));
    }
    root.ok_or_else(|| loom_types::LoomError::invalid("Redmine XML is empty"))
}

fn redmine_xml_attrs(
    element: &quick_xml::events::BytesStart<'_>,
    decoder: quick_xml::encoding::Decoder,
) -> Result<BTreeMap<String, String>> {
    let mut attrs = BTreeMap::new();
    for attr in element.attributes().with_checks(false) {
        let attr = attr.map_err(|e| loom_types::LoomError::invalid(e.to_string()))?;
        let name = redmine_xml_name(attr.key.as_ref());
        let value = attr
            .decoded_and_normalized_value(XmlVersion::Implicit1_0, decoder)
            .map_err(|e| loom_types::LoomError::invalid(e.to_string()))?;
        attrs.insert(name, value.into_owned());
    }
    Ok(attrs)
}

fn redmine_xml_push_node(
    stack: &mut [RedmineXmlNode],
    root: &mut Option<RedmineXmlNode>,
    node: RedmineXmlNode,
) -> Result<()> {
    if let Some(parent) = stack.last_mut() {
        parent.children.push(node);
    } else if root.is_none() {
        *root = Some(node);
    } else {
        return Err(loom_types::LoomError::invalid(
            "Redmine XML has multiple root elements",
        ));
    }
    Ok(())
}

fn redmine_xml_name(name: &[u8]) -> String {
    let local = name.split(|byte| *byte == b':').next_back().unwrap_or(name);
    String::from_utf8_lossy(local).replace('-', "_")
}

fn redmine_xml_descendants<'a>(node: &'a RedmineXmlNode, name: &str) -> Vec<&'a RedmineXmlNode> {
    let mut out = Vec::new();
    if node.name == name {
        out.push(node);
    }
    for child in &node.children {
        out.extend(redmine_xml_descendants(child, name));
    }
    out
}

fn redmine_xml_collection_items<'a>(
    root: &'a RedmineXmlNode,
    collection_name: &str,
    item_name: &str,
) -> Vec<&'a RedmineXmlNode> {
    if root.name == item_name {
        return vec![root];
    }
    let collections = redmine_xml_descendants(root, collection_name);
    if collections.is_empty() {
        return redmine_xml_descendants(root, item_name);
    }
    collections
        .into_iter()
        .flat_map(|collection| {
            collection
                .children
                .iter()
                .filter(move |child| child.name == item_name)
        })
        .collect()
}

fn redmine_xml_child_text(node: &RedmineXmlNode, name: &str) -> Option<String> {
    node.children
        .iter()
        .find(|child| child.name == name)
        .and_then(redmine_xml_node_text)
        .or_else(|| node.attrs.get(name).cloned())
}

fn redmine_xml_node_text(node: &RedmineXmlNode) -> Option<String> {
    let value = node.text.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn redmine_xml_child_value(node: &RedmineXmlNode, name: &str) -> Option<serde_json::Value> {
    redmine_xml_child_text(node, name).map(serde_json::Value::String)
}

fn redmine_xml_child_scalar_value(node: &RedmineXmlNode, name: &str) -> Option<serde_json::Value> {
    redmine_xml_child_text(node, name).map(|value| {
        if value.eq_ignore_ascii_case("true") {
            serde_json::Value::Bool(true)
        } else if value.eq_ignore_ascii_case("false") {
            serde_json::Value::Bool(false)
        } else if let Ok(parsed) = value.parse::<i64>() {
            serde_json::Value::Number(parsed.into())
        } else if let Ok(parsed) = value.parse::<f64>() {
            serde_json::Number::from_f64(parsed)
                .map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::String(value))
        } else {
            serde_json::Value::String(value)
        }
    })
}

fn redmine_xml_has_any(node: &RedmineXmlNode, names: &[&str]) -> bool {
    names
        .iter()
        .any(|name| redmine_xml_child_text(node, name).is_some())
}

fn redmine_xml_has_meaningful_project(node: &RedmineXmlNode) -> bool {
    redmine_xml_has_any(node, &["id", "identifier", "name"]) || node.attrs.contains_key("id")
}

fn redmine_project_from_xml(node: &RedmineXmlNode) -> Result<RedmineProject> {
    Ok(RedmineProject {
        id: redmine_xml_child_value(node, "id")
            .or_else(|| node.attrs.get("id").cloned().map(serde_json::Value::String))
            .ok_or_else(|| loom_types::LoomError::invalid("Redmine XML project missing id"))?,
        identifier: redmine_xml_child_text(node, "identifier"),
        name: redmine_xml_child_text(node, "name")
            .ok_or_else(|| loom_types::LoomError::invalid("Redmine XML project missing name"))?,
        key_prefix: redmine_xml_child_text(node, "key_prefix"),
        description: redmine_xml_child_text(node, "description"),
        homepage: redmine_xml_child_text(node, "homepage"),
        status: redmine_xml_child_scalar_value(node, "status"),
        is_public: redmine_xml_child_scalar_value(node, "is_public"),
        parent: redmine_xml_child_ref_value(node, "parent", "id"),
        default_version: redmine_xml_child_ref_value(node, "default_version", "id"),
        default_assignee: redmine_xml_child_ref_value(node, "default_assignee", "id"),
        created_on: redmine_xml_child_text(node, "created_on"),
        updated_on: redmine_xml_child_text(node, "updated_on"),
        trackers: redmine_xml_named_children(node, "trackers"),
        issue_categories: redmine_xml_named_children(node, "issue_categories"),
        enabled_modules: redmine_xml_named_children(node, "enabled_modules"),
        time_entry_activities: redmine_xml_named_children(node, "time_entry_activities"),
        issue_custom_fields: redmine_xml_named_children(node, "issue_custom_fields"),
    })
}

fn redmine_issue_from_xml(node: &RedmineXmlNode) -> Result<RedmineIssue> {
    Ok(RedmineIssue {
        id: redmine_xml_child_value(node, "id")
            .or_else(|| node.attrs.get("id").cloned().map(serde_json::Value::String))
            .ok_or_else(|| loom_types::LoomError::invalid("Redmine XML issue missing id"))?,
        project_id: redmine_xml_child_value(node, "project_id")
            .or_else(|| redmine_xml_child_ref_value(node, "project", "id")),
        project_identifier: redmine_xml_child_text(node, "project_identifier")
            .or_else(|| redmine_xml_child_ref_text(node, "project", "identifier")),
        tracker: redmine_xml_child_text(node, "tracker")
            .or_else(|| redmine_xml_child_ref_text(node, "tracker", "name")),
        subject: redmine_xml_child_text(node, "subject")
            .ok_or_else(|| loom_types::LoomError::invalid("Redmine XML issue missing subject"))?,
        description: redmine_xml_child_text(node, "description"),
        status: redmine_xml_child_text(node, "status")
            .or_else(|| redmine_xml_child_ref_text(node, "status", "name")),
        priority: redmine_xml_child_text(node, "priority")
            .or_else(|| redmine_xml_child_ref_text(node, "priority", "name")),
        category: redmine_xml_child_text(node, "category")
            .or_else(|| redmine_xml_child_ref_text(node, "category", "name")),
        assigned_to: redmine_xml_child_text(node, "assigned_to")
            .or_else(|| redmine_xml_child_ref_text(node, "assigned_to", "name")),
        author: redmine_xml_child_text(node, "author")
            .or_else(|| redmine_xml_child_ref_text(node, "author", "name")),
        created_at: redmine_xml_child_text(node, "created_at"),
        created_on: redmine_xml_child_text(node, "created_on"),
        updated_at: redmine_xml_child_text(node, "updated_at"),
        updated_on: redmine_xml_child_text(node, "updated_on"),
        start_date: redmine_xml_child_text(node, "start_date"),
        due_date: redmine_xml_child_text(node, "due_date"),
        closed_on: redmine_xml_child_text(node, "closed_on"),
        done_ratio: redmine_xml_child_scalar_value(node, "done_ratio"),
        estimated_hours: redmine_xml_child_scalar_value(node, "estimated_hours"),
        fixed_version: redmine_xml_child_text(node, "fixed_version")
            .or_else(|| redmine_xml_child_ref_text(node, "fixed_version", "name")),
        affected_version: redmine_xml_child_text(node, "affected_version")
            .or_else(|| redmine_xml_child_ref_text(node, "affected_version", "name")),
        affected_versions: redmine_xml_named_children(node, "affected_versions"),
        parent_issue_id: redmine_xml_child_scalar_value(node, "parent_issue_id")
            .or_else(|| redmine_xml_child_ref_value(node, "parent", "id")),
        is_private: redmine_xml_child_scalar_value(node, "is_private"),
        url: redmine_xml_child_text(node, "url"),
        custom_fields: redmine_xml_named_children_value(node, "custom_fields"),
        policy_labels: redmine_xml_labels(node),
        journals: redmine_xml_named_children(node, "journals"),
        comments: redmine_xml_named_children(node, "comments"),
        watchers: redmine_xml_named_children(node, "watchers"),
        attachments: redmine_xml_named_children(node, "attachments"),
        time_entries: redmine_xml_named_children(node, "time_entries"),
        relations: redmine_xml_named_children(node, "relations"),
        children: redmine_xml_named_children(node, "children"),
        changesets: redmine_xml_named_children(node, "changesets"),
        allowed_statuses: redmine_xml_named_children(node, "allowed_statuses"),
    })
}

fn redmine_wiki_page_from_xml(node: &RedmineXmlNode) -> Result<RedmineWikiPage> {
    Ok(RedmineWikiPage {
        id: redmine_xml_child_value(node, "id")
            .or_else(|| node.attrs.get("id").cloned().map(serde_json::Value::String))
            .or_else(|| redmine_xml_child_text(node, "title").map(serde_json::Value::String))
            .ok_or_else(|| loom_types::LoomError::invalid("Redmine XML wiki page missing id"))?,
        title: redmine_xml_child_text(node, "title")
            .ok_or_else(|| loom_types::LoomError::invalid("Redmine XML wiki page missing title"))?,
        project_id: redmine_xml_child_value(node, "project_id")
            .or_else(|| redmine_xml_child_ref_value(node, "project", "id")),
        project_identifier: redmine_xml_child_text(node, "project_identifier")
            .or_else(|| redmine_xml_child_ref_text(node, "project", "identifier")),
        space_id: redmine_xml_child_text(node, "space_id"),
        page_id: redmine_xml_child_text(node, "page_id"),
        parent_page_id: redmine_xml_child_text(node, "parent_page_id").or_else(|| {
            redmine_xml_child_ref_text(node, "parent", "title").map(|title| stable_page_id(&title))
        }),
        parent_title: redmine_xml_child_ref_text(node, "parent", "title"),
        text: redmine_xml_child_text(node, "text")
            .or_else(|| redmine_xml_child_text(node, "content")),
        body: redmine_xml_child_text(node, "body"),
        markdown: redmine_xml_child_text(node, "markdown"),
        version: redmine_xml_child_scalar_value(node, "version"),
        author: redmine_xml_child_ref_value(node, "author", "id")
            .or_else(|| redmine_xml_named_children_value(node, "author")),
        comments: redmine_xml_child_text(node, "comments"),
        created_on: redmine_xml_child_text(node, "created_on"),
        updated_on: redmine_xml_child_text(node, "updated_on"),
        attachments: redmine_xml_named_children(node, "attachments"),
    })
}

fn redmine_time_entry_from_xml(node: &RedmineXmlNode) -> Result<RedmineTimeEntry> {
    Ok(RedmineTimeEntry {
        id: redmine_xml_child_value(node, "id")
            .or_else(|| node.attrs.get("id").cloned().map(serde_json::Value::String))
            .ok_or_else(|| loom_types::LoomError::invalid("Redmine XML time entry missing id"))?,
        issue_id: redmine_xml_child_value(node, "issue_id")
            .or_else(|| redmine_xml_child_ref_value(node, "issue", "id")),
        project_id: redmine_xml_child_value(node, "project_id")
            .or_else(|| redmine_xml_child_ref_value(node, "project", "id")),
        user: redmine_xml_child_text(node, "user")
            .or_else(|| redmine_xml_child_ref_text(node, "user", "name")),
        activity: redmine_xml_child_text(node, "activity")
            .or_else(|| redmine_xml_child_ref_text(node, "activity", "name")),
        hours: redmine_xml_child_scalar_value(node, "hours"),
        comments: redmine_xml_child_text(node, "comments"),
        spent_on: redmine_xml_child_text(node, "spent_on"),
        created_on: redmine_xml_child_text(node, "created_on"),
        updated_on: redmine_xml_child_text(node, "updated_on"),
    })
}

fn redmine_time_entry_value(entry: &RedmineTimeEntry) -> Result<serde_json::Value> {
    let mut value = serde_json::Map::new();
    value.insert("id".to_string(), entry.id.clone());
    if let Some(issue_id) = &entry.issue_id {
        value.insert("issue_id".to_string(), issue_id.clone());
    }
    if let Some(project_id) = &entry.project_id {
        value.insert("project_id".to_string(), project_id.clone());
    }
    insert_optional_json_text(&mut value, "user", entry.user.as_deref());
    insert_optional_json_text(&mut value, "activity", entry.activity.as_deref());
    if let Some(hours) = &entry.hours {
        value.insert("hours".to_string(), hours.clone());
    }
    insert_optional_json_text(&mut value, "comments", entry.comments.as_deref());
    insert_optional_json_text(&mut value, "spent_on", entry.spent_on.as_deref());
    insert_optional_json_text(&mut value, "created_on", entry.created_on.as_deref());
    insert_optional_json_text(&mut value, "updated_on", entry.updated_on.as_deref());
    Ok(serde_json::Value::Object(value))
}

fn redmine_time_entries_by_issue(
    entries: &[RedmineTimeEntry],
) -> Result<BTreeMap<String, Vec<serde_json::Value>>> {
    let mut grouped = BTreeMap::<String, Vec<serde_json::Value>>::new();
    for entry in entries {
        let Some(issue_id) = entry.issue_id.as_ref() else {
            continue;
        };
        grouped
            .entry(redmine_source_id("issue", issue_id)?)
            .or_default()
            .push(redmine_time_entry_value(entry)?);
    }
    Ok(grouped)
}

fn redmine_xml_child_ref_text(
    node: &RedmineXmlNode,
    child_name: &str,
    attr_name: &str,
) -> Option<String> {
    node.children
        .iter()
        .find(|child| child.name == child_name)
        .and_then(|child| {
            child
                .attrs
                .get(attr_name)
                .cloned()
                .or_else(|| redmine_xml_node_text(child))
        })
}

fn redmine_xml_child_ref_value(
    node: &RedmineXmlNode,
    child_name: &str,
    attr_name: &str,
) -> Option<serde_json::Value> {
    redmine_xml_child_ref_text(node, child_name, attr_name).map(serde_json::Value::String)
}

fn redmine_xml_named_children(node: &RedmineXmlNode, name: &str) -> Vec<serde_json::Value> {
    node.children
        .iter()
        .find(|child| child.name == name)
        .map(|child| child.children.iter().map(redmine_xml_node_value).collect())
        .unwrap_or_default()
}

fn redmine_xml_named_children_value(
    node: &RedmineXmlNode,
    name: &str,
) -> Option<serde_json::Value> {
    let children = redmine_xml_named_children(node, name);
    (!children.is_empty()).then_some(serde_json::Value::Array(children))
}

fn redmine_xml_labels(node: &RedmineXmlNode) -> Vec<String> {
    redmine_xml_named_children(node, "policy_labels")
        .into_iter()
        .chain(redmine_xml_named_children(node, "labels"))
        .filter_map(|value| match value {
            serde_json::Value::String(text) => Some(text),
            serde_json::Value::Object(map) => map.get("name").and_then(|name| {
                name.as_str()
                    .or_else(|| map.get("value").and_then(serde_json::Value::as_str))
                    .map(str::to_string)
            }),
            _ => None,
        })
        .collect()
}

fn redmine_xml_node_value(node: &RedmineXmlNode) -> serde_json::Value {
    if node.children.is_empty() && node.attrs.is_empty() {
        return redmine_xml_node_text(node)
            .map(serde_json::Value::String)
            .unwrap_or(serde_json::Value::Null);
    }
    let mut map = serde_json::Map::new();
    for (key, value) in &node.attrs {
        map.insert(key.clone(), serde_json::Value::String(value.clone()));
    }
    if let Some(text) = redmine_xml_node_text(node) {
        map.insert("text".to_string(), serde_json::Value::String(text));
    }
    for child in &node.children {
        let value = redmine_xml_node_value(child);
        match map.get_mut(&child.name) {
            Some(serde_json::Value::Array(values)) => values.push(value),
            Some(existing) => {
                let previous = std::mem::replace(existing, serde_json::Value::Null);
                *existing = serde_json::Value::Array(vec![previous, value]);
            }
            None => {
                map.insert(child.name.clone(), value);
            }
        }
    }
    serde_json::Value::Object(map)
}

pub fn import_asana_bytes(
    loom: &mut Loom<FileStore>,
    ns: WorkspaceId,
    workspace_id: &str,
    source_path: &str,
    bytes: &[u8],
    dry_run: bool,
) -> Result<ImportReport> {
    import_asana_bytes_with_field_policy(
        loom,
        ns,
        workspace_id,
        source_path,
        bytes,
        dry_run,
        TicketImportFieldPolicy::Strict,
    )
}

pub fn import_asana_bytes_with_field_policy(
    loom: &mut Loom<FileStore>,
    ns: WorkspaceId,
    workspace_id: &str,
    source_path: &str,
    bytes: &[u8],
    dry_run: bool,
    field_policy: TicketImportFieldPolicy,
) -> Result<ImportReport> {
    let parsed: AsanaImportSnapshot = serde_json::from_slice(bytes)
        .map_err(|e| loom_types::LoomError::invalid(format!("parse Asana snapshot JSON: {e}")))?;
    import_asana_snapshot(
        loom,
        ns,
        workspace_id,
        source_path,
        bytes.len() as u64,
        parsed,
        dry_run,
        field_policy,
    )
}

pub fn import_asana_snapshot(
    loom: &mut Loom<FileStore>,
    ns: WorkspaceId,
    workspace_id: &str,
    fallback_source_scope: &str,
    bytes_in: u64,
    parsed: AsanaImportSnapshot,
    dry_run: bool,
    field_policy: TicketImportFieldPolicy,
) -> Result<ImportReport> {
    let source_scope = parsed
        .source_scope
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(fallback_source_scope);
    let mut report = ImportReport::new(ImportReportInput {
        profile: "asana",
        source_scope,
        commit: None,
        objects_added: 0,
        bytes_in,
        bytes_stored: 0,
        rows_imported: 0,
        skipped: 0,
        operations_planned: (parsed.projects.len()
            + parsed.tasks.len()
            + asana_board_project_count(&parsed.tasks)
            + asana_board_card_count(&parsed.tasks)) as u64,
        operations_applied: 0,
        dry_run,
    })?;
    for project in &parsed.projects {
        add_asana_project_fidelity_issues(&mut report, project)?;
    }
    for task in &parsed.tasks {
        add_asana_task_fidelity_issues(&mut report, task)?;
    }
    if dry_run {
        return Ok(report);
    }

    for project in &parsed.projects {
        let project_id = asana_project_id(project)?;
        match loom_tickets::create_project(
            loom,
            ns,
            workspace_id,
            &project_id,
            &asana_project_key_prefix(project)?,
            &project.name,
            None,
        ) {
            Ok(_) => {
                report.operations_applied += 1;
                report.rows_imported += 1;
            }
            Err(error) if error.code == Code::AlreadyExists => {
                report.skipped += 1;
            }
            Err(error) => return Err(error),
        }
    }
    let asana_boards = asana_project_boards(&parsed.tasks)?;
    for (project_id, sections) in &asana_boards {
        let card_display_fields = vec!["subject".to_string(), "assignee".to_string()];
        let columns = sections
            .iter()
            .enumerate()
            .map(|(index, (section_id, section_name))| {
                loom_tickets::BoardColumn::with_display(
                    section_id.clone(),
                    section_name.clone(),
                    BTreeSet::new(),
                    None,
                    false,
                    (index as u64 + 1) * 10,
                )
            })
            .collect::<Result<Vec<_>>>()?;
        match loom_tickets::create_board(
            loom,
            ns,
            loom_tickets::BoardCreateRequest {
                workspace_id,
                board_id: &format!("{project_id}-asana-board"),
                board_key: &format!("ASANA-{project_id}"),
                name: "Asana sections",
                description: "Imported Asana section board",
                project_id,
                scope: loom_tickets::BoardScope::ManualSet,
                mode: loom_tickets::BoardMode::Manual,
                columns: &columns,
                swimlanes: &[],
                card_display_fields: &card_display_fields,
                owner_principal: None,
                coordinator_principal: None,
                updated_by: "import:asana",
                expected_root: None,
            },
        ) {
            Ok(_) => {
                report.operations_applied += 1;
                report.rows_imported += 1;
            }
            Err(error) if error.code == Code::AlreadyExists => {
                report.skipped += 1;
            }
            Err(error) => return Err(error),
        }
    }
    let mut asana_card_ranks = BTreeMap::<(String, String), u64>::new();
    for task in &parsed.tasks {
        let fields = asana_task_fields(task)?;
        match import_ticket_with_field_policy(
            loom,
            ns,
            field_policy,
            loom_tickets::TicketCreateRequest {
                workspace_id,
                project_id: &asana_task_project_id(task)?,
                ticket_type: asana_ticket_type(task.resource_subtype.as_deref()),
                external_source: Some("asana"),
                external_id: Some(&asana_source_id("task", &task.gid)?),
                fields: &fields,
                policy_labels: &task.tags,
                expected_root: None,
            },
        ) {
            Ok(ticket) => {
                report.operations_applied += 1;
                report.rows_imported += 1;
                let project_id = asana_task_project_id(task)?;
                if asana_boards.contains_key(&project_id) {
                    let (section_id, _) = asana_task_section(task)?;
                    let rank = asana_card_ranks
                        .entry((project_id.clone(), section_id.clone()))
                        .and_modify(|rank| *rank += 1)
                        .or_insert(1);
                    loom_tickets::move_board_card(
                        loom,
                        ns,
                        loom_tickets::BoardCardMoveRequest {
                            workspace_id,
                            board_id: &format!("{project_id}-asana-board"),
                            ticket_id: &ticket.ticket_id,
                            column_id: &section_id,
                            rank_token: &format!("{rank:016}"),
                            swimlane_id: None,
                            updated_by: "import:asana",
                            expected_root: None,
                        },
                    )?;
                    report.operations_applied += 1;
                    report.rows_imported += 1;
                }
            }
            Err(error) if error.code == Code::AlreadyExists => {
                report.skipped += 1;
            }
            Err(error) => return Err(error),
        }
    }
    Ok(report)
}

pub fn import_jira_bytes(
    loom: &mut Loom<FileStore>,
    ns: WorkspaceId,
    workspace_id: &str,
    source_path: &str,
    bytes: &[u8],
    dry_run: bool,
) -> Result<ImportReport> {
    import_jira_bytes_with_field_policy(
        loom,
        ns,
        workspace_id,
        source_path,
        bytes,
        dry_run,
        TicketImportFieldPolicy::Strict,
    )
}

pub fn import_jira_bytes_with_field_policy(
    loom: &mut Loom<FileStore>,
    ns: WorkspaceId,
    workspace_id: &str,
    source_path: &str,
    bytes: &[u8],
    dry_run: bool,
    field_policy: TicketImportFieldPolicy,
) -> Result<ImportReport> {
    let parsed: JiraImportSnapshot = serde_json::from_slice(bytes)
        .map_err(|e| loom_types::LoomError::invalid(format!("parse Jira snapshot JSON: {e}")))?;
    import_jira_snapshot(
        loom,
        ns,
        workspace_id,
        source_path,
        bytes.len() as u64,
        parsed,
        dry_run,
        field_policy,
    )
}

pub fn import_jira_snapshot(
    loom: &mut Loom<FileStore>,
    ns: WorkspaceId,
    workspace_id: &str,
    fallback_source_scope: &str,
    bytes_in: u64,
    parsed: JiraImportSnapshot,
    dry_run: bool,
    field_policy: TicketImportFieldPolicy,
) -> Result<ImportReport> {
    let source_scope = parsed
        .source_scope
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(fallback_source_scope);
    let mut report = ImportReport::new(ImportReportInput {
        profile: "jira",
        source_scope,
        commit: None,
        objects_added: 0,
        bytes_in,
        bytes_stored: 0,
        rows_imported: 0,
        skipped: 0,
        operations_planned: (parsed.projects.len()
            + parsed.issues.len()
            + jira_board_project_count(&parsed.issues)
            + jira_board_card_count(&parsed.issues)) as u64,
        operations_applied: 0,
        dry_run,
    })?;
    for project in &parsed.projects {
        add_jira_project_fidelity_issues(&mut report, project)?;
    }
    for issue in &parsed.issues {
        add_jira_issue_fidelity_issues(&mut report, issue)?;
    }
    if dry_run {
        return Ok(report);
    }

    for project in &parsed.projects {
        match loom_tickets::create_project(
            loom,
            ns,
            workspace_id,
            &jira_project_id(project)?,
            &jira_project_key_prefix(project)?,
            &project.name,
            None,
        ) {
            Ok(_) => {
                report.operations_applied += 1;
                report.rows_imported += 1;
            }
            Err(error) if error.code == Code::AlreadyExists => {
                report.skipped += 1;
            }
            Err(error) => return Err(error),
        }
    }
    let jira_boards = jira_project_boards(&parsed.issues)?;
    for (project_id, statuses) in &jira_boards {
        let card_display_fields = vec!["subject".to_string(), "priority".to_string()];
        let columns = statuses
            .iter()
            .enumerate()
            .map(|(index, status)| {
                loom_tickets::BoardColumn::with_display(
                    import_column_id(status),
                    status.clone(),
                    BTreeSet::from([status.clone()]),
                    None,
                    false,
                    (index as u64 + 1) * 10,
                )
            })
            .collect::<Result<Vec<_>>>()?;
        match loom_tickets::create_board(
            loom,
            ns,
            loom_tickets::BoardCreateRequest {
                workspace_id,
                board_id: &format!("{project_id}-jira-board"),
                board_key: &format!("JIRA-{project_id}"),
                name: "Jira status board",
                description: "Imported Jira status board",
                project_id,
                scope: loom_tickets::BoardScope::project(project_id.clone()),
                mode: loom_tickets::BoardMode::StatusMapped,
                columns: &columns,
                swimlanes: &[],
                card_display_fields: &card_display_fields,
                owner_principal: None,
                coordinator_principal: None,
                updated_by: "import:jira",
                expected_root: None,
            },
        ) {
            Ok(_) => {
                report.operations_applied += 1;
                report.rows_imported += 1;
            }
            Err(error) if error.code == Code::AlreadyExists => {
                report.skipped += 1;
            }
            Err(error) => return Err(error),
        }
    }
    let mut jira_card_ranks = BTreeMap::<(String, String), u64>::new();
    for issue in &parsed.issues {
        let fields = jira_issue_fields(issue)?;
        match import_ticket_with_field_policy(
            loom,
            ns,
            field_policy,
            loom_tickets::TicketCreateRequest {
                workspace_id,
                project_id: &jira_issue_project_id(issue)?,
                ticket_type: jira_ticket_type(issue.issue_type.as_deref()),
                external_source: Some("jira"),
                external_id: Some(&jira_source_id("issue", &issue.id)?),
                fields: &fields,
                policy_labels: &issue.labels,
                expected_root: None,
            },
        ) {
            Ok(ticket) => {
                report.operations_applied += 1;
                report.rows_imported += 1;
                if let Some(status) = issue.status.as_deref().filter(|value| !value.is_empty()) {
                    let project_id = jira_issue_project_id(issue)?;
                    if jira_boards.contains_key(&project_id) {
                        let column_id = import_column_id(status);
                        let rank = jira_card_ranks
                            .entry((project_id.clone(), column_id.clone()))
                            .and_modify(|rank| *rank += 1)
                            .or_insert(1);
                        loom_tickets::move_board_card(
                            loom,
                            ns,
                            loom_tickets::BoardCardMoveRequest {
                                workspace_id,
                                board_id: &format!("{project_id}-jira-board"),
                                ticket_id: &ticket.ticket_id,
                                column_id: &column_id,
                                rank_token: &format!("{rank:016}"),
                                swimlane_id: None,
                                updated_by: "import:jira",
                                expected_root: None,
                            },
                        )?;
                        report.operations_applied += 1;
                        report.rows_imported += 1;
                    }
                }
            }
            Err(error) if error.code == Code::AlreadyExists => {
                report.skipped += 1;
            }
            Err(error) => return Err(error),
        }
    }
    Ok(report)
}

fn import_ticket_with_field_policy(
    loom: &mut Loom<FileStore>,
    ns: WorkspaceId,
    field_policy: TicketImportFieldPolicy,
    request: loom_tickets::TicketCreateRequest<'_>,
) -> Result<loom_tickets::TicketSummary> {
    match field_policy {
        TicketImportFieldPolicy::Strict => {
            let fields = request.fields;
            loom_tickets::create_ticket(loom, ns, request)
                .map_err(|error| ticket_import_strict_field_error(error, fields))
        }
        TicketImportFieldPolicy::Infer => {
            infer_ticket_field_definitions(loom, ns, &request)?;
            loom_tickets::create_ticket(loom, ns, request)
        }
    }
}

fn asana_board_project_count(tasks: &[AsanaTask]) -> usize {
    tasks
        .iter()
        .filter_map(|task| asana_task_project_id(task).ok())
        .collect::<BTreeSet<_>>()
        .len()
}

fn asana_board_card_count(tasks: &[AsanaTask]) -> usize {
    tasks
        .iter()
        .filter(|task| asana_task_project_id(task).is_ok())
        .count()
}

fn jira_board_project_count(issues: &[JiraIssue]) -> usize {
    issues
        .iter()
        .filter_map(|issue| jira_issue_project_id(issue).ok())
        .collect::<BTreeSet<_>>()
        .len()
}

fn jira_board_card_count(issues: &[JiraIssue]) -> usize {
    issues
        .iter()
        .filter(|issue| {
            issue
                .status
                .as_deref()
                .is_some_and(|status| !status.is_empty())
                && jira_issue_project_id(issue).is_ok()
        })
        .count()
}

fn asana_project_boards(tasks: &[AsanaTask]) -> Result<BTreeMap<String, Vec<(String, String)>>> {
    let mut boards = BTreeMap::<String, Vec<(String, String)>>::new();
    let mut seen = BTreeSet::<(String, String)>::new();
    for task in tasks {
        let project_id = asana_task_project_id(task)?;
        let (section_id, section_name) = asana_task_section(task)?;
        if seen.insert((project_id.clone(), section_id.clone())) {
            boards
                .entry(project_id)
                .or_default()
                .push((section_id, section_name));
        }
    }
    Ok(boards)
}

fn jira_project_boards(issues: &[JiraIssue]) -> Result<BTreeMap<String, Vec<String>>> {
    let mut boards = BTreeMap::<String, Vec<String>>::new();
    let mut seen = BTreeSet::<(String, String)>::new();
    for issue in issues {
        let Some(status) = issue.status.as_deref().filter(|value| !value.is_empty()) else {
            continue;
        };
        let project_id = jira_issue_project_id(issue)?;
        if seen.insert((project_id.clone(), status.to_string())) {
            boards
                .entry(project_id)
                .or_default()
                .push(status.to_string());
        }
    }
    Ok(boards)
}

fn asana_task_section(task: &AsanaTask) -> Result<(String, String)> {
    if let Some(section) = task.assignee_section.as_ref() {
        return import_named_id(section, "section", "Unsectioned");
    }
    for membership in &task.memberships {
        if let Some(section) = membership.get("section") {
            return import_named_id(section, "section", "Unsectioned");
        }
    }
    Ok(("unsectioned".to_string(), "Unsectioned".to_string()))
}

fn import_named_id(
    value: &serde_json::Value,
    fallback_prefix: &str,
    fallback_name: &str,
) -> Result<(String, String)> {
    if let Some(text) = asana_id_string(value).filter(|value| !value.trim().is_empty()) {
        let name = if text.trim().is_empty() {
            fallback_name.to_string()
        } else {
            text.clone()
        };
        return Ok((import_column_id(&format!("{fallback_prefix}-{text}")), name));
    }
    let name = value
        .get("name")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(fallback_name)
        .to_string();
    let raw_id = value
        .get("gid")
        .or_else(|| value.get("id"))
        .and_then(asana_id_string)
        .or_else(|| {
            value
                .get("name")
                .and_then(serde_json::Value::as_str)
                .map(|value| value.to_string())
        })
        .unwrap_or_else(|| name.clone());
    Ok((
        import_column_id(&format!("{fallback_prefix}-{raw_id}")),
        name,
    ))
}

fn import_column_id(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() {
        "column".to_string()
    } else {
        out
    }
}

fn ticket_import_strict_field_error(
    error: loom_types::LoomError,
    fields: &serde_json::Value,
) -> loom_types::LoomError {
    if error.code != Code::InvalidArgument {
        return error;
    }
    let unknown_fields = unknown_import_field_names(fields);
    if unknown_fields.is_empty() {
        return error;
    }
    loom_types::LoomError::invalid(format!(
        "ticket import encountered undeclared fields: {}; create project custom-field definitions or rerun the import with field policy infer",
        unknown_fields.join(", ")
    ))
}

fn infer_ticket_field_definitions(
    loom: &mut Loom<FileStore>,
    ns: WorkspaceId,
    request: &loom_tickets::TicketCreateRequest<'_>,
) -> Result<()> {
    let catalog = loom_tickets::ticket_field_catalog_for_project(
        loom,
        ns,
        request.workspace_id,
        request.project_id,
        None,
        Some("write"),
    )?;
    let known_fields: BTreeSet<String> = catalog
        .fields
        .iter()
        .map(|field| field.native_field.clone())
        .collect();
    let serde_json::Value::Object(fields) = request.fields else {
        return Err(loom_types::LoomError::invalid(
            "ticket import fields must be a JSON object",
        ));
    };
    let applicable_type_ids: Vec<String> = Vec::new();
    for (field, value) in fields {
        if known_fields.contains(field) {
            continue;
        }
        let shape = inferred_ticket_field_shape(value);
        loom_tickets::put_ticket_field_definition(
            loom,
            ns,
            loom_tickets::TicketFieldDefinitionWriteRequest {
                workspace_id: request.workspace_id,
                project_id: request.project_id,
                field_id: field,
                key: field,
                name: field,
                description: Some("Inferred from ticket import source data."),
                field_type: shape.field_type,
                option_set: None,
                max_length: shape.max_length,
                required: false,
                searchable: shape.searchable,
                orderable: shape.orderable,
                cardinality: shape.cardinality,
                applicable_type_ids: &applicable_type_ids,
                expected_root: None,
            },
        )?;
    }
    Ok(())
}

fn unknown_import_field_names(fields: &serde_json::Value) -> Vec<String> {
    let serde_json::Value::Object(fields) = fields else {
        return Vec::new();
    };
    let mut names: Vec<String> = fields
        .keys()
        .filter(|field| !matches_known_ticket_import_field(field))
        .cloned()
        .collect();
    names.sort();
    names
}

fn matches_known_ticket_import_field(field: &str) -> bool {
    loom_tickets::ticket_field_catalog(
        Some(loom_tickets::TicketProjectionProfile::Native),
        Some("write"),
    )
    .map(|catalog| {
        catalog
            .fields
            .iter()
            .any(|known| known.native_field == field)
    })
    .unwrap_or(false)
}

struct InferredTicketFieldShape {
    field_type: &'static str,
    max_length: Option<u32>,
    searchable: bool,
    orderable: bool,
    cardinality: loom_tickets::TicketFieldCardinality,
}

fn inferred_ticket_field_shape(value: &serde_json::Value) -> InferredTicketFieldShape {
    match value {
        serde_json::Value::String(value) => {
            let max_length = value.len().clamp(4096, 1_048_576) as u32;
            InferredTicketFieldShape {
                field_type: "string",
                max_length: Some(max_length),
                searchable: true,
                orderable: true,
                cardinality: loom_tickets::TicketFieldCardinality::Optional,
            }
        }
        serde_json::Value::Number(value) if value.is_i64() || value.is_u64() => {
            InferredTicketFieldShape {
                field_type: "integer",
                max_length: None,
                searchable: true,
                orderable: true,
                cardinality: loom_tickets::TicketFieldCardinality::Optional,
            }
        }
        serde_json::Value::Number(_) => InferredTicketFieldShape {
            field_type: "number",
            max_length: None,
            searchable: true,
            orderable: true,
            cardinality: loom_tickets::TicketFieldCardinality::Optional,
        },
        serde_json::Value::Bool(_) => InferredTicketFieldShape {
            field_type: "boolean",
            max_length: None,
            searchable: true,
            orderable: true,
            cardinality: loom_tickets::TicketFieldCardinality::Optional,
        },
        serde_json::Value::Null => InferredTicketFieldShape {
            field_type: "string",
            max_length: Some(4096),
            searchable: true,
            orderable: true,
            cardinality: loom_tickets::TicketFieldCardinality::Optional,
        },
        serde_json::Value::Array(values) => InferredTicketFieldShape {
            field_type: inferred_ticket_list_field_type(values),
            max_length: Some(1_048_576),
            searchable: true,
            orderable: false,
            cardinality: loom_tickets::TicketFieldCardinality::List {
                min_items: 0,
                max_items: None,
            },
        },
        serde_json::Value::Object(_) => InferredTicketFieldShape {
            field_type: "opaque_json",
            max_length: Some(1_048_576),
            searchable: true,
            orderable: false,
            cardinality: loom_tickets::TicketFieldCardinality::Optional,
        },
    }
}

fn inferred_ticket_list_field_type(values: &[serde_json::Value]) -> &'static str {
    let mut field_type: Option<&'static str> = None;
    for value in values {
        if value.is_null() {
            continue;
        }
        let next = match value {
            serde_json::Value::String(_) => "string",
            serde_json::Value::Number(value) if value.is_i64() || value.is_u64() => "integer",
            serde_json::Value::Number(_) => "number",
            serde_json::Value::Bool(_) => "boolean",
            serde_json::Value::Array(_) | serde_json::Value::Object(_) => "opaque_json",
            serde_json::Value::Null => continue,
        };
        match field_type {
            None => field_type = Some(next),
            Some(current) if current == next => {}
            Some(_) => return "opaque_json",
        }
    }
    field_type.unwrap_or("opaque_json")
}

pub fn import_confluence_bytes(
    loom: &mut Loom<FileStore>,
    ns: WorkspaceId,
    workspace_id: &str,
    source_path: &str,
    default_space_id: &str,
    bytes: &[u8],
    dry_run: bool,
) -> Result<ImportReport> {
    let parsed: ConfluenceImportSnapshot = serde_json::from_slice(bytes).map_err(|e| {
        loom_types::LoomError::invalid(format!("parse Confluence snapshot JSON: {e}"))
    })?;
    import_confluence_snapshot(
        loom,
        ns,
        workspace_id,
        source_path,
        default_space_id,
        bytes.len() as u64,
        parsed,
        dry_run,
    )
}

pub fn import_confluence_snapshot(
    loom: &mut Loom<FileStore>,
    ns: WorkspaceId,
    workspace_id: &str,
    fallback_source_scope: &str,
    default_space_id: &str,
    bytes_in: u64,
    parsed: ConfluenceImportSnapshot,
    dry_run: bool,
) -> Result<ImportReport> {
    let source_scope = parsed
        .source_scope
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(fallback_source_scope);
    let mut planned_space_ids = parsed
        .spaces
        .iter()
        .map(confluence_space_id)
        .collect::<Result<BTreeSet<_>>>()?;
    planned_space_ids.extend(
        parsed
            .pages
            .iter()
            .map(|page| confluence_page_space_id(page, default_space_id).to_string()),
    );
    let mut report = ImportReport::new(ImportReportInput {
        profile: "confluence",
        source_scope,
        commit: None,
        objects_added: 0,
        bytes_in,
        bytes_stored: 0,
        rows_imported: 0,
        skipped: 0,
        operations_planned: (parsed.pages.len() + planned_space_ids.len()) as u64,
        operations_applied: 0,
        dry_run,
    })?;
    for space in &parsed.spaces {
        add_confluence_space_fidelity_issues(&mut report, space)?;
    }
    for page in &parsed.pages {
        add_confluence_fidelity_issues(&mut report, page)?;
    }
    if dry_run {
        return Ok(report);
    }

    let mut spaces = BTreeSet::new();
    for space in &parsed.spaces {
        let space_id = confluence_space_id(space)?;
        if spaces.insert(space_id.clone()) {
            match loom_pages::create_space(
                loom,
                ns,
                workspace_id,
                &space_id,
                confluence_space_title(space)
                    .as_deref()
                    .unwrap_or(&space_id),
                None,
            ) {
                Ok(_) => {
                    report.operations_applied += 1;
                }
                Err(error) if error.code == Code::AlreadyExists => {
                    report.skipped += 1;
                }
                Err(error) => return Err(error),
            }
        }
    }
    for page in &parsed.pages {
        let space = confluence_page_space_id(page, default_space_id);
        if spaces.insert(space.to_string()) {
            match loom_pages::create_space(loom, ns, workspace_id, space, space, None) {
                Ok(_) => {
                    report.operations_applied += 1;
                }
                Err(error) if error.code == Code::AlreadyExists => {
                    report.skipped += 1;
                }
                Err(error) => return Err(error),
            }
        }
        let body = confluence_page_body(page)?;
        let body_bytes = body.encode()?;
        apply_page_import(
            loom,
            ns,
            workspace_id,
            space,
            &stable_page_id(&page.id),
            &page.title,
            page.parent_page_id.as_deref(),
            body_bytes,
            &mut report,
        )?;
        report.bytes_stored += confluence_page_source_len(page) as u64;
    }
    Ok(report)
}

pub fn import_markdown_path(
    loom: &mut Loom<FileStore>,
    ns: WorkspaceId,
    workspace_id: &str,
    source_scope: &str,
    src: &Path,
    space_id: &str,
    dry_run: bool,
) -> Result<ImportReport> {
    let items = collect_markdown_import_items(src)?;
    let bytes_in = items
        .iter()
        .map(|item| {
            std::fs::metadata(&item.path)
                .map(|meta| meta.len())
                .unwrap_or(0)
        })
        .sum();
    let mut report = ImportReport::new(ImportReportInput {
        profile: "markdown",
        source_scope,
        commit: None,
        objects_added: 0,
        bytes_in,
        bytes_stored: 0,
        rows_imported: 0,
        skipped: 0,
        operations_planned: (items.len() + 1) as u64,
        operations_applied: 0,
        dry_run,
    })?;
    add_markdown_vault_fidelity_issues(&mut report, src)?;
    for item in &items {
        add_markdown_fidelity_issues(&mut report, item)?;
    }
    if dry_run {
        return Ok(report);
    }

    match loom_pages::create_space(loom, ns, workspace_id, space_id, "Markdown", None) {
        Ok(_) => {
            report.operations_applied += 1;
        }
        Err(error) if error.code == Code::AlreadyExists => {
            report.skipped += 1;
        }
        Err(error) => return Err(error),
    }
    for item in items {
        let markdown = std::fs::read_to_string(&item.path)
            .map_err(|e| loom_types::LoomError::new(Code::Io, e.to_string()))?;
        let body = markdown_body(&markdown)?;
        let body_bytes = body.encode()?;
        apply_page_import(
            loom,
            ns,
            workspace_id,
            space_id,
            &item.page_id,
            &item.title,
            None,
            body_bytes,
            &mut report,
        )?;
        report.bytes_stored += markdown.len() as u64;
    }
    Ok(report)
}

fn collect_markdown_import_items(src: &Path) -> Result<Vec<MarkdownImportItem>> {
    let mut files = Vec::new();
    collect_markdown_files(src, src, &mut files)?;
    files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    let mut seen = BTreeSet::new();
    for item in &mut files {
        let base = item.page_id.clone();
        let mut suffix = 2;
        while !seen.insert(item.page_id.clone()) {
            item.page_id = format!("{base}-{suffix}");
            suffix += 1;
        }
    }
    Ok(files)
}

fn collect_markdown_files(
    root: &Path,
    current: &Path,
    files: &mut Vec<MarkdownImportItem>,
) -> Result<()> {
    for entry in std::fs::read_dir(current)
        .map_err(|e| loom_types::LoomError::new(Code::Io, e.to_string()))?
    {
        let entry = entry.map_err(|e| loom_types::LoomError::new(Code::Io, e.to_string()))?;
        let path = entry.path();
        let metadata = entry
            .metadata()
            .map_err(|e| loom_types::LoomError::new(Code::Io, e.to_string()))?;
        if metadata.is_dir() {
            collect_markdown_files(root, &path, files)?;
        } else if metadata.is_file()
            && path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
        {
            let relative = path
                .strip_prefix(root)
                .map_err(|e| loom_types::LoomError::invalid(e.to_string()))?
                .to_string_lossy()
                .replace('\\', "/");
            let (page_id, title) = markdown_page_identity(&relative, &path);
            let markdown = std::fs::read_to_string(&path)
                .map_err(|e| loom_types::LoomError::new(Code::Io, e.to_string()))?;
            files.push(MarkdownImportItem {
                page_id,
                path,
                relative_path: relative,
                title,
                unsupported_fields: markdown_unsupported_fields(&markdown),
            });
        }
    }
    Ok(())
}

fn add_markdown_fidelity_issues(
    report: &mut ImportReport,
    item: &MarkdownImportItem,
) -> Result<()> {
    for field in &item.unsupported_fields {
        add_unsupported(
            report,
            &format!("page:{}", item.relative_path),
            field,
            "Markdown or Obsidian construct is not lowered by this importer slice",
        )?;
    }
    Ok(())
}

fn add_markdown_vault_fidelity_issues(report: &mut ImportReport, src: &Path) -> Result<()> {
    let fields = markdown_vault_unsupported_fields(src)?;
    for field in fields {
        add_unsupported(
            report,
            "vault",
            field,
            "Markdown or Obsidian vault construct is not lowered by this importer slice",
        )?;
    }
    Ok(())
}

fn markdown_vault_unsupported_fields(src: &Path) -> Result<Vec<&'static str>> {
    let mut fields = BTreeSet::new();
    if src.join(".obsidian").is_dir() {
        fields.insert("obsidian-config");
    }
    collect_markdown_vault_unsupported_fields(src, &mut fields)?;
    Ok(fields.into_iter().collect())
}

fn collect_markdown_vault_unsupported_fields(
    current: &Path,
    fields: &mut BTreeSet<&'static str>,
) -> Result<()> {
    for entry in std::fs::read_dir(current).map_err(|e| {
        loom_types::LoomError::new(
            Code::Io,
            format!("read directory {}: {e}", current.display()),
        )
    })? {
        let entry = entry.map_err(|e| loom_types::LoomError::new(Code::Io, e.to_string()))?;
        let path = entry.path();
        let metadata = entry.metadata().map_err(|e| {
            loom_types::LoomError::new(
                Code::Io,
                format!("stat import path {}: {e}", path.display()),
            )
        })?;
        if metadata.is_dir() {
            collect_markdown_vault_unsupported_fields(&path, fields)?;
        } else if metadata.is_file() {
            match path
                .extension()
                .and_then(|ext| ext.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase()
                .as_str()
            {
                "canvas" => {
                    fields.insert("canvas");
                }
                "excalidraw" => {
                    fields.insert("excalidraw");
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn markdown_page_identity(relative: &str, path: &Path) -> (String, String) {
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .unwrap_or("Untitled");
    let Some((parent, _)) = relative.rsplit_once('/') else {
        return (stable_page_id(relative), stem.to_string());
    };
    let folder = parent.rsplit('/').next().unwrap_or(parent);
    if stem.eq_ignore_ascii_case(folder) || stem.eq_ignore_ascii_case("index") {
        return (stable_page_id(parent), folder.to_string());
    }
    (stable_page_id(relative), stem.to_string())
}

fn markdown_unsupported_fields(markdown: &str) -> Vec<&'static str> {
    let mut fields = BTreeSet::new();
    if has_yaml_frontmatter(markdown) {
        fields.insert("frontmatter");
    }
    if markdown.contains("[[") {
        fields.insert("wikilinks");
    }
    if markdown.contains("](") {
        fields.insert("markdown-links");
    }
    if markdown.contains("~~") {
        fields.insert("strikethrough");
    }
    if markdown.contains("==") {
        fields.insert("highlights");
    }
    if markdown.contains("%%") {
        fields.insert("obsidian-comments");
    }
    if markdown.contains('`') {
        fields.insert("inline-code");
    }
    if markdown.lines().any(markdown_line_is_autolink) {
        fields.insert("autolinks");
    }
    if markdown.lines().any(markdown_line_has_inline_html) {
        fields.insert("html");
    }
    if markdown_has_generic_code_block(markdown) {
        fields.insert("code-blocks");
    }
    if markdown
        .lines()
        .any(|line| line.split_whitespace().any(|token| token.starts_with('^')))
    {
        fields.insert("block-ids");
    }
    if markdown_has_unsupported_attachment(markdown) {
        fields.insert("attachments");
    }
    if markdown
        .lines()
        .any(|line| line.trim_start().starts_with("> [!"))
    {
        fields.insert("callouts");
    }
    if markdown.lines().any(|line| {
        let trimmed = line.trim();
        trimmed.starts_with('|') && trimmed.ends_with('|')
    }) {
        fields.insert("tables");
    }
    if markdown.contains("[^") {
        fields.insert("footnotes");
    }
    if markdown.contains("$$") {
        fields.insert("equations");
    }
    if markdown.contains("```dataview") {
        fields.insert("dataview");
    }
    if markdown.lines().any(|line| line.contains("::")) {
        fields.insert("dataview");
    }
    if markdown.lines().any(|line| line.contains("tasks")) {
        fields.insert("tasks-plugin");
    }
    if markdown.contains("```mermaid") {
        fields.insert("mermaid");
    }
    if markdown.to_ascii_lowercase().contains("excalidraw") {
        fields.insert("excalidraw");
    }
    fields.into_iter().collect()
}

fn markdown_line_is_autolink(line: &str) -> bool {
    line.split_whitespace().any(|token| {
        token.starts_with("<http://")
            || token.starts_with("<https://")
            || token.starts_with("<mailto:")
    })
}

fn markdown_line_has_inline_html(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with('<')
        && !trimmed.starts_with("<!--")
        && !trimmed.starts_with("<http://")
        && !trimmed.starts_with("<https://")
        && !trimmed.starts_with("<mailto:")
}

fn markdown_has_generic_code_block(markdown: &str) -> bool {
    markdown.lines().any(|line| {
        let trimmed = line.trim_start();
        let Some(info) = trimmed.strip_prefix("```") else {
            return false;
        };
        let info = info.trim().to_ascii_lowercase();
        !info.is_empty() && !matches!(info.as_str(), "dataview" | "mermaid")
    })
}

fn has_yaml_frontmatter(markdown: &str) -> bool {
    let mut lines = markdown.lines();
    if lines.next().map(str::trim) != Some("---") {
        return false;
    }
    lines.any(|line| line.trim() == "---")
}

fn markdown_has_unsupported_attachment(markdown: &str) -> bool {
    markdown.lines().any(|line| {
        let trimmed = line.trim();
        if trimmed.starts_with("![[") {
            return obsidian_embed_target(trimmed).is_none();
        }
        trimmed.contains("![")
    })
}

fn redmine_project_id(project: &RedmineProject) -> Result<String> {
    project
        .identifier
        .clone()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| redmine_id_string(&project.id))
        .ok_or_else(|| {
            loom_types::LoomError::invalid(
                "Redmine project id must be a string, integer, or identifier",
            )
        })
}

fn redmine_project_key_prefix(project: &RedmineProject) -> Result<String> {
    if let Some(prefix) = project.key_prefix.as_ref() {
        return Ok(prefix.clone());
    }
    let fallback_id = redmine_id_string(&project.id);
    let seed = project
        .identifier
        .as_deref()
        .or(fallback_id.as_deref())
        .unwrap_or(&project.name);
    let mut prefix = seed
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .take(10)
        .collect::<String>()
        .to_ascii_uppercase();
    if prefix.len() < 2 {
        prefix = "RM".to_string();
    }
    if !prefix
        .as_bytes()
        .first()
        .is_some_and(u8::is_ascii_alphabetic)
    {
        prefix.insert(0, 'R');
        prefix.truncate(10);
    }
    Ok(prefix)
}

fn redmine_issue_project_id(issue: &RedmineIssue) -> Result<String> {
    issue
        .project_identifier
        .clone()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| issue.project_id.as_ref().and_then(redmine_id_string))
        .ok_or_else(|| {
            loom_types::LoomError::invalid(
                "Redmine issue project_id or project_identifier is required for ticket lowering",
            )
        })
}

fn redmine_source_id(kind: &str, value: &serde_json::Value) -> Result<String> {
    redmine_id_string(value)
        .map(|id| format!("{kind}:{id}"))
        .ok_or_else(|| {
            loom_types::LoomError::invalid(format!("Redmine {kind} id must be a string or integer"))
        })
}

fn redmine_id_string(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(value) if !value.trim().is_empty() => Some(value.clone()),
        serde_json::Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

fn redmine_ticket_type(tracker: Option<&str>) -> &'static str {
    match tracker.unwrap_or("").to_ascii_lowercase().as_str() {
        "bug" | "defect" => "bug",
        "feature" | "story" | "user story" => "story",
        "epic" => "epic",
        "spike" => "spike",
        "subtask" | "sub-task" => "subtask",
        _ => "task",
    }
}

fn redmine_issue_fields(
    issue: &RedmineIssue,
    linked_time_entries: &[serde_json::Value],
) -> Result<serde_json::Value> {
    let mut fields = serde_json::Map::new();
    fields.insert("subject".to_string(), issue.subject.clone().into());
    insert_optional_json_text(&mut fields, "description", issue.description.as_deref());
    insert_optional_json_text(&mut fields, "status", issue.status.as_deref());
    insert_optional_json_text(&mut fields, "priority", issue.priority.as_deref());
    insert_optional_json_text(&mut fields, "category", issue.category.as_deref());
    insert_optional_json_text(&mut fields, "assigned_to", issue.assigned_to.as_deref());
    insert_optional_json_text(&mut fields, "author", issue.author.as_deref());
    insert_optional_json_text(
        &mut fields,
        "created_at",
        issue.created_at.as_deref().or(issue.created_on.as_deref()),
    );
    insert_optional_json_text(
        &mut fields,
        "updated_at",
        issue.updated_at.as_deref().or(issue.updated_on.as_deref()),
    );
    insert_optional_json_text(&mut fields, "start_date", issue.start_date.as_deref());
    insert_optional_json_text(&mut fields, "due_date", issue.due_date.as_deref());
    insert_optional_json_text(&mut fields, "closed_on", issue.closed_on.as_deref());
    if let Some(done_ratio) = &issue.done_ratio {
        fields.insert("done_ratio".to_string(), done_ratio.clone());
    }
    if let Some(estimated_hours) = &issue.estimated_hours {
        fields.insert("estimated_hours".to_string(), estimated_hours.clone());
    }
    insert_optional_json_text(&mut fields, "fixed_version", issue.fixed_version.as_deref());
    insert_optional_json_text(
        &mut fields,
        "affected_version",
        issue.affected_version.as_deref(),
    );
    if let Some(parent_issue_id) = &issue.parent_issue_id {
        fields.insert("parent_issue_id".to_string(), parent_issue_id.clone());
    }
    if let Some(is_private) = &issue.is_private {
        fields.insert("is_private".to_string(), is_private.clone());
    }
    insert_optional_json_text(&mut fields, "url", issue.url.as_deref());
    if let Some(tracker) = issue.tracker.as_deref().filter(|value| !value.is_empty()) {
        fields.insert("tracker".to_string(), tracker.to_string().into());
    }
    fields.insert(
        "redmine_issue_id".to_string(),
        redmine_source_id("issue", &issue.id)?.into(),
    );
    if let Some(custom_fields) = &issue.custom_fields {
        fields.insert("custom_fields".to_string(), custom_fields.clone());
    }
    insert_non_empty_json_array(&mut fields, "redmine_journals", &issue.journals);
    insert_non_empty_json_array(&mut fields, "redmine_comments", &issue.comments);
    insert_non_empty_json_array(&mut fields, "redmine_watchers", &issue.watchers);
    insert_non_empty_json_array(
        &mut fields,
        "redmine_affected_versions",
        &issue.affected_versions,
    );
    insert_non_empty_json_array(&mut fields, "redmine_attachments", &issue.attachments);
    let mut time_entries = issue.time_entries.clone();
    time_entries.extend_from_slice(linked_time_entries);
    insert_non_empty_json_array(&mut fields, "redmine_time_entries", &time_entries);
    insert_non_empty_json_array(&mut fields, "redmine_relations", &issue.relations);
    insert_non_empty_json_array(&mut fields, "redmine_children", &issue.children);
    insert_non_empty_json_array(&mut fields, "redmine_changesets", &issue.changesets);
    insert_non_empty_json_array(
        &mut fields,
        "redmine_allowed_statuses",
        &issue.allowed_statuses,
    );
    Ok(serde_json::Value::Object(fields))
}

fn insert_non_empty_json_array(
    fields: &mut serde_json::Map<String, serde_json::Value>,
    key: &str,
    values: &[serde_json::Value],
) {
    if !values.is_empty() {
        fields.insert(key.to_string(), serde_json::Value::Array(values.to_vec()));
    }
}

fn redmine_wiki_space_ids(pages: &[RedmineWikiPage]) -> Result<BTreeSet<String>> {
    pages.iter().map(redmine_wiki_page_space_id).collect()
}

fn redmine_wiki_page_space_id(page: &RedmineWikiPage) -> Result<String> {
    page.space_id
        .clone()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            page.project_identifier
                .clone()
                .filter(|value| !value.trim().is_empty())
        })
        .or_else(|| page.project_id.as_ref().and_then(redmine_id_string))
        .ok_or_else(|| {
            loom_types::LoomError::invalid(
                "Redmine wiki page space_id, project_identifier, or project_id is required",
            )
        })
}

fn redmine_wiki_page_id(page: &RedmineWikiPage) -> Result<String> {
    page.page_id
        .clone()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| redmine_id_string(&page.id).map(|id| stable_page_id(&id)))
        .ok_or_else(|| {
            loom_types::LoomError::invalid("Redmine wiki page id must be a string or integer")
        })
}

fn redmine_wiki_page_markdown(page: &RedmineWikiPage) -> String {
    page.markdown
        .as_ref()
        .or(page.body.as_ref())
        .or(page.text.as_ref())
        .cloned()
        .unwrap_or_default()
}

fn add_redmine_project_fidelity_issues(
    report: &mut ImportReport,
    project: &RedmineProject,
) -> Result<()> {
    let source = format!("project:{}", redmine_project_id(project)?);
    if redmine_project_has_unstored_metadata(project) {
        add_unsupported(
            report,
            &source,
            "project_metadata",
            "Redmine project metadata is not lowered by this importer slice",
        )?;
    }
    add_unsupported_list(
        report,
        &source,
        "trackers",
        &project.trackers,
        "Redmine project tracker definitions are not lowered by this importer slice",
    )?;
    add_unsupported_list(
        report,
        &source,
        "issue_categories",
        &project.issue_categories,
        "Redmine project issue categories are not lowered by this importer slice",
    )?;
    add_unsupported_list(
        report,
        &source,
        "enabled_modules",
        &project.enabled_modules,
        "Redmine project enabled modules are not lowered by this importer slice",
    )?;
    add_unsupported_list(
        report,
        &source,
        "time_entry_activities",
        &project.time_entry_activities,
        "Redmine project time-entry activities are not lowered by this importer slice",
    )?;
    add_unsupported_list(
        report,
        &source,
        "issue_custom_fields",
        &project.issue_custom_fields,
        "Redmine project issue custom-field definitions are not lowered by this importer slice",
    )
}

fn redmine_project_has_unstored_metadata(project: &RedmineProject) -> bool {
    project.description.is_some()
        || project.homepage.is_some()
        || project.status.is_some()
        || project.is_public.is_some()
        || project.parent.is_some()
        || project.default_version.is_some()
        || project.default_assignee.is_some()
        || project.created_on.is_some()
        || project.updated_on.is_some()
}

fn add_redmine_wiki_page_fidelity_issues(
    report: &mut ImportReport,
    page: &RedmineWikiPage,
) -> Result<()> {
    let source = format!("wiki_page:{}", redmine_wiki_page_id(page)?);
    if redmine_wiki_page_has_revision_metadata(page) {
        add_unsupported(
            report,
            &source,
            "wiki_revision_metadata",
            "Redmine wiki revision metadata is not lowered by this importer slice",
        )?;
    }
    add_unsupported_list(
        report,
        &source,
        "attachments",
        &page.attachments,
        "Redmine wiki attachments are not lowered by this importer slice",
    )
}

fn redmine_wiki_page_has_revision_metadata(page: &RedmineWikiPage) -> bool {
    page.version.is_some()
        || page.author.is_some()
        || page.comments.is_some()
        || page.created_on.is_some()
        || page.updated_on.is_some()
        || page.parent_title.is_some()
}

fn add_asana_project_fidelity_issues(
    report: &mut ImportReport,
    project: &AsanaProject,
) -> Result<()> {
    let source = asana_source_id("project", &project.gid)?;
    add_unsupported_option(
        report,
        &source,
        "archived",
        project.archived.as_ref(),
        "Asana project archived state is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &source,
        "color",
        project.color.as_ref(),
        "Asana project color is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &source,
        "icon",
        project.icon.as_ref(),
        "Asana project icon is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &source,
        "created_at",
        project.created_at.as_ref(),
        "Asana project creation time is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &source,
        "modified_at",
        project.modified_at.as_ref(),
        "Asana project modification time is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &source,
        "start_on",
        project.start_on.as_ref(),
        "Asana project start date is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &source,
        "due_on",
        project.due_on.as_ref(),
        "Asana project due date is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &source,
        "default_view",
        project.default_view.as_ref(),
        "Asana project default view is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &source,
        "permalink_url",
        project.permalink_url.as_ref(),
        "Asana project permalink is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &source,
        "workspace",
        project.workspace.as_ref(),
        "Asana project workspace metadata is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &source,
        "team",
        project.team.as_ref(),
        "Asana project team metadata is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &source,
        "owner",
        project.owner.as_ref(),
        "Asana project owner metadata is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &source,
        "current_status",
        project.current_status.as_ref(),
        "Asana project status updates are not lowered by this importer slice",
    )?;
    add_unsupported_list(
        report,
        &source,
        "custom_field_settings",
        &project.custom_field_settings,
        "Asana project custom-field settings are not lowered by this importer slice",
    )?;
    add_unsupported_list(
        report,
        &source,
        "members",
        &project.members,
        "Asana project members are not lowered by this importer slice",
    )?;
    add_unsupported_list(
        report,
        &source,
        "followers",
        &project.followers,
        "Asana project followers are not lowered by this importer slice",
    )
}

fn add_asana_task_fidelity_issues(report: &mut ImportReport, task: &AsanaTask) -> Result<()> {
    let source = asana_source_id("task", &task.gid)?;
    add_unsupported_list(
        report,
        &source,
        "memberships",
        &task.memberships,
        "Asana project and section memberships are not lowered by this importer slice",
    )?;
    add_unsupported_list(
        report,
        &source,
        "subtasks",
        &task.subtasks,
        "Asana subtasks are not lowered by this importer slice",
    )?;
    add_unsupported_list(
        report,
        &source,
        "stories",
        &task.stories,
        "Asana stories are not lowered by this importer slice",
    )?;
    add_unsupported_list(
        report,
        &source,
        "attachments",
        &task.attachments,
        "Asana attachments are not lowered by this importer slice",
    )?;
    add_unsupported_list(
        report,
        &source,
        "portfolios",
        &task.portfolios,
        "Asana portfolio membership is not lowered by this importer slice",
    )?;
    add_unsupported_list(
        report,
        &source,
        "goals",
        &task.goals,
        "Asana goal links are not lowered by this importer slice",
    )
}

fn asana_project_id(project: &AsanaProject) -> Result<String> {
    asana_id_string(&project.gid).ok_or_else(|| {
        loom_types::LoomError::invalid("Asana project gid must be a string or integer")
    })
}

fn asana_project_key_prefix(project: &AsanaProject) -> Result<String> {
    if let Some(prefix) = project.key_prefix.as_ref() {
        return Ok(prefix.clone());
    }
    let mut prefix = project
        .name
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .take(10)
        .collect::<String>()
        .to_ascii_uppercase();
    if prefix.len() < 2 {
        prefix = "AS".to_string();
    }
    if !prefix
        .as_bytes()
        .first()
        .is_some_and(u8::is_ascii_alphabetic)
    {
        prefix.insert(0, 'A');
        prefix.truncate(10);
    }
    Ok(prefix)
}

fn asana_task_project_id(task: &AsanaTask) -> Result<String> {
    task.project_gid
        .as_ref()
        .or(task.project_id.as_ref())
        .and_then(asana_id_string)
        .ok_or_else(|| {
            loom_types::LoomError::invalid("Asana task project_gid or project_id is required")
        })
}

fn asana_source_id(kind: &str, value: &serde_json::Value) -> Result<String> {
    asana_id_string(value)
        .map(|id| format!("{kind}:{id}"))
        .ok_or_else(|| {
            loom_types::LoomError::invalid(format!("Asana {kind} gid must be a string or integer"))
        })
}

fn asana_id_string(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(value) if !value.trim().is_empty() => Some(value.clone()),
        serde_json::Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

fn asana_ticket_type(resource_subtype: Option<&str>) -> &'static str {
    match resource_subtype.unwrap_or("").to_ascii_lowercase().as_str() {
        "approval" => "task",
        "milestone" => "task",
        _ => "task",
    }
}

fn asana_task_fields(task: &AsanaTask) -> Result<serde_json::Value> {
    let mut fields = serde_json::Map::new();
    fields.insert("subject".to_string(), task.name.clone().into());
    insert_optional_json_text(&mut fields, "description", task.notes.as_deref());
    insert_optional_json_text(
        &mut fields,
        "resource_subtype",
        task.resource_subtype.as_deref(),
    );
    insert_optional_json_text(
        &mut fields,
        "approval_status",
        task.approval_status.as_deref(),
    );
    insert_optional_json_text(
        &mut fields,
        "assignee_status",
        task.assignee_status.as_deref(),
    );
    insert_optional_json_text(&mut fields, "completed_at", task.completed_at.as_deref());
    insert_optional_json_text(&mut fields, "created_at", task.created_at.as_deref());
    insert_optional_json_text(&mut fields, "modified_at", task.modified_at.as_deref());
    insert_optional_json_text(&mut fields, "html_notes", task.html_notes.as_deref());
    insert_optional_json_text(&mut fields, "due_on", task.due_on.as_deref());
    insert_optional_json_text(&mut fields, "due_at", task.due_at.as_deref());
    insert_optional_json_text(&mut fields, "start_on", task.start_on.as_deref());
    insert_optional_json_text(&mut fields, "start_at", task.start_at.as_deref());
    if let Some(completed) = task.completed {
        fields.insert("completed".to_string(), completed.into());
    }
    if let Some(liked) = task.liked {
        fields.insert("liked".to_string(), liked.into());
    }
    if let Some(num_likes) = task.num_likes {
        fields.insert("num_likes".to_string(), num_likes.into());
    }
    if let Some(num_subtasks) = task.num_subtasks {
        fields.insert("num_subtasks".to_string(), num_subtasks.into());
    }
    if let Some(actual_time_minutes) = task.actual_time_minutes {
        fields.insert(
            "actual_time_minutes".to_string(),
            actual_time_minutes.into(),
        );
    }
    if let Some(is_rendered_as_separator) = task.is_rendered_as_separator {
        fields.insert(
            "is_rendered_as_separator".to_string(),
            is_rendered_as_separator.into(),
        );
    }
    if !task.tags.is_empty() {
        fields.insert("tags".to_string(), task.tags.clone().into());
    }
    insert_optional_json_value(&mut fields, "assignee", task.assignee.as_ref());
    insert_optional_json_value(&mut fields, "assigned_by", task.assigned_by.as_ref());
    insert_optional_json_value(
        &mut fields,
        "assignee_section",
        task.assignee_section.as_ref(),
    );
    insert_optional_json_value(&mut fields, "completed_by", task.completed_by.as_ref());
    insert_optional_json_value(&mut fields, "created_by", task.created_by.as_ref());
    insert_optional_json_value(&mut fields, "workspace", task.workspace.as_ref());
    insert_optional_json_value(&mut fields, "parent", task.parent.as_ref());
    insert_optional_json_value(&mut fields, "external", task.external.as_ref());
    insert_non_empty_json_list(&mut fields, "dependencies", &task.dependencies);
    insert_non_empty_json_list(&mut fields, "dependents", &task.dependents);
    insert_non_empty_json_list(&mut fields, "memberships", &task.memberships);
    insert_non_empty_json_list(&mut fields, "followers", &task.followers);
    insert_non_empty_json_list(&mut fields, "likes", &task.likes);
    fields.insert(
        "asana_task_gid".to_string(),
        asana_source_id("task", &task.gid)?.into(),
    );
    if let Some(custom_fields) = &task.custom_fields {
        fields.insert("custom_fields".to_string(), custom_fields.clone());
    }
    Ok(serde_json::Value::Object(fields))
}

fn add_jira_project_fidelity_issues(
    report: &mut ImportReport,
    project: &JiraProject,
) -> Result<()> {
    let source = jira_source_id("project", &project.id)?;
    add_unsupported_option(
        report,
        &source,
        "description",
        project.description.as_ref(),
        "Jira project description is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &source,
        "project_type_key",
        project.project_type_key.as_ref(),
        "Jira project type is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &source,
        "style",
        project.style.as_ref(),
        "Jira project style is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &source,
        "simplified",
        project.simplified.as_ref(),
        "Jira simplified project flag is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &source,
        "archived",
        project.archived.as_ref(),
        "Jira project archive state is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &source,
        "deleted",
        project.deleted.as_ref(),
        "Jira project deleted state is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &source,
        "private",
        project.private.as_ref(),
        "Jira project privacy flag is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &source,
        "self_url",
        project.self_url.as_ref(),
        "Jira project API URL is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &source,
        "avatar_urls",
        project.avatar_urls.as_ref(),
        "Jira project avatars are not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &source,
        "lead",
        project.lead.as_ref(),
        "Jira project lead is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &source,
        "project_category",
        project.project_category.as_ref(),
        "Jira project category is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &source,
        "insight",
        project.insight.as_ref(),
        "Jira project insight is not lowered by this importer slice",
    )?;
    add_unsupported_list(
        report,
        &source,
        "issue_types",
        &project.issue_types,
        "Jira project issue-type schema is not lowered by this importer slice",
    )?;
    add_unsupported_list(
        report,
        &source,
        "components",
        &project.components,
        "Jira project components are not lowered by this importer slice",
    )?;
    add_unsupported_list(
        report,
        &source,
        "versions",
        &project.versions,
        "Jira project versions are not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &source,
        "roles",
        project.roles.as_ref(),
        "Jira project roles are not lowered by this importer slice",
    )
}

fn add_jira_issue_fidelity_issues(report: &mut ImportReport, issue: &JiraIssue) -> Result<()> {
    let source = jira_source_id("issue", &issue.id)?;
    add_unsupported_list(
        report,
        &source,
        "issue_links",
        &issue.issue_links,
        "Jira issue links are retained but not lowered as native typed relations by this importer slice",
    )?;
    add_unsupported_list(
        report,
        &source,
        "subtasks",
        &issue.subtasks,
        "Jira subtasks are retained but not lowered as native hierarchy by this importer slice",
    )?;
    add_unsupported_list(
        report,
        &source,
        "transitions",
        &issue.transitions,
        "Jira transitions are retained but not lowered as native workflow transitions by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &source,
        "changelog",
        issue.changelog.as_ref(),
        "Jira changelog is not lowered by this importer slice",
    )?;
    add_unsupported_list(
        report,
        &source,
        "comments",
        &issue.comments,
        "Jira comments are not lowered by this importer slice",
    )?;
    add_unsupported_list(
        report,
        &source,
        "attachments",
        &issue.attachments,
        "Jira attachments are not lowered by this importer slice",
    )?;
    add_unsupported_list(
        report,
        &source,
        "worklog",
        &issue.worklog,
        "Jira worklog is not lowered by this importer slice",
    )
}

fn jira_project_id(project: &JiraProject) -> Result<String> {
    if !project.key.trim().is_empty() {
        Ok(project.key.clone())
    } else {
        jira_id_string(&project.id).ok_or_else(|| {
            loom_types::LoomError::invalid("Jira project id must be a string or integer")
        })
    }
}

fn jira_project_key_prefix(project: &JiraProject) -> Result<String> {
    let prefix = project.key.trim();
    if prefix.is_empty() {
        return Err(loom_types::LoomError::invalid(
            "Jira project key must not be empty",
        ));
    }
    Ok(prefix.to_ascii_uppercase())
}

fn jira_issue_project_id(issue: &JiraIssue) -> Result<String> {
    issue
        .project_key
        .clone()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| issue.project_id.as_ref().and_then(jira_id_string))
        .ok_or_else(|| {
            loom_types::LoomError::invalid("Jira issue project_key or project_id is required")
        })
}

fn jira_source_id(kind: &str, value: &serde_json::Value) -> Result<String> {
    jira_id_string(value)
        .map(|id| format!("{kind}:{id}"))
        .ok_or_else(|| {
            loom_types::LoomError::invalid(format!("Jira {kind} id must be a string or integer"))
        })
}

fn jira_id_string(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(value) if !value.trim().is_empty() => Some(value.clone()),
        serde_json::Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

fn jira_ticket_type(issue_type: Option<&str>) -> &'static str {
    match issue_type.unwrap_or("").to_ascii_lowercase().as_str() {
        "bug" | "defect" => "bug",
        "story" | "user story" => "story",
        "epic" => "epic",
        "spike" => "spike",
        "subtask" | "sub-task" => "subtask",
        _ => "task",
    }
}

fn jira_issue_fields(issue: &JiraIssue) -> Result<serde_json::Value> {
    let mut fields = serde_json::Map::new();
    fields.insert("subject".to_string(), issue.summary.clone().into());
    fields.insert("jira_issue_key".to_string(), issue.key.clone().into());
    insert_optional_json_value(&mut fields, "description", issue.description.as_ref());
    insert_optional_json_text(&mut fields, "issue_type", issue.issue_type.as_deref());
    insert_optional_json_text(&mut fields, "status", issue.status.as_deref());
    insert_optional_json_text(
        &mut fields,
        "status_category",
        issue.status_category.as_deref(),
    );
    insert_optional_json_text(&mut fields, "priority", issue.priority.as_deref());
    insert_optional_json_value(&mut fields, "resolution", issue.resolution.as_ref());
    insert_optional_json_text(
        &mut fields,
        "resolution_date",
        issue.resolution_date.as_deref(),
    );
    insert_optional_json_value(&mut fields, "assignee", issue.assignee.as_ref());
    insert_optional_json_value(&mut fields, "reporter", issue.reporter.as_ref());
    insert_optional_json_value(&mut fields, "creator", issue.creator.as_ref());
    insert_optional_json_text(&mut fields, "created_at", issue.created_at.as_deref());
    insert_optional_json_text(&mut fields, "updated_at", issue.updated_at.as_deref());
    insert_optional_json_text(&mut fields, "due_date", issue.due_date.as_deref());
    insert_optional_json_value(&mut fields, "environment", issue.environment.as_ref());
    insert_optional_json_value(&mut fields, "parent", issue.parent.as_ref());
    insert_optional_json_value(&mut fields, "security", issue.security.as_ref());
    insert_optional_json_value(&mut fields, "votes", issue.votes.as_ref());
    insert_optional_json_value(&mut fields, "watches", issue.watches.as_ref());
    insert_optional_json_value(&mut fields, "sprint", issue.sprint.as_ref());
    if !issue.labels.is_empty() {
        fields.insert("labels".to_string(), issue.labels.clone().into());
    }
    insert_non_empty_json_list(&mut fields, "components", &issue.components);
    insert_non_empty_json_list(&mut fields, "fix_versions", &issue.fix_versions);
    insert_non_empty_json_list(&mut fields, "affected_versions", &issue.affected_versions);
    insert_non_empty_json_list(&mut fields, "issue_links", &issue.issue_links);
    insert_non_empty_json_list(&mut fields, "subtasks", &issue.subtasks);
    insert_non_empty_json_list(&mut fields, "transitions", &issue.transitions);
    insert_optional_json_value(&mut fields, "properties", issue.properties.as_ref());
    insert_optional_json_value(&mut fields, "development", issue.development.as_ref());
    fields.insert(
        "jira_issue_id".to_string(),
        jira_source_id("issue", &issue.id)?.into(),
    );
    if let Some(custom_fields) = &issue.custom_fields {
        fields.insert("custom_fields".to_string(), custom_fields.clone());
    }
    Ok(serde_json::Value::Object(fields))
}

fn add_confluence_space_fidelity_issues(
    report: &mut ImportReport,
    space: &ConfluenceSpace,
) -> Result<()> {
    let source = format!("space:{}", confluence_space_id(space)?);
    add_unsupported_option(
        report,
        &source,
        "description",
        space.description.as_ref(),
        "Confluence space description is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &source,
        "type",
        space.r#type.as_ref(),
        "Confluence space type is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &source,
        "status",
        space.status.as_ref(),
        "Confluence space status is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &source,
        "homepage_id",
        space.homepage_id.as_ref(),
        "Confluence space homepage is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &source,
        "author_id",
        space.author_id.as_ref(),
        "Confluence space author is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &source,
        "created_at",
        space.created_at.as_ref(),
        "Confluence space creation time is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &source,
        "links",
        space.links.as_ref(),
        "Confluence space links are not lowered by this importer slice",
    )?;
    add_unsupported_list(
        report,
        &source,
        "labels",
        &space.labels,
        "Confluence space labels are not lowered by this importer slice",
    )?;
    add_unsupported_list(
        report,
        &source,
        "properties",
        &space.properties,
        "Confluence space properties are not lowered by this importer slice",
    )?;
    add_unsupported_list(
        report,
        &source,
        "permissions",
        &space.permissions,
        "Confluence space permissions are not lowered by this importer slice",
    )
}

fn add_confluence_fidelity_issues(report: &mut ImportReport, page: &ConfluencePage) -> Result<()> {
    add_unsupported_option(
        report,
        &format!("page:{}", page.id),
        "status",
        page.status.as_ref(),
        "Confluence page status is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &format!("page:{}", page.id),
        "version",
        page.version.as_ref(),
        "Confluence page version is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &format!("page:{}", page.id),
        "author_id",
        page.author_id.as_ref(),
        "Confluence page author is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &format!("page:{}", page.id),
        "owner_id",
        page.owner_id.as_ref(),
        "Confluence page owner is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &format!("page:{}", page.id),
        "created_at",
        page.created_at.as_ref(),
        "Confluence page creation time is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &format!("page:{}", page.id),
        "links",
        page.links.as_ref(),
        "Confluence page links are not lowered by this importer slice",
    )?;
    add_unsupported_list(
        report,
        &format!("page:{}", page.id),
        "ancestors",
        &page.ancestors,
        "Confluence page ancestors are not lowered by this importer slice",
    )?;
    add_unsupported_list(
        report,
        &format!("page:{}", page.id),
        "descendants",
        &page.descendants,
        "Confluence page descendants are not lowered by this importer slice",
    )?;
    add_unsupported_list(
        report,
        &format!("page:{}", page.id),
        "labels",
        &page.labels,
        "Confluence page labels are not lowered by this importer slice",
    )?;
    add_unsupported_list(
        report,
        &format!("page:{}", page.id),
        "properties",
        &page.properties,
        "Confluence page properties are not lowered by this importer slice",
    )?;
    add_unsupported_list(
        report,
        &format!("page:{}", page.id),
        "restrictions",
        &page.restrictions,
        "Confluence page restrictions are not lowered by this importer slice",
    )?;
    add_unsupported_list(
        report,
        &format!("page:{}", page.id),
        "attachments",
        &page.attachments,
        "Confluence attachments are not lowered by this importer slice",
    )?;
    add_unsupported_list(
        report,
        &format!("page:{}", page.id),
        "comments",
        &page.comments,
        "Confluence comments are not lowered by this importer slice",
    )
}

fn confluence_space_id(space: &ConfluenceSpace) -> Result<String> {
    space
        .key
        .as_ref()
        .or(Some(&space.id))
        .filter(|value| !value.trim().is_empty())
        .cloned()
        .ok_or_else(|| loom_types::LoomError::invalid("Confluence space id must not be empty"))
}

fn confluence_space_title(space: &ConfluenceSpace) -> Option<String> {
    space
        .name
        .as_ref()
        .filter(|value| !value.trim().is_empty())
        .cloned()
}

fn confluence_page_space_id<'a>(page: &'a ConfluencePage, default_space_id: &'a str) -> &'a str {
    page.space_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(default_space_id)
}

fn confluence_page_body(page: &ConfluencePage) -> Result<Body> {
    if let Some(markdown) = page.markdown.as_deref().or(page.text.as_deref()) {
        return markdown_body(markdown);
    }
    if let Some(storage) = page.storage_xhtml.as_ref() {
        return opaque_source_body("confluence.storage", storage.as_bytes());
    }
    if let Some(adf) = page.adf_json.as_ref() {
        let bytes =
            serde_json::to_vec(adf).map_err(|e| loom_types::LoomError::invalid(e.to_string()))?;
        return opaque_source_body("confluence.adf", &bytes);
    }
    Ok(Body::new(Vec::new()))
}

fn opaque_source_body(kind: &str, bytes: &[u8]) -> Result<Body> {
    Ok(Body::new(vec![Block::new(
        "b1",
        first_token(),
        BlockKind::Opaque {
            kind: kind.to_string(),
            payload: bytes.to_vec(),
        },
        Vec::new(),
        Vec::new(),
    )?]))
}

fn confluence_page_source_len(page: &ConfluencePage) -> usize {
    page.markdown
        .as_ref()
        .or(page.text.as_ref())
        .map(String::len)
        .or_else(|| page.storage_xhtml.as_ref().map(String::len))
        .or_else(|| {
            page.adf_json
                .as_ref()
                .and_then(|value| serde_json::to_vec(value).ok())
                .map(|bytes| bytes.len())
        })
        .unwrap_or(0)
}

fn insert_optional_json_text(
    fields: &mut serde_json::Map<String, serde_json::Value>,
    key: &str,
    value: Option<&str>,
) {
    if let Some(value) = value.filter(|value| !value.is_empty()) {
        fields.insert(key.to_string(), value.to_string().into());
    }
}

fn insert_optional_json_value(
    fields: &mut serde_json::Map<String, serde_json::Value>,
    key: &str,
    value: Option<&serde_json::Value>,
) {
    if let Some(value) = value.filter(|value| !value.is_null()) {
        fields.insert(key.to_string(), value.clone());
    }
}

fn insert_non_empty_json_list(
    fields: &mut serde_json::Map<String, serde_json::Value>,
    key: &str,
    values: &[serde_json::Value],
) {
    if !values.is_empty() {
        fields.insert(key.to_string(), values.to_vec().into());
    }
}

fn apply_page_import(
    loom: &mut Loom<FileStore>,
    ns: WorkspaceId,
    workspace_id: &str,
    space: &str,
    page_id: &str,
    title: &str,
    parent_page_id: Option<&str>,
    body_bytes: Vec<u8>,
    report: &mut ImportReport,
) -> Result<()> {
    match loom_pages::get_page(loom, ns, workspace_id, page_id) {
        Ok(Some(page)) if page.body.as_deref() == Some(body_bytes.as_slice()) => {
            report.skipped += 1;
        }
        Ok(Some(_)) => {
            loom_pages::update_page(loom, ns, workspace_id, page_id, body_bytes, now_ms(), None)?;
            loom_pages::publish_page(loom, ns, workspace_id, page_id, now_ms(), None)?;
            report.operations_applied += 1;
            report.rows_imported += 1;
        }
        Ok(None) => {
            loom_pages::create_page(
                loom,
                ns,
                loom_pages::PageCreateRequest {
                    workspace_id,
                    page_id,
                    space_id: space,
                    parent_page_id,
                    title,
                    expected_root: None,
                },
            )?;
            loom_pages::update_page(loom, ns, workspace_id, page_id, body_bytes, now_ms(), None)?;
            loom_pages::publish_page(loom, ns, workspace_id, page_id, now_ms(), None)?;
            report.operations_applied += 1;
            report.rows_imported += 1;
        }
        Err(error) => return Err(error),
    }
    Ok(())
}

fn stable_page_id(relative_path: &str) -> String {
    let mut out = String::new();
    for ch in relative_path.trim_end_matches(".md").chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let value = out.trim_matches('-').to_string();
    if value.is_empty() {
        "page".to_string()
    } else {
        value
    }
}

fn markdown_body(markdown: &str) -> Result<Body> {
    let mut blocks = Vec::new();
    let mut after = None;
    for (idx, line) in markdown.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let (kind, runs) = markdown_block(trimmed)?;
        let order = match after.as_ref() {
            Some(token) => insert_between(Some(token), None)?,
            None => first_token(),
        };
        after = Some(order.clone());
        blocks.push(Block::new(
            format!("b{}", idx + 1),
            order,
            kind,
            runs,
            Vec::new(),
        )?);
    }
    Ok(Body::new(blocks))
}

fn markdown_block(line: &str) -> Result<(BlockKind, Vec<TextRun>)> {
    if let Some(target) = obsidian_embed_target(line) {
        return Ok((
            BlockKind::BlockRef {
                entity_id: target.entity_id,
                block_id: target.block_id,
                section: target.section,
                pin: None,
            },
            vec![TextRun::new(line, Vec::new())?],
        ));
    }
    let (kind, text) = markdown_block_kind(line);
    let runs = match text {
        Some(text) => vec![TextRun::new(text, Vec::new())?],
        None => Vec::new(),
    };
    Ok((kind, runs))
}

fn markdown_block_kind(line: &str) -> (BlockKind, Option<String>) {
    let marker_len = line.chars().take_while(|ch| *ch == '#').count();
    if (1..=6).contains(&marker_len)
        && line
            .as_bytes()
            .get(marker_len)
            .is_some_and(u8::is_ascii_whitespace)
    {
        (
            BlockKind::Heading {
                level: marker_len as u8,
            },
            Some(line[marker_len..].trim().to_string()),
        )
    } else if let Some(text) = markdown_unordered_list_item(line) {
        (
            BlockKind::ListItem { ordered: false },
            Some(text.to_string()),
        )
    } else if let Some(text) = markdown_ordered_list_item(line) {
        (
            BlockKind::ListItem { ordered: true },
            Some(text.to_string()),
        )
    } else if let Some(text) = line.strip_prefix("> ") {
        (BlockKind::Quote, Some(text.trim().to_string()))
    } else if matches!(line, "---" | "***" | "___") {
        (BlockKind::Divider, None)
    } else {
        (BlockKind::Paragraph, Some(line.to_string()))
    }
}

struct ObsidianEmbedTarget {
    entity_id: String,
    block_id: Option<String>,
    section: bool,
}

fn obsidian_embed_target(line: &str) -> Option<ObsidianEmbedTarget> {
    let inner = line.strip_prefix("![[")?.strip_suffix("]]")?.trim();
    if inner.is_empty() || inner.contains('|') {
        return None;
    }
    let (page, anchor) = inner.split_once('#').unwrap_or((inner, ""));
    let page = page.trim();
    if page.is_empty() {
        return None;
    }
    if obsidian_embed_points_to_file(page) {
        return None;
    }
    let anchor = anchor.trim();
    let (block_id, section) = if let Some(block_id) = anchor.strip_prefix('^') {
        let block_id = block_id.trim();
        if block_id.is_empty() {
            return None;
        }
        (Some(block_id.to_string()), false)
    } else if anchor.is_empty() {
        (None, false)
    } else {
        (Some(stable_page_id(anchor)), true)
    };
    Some(ObsidianEmbedTarget {
        entity_id: format!("page:{}", stable_page_id(page)),
        block_id,
        section,
    })
}

fn obsidian_embed_points_to_file(page: &str) -> bool {
    let file_name = page.rsplit('/').next().unwrap_or(page);
    let Some((_, ext)) = file_name.rsplit_once('.') else {
        return false;
    };
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "avif"
            | "bmp"
            | "csv"
            | "gif"
            | "jpeg"
            | "jpg"
            | "json"
            | "mov"
            | "mp3"
            | "mp4"
            | "pdf"
            | "png"
            | "svg"
            | "webm"
            | "webp"
            | "xls"
            | "xlsx"
            | "zip"
    )
}

fn markdown_unordered_list_item(line: &str) -> Option<&str> {
    line.strip_prefix("- [ ] ")
        .or_else(|| line.strip_prefix("- [x] "))
        .or_else(|| line.strip_prefix("- [X] "))
        .or_else(|| {
            ["- ", "* ", "+ "]
                .iter()
                .find_map(|prefix| line.strip_prefix(prefix))
        })
}

fn markdown_ordered_list_item(line: &str) -> Option<&str> {
    let (number, rest) = line.split_once(". ")?;
    if !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit()) {
        Some(rest)
    } else {
        None
    }
}

#[derive(Debug, serde::Deserialize)]
pub struct SlackImportSnapshot {
    #[serde(default)]
    pub source_scope: Option<String>,
    #[serde(default)]
    pub channels: Vec<SlackChannel>,
    #[serde(default)]
    pub messages: Vec<SlackMessage>,
    #[serde(default)]
    pub users: Vec<serde_json::Value>,
    #[serde(default)]
    pub usergroups: Vec<serde_json::Value>,
    #[serde(default)]
    pub files: Vec<serde_json::Value>,
    #[serde(default)]
    pub custom_emoji: Vec<serde_json::Value>,
    #[serde(default)]
    pub pins: Vec<serde_json::Value>,
}

#[derive(Debug, serde::Deserialize)]
pub struct SlackChannel {
    pub id: String,
    #[serde(default)]
    pub handle: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub name_normalized: Option<String>,
    #[serde(default)]
    pub is_channel: Option<bool>,
    #[serde(default)]
    pub is_group: Option<bool>,
    #[serde(default)]
    pub is_im: Option<bool>,
    #[serde(default)]
    pub is_mpim: Option<bool>,
    #[serde(default)]
    pub is_private: Option<bool>,
    #[serde(default)]
    pub is_archived: Option<bool>,
    #[serde(default)]
    pub is_general: Option<bool>,
    #[serde(default)]
    pub is_shared: Option<bool>,
    #[serde(default)]
    pub is_ext_shared: Option<bool>,
    #[serde(default)]
    pub created: Option<u64>,
    #[serde(default)]
    pub updated: Option<u64>,
    #[serde(default)]
    pub creator: Option<String>,
    #[serde(default)]
    pub topic: Option<serde_json::Value>,
    #[serde(default)]
    pub purpose: Option<serde_json::Value>,
    #[serde(default)]
    pub properties: Option<serde_json::Value>,
    #[serde(default)]
    pub previous_names: Vec<String>,
    #[serde(default)]
    pub shared_team_ids: Vec<String>,
    #[serde(default)]
    pub members: Vec<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct SlackMessage {
    #[serde(default)]
    pub r#type: Option<String>,
    #[serde(default)]
    pub subtype: Option<String>,
    #[serde(default)]
    pub channel_id: String,
    pub ts: String,
    #[serde(default)]
    pub thread_ts: Option<String>,
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub bot_id: Option<String>,
    #[serde(default)]
    pub app_id: Option<String>,
    #[serde(default)]
    pub team: Option<String>,
    #[serde(default)]
    pub channel_type: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub edited: Option<serde_json::Value>,
    #[serde(default)]
    pub is_starred: Option<bool>,
    #[serde(default)]
    pub pinned_to: Vec<String>,
    #[serde(default)]
    pub blocks: Vec<serde_json::Value>,
    #[serde(default)]
    pub attachments: Vec<serde_json::Value>,
    #[serde(default)]
    pub files: Vec<serde_json::Value>,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
    #[serde(default)]
    pub client_msg_id: Option<String>,
    #[serde(default)]
    pub permalink: Option<String>,
    #[serde(default)]
    pub hidden: Option<bool>,
    #[serde(default)]
    pub deleted_ts: Option<String>,
    #[serde(default)]
    pub event_ts: Option<String>,
    #[serde(default)]
    pub reactions: Vec<SlackReaction>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct SlackReaction {
    pub name: String,
    #[serde(default)]
    pub count: Option<u64>,
    #[serde(default)]
    pub users: Vec<String>,
}

#[derive(Debug, serde::Deserialize)]
pub struct DriveImportSnapshot {
    #[serde(default)]
    pub source_scope: Option<String>,
    #[serde(default)]
    pub folders: Vec<DriveFolder>,
    #[serde(default)]
    pub files: Vec<DriveFile>,
}

#[derive(Debug, serde::Deserialize)]
pub struct DriveFolder {
    pub id: String,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub parents: Vec<String>,
    pub name: String,
    #[serde(default)]
    pub source_system: Option<String>,
    #[serde(default)]
    pub mime_type: Option<String>,
    #[serde(default)]
    pub drive_id: Option<String>,
    #[serde(default)]
    pub created_time: Option<String>,
    #[serde(default)]
    pub modified_time: Option<String>,
    #[serde(default)]
    pub trashed: Option<bool>,
    #[serde(default)]
    pub web_view_link: Option<String>,
    #[serde(default)]
    pub sharepoint_ids: Option<serde_json::Value>,
    #[serde(default)]
    pub retention_label: Option<serde_json::Value>,
    #[serde(default)]
    pub permissions: Vec<serde_json::Value>,
    #[serde(default)]
    pub comments: Vec<serde_json::Value>,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, serde::Deserialize)]
pub struct DriveFile {
    pub id: String,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub parents: Vec<String>,
    pub name: String,
    #[serde(default)]
    pub source_system: Option<String>,
    #[serde(default)]
    pub mime_type: Option<String>,
    #[serde(default)]
    pub drive_id: Option<String>,
    #[serde(default)]
    pub created_time: Option<String>,
    #[serde(default)]
    pub modified_time: Option<String>,
    #[serde(default)]
    pub trashed: Option<bool>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub content_hex: Option<String>,
    #[serde(default)]
    pub content_path: Option<String>,
    #[serde(default)]
    pub web_view_link: Option<String>,
    #[serde(default)]
    pub web_content_link: Option<String>,
    #[serde(default)]
    pub download_url: Option<String>,
    #[serde(default)]
    pub size: Option<String>,
    #[serde(default)]
    pub md5_checksum: Option<String>,
    #[serde(default)]
    pub sha1_checksum: Option<String>,
    #[serde(default)]
    pub sha256_checksum: Option<String>,
    #[serde(default)]
    pub owners: Vec<serde_json::Value>,
    #[serde(default)]
    pub last_modifying_user: Option<serde_json::Value>,
    #[serde(default)]
    pub labels: Vec<serde_json::Value>,
    #[serde(default)]
    pub capabilities: Option<serde_json::Value>,
    #[serde(default)]
    pub content_restrictions: Vec<serde_json::Value>,
    #[serde(default)]
    pub link_share_metadata: Option<serde_json::Value>,
    #[serde(default)]
    pub sharepoint_ids: Option<serde_json::Value>,
    #[serde(default)]
    pub retention_label: Option<serde_json::Value>,
    #[serde(default)]
    pub list_item: Option<serde_json::Value>,
    #[serde(default)]
    pub thumbnails: Vec<serde_json::Value>,
    #[serde(default)]
    pub remote_item: Option<serde_json::Value>,
    #[serde(default)]
    pub permissions: Vec<serde_json::Value>,
    #[serde(default)]
    pub comments: Vec<serde_json::Value>,
    #[serde(default)]
    pub revisions: Vec<serde_json::Value>,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
    #[serde(default)]
    pub shortcut_target: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
pub struct NotionImportSnapshot {
    #[serde(default)]
    pub source_scope: Option<String>,
    #[serde(default)]
    pub pages: Vec<NotionPage>,
}

#[derive(Debug, serde::Deserialize)]
pub struct NotionPage {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub space_id: Option<String>,
    #[serde(default)]
    pub parent_page_id: Option<String>,
    #[serde(default)]
    pub markdown: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub blocks: Vec<String>,
    #[serde(default)]
    pub database: Option<serde_json::Value>,
    #[serde(default)]
    pub property_values: Vec<serde_json::Value>,
    #[serde(default)]
    pub formulas: Vec<serde_json::Value>,
    #[serde(default)]
    pub rollups: Vec<serde_json::Value>,
    #[serde(default)]
    pub views: Vec<serde_json::Value>,
    #[serde(default)]
    pub comments: Vec<serde_json::Value>,
    #[serde(default)]
    pub permissions: Vec<serde_json::Value>,
    #[serde(default)]
    pub attachments: Vec<serde_json::Value>,
    #[serde(default)]
    pub synced_blocks: Vec<serde_json::Value>,
    #[serde(default)]
    pub rich_text_semantics: Vec<serde_json::Value>,
    #[serde(default)]
    pub unsupported_blocks: Vec<serde_json::Value>,
    #[serde(default)]
    pub users: Vec<serde_json::Value>,
    #[serde(default)]
    pub source_metadata: Option<serde_json::Value>,
}

pub fn import_notion_bytes(
    loom: &mut Loom<FileStore>,
    ns: WorkspaceId,
    workspace_id: &str,
    source_path: &str,
    default_space_id: &str,
    bytes: &[u8],
    dry_run: bool,
) -> Result<ImportReport> {
    let parsed = parse_notion_import_snapshot(bytes)?;
    import_notion_snapshot(
        loom,
        ns,
        workspace_id,
        source_path,
        default_space_id,
        bytes.len() as u64,
        parsed,
        dry_run,
    )
}

fn parse_notion_import_snapshot(bytes: &[u8]) -> Result<NotionImportSnapshot> {
    let value: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|e| loom_types::LoomError::invalid(format!("parse Notion snapshot JSON: {e}")))?;
    if notion_import_value_is_api_bundle(&value) {
        return notion_api_bundle_to_snapshot(value);
    }
    serde_json::from_value(value)
        .map_err(|e| loom_types::LoomError::invalid(format!("parse Notion snapshot JSON: {e}")))
}

fn notion_import_value_is_api_bundle(value: &serde_json::Value) -> bool {
    value.get("block_children").is_some()
        || value
            .get("pages")
            .and_then(serde_json::Value::as_array)
            .and_then(|pages| pages.first())
            .and_then(|page| page.get("object"))
            .and_then(serde_json::Value::as_str)
            == Some("page")
}

fn notion_api_bundle_to_snapshot(value: serde_json::Value) -> Result<NotionImportSnapshot> {
    let source_scope = value
        .get("source_scope")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    let pages = value
        .get("pages")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| {
            loom_types::LoomError::invalid("Notion API bundle pages must be an array")
        })?;
    let block_children = value
        .get("block_children")
        .and_then(serde_json::Value::as_object);
    let mut normalized = Vec::new();
    for page in pages {
        normalized.push(notion_api_page_to_snapshot_page(
            page,
            block_children,
            &value,
        )?);
    }
    Ok(NotionImportSnapshot {
        source_scope,
        pages: normalized,
    })
}

fn notion_api_page_to_snapshot_page(
    page: &serde_json::Value,
    block_children: Option<&serde_json::Map<String, serde_json::Value>>,
    bundle: &serde_json::Value,
) -> Result<NotionPage> {
    let id = notion_json_str(page, "id")?.to_string();
    let title = notion_api_page_title(page).unwrap_or_else(|| id.clone());
    let parent_page_id = page
        .get("parent")
        .and_then(|parent| parent.get("page_id"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    let blocks = block_children
        .and_then(|children| children.get(&id))
        .map(notion_api_block_results)
        .transpose()?
        .unwrap_or_default();
    let (markdown, unsupported_blocks) = notion_api_blocks_to_markdown(&blocks);
    let attachments =
        notion_api_blocks_of_types(&blocks, &["audio", "file", "image", "pdf", "video"]);
    let synced_blocks = notion_api_blocks_of_types(&blocks, &["synced_block"]);
    let rich_text_semantics = notion_api_rich_text_semantics(&blocks);
    Ok(NotionPage {
        database: notion_api_page_database(page, bundle),
        property_values: notion_api_property_values_except(page, &["title", "formula", "rollup"]),
        formulas: notion_api_property_values(page, "formula"),
        rollups: notion_api_property_values(page, "rollup"),
        views: notion_api_sidecar_values(bundle, page, "views", &id),
        comments: notion_api_sidecar_values(bundle, page, "comments", &id),
        permissions: notion_api_sidecar_values(bundle, page, "permissions", &id),
        attachments,
        synced_blocks,
        id,
        title,
        space_id: None,
        parent_page_id,
        markdown: Some(markdown),
        text: None,
        blocks: Vec::new(),
        unsupported_blocks,
        rich_text_semantics,
        users: notion_api_users(bundle, page),
        source_metadata: Some(page.clone()),
    })
}

fn notion_json_str<'a>(value: &'a serde_json::Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .filter(|text| !text.trim().is_empty())
        .ok_or_else(|| loom_types::LoomError::invalid(format!("Notion API page missing {key}")))
}

fn notion_api_page_title(page: &serde_json::Value) -> Option<String> {
    let properties = page.get("properties")?.as_object()?;
    properties.values().find_map(|property| {
        if property.get("type").and_then(serde_json::Value::as_str) != Some("title") {
            return None;
        }
        let title = property.get("title")?.as_array()?;
        Some(notion_rich_text_plain(title))
    })
}

fn notion_api_block_results(value: &serde_json::Value) -> Result<Vec<serde_json::Value>> {
    if let Some(results) = value.as_array() {
        return Ok(results.clone());
    }
    value
        .get("results")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .ok_or_else(|| loom_types::LoomError::invalid("Notion block children must contain results"))
}

fn notion_api_blocks_to_markdown(blocks: &[serde_json::Value]) -> (String, Vec<serde_json::Value>) {
    let mut lines = Vec::new();
    let mut unsupported = Vec::new();
    for block in blocks {
        let Some(block_type) = block.get("type").and_then(serde_json::Value::as_str) else {
            unsupported.push(block.clone());
            continue;
        };
        let payload = block.get(block_type).unwrap_or(&serde_json::Value::Null);
        let rich_text = payload
            .get("rich_text")
            .and_then(serde_json::Value::as_array)
            .map(|items| notion_rich_text_plain(items))
            .unwrap_or_default();
        match block_type {
            "heading_1" => lines.push(format!("# {rich_text}")),
            "heading_2" => lines.push(format!("## {rich_text}")),
            "heading_3" => lines.push(format!("### {rich_text}")),
            "paragraph" => lines.push(rich_text),
            "bulleted_list_item" => lines.push(format!("- {rich_text}")),
            "numbered_list_item" => lines.push(format!("1. {rich_text}")),
            "to_do" => {
                let marker = if payload
                    .get("checked")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false)
                {
                    "[x]"
                } else {
                    "[ ]"
                };
                lines.push(format!("- {marker} {rich_text}"));
            }
            "quote" => lines.push(format!("> {rich_text}")),
            "divider" => lines.push("---".to_string()),
            _ => unsupported.push(block.clone()),
        }
    }
    (lines.join("\n"), unsupported)
}

fn notion_api_blocks_of_types(
    blocks: &[serde_json::Value],
    block_types: &[&str],
) -> Vec<serde_json::Value> {
    blocks
        .iter()
        .filter(|block| {
            block
                .get("type")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|block_type| block_types.contains(&block_type))
        })
        .cloned()
        .collect()
}

fn notion_api_property_values(
    page: &serde_json::Value,
    property_type: &str,
) -> Vec<serde_json::Value> {
    page.get("properties")
        .and_then(serde_json::Value::as_object)
        .map(|properties| {
            properties
                .values()
                .filter(|property| {
                    property.get("type").and_then(serde_json::Value::as_str) == Some(property_type)
                })
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

fn notion_api_property_values_except(
    page: &serde_json::Value,
    excluded_types: &[&str],
) -> Vec<serde_json::Value> {
    page.get("properties")
        .and_then(serde_json::Value::as_object)
        .map(|properties| {
            properties
                .values()
                .filter(|property| {
                    property
                        .get("type")
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|property_type| !excluded_types.contains(&property_type))
                })
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

fn notion_api_page_database(
    page: &serde_json::Value,
    bundle: &serde_json::Value,
) -> Option<serde_json::Value> {
    let database_id = page
        .get("parent")
        .and_then(|parent| parent.get("database_id"))
        .and_then(serde_json::Value::as_str)?;
    bundle
        .get("databases")
        .and_then(serde_json::Value::as_object)
        .and_then(|databases| databases.get(database_id))
        .cloned()
        .or_else(|| Some(serde_json::json!({ "id": database_id })))
}

fn notion_api_sidecar_values(
    bundle: &serde_json::Value,
    page: &serde_json::Value,
    key: &str,
    page_id: &str,
) -> Vec<serde_json::Value> {
    let mut values = Vec::new();
    if let Some(items) = page.get(key).and_then(serde_json::Value::as_array) {
        values.extend(items.iter().cloned());
    }
    match bundle.get(key) {
        Some(serde_json::Value::Object(by_page)) => {
            if let Some(items) = by_page.get(page_id).and_then(serde_json::Value::as_array) {
                values.extend(items.iter().cloned());
            }
        }
        Some(serde_json::Value::Array(items)) => {
            values.extend(items.iter().filter_map(|item| {
                let item_page_id = item
                    .get("page_id")
                    .or_else(|| item.get("parent").and_then(|parent| parent.get("page_id")))
                    .and_then(serde_json::Value::as_str);
                (item_page_id == Some(page_id)).then(|| item.clone())
            }));
        }
        _ => {}
    }
    values
}

fn notion_api_rich_text_semantics(blocks: &[serde_json::Value]) -> Vec<serde_json::Value> {
    let mut values = Vec::new();
    for block in blocks {
        let Some(block_type) = block.get("type").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let Some(rich_text) = block
            .get(block_type)
            .and_then(|payload| payload.get("rich_text"))
            .and_then(serde_json::Value::as_array)
        else {
            continue;
        };
        if rich_text
            .iter()
            .any(notion_rich_text_has_non_plain_semantics)
        {
            values.push(serde_json::json!({
                "block_id": block.get("id").cloned().unwrap_or(serde_json::Value::Null),
                "block_type": block_type,
                "rich_text": rich_text,
            }));
        }
    }
    values
}

fn notion_rich_text_has_non_plain_semantics(value: &serde_json::Value) -> bool {
    value
        .get("type")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|value_type| value_type != "text")
        || value
            .get("href")
            .and_then(serde_json::Value::as_str)
            .is_some()
        || value
            .get("annotations")
            .and_then(serde_json::Value::as_object)
            .is_some_and(|annotations| {
                annotations
                    .iter()
                    .any(|(key, value)| match (key.as_str(), value) {
                        ("color", serde_json::Value::String(color)) => color != "default",
                        (_, serde_json::Value::Bool(enabled)) => *enabled,
                        _ => false,
                    })
            })
}

fn notion_api_users(
    bundle: &serde_json::Value,
    page: &serde_json::Value,
) -> Vec<serde_json::Value> {
    let mut values = Vec::new();
    if let Some(user) = page.get("created_by") {
        values.push(user.clone());
    }
    if let Some(user) = page.get("last_edited_by") {
        values.push(user.clone());
    }
    if let Some(users) = bundle.get("users").and_then(serde_json::Value::as_array) {
        values.extend(users.iter().cloned());
    }
    values
}

fn notion_rich_text_plain(items: &[serde_json::Value]) -> String {
    items
        .iter()
        .filter_map(|item| item.get("plain_text").and_then(serde_json::Value::as_str))
        .collect::<Vec<_>>()
        .join("")
}

pub fn import_notion_snapshot(
    loom: &mut Loom<FileStore>,
    ns: WorkspaceId,
    workspace_id: &str,
    fallback_source_scope: &str,
    default_space_id: &str,
    bytes_in: u64,
    parsed: NotionImportSnapshot,
    dry_run: bool,
) -> Result<ImportReport> {
    let source_scope = parsed
        .source_scope
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(fallback_source_scope);
    let planned_spaces = parsed
        .pages
        .iter()
        .map(|page| notion_page_space_id(page, default_space_id))
        .collect::<BTreeSet<_>>()
        .len();
    let mut report = ImportReport::new(ImportReportInput {
        profile: "notion",
        source_scope,
        commit: None,
        objects_added: 0,
        bytes_in,
        bytes_stored: 0,
        rows_imported: 0,
        skipped: 0,
        operations_planned: (parsed.pages.len() + planned_spaces) as u64,
        operations_applied: 0,
        dry_run,
    })?;
    for page in &parsed.pages {
        add_notion_fidelity_issues(&mut report, page)?;
    }
    if dry_run {
        return Ok(report);
    }

    let mut spaces = BTreeSet::new();
    for page in &parsed.pages {
        let space = notion_page_space_id(page, default_space_id);
        if spaces.insert(space.to_string()) {
            match loom_pages::create_space(loom, ns, workspace_id, space, space, None) {
                Ok(_) => {
                    report.operations_applied += 1;
                }
                Err(error) if error.code == Code::AlreadyExists => {
                    report.skipped += 1;
                }
                Err(error) => return Err(error),
            }
        }
        let markdown = notion_page_markdown(page);
        let body = markdown_body(&markdown)?;
        let body_bytes = body.encode()?;
        apply_page_import(
            loom,
            ns,
            workspace_id,
            space,
            &stable_page_id(&page.id),
            &page.title,
            page.parent_page_id.as_deref(),
            body_bytes,
            &mut report,
        )?;
        report.bytes_stored += markdown.len() as u64;
    }
    Ok(report)
}

pub fn import_slack_bytes(
    loom: &mut Loom<FileStore>,
    ns: WorkspaceId,
    workspace_id: &str,
    source_path: &str,
    bytes: &[u8],
    dry_run: bool,
) -> Result<ImportReport> {
    let parsed = parse_slack_import_snapshot(source_path, bytes)?;
    import_slack_snapshot(
        loom,
        ns,
        workspace_id,
        source_path,
        bytes.len() as u64,
        parsed,
        dry_run,
    )
}

pub fn import_slack_snapshot(
    loom: &mut Loom<FileStore>,
    ns: WorkspaceId,
    workspace_id: &str,
    fallback_source_scope: &str,
    bytes_in: u64,
    parsed: SlackImportSnapshot,
    dry_run: bool,
) -> Result<ImportReport> {
    let source_scope = parsed
        .source_scope
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(fallback_source_scope);
    let reaction_count = parsed
        .messages
        .iter()
        .map(|message| {
            message
                .reactions
                .iter()
                .map(|reaction| reaction.users.len().max(1))
                .sum::<usize>()
        })
        .sum::<usize>();
    let thread_count = parsed
        .messages
        .iter()
        .filter_map(|message| {
            slack_thread_id(message).map(|thread| (message.channel_id.as_str(), thread))
        })
        .collect::<BTreeSet<_>>()
        .len()
        .min(parsed.messages.len());
    let mut report = ImportReport::new(ImportReportInput {
        profile: "slack",
        source_scope,
        commit: None,
        objects_added: 0,
        bytes_in,
        bytes_stored: 0,
        rows_imported: 0,
        skipped: 0,
        operations_planned: (parsed.channels.len()
            + parsed.messages.len()
            + reaction_count
            + thread_count) as u64,
        operations_applied: 0,
        dry_run,
    })?;
    for channel in &parsed.channels {
        add_slack_channel_fidelity_issues(&mut report, channel)?;
    }
    for message in &parsed.messages {
        add_slack_message_fidelity_issues(&mut report, message)?;
    }
    add_unsupported_list(
        &mut report,
        "slack:workspace",
        "users",
        &parsed.users,
        "Slack users are not lowered as principals by this importer slice",
    )?;
    add_unsupported_list(
        &mut report,
        "slack:workspace",
        "usergroups",
        &parsed.usergroups,
        "Slack user groups are not lowered by this importer slice",
    )?;
    add_unsupported_list(
        &mut report,
        "slack:workspace",
        "files",
        &parsed.files,
        "Slack file attachments are not lowered by this importer slice",
    )?;
    add_unsupported_list(
        &mut report,
        "slack:workspace",
        "custom_emoji",
        &parsed.custom_emoji,
        "custom emoji assets are not lowered by this importer slice",
    )?;
    add_unsupported_list(
        &mut report,
        "slack:workspace",
        "pins",
        &parsed.pins,
        "Slack pins are not lowered by this importer slice",
    )?;
    if dry_run {
        return Ok(report);
    }

    for channel in &parsed.channels {
        let channel_id = slack_channel_workspace_id(&channel.id);
        let selector = channel_id.to_string();
        match loom_chat::channel_projection(loom, ns, workspace_id, &selector) {
            Ok(_) => {
                report.skipped += 1;
                continue;
            }
            Err(error) if error.code == Code::NotFound => {}
            Err(error) => return Err(error),
        }
        match loom_chat::ensure_channel(
            loom,
            ns,
            workspace_id,
            channel_id,
            &slack_channel_handle(channel),
            &slack_channel_name(channel),
        ) {
            Ok(_) => report.operations_applied += 1,
            Err(error) if error.code == Code::AlreadyExists => report.skipped += 1,
            Err(error) => return Err(error),
        }
    }

    let mut messages = parsed.messages.clone();
    messages.sort_by(|left, right| {
        left.channel_id
            .cmp(&right.channel_id)
            .then_with(|| left.ts.cmp(&right.ts))
    });
    for message in &messages {
        let channel_id = slack_channel_workspace_id(&message.channel_id);
        loom_chat::ensure_channel(
            loom,
            ns,
            workspace_id,
            channel_id,
            &slack_channel_fallback_handle(&message.channel_id),
            &message.channel_id,
        )?;
        let channel_selector = channel_id.to_string();
        let message_id = slack_message_id(&message.channel_id, &message.ts);
        let projection = loom_chat::channel_projection(loom, ns, workspace_id, &channel_selector)?;
        let message_exists = projection
            .messages
            .iter()
            .any(|existing| existing.message_id == message_id);
        let mut thread_id = None;
        if let Some(thread) = slack_thread_id(message) {
            let parent_id = slack_message_id(&message.channel_id, thread);
            let thread_exists = projection
                .threads
                .iter()
                .any(|existing| existing.thread_id == thread);
            if !thread_exists && parent_id != message_id {
                match loom_chat::create_thread(
                    loom,
                    ns,
                    workspace_id,
                    &channel_selector,
                    thread,
                    &parent_id,
                ) {
                    Ok(_) => report.operations_applied += 1,
                    Err(error) if error.code == Code::AlreadyExists => report.skipped += 1,
                    Err(error) if error.code == Code::NotFound => report.skipped += 1,
                    Err(error) => return Err(error),
                }
            }
            if parent_id != message_id {
                thread_id = Some(thread);
            }
        }
        if message_exists {
            report.skipped += 1;
        } else {
            let body = slack_message_body(message);
            if body.is_empty() {
                report.skipped += 1;
                continue;
            }
            loom_chat::post_message(
                loom,
                ns,
                workspace_id,
                &channel_selector,
                &message_id,
                thread_id,
                body.as_bytes().to_vec(),
            )?;
            report.operations_applied += 1;
            report.rows_imported += 1;
            report.bytes_stored += body.len() as u64;
        }
        apply_slack_reactions(
            loom,
            ns,
            workspace_id,
            &channel_selector,
            &message_id,
            message,
            &mut report,
        )?;
    }
    Ok(report)
}

pub fn import_drive_bytes(
    loom: &mut Loom<FileStore>,
    ns: WorkspaceId,
    workspace_id: &str,
    source_path: &str,
    bytes: &[u8],
    snapshot_dir: &Path,
    dry_run: bool,
) -> Result<ImportReport> {
    let parsed: DriveImportSnapshot = serde_json::from_slice(bytes)
        .map_err(|e| loom_types::LoomError::invalid(format!("parse Drive snapshot JSON: {e}")))?;
    import_drive_snapshot(
        loom,
        ns,
        workspace_id,
        source_path,
        bytes.len() as u64,
        snapshot_dir,
        parsed,
        dry_run,
    )
}

pub fn import_drive_snapshot(
    loom: &mut Loom<FileStore>,
    ns: WorkspaceId,
    workspace_id: &str,
    fallback_source_scope: &str,
    bytes_in: u64,
    snapshot_dir: &Path,
    parsed: DriveImportSnapshot,
    dry_run: bool,
) -> Result<ImportReport> {
    let source_scope = parsed
        .source_scope
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(fallback_source_scope);
    let mut report = ImportReport::new(ImportReportInput {
        profile: "drive",
        source_scope,
        commit: None,
        objects_added: 0,
        bytes_in,
        bytes_stored: 0,
        rows_imported: 0,
        skipped: 0,
        operations_planned: (parsed.folders.len() + parsed.files.len()) as u64,
        operations_applied: 0,
        dry_run,
    })?;
    for folder in &parsed.folders {
        add_drive_folder_fidelity_issues(&mut report, folder)?;
    }
    for file in &parsed.files {
        add_drive_file_fidelity_issues(&mut report, file)?;
    }
    if dry_run {
        return Ok(report);
    }

    for folder in &parsed.folders {
        let parent = drive_parent_id(folder.parent_id.as_deref());
        match loom_drive::stat_node(loom, ns, workspace_id, parent, &folder.name) {
            Ok(stat) if stat.kind == "folder" => {
                report.skipped += 1;
                continue;
            }
            Ok(_) => {
                return Err(loom_types::LoomError::invalid(format!(
                    "drive entry {} exists and is not a folder",
                    folder.name
                )));
            }
            Err(error) if error.code == Code::NotFound => {}
            Err(error) => return Err(error),
        }
        let expected_root = current_drive_root(loom, ns, workspace_id)?;
        loom_drive::create_folder(
            loom,
            ns,
            workspace_id,
            parent,
            &folder.id,
            &folder.name,
            &expected_root,
        )?;
        report.operations_applied += 1;
        report.rows_imported += 1;
    }
    for file in &parsed.files {
        let parent = drive_parent_id(file.parent_id.as_deref());
        let content = drive_file_bytes(file, snapshot_dir)?;
        let stat = match loom_drive::stat_node(loom, ns, workspace_id, parent, &file.name) {
            Ok(stat) if stat.kind == "file" => Some(stat),
            Ok(_) => {
                return Err(loom_types::LoomError::invalid(format!(
                    "drive entry {} exists and is not a file",
                    file.name
                )));
            }
            Err(error) if error.code == Code::NotFound => None,
            Err(error) => return Err(error),
        };
        let replace_file = stat.is_some();
        let file_id = stat
            .as_ref()
            .map(|stat| stat.node_id.as_str())
            .unwrap_or(&file.id);
        if replace_file {
            let existing = loom_drive::read_file(loom, ns, workspace_id, file_id)?;
            if existing == content {
                report.skipped += 1;
                continue;
            }
        }
        let expected_root = current_drive_root(loom, ns, workspace_id)?;
        let upload_id = drive_import_upload_id(file_id, report.operations_applied + report.skipped);
        loom_drive::write_file_from_os(
            loom,
            ns,
            loom_drive::HostedDriveOsWrite {
                workspace_id,
                upload_id: &upload_id,
                parent_folder_id: parent,
                name: &file.name,
                file_id,
                expected_root: &expected_root,
                created_at_ms: now_ms(),
                replace_file,
                bytes: &content,
            },
        )?;
        report.operations_applied += 1;
        report.rows_imported += 1;
        report.bytes_stored += content.len() as u64;
    }
    Ok(report)
}

fn parse_slack_import_snapshot(path: &str, bytes: &[u8]) -> Result<SlackImportSnapshot> {
    if path.ends_with(".zip") {
        parse_slack_zip_export(path, bytes)
    } else {
        serde_json::from_slice(bytes)
            .map_err(|e| loom_types::LoomError::invalid(format!("parse Slack snapshot JSON: {e}")))
    }
}

fn parse_slack_zip_export(path: &str, bytes: &[u8]) -> Result<SlackImportSnapshot> {
    let reader = std::io::Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(reader)
        .map_err(|e| loom_types::LoomError::invalid(format!("read Slack zip: {e}")))?;
    let mut channels: Vec<SlackChannel> = Vec::new();
    let mut users: Vec<serde_json::Value> = Vec::new();
    let mut usergroups: Vec<serde_json::Value> = Vec::new();
    let mut channel_by_folder = BTreeMap::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|e| {
            loom_types::LoomError::invalid(format!("read Slack zip entry {index}: {e}"))
        })?;
        let name = entry.name().replace('\\', "/");
        if name == "channels.json" {
            let mut text = String::new();
            entry.read_to_string(&mut text).map_err(|e| {
                loom_types::LoomError::invalid(format!("read Slack channels.json: {e}"))
            })?;
            channels = serde_json::from_str(&text).map_err(|e| {
                loom_types::LoomError::invalid(format!("parse Slack channels.json: {e}"))
            })?;
            for channel in &channels {
                if let Some(name) = channel.name.as_ref().filter(|name| !name.is_empty()) {
                    channel_by_folder.insert(name.clone(), channel.id.clone());
                }
            }
        } else if name == "users.json" {
            let mut text = String::new();
            entry.read_to_string(&mut text).map_err(|e| {
                loom_types::LoomError::invalid(format!("read Slack users.json: {e}"))
            })?;
            users = serde_json::from_str(&text).map_err(|e| {
                loom_types::LoomError::invalid(format!("parse Slack users.json: {e}"))
            })?;
        } else if name == "usergroups.json" {
            let mut text = String::new();
            entry.read_to_string(&mut text).map_err(|e| {
                loom_types::LoomError::invalid(format!("read Slack usergroups.json: {e}"))
            })?;
            usergroups = serde_json::from_str(&text).map_err(|e| {
                loom_types::LoomError::invalid(format!("parse Slack usergroups.json: {e}"))
            })?;
        }
    }

    let mut messages = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|e| {
            loom_types::LoomError::invalid(format!("read Slack zip entry {index}: {e}"))
        })?;
        let name = entry.name().replace('\\', "/");
        if !name.ends_with(".json") || is_slack_zip_metadata_entry(&name) {
            continue;
        }
        let Some((folder, _file)) = name.split_once('/') else {
            continue;
        };
        let channel_id = channel_by_folder
            .get(folder)
            .cloned()
            .unwrap_or_else(|| folder.to_string());
        let mut text = String::new();
        entry.read_to_string(&mut text).map_err(|e| {
            loom_types::LoomError::invalid(format!("read Slack message file {name}: {e}"))
        })?;
        let mut file_messages: Vec<SlackMessage> = serde_json::from_str(&text).map_err(|e| {
            loom_types::LoomError::invalid(format!("parse Slack message file {name}: {e}"))
        })?;
        for message in &mut file_messages {
            if message.channel_id.is_empty() {
                message.channel_id.clone_from(&channel_id);
            }
        }
        messages.extend(file_messages);
    }
    Ok(SlackImportSnapshot {
        source_scope: Some(format!("slack-zip:{path}")),
        channels,
        messages,
        users,
        usergroups,
        files: Vec::new(),
        custom_emoji: Vec::new(),
        pins: Vec::new(),
    })
}

fn is_slack_zip_metadata_entry(name: &str) -> bool {
    matches!(
        name,
        "channels.json"
            | "users.json"
            | "usergroups.json"
            | "integration_logs.json"
            | "file_conversations.json"
            | "dms.json"
            | "groups.json"
            | "mpims.json"
            | "canvases.json"
            | "lists.json"
    )
}

fn apply_slack_reactions(
    loom: &mut Loom<FileStore>,
    ns: WorkspaceId,
    workspace_id: &str,
    channel_selector: &str,
    message_id: &str,
    message: &SlackMessage,
    report: &mut ImportReport,
) -> Result<()> {
    let projection = loom_chat::channel_projection(loom, ns, workspace_id, channel_selector)?;
    for reaction in &message.reactions {
        if reaction.name.trim().is_empty() {
            report.skipped += reaction.users.len().max(1) as u64;
            continue;
        }
        let exists = projection.messages.iter().any(|existing| {
            existing.message_id == message_id
                && existing
                    .reactions
                    .iter()
                    .any(|existing| existing.kind == reaction.name)
        });
        if exists {
            report.skipped += reaction.users.len().max(1) as u64;
            continue;
        }
        loom_chat::register_emoji(loom, ns, workspace_id, &reaction.name)?;
        match loom_chat::add_reaction(
            loom,
            ns,
            workspace_id,
            channel_selector,
            message_id,
            &reaction.name,
        ) {
            Ok(_) => report.operations_applied += 1,
            Err(error) if error.code == Code::AlreadyExists => report.skipped += 1,
            Err(error) if error.code == Code::NotFound => report.skipped += 1,
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn notion_page_space_id<'a>(page: &'a NotionPage, default_space_id: &'a str) -> &'a str {
    page.space_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(default_space_id)
}

fn notion_page_markdown(page: &NotionPage) -> String {
    page.markdown
        .as_ref()
        .or(page.text.as_ref())
        .cloned()
        .unwrap_or_else(|| page.blocks.join("\n"))
}

fn add_notion_fidelity_issues(report: &mut ImportReport, page: &NotionPage) -> Result<()> {
    add_unsupported_option(
        report,
        &format!("page:{}", page.id),
        "source_metadata",
        page.source_metadata.as_ref(),
        "Notion source metadata is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &format!("page:{}", page.id),
        "database",
        page.database.as_ref(),
        "Notion databases are not lowered by this importer slice",
    )?;
    add_unsupported_list(
        report,
        &format!("page:{}", page.id),
        "property_values",
        &page.property_values,
        "Notion property values are not lowered by this importer slice",
    )?;
    add_unsupported_list(
        report,
        &format!("page:{}", page.id),
        "formulas",
        &page.formulas,
        "Notion formulas are not lowered by this importer slice",
    )?;
    add_unsupported_list(
        report,
        &format!("page:{}", page.id),
        "rollups",
        &page.rollups,
        "Notion rollups are not lowered by this importer slice",
    )?;
    add_unsupported_list(
        report,
        &format!("page:{}", page.id),
        "views",
        &page.views,
        "Notion views are not lowered by this importer slice",
    )?;
    add_unsupported_list(
        report,
        &format!("page:{}", page.id),
        "comments",
        &page.comments,
        "Notion comments are not lowered by this importer slice",
    )?;
    add_unsupported_list(
        report,
        &format!("page:{}", page.id),
        "permissions",
        &page.permissions,
        "Notion permissions are not lowered by this importer slice",
    )?;
    add_unsupported_list(
        report,
        &format!("page:{}", page.id),
        "attachments",
        &page.attachments,
        "Notion attachments are not lowered by this importer slice",
    )?;
    add_unsupported_list(
        report,
        &format!("page:{}", page.id),
        "synced_blocks",
        &page.synced_blocks,
        "Notion synced blocks are not lowered by this importer slice",
    )
    .and_then(|_| {
        add_unsupported_list(
            report,
            &format!("page:{}", page.id),
            "rich_text_semantics",
            &page.rich_text_semantics,
            "Notion rich-text annotations, links, mentions, and equations are not lowered by this importer slice",
        )
    })
    .and_then(|_| {
        add_unsupported_list(
            report,
            &format!("page:{}", page.id),
            "users",
            &page.users,
            "Notion users are not mapped to principals by this importer slice",
        )
    })
    .and_then(|_| {
        add_unsupported_list(
            report,
            &format!("page:{}", page.id),
            "unsupported_blocks",
            &page.unsupported_blocks,
            "Notion block types outside the current page-body subset are not lowered by this importer slice",
        )
    })
}

fn add_slack_channel_fidelity_issues(
    report: &mut ImportReport,
    channel: &SlackChannel,
) -> Result<()> {
    add_unsupported_option(
        report,
        &format!("channel:{}", channel.id),
        "name_normalized",
        channel.name_normalized.as_ref(),
        "Slack channel normalized name is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &format!("channel:{}", channel.id),
        "is_channel",
        channel.is_channel.as_ref(),
        "Slack channel type flags are not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &format!("channel:{}", channel.id),
        "is_group",
        channel.is_group.as_ref(),
        "Slack channel type flags are not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &format!("channel:{}", channel.id),
        "is_im",
        channel.is_im.as_ref(),
        "Slack channel type flags are not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &format!("channel:{}", channel.id),
        "is_mpim",
        channel.is_mpim.as_ref(),
        "Slack channel type flags are not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &format!("channel:{}", channel.id),
        "is_private",
        channel.is_private.as_ref(),
        "Slack channel privacy flag is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &format!("channel:{}", channel.id),
        "is_archived",
        channel.is_archived.as_ref(),
        "Slack channel archive state is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &format!("channel:{}", channel.id),
        "is_general",
        channel.is_general.as_ref(),
        "Slack general-channel flag is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &format!("channel:{}", channel.id),
        "is_shared",
        channel.is_shared.as_ref(),
        "Slack shared-channel state is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &format!("channel:{}", channel.id),
        "is_ext_shared",
        channel.is_ext_shared.as_ref(),
        "Slack external shared-channel state is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &format!("channel:{}", channel.id),
        "created",
        channel.created.as_ref(),
        "Slack channel creation time is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &format!("channel:{}", channel.id),
        "updated",
        channel.updated.as_ref(),
        "Slack channel update time is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &format!("channel:{}", channel.id),
        "creator",
        channel.creator.as_ref(),
        "Slack channel creator is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &format!("channel:{}", channel.id),
        "topic",
        channel.topic.as_ref(),
        "Slack channel topic is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &format!("channel:{}", channel.id),
        "purpose",
        channel.purpose.as_ref(),
        "Slack channel purpose is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &format!("channel:{}", channel.id),
        "properties",
        channel.properties.as_ref(),
        "Slack channel properties are not lowered by this importer slice",
    )?;
    if !channel.previous_names.is_empty() {
        add_unsupported(
            report,
            &format!("channel:{}", channel.id),
            "previous_names",
            "Slack channel previous names are not lowered by this importer slice",
        )?;
    }
    if !channel.shared_team_ids.is_empty() {
        add_unsupported(
            report,
            &format!("channel:{}", channel.id),
            "shared_team_ids",
            "Slack shared team ids are not lowered by this importer slice",
        )?;
    }
    if channel.members.is_empty() {
        return Ok(());
    }
    add_unsupported(
        report,
        &format!("channel:{}", channel.id),
        "members",
        "Slack channel membership is not lowered by this importer slice",
    )
}

fn add_slack_message_fidelity_issues(
    report: &mut ImportReport,
    message: &SlackMessage,
) -> Result<()> {
    let source = format!(
        "message:{}",
        slack_message_id(&message.channel_id, &message.ts)
    );
    add_unsupported_option(
        report,
        &source,
        "type",
        message.r#type.as_ref(),
        "Slack message type is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &source,
        "subtype",
        message.subtype.as_ref(),
        "Slack message subtype is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &source,
        "user",
        message.user.as_ref(),
        "Slack message user is not lowered as a principal by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &source,
        "username",
        message.username.as_ref(),
        "Slack message username is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &source,
        "bot_id",
        message.bot_id.as_ref(),
        "Slack bot id is not lowered as an agent principal by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &source,
        "app_id",
        message.app_id.as_ref(),
        "Slack app id is not lowered as an agent principal by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &source,
        "team",
        message.team.as_ref(),
        "Slack message team is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &source,
        "channel_type",
        message.channel_type.as_ref(),
        "Slack message channel type is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &source,
        "edited",
        message.edited.as_ref(),
        "Slack edited marker is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &source,
        "is_starred",
        message.is_starred.as_ref(),
        "Slack starred state is not lowered by this importer slice",
    )?;
    if !message.pinned_to.is_empty() {
        add_unsupported(
            report,
            &source,
            "pinned_to",
            "Slack message pins are not lowered by this importer slice",
        )?;
    }
    add_unsupported_list(
        report,
        &source,
        "blocks",
        &message.blocks,
        "Slack Block Kit blocks are not lowered by this importer slice",
    )?;
    add_unsupported_list(
        report,
        &source,
        "attachments",
        &message.attachments,
        "Slack message attachments are not lowered by this importer slice",
    )?;
    add_unsupported_list(
        report,
        &source,
        "files",
        &message.files,
        "Slack message files are not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &source,
        "metadata",
        message.metadata.as_ref(),
        "Slack message metadata is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &source,
        "client_msg_id",
        message.client_msg_id.as_ref(),
        "Slack client message id is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &source,
        "permalink",
        message.permalink.as_ref(),
        "Slack message permalink is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &source,
        "hidden",
        message.hidden.as_ref(),
        "Slack hidden message state is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &source,
        "deleted_ts",
        message.deleted_ts.as_ref(),
        "Slack deleted message timestamp is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &source,
        "event_ts",
        message.event_ts.as_ref(),
        "Slack event timestamp is not lowered by this importer slice",
    )?;
    for reaction in &message.reactions {
        if !reaction.users.is_empty() {
            add_unsupported(
                report,
                &source,
                "reaction_users",
                "Slack per-user reaction authorship is not lowered by this importer slice",
            )?;
        }
        if reaction.count.is_some() {
            add_unsupported(
                report,
                &source,
                "reaction_count",
                "Slack reaction counts are not lowered by this importer slice",
            )?;
        }
    }
    Ok(())
}

fn add_drive_folder_fidelity_issues(report: &mut ImportReport, folder: &DriveFolder) -> Result<()> {
    if !folder.parents.is_empty() {
        add_unsupported(
            report,
            &format!("folder:{}", folder.id),
            "parents",
            "Drive folder multi-parent metadata is not lowered by this importer slice",
        )?;
    }
    add_unsupported_option(
        report,
        &format!("folder:{}", folder.id),
        "source_system",
        folder.source_system.as_ref(),
        "Drive folder source-system metadata is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &format!("folder:{}", folder.id),
        "mime_type",
        folder.mime_type.as_ref(),
        "Drive folder MIME metadata is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &format!("folder:{}", folder.id),
        "drive_id",
        folder.drive_id.as_ref(),
        "Drive folder drive id is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &format!("folder:{}", folder.id),
        "created_time",
        folder.created_time.as_ref(),
        "Drive folder created time is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &format!("folder:{}", folder.id),
        "modified_time",
        folder.modified_time.as_ref(),
        "Drive folder modified time is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &format!("folder:{}", folder.id),
        "trashed",
        folder.trashed.as_ref(),
        "Drive folder trash state is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &format!("folder:{}", folder.id),
        "web_view_link",
        folder.web_view_link.as_ref(),
        "Drive folder web link is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &format!("folder:{}", folder.id),
        "sharepoint_ids",
        folder.sharepoint_ids.as_ref(),
        "SharePoint folder ids are not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &format!("folder:{}", folder.id),
        "retention_label",
        folder.retention_label.as_ref(),
        "SharePoint retention labels are not lowered by this importer slice",
    )?;
    add_unsupported_list(
        report,
        &format!("folder:{}", folder.id),
        "permissions",
        &folder.permissions,
        "Drive folder permissions are not lowered by this importer slice",
    )?;
    add_unsupported_list(
        report,
        &format!("folder:{}", folder.id),
        "comments",
        &folder.comments,
        "Drive folder comments are not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &format!("folder:{}", folder.id),
        "metadata",
        folder.metadata.as_ref(),
        "Drive folder metadata is not lowered by this importer slice",
    )
}

fn add_drive_file_fidelity_issues(report: &mut ImportReport, file: &DriveFile) -> Result<()> {
    if !file.parents.is_empty() {
        add_unsupported(
            report,
            &format!("file:{}", file.id),
            "parents",
            "Drive file multi-parent metadata is not lowered by this importer slice",
        )?;
    }
    add_unsupported_option(
        report,
        &format!("file:{}", file.id),
        "source_system",
        file.source_system.as_ref(),
        "Drive file source-system metadata is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &format!("file:{}", file.id),
        "mime_type",
        file.mime_type.as_ref(),
        "Drive file MIME metadata is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &format!("file:{}", file.id),
        "drive_id",
        file.drive_id.as_ref(),
        "Drive file drive id is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &format!("file:{}", file.id),
        "created_time",
        file.created_time.as_ref(),
        "Drive file created time is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &format!("file:{}", file.id),
        "modified_time",
        file.modified_time.as_ref(),
        "Drive file modified time is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &format!("file:{}", file.id),
        "trashed",
        file.trashed.as_ref(),
        "Drive file trash state is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &format!("file:{}", file.id),
        "web_view_link",
        file.web_view_link.as_ref(),
        "Drive file web view link is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &format!("file:{}", file.id),
        "web_content_link",
        file.web_content_link.as_ref(),
        "Drive file web content link is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &format!("file:{}", file.id),
        "download_url",
        file.download_url.as_ref(),
        "Drive file download URL is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &format!("file:{}", file.id),
        "size",
        file.size.as_ref(),
        "Drive file declared size is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &format!("file:{}", file.id),
        "md5_checksum",
        file.md5_checksum.as_ref(),
        "Drive file MD5 checksum is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &format!("file:{}", file.id),
        "sha1_checksum",
        file.sha1_checksum.as_ref(),
        "Drive file SHA1 checksum is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &format!("file:{}", file.id),
        "sha256_checksum",
        file.sha256_checksum.as_ref(),
        "Drive file SHA256 checksum is not lowered by this importer slice",
    )?;
    add_unsupported_list(
        report,
        &format!("file:{}", file.id),
        "owners",
        &file.owners,
        "Drive file owners are not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &format!("file:{}", file.id),
        "last_modifying_user",
        file.last_modifying_user.as_ref(),
        "Drive file last modifying user is not lowered by this importer slice",
    )?;
    add_unsupported_list(
        report,
        &format!("file:{}", file.id),
        "labels",
        &file.labels,
        "Drive file labels are not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &format!("file:{}", file.id),
        "capabilities",
        file.capabilities.as_ref(),
        "Drive file capabilities are not lowered by this importer slice",
    )?;
    add_unsupported_list(
        report,
        &format!("file:{}", file.id),
        "content_restrictions",
        &file.content_restrictions,
        "Drive file content restrictions are not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &format!("file:{}", file.id),
        "link_share_metadata",
        file.link_share_metadata.as_ref(),
        "Drive file link-share metadata is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &format!("file:{}", file.id),
        "sharepoint_ids",
        file.sharepoint_ids.as_ref(),
        "SharePoint file ids are not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &format!("file:{}", file.id),
        "retention_label",
        file.retention_label.as_ref(),
        "SharePoint file retention label is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &format!("file:{}", file.id),
        "list_item",
        file.list_item.as_ref(),
        "SharePoint list item metadata is not lowered by this importer slice",
    )?;
    add_unsupported_list(
        report,
        &format!("file:{}", file.id),
        "thumbnails",
        &file.thumbnails,
        "Drive file thumbnails are not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &format!("file:{}", file.id),
        "remote_item",
        file.remote_item.as_ref(),
        "Drive remote item metadata is not lowered by this importer slice",
    )?;
    add_unsupported_list(
        report,
        &format!("file:{}", file.id),
        "permissions",
        &file.permissions,
        "Drive file permissions are not lowered by this importer slice",
    )?;
    add_unsupported_list(
        report,
        &format!("file:{}", file.id),
        "comments",
        &file.comments,
        "Drive file comments are not lowered by this importer slice",
    )?;
    add_unsupported_list(
        report,
        &format!("file:{}", file.id),
        "revisions",
        &file.revisions,
        "Drive historical revisions are not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &format!("file:{}", file.id),
        "metadata",
        file.metadata.as_ref(),
        "Drive file metadata is not lowered by this importer slice",
    )?;
    add_unsupported_option(
        report,
        &format!("file:{}", file.id),
        "shortcut_target",
        file.shortcut_target.as_ref(),
        "Drive shortcuts are not lowered by this importer slice",
    )
}

fn add_unsupported_list(
    report: &mut ImportReport,
    source_entity_id: &str,
    field: &str,
    values: &[serde_json::Value],
    reason: &str,
) -> Result<()> {
    if values.is_empty() {
        return Ok(());
    }
    add_unsupported(report, source_entity_id, field, reason)
}

fn add_unsupported_option<T>(
    report: &mut ImportReport,
    source_entity_id: &str,
    field: &str,
    value: Option<&T>,
    reason: &str,
) -> Result<()> {
    if value.is_none() {
        return Ok(());
    }
    add_unsupported(report, source_entity_id, field, reason)
}

fn add_unsupported(
    report: &mut ImportReport,
    source_entity_id: &str,
    field: &str,
    reason: &str,
) -> Result<()> {
    report.fidelity_issues.push(FidelityIssue::new(
        FidelitySeverity::Warning,
        source_entity_id,
        field,
        reason,
    )?);
    Ok(())
}

fn current_drive_root(
    loom: &Loom<FileStore>,
    ns: WorkspaceId,
    workspace_id: &str,
) -> Result<String> {
    loom_drive::list_folder(loom, ns, workspace_id, "root").map(|folder| folder.profile_root)
}

fn drive_parent_id(parent_id: Option<&str>) -> &str {
    parent_id
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("root")
}

fn drive_file_bytes(file: &DriveFile, snapshot_dir: &Path) -> Result<Vec<u8>> {
    if let Some(text) = file.text.as_ref() {
        return Ok(text.as_bytes().to_vec());
    }
    if let Some(hex) = file.content_hex.as_ref() {
        return decode_import_hex(hex);
    }
    let path = file.content_path.as_deref().ok_or_else(|| {
        loom_types::LoomError::invalid(format!(
            "drive file {} needs text, content_hex, or content_path",
            file.id
        ))
    })?;
    let path = Path::new(path);
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        snapshot_dir.join(path)
    };
    std::fs::read(path).map_err(|e| loom_types::LoomError::new(Code::Io, e.to_string()))
}

fn decode_import_hex(value: &str) -> Result<Vec<u8>> {
    let value = value.trim();
    if !value.len().is_multiple_of(2) {
        return Err(loom_types::LoomError::invalid(
            "hex payload must have an even number of characters",
        ));
    }
    let mut out = Vec::with_capacity(value.len() / 2);
    for chunk in value.as_bytes().chunks_exact(2) {
        let high = import_hex_value(chunk[0])?;
        let low = import_hex_value(chunk[1])?;
        out.push((high << 4) | low);
    }
    Ok(out)
}

fn import_hex_value(value: u8) -> Result<u8> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        b'A'..=b'F' => Ok(value - b'A' + 10),
        _ => Err(loom_types::LoomError::invalid(
            "hex payload contains a non-hex character",
        )),
    }
}

fn drive_import_upload_id(file_id: &str, sequence: u64) -> String {
    let digest = Digest::hash(
        Algo::Sha256,
        format!("drive-import:{file_id}:{sequence}:{}", now_ms()).as_bytes(),
    );
    let hex = digest.to_hex();
    format!("import-{}", &hex[..24])
}

fn slack_channel_workspace_id(source_id: &str) -> WorkspaceId {
    if let Ok(id) = WorkspaceId::parse(source_id) {
        return id;
    }
    let digest = Digest::hash(
        Algo::Sha256,
        format!("slack:channel:{source_id}").as_bytes(),
    );
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest.bytes()[..16]);
    WorkspaceId::v4_from_bytes(bytes)
}

fn slack_channel_handle(channel: &SlackChannel) -> String {
    channel
        .handle
        .as_deref()
        .or(channel.name.as_deref())
        .map(slack_channel_fallback_handle)
        .unwrap_or_else(|| slack_channel_fallback_handle(&channel.id))
}

fn slack_channel_fallback_handle(value: &str) -> String {
    let mut out = String::new();
    for ch in value.trim_start_matches('#').chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches('-');
    if out.is_empty() {
        "channel".to_string()
    } else {
        out.chars().take(64).collect()
    }
}

fn slack_channel_name(channel: &SlackChannel) -> String {
    channel
        .name
        .clone()
        .or_else(|| channel.handle.clone())
        .unwrap_or_else(|| channel.id.clone())
}

fn slack_message_id(channel_id: &str, ts: &str) -> String {
    let digest = Digest::hash(
        Algo::Sha256,
        format!("slack:message:{channel_id}:{ts}").as_bytes(),
    );
    let hex = digest.to_hex();
    format!("slack-{}", &hex[..24])
}

fn slack_thread_id(message: &SlackMessage) -> Option<&str> {
    message
        .thread_ts
        .as_deref()
        .filter(|thread| !thread.trim().is_empty() && *thread != message.ts)
}

fn slack_message_body(message: &SlackMessage) -> String {
    message
        .body
        .as_ref()
        .or(message.text.as_ref())
        .map(|value| value.trim().to_string())
        .unwrap_or_default()
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redmine_xml_import_lowers_tickets_pages_and_source_extras() {
        let temp = std::env::temp_dir().join(format!("loom-redmine-xml-{}", now_ms()));
        let store_path = temp.join("redmine.loom");
        std::fs::create_dir_all(&temp).unwrap();
        let store = FileStore::create_with_profile(&store_path, Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        let workspace = WorkspaceId::from_bytes([88; 16]);
        loom.registry_mut()
            .create(loom_core::FacetKind::Vcs, Some("main"), workspace)
            .unwrap();
        let xml =
            include_bytes!("../../../specs/studio/fixtures/redmine/source/redmine-api-bundle.xml");
        let expected: serde_json::Value = serde_json::from_slice(include_bytes!(
            "../../../specs/studio/fixtures/redmine/expected/comparison.json"
        ))
        .unwrap();

        let report = import_redmine_bytes_with_field_policy(
            &mut loom,
            workspace,
            "studio",
            "redmine.xml",
            xml,
            false,
            TicketImportFieldPolicy::Infer,
        )
        .unwrap();

        assert_eq!(report.source_scope, expected["source_scope"]);
        assert_eq!(report.rows_imported, 4);
        let unsupported_fields = report
            .fidelity_issues
            .iter()
            .map(|issue| issue.field.as_str())
            .collect::<BTreeSet<_>>();
        let expected_unsupported_fields = expected["unsupported_fields"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect::<BTreeSet<_>>();
        assert_eq!(unsupported_fields, expected_unsupported_fields);
        let reader = loom_tickets::TicketProfileReader::open(&loom, workspace, "studio")
            .unwrap()
            .unwrap();
        let project = reader.project("core").unwrap().unwrap();
        assert_eq!(project.key_prefix, expected["project"]["key_prefix"]);
        assert_eq!(project.name, expected["project"]["name"]);
        let identity = loom_tickets::ExternalTicketIdentity::new("redmine", "issue:42").unwrap();
        let ticket = reader
            .ticket_by_external_identity(&identity)
            .unwrap()
            .unwrap();
        assert_eq!(ticket.project_id, expected["issue"]["project_id"]);
        assert_eq!(
            ticket.fields.get("subject").unwrap().to_json(),
            expected["issue"]["subject"]
        );
        assert_eq!(
            ticket.fields.get("description").unwrap().to_json(),
            expected["issue"]["description"]
        );
        assert_eq!(
            ticket.fields.get("status").unwrap().to_json(),
            expected["issue"]["status"]
        );
        assert_eq!(
            ticket.fields.get("priority").unwrap().to_json(),
            expected["issue"]["priority"]
        );
        assert_eq!(
            ticket.fields.get("category").unwrap().to_json(),
            expected["issue"]["category"]
        );
        assert_eq!(
            ticket.fields.get("assigned_to").unwrap().to_json(),
            expected["issue"]["assigned_to"]
        );
        assert_eq!(
            ticket.fields.get("author").unwrap().to_json(),
            expected["issue"]["author"]
        );
        assert_eq!(
            ticket.fields.get("tracker").unwrap().to_json(),
            expected["issue"]["tracker"]
        );
        assert_eq!(
            ticket.fields.get("created_at").unwrap().to_json(),
            expected["issue"]["created_at"]
        );
        assert_eq!(
            ticket.fields.get("updated_at").unwrap().to_json(),
            expected["issue"]["updated_at"]
        );
        assert_eq!(
            ticket.fields.get("start_date").unwrap().to_json(),
            expected["issue"]["start_date"]
        );
        assert_eq!(
            ticket.fields.get("due_date").unwrap().to_json(),
            expected["issue"]["due_date"]
        );
        assert_eq!(
            ticket.fields.get("closed_on").unwrap().to_json(),
            expected["issue"]["closed_on"]
        );
        assert_eq!(
            ticket.fields.get("done_ratio").unwrap().to_json(),
            expected["issue"]["done_ratio"]
        );
        assert_eq!(
            ticket.fields.get("estimated_hours").unwrap().to_json(),
            expected["issue"]["estimated_hours"]
        );
        assert_eq!(
            ticket.fields.get("fixed_version").unwrap().to_json(),
            expected["issue"]["fixed_version"]
        );
        assert_eq!(
            ticket.fields.get("affected_version").unwrap().to_json(),
            expected["issue"]["affected_version"]
        );
        assert_eq!(
            ticket.fields.get("parent_issue_id").unwrap().to_json(),
            expected["issue"]["parent_issue_id"]
        );
        assert_eq!(
            ticket.fields.get("is_private").unwrap().to_json(),
            expected["issue"]["is_private"]
        );
        assert_eq!(
            ticket.fields.get("url").unwrap().to_json(),
            expected["issue"]["url"]
        );
        assert_eq!(
            redmine_field_values(&ticket, "redmine_journals")[0]["id"],
            "7"
        );
        assert_eq!(
            redmine_field_values(&ticket, "redmine_journals")[0]["notes"],
            expected["retained_fields"]["redmine_journals"]
        );
        assert_eq!(
            redmine_field_values(&ticket, "redmine_journals")[0]["user"]["name"],
            expected["retained_fields"]["redmine_journal_user"]
        );
        assert_eq!(
            redmine_field_values(&ticket, "redmine_journals")[0]["details"]["detail"]["name"],
            expected["retained_fields"]["redmine_journal_detail"]
        );
        assert_eq!(
            redmine_field_values(&ticket, "redmine_comments")[0]["text"],
            expected["retained_fields"]["redmine_comments"]
        );
        assert_eq!(
            redmine_field_values(&ticket, "redmine_comments")[0]["author"]["name"],
            expected["retained_fields"]["redmine_comment_author"]
        );
        let watcher_names = redmine_field_values(&ticket, "redmine_watchers")
            .into_iter()
            .map(|watcher| watcher["name"].clone())
            .collect::<Vec<_>>();
        assert_eq!(
            serde_json::Value::Array(watcher_names),
            expected["retained_fields"]["redmine_watchers"]
        );
        let affected_versions = redmine_field_values(&ticket, "redmine_affected_versions")
            .into_iter()
            .map(|version| version["name"].clone())
            .collect::<Vec<_>>();
        assert_eq!(
            serde_json::Value::Array(affected_versions),
            expected["retained_fields"]["redmine_affected_versions"]
        );
        assert_eq!(
            redmine_field_values(&ticket, "redmine_attachments")[0]["filename"],
            expected["retained_fields"]["redmine_attachments"]
        );
        assert_eq!(
            redmine_field_values(&ticket, "redmine_attachments")[0]["content_type"],
            expected["retained_fields"]["redmine_attachment_content_type"]
        );
        assert_eq!(
            redmine_field_values(&ticket, "redmine_attachments")[0]["content_url"],
            expected["retained_fields"]["redmine_attachment_url"]
        );
        assert_eq!(
            redmine_field_values(&ticket, "redmine_attachments")[0]["description"],
            expected["retained_fields"]["redmine_attachment_description"]
        );
        assert_eq!(
            redmine_field_values(&ticket, "redmine_attachments")[0]["author"]["name"],
            expected["retained_fields"]["redmine_attachment_author"]
        );
        assert_eq!(
            redmine_field_values(&ticket, "redmine_time_entries")[0]["comments"],
            expected["retained_fields"]["redmine_time_entries"]
        );
        assert_eq!(
            redmine_field_values(&ticket, "redmine_time_entries")[0]["activity"],
            expected["retained_fields"]["redmine_time_entry_activity"]
        );
        assert_eq!(
            redmine_field_values(&ticket, "redmine_time_entries")[0]["hours"],
            expected["retained_fields"]["redmine_time_entry_hours"]
        );
        assert_eq!(
            redmine_field_values(&ticket, "redmine_time_entries")[0]["created_on"],
            expected["retained_fields"]["redmine_time_entry_created_on"]
        );
        assert_eq!(
            redmine_field_values(&ticket, "redmine_relations")[0]["relation_type"],
            expected["retained_fields"]["redmine_relations"][0]
        );
        let relation_types = redmine_field_values(&ticket, "redmine_relations")
            .into_iter()
            .map(|relation| relation["relation_type"].clone())
            .collect::<Vec<_>>();
        assert_eq!(
            serde_json::Value::Array(relation_types),
            expected["retained_fields"]["redmine_relations"]
        );
        assert_eq!(
            redmine_field_values(&ticket, "redmine_children")[0]["subject"],
            expected["retained_fields"]["redmine_children"]
        );
        assert_eq!(
            redmine_field_values(&ticket, "redmine_changesets")[0]["revision"],
            expected["retained_fields"]["redmine_changesets"]
        );
        let allowed_statuses = redmine_field_values(&ticket, "redmine_allowed_statuses")
            .into_iter()
            .map(|status| status["name"].clone())
            .collect::<Vec<_>>();
        assert_eq!(
            serde_json::Value::Array(allowed_statuses),
            expected["retained_fields"]["redmine_allowed_statuses"]
        );
        let page = loom_pages::get_page(&loom, workspace, "studio", "home")
            .unwrap()
            .unwrap();
        assert_eq!(page.title, expected["wiki_page"]["title"]);
        assert_eq!(
            page.parent_page_id.as_deref(),
            expected["wiki_page"]["parent_page_id"].as_str()
        );
        let body = Body::decode(page.body.as_deref().unwrap()).unwrap();
        let text = body_text(&body);
        assert!(text.contains(expected["wiki_page"]["body_contains"].as_str().unwrap()));
        let source_xml = std::str::from_utf8(xml).unwrap();
        assert!(source_xml.contains("<version>22</version>"));
        assert!(
            expected["wiki_page"]["metadata_gap"]
                .as_str()
                .unwrap()
                .contains("not currently stored")
        );
        let catalog = loom_tickets::ticket_field_catalog_for_project(
            &loom,
            workspace,
            "studio",
            "core",
            None,
            Some("write"),
        )
        .unwrap();
        assert!(catalog.strict_unknown_fields);
        let redmine_issue_id = catalog
            .fields
            .iter()
            .find(|field| field.native_field == "redmine_issue_id")
            .unwrap();
        assert_eq!(redmine_issue_id.field_type, "string");
        let redmine_journals = catalog
            .fields
            .iter()
            .find(|field| field.native_field == "redmine_journals")
            .unwrap();
        assert_eq!(redmine_journals.field_type, "opaque_json");
        assert_eq!(redmine_journals.cardinality, "list");

        std::fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn ticket_import_strict_rejects_unknown_fields_with_infer_guidance() {
        let temp = std::env::temp_dir().join(format!("loom-redmine-strict-{}", now_ms()));
        let store_path = temp.join("redmine.loom");
        std::fs::create_dir_all(&temp).unwrap();
        let store = FileStore::create_with_profile(&store_path, Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        let workspace = WorkspaceId::from_bytes([87; 16]);
        loom.registry_mut()
            .create(loom_core::FacetKind::Vcs, Some("main"), workspace)
            .unwrap();
        let xml =
            include_bytes!("../../../specs/studio/fixtures/redmine/source/redmine-api-bundle.xml");

        let error = import_redmine_bytes(&mut loom, workspace, "studio", "redmine.xml", xml, false)
            .unwrap_err();

        assert_eq!(error.code, Code::InvalidArgument);
        assert!(error.message.contains("undeclared fields"));
        assert!(error.message.contains("field policy infer"));
        assert!(error.message.contains("redmine_issue_id"));

        std::fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn markdown_fixture_import_lowers_pages_and_classifies_obsidian_gaps() {
        let temp = std::env::temp_dir().join(format!("loom-markdown-fixture-{}", now_ms()));
        let store_path = temp.join("markdown.loom");
        std::fs::create_dir_all(&temp).unwrap();
        let store = FileStore::create_with_profile(&store_path, Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        let workspace = WorkspaceId::from_bytes([87; 16]);
        loom.registry_mut()
            .create(loom_core::FacetKind::Vcs, Some("main"), workspace)
            .unwrap();
        let fixture_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../specs/studio/fixtures/markdown/source/vault");
        let expected: serde_json::Value = serde_json::from_slice(include_bytes!(
            "../../../specs/studio/fixtures/markdown/expected/comparison.json"
        ))
        .unwrap();

        let report = import_markdown_path(
            &mut loom,
            workspace,
            "pages",
            expected["source_scope"].as_str().unwrap(),
            &fixture_root,
            expected["space"]["space_id"].as_str().unwrap(),
            false,
        )
        .unwrap();

        assert_eq!(report.source_scope, expected["source_scope"]);
        assert_eq!(report.rows_imported, 4);
        let issue_fields = report
            .fidelity_issues
            .iter()
            .map(|issue| issue.field.as_str())
            .collect::<BTreeSet<_>>();
        let expected_unsupported = expected["unsupported_fields"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect::<BTreeSet<_>>();
        assert_eq!(issue_fields, expected_unsupported);
        assert!(!issue_fields.contains("whole-page-obsidian-embed"));

        let spaces = loom_pages::list_spaces(&loom, workspace, "pages").unwrap();
        assert_eq!(spaces.len(), 1);
        assert_eq!(
            spaces[0].space_id,
            expected["space"]["space_id"].as_str().unwrap()
        );
        assert_eq!(
            spaces[0].title,
            expected["space"]["title"].as_str().unwrap()
        );

        for page_id in ["intro", "guides", "guides-setup"] {
            let page_expected = &expected["pages"][page_id];
            let page = loom_pages::get_page(&loom, workspace, "pages", page_id)
                .unwrap()
                .unwrap();
            assert_eq!(page.title, page_expected["title"].as_str().unwrap());
            let body = Body::decode(page.body.as_deref().unwrap()).unwrap();
            let text = body_text(&body);
            for expected_text in page_expected["body_contains"].as_array().unwrap() {
                assert!(text.contains(expected_text.as_str().unwrap()));
            }
        }

        let embed = loom_pages::get_page(&loom, workspace, "pages", "embed")
            .unwrap()
            .unwrap();
        let embed_body = Body::decode(embed.body.as_deref().unwrap()).unwrap();
        let expected_refs = expected["pages"]["embed"]["block_refs"].as_array().unwrap();
        assert_eq!(
            expected["block_ref_fidelity"]["mode"].as_str().unwrap(),
            "native-block-ref"
        );
        assert_eq!(
            expected_refs.len() as u64,
            expected["block_ref_fidelity"]["count"].as_u64().unwrap()
        );
        assert_eq!(embed_body.blocks.len(), expected_refs.len());
        for (block, expected_ref) in embed_body.blocks.iter().zip(expected_refs) {
            match &block.kind {
                BlockKind::BlockRef {
                    entity_id,
                    block_id,
                    section,
                    pin,
                } => {
                    assert_eq!(entity_id, expected_ref["entity"].as_str().unwrap());
                    assert_eq!(block_id.as_deref(), expected_ref["block"].as_str());
                    assert_eq!(*section, expected_ref["section"].as_bool().unwrap());
                    assert!(pin.is_none());
                }
                other => panic!("expected block ref, got {other:?}"),
            }
        }

        std::fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn notion_api_fixture_import_lowers_pages_and_classifies_gaps() {
        let temp = std::env::temp_dir().join(format!("loom-notion-api-{}", now_ms()));
        let store_path = temp.join("notion.loom");
        std::fs::create_dir_all(&temp).unwrap();
        let store = FileStore::create_with_profile(&store_path, Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        let workspace = WorkspaceId::from_bytes([86; 16]);
        loom.registry_mut()
            .create(loom_core::FacetKind::Vcs, Some("main"), workspace)
            .unwrap();
        let source =
            include_bytes!("../../../specs/studio/fixtures/notion/source/notion-api-bundle.json");
        let expected: serde_json::Value = serde_json::from_slice(include_bytes!(
            "../../../specs/studio/fixtures/notion/expected/comparison.json"
        ))
        .unwrap();

        let report = import_notion_bytes(
            &mut loom,
            workspace,
            "pages",
            "notion-api-bundle.json",
            expected["space"]["space_id"].as_str().unwrap(),
            source,
            false,
        )
        .unwrap();

        assert_eq!(report.source_scope, expected["source_scope"]);
        assert_eq!(
            report.rows_imported,
            expected["pages"].as_object().unwrap().len() as u64
        );
        let issue_fields = report
            .fidelity_issues
            .iter()
            .map(|issue| issue.field.as_str())
            .collect::<BTreeSet<_>>();
        let expected_unsupported = expected["unsupported_fields"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect::<BTreeSet<_>>();
        assert_eq!(issue_fields, expected_unsupported);
        let block_ref_expected = &expected["block_ref_fidelity"];
        let synced_issue = report
            .fidelity_issues
            .iter()
            .find(|issue| {
                issue.source_entity_id == block_ref_expected["source_entity_id"].as_str().unwrap()
                    && issue.field == block_ref_expected["field"].as_str().unwrap()
            })
            .unwrap();
        assert_eq!(
            block_ref_expected["mode"].as_str().unwrap(),
            "fidelity-issue"
        );
        assert!(
            synced_issue
                .reason
                .contains(block_ref_expected["reason_contains"].as_str().unwrap())
        );

        let spaces = loom_pages::list_spaces(&loom, workspace, "pages").unwrap();
        assert_eq!(spaces.len(), 1);
        assert_eq!(
            spaces[0].space_id,
            expected["space"]["space_id"].as_str().unwrap()
        );
        assert_eq!(
            spaces[0].title,
            expected["space"]["title"].as_str().unwrap()
        );

        for (page_id, page_expected) in expected["pages"].as_object().unwrap() {
            let page = loom_pages::get_page(&loom, workspace, "pages", page_id)
                .unwrap()
                .unwrap();
            assert_eq!(page.title, page_expected["title"].as_str().unwrap());
            if let Some(parent) = page_expected["parent_page_id"].as_str() {
                assert_eq!(page.parent_page_id.as_deref(), Some(parent));
            }
            let body = Body::decode(page.body.as_deref().unwrap()).unwrap();
            let text = body_text(&body);
            for expected_text in page_expected["body_contains"].as_array().unwrap() {
                assert!(text.contains(expected_text.as_str().unwrap()));
            }
        }

        std::fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn asana_fixture_import_lowers_tasks_and_classifies_gaps() {
        let temp = std::env::temp_dir().join(format!("loom-asana-fixture-{}", now_ms()));
        let store_path = temp.join("asana.loom");
        std::fs::create_dir_all(&temp).unwrap();
        let store = FileStore::create_with_profile(&store_path, Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        let workspace = WorkspaceId::from_bytes([85; 16]);
        loom.registry_mut()
            .create(loom_core::FacetKind::Vcs, Some("main"), workspace)
            .unwrap();
        let source = include_bytes!(
            "../../../specs/studio/fixtures/asana/source/asana-normalized-snapshot.json"
        );
        let expected: serde_json::Value = serde_json::from_slice(include_bytes!(
            "../../../specs/studio/fixtures/asana/expected/comparison.json"
        ))
        .unwrap();

        let report = import_asana_bytes_with_field_policy(
            &mut loom,
            workspace,
            "tickets",
            "asana-normalized-snapshot.json",
            source,
            false,
            TicketImportFieldPolicy::Infer,
        )
        .unwrap();

        assert_eq!(report.source_scope, expected["source_scope"]);
        assert_eq!(
            report.rows_imported,
            expected["rows_imported"].as_u64().unwrap()
        );
        let issue_fields = report
            .fidelity_issues
            .iter()
            .map(|issue| issue.field.as_str())
            .collect::<BTreeSet<_>>();
        let expected_unsupported = expected["unsupported_fields"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect::<BTreeSet<_>>();
        assert_eq!(issue_fields, expected_unsupported);

        let reader = loom_tickets::TicketProfileReader::open(&loom, workspace, "tickets")
            .unwrap()
            .unwrap();
        let project = reader
            .project(expected["project"]["project_id"].as_str().unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(project.key_prefix, expected["project"]["key_prefix"]);
        assert_eq!(project.name, expected["project"]["name"]);
        let board = reader
            .board(expected["board"]["board_id"].as_str().unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(board.board_key, expected["board"]["board_key"]);
        assert_eq!(board.name, expected["board"]["name"]);
        assert_eq!(board.mode.as_str(), expected["board"]["mode"]);
        let board_columns = board
            .columns
            .iter()
            .map(|column| (column.column_id.as_str(), column.name.as_str()))
            .collect::<BTreeMap<_, _>>();
        for column in expected["board"]["columns"].as_array().unwrap() {
            assert_eq!(
                board_columns.get(column["column_id"].as_str().unwrap()),
                Some(&column["name"].as_str().unwrap())
            );
        }

        let identity = loom_tickets::ExternalTicketIdentity::new(
            "asana",
            expected["task"]["external_identity"].as_str().unwrap(),
        )
        .unwrap();
        let ticket = reader
            .ticket_by_external_identity(&identity)
            .unwrap()
            .unwrap();
        assert_eq!(
            ticket.project_id,
            expected["task"]["project_id"].as_str().unwrap()
        );
        for field in [
            "subject",
            "description",
            "html_notes",
            "resource_subtype",
            "approval_status",
            "assignee_status",
            "completed",
            "created_at",
            "created_by",
            "modified_at",
            "assigned_by",
            "assignee",
            "assignee_section",
            "workspace",
            "external",
            "due_on",
            "due_at",
            "start_on",
            "start_at",
            "actual_time_minutes",
            "liked",
            "num_likes",
            "num_subtasks",
            "is_rendered_as_separator",
            "tags",
            "custom_fields",
            "dependencies",
            "dependents",
            "memberships",
            "followers",
            "likes",
            "asana_task_gid",
        ] {
            assert_eq!(
                ticket.fields.get(field).unwrap().to_json(),
                expected["task"][field]
            );
        }
        assert_eq!(
            ticket
                .policy_labels
                .iter()
                .cloned()
                .collect::<BTreeSet<_>>(),
            expected["task"]["tags"]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| value.as_str().unwrap().to_string())
                .collect::<BTreeSet<_>>()
        );

        let approval_identity = loom_tickets::ExternalTicketIdentity::new(
            "asana",
            expected["approval_task"]["external_identity"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        let approval = reader
            .ticket_by_external_identity(&approval_identity)
            .unwrap()
            .unwrap();
        assert_eq!(
            approval.fields.get("subject").unwrap().to_json(),
            expected["approval_task"]["subject"]
        );
        assert_eq!(
            approval.fields.get("resource_subtype").unwrap().to_json(),
            expected["approval_task"]["resource_subtype"]
        );
        assert_eq!(
            approval.fields.get("completed").unwrap().to_json(),
            expected["approval_task"]["completed"]
        );
        assert_eq!(
            approval
                .policy_labels
                .iter()
                .cloned()
                .collect::<BTreeSet<_>>(),
            expected["approval_task"]["policy_labels"]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| value.as_str().unwrap().to_string())
                .collect::<BTreeSet<_>>()
        );
        let cards = reader.board_cards(&board.board_id).unwrap();
        let card_columns = cards
            .iter()
            .map(|card| (card.ticket_id.as_str(), card.column_id.as_str()))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(
            card_columns.get(ticket.ticket_id.as_str()),
            Some(&"section-my-today")
        );
        assert_eq!(
            card_columns.get(approval.ticket_id.as_str()),
            Some(&"unsectioned")
        );

        std::fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn jira_fixture_import_lowers_issues_and_classifies_gaps() {
        let temp = std::env::temp_dir().join(format!("loom-jira-fixture-{}", now_ms()));
        let store_path = temp.join("jira.loom");
        std::fs::create_dir_all(&temp).unwrap();
        let store = FileStore::create_with_profile(&store_path, Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        let workspace = WorkspaceId::from_bytes([84; 16]);
        loom.registry_mut()
            .create(loom_core::FacetKind::Vcs, Some("main"), workspace)
            .unwrap();
        let source = include_bytes!(
            "../../../specs/studio/fixtures/jira/source/jira-normalized-snapshot.json"
        );
        let expected: serde_json::Value = serde_json::from_slice(include_bytes!(
            "../../../specs/studio/fixtures/jira/expected/comparison.json"
        ))
        .unwrap();

        let report = import_jira_bytes_with_field_policy(
            &mut loom,
            workspace,
            "tickets",
            "jira-normalized-snapshot.json",
            source,
            false,
            TicketImportFieldPolicy::Infer,
        )
        .unwrap();

        assert_eq!(report.source_scope, expected["source_scope"]);
        assert_eq!(
            report.rows_imported,
            expected["rows_imported"].as_u64().unwrap()
        );
        let issue_fields = report
            .fidelity_issues
            .iter()
            .map(|issue| issue.field.as_str())
            .collect::<BTreeSet<_>>();
        let expected_unsupported = expected["unsupported_fields"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect::<BTreeSet<_>>();
        assert_eq!(issue_fields, expected_unsupported);

        let reader = loom_tickets::TicketProfileReader::open(&loom, workspace, "tickets")
            .unwrap()
            .unwrap();
        let project = reader
            .project(expected["project"]["project_id"].as_str().unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(project.key_prefix, expected["project"]["key_prefix"]);
        assert_eq!(project.name, expected["project"]["name"]);
        let board = reader
            .board(expected["board"]["board_id"].as_str().unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(board.board_key, expected["board"]["board_key"]);
        assert_eq!(board.name, expected["board"]["name"]);
        assert_eq!(board.mode.as_str(), expected["board"]["mode"]);
        let board_columns = board
            .columns
            .iter()
            .map(|column| (column.column_id.as_str(), column))
            .collect::<BTreeMap<_, _>>();
        for column in expected["board"]["columns"].as_array().unwrap() {
            let board_column = board_columns
                .get(column["column_id"].as_str().unwrap())
                .unwrap();
            assert_eq!(board_column.name, column["name"]);
            assert_eq!(
                board_column
                    .mapped_statuses
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>(),
                column["mapped_statuses"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|value| value.as_str().unwrap().to_string())
                    .collect::<Vec<_>>()
            );
        }

        let identity = loom_tickets::ExternalTicketIdentity::new(
            "jira",
            expected["issue"]["external_identity"].as_str().unwrap(),
        )
        .unwrap();
        let issue = reader
            .ticket_by_external_identity(&identity)
            .unwrap()
            .unwrap();
        assert_eq!(
            issue.project_id,
            expected["issue"]["project_id"].as_str().unwrap()
        );
        for field in [
            "subject",
            "jira_issue_key",
            "description",
            "issue_type",
            "status",
            "status_category",
            "priority",
            "assignee",
            "reporter",
            "creator",
            "created_at",
            "updated_at",
            "due_date",
            "environment",
            "parent",
            "security",
            "votes",
            "watches",
            "sprint",
            "labels",
            "components",
            "fix_versions",
            "affected_versions",
            "issue_links",
            "subtasks",
            "transitions",
            "properties",
            "development",
            "custom_fields",
            "jira_issue_id",
        ] {
            assert_eq!(
                issue.fields.get(field).unwrap().to_json(),
                expected["issue"][field]
            );
        }
        assert_eq!(
            issue.policy_labels.iter().cloned().collect::<BTreeSet<_>>(),
            expected["issue"]["labels"]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| value.as_str().unwrap().to_string())
                .collect::<BTreeSet<_>>()
        );

        let bug_identity = loom_tickets::ExternalTicketIdentity::new(
            "jira",
            expected["bug"]["external_identity"].as_str().unwrap(),
        )
        .unwrap();
        let bug = reader
            .ticket_by_external_identity(&bug_identity)
            .unwrap()
            .unwrap();
        assert_eq!(
            bug.fields.get("subject").unwrap().to_json(),
            expected["bug"]["subject"]
        );
        assert_eq!(
            bug.fields.get("issue_type").unwrap().to_json(),
            expected["bug"]["issue_type"]
        );
        assert_eq!(
            bug.policy_labels.iter().cloned().collect::<BTreeSet<_>>(),
            expected["bug"]["policy_labels"]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| value.as_str().unwrap().to_string())
                .collect::<BTreeSet<_>>()
        );
        let cards = reader.board_cards(&board.board_id).unwrap();
        let card_columns = cards
            .iter()
            .map(|card| (card.ticket_id.as_str(), card.column_id.as_str()))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(
            card_columns.get(issue.ticket_id.as_str()),
            Some(&"in-progress")
        );
        assert_eq!(card_columns.get(bug.ticket_id.as_str()), Some(&"to-do"));

        std::fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn confluence_fixture_import_lowers_pages_and_classifies_gaps() {
        let temp = std::env::temp_dir().join(format!("loom-confluence-fixture-{}", now_ms()));
        let store_path = temp.join("confluence.loom");
        std::fs::create_dir_all(&temp).unwrap();
        let store = FileStore::create_with_profile(&store_path, Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        let workspace = WorkspaceId::from_bytes([83; 16]);
        loom.registry_mut()
            .create(loom_core::FacetKind::Vcs, Some("main"), workspace)
            .unwrap();
        let source = include_bytes!(
            "../../../specs/studio/fixtures/confluence/source/confluence-normalized-snapshot.json"
        );
        let expected: serde_json::Value = serde_json::from_slice(include_bytes!(
            "../../../specs/studio/fixtures/confluence/expected/comparison.json"
        ))
        .unwrap();

        let report = import_confluence_bytes(
            &mut loom,
            workspace,
            "pages",
            "confluence-normalized-snapshot.json",
            "default",
            source,
            false,
        )
        .unwrap();

        assert_eq!(report.source_scope, expected["source_scope"]);
        assert_eq!(
            report.rows_imported,
            expected["rows_imported"].as_u64().unwrap()
        );
        let issue_fields = report
            .fidelity_issues
            .iter()
            .map(|issue| issue.field.as_str())
            .collect::<BTreeSet<_>>();
        let expected_unsupported = expected["unsupported_fields"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect::<BTreeSet<_>>();
        assert_eq!(issue_fields, expected_unsupported);

        let spaces = loom_pages::list_spaces(&loom, workspace, "pages").unwrap();
        let actual_spaces = spaces
            .into_iter()
            .map(|space| {
                serde_json::json!({
                    "space_id": space.space_id,
                    "title": space.title,
                })
            })
            .collect::<Vec<_>>();
        assert_eq!(serde_json::Value::Array(actual_spaces), expected["spaces"]);

        for (page_id, page_expected) in expected["pages"].as_object().unwrap() {
            let page = loom_pages::get_page(&loom, workspace, "pages", page_id)
                .unwrap()
                .unwrap();
            assert_eq!(page.title, page_expected["title"].as_str().unwrap());
            if let Some(parent) = page_expected["parent_page_id"].as_str() {
                assert_eq!(page.parent_page_id.as_deref(), Some(parent));
            }
            let body = Body::decode(page.body.as_deref().unwrap()).unwrap();
            if let Some(kind) = page_expected["opaque_kind"].as_str() {
                match &body.blocks[0].kind {
                    BlockKind::Opaque {
                        kind: actual,
                        payload,
                    } => {
                        assert_eq!(actual, kind);
                        let payload = std::str::from_utf8(payload).unwrap();
                        assert!(
                            payload.contains(page_expected["opaque_contains"].as_str().unwrap())
                        );
                    }
                    other => panic!("expected opaque block, got {other:?}"),
                }
            } else {
                let text = body_text(&body);
                for expected_text in page_expected["body_contains"].as_array().unwrap() {
                    assert!(text.contains(expected_text.as_str().unwrap()));
                }
            }
        }
        assert_eq!(
            expected["block_ref_fidelity"]["mode"].as_str().unwrap(),
            "opaque-body-retention"
        );
        for page_id in expected["block_ref_fidelity"]["opaque_pages"]
            .as_array()
            .unwrap()
        {
            let page_id = page_id.as_str().unwrap();
            let page = loom_pages::get_page(&loom, workspace, "pages", page_id)
                .unwrap()
                .unwrap();
            let body = Body::decode(page.body.as_deref().unwrap()).unwrap();
            assert!(matches!(&body.blocks[0].kind, BlockKind::Opaque { .. }));
        }

        std::fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn slack_fixture_import_lowers_chat_and_classifies_gaps() {
        let temp = std::env::temp_dir().join(format!("loom-slack-fixture-{}", now_ms()));
        let store_path = temp.join("slack.loom");
        std::fs::create_dir_all(&temp).unwrap();
        let store = FileStore::create_with_profile(&store_path, Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        let workspace = WorkspaceId::from_bytes([82; 16]);
        loom.registry_mut()
            .create(loom_core::FacetKind::Vcs, Some("main"), workspace)
            .unwrap();
        let source = include_bytes!(
            "../../../specs/studio/fixtures/slack/source/slack-normalized-snapshot.json"
        );
        let expected: serde_json::Value = serde_json::from_slice(include_bytes!(
            "../../../specs/studio/fixtures/slack/expected/comparison.json"
        ))
        .unwrap();

        let report = import_slack_bytes(
            &mut loom,
            workspace,
            "chat",
            "slack-normalized-snapshot.json",
            source,
            false,
        )
        .unwrap();

        assert_eq!(report.source_scope, expected["source_scope"]);
        assert_eq!(
            report.rows_imported,
            expected["rows_imported"].as_u64().unwrap()
        );
        let issue_fields = report
            .fidelity_issues
            .iter()
            .map(|issue| issue.field.as_str())
            .collect::<BTreeSet<_>>();
        let expected_unsupported = expected["unsupported_fields"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect::<BTreeSet<_>>();
        assert_eq!(issue_fields, expected_unsupported);

        let channel = loom_chat::resolve_channel_id(
            &loom,
            workspace,
            "chat",
            expected["channel"]["handle"].as_str().unwrap(),
        )
        .unwrap();
        let projection = loom_chat::channel_projection(&loom, workspace, "chat", &channel).unwrap();
        assert_eq!(
            projection.messages.len() as u64,
            expected["channel"]["message_count"].as_u64().unwrap()
        );
        assert_eq!(
            projection.threads.len() as u64,
            expected["channel"]["thread_count"].as_u64().unwrap()
        );
        let message_bodies = projection
            .messages
            .iter()
            .map(|message| String::from_utf8(message.body.clone()).unwrap())
            .collect::<BTreeSet<_>>();
        assert!(message_bodies.contains(expected["channel"]["root_body"].as_str().unwrap()));
        assert!(message_bodies.contains(expected["channel"]["thread_body"].as_str().unwrap()));
        assert!(projection.messages.iter().any(|message| {
            message
                .reactions
                .iter()
                .any(|reaction| reaction.kind == expected["channel"]["reaction_kind"])
        }));

        std::fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn drive_fixture_import_lowers_files_and_classifies_gaps() {
        let temp = std::env::temp_dir().join(format!("loom-drive-fixture-{}", now_ms()));
        let store_path = temp.join("drive.loom");
        std::fs::create_dir_all(&temp).unwrap();
        let store = FileStore::create_with_profile(&store_path, Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        let workspace = WorkspaceId::from_bytes([83; 16]);
        loom.registry_mut()
            .create(loom_core::FacetKind::Vcs, Some("main"), workspace)
            .unwrap();
        let source = include_bytes!(
            "../../../specs/studio/fixtures/drive/source/drive-sharepoint-snapshot.json"
        );
        let expected: serde_json::Value = serde_json::from_slice(include_bytes!(
            "../../../specs/studio/fixtures/drive/expected/comparison.json"
        ))
        .unwrap();
        let snapshot_dir =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../specs/studio/fixtures/drive/source");

        let report = import_drive_bytes(
            &mut loom,
            workspace,
            "drive",
            "drive-sharepoint-snapshot.json",
            source,
            &snapshot_dir,
            false,
        )
        .unwrap();

        assert_eq!(report.source_scope, expected["source_scope"]);
        assert_eq!(
            report.rows_imported,
            expected["rows_imported"].as_u64().unwrap()
        );
        let issue_fields = report
            .fidelity_issues
            .iter()
            .map(|issue| issue.field.as_str())
            .collect::<BTreeSet<_>>();
        let expected_unsupported = expected["unsupported_fields"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect::<BTreeSet<_>>();
        assert_eq!(issue_fields, expected_unsupported);

        let root = loom_drive::list_folder(&loom, workspace, "drive", "root").unwrap();
        assert_eq!(
            root.entries.len() as u64,
            expected["folders"]["root_count"].as_u64().unwrap()
        );
        let engineering = loom_drive::list_folder(&loom, workspace, "drive", "folder-eng").unwrap();
        assert_eq!(
            engineering.entries.len() as u64,
            expected["folders"]["engineering_count"].as_u64().unwrap()
        );
        let sharepoint = loom_drive::list_folder(&loom, workspace, "drive", "folder-sp").unwrap();
        assert_eq!(
            sharepoint.entries.len() as u64,
            expected["folders"]["sharepoint_count"].as_u64().unwrap()
        );
        assert_eq!(
            loom_drive::read_file(&loom, workspace, "drive", "file-readme").unwrap(),
            expected["files"]["readme"].as_str().unwrap().as_bytes()
        );
        assert_eq!(
            loom_drive::read_file(&loom, workspace, "drive", "file-binary").unwrap(),
            vec![0, 1, 2, 255]
        );
        assert_eq!(
            loom_drive::read_file(&loom, workspace, "drive", "file-sidecar").unwrap(),
            expected["files"]["sidecar"].as_str().unwrap().as_bytes()
        );
        assert_eq!(
            loom_drive::read_file(&loom, workspace, "drive", "file-shortcut").unwrap(),
            expected["files"]["shortcut"].as_str().unwrap().as_bytes()
        );

        std::fs::remove_dir_all(temp).unwrap();
    }

    fn body_text(body: &Body) -> String {
        body.blocks
            .iter()
            .flat_map(|block| block.runs.iter())
            .map(|run| run.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn redmine_field_values(
        ticket: &loom_tickets::Ticket,
        field_id: &str,
    ) -> Vec<serde_json::Value> {
        ticket
            .fields
            .get(field_id)
            .unwrap()
            .to_json()
            .as_array()
            .unwrap()
            .iter()
            .map(|value| match value {
                serde_json::Value::String(text) => serde_json::from_str(text).unwrap(),
                value => value.clone(),
            })
            .collect()
    }

    #[test]
    fn markdown_folder_notes_use_folder_identity() {
        let temp = std::env::temp_dir().join(format!("loom-markdown-folder-{}", now_ms()));
        let vault = temp.join("vault");
        let store_path = temp.join("markdown.loom");
        std::fs::create_dir_all(vault.join("Guides")).unwrap();
        std::fs::write(
            vault.join("Guides").join("Guides.md"),
            "# Guides\nFolder note",
        )
        .unwrap();
        std::fs::write(vault.join("Guides").join("Setup.md"), "# Setup\nRun init").unwrap();
        let store = FileStore::create_with_profile(&store_path, Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        let workspace = WorkspaceId::from_bytes([89; 16]);
        loom.registry_mut()
            .create(loom_core::FacetKind::Vcs, Some("main"), workspace)
            .unwrap();

        let report = import_markdown_path(
            &mut loom, workspace, "pages", "vault", &vault, "docs", false,
        )
        .unwrap();

        assert_eq!(report.rows_imported, 2);
        let folder = loom_pages::get_page(&loom, workspace, "pages", "guides")
            .unwrap()
            .unwrap();
        assert_eq!(folder.title, "Guides");
        let setup = loom_pages::get_page(&loom, workspace, "pages", "guides-setup")
            .unwrap()
            .unwrap();
        assert_eq!(setup.title, "Setup");

        std::fs::remove_dir_all(temp).unwrap();
    }
}
