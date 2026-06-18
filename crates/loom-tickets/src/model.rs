use std::collections::{BTreeMap, BTreeSet};

use loom_codec::Value;
use loom_types::{Code, Digest, LoomError, Result};

use loom_substrate::changes::{OperationChangeBatch, OperationChangeCursor, OperationChangeRecord};
use loom_substrate::facilities::{FieldDefinition, FieldType, FieldValue};
use loom_substrate::{Fields, OperationEnvelope, codec_error, validate_text};

pub const APP_ID: &str = "tickets";
pub const PROJECT_SCHEMA: &str = "loom.studio.tickets.project.v1";
pub const TICKET_SCHEMA: &str = "loom.studio.tickets.ticket.v1";
pub const PROFILE_SNAPSHOT_SCHEMA: &str = "loom.studio.tickets.profile-snapshot.v2";
pub const PROFILE_OPERATION_LOG_SCHEMA: &str = "loom.studio.tickets.operation-log.v1";
pub const PROFILE_STATE_SCHEMA: &str = "loom.studio.tickets.profile-state.v2";
pub const PROFILE_CONTROL_PREFIX: &str = "profile/tickets/v2";
pub const TICKET_COMPACT_TEXT_MAX_BYTES: usize = 4_096;
pub const TICKET_DEFAULT_FIELD_TEXT_MAX_BYTES: usize = 16_384;
pub const TICKET_RICH_TEXT_MAX_BYTES: usize = 1_048_576;
pub const TICKET_DEFAULT_BODY_CONTENT_TYPE: &str = "text/markdown";
pub const TICKET_DEFAULT_COMMENT_TYPE: &str = "general";
pub const TICKET_DEFAULT_OWNER_CONTRACT: &str = r#"# Project Owner Contract

AGENTS.md governs coding and repository behavior when it exists. This project contract governs Loom project interaction.

## Role

The owner coordinates the project, reviews completed ticket work, manages lane assignments, records decisions on tickets, and asks the human owner before pulling new scope into the project.

## Go Contract

When the owner is told to go, read the current project settings, inspect lanes with tickets that are waiting for review, verify completed work, record feedback or acceptance on the ticket, and then decide whether the existing queues need more work. If no completed work needs review, prepare the next useful ticket batch and ask the human owner before adding new scope.

## Review Rules

Accepting a ticket is a correctness and code-review pass, not a cursory source-link inspection. Before accepting, personally inspect the relevant source anchors and verify that the changed code implements the requested behavior, follows the intended architecture, handles relevant edge cases and error paths, and updates public, generated, binding, protocol, test, and documentation surfaces together when contracts change.

Review feedback belongs on the ticket as a ticket comment. Do not rely on chat-only feedback. Feedback must name the issue, cite the source anchor when source is involved, and state the next action needed from the worker.

## Lane Management

Use lanes as assignment queues. A lane owns an ordered set of ticket ids. Do not remove or reshuffle unrelated tickets while reviewing one ticket. When a lane is empty and more project work appears valuable, ask the human owner before pulling in new scope. When moving existing approved-scope tickets between lanes, keep dependency order intact and avoid assigning build-heavy work to lanes that cannot run the needed checks.

## Ticket State

Use the ticket status as the durable workflow signal. If a ticket is complete enough for review, it should be waiting_for_review. If review finds changes are needed, move it to feedback_available and record the feedback comment. If a ticket is accepted, the acceptance comment must include the source anchors checked, checks run, and any remaining risk.

## Questions

Ask project questions in chat only when the decision truly needs the human owner. Also record the question on the ticket. Questions include Context, Options, and Recommendation. Recommendations must prefer enterprise-quality design, clear contracts, maintainable structure, reusable patterns, efficient execution, source-backed behavior, operational reliability, and long-term project health over short-term convenience.

## Handoff Output

Before handing control back to chat, state which tickets were accepted, which tickets need worker follow-up, which lanes can be told go, which commands or checks were run, and what should happen next.
"#;
pub const TICKET_DEFAULT_WORKER_CONTRACT: &str = r#"# Project Worker Contract

AGENTS.md governs coding and repository behavior when it exists. This project contract governs Loom project interaction.

## Role

The worker completes tickets assigned to its lane. The worker reads the lane, selects the next workable ticket in rank order, performs source-backed work, records progress and closeout evidence on the ticket, and leaves the ticket ready for owner review.

## Go Contract

When the worker is told to go, read the current project settings and this worker contract, read the assigned lane, check whether the active ticket has feedback, resolve that feedback first, and then continue with the next available ticket in the lane. Do not invent scope. Do not skip dependency or blocker notes on the ticket.

## Ticket Source Of Truth

Durable workflow state belongs on tickets and ticket comments, not only in chat. Record progress, blockers, questions, source anchors, checks run, files changed, and closeout evidence on the ticket. Chat output is only a human-readable handoff summary.

## Work Rules

Before changing code, read the authoritative source for every cross-boundary contract you rely on. Follow existing local patterns unless a ticket explicitly asks for a design change. Keep diffs scoped to the ticket. Do not add temporary migration or repair tools to final source unless the ticket explicitly asks for a durable tool.

Implementation choices must prefer enterprise-quality design, clear contracts, maintainable structure, reusable patterns, efficient execution, source-backed behavior, operational reliability, and long-term project health over short-term convenience.

## Questions And Blockers

If a real design decision or blocker prevents correct work, stop the ticket, record the question or blocker on the ticket, and summarize it in chat. Questions include Context, Options, and Recommendation. Recommendations must identify the long-term enterprise choice, not merely the easiest implementation.

## Checks And Handoff

Run the focused checks appropriate for the ticket when the lane environment supports them. If the lane cannot run a needed command, record the exact command and why it is needed on the ticket. Before handing control back to chat, print the ticket id, status, source anchors checked, files changed, checks run or not run, blockers or questions, and the next ticket the lane expects to work.
"#;
pub const TICKET_CONTRACTS_NOTE: &str = "Contract summaries are shown by default. Full contract details can be read by requesting project settings with contracts included or by reading contracts.owner.details / contracts.worker.details directly.";
pub const TICKET_COMMENT_TYPES: &[&str] = &[
    "general",
    "progress",
    "acceptance_evidence",
    "closeout_evidence",
    "review_request",
    "review_feedback",
    "code_review",
    "design_review",
    "decision",
    "blocker",
    "resolution",
];

#[derive(Debug, Clone, PartialEq)]
pub struct TicketProject {
    pub project_id: String,
    pub key_prefix: String,
    pub name: String,
    pub next_ticket_number: u64,
    pub retired_prefixes: BTreeSet<String>,
    pub lifecycle_authorization_policy: TicketLifecycleAuthorizationPolicy,
    pub project_owner_principal: Option<String>,
    pub acceptance_authorities: BTreeSet<String>,
    pub acceptance_evidence_policy: TicketAcceptanceEvidencePolicy,
    pub active_workflow: Option<WorkflowDefinition>,
    pub projection_config: TicketProjectionProjectConfig,
    pub contracts: TicketProjectContracts,
    pub custom_field_definitions: BTreeMap<String, TicketCustomFieldDefinition>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketProjectContracts {
    pub note: String,
    pub owner: TicketProjectContract,
    pub worker: TicketProjectContract,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketProjectContract {
    pub summary: String,
    pub details: String,
}

impl Default for TicketProjectContracts {
    fn default() -> Self {
        Self {
            note: TICKET_CONTRACTS_NOTE.to_string(),
            owner: TicketProjectContract {
                summary: "Owner verifies completed work before acceptance.".to_string(),
                details: TICKET_DEFAULT_OWNER_CONTRACT.to_string(),
            },
            worker: TicketProjectContract {
                summary: "Worker records durable ticket state and asks before new scope."
                    .to_string(),
                details: TICKET_DEFAULT_WORKER_CONTRACT.to_string(),
            },
        }
    }
}

impl TicketProjectContracts {
    const SCHEMA: &'static str = "loom.studio.tickets.project-contracts.v1";

    pub fn validate(&self) -> Result<()> {
        validate_text("project contracts note", &self.note)?;
        self.owner.validate("owner")?;
        self.worker.validate("worker")?;
        Ok(())
    }

    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(Self::SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.note.clone()),
                self.owner.to_value(),
                self.worker.to_value(),
            ]),
        ])
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "ticket project contracts")?;
        outer.expect_text(Self::SCHEMA)?;
        let raw_fields = match outer.next("ticket project contract fields")? {
            Value::Array(values) => values,
            _ => {
                return Err(LoomError::corrupt(
                    "ticket project contract fields must be an array",
                ));
            }
        };
        outer.end("ticket project contracts")?;
        let mut values = raw_fields.into_iter();
        let first = read_required_value(values.next(), "project contracts note or owner")?;
        let (note, owner_value) = match first {
            Value::Text(note) => (note, read_required_value(values.next(), "owner contract")?),
            value => (TICKET_CONTRACTS_NOTE.to_string(), value),
        };
        let contracts = Self {
            note,
            owner: TicketProjectContract::from_value(owner_value)?,
            worker: TicketProjectContract::from_value(read_required_value(
                values.next(),
                "worker contract",
            )?)?,
        };
        if values.next().is_some() {
            return Err(LoomError::corrupt(
                "ticket project contracts have trailing fields",
            ));
        }
        contracts.validate()?;
        Ok(contracts)
    }
}

impl TicketProjectContract {
    const SCHEMA: &'static str = "loom.studio.tickets.project-contract.v1";

    pub fn validate(&self, role: &str) -> Result<()> {
        validate_text(&format!("project {role} contract summary"), &self.summary)?;
        validate_ticket_rich_text(&format!("project {role} contract details"), &self.details)?;
        Ok(())
    }

    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(Self::SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.summary.clone()),
                Value::Text(self.details.clone()),
            ]),
        ])
    }

    pub fn from_value(value: Value) -> Result<Self> {
        if matches!(value, Value::Text(_)) {
            let details = read_text_value(Some(value), "project contract details")?;
            let contract = Self {
                summary: contract_summary_from_details(&details),
                details,
            };
            contract.validate("legacy")?;
            return Ok(contract);
        }
        let mut outer = Fields::array(value, "ticket project contract")?;
        outer.expect_text(Self::SCHEMA)?;
        let raw_fields = match outer.next("ticket project contract fields")? {
            Value::Array(values) => values,
            _ => {
                return Err(LoomError::corrupt(
                    "ticket project contract fields must be an array",
                ));
            }
        };
        outer.end("ticket project contract")?;
        let mut values = raw_fields.into_iter();
        let contract = Self {
            summary: read_text_value(values.next(), "project contract summary")?,
            details: read_text_value(values.next(), "project contract details")?,
        };
        if values.next().is_some() {
            return Err(LoomError::corrupt(
                "ticket project contract has trailing fields",
            ));
        }
        contract.validate("project")?;
        Ok(contract)
    }
}

fn contract_summary_from_details(details: &str) -> String {
    details
        .lines()
        .find_map(|line| {
            let line = line.trim();
            (!line.is_empty() && !line.starts_with('#')).then(|| line.to_string())
        })
        .unwrap_or_else(|| "Project contract".to_string())
}

#[derive(Debug, Clone, PartialEq)]
pub struct TicketProfileSnapshot {
    pub workspace_id: String,
    pub projects: Vec<TicketProject>,
    pub tickets: Vec<Ticket>,
    pub boards: Vec<TicketBoard>,
    pub sprints: Vec<Sprint>,
}

impl TicketProfileSnapshot {
    pub fn new(
        workspace_id: impl Into<String>,
        projects: Vec<TicketProject>,
        tickets: Vec<Ticket>,
        boards: Vec<TicketBoard>,
        sprints: Vec<Sprint>,
    ) -> Result<Self> {
        let snapshot = Self {
            workspace_id: workspace_id.into(),
            projects,
            tickets,
            boards,
            sprints,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn from_workspaces(
        workspace_id: impl Into<String>,
        tickets: &TicketWorkspace,
        agile: &AgileWorkspace,
    ) -> Result<Self> {
        Self::new(
            workspace_id,
            tickets.projects.values().cloned().collect(),
            tickets.tickets.values().cloned().collect(),
            Vec::new(),
            agile.sprints.values().cloned().collect(),
        )
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(PROFILE_SNAPSHOT_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.workspace_id.clone()),
                Value::Array(self.projects.iter().map(TicketProject::to_value).collect()),
                Value::Array(self.tickets.iter().map(Ticket::to_value).collect()),
                Value::Array(self.boards.iter().map(TicketBoard::to_value).collect()),
                Value::Array(self.sprints.iter().map(Sprint::to_value).collect()),
            ]),
        ])
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "ticket profile snapshot")?;
        outer.expect_text(PROFILE_SNAPSHOT_SCHEMA)?;
        let mut fields = Fields::array(
            outer.next("ticket profile snapshot fields")?,
            "ticket profile snapshot",
        )?;
        outer.end("ticket profile snapshot")?;
        let workspace_id = fields.text("workspace_id")?;
        let projects = project_list(fields.next("projects")?)?;
        let tickets = ticket_list(fields.next("tickets")?)?;
        let boards = board_list(fields.next("boards")?)?;
        let sprints = sprint_list(fields.next("sprints")?)?;
        fields.end("ticket profile snapshot")?;
        Self::new(workspace_id, projects, tickets, boards, sprints)
    }

    fn validate(&self) -> Result<()> {
        validate_text("workspace_id", &self.workspace_id)?;
        let mut project_ids = BTreeSet::new();
        for project in &self.projects {
            project.validate()?;
            if !project_ids.insert(project.project_id.clone()) {
                return Err(LoomError::invalid("snapshot project ids must be unique"));
            }
        }
        let mut ticket_ids = BTreeSet::new();
        for ticket in &self.tickets {
            ticket.validate()?;
            if !ticket_ids.insert(ticket.ticket_id.clone()) {
                return Err(LoomError::invalid("snapshot ticket ids must be unique"));
            }
            if !project_ids.contains(&ticket.project_id) {
                return Err(LoomError::invalid("snapshot ticket project is missing"));
            }
        }
        let mut board_ids = BTreeSet::new();
        for board in &self.boards {
            board.validate()?;
            if !board_ids.insert(board.board_id.clone()) {
                return Err(LoomError::invalid("snapshot board ids must be unique"));
            }
            if !project_ids.contains(&board.project_id) {
                return Err(LoomError::invalid("snapshot board project is missing"));
            }
        }
        let mut sprint_ids = BTreeSet::new();
        for sprint in &self.sprints {
            sprint.validate()?;
            if !sprint_ids.insert(sprint.sprint_id.clone()) {
                return Err(LoomError::invalid("snapshot sprint ids must be unique"));
            }
            if !project_ids.contains(&sprint.project_id) {
                return Err(LoomError::invalid("snapshot sprint project is missing"));
            }
        }
        Ok(())
    }

    pub fn resolve_ticket_key(&self, ticket_key: &str) -> Result<Option<TicketKeyResolution>> {
        let requested_key = TicketKey::parse(ticket_key)?;
        let Some(project) = self
            .projects
            .iter()
            .find(|project| project.prefix_status(&requested_key.prefix).is_some())
        else {
            return Ok(None);
        };
        let Some(status) = project.prefix_status(&requested_key.prefix) else {
            return Ok(None);
        };
        let Some(ticket) = self.tickets.iter().find(|ticket| {
            ticket.project_id == project.project_id && ticket.ticket_number == requested_key.number
        }) else {
            return Ok(None);
        };
        Ok(Some(TicketKeyResolution {
            ticket_id: ticket.ticket_id.clone(),
            requested_key,
            current_key: project.ticket_key(ticket.ticket_number)?,
            status,
        }))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketProfileState {
    pub workspace_id: String,
    pub next_sequence: u64,
    pub projects_root: Digest,
    pub prefixes_root: Digest,
    pub tickets_root: Digest,
    pub ticket_numbers_root: Digest,
    pub external_ids_root: Digest,
    pub boards_root: Digest,
    pub board_cards_root: Digest,
    pub board_roots_present: bool,
}

impl TicketProfileState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        workspace_id: impl Into<String>,
        next_sequence: u64,
        projects_root: Digest,
        prefixes_root: Digest,
        tickets_root: Digest,
        ticket_numbers_root: Digest,
        external_ids_root: Digest,
        boards_root: Digest,
        board_cards_root: Digest,
    ) -> Result<Self> {
        let state = Self {
            workspace_id: workspace_id.into(),
            next_sequence,
            projects_root,
            prefixes_root,
            tickets_root,
            ticket_numbers_root,
            external_ids_root,
            boards_root,
            board_cards_root,
            board_roots_present: true,
        };
        validate_text("ticket profile workspace_id", &state.workspace_id)?;
        if state.next_sequence == 0 {
            return Err(LoomError::invalid(
                "ticket profile next sequence must start at 1",
            ));
        }
        Ok(state)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(PROFILE_STATE_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.workspace_id.clone()),
                Value::Uint(self.next_sequence),
                Value::Text(self.projects_root.to_string()),
                Value::Text(self.prefixes_root.to_string()),
                Value::Text(self.tickets_root.to_string()),
                Value::Text(self.ticket_numbers_root.to_string()),
                Value::Text(self.external_ids_root.to_string()),
                Value::Text(self.boards_root.to_string()),
                Value::Text(self.board_cards_root.to_string()),
            ]),
        ])
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "ticket profile state")?;
        outer.expect_text(PROFILE_STATE_SCHEMA)?;
        let state_fields = outer.next("ticket profile state fields")?;
        outer.end("ticket profile state")?;
        let Value::Array(values) = state_fields else {
            return Err(LoomError::corrupt(
                "ticket profile state fields must be an array",
            ));
        };
        let legacy_tail = values.len() == 7;
        let mut fields = Fields::array(Value::Array(values), "ticket profile state")?;
        let workspace_id = fields.text("workspace_id")?;
        let next_sequence = fields.uint("next_sequence")?;
        let projects_root = fields.digest("projects_root")?;
        let prefixes_root = fields.digest("prefixes_root")?;
        let tickets_root = fields.digest("tickets_root")?;
        let ticket_numbers_root = fields.digest("ticket_numbers_root")?;
        let external_ids_root = fields.digest("external_ids_root")?;
        let boards_root = if legacy_tail {
            external_ids_root
        } else {
            fields.digest("boards_root")?
        };
        let board_cards_root = if legacy_tail {
            external_ids_root
        } else {
            fields.digest("board_cards_root")?
        };
        fields.end("ticket profile state")?;
        let mut state = Self::new(
            workspace_id,
            next_sequence,
            projects_root,
            prefixes_root,
            tickets_root,
            ticket_numbers_root,
            external_ids_root,
            boards_root,
            board_cards_root,
        )?;
        state.board_roots_present = !legacy_tail;
        Ok(state)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketOperationRecord {
    pub sequence: u64,
    pub operation_id: String,
    pub operation_kind: String,
    pub target_entity_id: Option<String>,
    pub root_after: Digest,
    pub envelope: Vec<u8>,
    pub validation: Option<WorkflowValidationRecord>,
}

impl TicketOperationRecord {
    pub fn new(
        sequence: u64,
        operation_id: impl Into<String>,
        operation_kind: impl Into<String>,
        target_entity_id: Option<String>,
        root_after: Digest,
        envelope: Vec<u8>,
        validation: Option<WorkflowValidationRecord>,
    ) -> Result<Self> {
        let record = Self {
            sequence,
            operation_id: operation_id.into(),
            operation_kind: operation_kind.into(),
            target_entity_id,
            root_after,
            envelope,
            validation,
        };
        record.validate()?;
        Ok(record)
    }

