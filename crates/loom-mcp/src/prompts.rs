//! The curated MCP prompt surface.
//!
//! Prompts are reusable, area-scoped workflow templates (`prompts/list`, `prompts/get`) that orchestrate
//! the tools of an area. The prompt catalog is a deliberate curation, not a mechanical projection. The
//! rmcp `#[prompt]` registration lives in
//! [`crate::server`] (feature `server`).
//!
//! Licensed under BUSL-1.1.

/// One curated prompt.
#[derive(Clone, Copy, Debug)]
pub struct PromptSpec {
    /// The wire name, `<area>.<verb>`.
    pub name: &'static str,
    /// The area (facet/subsystem) the prompt belongs to.
    pub area: &'static str,
    /// A one-line description of the workflow the prompt drives.
    pub summary: &'static str,
}

const fn p(name: &'static str, area: &'static str, summary: &'static str) -> PromptSpec {
    PromptSpec {
        name,
        area,
        summary,
    }
}

/// The curated prompt surface.
pub const PROMPT_SURFACE: &[PromptSpec] = &[
    p(
        "calendar_summarize_period",
        "calendar",
        "Summarize events and todos in a date range.",
    ),
    p(
        "calendar_find_conflicts",
        "calendar",
        "Detect overlapping or double-booked events.",
    ),
    p(
        "calendar_schedule_event",
        "calendar",
        "Propose and create an event around existing commitments.",
    ),
    p("calendar_agenda", "calendar", "Build a next-N-days agenda."),
    p(
        "contacts_find",
        "contacts",
        "Natural-language contact lookup.",
    ),
    p(
        "contacts_deduplicate",
        "contacts",
        "Find and merge duplicate cards.",
    ),
    p(
        "contacts_enrich",
        "contacts",
        "Fill missing fields on a card from context.",
    ),
    p(
        "mail_triage",
        "mail",
        "Classify and prioritize unread mail and propose flags.",
    ),
    p("mail_summarize_thread", "mail", "Summarize a conversation."),
    p("mail_draft_reply", "mail", "Draft a reply to a message."),
    p(
        "mail_find",
        "mail",
        "Natural-language search across mailboxes.",
    ),
    p(
        "vcs_summarize_changes",
        "vcs",
        "Summarize the diff between two refs.",
    ),
    p(
        "vcs_explain_conflict",
        "vcs",
        "Explain a merge conflict and propose a resolution.",
    ),
    p("vcs_blame", "vcs", "Attribute changes to a file or table."),
    p(
        "vcs_release_notes",
        "vcs",
        "Generate notes from the log between two tags.",
    ),
    p(
        "fs_summarize_tree",
        "fs",
        "Summarize the documents under a directory.",
    ),
    p("fs_find", "fs", "Locate files by name or content."),
    p(
        "sql_ask",
        "sql",
        "Answer a natural-language question over a database.",
    ),
    p("sql_schema_overview", "sql", "Describe tables and columns."),
    p(
        "timeseries_trend",
        "timeseries",
        "Summarize a metric's trend over a window.",
    ),
    p(
        "ledger_audit",
        "ledger",
        "Verify the chain and summarize entries.",
    ),
    p(
        "queue_inspect",
        "queue",
        "Summarize backlog and consumer lag.",
    ),
    p(
        "document_summarize_collection",
        "document",
        "Summarize a document collection.",
    ),
    p(
        "lifecycle_feature_ideate",
        "lifecycle",
        "Frame a feature idea and identify subject scope.",
    ),
    p(
        "lifecycle_feature_draft",
        "lifecycle",
        "Draft the feature plan and refine page content.",
    ),
    p(
        "lifecycle_feature_structure",
        "lifecycle",
        "Decompose feature scope into structure and tickets.",
    ),
    p(
        "lifecycle_feature_ready",
        "lifecycle",
        "Check readiness and prepare a frozen scope snapshot.",
    ),
    p(
        "lifecycle_feature_build",
        "lifecycle",
        "Drive build work from tickets and published pages.",
    ),
    p(
        "lifecycle_feature_done",
        "lifecycle",
        "Close a completed feature against its frozen scope.",
    ),
    p(
        "lifecycle_bug_triage",
        "lifecycle",
        "Triage a bug and identify impacted tickets or pages.",
    ),
    p(
        "lifecycle_bug_reproduce",
        "lifecycle",
        "Capture reproduction evidence for a bug lifecycle.",
    ),
    p(
        "lifecycle_bug_fix",
        "lifecycle",
        "Coordinate bug fix work across tickets and notes.",
    ),
    p(
        "lifecycle_bug_verify",
        "lifecycle",
        "Verify bug resolution before closing scope.",
    ),
    p(
        "lifecycle_bug_done",
        "lifecycle",
        "Close a bug lifecycle with final evidence.",
    ),
    p(
        "lifecycle_incident_triage",
        "lifecycle",
        "Triage an incident and open the response scope.",
    ),
    p(
        "lifecycle_incident_mitigate",
        "lifecycle",
        "Coordinate mitigation work and team updates.",
    ),
    p(
        "lifecycle_incident_resolve",
        "lifecycle",
        "Resolve an incident and collect closure evidence.",
    ),
    p(
        "lifecycle_incident_review",
        "lifecycle",
        "Prepare incident review material and action items.",
    ),
    p(
        "lifecycle_design_ideate",
        "lifecycle",
        "Frame a design topic and initial alternatives.",
    ),
    p(
        "lifecycle_design_draft",
        "lifecycle",
        "Draft a design proposal for review.",
    ),
    p(
        "lifecycle_design_review",
        "lifecycle",
        "Review design tradeoffs and unresolved questions.",
    ),
    p(
        "lifecycle_design_accepted",
        "lifecycle",
        "Finalize an accepted design decision.",
    ),
    p(
        "lifecycle_archive",
        "lifecycle",
        "Summarize an archived lifecycle instance.",
    ),
    p("apps_author", "apps", "Create or update a Loom MCP App."),
    p(
        "apps_inspect",
        "apps",
        "Inspect app candidates and MCP resource visibility.",
    ),
    p(
        "store_inventory",
        "store",
        "Overview of the loom: workspaces, facets, and capabilities.",
    ),
];

/// The whole curated prompt surface.
pub fn prompt_surface() -> &'static [PromptSpec] {
    PROMPT_SURFACE
}

/// Look up a prompt by name.
pub fn prompt(name: &str) -> Option<&'static PromptSpec> {
    PROMPT_SURFACE.iter().find(|p| p.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    const SPEC: &str = include_str!("../../../specs/0008-wire-protocols.md");

    fn backtick_tokens(cell: &str) -> Vec<String> {
        cell.split('`')
            .skip(1)
            .step_by(2)
            .map(|s| s.to_string())
            .collect()
    }

    /// Parse the documented "Area | Prompt | Purpose" table into the set of prompt names.
    fn spec_prompt_names() -> BTreeSet<String> {
        let start = SPEC
            .find("| Area | Prompt | Purpose and tools orchestrated |")
            .expect("documented prompt table exists");
        let mut names = BTreeSet::new();
        let mut seen = false;
        for line in SPEC[start..].lines() {
            let line = line.trim();
            if !line.starts_with('|') {
                if seen {
                    break;
                }
                continue;
            }
            let cols: Vec<&str> = line
                .trim_matches('|')
                .split('|')
                .map(|c| c.trim())
                .collect();
            if cols.len() != 3 || cols[0] == "Area" || cols[0].starts_with("---") {
                continue;
            }
            seen = true;
            // The prompt name is the single backtick token in column 2.
            if let Some(name) = backtick_tokens(cols[1]).into_iter().next() {
                names.insert(name);
            }
        }
        names
    }

    #[test]
    fn surface_has_unique_names_and_valid_areas() {
        let mut seen = BTreeSet::new();
        for spec in PROMPT_SURFACE {
            assert!(seen.insert(spec.name), "duplicate prompt {}", spec.name);
            let (area, _) = spec.name.split_once('_').expect("prompt name is area_verb");
            assert_eq!(area, spec.area, "prompt {} area mismatch", spec.name);
        }
    }

    /// Drift: the catalog and documented table list the same prompt names.
    #[test]
    fn surface_matches_documented_prompt_table() {
        let from_source: BTreeSet<String> =
            PROMPT_SURFACE.iter().map(|p| p.name.to_string()).collect();
        let from_spec = spec_prompt_names();
        assert_eq!(
            from_source, from_spec,
            "PROMPT_SURFACE and documented prompt table have drifted; update both together"
        );
    }
}