    fn validate(&self) -> Result<()> {
        validate_text("ticket operation_id", &self.operation_id)?;
        validate_text("ticket operation_kind", &self.operation_kind)?;
        if let Some(target) = &self.target_entity_id {
            validate_text("ticket operation target", target)?;
        }
        if self.envelope.is_empty() {
            return Err(LoomError::invalid(
                "ticket operation envelope must not be empty",
            ));
        }
        Ok(())
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Uint(self.sequence),
            Value::Text(self.operation_id.clone()),
            Value::Text(self.operation_kind.clone()),
            optional_text_value(self.target_entity_id.as_deref()),
            digest_value(self.root_after),
            Value::Bytes(self.envelope.clone()),
            optional_validation_value(self.validation.as_ref()),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut fields = Fields::array(value, "ticket operation record")?;
        let sequence = fields.uint("sequence")?;
        let operation_id = fields.text("operation_id")?;
        let operation_kind = fields.text("operation_kind")?;
        let target_entity_id = read_optional_text_field(&mut fields, "target_entity_id")?;
        let root_after = fields.digest("root_after")?;
        let envelope = fields.bytes("envelope")?;
        let validation = read_optional_validation(&mut fields, "validation")?;
        fields.end("ticket operation record")?;
        Self::new(
            sequence,
            operation_id,
            operation_kind,
            target_entity_id,
            root_after,
            envelope,
            validation,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketOperationLog {
    pub workspace_id: String,
    pub records: Vec<TicketOperationRecord>,
}

impl TicketOperationLog {
    pub fn new(
        workspace_id: impl Into<String>,
        records: Vec<TicketOperationRecord>,
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

    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(PROFILE_OPERATION_LOG_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.workspace_id.clone()),
                Value::Array(
                    self.records
                        .iter()
                        .map(TicketOperationRecord::to_value)
                        .collect(),
                ),
            ]),
        ])
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "ticket operation log")?;
        outer.expect_text(PROFILE_OPERATION_LOG_SCHEMA)?;
        let mut fields = Fields::array(
            outer.next("ticket operation log fields")?,
            "ticket operation log",
        )?;
        outer.end("ticket operation log")?;
        let workspace_id = fields.text("workspace_id")?;
        let records = operation_record_list(fields.next("records")?)?;
        fields.end("ticket operation log")?;
        Self::new(workspace_id, records)
    }

    fn validate(&self) -> Result<()> {
        validate_text("workspace_id", &self.workspace_id)?;
        let mut previous = None;
        let mut ids = BTreeSet::new();
        for record in &self.records {
            record.validate()?;
            if !ids.insert(record.operation_id.clone()) {
                return Err(LoomError::invalid("ticket operation ids must be unique"));
            }
            if let Some(previous) = previous
                && record.sequence <= previous
            {
                return Err(LoomError::invalid(
                    "ticket operation records must be ordered by increasing sequence",
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
        let expected_scope = ticket_operation_cursor_scope(&self.workspace_id);
        if cursor.scope_id != expected_scope {
            return Err(LoomError::invalid(
                "operation change cursor scope does not match ticket operation log",
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

impl TicketProject {
    pub fn new(
        project_id: impl Into<String>,
        key_prefix: impl Into<String>,
        name: impl Into<String>,
    ) -> Result<Self> {
        let project_id = project_id.into();
        let key_prefix = normalize_ticket_key_prefix(&key_prefix.into())?;
        let name = name.into();
        validate_text("project_id", &project_id)?;
        validate_text("project name", &name)?;
        Ok(Self {
            project_id,
            key_prefix,
            name,
            next_ticket_number: 1,
            retired_prefixes: BTreeSet::new(),
            lifecycle_authorization_policy: TicketLifecycleAuthorizationPolicy::WriteAccess,
            project_owner_principal: None,
            acceptance_authorities: BTreeSet::new(),
            acceptance_evidence_policy: TicketAcceptanceEvidencePolicy::default(),
            active_workflow: None,
            projection_config: TicketProjectionProjectConfig::default(),
            contracts: TicketProjectContracts::default(),
            custom_field_definitions: BTreeMap::new(),
        })
    }

    pub fn allocate_ticket_number(&mut self) -> Result<u64> {
        if self.next_ticket_number == 0 {
            return Err(LoomError::invalid("project ticket counter must start at 1"));
        }
        let number = self.next_ticket_number;
        self.next_ticket_number = self
            .next_ticket_number
            .checked_add(1)
            .ok_or_else(|| LoomError::invalid("project ticket counter overflow"))?;
        Ok(number)
    }

    pub fn ticket_key(&self, ticket_number: u64) -> Result<TicketKey> {
        if ticket_number == 0 {
            return Err(LoomError::invalid("ticket number must be positive"));
        }
        Ok(TicketKey {
            prefix: self.key_prefix.clone(),
            number: ticket_number,
        })
    }

    pub fn rekey(&mut self, key_prefix: impl Into<String>) -> Result<String> {
        let key_prefix = normalize_ticket_key_prefix(&key_prefix.into())?;
        if key_prefix == self.key_prefix {
            return Err(LoomError::new(
                Code::AlreadyExists,
                "ticket project already uses this key prefix",
            ));
        }
        let prior_prefix = std::mem::replace(&mut self.key_prefix, key_prefix);
        self.retired_prefixes.insert(prior_prefix.clone());
        Ok(prior_prefix)
    }

    pub fn prefix_status(&self, prefix: &str) -> Option<TicketKeyStatus> {
        let prefix = normalize_ticket_key_prefix(prefix).ok()?;
        if prefix == self.key_prefix {
            Some(TicketKeyStatus::Active)
        } else if self.retired_prefixes.contains(&prefix) {
            Some(TicketKeyStatus::Retired)
        } else {
            None
        }
    }

    pub fn put_custom_field_definition(
        &mut self,
        field: TicketCustomFieldDefinition,
    ) -> Result<()> {
        field.validate()?;
        let field_id = field.definition.field_id.clone();
        validate_field_key(&field_id)?;
        if !field.applicable_project_ids.is_empty()
            && !field.applicable_project_ids.contains(&self.project_id)
        {
            return Err(LoomError::invalid(
                "ticket custom field does not apply to this project",
            ));
        }
        self.custom_field_definitions.insert(field_id, field);
        self.validate()
    }

    pub fn custom_field_definition(
        &self,
        field_id: &str,
    ) -> Result<Option<&TicketCustomFieldDefinition>> {
        validate_field_key(field_id)?;
        Ok(self.custom_field_definitions.get(field_id))
    }

    pub fn retire_custom_field_definition(&mut self, field_id: &str) -> Result<()> {
        validate_field_key(field_id)?;
        let field = self
            .custom_field_definitions
            .get_mut(field_id)
            .ok_or_else(|| LoomError::not_found("ticket custom field definition not found"))?;
        field.retire();
        self.validate()
    }

    pub fn validate(&self) -> Result<()> {
        validate_text("project_id", &self.project_id)?;
        normalize_ticket_key_prefix(&self.key_prefix)?;
        validate_text("project name", &self.name)?;
        if self.next_ticket_number == 0 {
            return Err(LoomError::invalid("project ticket counter must start at 1"));
        }
        for prefix in &self.retired_prefixes {
            normalize_ticket_key_prefix(prefix)?;
        }
        if let Some(principal) = &self.project_owner_principal {
            validate_text("ticket project owner", principal)?;
        }
        for principal in &self.acceptance_authorities {
            validate_text("ticket acceptance authority", principal)?;
        }
        self.acceptance_evidence_policy.validate()?;
        self.contracts.validate()?;
        if let Some(workflow) = &self.active_workflow {
            workflow.validate()?;
        }
        self.projection_config.validate()?;
        for (field_id, field) in &self.custom_field_definitions {
            validate_field_key(field_id)?;
            if field_id != &field.definition.field_id {
                return Err(LoomError::invalid(
                    "ticket custom field key must match field definition id",
                ));
            }
            field.validate()?;
            if !field.is_applicable(&self.project_id, "task")
                && field.applicable_project_ids.is_empty()
            {
                return Err(LoomError::invalid(
                    "ticket custom field project applicability is invalid",
                ));
            }
        }
        Ok(())
    }

    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(PROJECT_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.project_id.clone()),
                Value::Text(self.key_prefix.clone()),
                Value::Text(self.name.clone()),
                Value::Uint(self.next_ticket_number),
                Value::Array(
                    self.retired_prefixes
                        .iter()
                        .map(|prefix| Value::Text(prefix.clone()))
                        .collect(),
                ),
                Value::Uint(self.lifecycle_authorization_policy.tag()),
                self.project_owner_principal
                    .as_ref()
                    .map(|principal| Value::Text(principal.clone()))
                    .unwrap_or(Value::Null),
                Value::Array(
                    self.acceptance_authorities
                        .iter()
                        .map(|principal| Value::Text(principal.clone()))
                        .collect(),
                ),
                self.acceptance_evidence_policy.to_value(),
                optional_workflow_definition_value(self.active_workflow.as_ref()),
                self.projection_config.to_value(),
                self.contracts.to_value(),
                custom_field_definition_map_value(&self.custom_field_definitions),
            ]),
        ])
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "ticket project")?;
        outer.expect_text(PROJECT_SCHEMA)?;
        let raw_fields = match outer.next("ticket project fields")? {
            Value::Array(values) => values,
            _ => return Err(LoomError::corrupt("ticket project fields must be an array")),
        };
        outer.end("ticket project")?;
        let mut values = raw_fields.into_iter();
        let project_id = read_text_value(values.next(), "project_id")?;
        let key_prefix =
            normalize_ticket_key_prefix(&read_text_value(values.next(), "key_prefix")?)?;
        let name = read_text_value(values.next(), "name")?;
        let next_ticket_number = read_uint_value(values.next(), "next_ticket_number")?;
        let retired_prefixes = string_set(
            read_required_value(values.next(), "retired_prefixes")?,
            "retired_prefixes",
        )?;
        let lifecycle_authorization_policy = TicketLifecycleAuthorizationPolicy::from_tag(
            read_uint_value(values.next(), "lifecycle_authorization_policy")?,
        )?;
        let project_owner_principal = optional_text(read_required_value(
            values.next(),
            "project_owner_principal",
        )?)?;
        let acceptance_authorities = string_set(
            read_required_value(values.next(), "acceptance_authorities")?,
            "acceptance_authorities",
        )?;
        let next = read_required_value(values.next(), "active_workflow")?;
        let (acceptance_evidence_policy, active_workflow_value) = if matches!(
            next,
            Value::Array(ref values)
                if values.first() == Some(&Value::Text(TicketAcceptanceEvidencePolicy::SCHEMA.to_string()))
        ) {
            (
                TicketAcceptanceEvidencePolicy::from_value(next)?,
                read_required_value(values.next(), "active_workflow")?,
            )
        } else {
            (TicketAcceptanceEvidencePolicy::default(), next)
        };
        let active_workflow = optional_workflow_definition_from_value(active_workflow_value)?;
        let projection_config = TicketProjectionProjectConfig::from_value(read_required_value(
            values.next(),
            "projection_config",
        )?)?;
        let next = values.next();
        let (contracts, custom_field_definitions_value) = match next {
            Some(ref value @ Value::Array(ref fields))
                if fields.first().and_then(|value| match value {
                    Value::Text(text) => Some(text.as_str()),
                    _ => None,
                }) == Some(TicketProjectContracts::SCHEMA) =>
            {
                (
                    TicketProjectContracts::from_value(value.clone())?,
                    values.next(),
                )
            }
            value => (TicketProjectContracts::default(), value),
        };
        let custom_field_definitions = match custom_field_definitions_value {
            Some(value) => custom_field_definition_map(value, "custom_field_definitions")?,
            None => BTreeMap::new(),
        };
        if values.next().is_some() {
            return Err(LoomError::corrupt("ticket project has trailing fields"));
        }
        let project = Self {
            project_id,
            key_prefix,
            name,
            next_ticket_number,
            retired_prefixes,
            lifecycle_authorization_policy,
            project_owner_principal,
            acceptance_authorities,
            acceptance_evidence_policy,
            active_workflow,
            projection_config,
            contracts,
            custom_field_definitions,
        };
        project.validate()?;
        Ok(project)
    }
}

fn optional_text(value: Value) -> Result<Option<String>> {
    match value {
        Value::Null => Ok(None),
        Value::Text(text) => Ok(Some(text)),
        _ => Err(LoomError::corrupt("expected text or null")),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TicketAcceptanceEvidencePolicy {
    pub enforcement_enabled: bool,
    pub required_keys: BTreeSet<TicketAcceptanceEvidenceKey>,
}

impl TicketAcceptanceEvidencePolicy {
    pub const SCHEMA: &'static str = "loom.studio.tickets.acceptance-evidence-policy.v1";

    pub fn validate(&self) -> Result<()> {
        Ok(())
    }

    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(Self::SCHEMA.to_string()),
            Value::Array(vec![
                Value::Bool(self.enforcement_enabled),
                Value::Array(
                    self.required_keys
                        .iter()
                        .map(|key| Value::Text(key.as_str().to_string()))
                        .collect(),
                ),
            ]),
        ])
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "ticket acceptance evidence policy")?;
        outer.expect_text(Self::SCHEMA)?;
        let mut fields = Fields::array(
            outer.next("ticket acceptance evidence policy fields")?,
            "ticket acceptance evidence policy",
        )?;
        outer.end("ticket acceptance evidence policy")?;
        let enforcement_enabled = match fields.next("enforcement_enabled")? {
            Value::Bool(value) => value,
            _ => {
                return Err(LoomError::corrupt(
                    "ticket acceptance evidence enforcement_enabled must be a boolean",
                ));
            }
        };
        let required_keys = match fields.next("required_keys")? {
            Value::Array(values) => values
                .into_iter()
                .map(|value| match value {
                    Value::Text(text) => TicketAcceptanceEvidenceKey::parse(&text),
                    _ => Err(LoomError::corrupt(
                        "ticket acceptance evidence key must be text",
                    )),
                })
                .collect::<Result<BTreeSet<_>>>()?,
            _ => {
                return Err(LoomError::corrupt(
                    "ticket acceptance evidence required_keys must be an array",
                ));
            }
        };
        fields.end("ticket acceptance evidence policy")?;
        let policy = Self {
            enforcement_enabled,
            required_keys,
        };
        policy.validate()?;
        Ok(policy)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TicketAcceptanceEvidenceKey {
    SourceAnchors,
    ChecksRun,
    NotRunRationale,
    FilesChanged,
    Followups,
    DecisionPoints,
    RiskNotes,
}

impl TicketAcceptanceEvidenceKey {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "source_anchors" => Ok(Self::SourceAnchors),
            "checks_run" => Ok(Self::ChecksRun),
            "not_run_rationale" => Ok(Self::NotRunRationale),
            "files_changed" => Ok(Self::FilesChanged),
            "followups" => Ok(Self::Followups),
            "decision_points" => Ok(Self::DecisionPoints),
            "risk_notes" => Ok(Self::RiskNotes),
            _ => Err(LoomError::invalid("unknown ticket acceptance evidence key")),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SourceAnchors => "source_anchors",
            Self::ChecksRun => "checks_run",
            Self::NotRunRationale => "not_run_rationale",
            Self::FilesChanged => "files_changed",
            Self::Followups => "followups",
            Self::DecisionPoints => "decision_points",
            Self::RiskNotes => "risk_notes",
        }
    }

    pub const fn meaning(self) -> &'static str {
        match self {
            Self::SourceAnchors => {
                "authoritative file and line references checked before acceptance"
            }
            Self::ChecksRun => "commands or automated validations executed before acceptance",
            Self::NotRunRationale => "why expected checks were not run",
            Self::FilesChanged => "files materially changed by the ticket",
            Self::Followups => "known follow-up work left outside this ticket",
            Self::DecisionPoints => "owner decisions raised or resolved for this ticket",
            Self::RiskNotes => "risks, caveats, or rollback notes relevant to acceptance",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TicketLifecycleAuthorizationPolicy {
    WriteAccess,
    OwnershipGoverned,
    Assignee,
    ReviewAuthority,
}

impl TicketLifecycleAuthorizationPolicy {
    pub const fn tag(self) -> u64 {
        match self {
            Self::WriteAccess => 0,
            Self::OwnershipGoverned => 1,
            Self::Assignee => 2,
            Self::ReviewAuthority => 3,
        }
    }

    pub fn from_tag(tag: u64) -> Result<Self> {
        match tag {
            0 => Ok(Self::WriteAccess),
            1 => Ok(Self::OwnershipGoverned),
            2 => Ok(Self::Assignee),
            3 => Ok(Self::ReviewAuthority),
            other => Err(LoomError::corrupt(format!(
                "unknown ticket lifecycle authorization policy tag {other}"
            ))),
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "write_access" | "write-access" => Ok(Self::WriteAccess),
            "assignee" => Ok(Self::Assignee),
            "review_authority" | "review-authority" => Ok(Self::ReviewAuthority),
            "ownership_governed" | "ownership-governed" => Ok(Self::OwnershipGoverned),
            _ => Err(LoomError::invalid(
                "unknown ticket lifecycle authorization policy",
            )),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WriteAccess => "write_access",
            Self::OwnershipGoverned => "ownership_governed",
            Self::Assignee => "assignee",
            Self::ReviewAuthority => "review_authority",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct TicketKey {
    pub prefix: String,
    pub number: u64,
}

impl TicketKey {
    pub fn parse(value: &str) -> Result<Self> {
        let Some((prefix, number)) = value.split_once('-') else {
            return Err(LoomError::invalid("ticket key must be PREFIX-N"));
        };
        let prefix = normalize_ticket_key_prefix(prefix)?;
        let number = number
            .parse::<u64>()
            .map_err(|_| LoomError::invalid("ticket key number must be a positive integer"))?;
        if number == 0 {
            return Err(LoomError::invalid("ticket key number must be positive"));
        }
        Ok(Self { prefix, number })
    }

    pub fn canonical(&self) -> String {
        format!("{}-{}", self.prefix, self.number)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TicketType {
    Epic,
    Story,
    Task,
    Bug,
    Spike,
    Subtask,
}

impl TicketType {
    pub const fn type_id(self) -> &'static str {
        match self {
            Self::Epic => "epic",
            Self::Story => "story",
            Self::Task => "task",
            Self::Bug => "bug",
            Self::Spike => "spike",
            Self::Subtask => "subtask",
        }
    }

    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Epic => "Epic",
            Self::Story => "Story",
            Self::Task => "Task",
            Self::Bug => "Bug",
            Self::Spike => "Spike",
            Self::Subtask => "Subtask",
        }
    }

    pub const fn semantic_kind(self) -> TicketTypeSemanticKind {
        match self {
            Self::Epic => TicketTypeSemanticKind::Portfolio,
            Self::Story | Self::Task => TicketTypeSemanticKind::WorkItem,
            Self::Bug => TicketTypeSemanticKind::Defect,
            Self::Spike => TicketTypeSemanticKind::Research,
            Self::Subtask => TicketTypeSemanticKind::Subtask,
        }
    }

    pub fn from_type_id(type_id: &str) -> Result<Self> {
        match normalize_ticket_type_id(type_id)?.as_str() {
            "epic" => Ok(Self::Epic),
            "story" => Ok(Self::Story),
            "task" => Ok(Self::Task),
            "bug" => Ok(Self::Bug),
            "spike" => Ok(Self::Spike),
            "subtask" => Ok(Self::Subtask),
            _ => Err(LoomError::invalid("unsupported ticket type")),
        }
    }

    const fn tag(self) -> u64 {
        match self {
            Self::Epic => 0,
            Self::Story => 1,
            Self::Task => 2,
            Self::Bug => 3,
            Self::Spike => 4,
            Self::Subtask => 5,
        }
    }

    fn from_tag(tag: u64) -> Result<Self> {
        match tag {
            0 => Ok(Self::Epic),
            1 => Ok(Self::Story),
            2 => Ok(Self::Task),
            3 => Ok(Self::Bug),
            4 => Ok(Self::Spike),
            5 => Ok(Self::Subtask),
            other => Err(LoomError::corrupt(format!(
                "unknown ticket type tag {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TicketTypeSemanticKind {
    Portfolio,
    WorkItem,
    Defect,
    Research,
    Subtask,
    Custom,
}

impl TicketTypeSemanticKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Portfolio => "portfolio",
            Self::WorkItem => "work_item",
            Self::Defect => "defect",
            Self::Research => "research",
            Self::Subtask => "subtask",
            Self::Custom => "custom",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketTypeDefinition {
    pub type_id: String,
    pub display_name: String,
    pub semantic_kind: TicketTypeSemanticKind,
    pub retired: bool,
    pub applicable_project_ids: BTreeSet<String>,
}

impl TicketTypeDefinition {
    pub fn new(
        type_id: impl Into<String>,
        display_name: impl Into<String>,
        semantic_kind: TicketTypeSemanticKind,
        applicable_project_ids: BTreeSet<String>,
    ) -> Result<Self> {
        let definition = Self {
            type_id: normalize_ticket_type_id(&type_id.into())?,
            display_name: display_name.into(),
            semantic_kind,
            retired: false,
            applicable_project_ids,
        };
        definition.validate()?;
        Ok(definition)
    }

    pub fn builtin(ticket_type: TicketType) -> Result<Self> {
        Self::new(
            ticket_type.type_id(),
            ticket_type.display_name(),
            ticket_type.semantic_kind(),
            BTreeSet::new(),
        )
    }

    pub fn is_applicable_to_project(&self, project_id: &str) -> bool {
        self.applicable_project_ids.is_empty() || self.applicable_project_ids.contains(project_id)
    }

    pub fn retire(&mut self) {
        self.retired = true;
    }

    pub fn validate(&self) -> Result<()> {
        validate_ticket_type_id(&self.type_id)?;
        validate_text("ticket type display_name", &self.display_name)?;
        for project_id in &self.applicable_project_ids {
            validate_text("ticket type project_id", project_id)?;
        }
        Ok(())
    }
}

pub fn builtin_ticket_type_definitions() -> Result<Vec<TicketTypeDefinition>> {
    [
        TicketType::Epic,
        TicketType::Story,
        TicketType::Task,
        TicketType::Bug,
        TicketType::Spike,
        TicketType::Subtask,
    ]
    .into_iter()
    .map(TicketTypeDefinition::builtin)
    .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TicketFieldCardinality {
    Single,
    Optional,
    List {
        min_items: u32,
        max_items: Option<u32>,
    },
}

impl TicketFieldCardinality {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Single | Self::Optional => Ok(()),
            Self::List {
                min_items,
                max_items,
            } => {
                if let Some(max_items) = max_items
                    && min_items > max_items
                {
                    return Err(LoomError::invalid(
                        "ticket field cardinality min_items exceeds max_items",
                    ));
                }
                Ok(())
            }
        }
    }

    fn to_value(&self) -> Value {
        match self {
            Self::Single => tagged(0, Vec::new()),
            Self::Optional => tagged(1, Vec::new()),
            Self::List {
                min_items,
                max_items,
            } => tagged(
                2,
                vec![
                    Value::Uint(u64::from(*min_items)),
                    optional_u32_value(*max_items),
                ],
            ),
        }
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut fields = Fields::array(value, "ticket field cardinality")?;
        let tag = fields.uint("ticket field cardinality tag")?;
        let cardinality = match tag {
            0 => Self::Single,
            1 => Self::Optional,
            2 => Self::List {
                min_items: u32::try_from(fields.uint("min_items")?)
                    .map_err(|_| LoomError::corrupt("ticket field min_items exceeds u32"))?,
                max_items: read_optional_u32_field(&mut fields, "max_items")?,
            },
            _ => return Err(LoomError::corrupt("unknown ticket field cardinality tag")),
        };
        fields.end("ticket field cardinality")?;
        cardinality.validate()?;
        Ok(cardinality)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TicketCustomFieldDefinition {
    pub definition: FieldDefinition,
    pub max_length: Option<u32>,
    pub searchable: bool,
    pub orderable: bool,
    pub cardinality: TicketFieldCardinality,
    pub default_value: Option<FieldValue>,
    pub applicable_project_ids: BTreeSet<String>,
    pub applicable_type_ids: BTreeSet<String>,
    pub retired: bool,
}

impl TicketCustomFieldDefinition {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        definition: FieldDefinition,
        max_length: Option<u32>,
        searchable: bool,
        orderable: bool,
        cardinality: TicketFieldCardinality,
        default_value: Option<FieldValue>,
        applicable_project_ids: BTreeSet<String>,
        applicable_type_ids: BTreeSet<String>,
    ) -> Result<Self> {
        let field = Self {
            definition,
            max_length,
            searchable,
            orderable,
            cardinality,
            default_value,
            applicable_project_ids,
            applicable_type_ids: applicable_type_ids
                .into_iter()
                .map(|type_id| normalize_ticket_type_id(&type_id))
                .collect::<Result<BTreeSet<_>>>()?,
            retired: false,
        };
        field.validate()?;
        Ok(field)
    }

    pub fn retire(&mut self) {
        self.retired = true;
    }

    pub fn validate_ticket_value(&self, value: &FieldValue) -> Result<()> {
        if self.retired {
            return Err(LoomError::invalid("ticket custom field is retired"));
        }
        match &self.cardinality {
            TicketFieldCardinality::Single => {
                if matches!(value, FieldValue::Null | FieldValue::List(_)) {
                    return Err(LoomError::invalid(
                        "ticket custom field requires a single value",
                    ));
                }
                self.definition.validate_value(value)?;
                if let Some(max_length) = self.max_length {
                    validate_ticket_value_max_length(value, max_length as usize)?;
                }
            }
            TicketFieldCardinality::Optional => {
                if matches!(value, FieldValue::List(_)) {
                    return Err(LoomError::invalid(
                        "ticket custom field requires a single optional value",
                    ));
                }
                if !matches!(value, FieldValue::Null) {
                    self.definition.validate_value(value)?;
                    if let Some(max_length) = self.max_length {
                        validate_ticket_value_max_length(value, max_length as usize)?;
                    }
                }
            }
            TicketFieldCardinality::List {
                min_items,
                max_items,
            } => {
                let FieldValue::List(values) = value else {
                    return Err(LoomError::invalid(
                        "ticket custom field requires a list value",
                    ));
                };
                if values.len() < *min_items as usize {
                    return Err(LoomError::invalid(
                        "ticket custom field list has too few items",
                    ));
                }
                if let Some(max_items) = max_items
                    && values.len() > *max_items as usize
                {
                    return Err(LoomError::invalid(
                        "ticket custom field list has too many items",
                    ));
                }
                for value in values {
                    self.definition.validate_value(value)?;
                    if let Some(max_length) = self.max_length {
                        validate_ticket_value_max_length(value, max_length as usize)?;
                    }
                }
            }
        }
        Ok(())
    }

    pub fn is_applicable(&self, project_id: &str, type_id: &str) -> bool {
        let Ok(type_id) = normalize_ticket_type_id(type_id) else {
            return false;
        };
        (self.applicable_project_ids.is_empty() || self.applicable_project_ids.contains(project_id))
            && (self.applicable_type_ids.is_empty() || self.applicable_type_ids.contains(&type_id))
    }

    pub fn validate(&self) -> Result<()> {
        if let Some(max_length) = self.max_length
            && max_length == 0
        {
            return Err(LoomError::invalid(
                "ticket field max_length must be positive",
            ));
        }
        if self.orderable && !is_orderable_field_type(&self.definition.field_type) {
            return Err(LoomError::invalid("ticket field type is not orderable"));
        }
        self.cardinality.validate()?;
        if let Some(default_value) = &self.default_value {
            validate_ticket_custom_field_default(self, default_value)?;
        }
        for project_id in &self.applicable_project_ids {
            validate_text("ticket field project_id", project_id)?;
        }
        for type_id in &self.applicable_type_ids {
            validate_ticket_type_id(type_id)?;
        }
        Ok(())
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            self.definition.to_value(),
            optional_u32_value(self.max_length),
            Value::Bool(self.searchable),
            Value::Bool(self.orderable),
            self.cardinality.to_value(),
            optional_field_value(self.default_value.as_ref()),
            Value::Array(
                self.applicable_project_ids
                    .iter()
                    .map(|project_id| Value::Text(project_id.clone()))
                    .collect(),
            ),
            Value::Array(
                self.applicable_type_ids
                    .iter()
                    .map(|type_id| Value::Text(type_id.clone()))
                    .collect(),
            ),
            Value::Bool(self.retired),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut fields = Fields::array(value, "ticket custom field definition")?;
        let definition = FieldDefinition::from_value(fields.next("definition")?)?;
        let max_length = read_optional_u32_field(&mut fields, "max_length")?;
        let searchable = read_bool_field(&mut fields, "searchable")?;
        let orderable = read_bool_field(&mut fields, "orderable")?;
        let cardinality = TicketFieldCardinality::from_value(fields.next("cardinality")?)?;
        let default_value = optional_field_value_from_value(fields.next("default_value")?)?;
        let applicable_project_ids = string_set(
            fields.next("applicable_project_ids")?,
            "applicable_project_ids",
        )?;
        let applicable_type_ids =
            string_set(fields.next("applicable_type_ids")?, "applicable_type_ids")?;
        let retired = read_bool_field(&mut fields, "retired")?;
        fields.end("ticket custom field definition")?;
        let mut definition = Self::new(
            definition,
            max_length,
            searchable,
            orderable,
            cardinality,
            default_value,
            applicable_project_ids,
            applicable_type_ids,
        )?;
        definition.retired = retired;
        definition.validate()?;
        Ok(definition)
    }
}

fn is_orderable_field_type(field_type: &FieldType) -> bool {
    matches!(
        field_type,
        FieldType::String
            | FieldType::Integer
            | FieldType::Number
            | FieldType::Boolean
            | FieldType::Date
            | FieldType::DateTime
            | FieldType::Duration
            | FieldType::Principal
            | FieldType::Enum { .. }
            | FieldType::Url
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TicketProjectionProfile {
    Native,
    Jira,
    Asana,
    Notion,
    Redmine,
}

impl TicketProjectionProfile {
    pub const fn profile_id(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::Jira => "jira",
            Self::Asana => "asana",
            Self::Notion => "notion",
            Self::Redmine => "redmine",
        }
    }

    pub fn parse(profile_id: &str) -> Result<Self> {
        match profile_id {
            "native" => Ok(Self::Native),
            "jira" => Ok(Self::Jira),
            "asana" => Ok(Self::Asana),
            "notion" => Ok(Self::Notion),
            "redmine" => Ok(Self::Redmine),
            _ => Err(LoomError::invalid("unsupported ticket projection profile")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketProjectionContract {
    pub profile: TicketProjectionProfile,
    pub source: &'static str,
    pub tagged_response_kind: &'static str,
    pub silently_mutates_native_schema: bool,
}

pub fn ticket_projection_contract(profile: TicketProjectionProfile) -> TicketProjectionContract {
    TicketProjectionContract {
        profile,
        source: "canonical_ticket",
        tagged_response_kind: match profile {
            TicketProjectionProfile::Native => "ticket.native",
            TicketProjectionProfile::Jira => "ticket.projected.jira",
            TicketProjectionProfile::Asana => "ticket.projected.asana",
            TicketProjectionProfile::Notion => "ticket.projected.notion",
            TicketProjectionProfile::Redmine => "ticket.projected.redmine",
        },
        silently_mutates_native_schema: false,
    }
}

pub fn builtin_ticket_projection_contracts() -> Vec<TicketProjectionContract> {
    [
        TicketProjectionProfile::Native,
        TicketProjectionProfile::Jira,
        TicketProjectionProfile::Asana,
        TicketProjectionProfile::Notion,
        TicketProjectionProfile::Redmine,
    ]
    .into_iter()
    .map(ticket_projection_contract)
    .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TicketProjectionRequestContext {
    HumanDisplay,
    MachineApi,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TicketProjectionSelectionSource {
    ExplicitRequest,
    ProjectDefaultDisplay,
    MachineDefaultNative,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketProjectionProfileConfig {
    pub profile: TicketProjectionProfile,
    pub field_aliases: BTreeMap<String, String>,
    pub options: BTreeMap<String, String>,
}

impl TicketProjectionProfileConfig {
    pub fn new(
        profile: TicketProjectionProfile,
        field_aliases: BTreeMap<String, String>,
        options: BTreeMap<String, String>,
    ) -> Result<Self> {
        let config = Self {
            profile,
            field_aliases,
            options,
        };
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        for (field_id, alias) in &self.field_aliases {
            validate_text("ticket projection field_id", field_id)?;
            validate_text("ticket projection field alias", alias)?;
        }
        for (key, value) in &self.options {
            validate_text("ticket projection option key", key)?;
            validate_text("ticket projection option value", value)?;
        }
        Ok(())
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            projection_profile_value(self.profile),
            string_map_value(&self.field_aliases),
            string_map_value(&self.options),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut fields = Fields::array(value, "ticket projection profile config")?;
        let profile = projection_profile_from_value(fields.next("profile")?)?;
        let field_aliases = string_map(fields.next("field_aliases")?, "field_aliases")?;
        let options = string_map(fields.next("options")?, "options")?;
        fields.end("ticket projection profile config")?;
        Self::new(profile, field_aliases, options)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketProjectionProjectConfig {
    pub default_display_projection: TicketProjectionProfile,
    pub enabled_projections: BTreeSet<TicketProjectionProfile>,
    pub profile_configs: BTreeMap<TicketProjectionProfile, TicketProjectionProfileConfig>,
}

impl Default for TicketProjectionProjectConfig {
    fn default() -> Self {
        Self {
            default_display_projection: TicketProjectionProfile::Native,
            enabled_projections: builtin_ticket_projection_profiles().into_iter().collect(),
            profile_configs: builtin_ticket_projection_profile_configs(),
        }
    }
}

impl TicketProjectionProjectConfig {
    pub fn new(
        default_display_projection: TicketProjectionProfile,
        enabled_projections: BTreeSet<TicketProjectionProfile>,
        profile_configs: BTreeMap<TicketProjectionProfile, TicketProjectionProfileConfig>,
    ) -> Result<Self> {
        let config = Self {
            default_display_projection,
            enabled_projections,
            profile_configs,
        };
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        if !self
            .enabled_projections
            .contains(&TicketProjectionProfile::Native)
        {
            return Err(LoomError::invalid(
                "native ticket projection must remain enabled",
            ));
        }
        if !self
            .enabled_projections
            .contains(&self.default_display_projection)
        {
            return Err(LoomError::invalid(
                "default display projection must be enabled",
            ));
        }
        for (profile, config) in &self.profile_configs {
            if profile != &config.profile {
                return Err(LoomError::invalid(
                    "ticket projection config profile key mismatch",
                ));
            }
            if !self.enabled_projections.contains(profile) {
                return Err(LoomError::invalid(
                    "ticket projection config must target an enabled projection",
                ));
            }
            config.validate()?;
        }
        Ok(())
    }

    pub fn select(
        &self,
        context: TicketProjectionRequestContext,
        requested_projection: Option<TicketProjectionProfile>,
    ) -> Result<TicketProjectionSelection> {
        let (profile, selection_source) = match requested_projection {
            Some(profile) => (profile, TicketProjectionSelectionSource::ExplicitRequest),
            None => match context {
                TicketProjectionRequestContext::HumanDisplay => (
                    self.default_display_projection,
                    TicketProjectionSelectionSource::ProjectDefaultDisplay,
                ),
                TicketProjectionRequestContext::MachineApi => (
                    TicketProjectionProfile::Native,
                    TicketProjectionSelectionSource::MachineDefaultNative,
                ),
            },
        };
        if !self.enabled_projections.contains(&profile) {
            return Err(LoomError::invalid("ticket projection is not enabled"));
        }
        Ok(TicketProjectionSelection {
            profile,
            contract: ticket_projection_contract(profile),
            profile_config: self.profile_configs.get(&profile).cloned(),
            selection_source,
        })
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            projection_profile_value(self.default_display_projection),
            Value::Array(
                self.enabled_projections
                    .iter()
                    .copied()
                    .map(projection_profile_value)
                    .collect(),
            ),
            Value::Array(
                self.profile_configs
                    .iter()
                    .map(|(profile, config)| {
                        Value::Array(vec![projection_profile_value(*profile), config.to_value()])
                    })
                    .collect(),
            ),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut fields = Fields::array(value, "ticket projection project config")?;
        let default_display_projection =
            projection_profile_from_value(fields.next("default_display_projection")?)?;
        let enabled_projections =
            projection_profile_set(fields.next("enabled_projections")?, "enabled_projections")?;
        let profile_configs =
            projection_profile_config_map(fields.next("profile_configs")?, "profile_configs")?;
        fields.end("ticket projection project config")?;
        Self::new(
            default_display_projection,
            enabled_projections,
            profile_configs,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketProjectionSelection {
    pub profile: TicketProjectionProfile,
    pub contract: TicketProjectionContract,
    pub profile_config: Option<TicketProjectionProfileConfig>,
    pub selection_source: TicketProjectionSelectionSource,
}

pub fn builtin_ticket_projection_profiles() -> [TicketProjectionProfile; 5] {
    [
        TicketProjectionProfile::Native,
        TicketProjectionProfile::Jira,
        TicketProjectionProfile::Asana,
        TicketProjectionProfile::Notion,
        TicketProjectionProfile::Redmine,
    ]
}

fn builtin_ticket_projection_profile_configs()
-> BTreeMap<TicketProjectionProfile, TicketProjectionProfileConfig> {
    [
        (
            TicketProjectionProfile::Jira,
            [("title", "fields.summary")].as_slice(),
        ),
        (
            TicketProjectionProfile::Asana,
            [("title", "name")].as_slice(),
        ),
        (
            TicketProjectionProfile::Notion,
            [("title", "properties.Name.title")].as_slice(),
        ),
        (
            TicketProjectionProfile::Redmine,
            [("title", "subject")].as_slice(),
        ),
    ]
    .into_iter()
    .map(|(profile, aliases)| {
        (
            profile,
            TicketProjectionProfileConfig {
                profile,
                field_aliases: aliases
                    .iter()
                    .map(|(field_id, alias)| (field_id.to_string(), alias.to_string()))
                    .collect(),
                options: BTreeMap::new(),
            },
        )
    })
    .collect()
}

fn projection_profile_value(profile: TicketProjectionProfile) -> Value {
    Value::Text(profile.profile_id().to_string())
}

fn projection_profile_from_value(value: Value) -> Result<TicketProjectionProfile> {
    match value {
        Value::Text(profile_id) => TicketProjectionProfile::parse(&profile_id),
        _ => Err(LoomError::invalid("ticket projection profile must be text")),
    }
}

fn projection_profile_set(value: Value, label: &str) -> Result<BTreeSet<TicketProjectionProfile>> {
    match value {
        Value::Array(values) => values
            .into_iter()
            .map(projection_profile_from_value)
            .collect::<Result<BTreeSet<_>>>(),
        _ => Err(LoomError::invalid(format!("{label} must be an array"))),
    }
}

fn projection_profile_config_map(
    value: Value,
    label: &str,
) -> Result<BTreeMap<TicketProjectionProfile, TicketProjectionProfileConfig>> {
    let values = match value {
        Value::Array(values) => values,
        _ => return Err(LoomError::invalid(format!("{label} must be an array"))),
    };
    let mut configs = BTreeMap::new();
    for value in values {
        let mut fields = Fields::array(value, label)?;
        let profile = projection_profile_from_value(fields.next("profile")?)?;
        let config = TicketProjectionProfileConfig::from_value(fields.next("config")?)?;
        fields.end(label)?;
        if profile != config.profile {
            return Err(LoomError::invalid(
                "ticket projection config profile key mismatch",
            ));
        }
        if configs.insert(profile, config).is_some() {
            return Err(LoomError::invalid(
                "ticket projection config profiles must be unique",
            ));
        }
    }
    Ok(configs)
}

pub type TicketFieldValue = FieldValue;

#[derive(Debug, Clone, PartialEq)]
pub struct Ticket {
    pub ticket_id: String,
    pub project_id: String,
    pub ticket_number: u64,
    pub ticket_type: TicketType,
    pub external_identity: Option<ExternalTicketIdentity>,
    pub fields: BTreeMap<String, TicketFieldValue>,
    pub relations: BTreeMap<String, TicketRelation>,
    pub policy_labels: BTreeSet<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TicketCoreFields {
    pub title: Option<String>,
    pub description: Option<String>,
    pub status: Option<String>,
    pub status_category: Option<String>,
    pub assignee: Option<String>,
    /// additive display alias for `assignee`. Holds the resolved handle for the canonical
    /// principal id in `assignee`; falls back to the id string when no handle is registered. `None`
    /// when `assignee` is `None`. `assignee` remains the canonical source of truth.
    pub assignee_display: Option<String>,
    pub reporter: Option<String>,
    pub priority: Option<String>,
    pub resolution: Option<String>,
    pub labels: Vec<String>,
    pub start_date: Option<String>,
    pub due_date: Option<String>,
    pub original_estimate_ms: Option<i64>,
    pub remaining_estimate_ms: Option<i64>,
    pub time_spent_ms: Option<i64>,
    pub story_points: Option<f64>,
    pub security_level: Option<String>,
    pub policy_labels: Vec<String>,
}

impl TicketCoreFields {
    pub fn from_ticket(ticket: &Ticket) -> Self {
        Self {
            title: text_like_field(&ticket.fields, "title")
                .or_else(|| text_like_field(&ticket.fields, "summary")),
            description: text_like_field(&ticket.fields, "description"),
            status: normalized_status_field(&ticket.fields),
            status_category: text_like_field(&ticket.fields, "status_category"),
            assignee: text_like_field(&ticket.fields, "assignee"),
            assignee_display: None,
            reporter: text_like_field(&ticket.fields, "reporter"),
            priority: text_like_field(&ticket.fields, "priority"),
            resolution: text_like_field(&ticket.fields, "resolution"),
            labels: list_text_like_field(&ticket.fields, "labels"),
            start_date: text_like_field(&ticket.fields, "start_date"),
            due_date: text_like_field(&ticket.fields, "due_date"),
            original_estimate_ms: duration_like_field(&ticket.fields, "original_estimate"),
            remaining_estimate_ms: duration_like_field(&ticket.fields, "remaining_estimate"),
            time_spent_ms: duration_like_field(&ticket.fields, "time_spent"),
            story_points: number_like_field(&ticket.fields, "story_points"),
            security_level: text_like_field(&ticket.fields, "security_level"),
            policy_labels: ticket.policy_labels.iter().cloned().collect(),
        }
    }

    /// populate `assignee_display` from the canonical `assignee` using the shared display
    /// resolver. Leaves `assignee` (the canonical id) untouched. When `assignee` is `None` the
    /// display stays `None`; when no handle is registered the display falls back to the id string.
    pub fn resolve_displays(&mut self, identity: Option<&loom_core::IdentityStore>) {
        self.assignee_display = self
            .assignee
            .as_deref()
            .map(|id| crate::resolve_principal_display(identity, id));
    }

    /// Build a core-field projection and resolve its display aliases in one step.
    pub fn from_ticket_with_identity(
        ticket: &Ticket,
        identity: Option<&loom_core::IdentityStore>,
    ) -> Self {
        let mut core = Self::from_ticket(ticket);
        core.resolve_displays(identity);
        core
    }
}

pub const NORMALIZED_TICKET_STATUSES: [&str; 10] = [
    "backlog",
    "planned",
    "ready",
    "in_progress",
    "blocked",
    "waiting_for_review",
    "feedback_available",
    "accepted",
    "rejected",
    "closed",
];

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TicketComment {
    pub comment_id: String,
    pub comment_type: String,
    pub author_principal: String,
    pub body: String,
    pub content_type: String,
    pub evidence: Option<TicketCommentEvidence>,
    pub created_at_ms: u64,
    pub updated_at_ms: Option<u64>,
    pub redacted: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TicketCommentEvidence {
    pub entries: BTreeMap<TicketAcceptanceEvidenceKey, Vec<String>>,
}

impl serde::Serialize for TicketCommentEvidence {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(self.entries.len()))?;
        for (key, values) in &self.entries {
            map.serialize_entry(key.as_str(), values)?;
        }
        map.end()
    }
}

impl TicketCommentEvidence {
    pub fn from_json(value: &serde_json::Value) -> Result<Self> {
        let serde_json::Value::Object(map) = value else {
            return Err(LoomError::invalid(
                "ticket comment evidence must be an object",
            ));
        };
        let mut entries = BTreeMap::new();
        for (key, value) in map {
            let key = TicketAcceptanceEvidenceKey::parse(key)?;
            let values = match value {
                serde_json::Value::String(value) => vec![value.clone()],
                serde_json::Value::Array(values) => values
                    .iter()
                    .map(|value| match value {
                        serde_json::Value::String(value) => Ok(value.clone()),
                        _ => Err(LoomError::invalid(
                            "ticket comment evidence arrays must contain strings",
                        )),
                    })
                    .collect::<Result<Vec<_>>>()?,
                serde_json::Value::Null => Vec::new(),
                _ => {
                    return Err(LoomError::invalid(
                        "ticket comment evidence values must be strings or string arrays",
                    ));
                }
            };
            entries.insert(key, values);
        }
        let evidence = Self { entries };
        evidence.validate()?;
        Ok(evidence)
    }

    pub fn validate(&self) -> Result<()> {
        for values in self.entries.values() {
            for value in values {
                validate_text("ticket comment evidence value", value)?;
            }
        }
        Ok(())
    }

    pub fn has_key_value(&self, key: TicketAcceptanceEvidenceKey) -> bool {
        self.entries
            .get(&key)
            .is_some_and(|values| values.iter().any(|value| !value.trim().is_empty()))
    }

    fn to_value(&self) -> Value {
        Value::Array(
            self.entries
                .iter()
                .map(|(key, values)| {
                    Value::Array(vec![
                        Value::Text(key.as_str().to_string()),
                        Value::Array(
                            values
                                .iter()
                                .map(|value| Value::Text(value.clone()))
                                .collect(),
                        ),
                    ])
                })
                .collect(),
        )
    }

    fn from_value(value: Value) -> Result<Self> {
        let Value::Array(items) = value else {
            return Err(LoomError::corrupt(
                "ticket comment evidence must be an array",
            ));
        };
        let mut entries = BTreeMap::new();
        for item in items {
            let mut fields = Fields::array(item, "ticket comment evidence entry")?;
            let key = TicketAcceptanceEvidenceKey::parse(&fields.text("key")?)?;
            let values = match fields.next("values")? {
                Value::Array(values) => values
                    .into_iter()
                    .map(|value| match value {
                        Value::Text(value) => Ok(value),
                        _ => Err(LoomError::corrupt(
                            "ticket comment evidence value must be text",
                        )),
                    })
                    .collect::<Result<Vec<_>>>()?,
                _ => {
                    return Err(LoomError::corrupt(
                        "ticket comment evidence values must be an array",
                    ));
                }
            };
            fields.end("ticket comment evidence entry")?;
            entries.insert(key, values);
        }
        let evidence = Self { entries };
        evidence.validate()?;
        Ok(evidence)
    }
}

impl TicketComment {
    pub fn new(
        comment_id: impl Into<String>,
        author_principal: impl Into<String>,
        body: impl Into<String>,
        created_at_ms: u64,
    ) -> Result<Self> {
        let comment = Self {
            comment_id: comment_id.into(),
            comment_type: TICKET_DEFAULT_COMMENT_TYPE.to_string(),
            author_principal: author_principal.into(),
            body: body.into(),
            content_type: TICKET_DEFAULT_BODY_CONTENT_TYPE.to_string(),
            evidence: None,
            created_at_ms,
            updated_at_ms: None,
            redacted: false,
        };
        comment.validate()?;
        Ok(comment)
    }

    pub fn validate(&self) -> Result<()> {
        validate_text("ticket comment_id", &self.comment_id)?;
        validate_ticket_comment_type(&self.comment_type)?;
        validate_text("ticket comment author", &self.author_principal)?;
        validate_text("ticket comment content_type", &self.content_type)?;
        if let Some(evidence) = &self.evidence {
            evidence.validate()?;
        }
        if !self.redacted {
            validate_ticket_rich_text("ticket comment body", &self.body)?;
        }
        if self
            .updated_at_ms
            .is_some_and(|updated| updated < self.created_at_ms)
        {
            return Err(LoomError::invalid(
                "ticket comment updated_at_ms must not be earlier than created_at_ms",
            ));
        }
        Ok(())
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(self.comment_id.clone()),
            Value::Text(self.comment_type.clone()),
            Value::Text(self.author_principal.clone()),
            Value::Text(self.body.clone()),
            Value::Text(self.content_type.clone()),
            self.evidence
                .as_ref()
                .map(TicketCommentEvidence::to_value)
                .unwrap_or(Value::Null),
            Value::Uint(self.created_at_ms),
            optional_u64_value(self.updated_at_ms),
            Value::Bool(self.redacted),
        ])
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    fn from_value(value: Value) -> Result<Self> {
        let Value::Array(values) = value else {
            return Err(LoomError::corrupt("ticket comment must be an array"));
        };
        let mut fields = Fields::array(Value::Array(values.clone()), "ticket comment")?;
        let comment_id = fields.text("comment_id")?;
        let (
            comment_type,
            author_principal,
            body,
            content_type,
            evidence,
            created_at_ms,
            updated_at_ms,
            redacted,
        ) = if values.len() == 6 {
            let author_principal = fields.text("author_principal")?;
            let body = fields.text("comment body")?;
            let created_at_ms = fields.uint("created_at_ms")?;
            let updated_at_ms = read_optional_u64_field(&mut fields, "updated_at_ms")?;
            let redacted = read_bool_field(&mut fields, "redacted")?;
            (
                TICKET_DEFAULT_COMMENT_TYPE.to_string(),
                author_principal,
                body,
                TICKET_DEFAULT_BODY_CONTENT_TYPE.to_string(),
                None,
                created_at_ms,
                updated_at_ms,
                redacted,
            )
        } else if values.len() == 7 {
            let author_principal = fields.text("author_principal")?;
            let body = fields.text("comment body")?;
            let content_type = fields.text("content_type")?;
            let created_at_ms = fields.uint("created_at_ms")?;
            let updated_at_ms = read_optional_u64_field(&mut fields, "updated_at_ms")?;
            let redacted = read_bool_field(&mut fields, "redacted")?;
            (
                TICKET_DEFAULT_COMMENT_TYPE.to_string(),
                author_principal,
                body,
                content_type,
                None,
                created_at_ms,
                updated_at_ms,
                redacted,
            )
        } else {
            let comment_type = fields.text("comment_type")?;
            let author_principal = fields.text("author_principal")?;
            let body = fields.text("comment body")?;
            let content_type = fields.text("content_type")?;
            let evidence = if values.len() >= 9 {
                match fields.next("evidence")? {
                    Value::Null => None,
                    value => Some(TicketCommentEvidence::from_value(value)?),
                }
            } else {
                None
            };
            let created_at_ms = fields.uint("created_at_ms")?;
            let updated_at_ms = read_optional_u64_field(&mut fields, "updated_at_ms")?;
            let redacted = read_bool_field(&mut fields, "redacted")?;
            (
                comment_type,
                author_principal,
                body,
                content_type,
                evidence,
                created_at_ms,
                updated_at_ms,
                redacted,
            )
        };
        fields.end("ticket comment")?;
        let comment = Self {
            comment_id,
            comment_type,
            author_principal,
            body,
            content_type,
            evidence,
            created_at_ms,
            updated_at_ms,
            redacted,
        };
        comment.validate()?;
        Ok(comment)
    }
}

pub fn validate_ticket_comment_type(value: &str) -> Result<()> {
    validate_text("ticket comment_type", value)?;
    if TICKET_COMMENT_TYPES.contains(&value) {
        Ok(())
    } else {
        Err(LoomError::invalid(format!(
            "ticket comment_type must be one of {}",
            TICKET_COMMENT_TYPES.join(", ")
        )))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketAttachment {
    pub attachment_id: String,
    pub digest: Digest,
    pub name: String,
    pub media_type: String,
    pub size: u64,
    pub uploaded_by: String,
    pub created_at_ms: u64,
    pub shared: bool,
}

impl TicketAttachment {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        attachment_id: impl Into<String>,
        digest: Digest,
        name: impl Into<String>,
        media_type: impl Into<String>,
        size: u64,
        uploaded_by: impl Into<String>,
        created_at_ms: u64,
        shared: bool,
    ) -> Result<Self> {
        let attachment = Self {
            attachment_id: attachment_id.into(),
            digest,
            name: name.into(),
            media_type: media_type.into(),
            size,
            uploaded_by: uploaded_by.into(),
            created_at_ms,
            shared,
        };
        attachment.validate()?;
        Ok(attachment)
    }

    pub fn validate(&self) -> Result<()> {
        validate_text("ticket attachment_id", &self.attachment_id)?;
        validate_text("ticket attachment name", &self.name)?;
        validate_text("ticket attachment media_type", &self.media_type)?;
        validate_text("ticket attachment uploaded_by", &self.uploaded_by)
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(self.attachment_id.clone()),
            digest_value(self.digest),
            Value::Text(self.name.clone()),
            Value::Text(self.media_type.clone()),
            Value::Uint(self.size),
            Value::Text(self.uploaded_by.clone()),
            Value::Uint(self.created_at_ms),
            Value::Bool(self.shared),
        ])
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut fields = Fields::array(value, "ticket attachment")?;
        let attachment = Self {
            attachment_id: fields.text("attachment_id")?,
            digest: Digest::parse(&fields.text("attachment digest")?)
                .map_err(|_| LoomError::corrupt("ticket attachment digest is invalid"))?,
            name: fields.text("attachment name")?,
            media_type: fields.text("attachment media_type")?,
            size: fields.uint("attachment size")?,
            uploaded_by: fields.text("attachment uploaded_by")?,
            created_at_ms: fields.uint("attachment created_at_ms")?,
            shared: read_bool_field(&mut fields, "attachment shared")?,
        };
        fields.end("ticket attachment")?;
        attachment.validate()?;
        Ok(attachment)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TicketRelationKind {
    DependsOn,
    Blocks,
    ParentOf,
    ChildOf,
    RelatesTo,
    Duplicates,
    Supersedes,
    ReferencesPage,
    ReferencesDocument,
    HasPrompt,
    HasResult,
    HasDecision,
    AssignedTo,
}

impl TicketRelationKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DependsOn => "depends_on",
            Self::Blocks => "blocks",
            Self::ParentOf => "parent_of",
            Self::ChildOf => "child_of",
            Self::RelatesTo => "relates_to",
            Self::Duplicates => "duplicates",
            Self::Supersedes => "supersedes",
            Self::ReferencesPage => "references_page",
            Self::ReferencesDocument => "references_document",
            Self::HasPrompt => "has_prompt",
            Self::HasResult => "has_result",
            Self::HasDecision => "has_decision",
            Self::AssignedTo => "assigned_to",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "depends_on" => Ok(Self::DependsOn),
            "blocks" => Ok(Self::Blocks),
            "parent_of" => Ok(Self::ParentOf),
            "child_of" => Ok(Self::ChildOf),
            "relates_to" => Ok(Self::RelatesTo),
            "duplicates" => Ok(Self::Duplicates),
            "supersedes" => Ok(Self::Supersedes),
            "references_page" => Ok(Self::ReferencesPage),
            "references_document" => Ok(Self::ReferencesDocument),
            "has_prompt" => Ok(Self::HasPrompt),
            "has_result" => Ok(Self::HasResult),
            "has_decision" => Ok(Self::HasDecision),
            "assigned_to" => Ok(Self::AssignedTo),
            _ => Err(LoomError::invalid("unsupported ticket relation kind")),
        }
    }

    pub const fn target_type(self) -> TicketRelationTargetType {
        match self {
            Self::DependsOn
            | Self::Blocks
            | Self::ParentOf
            | Self::ChildOf
            | Self::RelatesTo
            | Self::Duplicates
            | Self::Supersedes => TicketRelationTargetType::Ticket,
            Self::ReferencesPage => TicketRelationTargetType::Page,
            Self::ReferencesDocument => TicketRelationTargetType::Document,
            Self::HasPrompt => TicketRelationTargetType::Prompt,
            Self::HasResult => TicketRelationTargetType::Result,
            Self::HasDecision => TicketRelationTargetType::Decision,
            Self::AssignedTo => TicketRelationTargetType::Principal,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TicketRelationTargetType {
    Ticket,
    Page,
    Document,
    Prompt,
    Result,
    Decision,
    Principal,
}

impl TicketRelationTargetType {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ticket => "ticket",
            Self::Page => "page",
            Self::Document => "document",
            Self::Prompt => "prompt",
            Self::Result => "result",
            Self::Decision => "decision",
            Self::Principal => "principal",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "ticket" => Ok(Self::Ticket),
            "page" => Ok(Self::Page),
            "document" => Ok(Self::Document),
            "prompt" => Ok(Self::Prompt),
            "result" => Ok(Self::Result),
            "decision" => Ok(Self::Decision),
            "principal" => Ok(Self::Principal),
            _ => Err(LoomError::invalid(
                "unsupported ticket relation target type",
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketRelation {
    pub relation_id: String,
    pub kind: TicketRelationKind,
    pub target_type: TicketRelationTargetType,
    pub target_id: String,
}

impl TicketRelation {
    pub fn new(
        relation_id: impl Into<String>,
        kind: TicketRelationKind,
        target_type: TicketRelationTargetType,
        target_id: impl Into<String>,
    ) -> Result<Self> {
        let relation = Self {
            relation_id: relation_id.into(),
            kind,
            target_type,
            target_id: target_id.into(),
        };
        relation.validate()?;
        Ok(relation)
    }

    pub fn validate(&self) -> Result<()> {
        validate_text("ticket relation_id", &self.relation_id)?;
        validate_text("ticket relation target_id", &self.target_id)?;
        if self.kind.target_type() != self.target_type {
            return Err(LoomError::invalid(
                "ticket relation target type does not match relation kind",
            ));
        }
        Ok(())
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(self.relation_id.clone()),
            Value::Text(self.kind.as_str().to_string()),
            Value::Text(self.target_type.as_str().to_string()),
            Value::Text(self.target_id.clone()),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut fields = Fields::array(value, "ticket relation")?;
        let relation_id = fields.text("relation_id")?;
        let kind = TicketRelationKind::parse(&fields.text("relation kind")?)?;
        let target_type = TicketRelationTargetType::parse(&fields.text("relation target type")?)?;
        let target_id = fields.text("relation target_id")?;
        fields.end("ticket relation")?;
        Self::new(relation_id, kind, target_type, target_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ExternalTicketIdentity {
    pub source: String,
    pub id: String,
}

impl ExternalTicketIdentity {
    pub fn new(source: impl Into<String>, id: impl Into<String>) -> Result<Self> {
        let identity = Self {
            source: source.into(),
            id: id.into(),
        };
        identity.validate()?;
        Ok(identity)
    }

    pub fn validate(&self) -> Result<()> {
        validate_text("ticket external source", &self.source)?;
        validate_text("ticket external id", &self.id)
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(self.source.clone()),
            Value::Text(self.id.clone()),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut fields = Fields::array(value, "ticket external identity")?;
        let source = fields.text("source")?;
        let id = fields.text("id")?;
        fields.end("ticket external identity")?;
        Self::new(source, id)
    }
}

impl Ticket {
    pub fn new(input: TicketInput<'_>) -> Result<Self> {
        validate_ticket_id(input.ticket_id)?;
        validate_text("project_id", input.project_id)?;
        let ticket = Self {
            ticket_id: input.ticket_id.to_string(),
            project_id: input.project_id.to_string(),
            ticket_number: input.ticket_number,
            ticket_type: input.ticket_type,
            external_identity: input.external_identity,
            fields: input.fields,
            relations: BTreeMap::new(),
            policy_labels: input
                .policy_labels
                .iter()
                .map(|label| (*label).to_string())
                .collect(),
        };
        ticket.validate()?;
        Ok(ticket)
    }

    pub fn validate(&self) -> Result<()> {
        validate_ticket_id(&self.ticket_id)?;
        validate_text("project_id", &self.project_id)?;
        if self.ticket_number == 0 {
            return Err(LoomError::invalid(
                "primary ticket key number must be positive",
            ));
        }
        if let Some(identity) = &self.external_identity {
            identity.validate()?;
        }
        for (field, value) in &self.fields {
            validate_field_key(field)?;
            validate_ticket_field_value(field, value)?;
        }
        validate_relation_cardinality(&self.relations)?;
        for (relation_id, relation) in &self.relations {
            if relation_id != &relation.relation_id {
                return Err(LoomError::invalid(
                    "ticket relation map key must match relation_id",
                ));
            }
            relation.validate()?;
        }
        for label in &self.policy_labels {
            validate_text("ticket policy label", label)?;
        }
        Ok(())
    }

    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(TICKET_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.ticket_id.clone()),
                Value::Text(self.project_id.clone()),
                Value::Uint(self.ticket_number),
                Value::Uint(self.ticket_type.tag()),
                optional_external_identity_value(self.external_identity.as_ref()),
                Value::Map(
                    self.fields
                        .iter()
                        .map(|(key, value)| (Value::Text(key.clone()), value.to_value()))
                        .collect(),
                ),
                Value::Array(
                    self.relations
                        .values()
                        .map(TicketRelation::to_value)
                        .collect(),
                ),
                Value::Array(
                    self.policy_labels
                        .iter()
                        .map(|label| Value::Text(label.clone()))
                        .collect(),
                ),
            ]),
        ])
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "ticket")?;
        outer.expect_text(TICKET_SCHEMA)?;
        let mut fields = Fields::array(outer.next("ticket fields")?, "ticket")?;
        outer.end("ticket")?;
        let ticket_id = fields.text("ticket_id")?;
        let project_id = fields.text("project_id")?;
        let ticket_number = fields.uint("ticket_number")?;
        let ticket_type = TicketType::from_tag(fields.uint("ticket_type")?)?;
        let external_identity = read_optional_external_identity(&mut fields, "external_identity")?;
        let field_values = field_map(fields.next("fields")?)?;
        let relations = relation_map(fields.next("relations")?)?;
        let policy_labels = string_set(fields.next("policy_labels")?, "policy_labels")?;
        fields.end("ticket")?;
        let ticket = Self {
            ticket_id,
            project_id,
            ticket_number,
            ticket_type,
            external_identity,
            fields: field_values,
            relations,
            policy_labels,
        };
        ticket.validate()?;
        Ok(ticket)
    }
}

pub struct TicketInput<'a> {
    pub ticket_id: &'a str,
    pub project_id: &'a str,
    pub ticket_number: u64,
    pub ticket_type: TicketType,
    pub external_identity: Option<ExternalTicketIdentity>,
    pub fields: BTreeMap<String, TicketFieldValue>,
    pub policy_labels: &'a [&'a str],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TicketKeyStatus {
    Active,
    Retired,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketKeyResolution {
    pub ticket_id: String,
    pub requested_key: TicketKey,
    pub current_key: TicketKey,
    pub status: TicketKeyStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScalarConflictClass {
    LastWriteWins,
    Guarded,
    HumanReview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScalarConflictOutcome {
    AppliedNoConflict,
    AppliedNoRecord,
    AppliedWithConflictRecord,
    HeldForHumanReview,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldWriteObservation {
    pub field_id: String,
    pub sequence: u64,
}

pub fn scalar_conflict_outcome(
    field_id: &str,
    class: ScalarConflictClass,
    base_entity_version: Option<u64>,
    applied_writes: &[FieldWriteObservation],
) -> Result<ScalarConflictOutcome> {
    validate_field_key(field_id)?;
    let Some(base_entity_version) = base_entity_version else {
        return Ok(ScalarConflictOutcome::AppliedNoConflict);
    };
    let conflicts = applied_writes
        .iter()
        .any(|write| write.field_id == field_id && write.sequence > base_entity_version);
    if !conflicts {
        return Ok(ScalarConflictOutcome::AppliedNoConflict);
    }
    match class {
        ScalarConflictClass::LastWriteWins => Ok(ScalarConflictOutcome::AppliedNoRecord),
        ScalarConflictClass::Guarded => Ok(ScalarConflictOutcome::AppliedWithConflictRecord),
        ScalarConflictClass::HumanReview => Ok(ScalarConflictOutcome::HeldForHumanReview),
    }
}

pub fn default_scalar_conflict_class(field_id: &str) -> Result<ScalarConflictClass> {
    validate_field_key(field_id)?;
    match field_id {
        "security_level" => Ok(ScalarConflictClass::HumanReview),
        "rank_token" | "status" | "description" => Err(LoomError::invalid(
            "field is not governed by scalar conflict policy",
        )),
        _ => Ok(ScalarConflictClass::Guarded),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowDefinition {
    pub workflow_id: String,
    pub version: String,
    pub states: BTreeSet<String>,
    pub edges: Vec<WorkflowEdge>,
}

impl WorkflowDefinition {
    pub fn new(
        workflow_id: impl Into<String>,
        version: impl Into<String>,
        states: BTreeSet<String>,
        edges: Vec<WorkflowEdge>,
    ) -> Result<Self> {
        let workflow_id = workflow_id.into();
        let version = version.into();
        validate_text("workflow_id", &workflow_id)?;
        validate_text("workflow version", &version)?;
        if states.is_empty() {
            return Err(LoomError::invalid("workflow must have states"));
        }
        for state in &states {
            validate_text("workflow state", state)?;
        }
        for edge in &edges {
            edge.validate()?;
            if !states.contains(&edge.from) || !states.contains(&edge.to) {
                return Err(LoomError::invalid("workflow edge references unknown state"));
            }
        }
        Ok(Self {
            workflow_id,
            version,
            states,
            edges,
        })
    }

    pub fn validate(&self) -> Result<()> {
        validate_text("workflow_id", &self.workflow_id)?;
        validate_text("workflow version", &self.version)?;
        if self.states.is_empty() {
            return Err(LoomError::invalid("workflow must have states"));
        }
        for state in &self.states {
            validate_text("workflow state", state)?;
        }
        for edge in &self.edges {
            edge.validate()?;
            if !self.states.contains(&edge.from) || !self.states.contains(&edge.to) {
                return Err(LoomError::invalid("workflow edge references unknown state"));
            }
        }
        Ok(())
    }

    fn edge(&self, from: &str, to: &str) -> Option<&WorkflowEdge> {
        self.edges
            .iter()
            .find(|edge| edge.from == from && edge.to == to)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowEdge {
    pub edge_id: String,
    pub from: String,
    pub to: String,
    pub guards: Vec<WorkflowGuard>,
}

impl WorkflowEdge {
    pub fn new(
        edge_id: impl Into<String>,
        from: impl Into<String>,
        to: impl Into<String>,
        guards: Vec<WorkflowGuard>,
    ) -> Result<Self> {
        let edge = Self {
            edge_id: edge_id.into(),
            from: from.into(),
            to: to.into(),
            guards,
        };
        edge.validate()?;
        Ok(edge)
    }

    fn validate(&self) -> Result<()> {
        validate_text("workflow edge_id", &self.edge_id)?;
        validate_text("workflow edge from", &self.from)?;
        validate_text("workflow edge to", &self.to)?;
        for guard in &self.guards {
            guard.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkflowGuard {
    RequiredFields(Vec<String>),
    PermissionRole(String),
    PrincipalSet(BTreeSet<String>),
    LinkedTicketState {
        edge_kind: String,
        all_in: BTreeSet<String>,
    },
    ChecklistGate(String),
    ResolutionRequired,
    Predicate(String),
}

impl WorkflowGuard {
    fn validate(&self) -> Result<()> {
        match self {
            Self::RequiredFields(fields) => {
                if fields.is_empty() {
                    return Err(LoomError::invalid("required_fields guard must name fields"));
                }
                for field in fields {
                    validate_field_key(field)?;
                }
                Ok(())
            }
            Self::PermissionRole(role) => validate_text("workflow permission role", role),
            Self::PrincipalSet(principals) => {
                if principals.is_empty() {
                    return Err(LoomError::invalid(
                        "principal-set guard must name principals",
                    ));
                }
                for principal in principals {
                    validate_text("workflow principal", principal)?;
                }
                Ok(())
            }
            Self::LinkedTicketState { edge_kind, all_in } => {
                validate_text("workflow linked edge kind", edge_kind)?;
                if all_in.is_empty() {
                    return Err(LoomError::invalid(
                        "linked_ticket_state guard must name allowed statuses",
                    ));
                }
                for status in all_in {
                    validate_text("workflow linked status", status)?;
                }
                Ok(())
            }
            Self::ChecklistGate(gate_id) => validate_text("workflow checklist gate", gate_id),
            Self::ResolutionRequired => Ok(()),
            Self::Predicate(predicate_id) => validate_text("workflow predicate", predicate_id),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TransitionOperation {
    pub operation_id: String,
    pub actor_principal: String,
    pub target_status: String,
    pub observed_source_status: String,
    pub observed_workflow_version: String,
    pub attached_fields: BTreeMap<String, TicketFieldValue>,
}

impl TransitionOperation {
    pub fn validate(&self) -> Result<()> {
        validate_text("transition operation_id", &self.operation_id)?;
        validate_text("transition actor principal", &self.actor_principal)?;
        validate_text("transition target status", &self.target_status)?;
        validate_text(
            "transition observed source status",
            &self.observed_source_status,
        )?;
        validate_text(
            "transition observed workflow version",
            &self.observed_workflow_version,
        )?;
        for (field, value) in &self.attached_fields {
            validate_field_key(field)?;
            value.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkflowValidationContext {
    pub actor_roles: BTreeSet<String>,
    pub actor_principals: BTreeSet<String>,
    pub linked_ticket_statuses: BTreeMap<String, Vec<String>>,
    pub attested_gates: BTreeSet<String>,
    pub predicate_results: BTreeMap<String, bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowValidationRecord {
    pub operation_id: String,
    pub validation_state: WorkflowValidationState,
    pub rule: Option<WorkflowRejectionRule>,
    pub validated_against_status: String,
    pub validated_against_workflow_version: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowValidationState {
    Applied,
    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TicketLifecycleAction {
    Assign,
    Claim,
    Release,
    RequestReview,
    Accept,
    Reject,
    Block,
    Complete,
}

impl TicketLifecycleAction {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "assign" => Ok(Self::Assign),
            "claim" => Ok(Self::Claim),
            "release" => Ok(Self::Release),
            "request_review" => Ok(Self::RequestReview),
            "accept" => Ok(Self::Accept),
            "reject" => Ok(Self::Reject),
            "block" => Ok(Self::Block),
            "complete" => Ok(Self::Complete),
            _ => Err(LoomError::invalid("unknown ticket lifecycle action")),
        }
    }

    pub const fn target_status(self) -> Option<&'static str> {
        match self {
            Self::Assign => None,
            Self::Claim => Some("in_progress"),
            Self::Release => None,
            Self::RequestReview => Some("waiting_for_review"),
            Self::Accept => Some("accepted"),
            Self::Reject => Some("rejected"),
            Self::Block => Some("blocked"),
            Self::Complete => Some("waiting_for_review"),
        }
    }

    pub fn allows_transition(self, current_status: &str) -> bool {
        match self {
            Self::Assign => matches!(
                current_status,
                "backlog" | "planned" | "ready" | "blocked" | "rejected"
            ),
            Self::Claim => matches!(
                current_status,
                "backlog" | "planned" | "ready" | "blocked" | "rejected" | "accepted"
            ),
            Self::Release => matches!(
                current_status,
                "in_progress" | "blocked" | "waiting_for_review"
            ),
            Self::RequestReview => matches!(current_status, "in_progress" | "blocked"),
            Self::Accept | Self::Reject => current_status == "waiting_for_review",
            Self::Complete => matches!(current_status, "in_progress" | "blocked"),
            Self::Block => matches!(
                current_status,
                "ready" | "in_progress" | "waiting_for_review"
            ),
        }
    }

    pub const fn operation_kind(self) -> &'static str {
        match self {
            Self::Assign => "ticket.assigned",
            Self::Claim => "ticket.claimed",
            Self::Release => "ticket.released",
            Self::RequestReview => "ticket.review_requested",
            Self::Accept => "ticket.accepted",
            Self::Reject => "ticket.rejected",
            Self::Block => "ticket.blocked",
            Self::Complete => "ticket.completed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkflowRejectionRule {
    EdgeMissing,
    GuardFailed { guard_id: String },
    FieldMissing { field_ids: Vec<String> },
    Permission,
    WorkflowVersionGone,
}

impl WorkflowValidationRecord {
    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(self.operation_id.clone()),
            Value::Uint(match self.validation_state {
                WorkflowValidationState::Applied => 0,
                WorkflowValidationState::Rejected => 1,
            }),
            optional_rejection_rule_value(self.rule.as_ref()),
            Value::Text(self.validated_against_status.clone()),
            optional_text_value(self.validated_against_workflow_version.as_deref()),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut fields = Fields::array(value, "workflow validation record")?;
        let operation_id = fields.text("operation_id")?;
        let validation_state = match fields.uint("validation_state")? {
            0 => WorkflowValidationState::Applied,
            1 => WorkflowValidationState::Rejected,
            other => {
                return Err(LoomError::corrupt(format!(
                    "unknown workflow validation state tag {other}"
                )));
            }
        };
        let rule = read_optional_rejection_rule(&mut fields, "rejection_rule")?;
        let validated_against_status = fields.text("validated_against_status")?;
        let validated_against_workflow_version =
            read_optional_text_field(&mut fields, "validated_against_workflow_version")?;
        fields.end("workflow validation record")?;
        validate_text("validation operation_id", &operation_id)?;
        validate_text("validated_against_status", &validated_against_status)?;
        if let Some(version) = &validated_against_workflow_version {
            validate_text("validated_against_workflow_version", version)?;
        }
        if validation_state == WorkflowValidationState::Applied && rule.is_some() {
            return Err(LoomError::corrupt(
                "applied workflow validation must not carry rejection rule",
            ));
        }
        if validation_state == WorkflowValidationState::Rejected && rule.is_none() {
            return Err(LoomError::corrupt(
                "rejected workflow validation must carry rejection rule",
            ));
        }
        Ok(Self {
            operation_id,
            validation_state,
            rule,
            validated_against_status,
            validated_against_workflow_version,
        })
    }
}

pub fn validate_transition(
    workflow: Option<&WorkflowDefinition>,
    current_status: &str,
    ticket_fields: &BTreeMap<String, TicketFieldValue>,
    operation: &TransitionOperation,
    context: &WorkflowValidationContext,
) -> Result<WorkflowValidationRecord> {
    validate_text("current status", current_status)?;
    operation.validate()?;
    let Some(workflow) = workflow else {
        return Ok(rejected_transition(
            operation,
            current_status,
            None,
            WorkflowRejectionRule::WorkflowVersionGone,
        ));
    };
    let Some(edge) = workflow.edge(current_status, &operation.target_status) else {
        return Ok(rejected_transition(
            operation,
            current_status,
            Some(&workflow.version),
            WorkflowRejectionRule::EdgeMissing,
        ));
    };
    for guard in &edge.guards {
        if let Some(rule) = evaluate_guard(guard, ticket_fields, operation, context) {
            return Ok(rejected_transition(
                operation,
                current_status,
                Some(&workflow.version),
                rule,
            ));
        }
    }
    Ok(WorkflowValidationRecord {
        operation_id: operation.operation_id.clone(),
        validation_state: WorkflowValidationState::Applied,
        rule: None,
        validated_against_status: current_status.to_string(),
        validated_against_workflow_version: Some(workflow.version.clone()),
    })
}

fn evaluate_guard(
    guard: &WorkflowGuard,
    ticket_fields: &BTreeMap<String, TicketFieldValue>,
    operation: &TransitionOperation,
    context: &WorkflowValidationContext,
) -> Option<WorkflowRejectionRule> {
    match guard {
        WorkflowGuard::RequiredFields(fields) => {
            let missing = fields
                .iter()
                .filter(|field| {
                    !field_has_value(ticket_fields, field)
                        && !field_has_value(&operation.attached_fields, field)
                })
                .cloned()
                .collect::<Vec<_>>();
            if missing.is_empty() {
                None
            } else {
                Some(WorkflowRejectionRule::FieldMissing { field_ids: missing })
            }
        }
        WorkflowGuard::PermissionRole(role) => {
            if context.actor_roles.contains(role) {
                None
            } else {
                Some(WorkflowRejectionRule::Permission)
            }
        }
        WorkflowGuard::PrincipalSet(principals) => {
            if principals.contains(&operation.actor_principal)
                || context
                    .actor_principals
                    .contains(&operation.actor_principal)
            {
                None
            } else {
                Some(WorkflowRejectionRule::Permission)
            }
        }
        WorkflowGuard::LinkedTicketState { edge_kind, all_in } => {
            let statuses = context
                .linked_ticket_statuses
                .get(edge_kind)
                .cloned()
                .unwrap_or_default();
            if !statuses.is_empty() && statuses.iter().all(|status| all_in.contains(status)) {
                None
            } else {
                Some(WorkflowRejectionRule::GuardFailed {
                    guard_id: format!("linked_ticket_state:{edge_kind}"),
                })
            }
        }
        WorkflowGuard::ChecklistGate(gate_id) => {
            if context.attested_gates.contains(gate_id) {
                None
            } else {
                Some(WorkflowRejectionRule::GuardFailed {
                    guard_id: format!("checklist_gate:{gate_id}"),
                })
            }
        }
        WorkflowGuard::ResolutionRequired => {
            if field_has_value(ticket_fields, "resolution")
                || field_has_value(&operation.attached_fields, "resolution")
            {
                None
            } else {
                Some(WorkflowRejectionRule::GuardFailed {
                    guard_id: "resolution_required".to_string(),
                })
            }
        }
        WorkflowGuard::Predicate(predicate_id) => {
            if context
                .predicate_results
                .get(predicate_id)
                .copied()
                .unwrap_or(false)
            {
                None
            } else {
                Some(WorkflowRejectionRule::GuardFailed {
                    guard_id: format!("predicate:{predicate_id}"),
                })
            }
        }
    }
}

fn rejected_transition(
    operation: &TransitionOperation,
    current_status: &str,
    workflow_version: Option<&str>,
    rule: WorkflowRejectionRule,
) -> WorkflowValidationRecord {
    WorkflowValidationRecord {
        operation_id: operation.operation_id.clone(),
        validation_state: WorkflowValidationState::Rejected,
        rule: Some(rule),
        validated_against_status: current_status.to_string(),
        validated_against_workflow_version: workflow_version.map(str::to_string),
    }
}

fn field_has_value(fields: &BTreeMap<String, TicketFieldValue>, field: &str) -> bool {
    fields
        .get(field)
        .is_some_and(|value| !matches!(value, TicketFieldValue::Null))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoardMode {
    StatusMapped,
    Manual,
}

impl BoardMode {
    pub fn as_str(self) -> &'static str {
        match self {
            BoardMode::StatusMapped => "status_mapped",
            BoardMode::Manual => "manual",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "status_mapped" => Ok(BoardMode::StatusMapped),
            "manual" => Ok(BoardMode::Manual),
            _ => Err(LoomError::invalid("unknown board mode")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoardStatus {
    Active,
    Archived,
    Deleted,
}

impl BoardStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            BoardStatus::Active => "active",
            BoardStatus::Archived => "archived",
            BoardStatus::Deleted => "deleted",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "active" => Ok(BoardStatus::Active),
            "archived" => Ok(BoardStatus::Archived),
            "deleted" => Ok(BoardStatus::Deleted),
            _ => Err(LoomError::invalid("unknown board status")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoardScope {
    Project { project_id: String },
    Filter { filter_id: String },
    ManualSet,
}

impl BoardScope {
    pub fn project(project_id: impl Into<String>) -> Self {
        Self::Project {
            project_id: project_id.into(),
        }
    }

    pub fn validate(&self) -> Result<()> {
        match self {
            BoardScope::Project { project_id } => {
                validate_text("board scope project_id", project_id)
            }
            BoardScope::Filter { filter_id } => validate_text("board scope filter_id", filter_id),
            BoardScope::ManualSet => Ok(()),
        }
    }

    fn to_value(&self) -> Value {
        match self {
            BoardScope::Project { project_id } => Value::Array(vec![
                Value::Text("project".to_string()),
                Value::Text(project_id.clone()),
            ]),
            BoardScope::Filter { filter_id } => Value::Array(vec![
                Value::Text("filter".to_string()),
                Value::Text(filter_id.clone()),
            ]),
            BoardScope::ManualSet => {
                Value::Array(vec![Value::Text("manual_set".to_string()), Value::Null])
            }
        }
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut fields = Fields::array(value, "board scope")?;
        let kind = fields.text("scope kind")?;
        let scope = match kind.as_str() {
            "project" => BoardScope::Project {
                project_id: fields.text("project_id")?,
            },
            "filter" => BoardScope::Filter {
                filter_id: fields.text("filter_id")?,
            },
            "manual_set" => {
                let marker = fields.next("manual_set marker")?;
                if !matches!(marker, Value::Null) {
                    return Err(LoomError::corrupt("manual board scope marker is invalid"));
                }
                BoardScope::ManualSet
            }
            _ => return Err(LoomError::corrupt("unknown board scope kind")),
        };
        fields.end("board scope")?;
        scope.validate()?;
        Ok(scope)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardSwimlane {
    pub swimlane_id: String,
    pub name: String,
    pub predicate: Option<String>,
    pub rank: u64,
}

impl BoardSwimlane {
    pub fn new(
        swimlane_id: impl Into<String>,
        name: impl Into<String>,
        predicate: Option<String>,
        rank: u64,
    ) -> Result<Self> {
        let swimlane = Self {
            swimlane_id: swimlane_id.into(),
            name: name.into(),
            predicate,
            rank,
        };
        swimlane.validate()?;
        Ok(swimlane)
    }

    fn validate(&self) -> Result<()> {
        validate_text("board swimlane_id", &self.swimlane_id)?;
        validate_text("board swimlane name", &self.name)?;
        if let Some(predicate) = &self.predicate {
            validate_text("board swimlane predicate", predicate)?;
        }
        Ok(())
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(self.swimlane_id.clone()),
            Value::Text(self.name.clone()),
            optional_text_value(self.predicate.as_deref()),
            Value::Uint(self.rank),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut fields = Fields::array(value, "board swimlane")?;
        let swimlane_id = fields.text("swimlane_id")?;
        let name = fields.text("name")?;
        let predicate = read_optional_text_field(&mut fields, "predicate")?;
        let rank = fields.uint("rank")?;
        fields.end("board swimlane")?;
        Self::new(swimlane_id, name, predicate, rank)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardCardPlacement {
    pub board_id: String,
    pub ticket_id: String,
    pub column_id: String,
    pub rank_token: String,
    pub swimlane_id: Option<String>,
    pub updated_at: u64,
    pub updated_by: String,
}

impl BoardCardPlacement {
    pub fn new(
        board_id: impl Into<String>,
        ticket_id: impl Into<String>,
        column_id: impl Into<String>,
        rank_token: impl Into<String>,
        swimlane_id: Option<String>,
        updated_at: u64,
        updated_by: impl Into<String>,
    ) -> Result<Self> {
        let placement = Self {
            board_id: board_id.into(),
            ticket_id: ticket_id.into(),
            column_id: column_id.into(),
            rank_token: rank_token.into(),
            swimlane_id,
            updated_at,
            updated_by: updated_by.into(),
        };
        placement.validate()?;
        Ok(placement)
    }

    pub fn validate(&self) -> Result<()> {
        validate_text("board placement board_id", &self.board_id)?;
        validate_text("board placement ticket_id", &self.ticket_id)?;
        validate_text("board placement column_id", &self.column_id)?;
        validate_text("board placement rank_token", &self.rank_token)?;
        if let Some(swimlane_id) = &self.swimlane_id {
            validate_text("board placement swimlane_id", swimlane_id)?;
        }
        validate_text("board placement updated_by", &self.updated_by)?;
        Ok(())
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(self.board_id.clone()),
            Value::Text(self.ticket_id.clone()),
            Value::Text(self.column_id.clone()),
            Value::Text(self.rank_token.clone()),
            optional_text_value(self.swimlane_id.as_deref()),
            Value::Uint(self.updated_at),
            Value::Text(self.updated_by.clone()),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut fields = Fields::array(value, "board card placement")?;
        let board_id = fields.text("board_id")?;
        let ticket_id = fields.text("ticket_id")?;
        let column_id = fields.text("column_id")?;
        let rank_token = fields.text("rank_token")?;
        let swimlane_id = read_optional_text_field(&mut fields, "swimlane_id")?;
        let updated_at = fields.uint("updated_at")?;
        let updated_by = fields.text("updated_by")?;
        fields.end("board card placement")?;
        Self::new(
            board_id,
            ticket_id,
            column_id,
            rank_token,
            swimlane_id,
            updated_at,
            updated_by,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketBoard {
    pub board_id: String,
    pub board_key: String,
    pub name: String,
    pub description: String,
    pub project_id: String,
    pub scope: BoardScope,
    pub mode: BoardMode,
    pub columns: Vec<BoardColumn>,
    pub swimlanes: Vec<BoardSwimlane>,
    pub card_display_fields: Vec<String>,
    pub owner_principal: Option<String>,
    pub coordinator_principal: Option<String>,
    pub board_status: BoardStatus,
    pub updated_at: u64,
    pub updated_by: String,
}

impl TicketBoard {
    pub fn new(
        board_id: impl Into<String>,
        project_id: impl Into<String>,
        mode: BoardMode,
        columns: Vec<BoardColumn>,
    ) -> Result<Self> {
        let board_id = board_id.into();
        let project_id = project_id.into();
        Self::first_class(
            board_id.clone(),
            board_id.clone(),
            board_id,
            String::new(),
            project_id.clone(),
            BoardScope::project(project_id),
            mode,
            columns,
            Vec::new(),
            Vec::new(),
            None,
            None,
            BoardStatus::Active,
            0,
            "system",
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn first_class(
        board_id: impl Into<String>,
        board_key: impl Into<String>,
        name: impl Into<String>,
        description: impl Into<String>,
        project_id: impl Into<String>,
        scope: BoardScope,
        mode: BoardMode,
        columns: Vec<BoardColumn>,
        swimlanes: Vec<BoardSwimlane>,
        card_display_fields: Vec<String>,
        owner_principal: Option<String>,
        coordinator_principal: Option<String>,
        board_status: BoardStatus,
        updated_at: u64,
        updated_by: impl Into<String>,
    ) -> Result<Self> {
        let board = Self {
            board_id: board_id.into(),
            board_key: board_key.into(),
            name: name.into(),
            description: description.into(),
            project_id: project_id.into(),
            scope,
            mode,
            columns,
            swimlanes,
            card_display_fields,
            owner_principal,
            coordinator_principal,
            board_status,
            updated_at,
            updated_by: updated_by.into(),
        };
        board.validate()?;
        Ok(board)
    }

    pub fn validate(&self) -> Result<()> {
        validate_text("board_id", &self.board_id)?;
        validate_text("board_key", &self.board_key)?;
        validate_text("board name", &self.name)?;
        if !self.description.is_empty() {
            validate_text("board description", &self.description)?;
        }
        validate_text("board project_id", &self.project_id)?;
        self.scope.validate()?;
        if self.columns.is_empty() {
            return Err(LoomError::invalid("board must have columns"));
        }
        let mut ids = BTreeSet::new();
        for column in &self.columns {
            column.validate()?;
            if !ids.insert(column.column_id.clone()) {
                return Err(LoomError::invalid("board column ids must be unique"));
            }
        }
        let mut swimlane_ids = BTreeSet::new();
        for swimlane in &self.swimlanes {
            swimlane.validate()?;
            if !swimlane_ids.insert(swimlane.swimlane_id.clone()) {
                return Err(LoomError::invalid("board swimlane ids must be unique"));
            }
        }
        for field in &self.card_display_fields {
            validate_text("board card display field", field)?;
        }
        if let Some(owner) = &self.owner_principal {
            validate_text("board owner_principal", owner)?;
        }
        if let Some(coordinator) = &self.coordinator_principal {
            validate_text("board coordinator_principal", coordinator)?;
        }
        validate_text("board updated_by", &self.updated_by)?;
        Ok(())
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(self.board_id.clone()),
            Value::Text(self.board_key.clone()),
            Value::Text(self.name.clone()),
            Value::Text(self.description.clone()),
            Value::Text(self.project_id.clone()),
            Value::Uint(match self.mode {
                BoardMode::StatusMapped => 0,
                BoardMode::Manual => 1,
            }),
            Value::Array(self.columns.iter().map(BoardColumn::to_value).collect()),
            self.scope.to_value(),
            Value::Array(self.swimlanes.iter().map(BoardSwimlane::to_value).collect()),
            Value::Array(
                self.card_display_fields
                    .iter()
                    .map(|field| Value::Text(field.clone()))
                    .collect(),
            ),
            optional_text_value(self.owner_principal.as_deref()),
            optional_text_value(self.coordinator_principal.as_deref()),
            Value::Text(self.board_status.as_str().to_string()),
            Value::Uint(self.updated_at),
            Value::Text(self.updated_by.clone()),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let Value::Array(values) = value else {
            return Err(LoomError::corrupt("ticket board is not an array"));
        };
        if values.len() == 4 {
            let mut fields = Fields::array(Value::Array(values), "ticket board")?;
            let board_id = fields.text("board_id")?;
            let project_id = fields.text("project_id")?;
            let mode = match fields.uint("board mode")? {
                0 => BoardMode::StatusMapped,
                1 => BoardMode::Manual,
                other => {
                    return Err(LoomError::corrupt(format!(
                        "unknown board mode tag {other}"
                    )));
                }
            };
            let columns = board_column_list(fields.next("board columns")?)?;
            fields.end("ticket board")?;
            return Self::new(board_id, project_id, mode, columns);
        }
        let mut fields = Fields::array(Value::Array(values), "ticket board")?;
        let board_id = fields.text("board_id")?;
        let board_key = fields.text("board_key")?;
        let name = fields.text("name")?;
        let description = fields.text("description")?;
        let project_id = fields.text("project_id")?;
        let mode = match fields.uint("board mode")? {
            0 => BoardMode::StatusMapped,
            1 => BoardMode::Manual,
            other => {
                return Err(LoomError::corrupt(format!(
                    "unknown board mode tag {other}"
                )));
            }
        };
        let columns = board_column_list(fields.next("board columns")?)?;
        let scope = BoardScope::from_value(fields.next("scope")?)?;
        let swimlanes = board_swimlane_list(fields.next("swimlanes")?)?;
        let card_display_fields =
            read_string_list(fields.next("card_display_fields")?, "card_display_fields")?;
        let owner_principal = read_optional_text_field(&mut fields, "owner_principal")?;
        let coordinator_principal = read_optional_text_field(&mut fields, "coordinator_principal")?;
        let board_status = BoardStatus::parse(&fields.text("board_status")?)?;
        let updated_at = fields.uint("updated_at")?;
        let updated_by = fields.text("updated_by")?;
        fields.end("ticket board")?;
        Self::first_class(
            board_id,
            board_key,
            name,
            description,
            project_id,
            scope,
            mode,
            columns,
            swimlanes,
            card_display_fields,
            owner_principal,
            coordinator_principal,
            board_status,
            updated_at,
            updated_by,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardColumn {
    pub column_id: String,
    pub name: String,
    pub mapped_statuses: BTreeSet<String>,
    pub wip_limit: Option<u32>,
    pub hidden: bool,
    pub rank: u64,
}

impl BoardColumn {
    pub fn new(
        column_id: impl Into<String>,
        name: impl Into<String>,
        mapped_statuses: BTreeSet<String>,
        wip_limit: Option<u32>,
    ) -> Result<Self> {
        Self::with_display(column_id, name, mapped_statuses, wip_limit, false, 0)
    }

    pub fn with_display(
        column_id: impl Into<String>,
        name: impl Into<String>,
        mapped_statuses: BTreeSet<String>,
        wip_limit: Option<u32>,
        hidden: bool,
        rank: u64,
    ) -> Result<Self> {
        let column = Self {
            column_id: column_id.into(),
            name: name.into(),
            mapped_statuses,
            wip_limit,
            hidden,
            rank,
        };
        column.validate()?;
        Ok(column)
    }

    fn validate(&self) -> Result<()> {
        validate_text("board column_id", &self.column_id)?;
        validate_text("board column name", &self.name)?;
        for status in &self.mapped_statuses {
            validate_text("board mapped status", status)?;
        }
        Ok(())
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(self.column_id.clone()),
            Value::Text(self.name.clone()),
            Value::Array(
                self.mapped_statuses
                    .iter()
                    .map(|status| Value::Text(status.clone()))
                    .collect(),
            ),
            optional_u32_value(self.wip_limit),
            Value::Bool(self.hidden),
            Value::Uint(self.rank),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let Value::Array(values) = value else {
            return Err(LoomError::corrupt("board column is not an array"));
        };
        let legacy = values.len() == 4;
        let mut fields = Fields::array(Value::Array(values), "board column")?;
        let column_id = fields.text("column_id")?;
        let name = fields.text("name")?;
        let mapped_statuses = string_set(fields.next("mapped_statuses")?, "mapped_statuses")?;
        let wip_limit = read_optional_u32_field(&mut fields, "wip_limit")?;
        let hidden = if legacy {
            false
        } else {
            bool_value(fields.next("hidden")?, "hidden")?
        };
        let rank = if legacy { 0 } else { fields.uint("rank")? };
        fields.end("board column")?;
        Self::with_display(column_id, name, mapped_statuses, wip_limit, hidden, rank)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SprintState {
    Planned,
    Active,
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sprint {
    pub sprint_id: String,
    pub project_id: String,
    pub name: String,
    pub goal: Option<String>,
    pub state: SprintState,
    pub members: BTreeSet<String>,
    pub committed_scope: BTreeSet<String>,
}

impl Sprint {
    pub fn new(
        sprint_id: impl Into<String>,
        project_id: impl Into<String>,
        name: impl Into<String>,
        goal: Option<String>,
    ) -> Result<Self> {
        let sprint = Self {
            sprint_id: sprint_id.into(),
            project_id: project_id.into(),
            name: name.into(),
            goal,
            state: SprintState::Planned,
            members: BTreeSet::new(),
            committed_scope: BTreeSet::new(),
        };
        sprint.validate()?;
        Ok(sprint)
    }

    fn validate(&self) -> Result<()> {
        validate_text("sprint_id", &self.sprint_id)?;
        validate_text("sprint project_id", &self.project_id)?;
        validate_text("sprint name", &self.name)?;
        if let Some(goal) = &self.goal {
            validate_text("sprint goal", goal)?;
        }
        for ticket_id in &self.members {
            validate_text("sprint member ticket_id", ticket_id)?;
        }
        for ticket_id in &self.committed_scope {
            validate_text("sprint committed ticket_id", ticket_id)?;
        }
        Ok(())
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(self.sprint_id.clone()),
            Value::Text(self.project_id.clone()),
            Value::Text(self.name.clone()),
            optional_text_value(self.goal.as_deref()),
            Value::Uint(match self.state {
                SprintState::Planned => 0,
                SprintState::Active => 1,
                SprintState::Closed => 2,
            }),
            Value::Array(
                self.members
                    .iter()
                    .map(|ticket_id| Value::Text(ticket_id.clone()))
                    .collect(),
            ),
            Value::Array(
                self.committed_scope
                    .iter()
                    .map(|ticket_id| Value::Text(ticket_id.clone()))
                    .collect(),
            ),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut fields = Fields::array(value, "sprint")?;
        let sprint_id = fields.text("sprint_id")?;
        let project_id = fields.text("project_id")?;
        let name = fields.text("name")?;
        let goal = read_optional_text_field(&mut fields, "goal")?;
        let state = match fields.uint("sprint state")? {
            0 => SprintState::Planned,
            1 => SprintState::Active,
            2 => SprintState::Closed,
            other => {
                return Err(LoomError::corrupt(format!(
                    "unknown sprint state tag {other}"
                )));
            }
        };
        let members = string_set(fields.next("members")?, "members")?;
        let committed_scope = string_set(fields.next("committed_scope")?, "committed_scope")?;
        fields.end("sprint")?;
        let sprint = Self {
            sprint_id,
            project_id,
            name,
            goal,
            state,
            members,
            committed_scope,
        };
        sprint.validate()?;
        Ok(sprint)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SprintCloseTarget {
    Backlog,
    NextSprint(String),
    DoneWithResolution(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SprintCloseDisposition {
    pub ticket_id: String,
    pub target: SprintCloseTarget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SprintCloseResult {
    pub sprint_id: String,
    pub carried_from_edges: Vec<(String, String)>,
    pub returned_to_backlog: BTreeSet<String>,
    pub resolution_updates: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgileWorkspace {
    sprints: BTreeMap<String, Sprint>,
    open_membership: BTreeMap<String, String>,
}

impl AgileWorkspace {
    pub fn new() -> Self {
        Self {
            sprints: BTreeMap::new(),
            open_membership: BTreeMap::new(),
        }
    }

    pub fn create_sprint(
        &mut self,
        sprint_id: impl Into<String>,
        project_id: impl Into<String>,
        name: impl Into<String>,
        goal: Option<String>,
    ) -> Result<&Sprint> {
        let sprint = Sprint::new(sprint_id, project_id, name, goal)?;
        if self.sprints.contains_key(&sprint.sprint_id) {
            return Err(LoomError::new(Code::AlreadyExists, "sprint already exists"));
        }
        let sprint_id = sprint.sprint_id.clone();
        self.sprints.insert(sprint_id.clone(), sprint);
        Ok(self.sprints.get(&sprint_id).unwrap())
    }

    pub fn add_ticket_to_sprint(&mut self, sprint_id: &str, ticket_id: &str) -> Result<()> {
        validate_text("ticket_id", ticket_id)?;
        let sprint = self
            .sprints
            .get(sprint_id)
            .ok_or_else(|| LoomError::not_found("sprint not found"))?;
        if sprint.state == SprintState::Closed {
            return Err(LoomError::new(Code::Conflict, "sprint is closed"));
        }
        if let Some(previous_sprint_id) = self.open_membership.get(ticket_id).cloned()
            && previous_sprint_id != sprint_id
            && let Some(previous) = self.sprints.get_mut(&previous_sprint_id)
        {
            previous.members.remove(ticket_id);
        }
        let sprint = self
            .sprints
            .get_mut(sprint_id)
            .ok_or_else(|| LoomError::not_found("sprint not found"))?;
        sprint.members.insert(ticket_id.to_string());
        self.open_membership
            .insert(ticket_id.to_string(), sprint_id.to_string());
        Ok(())
    }

    pub fn start_sprint(&mut self, sprint_id: &str) -> Result<()> {
        let sprint = self
            .sprints
            .get_mut(sprint_id)
            .ok_or_else(|| LoomError::not_found("sprint not found"))?;
        if sprint.state == SprintState::Closed {
            return Err(LoomError::new(Code::Conflict, "sprint is closed"));
        }
        sprint.state = SprintState::Active;
        sprint.committed_scope = sprint.members.clone();
        Ok(())
    }

    pub fn close_sprint(
        &mut self,
        sprint_id: &str,
        dispositions: Vec<SprintCloseDisposition>,
    ) -> Result<SprintCloseResult> {
        let members = {
            let sprint = self
                .sprints
                .get(sprint_id)
                .ok_or_else(|| LoomError::not_found("sprint not found"))?;
            if sprint.state == SprintState::Closed {
                return Err(LoomError::new(Code::Conflict, "sprint is closed"));
            }
            sprint.members.clone()
        };
        let mut disposition_by_ticket = BTreeMap::new();
        for disposition in dispositions {
            validate_text("sprint close ticket_id", &disposition.ticket_id)?;
            if !members.contains(&disposition.ticket_id) {
                return Err(LoomError::invalid(
                    "sprint close disposition references non-member ticket",
                ));
            }
            if disposition_by_ticket
                .insert(disposition.ticket_id.clone(), disposition.target)
                .is_some()
            {
                return Err(LoomError::invalid(
                    "sprint close disposition duplicates ticket",
                ));
            }
        }
        for target in disposition_by_ticket.values() {
            if let SprintCloseTarget::NextSprint(next_sprint_id) = target {
                let next = self
                    .sprints
                    .get(next_sprint_id)
                    .ok_or_else(|| LoomError::not_found("target sprint not found"))?;
                if next.state == SprintState::Closed {
                    return Err(LoomError::new(Code::Conflict, "target sprint is closed"));
                }
            }
        }
        let mut result = SprintCloseResult {
            sprint_id: sprint_id.to_string(),
            carried_from_edges: Vec::new(),
            returned_to_backlog: BTreeSet::new(),
            resolution_updates: BTreeMap::new(),
        };
        for ticket_id in members {
            self.open_membership.remove(&ticket_id);
            match disposition_by_ticket
                .remove(&ticket_id)
                .unwrap_or(SprintCloseTarget::Backlog)
            {
                SprintCloseTarget::Backlog => {
                    result.returned_to_backlog.insert(ticket_id);
                }
                SprintCloseTarget::NextSprint(next_sprint_id) => {
                    self.add_ticket_to_sprint(&next_sprint_id, &ticket_id)?;
                    result
                        .carried_from_edges
                        .push((ticket_id, sprint_id.to_string()));
                }
                SprintCloseTarget::DoneWithResolution(resolution) => {
                    validate_text("sprint close resolution", &resolution)?;
                    result.resolution_updates.insert(ticket_id, resolution);
                }
            }
        }
        let sprint = self
            .sprints
            .get_mut(sprint_id)
            .ok_or_else(|| LoomError::not_found("sprint not found"))?;
        sprint.state = SprintState::Closed;
        sprint.members.clear();
        Ok(result)
    }

    pub fn sprint(&self, sprint_id: &str) -> Option<&Sprint> {
        self.sprints.get(sprint_id)
    }

    pub fn open_sprint_for_ticket(&self, ticket_id: &str) -> Option<&str> {
        self.open_membership.get(ticket_id).map(String::as_str)
    }
}

impl Default for AgileWorkspace {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortfolioRollup {
    TicketCount,
    StoryPoints,
    Manual,
    WeightedField,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortfolioLevel {
    pub level_id: String,
    pub name: String,
    pub ticket_type: TicketType,
    pub rollup: PortfolioRollup,
    pub timeframed: bool,
    pub retired: bool,
}

impl PortfolioLevel {
    pub fn new(
        level_id: impl Into<String>,
        name: impl Into<String>,
        ticket_type: TicketType,
        rollup: PortfolioRollup,
        timeframed: bool,
    ) -> Result<Self> {
        let level = Self {
            level_id: level_id.into(),
            name: name.into(),
            ticket_type,
            rollup,
            timeframed,
            retired: false,
        };
        level.validate()?;
        Ok(level)
    }

    fn validate(&self) -> Result<()> {
        validate_text("portfolio level_id", &self.level_id)?;
        validate_text("portfolio level name", &self.name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortfolioTaxonomy {
    pub taxonomy_id: String,
    pub levels: Vec<PortfolioLevel>,
}

impl PortfolioTaxonomy {
    pub fn new(taxonomy_id: impl Into<String>, levels: Vec<PortfolioLevel>) -> Result<Self> {
        let taxonomy = Self {
            taxonomy_id: taxonomy_id.into(),
            levels,
        };
        taxonomy.validate()?;
        Ok(taxonomy)
    }

    pub fn validate_parent_edge(
        &self,
        parent_type: TicketType,
        child_type: TicketType,
    ) -> Result<()> {
        let Some(parent_level) = self.level_index(parent_type) else {
            return Err(LoomError::invalid(
                "parent ticket type is not in portfolio taxonomy",
            ));
        };
        if parent_level == 0 {
            if matches!(
                child_type,
                TicketType::Story | TicketType::Task | TicketType::Bug | TicketType::Spike
            ) {
                return Ok(());
            }
            return Err(LoomError::new(Code::Conflict, "hierarchy violation"));
        }
        let expected_child = self.levels[parent_level - 1].ticket_type;
        if child_type == expected_child {
            Ok(())
        } else {
            Err(LoomError::new(Code::Conflict, "hierarchy violation"))
        }
    }

    fn validate(&self) -> Result<()> {
        validate_text("portfolio taxonomy_id", &self.taxonomy_id)?;
        if self.levels.is_empty() {
            return Err(LoomError::invalid("portfolio taxonomy must have levels"));
        }
        let mut ticket_types = BTreeSet::new();
        for level in &self.levels {
            level.validate()?;
            if !ticket_types.insert(level.ticket_type) {
                return Err(LoomError::invalid("portfolio ticket types must be unique"));
            }
        }
        Ok(())
    }

    fn level_index(&self, ticket_type: TicketType) -> Option<usize> {
        self.levels
            .iter()
            .position(|level| level.ticket_type == ticket_type && !level.retired)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapacityUnit {
    Hours,
    Points,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CapacityRecord {
    pub sprint_id: String,
    pub principal: String,
    pub unit: CapacityUnit,
    pub capacity: f64,
    pub availability_pct: Option<f64>,
}

impl CapacityRecord {
    pub fn new(
        sprint_id: impl Into<String>,
        principal: impl Into<String>,
        unit: CapacityUnit,
        capacity: f64,
        availability_pct: Option<f64>,
    ) -> Result<Self> {
        let record = Self {
            sprint_id: sprint_id.into(),
            principal: principal.into(),
            unit,
            capacity,
            availability_pct,
        };
        record.validate()?;
        Ok(record)
    }

    fn validate(&self) -> Result<()> {
        validate_text("capacity sprint_id", &self.sprint_id)?;
        validate_text("capacity principal", &self.principal)?;
        validate_non_negative_finite("capacity", self.capacity)?;
        if let Some(availability_pct) = self.availability_pct {
            validate_non_negative_finite("availability_pct", availability_pct)?;
            if availability_pct > 1.0 {
                return Err(LoomError::invalid("availability_pct must be 0.0 to 1.0"));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CapacityLoad {
    pub principal: String,
    pub capacity: f64,
    pub load: f64,
    pub unit: CapacityUnit,
}

pub fn load_for_capacity(capacity: &CapacityRecord, tickets: &[&Ticket]) -> Result<CapacityLoad> {
    capacity.validate()?;
    let mut load = 0.0;
    for ticket in tickets {
        if assignee(ticket).as_deref() != Some(capacity.principal.as_str()) {
            continue;
        }
        if ticket_category(ticket)
            .is_some_and(|category| category == "done" || category == "accepted")
        {
            continue;
        }
        load += match capacity.unit {
            CapacityUnit::Hours => numeric_field(ticket, "remaining_estimate")?.unwrap_or(0.0),
            CapacityUnit::Points => numeric_field(ticket, "story_points")?.unwrap_or(0.0),
        };
    }
    Ok(CapacityLoad {
        principal: capacity.principal.clone(),
        capacity: capacity.capacity,
        load,
        unit: capacity.unit,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressRollup {
    TicketCount,
    StoryPoints,
    Manual,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProgressRollupResult {
    pub completed: f64,
    pub total: f64,
    pub rule: ProgressRollup,
}

pub fn progress_rollup(rule: ProgressRollup, tickets: &[&Ticket]) -> Result<ProgressRollupResult> {
    if rule == ProgressRollup::Manual {
        return Ok(ProgressRollupResult {
            completed: 0.0,
            total: 0.0,
            rule,
        });
    }
    let mut completed = 0.0;
    let mut total = 0.0;
    for ticket in tickets {
        let weight = match rule {
            ProgressRollup::TicketCount => 1.0,
            ProgressRollup::StoryPoints => numeric_field(ticket, "story_points")?.unwrap_or(0.0),
            ProgressRollup::Manual => 0.0,
        };
        total += weight;
        if ticket_category(ticket)
            .is_some_and(|category| category == "done" || category == "accepted")
        {
            completed += weight;
        }
    }
    Ok(ProgressRollupResult {
        completed,
        total,
        rule,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DependencyKind {
    FinishStart,
    StartStart,
    FinishFinish,
    StartFinish,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanningDependency {
    pub from_entity: String,
    pub to_entity: String,
    pub kind: DependencyKind,
    pub lag_days: i32,
}

impl PlanningDependency {
    pub fn new(
        from_entity: impl Into<String>,
        to_entity: impl Into<String>,
        kind: DependencyKind,
        lag_days: i32,
    ) -> Result<Self> {
        let dependency = Self {
            from_entity: from_entity.into(),
            to_entity: to_entity.into(),
            kind,
            lag_days,
        };
        dependency.validate()?;
        Ok(dependency)
    }

    fn validate(&self) -> Result<()> {
        validate_text("dependency from_entity", &self.from_entity)?;
        validate_text("dependency to_entity", &self.to_entity)?;
        if self.from_entity == self.to_entity {
            return Err(LoomError::invalid("dependency cannot target itself"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoadmapItemKind {
    Initiative,
    PortfolioRef { level_id: String, ticket_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoadmapTimeframe {
    DateRange { start: String, target: String },
    BucketNow,
    BucketNext,
    BucketLater,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoadmapItem {
    pub item_id: String,
    pub title: String,
    pub kind: RoadmapItemKind,
    pub owner: Option<String>,
    pub status: String,
    pub confidence_ppm: Option<u32>,
    pub timeframe: RoadmapTimeframe,
    pub progress_rollup: ProgressRollup,
}

impl RoadmapItem {
    pub fn validate(&self) -> Result<()> {
        validate_text("roadmap item_id", &self.item_id)?;
        validate_text("roadmap title", &self.title)?;
        validate_text("roadmap status", &self.status)?;
        if let Some(owner) = &self.owner {
            validate_text("roadmap owner", owner)?;
        }
        if let Some(confidence_ppm) = self.confidence_ppm
            && confidence_ppm > 1_000_000
        {
            return Err(LoomError::invalid("confidence_ppm exceeds 1000000"));
        }
        match &self.kind {
            RoadmapItemKind::Initiative => {}
            RoadmapItemKind::PortfolioRef {
                level_id,
                ticket_id,
            } => {
                validate_text("roadmap level_id", level_id)?;
                validate_text("roadmap ticket_id", ticket_id)?;
            }
        }
        match &self.timeframe {
            RoadmapTimeframe::DateRange { start, target } => {
                validate_text("roadmap start", start)?;
                validate_text("roadmap target", target)?;
            }
            RoadmapTimeframe::BucketNow
            | RoadmapTimeframe::BucketNext
            | RoadmapTimeframe::BucketLater => {}
        }
        Ok(())
    }
}

fn validate_non_negative_finite(name: &str, value: f64) -> Result<()> {
    if !value.is_finite() || value < 0.0 {
        return Err(LoomError::invalid(format!("{name} must be non-negative")));
    }
    Ok(())
}

fn numeric_field(ticket: &Ticket, field: &str) -> Result<Option<f64>> {
    match number_like_field(&ticket.fields, field) {
        Some(value) if value.is_finite() => Ok(Some(value)),
        Some(_) => Err(LoomError::invalid("numeric field must be finite")),
        None => match ticket.fields.get(field) {
            Some(TicketFieldValue::Null) | None => Ok(None),
            Some(_) => Err(LoomError::invalid(format!("{field} must be numeric"))),
        },
    }
}

fn number_like_field(fields: &BTreeMap<String, TicketFieldValue>, field: &str) -> Option<f64> {
    match fields.get(field) {
        Some(TicketFieldValue::Integer(value)) => Some(*value as f64),
        Some(TicketFieldValue::Number(value)) => Some(*value),
        _ => None,
    }
}

fn duration_like_field(fields: &BTreeMap<String, TicketFieldValue>, field: &str) -> Option<i64> {
    match fields.get(field) {
        Some(TicketFieldValue::DurationMillis(value)) | Some(TicketFieldValue::Integer(value)) => {
            Some(*value)
        }
        _ => None,
    }
}

pub(crate) fn text_like_field(
    fields: &BTreeMap<String, TicketFieldValue>,
    field: &str,
) -> Option<String> {
    match fields.get(field) {
        Some(TicketFieldValue::String(value))
        | Some(TicketFieldValue::EnumOption(value))
        | Some(TicketFieldValue::Principal(value)) => Some(value.clone()),
        Some(TicketFieldValue::Date(value)) => value.to_iso8601().ok(),
        Some(TicketFieldValue::DateTime(value)) => value.to_iso8601().ok(),
        _ => None,
    }
}

fn normalized_status_field(fields: &BTreeMap<String, TicketFieldValue>) -> Option<String> {
    text_like_field(fields, "status")
}

fn list_text_like_field(fields: &BTreeMap<String, TicketFieldValue>, field: &str) -> Vec<String> {
    match fields.get(field) {
        Some(TicketFieldValue::List(values)) => values
            .iter()
            .filter_map(|value| match value {
                TicketFieldValue::String(value)
                | TicketFieldValue::EnumOption(value)
                | TicketFieldValue::Principal(value) => Some(value.clone()),
                _ => None,
            })
            .collect(),
        Some(TicketFieldValue::String(value)) | Some(TicketFieldValue::EnumOption(value)) => {
            vec![value.clone()]
        }
        _ => Vec::new(),
    }
}

fn validate_ticket_field_value(field: &str, value: &TicketFieldValue) -> Result<()> {
    if is_ticket_rich_text_field(field) {
        match value {
            TicketFieldValue::String(value) => {
                return validate_ticket_rich_text(field, value);
            }
            TicketFieldValue::Null => return Ok(()),
            _ => {}
        }
    }
    value.validate()?;
    validate_ticket_value_max_length(value, ticket_field_max_bytes(field))
}

fn ticket_field_value_from_value(field: &str, value: Value) -> Result<TicketFieldValue> {
    if !is_ticket_rich_text_field(field) {
        return TicketFieldValue::from_value(value);
    }
    match value {
        Value::Array(values) if values.len() == 2 => match values.as_slice() {
            [Value::Uint(0), Value::Text(value)] => {
                validate_ticket_rich_text(field, value)?;
                Ok(TicketFieldValue::String(value.clone()))
            }
            [Value::Uint(1), Value::Text(value)] => {
                validate_ticket_rich_text(field, value)?;
                Ok(TicketFieldValue::String(value.clone()))
            }
            _ => TicketFieldValue::from_value(Value::Array(values)),
        },
        value => TicketFieldValue::from_value(value),
    }
}

fn is_ticket_rich_text_field(field: &str) -> bool {
    matches!(field, "description")
}

fn ticket_field_max_bytes(field: &str) -> usize {
    match field {
        "title" | "status" | "status_category" | "assignee" | "reporter" | "priority"
        | "resolution" | "security_level" | "start_date" | "due_date" | "original_estimate"
        | "remaining_estimate" | "time_spent" | "story_points" | "deleted_at" | "deleted_by" => {
            TICKET_COMPACT_TEXT_MAX_BYTES
        }
        _ => TICKET_DEFAULT_FIELD_TEXT_MAX_BYTES,
    }
}

fn validate_ticket_rich_text(name: &str, value: &str) -> Result<()> {
    validate_ticket_text_length(name, value, TICKET_RICH_TEXT_MAX_BYTES)
}

fn validate_ticket_text_length(name: &str, value: &str, max_bytes: usize) -> Result<()> {
    if value.is_empty() {
        return Err(LoomError::invalid(format!("{name} must not be empty")));
    }
    if value.len() > max_bytes {
        return Err(LoomError::invalid(format!("{name} is too long")));
    }
    Ok(())
}

fn validate_ticket_custom_field_default(
    field: &TicketCustomFieldDefinition,
    value: &TicketFieldValue,
) -> Result<()> {
    validate_ticket_field_type_value(&field.definition.field_type, value)?;
    if let Some(max_length) = field.max_length {
        validate_ticket_value_max_length(value, max_length as usize)?;
    }
    Ok(())
}

fn validate_ticket_field_type_value(
    field_type: &FieldType,
    value: &TicketFieldValue,
) -> Result<()> {
    match (field_type, value) {
        (_, TicketFieldValue::Null) => Ok(()),
        (FieldType::List(inner), TicketFieldValue::List(values)) => {
            for value in values {
                validate_ticket_field_type_value(inner, value)?;
            }
            Ok(())
        }
        _ => field_type.validate_value(value),
    }
}

fn validate_ticket_value_max_length(value: &TicketFieldValue, max_length: usize) -> Result<()> {
    match value {
        TicketFieldValue::String(value)
        | TicketFieldValue::Principal(value)
        | TicketFieldValue::EnumOption(value)
        | TicketFieldValue::Url(value)
        | TicketFieldValue::OpaqueJson(value) => {
            validate_ticket_text_length("ticket field value", value, max_length)
        }
        TicketFieldValue::Date(value) => {
            validate_ticket_text_length("ticket field value", &value.to_iso8601()?, max_length)
        }
        TicketFieldValue::DateTime(value) => {
            validate_ticket_text_length("ticket field value", &value.to_iso8601()?, max_length)
        }
        TicketFieldValue::DateRange { start, end } => {
            validate_ticket_text_length("ticket field value", &start.to_iso8601()?, max_length)?;
            if let Some(end) = end {
                validate_ticket_text_length("ticket field value", &end.to_iso8601()?, max_length)?;
            }
            Ok(())
        }
        TicketFieldValue::EntityRef { kind, id } => {
            validate_ticket_text_length("ticket field value", kind, max_length)?;
            validate_ticket_text_length("ticket field value", id, max_length)
        }
        TicketFieldValue::List(values) => {
            for value in values {
                validate_ticket_value_max_length(value, max_length)?;
            }
            Ok(())
        }
        TicketFieldValue::Integer(_)
        | TicketFieldValue::Number(_)
        | TicketFieldValue::Boolean(_)
        | TicketFieldValue::DurationMillis(_)
        | TicketFieldValue::Null => Ok(()),
    }
}

fn text_field(ticket: &Ticket, field: &str) -> Option<String> {
    text_like_field(&ticket.fields, field)
}

fn assignee(ticket: &Ticket) -> Option<String> {
    text_field(ticket, "assignee")
}

fn ticket_category(ticket: &Ticket) -> Option<String> {
    text_field(ticket, "status_category")
}

#[derive(Debug, Clone, PartialEq)]
pub struct TicketWorkspace {
    projects: BTreeMap<String, TicketProject>,
    tickets: BTreeMap<String, Ticket>,
    aliases: BTreeMap<String, String>,
    external_ids: BTreeMap<ExternalTicketIdentity, String>,
}

impl TicketWorkspace {
    pub fn new() -> Self {
        Self {
            projects: BTreeMap::new(),
            tickets: BTreeMap::new(),
            aliases: BTreeMap::new(),
            external_ids: BTreeMap::new(),
        }
    }

    pub fn create_project(
        &mut self,
        project_id: impl Into<String>,
        key_prefix: impl Into<String>,
        name: impl Into<String>,
    ) -> Result<&TicketProject> {
        let project = TicketProject::new(project_id, key_prefix, name)?;
        if self.projects.contains_key(&project.project_id) {
            return Err(LoomError::new(
                Code::AlreadyExists,
                "ticket project already exists",
            ));
        }
        if self
            .projects
            .values()
            .any(|existing| existing.key_prefix == project.key_prefix)
        {
            return Err(LoomError::new(
                Code::AlreadyExists,
                "ticket project key prefix exists",
            ));
        }
        let project_id = project.project_id.clone();
        self.projects.insert(project_id.clone(), project);
        Ok(self.projects.get(&project_id).unwrap())
    }

    pub fn create_ticket(
        &mut self,
        project_id: &str,
        ticket_type: TicketType,
        external_identity: Option<ExternalTicketIdentity>,
        fields: BTreeMap<String, TicketFieldValue>,
        policy_labels: &[&str],
    ) -> Result<&Ticket> {
        if let Some(identity) = &external_identity
            && self.external_ids.contains_key(identity)
        {
            return Err(LoomError::new(
                Code::AlreadyExists,
                "ticket external identity already exists",
            ));
        }
        let ticket_id = uuid::Uuid::new_v4().to_string();
        let project = self
            .projects
            .get_mut(project_id)
            .ok_or_else(|| LoomError::not_found("ticket project not found"))?;
        let ticket_number = project.allocate_ticket_number()?;
        let key = project.ticket_key(ticket_number)?.canonical();
        let ticket = Ticket::new(TicketInput {
            ticket_id: &ticket_id,
            project_id,
            ticket_number,
            ticket_type,
            external_identity: external_identity.clone(),
            fields,
            policy_labels,
        })?;
        self.aliases.insert(key, ticket_id.clone());
        if let Some(identity) = external_identity {
            self.external_ids.insert(identity, ticket_id.clone());
        }
        self.tickets.insert(ticket_id.clone(), ticket);
        Ok(self.tickets.get(&ticket_id).unwrap())
    }

    pub fn ticket(&self, ticket_id: &str) -> Option<&Ticket> {
        self.tickets.get(ticket_id)
    }

    pub fn ticket_by_key(&self, ticket_key: &str) -> Option<&Ticket> {
        let requested = TicketKey::parse(ticket_key).ok()?;
        let project = self
            .projects
            .values()
            .find(|project| project.prefix_status(&requested.prefix).is_some())?;
        self.tickets.values().find(|ticket| {
            ticket.project_id == project.project_id && ticket.ticket_number == requested.number
        })
    }

    pub fn project(&self, project_id: &str) -> Option<&TicketProject> {
        self.projects.get(project_id)
    }
}

impl Default for TicketWorkspace {
    fn default() -> Self {
        Self::new()
    }
}

fn normalize_ticket_key_prefix(value: &str) -> Result<String> {
    let value = value.to_ascii_uppercase();
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return Err(LoomError::invalid("ticket key prefix must not be empty"));
    };
    if !first.is_ascii_uppercase() {
        return Err(LoomError::invalid("ticket key prefix must start with A-Z"));
    }
    let rest = chars.collect::<Vec<_>>();
    if rest.is_empty() || rest.len() > 9 {
        return Err(LoomError::invalid(
            "ticket key prefix must be 2 to 10 characters",
        ));
    }
    if rest
        .iter()
        .any(|ch| !ch.is_ascii_uppercase() && !ch.is_ascii_digit())
    {
        return Err(LoomError::invalid(
            "ticket key prefix must contain only A-Z and 0-9",
        ));
    }
    Ok(value)
}

fn validate_ticket_id(value: &str) -> Result<()> {
    validate_text("ticket_id", value)?;
    uuid::Uuid::parse_str(value)
        .map(|_| ())
        .map_err(|_| LoomError::invalid("ticket_id must be a UUID"))
}

fn normalize_ticket_type_id(value: &str) -> Result<String> {
    let value = value.trim().to_ascii_lowercase();
    validate_ticket_type_id(&value)?;
    Ok(value)
}

fn validate_ticket_type_id(value: &str) -> Result<()> {
    validate_text("ticket type_id", value)?;
    if value
        .chars()
        .any(|ch| !(ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '-'))
    {
        return Err(LoomError::invalid(
            "ticket type_id must contain only a-z, 0-9, underscore, or hyphen",
        ));
    }
    Ok(())
}

fn validate_field_key(value: &str) -> Result<()> {
    validate_text("ticket field key", value)?;
    if value
        .chars()
        .any(|ch| !(ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_'))
    {
        return Err(LoomError::invalid(
            "ticket field key must contain only a-z, 0-9, or underscore",
        ));
    }
    Ok(())
}

fn tagged(tag: u64, values: Vec<Value>) -> Value {
    let mut fields = Vec::with_capacity(values.len() + 1);
    fields.push(Value::Uint(tag));
    fields.extend(values);
    Value::Array(fields)
}

fn optional_text_value(value: Option<&str>) -> Value {
    match value {
        Some(value) => Value::Array(vec![Value::Uint(1), Value::Text(value.to_string())]),
        None => Value::Array(vec![Value::Uint(0)]),
    }
}

fn optional_external_identity_value(value: Option<&ExternalTicketIdentity>) -> Value {
    match value {
        Some(value) => Value::Array(vec![Value::Uint(1), value.to_value()]),
        None => Value::Array(vec![Value::Uint(0)]),
    }
}

fn digest_value(value: Digest) -> Value {
    Value::Text(value.to_string())
}

fn optional_u32_value(value: Option<u32>) -> Value {
    match value {
        Some(value) => Value::Array(vec![Value::Uint(1), Value::Uint(u64::from(value))]),
        None => Value::Array(vec![Value::Uint(0)]),
    }
}

fn optional_field_value(value: Option<&FieldValue>) -> Value {
    match value {
        Some(value) => Value::Array(vec![Value::Uint(1), value.to_value()]),
        None => Value::Array(vec![Value::Uint(0)]),
    }
}

fn workflow_definition_value(value: &WorkflowDefinition) -> Value {
    Value::Array(vec![
        Value::Text(value.workflow_id.clone()),
        Value::Text(value.version.clone()),
        Value::Array(
            value
                .states
                .iter()
                .map(|state| Value::Text(state.clone()))
                .collect(),
        ),
        Value::Array(value.edges.iter().map(workflow_edge_value).collect()),
    ])
}

fn workflow_definition_from_value(value: Value) -> Result<WorkflowDefinition> {
    let mut fields = Fields::array(value, "workflow definition")?;
    let workflow_id = fields.text("workflow_id")?;
    let version = fields.text("workflow version")?;
    let states = read_string_list(fields.next("workflow states")?, "workflow states")?
        .into_iter()
        .collect();
    let edges = workflow_edge_list(fields.next("workflow edges")?)?;
    fields.end("workflow definition")?;
    WorkflowDefinition::new(workflow_id, version, states, edges)
}

fn workflow_edge_value(value: &WorkflowEdge) -> Value {
    Value::Array(vec![
        Value::Text(value.edge_id.clone()),
        Value::Text(value.from.clone()),
        Value::Text(value.to.clone()),
        Value::Array(value.guards.iter().map(workflow_guard_value).collect()),
    ])
}

fn workflow_edge_from_value(value: Value) -> Result<WorkflowEdge> {
    let mut fields = Fields::array(value, "workflow edge")?;
    let edge_id = fields.text("workflow edge_id")?;
    let from = fields.text("workflow from")?;
    let to = fields.text("workflow to")?;
    let guards = workflow_guard_list(fields.next("workflow guards")?)?;
    fields.end("workflow edge")?;
    WorkflowEdge::new(edge_id, from, to, guards)
}

fn workflow_guard_value(value: &WorkflowGuard) -> Value {
    match value {
        WorkflowGuard::RequiredFields(fields) => tagged(
            0,
            vec![Value::Array(
                fields
                    .iter()
                    .map(|field| Value::Text(field.clone()))
                    .collect(),
            )],
        ),
        WorkflowGuard::PermissionRole(role) => tagged(1, vec![Value::Text(role.clone())]),
        WorkflowGuard::PrincipalSet(principals) => tagged(
            2,
            vec![Value::Array(
                principals
                    .iter()
                    .map(|principal| Value::Text(principal.clone()))
                    .collect(),
            )],
        ),
        WorkflowGuard::LinkedTicketState { edge_kind, all_in } => tagged(
            3,
            vec![
                Value::Text(edge_kind.clone()),
                Value::Array(
                    all_in
                        .iter()
                        .map(|status| Value::Text(status.clone()))
                        .collect(),
                ),
            ],
        ),
        WorkflowGuard::ChecklistGate(gate_id) => tagged(4, vec![Value::Text(gate_id.clone())]),
        WorkflowGuard::ResolutionRequired => tagged(5, Vec::new()),
        WorkflowGuard::Predicate(predicate_id) => {
            tagged(6, vec![Value::Text(predicate_id.clone())])
        }
    }
}

fn workflow_guard_from_value(value: Value) -> Result<WorkflowGuard> {
    let mut fields = Fields::array(value, "workflow guard")?;
    let tag = fields.uint("workflow guard tag")?;
    let guard = match tag {
        0 => WorkflowGuard::RequiredFields(read_string_list(
            fields.next("required fields")?,
            "required fields",
        )?),
        1 => WorkflowGuard::PermissionRole(fields.text("permission role")?),
        2 => WorkflowGuard::PrincipalSet(
            read_string_list(fields.next("principals")?, "principals")?
                .into_iter()
                .collect(),
        ),
        3 => WorkflowGuard::LinkedTicketState {
            edge_kind: fields.text("edge kind")?,
            all_in: read_string_list(fields.next("allowed statuses")?, "allowed statuses")?
                .into_iter()
                .collect(),
        },
        4 => WorkflowGuard::ChecklistGate(fields.text("checklist gate")?),
        5 => WorkflowGuard::ResolutionRequired,
        6 => WorkflowGuard::Predicate(fields.text("predicate")?),
        other => {
            return Err(LoomError::corrupt(format!(
                "unknown workflow guard tag {other}"
            )));
        }
    };
    fields.end("workflow guard")?;
    guard.validate()?;
    Ok(guard)
}

fn optional_workflow_definition_value(value: Option<&WorkflowDefinition>) -> Value {
    match value {
        Some(value) => Value::Array(vec![Value::Uint(1), workflow_definition_value(value)]),
        None => Value::Array(vec![Value::Uint(0)]),
    }
}

fn optional_validation_value(value: Option<&WorkflowValidationRecord>) -> Value {
    match value {
        Some(value) => Value::Array(vec![Value::Uint(1), value.to_value()]),
        None => Value::Array(vec![Value::Uint(0)]),
    }
}

fn optional_rejection_rule_value(value: Option<&WorkflowRejectionRule>) -> Value {
    match value {
        Some(value) => Value::Array(vec![Value::Uint(1), rejection_rule_value(value)]),
        None => Value::Array(vec![Value::Uint(0)]),
    }
}

fn rejection_rule_value(rule: &WorkflowRejectionRule) -> Value {
    match rule {
        WorkflowRejectionRule::EdgeMissing => tagged(0, Vec::new()),
        WorkflowRejectionRule::GuardFailed { guard_id } => {
            tagged(1, vec![Value::Text(guard_id.clone())])
        }
        WorkflowRejectionRule::FieldMissing { field_ids } => tagged(
            2,
            vec![Value::Array(
                field_ids
                    .iter()
                    .map(|field_id| Value::Text(field_id.clone()))
                    .collect(),
            )],
        ),
        WorkflowRejectionRule::Permission => tagged(3, Vec::new()),
        WorkflowRejectionRule::WorkflowVersionGone => tagged(4, Vec::new()),
    }
}

fn read_optional_text_field(fields: &mut Fields, name: &str) -> Result<Option<String>> {
    match optional_value(fields.next(name)?, name)? {
        Some(Value::Text(value)) => Ok(Some(value)),
        Some(_) => Err(LoomError::corrupt(format!("{name} must be text"))),
        None => Ok(None),
    }
}

fn read_optional_external_identity(
    fields: &mut Fields,
    name: &str,
) -> Result<Option<ExternalTicketIdentity>> {
    match optional_value(fields.next(name)?, name)? {
        Some(value) => ExternalTicketIdentity::from_value(value).map(Some),
        None => Ok(None),
    }
}

fn read_optional_u32_field(fields: &mut Fields, name: &str) -> Result<Option<u32>> {
    match optional_value(fields.next(name)?, name)? {
        Some(Value::Uint(value)) => u32::try_from(value)
            .map(Some)
            .map_err(|_| LoomError::corrupt(format!("{name} exceeds u32 range"))),
        Some(_) => Err(LoomError::corrupt(format!("{name} must be uint"))),
        None => Ok(None),
    }
}

fn read_optional_u64_field(fields: &mut Fields, name: &str) -> Result<Option<u64>> {
    match optional_value(fields.next(name)?, name)? {
        Some(Value::Uint(value)) => Ok(Some(value)),
        Some(_) => Err(LoomError::corrupt(format!("{name} must be uint"))),
        None => Ok(None),
    }
}

fn read_bool_field(fields: &mut Fields, name: &str) -> Result<bool> {
    match fields.next(name)? {
        Value::Bool(value) => Ok(value),
        _ => Err(LoomError::corrupt(format!("{name} must be bool"))),
    }
}

fn read_optional_validation(
    fields: &mut Fields,
    name: &str,
) -> Result<Option<WorkflowValidationRecord>> {
    match optional_value(fields.next(name)?, name)? {
        Some(value) => WorkflowValidationRecord::from_value(value).map(Some),
        None => Ok(None),
    }
}

fn optional_workflow_definition_from_value(value: Value) -> Result<Option<WorkflowDefinition>> {
    match optional_value(value, "active_workflow")? {
        Some(value) => workflow_definition_from_value(value).map(Some),
        None => Ok(None),
    }
}

fn optional_field_value_from_value(value: Value) -> Result<Option<FieldValue>> {
    match optional_value(value, "ticket custom field default_value")? {
        Some(value) => FieldValue::from_value(value).map(Some),
        None => Ok(None),
    }
}

fn read_required_value(value: Option<Value>, name: &str) -> Result<Value> {
    value.ok_or_else(|| LoomError::corrupt(format!("{name} is missing")))
}

fn read_text_value(value: Option<Value>, name: &str) -> Result<String> {
    match read_required_value(value, name)? {
        Value::Text(value) => Ok(value),
        _ => Err(LoomError::corrupt(format!("{name} must be text"))),
    }
}

fn read_uint_value(value: Option<Value>, name: &str) -> Result<u64> {
    match read_required_value(value, name)? {
        Value::Uint(value) => Ok(value),
        _ => Err(LoomError::corrupt(format!("{name} must be uint"))),
    }
}

fn custom_field_definition_map_value(
    fields: &BTreeMap<String, TicketCustomFieldDefinition>,
) -> Value {
    Value::Array(
        fields
            .iter()
            .map(|(field_id, field)| {
                Value::Array(vec![Value::Text(field_id.clone()), field.to_value()])
            })
            .collect(),
    )
}

fn custom_field_definition_map(
    value: Value,
    name: &str,
) -> Result<BTreeMap<String, TicketCustomFieldDefinition>> {
    let values = match value {
        Value::Array(values) => values,
        _ => return Err(LoomError::corrupt(format!("{name} must be an array"))),
    };
    let mut fields_by_id = BTreeMap::new();
    for value in values {
        let mut fields = Fields::array(value, name)?;
        let field_id = fields.text("field_id")?;
        validate_field_key(&field_id)?;
        let definition = TicketCustomFieldDefinition::from_value(fields.next("definition")?)?;
        fields.end(name)?;
        if field_id != definition.definition.field_id {
            return Err(LoomError::corrupt(
                "ticket custom field map key does not match field definition id",
            ));
        }
        if fields_by_id.insert(field_id, definition).is_some() {
            return Err(LoomError::corrupt(
                "ticket custom field definitions must be unique",
            ));
        }
    }
    Ok(fields_by_id)
}

fn workflow_edge_list(value: Value) -> Result<Vec<WorkflowEdge>> {
    let Value::Array(items) = value else {
        return Err(LoomError::corrupt("workflow edges must be an array"));
    };
    items
        .into_iter()
        .map(workflow_edge_from_value)
        .collect::<Result<Vec<_>>>()
}

fn workflow_guard_list(value: Value) -> Result<Vec<WorkflowGuard>> {
    let Value::Array(items) = value else {
        return Err(LoomError::corrupt("workflow guards must be an array"));
    };
    items
        .into_iter()
        .map(workflow_guard_from_value)
        .collect::<Result<Vec<_>>>()
}

fn read_optional_rejection_rule(
    fields: &mut Fields,
    name: &str,
) -> Result<Option<WorkflowRejectionRule>> {
    match optional_value(fields.next(name)?, name)? {
        Some(value) => rejection_rule_from_value(value).map(Some),
        None => Ok(None),
    }
}

fn rejection_rule_from_value(value: Value) -> Result<WorkflowRejectionRule> {
    let mut fields = Fields::array(value, "workflow rejection rule")?;
    let tag = fields.uint("workflow rejection rule tag")?;
    let rule = match tag {
        0 => WorkflowRejectionRule::EdgeMissing,
        1 => WorkflowRejectionRule::GuardFailed {
            guard_id: fields.text("guard_id")?,
        },
        2 => WorkflowRejectionRule::FieldMissing {
            field_ids: read_string_list(fields.next("field_ids")?, "field_ids")?,
        },
        3 => WorkflowRejectionRule::Permission,
        4 => WorkflowRejectionRule::WorkflowVersionGone,
        other => {
            return Err(LoomError::corrupt(format!(
                "unknown workflow rejection rule tag {other}"
            )));
        }
    };
    fields.end("workflow rejection rule")?;
    Ok(rule)
}

fn optional_value(value: Value, name: &str) -> Result<Option<Value>> {
    let mut fields = Fields::array(value, name)?;
    let tag = fields.uint(name)?;
    let value = match tag {
        0 => None,
        1 => Some(fields.next(name)?),
        other => {
            return Err(LoomError::corrupt(format!(
                "{name} has unknown optional tag {other}"
            )));
        }
    };
    fields.end(name)?;
    Ok(value)
}

fn optional_u64_value(value: Option<u64>) -> Value {
    match value {
        Some(value) => Value::Array(vec![Value::Uint(1), Value::Uint(value)]),
        None => Value::Array(vec![Value::Uint(0)]),
    }
}

fn field_map(value: Value) -> Result<BTreeMap<String, TicketFieldValue>> {
    match value {
        Value::Map(entries) => entries
            .into_iter()
            .map(|(key, value)| match key {
                Value::Text(key) => {
                    validate_field_key(&key)?;
                    let value = ticket_field_value_from_value(&key, value)?;
                    Ok((key, value))
                }
                _ => Err(LoomError::corrupt("ticket field key must be text")),
            })
            .collect(),
        _ => Err(LoomError::corrupt("ticket fields must be a map")),
    }
}

fn relation_map(value: Value) -> Result<BTreeMap<String, TicketRelation>> {
    let Value::Array(values) = value else {
        return Err(LoomError::corrupt("ticket relations must be an array"));
    };
    let mut relations = BTreeMap::new();
    for value in values {
        let relation = TicketRelation::from_value(value)?;
        if relations
            .insert(relation.relation_id.clone(), relation)
            .is_some()
        {
            return Err(LoomError::corrupt("ticket relations duplicate relation_id"));
        }
    }
    validate_relation_cardinality(&relations)?;
    Ok(relations)
}

fn validate_relation_cardinality(relations: &BTreeMap<String, TicketRelation>) -> Result<()> {
    let mut singletons = BTreeSet::new();
    for relation in relations.values() {
        if matches!(
            relation.kind,
            TicketRelationKind::ParentOf
                | TicketRelationKind::ChildOf
                | TicketRelationKind::Duplicates
                | TicketRelationKind::Supersedes
                | TicketRelationKind::AssignedTo
        ) && !singletons.insert(relation.kind)
        {
            return Err(LoomError::invalid(
                "ticket relation kind allows only one target",
            ));
        }
    }
    Ok(())
}

fn string_set(value: Value, name: &str) -> Result<BTreeSet<String>> {
    match value {
        Value::Array(values) => values
            .into_iter()
            .map(|value| match value {
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

fn string_map_value(values: &BTreeMap<String, String>) -> Value {
    Value::Array(
        values
            .iter()
            .map(|(key, value)| {
                Value::Array(vec![Value::Text(key.clone()), Value::Text(value.clone())])
            })
            .collect(),
    )
}

fn string_map(value: Value, name: &str) -> Result<BTreeMap<String, String>> {
    let values = match value {
        Value::Array(values) => values,
        _ => return Err(LoomError::corrupt(format!("{name} must be an array"))),
    };
    let mut map = BTreeMap::new();
    for value in values {
        let mut fields = Fields::array(value, name)?;
        let key = fields.text("key")?;
        let value = fields.text("value")?;
        fields.end(name)?;
        validate_text(name, &key)?;
        validate_text(name, &value)?;
        if map.insert(key, value).is_some() {
            return Err(LoomError::invalid(format!("{name} keys must be unique")));
        }
    }
    Ok(map)
}

fn read_string_list(value: Value, name: &str) -> Result<Vec<String>> {
    match value {
        Value::Array(values) => values
            .into_iter()
            .map(|value| match value {
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

fn bool_value(value: Value, name: &str) -> Result<bool> {
    match value {
        Value::Bool(value) => Ok(value),
        _ => Err(LoomError::corrupt(format!("{name} must be bool"))),
    }
}

fn project_list(value: Value) -> Result<Vec<TicketProject>> {
    match value {
        Value::Array(values) => values.into_iter().map(TicketProject::from_value).collect(),
        _ => Err(LoomError::corrupt("projects must be an array")),
    }
}

fn ticket_list(value: Value) -> Result<Vec<Ticket>> {
    match value {
        Value::Array(values) => values.into_iter().map(Ticket::from_value).collect(),
        _ => Err(LoomError::corrupt("tickets must be an array")),
    }
}

fn board_list(value: Value) -> Result<Vec<TicketBoard>> {
    match value {
        Value::Array(values) => values.into_iter().map(TicketBoard::from_value).collect(),
        _ => Err(LoomError::corrupt("boards must be an array")),
    }
}

fn board_column_list(value: Value) -> Result<Vec<BoardColumn>> {
    match value {
        Value::Array(values) => values.into_iter().map(BoardColumn::from_value).collect(),
        _ => Err(LoomError::corrupt("board columns must be an array")),
    }
}

fn board_swimlane_list(value: Value) -> Result<Vec<BoardSwimlane>> {
    match value {
        Value::Array(values) => values.into_iter().map(BoardSwimlane::from_value).collect(),
        _ => Err(LoomError::corrupt("board swimlanes must be an array")),
    }
}

fn sprint_list(value: Value) -> Result<Vec<Sprint>> {
    match value {
        Value::Array(values) => values.into_iter().map(Sprint::from_value).collect(),
        _ => Err(LoomError::corrupt("sprints must be an array")),
    }
}

fn operation_record_list(value: Value) -> Result<Vec<TicketOperationRecord>> {
    match value {
        Value::Array(values) => values
            .into_iter()
            .map(TicketOperationRecord::from_value)
            .collect(),
        _ => Err(LoomError::corrupt(
            "ticket operation records must be an array",
        )),
    }
}

pub fn ticket_profile_snapshot_key(workspace_id: &str) -> Result<Vec<u8>> {
    validate_text("workspace_id", workspace_id)?;
    Ok(format!("{PROFILE_CONTROL_PREFIX}/{workspace_id}/snapshot").into_bytes())
}

pub fn ticket_profile_state_key(workspace_id: &str) -> Result<Vec<u8>> {
    validate_text("workspace_id", workspace_id)?;
    Ok(format!("{PROFILE_CONTROL_PREFIX}/{workspace_id}/state").into_bytes())
}

pub fn ticket_profile_operation_log_key(workspace_id: &str) -> Result<Vec<u8>> {
    validate_text("workspace_id", workspace_id)?;
    Ok(format!("{PROFILE_CONTROL_PREFIX}/{workspace_id}/operations").into_bytes())
}

pub fn ticket_operation_cursor_scope(workspace_id: &str) -> String {
    format!("{APP_ID}:{workspace_id}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_substrate::facilities::DateValue;
    use loom_types::Algo;

    fn digest(value: &[u8]) -> Digest {
        Digest::hash(Algo::Blake3, value)
    }

    fn ticket_fields() -> BTreeMap<String, TicketFieldValue> {
        BTreeMap::from([
            (
                "title".to_string(),
                TicketFieldValue::String("Set up queue".to_string()),
            ),
            ("story_points".to_string(), TicketFieldValue::Integer(3)),
            (
                "labels".to_string(),
                TicketFieldValue::List(vec![TicketFieldValue::EnumOption("infra".to_string())]),
            ),
        ])
    }

    #[test]
    fn ticket_key_prefix_is_uppercase_and_validated() {
        assert_eq!(normalize_ticket_key_prefix("loom1").unwrap(), "LOOM1");
        assert!(normalize_ticket_key_prefix("l").is_err());
        assert!(normalize_ticket_key_prefix("1LOOM").is_err());
        assert!(TicketKey::parse("loom-42").is_ok());
        assert!(TicketKey::parse("LOOM-0").is_err());
    }

    #[test]
    fn seeded_ticket_type_definitions_preserve_builtin_contract() {
        let definitions = builtin_ticket_type_definitions().unwrap();
        let type_ids = definitions
            .iter()
            .map(|definition| definition.type_id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            type_ids,
            vec!["epic", "story", "task", "bug", "spike", "subtask"]
        );
        assert!(definitions.iter().all(|definition| !definition.retired));
        assert!(
            definitions
                .iter()
                .all(|definition| definition.is_applicable_to_project("matrix"))
        );
        assert_eq!(
            definitions
                .iter()
                .find(|definition| definition.type_id == "bug")
                .unwrap()
                .semantic_kind,
            TicketTypeSemanticKind::Defect
        );
        assert_eq!(TicketType::from_type_id("Task").unwrap(), TicketType::Task);
    }

    #[test]
    fn custom_ticket_type_definition_has_stable_id_retirement_and_project_scope() {
        let mut definition = TicketTypeDefinition::new(
            "jira-change",
            "Change Request",
            TicketTypeSemanticKind::Custom,
            BTreeSet::from(["matrix".to_string()]),
        )
        .unwrap();
        assert_eq!(definition.type_id, "jira-change");
        assert!(definition.is_applicable_to_project("matrix"));
        assert!(!definition.is_applicable_to_project("other"));
        definition.retire();
        assert!(definition.retired);
        assert!(
            TicketTypeDefinition::new(
                "Bad Type",
                "Bad",
                TicketTypeSemanticKind::Custom,
                BTreeSet::new(),
            )
            .is_err()
        );
    }

    #[test]
    fn ticket_core_fields_project_native_shape_without_dropping_custom_fields() {
        let ticket = Ticket::new(TicketInput {
            ticket_id: "00000000-0000-4000-8000-000000000083",
            project_id: "proj",
            ticket_number: 83,
            ticket_type: TicketType::Task,
            external_identity: None,
            fields: BTreeMap::from([
                (
                    "title".to_string(),
                    TicketFieldValue::String("Core shape".to_string()),
                ),
                (
                    "description".to_string(),
                    TicketFieldValue::String("Native description".to_string()),
                ),
                (
                    "status".to_string(),
                    TicketFieldValue::EnumOption("waiting_for_review".to_string()),
                ),
                (
                    "status_category".to_string(),
                    TicketFieldValue::EnumOption("active".to_string()),
                ),
                (
                    "assignee".to_string(),
                    TicketFieldValue::Principal("agent:3".to_string()),
                ),
                (
                    "reporter".to_string(),
                    TicketFieldValue::Principal("agent:arbiter".to_string()),
                ),
                (
                    "priority".to_string(),
                    TicketFieldValue::EnumOption("high".to_string()),
                ),
                (
                    "resolution".to_string(),
                    TicketFieldValue::EnumOption("fixed".to_string()),
                ),
                (
                    "labels".to_string(),
                    TicketFieldValue::List(vec![
                        TicketFieldValue::EnumOption("schema".to_string()),
                        TicketFieldValue::String("core".to_string()),
                    ]),
                ),
                (
                    "start_date".to_string(),
                    TicketFieldValue::Date(DateValue::parse("2026-07-15").unwrap()),
                ),
                (
                    "due_date".to_string(),
                    TicketFieldValue::Date(DateValue::parse("2026-07-16").unwrap()),
                ),
                (
                    "original_estimate".to_string(),
                    TicketFieldValue::DurationMillis(3_600_000),
                ),
                (
                    "remaining_estimate".to_string(),
                    TicketFieldValue::Integer(1_800_000),
                ),
                (
                    "time_spent".to_string(),
                    TicketFieldValue::DurationMillis(900_000),
                ),
                ("story_points".to_string(), TicketFieldValue::Number(5.0)),
                (
                    "security_level".to_string(),
                    TicketFieldValue::EnumOption("internal".to_string()),
                ),
                (
                    "customer".to_string(),
                    TicketFieldValue::String("kept as custom".to_string()),
                ),
            ]),
            policy_labels: &["confidential"],
        })
        .unwrap();

        let mut core = TicketCoreFields::from_ticket(&ticket);
        assert_eq!(core.title.as_deref(), Some("Core shape"));
        assert_eq!(core.description.as_deref(), Some("Native description"));
        assert_eq!(core.status.as_deref(), Some("waiting_for_review"));
        assert_eq!(core.status_category.as_deref(), Some("active"));
        assert_eq!(core.assignee.as_deref(), Some("agent:3"));
        // `from_ticket` leaves the display alias unresolved; resolving with no identity
        // store falls the display back to the canonical id string.
        assert_eq!(core.assignee_display, None);
        core.resolve_displays(None);
        assert_eq!(core.assignee_display.as_deref(), Some("agent:3"));
        assert_eq!(core.reporter.as_deref(), Some("agent:arbiter"));
        assert_eq!(core.priority.as_deref(), Some("high"));
        assert_eq!(core.resolution.as_deref(), Some("fixed"));
        assert_eq!(core.labels, vec!["schema", "core"]);
        assert_eq!(core.start_date.as_deref(), Some("2026-07-15"));
        assert_eq!(core.due_date.as_deref(), Some("2026-07-16"));
        assert_eq!(core.original_estimate_ms, Some(3_600_000));
        assert_eq!(core.remaining_estimate_ms, Some(1_800_000));
        assert_eq!(core.time_spent_ms, Some(900_000));
        assert_eq!(core.story_points, Some(5.0));
        assert_eq!(core.security_level.as_deref(), Some("internal"));
        assert_eq!(core.policy_labels, vec!["confidential"]);
        assert!(ticket.fields.contains_key("customer"));

        assert_eq!(
            normalized_status_field(&ticket.fields).as_deref(),
            Some("waiting_for_review")
        );
    }

    #[test]
    fn ticket_custom_field_definition_validates_context_and_default_value() {
        let definition = FieldDefinition::new(
            "customer-impact",
            "customer_impact",
            "Customer Impact",
            FieldType::enum_options("impact-options").unwrap(),
            Vec::new(),
            true,
        )
        .unwrap();
        let field = TicketCustomFieldDefinition::new(
            definition,
            Some(64),
            true,
            true,
            TicketFieldCardinality::Single,
            Some(TicketFieldValue::EnumOption("high".to_string())),
            BTreeSet::from(["matrix".to_string()]),
            BTreeSet::from(["bug".to_string(), "task".to_string()]),
        )
        .unwrap();
        assert!(field.is_applicable("matrix", "Bug"));
        assert!(!field.is_applicable("other", "bug"));
        assert!(!field.is_applicable("matrix", "epic"));

        assert!(
            TicketCustomFieldDefinition::new(
                FieldDefinition::new(
                    "bad-default",
                    "bad_default",
                    "Bad Default",
                    FieldType::integer(),
                    Vec::new(),
                    false,
                )
                .unwrap(),
                None,
                true,
                true,
                TicketFieldCardinality::Optional,
                Some(TicketFieldValue::String("not an integer".to_string())),
                BTreeSet::new(),
                BTreeSet::new(),
            )
            .is_err()
        );
        assert!(
            TicketCustomFieldDefinition::new(
                FieldDefinition::new(
                    "opaque",
                    "opaque",
                    "Opaque",
                    FieldType::OpaqueJson,
                    Vec::new(),
                    false,
                )
                .unwrap(),
                None,
                false,
                true,
                TicketFieldCardinality::List {
                    min_items: 2,
                    max_items: Some(1),
                },
                None,
                BTreeSet::new(),
                BTreeSet::new(),
            )
            .is_err()
        );
    }

    #[test]
    fn ticket_project_custom_field_definitions_round_trip() {
        let mut project = TicketProject::new("matrix", "MX", "Matrix").unwrap();
        let field = TicketCustomFieldDefinition::new(
            FieldDefinition::new(
                "severity",
                "severity",
                "Severity",
                FieldType::enum_options("severity").unwrap(),
                vec!["tickets".to_string()],
                false,
            )
            .unwrap(),
            Some(64),
            true,
            true,
            TicketFieldCardinality::Optional,
            Some(TicketFieldValue::EnumOption("medium".to_string())),
            BTreeSet::from(["matrix".to_string()]),
            BTreeSet::from(["bug".to_string()]),
        )
        .unwrap();
        project.put_custom_field_definition(field).unwrap();
        project.retire_custom_field_definition("severity").unwrap();

        let decoded = TicketProject::decode(&project.encode().unwrap()).unwrap();
        let decoded_field = decoded
            .custom_field_definition("severity")
            .unwrap()
            .unwrap();
        assert!(decoded_field.retired);
        assert_eq!(decoded_field.max_length, Some(64));
        assert_eq!(
            decoded_field.default_value,
            Some(TicketFieldValue::EnumOption("medium".to_string()))
        );
    }

    #[test]
    fn ticket_rich_text_fields_use_ticket_length_policy() {
        let long_body = "a".repeat(TICKET_COMPACT_TEXT_MAX_BYTES + 1);
        let ticket = Ticket::new(TicketInput {
            ticket_id: "00000000-0000-4000-8000-000000000087",
            project_id: "proj",
            ticket_number: 87,
            ticket_type: TicketType::Task,
            external_identity: None,
            fields: BTreeMap::from([
                (
                    "title".to_string(),
                    TicketFieldValue::String("Rich body".to_string()),
                ),
                (
                    "description".to_string(),
                    TicketFieldValue::String(long_body.clone()),
                ),
            ]),
            policy_labels: &[],
        })
        .unwrap();
        assert_eq!(
            ticket.fields.get("description"),
            Some(&TicketFieldValue::String(long_body.clone()))
        );
        assert_eq!(Ticket::decode(&ticket.encode().unwrap()).unwrap(), ticket);

        assert!(
            Ticket::new(TicketInput {
                ticket_id: "00000000-0000-4000-8000-000000000088",
                project_id: "proj",
                ticket_number: 88,
                ticket_type: TicketType::Task,
                external_identity: None,
                fields: BTreeMap::from([(
                    "title".to_string(),
                    TicketFieldValue::String(long_body.clone()),
                )]),
                policy_labels: &[],
            })
            .is_err()
        );

        let comment = TicketComment::new("comment:1", "agent:3", long_body.clone(), 1).unwrap();
        assert_eq!(comment.content_type, TICKET_DEFAULT_BODY_CONTENT_TYPE);
        assert_eq!(
            TicketComment::decode(&comment.encode().unwrap()).unwrap(),
            comment
        );
    }

    #[test]
    fn ticket_custom_text_field_definitions_own_lengths() {
        let long_body = "b".repeat(TICKET_COMPACT_TEXT_MAX_BYTES + 1);
        let definition = FieldDefinition::new(
            "release-note",
            "release_note",
            "Release Note",
            FieldType::text(),
            Vec::new(),
            false,
        )
        .unwrap();
        let field = TicketCustomFieldDefinition::new(
            definition.clone(),
            Some((long_body.len() + 1) as u32),
            true,
            false,
            TicketFieldCardinality::Optional,
            Some(TicketFieldValue::String(long_body.clone())),
            BTreeSet::new(),
            BTreeSet::from(["task".to_string()]),
        )
        .unwrap();
        assert!(field.is_applicable("matrix", "task"));

        assert!(
            TicketCustomFieldDefinition::new(
                definition,
                Some(TICKET_COMPACT_TEXT_MAX_BYTES as u32),
                true,
                false,
                TicketFieldCardinality::Optional,
                Some(TicketFieldValue::String(long_body)),
                BTreeSet::new(),
                BTreeSet::new(),
            )
            .is_err()
        );
    }

    #[test]
    fn ticket_projection_contracts_are_explicit_and_tagged() {
        let contracts = builtin_ticket_projection_contracts();
        let profiles = contracts
            .iter()
            .map(|contract| contract.profile.profile_id())
            .collect::<Vec<_>>();
        assert_eq!(
            profiles,
            vec!["native", "jira", "asana", "notion", "redmine"]
        );
        assert!(
            contracts
                .iter()
                .all(|contract| contract.source == "canonical_ticket")
        );
        assert!(
            contracts
                .iter()
                .all(|contract| !contract.silently_mutates_native_schema)
        );
        assert_eq!(
            ticket_projection_contract(TicketProjectionProfile::Jira).tagged_response_kind,
            "ticket.projected.jira"
        );
        assert_eq!(
            TicketProjectionProfile::parse("redmine").unwrap(),
            TicketProjectionProfile::Redmine
        );
        assert!(TicketProjectionProfile::parse("adaptive").is_err());
    }

    #[test]
    fn ticket_projection_project_config_uses_explicit_override_rules() {
        let jira_config = TicketProjectionProfileConfig::new(
            TicketProjectionProfile::Jira,
            BTreeMap::from([("title".to_string(), "fields.summary".to_string())]),
            BTreeMap::from([("issue_key_style".to_string(), "jira".to_string())]),
        )
        .unwrap();
        let config = TicketProjectionProjectConfig::new(
            TicketProjectionProfile::Jira,
            BTreeSet::from([
                TicketProjectionProfile::Native,
                TicketProjectionProfile::Jira,
            ]),
            BTreeMap::from([(TicketProjectionProfile::Jira, jira_config.clone())]),
        )
        .unwrap();

        let human_default = config
            .select(TicketProjectionRequestContext::HumanDisplay, None)
            .unwrap();
        assert_eq!(human_default.profile, TicketProjectionProfile::Jira);
        assert_eq!(
            human_default.selection_source,
            TicketProjectionSelectionSource::ProjectDefaultDisplay
        );
        assert_eq!(human_default.profile_config, Some(jira_config));
        assert_eq!(
            human_default.contract.tagged_response_kind,
            "ticket.projected.jira"
        );

        let machine_default = config
            .select(TicketProjectionRequestContext::MachineApi, None)
            .unwrap();
        assert_eq!(machine_default.profile, TicketProjectionProfile::Native);
        assert_eq!(
            machine_default.selection_source,
            TicketProjectionSelectionSource::MachineDefaultNative
        );
        assert_eq!(
            machine_default.contract.tagged_response_kind,
            "ticket.native"
        );

        let explicit_native = config
            .select(
                TicketProjectionRequestContext::HumanDisplay,
                Some(TicketProjectionProfile::Native),
            )
            .unwrap();
        assert_eq!(explicit_native.profile, TicketProjectionProfile::Native);
        assert_eq!(
            explicit_native.selection_source,
            TicketProjectionSelectionSource::ExplicitRequest
        );
        assert!(
            config
                .select(
                    TicketProjectionRequestContext::MachineApi,
                    Some(TicketProjectionProfile::Asana),
                )
                .is_err()
        );

        let mut project = TicketProject::new("proj", "LOOM", "Loom Project").unwrap();
        project.projection_config = config.clone();
        let bytes = project.encode().unwrap();
        assert_eq!(TicketProject::decode(&bytes).unwrap(), project);

        assert!(
            TicketProjectionProjectConfig::new(
                TicketProjectionProfile::Asana,
                BTreeSet::from([
                    TicketProjectionProfile::Native,
                    TicketProjectionProfile::Jira
                ]),
                BTreeMap::new(),
            )
            .is_err()
        );
        assert!(
            TicketProjectionProjectConfig::new(
                TicketProjectionProfile::Native,
                BTreeSet::from([TicketProjectionProfile::Jira]),
                BTreeMap::new(),
            )
            .is_err()
        );
    }

    #[test]
    fn workspace_allocates_monotonic_keys_and_resolves_aliases() {
        let mut organization = TicketWorkspace::new();
        organization
            .create_project("proj", "LOOM", "Loom Project")
            .unwrap();
        let first = organization
            .create_ticket(
                "proj",
                TicketType::Task,
                None,
                ticket_fields(),
                &["internal"],
            )
            .unwrap();
        let first_id = first.ticket_id.clone();
        let first_number = first.ticket_number;
        let second = organization
            .create_ticket("proj", TicketType::Bug, None, ticket_fields(), &[])
            .unwrap();
        let second_number = second.ticket_number;

        assert_eq!(first_number, 1);
        assert_eq!(second_number, 2);
        assert_eq!(
            organization.ticket_by_key("loom-1").unwrap().ticket_id,
            first_id
        );
        assert_eq!(organization.project("proj").unwrap().next_ticket_number, 3);
    }

    #[test]
    fn project_rekey_preserves_ticket_identity_and_resolves_retired_keys() {
        let mut project = TicketProject::new("proj", "LOOM", "Loom Project").unwrap();
        let ticket = Ticket::new(TicketInput {
            ticket_id: "00000000-0000-4000-8000-000000000001",
            project_id: "proj",
            ticket_number: project.allocate_ticket_number().unwrap(),
            ticket_type: TicketType::Task,
            external_identity: None,
            fields: ticket_fields(),
            policy_labels: &[],
        })
        .unwrap();
        project.rekey("core").unwrap();
        let snapshot = TicketProfileSnapshot::new(
            "studio",
            vec![project],
            vec![ticket],
            Vec::new(),
            Vec::new(),
        )
        .unwrap();

        let retired = snapshot.resolve_ticket_key("LOOM-1").unwrap().unwrap();
        assert_eq!(retired.ticket_id, "00000000-0000-4000-8000-000000000001");
        assert_eq!(retired.current_key.canonical(), "CORE-1");
        assert_eq!(retired.status, TicketKeyStatus::Retired);

        let active = snapshot.resolve_ticket_key("CORE-1").unwrap().unwrap();
        assert_eq!(active.ticket_id, "00000000-0000-4000-8000-000000000001");
        assert_eq!(active.status, TicketKeyStatus::Active);
    }

    #[test]
    fn project_and_ticket_round_trip_canonical_bytes() {
        let mut project = TicketProject::new("proj", "LOOM", "Loom Project").unwrap();
        project.retired_prefixes.insert("OLD".to_string());
        let project_bytes = project.encode().unwrap();
        assert_eq!(TicketProject::decode(&project_bytes).unwrap(), project);
        assert_eq!(
            TicketProject::decode(&project_bytes)
                .unwrap()
                .encode()
                .unwrap(),
            project_bytes
        );

        let ticket = Ticket::new(TicketInput {
            ticket_id: "00000000-0000-4000-8000-000000000001",
            project_id: "proj",
            ticket_number: 1,
            ticket_type: TicketType::Story,
            external_identity: None,
            fields: ticket_fields(),
            policy_labels: &["internal", "internal"],
        })
        .unwrap();
        let ticket_bytes = ticket.encode().unwrap();
        assert_eq!(Ticket::decode(&ticket_bytes).unwrap(), ticket);
        assert_eq!(
            Ticket::decode(&ticket_bytes).unwrap().encode().unwrap(),
            ticket_bytes
        );
    }

    #[test]
    fn draft_ticket_project_schema_is_rejected() {
        let value = Value::Array(vec![
            Value::Text("loom.studio.tickets.project.v2".to_string()),
            Value::Array(vec![
                Value::Text("proj".to_string()),
                Value::Text("LOOM".to_string()),
                Value::Text("Loom Project".to_string()),
                Value::Uint(1),
                Value::Array(Vec::new()),
                Value::Uint(0),
                Value::Array(Vec::new()),
            ]),
        ]);
        assert!(TicketProject::from_value(value).is_err());
    }

    #[test]
    fn ticket_rejects_invalid_field_key() {
        let err = Ticket::new(TicketInput {
            ticket_id: "00000000-0000-4000-8000-000000000001",
            project_id: "proj",
            ticket_number: 1,
            ticket_type: TicketType::Task,
            external_identity: None,
            fields: BTreeMap::from([(
                "Bad Field".to_string(),
                TicketFieldValue::String("value".to_string()),
            )]),
            policy_labels: &[],
        })
        .unwrap_err();
        assert_eq!(err.code, loom_types::Code::InvalidArgument);
    }

    #[test]
    fn ticket_identity_requires_uuid_and_preserves_external_identity() {
        let identity = ExternalTicketIdentity::new("jira-cloud", "10042").unwrap();
        let ticket = Ticket::new(TicketInput {
            ticket_id: "00000000-0000-4000-8000-000000000042",
            project_id: "proj",
            ticket_number: 42,
            ticket_type: TicketType::Task,
            external_identity: Some(identity.clone()),
            fields: ticket_fields(),
            policy_labels: &[],
        })
        .unwrap();
        assert_eq!(Ticket::decode(&ticket.encode().unwrap()).unwrap(), ticket);

        let error = Ticket::new(TicketInput {
            ticket_id: "CORE-42",
            project_id: "proj",
            ticket_number: 42,
            ticket_type: TicketType::Task,
            external_identity: Some(identity),
            fields: ticket_fields(),
            policy_labels: &[],
        })
        .unwrap_err();
        assert_eq!(error.code, loom_types::Code::InvalidArgument);
    }

    #[test]
    fn scalar_conflict_matrix_matches_profile_policy() {
        let writes = vec![
            FieldWriteObservation {
                field_id: "title".to_string(),
                sequence: 10,
            },
            FieldWriteObservation {
                field_id: "assignee".to_string(),
                sequence: 12,
            },
        ];

        assert_eq!(
            scalar_conflict_outcome("title", ScalarConflictClass::Guarded, None, &writes).unwrap(),
            ScalarConflictOutcome::AppliedNoConflict
        );
        assert_eq!(
            scalar_conflict_outcome("title", ScalarConflictClass::Guarded, Some(9), &writes)
                .unwrap(),
            ScalarConflictOutcome::AppliedWithConflictRecord
        );
        assert_eq!(
            scalar_conflict_outcome("title", ScalarConflictClass::HumanReview, Some(9), &writes)
                .unwrap(),
            ScalarConflictOutcome::HeldForHumanReview
        );
        assert_eq!(
            scalar_conflict_outcome(
                "title",
                ScalarConflictClass::LastWriteWins,
                Some(9),
                &writes
            )
            .unwrap(),
            ScalarConflictOutcome::AppliedNoRecord
        );
        assert_eq!(
            scalar_conflict_outcome("priority", ScalarConflictClass::Guarded, Some(9), &writes)
                .unwrap(),
            ScalarConflictOutcome::AppliedNoConflict
        );
        assert_eq!(
            default_scalar_conflict_class("security_level").unwrap(),
            ScalarConflictClass::HumanReview
        );
    }

    #[test]
    fn workflow_transition_allows_required_fields_attached_to_operation() {
        let workflow = workflow();
        let ticket_fields = BTreeMap::from([(
            "status".to_string(),
            TicketFieldValue::String("In Progress".to_string()),
        )]);
        let operation = transition(
            "op-1",
            "Done",
            BTreeMap::from([(
                "resolution".to_string(),
                TicketFieldValue::EnumOption("fixed".to_string()),
            )]),
        );
        let context = WorkflowValidationContext {
            actor_roles: BTreeSet::from(["developer".to_string()]),
            ..WorkflowValidationContext::default()
        };

        let record = validate_transition(
            Some(&workflow),
            "In Progress",
            &ticket_fields,
            &operation,
            &context,
        )
        .unwrap();
        assert_eq!(record.validation_state, WorkflowValidationState::Applied);
        assert_eq!(
            record.validated_against_workflow_version.as_deref(),
            Some("v1")
        );
    }

    #[test]
    fn workflow_transition_retargets_against_current_status() {
        let workflow = workflow();
        let operation = transition("op-2", "Blocked", BTreeMap::new());
        let record = validate_transition(
            Some(&workflow),
            "Done",
            &BTreeMap::new(),
            &operation,
            &WorkflowValidationContext::default(),
        )
        .unwrap();

        assert_eq!(record.validation_state, WorkflowValidationState::Rejected);
        assert_eq!(record.rule, Some(WorkflowRejectionRule::EdgeMissing));
        assert_eq!(record.validated_against_status, "Done");
    }

    #[test]
    fn workflow_transition_reports_permission_and_missing_workflow() {
        let workflow = workflow();
        let operation = transition("op-3", "Done", BTreeMap::new());
        let permission = validate_transition(
            Some(&workflow),
            "In Progress",
            &BTreeMap::new(),
            &operation,
            &WorkflowValidationContext::default(),
        )
        .unwrap();
        assert_eq!(
            permission.validation_state,
            WorkflowValidationState::Rejected
        );
        assert_eq!(permission.rule, Some(WorkflowRejectionRule::Permission));

        let gone = validate_transition(
            None,
            "In Progress",
            &BTreeMap::new(),
            &operation,
            &WorkflowValidationContext::default(),
        )
        .unwrap();
        assert_eq!(gone.validation_state, WorkflowValidationState::Rejected);
        assert_eq!(gone.rule, Some(WorkflowRejectionRule::WorkflowVersionGone));
        assert_eq!(gone.validated_against_workflow_version, None);
    }

    #[test]
    fn agile_workspace_moves_ticket_between_open_sprints() {
        let mut organization = AgileWorkspace::new();
        organization
            .create_sprint("s1", "proj", "Sprint 1", None)
            .unwrap();
        organization
            .create_sprint("s2", "proj", "Sprint 2", None)
            .unwrap();

        organization
            .add_ticket_to_sprint("s1", "00000000-0000-4000-8000-000000000001")
            .unwrap();
        organization
            .add_ticket_to_sprint("s2", "00000000-0000-4000-8000-000000000001")
            .unwrap();

        assert!(
            !organization
                .sprint("s1")
                .unwrap()
                .members
                .contains("00000000-0000-4000-8000-000000000001")
        );
        assert!(
            organization
                .sprint("s2")
                .unwrap()
                .members
                .contains("00000000-0000-4000-8000-000000000001")
        );
        assert_eq!(
            organization.open_sprint_for_ticket("00000000-0000-4000-8000-000000000001"),
            Some("s2")
        );
    }

    #[test]
    fn sprint_close_is_explicit_and_carries_open_work() {
        let mut organization = AgileWorkspace::new();
        organization
            .create_sprint("s1", "proj", "Sprint 1", None)
            .unwrap();
        organization
            .create_sprint("s2", "proj", "Sprint 2", None)
            .unwrap();
        organization
            .add_ticket_to_sprint("s1", "00000000-0000-4000-8000-000000000001")
            .unwrap();
        organization
            .add_ticket_to_sprint("s1", "00000000-0000-4000-8000-000000000002")
            .unwrap();
        organization.start_sprint("s1").unwrap();

        let result = organization
            .close_sprint(
                "s1",
                vec![
                    SprintCloseDisposition {
                        ticket_id: "00000000-0000-4000-8000-000000000001".to_string(),
                        target: SprintCloseTarget::NextSprint("s2".to_string()),
                    },
                    SprintCloseDisposition {
                        ticket_id: "00000000-0000-4000-8000-000000000002".to_string(),
                        target: SprintCloseTarget::DoneWithResolution("fixed".to_string()),
                    },
                ],
            )
            .unwrap();

        assert_eq!(
            organization.sprint("s1").unwrap().state,
            SprintState::Closed
        );
        assert_eq!(
            organization.open_sprint_for_ticket("00000000-0000-4000-8000-000000000001"),
            Some("s2")
        );
        assert_eq!(
            result.carried_from_edges,
            vec![(
                "00000000-0000-4000-8000-000000000001".to_string(),
                "s1".to_string()
            )]
        );
        assert_eq!(
            result
                .resolution_updates
                .get("00000000-0000-4000-8000-000000000002")
                .map(String::as_str),
            Some("fixed")
        );
        assert_eq!(
            organization
                .add_ticket_to_sprint("s1", "ticket-3")
                .unwrap_err()
                .code,
            Code::Conflict
        );
    }

    #[test]
    fn portfolio_taxonomy_enforces_strict_adjacency() {
        let taxonomy = PortfolioTaxonomy::new(
            "portfolio",
            vec![
                PortfolioLevel::new(
                    "feature",
                    "Feature",
                    TicketType::Epic,
                    PortfolioRollup::StoryPoints,
                    true,
                )
                .unwrap(),
                PortfolioLevel::new(
                    "initiative",
                    "Initiative",
                    TicketType::Spike,
                    PortfolioRollup::TicketCount,
                    true,
                )
                .unwrap(),
            ],
        )
        .unwrap();

        taxonomy
            .validate_parent_edge(TicketType::Epic, TicketType::Story)
            .unwrap();
        taxonomy
            .validate_parent_edge(TicketType::Spike, TicketType::Epic)
            .unwrap();
        assert_eq!(
            taxonomy
                .validate_parent_edge(TicketType::Spike, TicketType::Story)
                .unwrap_err()
                .code,
            Code::Conflict
        );
        assert_eq!(
            taxonomy
                .validate_parent_edge(TicketType::Task, TicketType::Story)
                .unwrap_err()
                .code,
            Code::InvalidArgument
        );
    }

    #[test]
    fn planning_load_uses_remaining_estimate_or_story_points_by_unit() {
        let ticket_a = planning_ticket(
            "00000000-0000-4000-8000-00000000000a",
            "alice",
            "in_progress",
            Some(5),
            Some(8),
        );
        let ticket_b = planning_ticket(
            "00000000-0000-4000-8000-00000000000b",
            "alice",
            "done",
            Some(3),
            Some(4),
        );
        let ticket_c = planning_ticket(
            "00000000-0000-4000-8000-00000000000c",
            "bob",
            "in_progress",
            Some(2),
            Some(6),
        );
        let tickets = vec![&ticket_a, &ticket_b, &ticket_c];

        let hours =
            CapacityRecord::new("s1", "alice", CapacityUnit::Hours, 32.0, Some(0.8)).unwrap();
        let points = CapacityRecord::new("s1", "alice", CapacityUnit::Points, 10.0, None).unwrap();

        assert_eq!(load_for_capacity(&hours, &tickets).unwrap().load, 8.0);
        assert_eq!(load_for_capacity(&points, &tickets).unwrap().load, 5.0);
    }

    #[test]
    fn progress_rollup_is_derived_from_ticket_state() {
        let ticket_a = planning_ticket(
            "00000000-0000-4000-8000-00000000000a",
            "alice",
            "done",
            Some(5),
            Some(8),
        );
        let ticket_b = planning_ticket(
            "00000000-0000-4000-8000-00000000000b",
            "alice",
            "in_progress",
            Some(3),
            Some(4),
        );
        let tickets = vec![&ticket_a, &ticket_b];

        let count = progress_rollup(ProgressRollup::TicketCount, &tickets).unwrap();
        let points = progress_rollup(ProgressRollup::StoryPoints, &tickets).unwrap();

        assert_eq!(count.completed, 1.0);
        assert_eq!(count.total, 2.0);
        assert_eq!(points.completed, 5.0);
        assert_eq!(points.total, 8.0);
    }

    #[test]
    fn planning_dependency_and_roadmap_item_validate() {
        assert_eq!(
            PlanningDependency::new("ticket:one", "ticket:one", DependencyKind::FinishStart, 0)
                .unwrap_err()
                .code,
            Code::InvalidArgument
        );
        PlanningDependency::new("ticket:one", "ticket:two", DependencyKind::FinishStart, 2)
            .unwrap();

        let item = RoadmapItem {
            item_id: "roadmap-item-1".to_string(),
            title: "Feature".to_string(),
            kind: RoadmapItemKind::PortfolioRef {
                level_id: "feature".to_string(),
                ticket_id: "ticket-a".to_string(),
            },
            owner: Some("principal:alice".to_string()),
            status: "green".to_string(),
            confidence_ppm: Some(800_000),
            timeframe: RoadmapTimeframe::BucketNow,
            progress_rollup: ProgressRollup::StoryPoints,
        };
        item.validate().unwrap();
    }

    #[test]
    fn ticket_profile_snapshot_round_trips_and_uses_stable_control_key() {
        let mut tickets = TicketWorkspace::new();
        tickets
            .create_project("proj", "LOOM", "Loom Project")
            .unwrap();
        let ticket_id = tickets
            .create_ticket("proj", TicketType::Task, None, ticket_fields(), &[])
            .unwrap()
            .ticket_id
            .clone();
        let mut agile = AgileWorkspace::new();
        agile
            .create_sprint("s1", "proj", "Sprint 1", Some("Ship".to_string()))
            .unwrap();
        agile.add_ticket_to_sprint("s1", &ticket_id).unwrap();
        let snapshot = TicketProfileSnapshot::from_workspaces("studio", &tickets, &agile).unwrap();
        let encoded = snapshot.encode().unwrap();
        let decoded = TicketProfileSnapshot::decode(&encoded).unwrap();

        assert_eq!(decoded, snapshot);
        assert_eq!(decoded.encode().unwrap(), encoded);
        assert_eq!(
            ticket_profile_snapshot_key("studio").unwrap(),
            b"profile/tickets/v2/studio/snapshot".to_vec()
        );
    }

    #[test]
    fn ticket_operation_log_round_trips_validation_projection() {
        let validation = WorkflowValidationRecord {
            operation_id: "op-2".to_string(),
            validation_state: WorkflowValidationState::Rejected,
            rule: Some(WorkflowRejectionRule::FieldMissing {
                field_ids: vec!["resolution".to_string()],
            }),
            validated_against_status: "In Progress".to_string(),
            validated_against_workflow_version: Some("v1".to_string()),
        };
        let log = TicketOperationLog::new(
            "studio",
            vec![
                TicketOperationRecord::new(
                    1,
                    "op-1",
                    "ticket.created",
                    Some("00000000-0000-4000-8000-000000000001".to_string()),
                    digest(b"root-1"),
                    b"envelope-1".to_vec(),
                    None,
                )
                .unwrap(),
                TicketOperationRecord::new(
                    2,
                    "op-2",
                    "ticket.transitioned",
                    Some("00000000-0000-4000-8000-000000000001".to_string()),
                    digest(b"root-2"),
                    b"envelope-2".to_vec(),
                    Some(validation),
                )
                .unwrap(),
            ],
        )
        .unwrap();
        let encoded = log.encode().unwrap();
        let decoded = TicketOperationLog::decode(&encoded).unwrap();

        assert_eq!(decoded, log);
        assert_eq!(decoded.encode().unwrap(), encoded);
        assert_eq!(
            ticket_profile_operation_log_key("studio").unwrap(),
            b"profile/tickets/v2/studio/operations".to_vec()
        );
    }

    #[test]
    fn ticket_operation_log_rejects_non_monotonic_sequences() {
        let records = vec![
            TicketOperationRecord::new(
                2,
                "op-2",
                "ticket.created",
                None,
                digest(b"root-2"),
                b"two".to_vec(),
                None,
            )
            .unwrap(),
            TicketOperationRecord::new(
                1,
                "op-1",
                "ticket.created",
                None,
                digest(b"root-1"),
                b"one".to_vec(),
                None,
            )
            .unwrap(),
        ];
        assert_eq!(
            TicketOperationLog::new("studio", records).unwrap_err().code,
            Code::InvalidArgument
        );
    }

    fn workflow() -> WorkflowDefinition {
        WorkflowDefinition::new(
            "default",
            "v1",
            BTreeSet::from([
                "In Progress".to_string(),
                "Done".to_string(),
                "Blocked".to_string(),
            ]),
            vec![
                WorkflowEdge::new(
                    "finish",
                    "In Progress",
                    "Done",
                    vec![
                        WorkflowGuard::PermissionRole("developer".to_string()),
                        WorkflowGuard::RequiredFields(vec!["resolution".to_string()]),
                        WorkflowGuard::ResolutionRequired,
                    ],
                )
                .unwrap(),
            ],
        )
        .unwrap()
    }

    fn transition(
        operation_id: &str,
        target_status: &str,
        attached_fields: BTreeMap<String, TicketFieldValue>,
    ) -> TransitionOperation {
        TransitionOperation {
            operation_id: operation_id.to_string(),
            actor_principal: "principal:alice".to_string(),
            target_status: target_status.to_string(),
            observed_source_status: "In Progress".to_string(),
            observed_workflow_version: "v1".to_string(),
            attached_fields,
        }
    }

    fn planning_ticket(
        ticket_id: &str,
        assignee: &str,
        status_category: &str,
        story_points: Option<i64>,
        remaining_estimate: Option<i64>,
    ) -> Ticket {
        let mut fields = BTreeMap::from([
            (
                "assignee".to_string(),
                TicketFieldValue::Principal(assignee.to_string()),
            ),
            (
                "status_category".to_string(),
                TicketFieldValue::EnumOption(status_category.to_string()),
            ),
        ]);
        if let Some(story_points) = story_points {
            fields.insert(
                "story_points".to_string(),
                TicketFieldValue::Integer(story_points),
            );
        }
        if let Some(remaining_estimate) = remaining_estimate {
            fields.insert(
                "remaining_estimate".to_string(),
                TicketFieldValue::Integer(remaining_estimate),
            );
        }
        Ticket::new(TicketInput {
            ticket_id,
            project_id: "proj",
            ticket_number: 1,
            ticket_type: TicketType::Story,
            external_identity: None,
            fields,
            policy_labels: &[],
        })
        .unwrap()
    }
}
