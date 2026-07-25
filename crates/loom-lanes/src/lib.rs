#![forbid(unsafe_code)]

use std::collections::BTreeSet;

use loom_codec::Value;
use loom_core::delivery::{DeliveryEnvelope, DeliveryProduceRequest, delivery_produce};
use loom_core::document::{doc_delete, doc_list, document_get_text, document_put_text};
use loom_core::error::{Code, LoomError, Result};
use loom_core::workspace::{FacetKind, WorkspaceId};
use loom_core::{
    AclDomain, AclRight, AtomicityBoundary, AuditIntent, CompareToken, Digest, FacetSideEffects,
    FacetWrite, FacetWriteOp, Loom, OverlayDurabilityPolicy, SecondaryIndexWrite,
    WorkflowTransaction,
};
use loom_store::FileStore;
use loom_tickets::{
    WorkflowCurrentRecordKind, read_workflow_current_record, workflow_active_assignment_record,
    workflow_current_secondary_index_delete, workflow_current_secondary_index_put,
    workflow_lane_current_record,
};
use loom_types::{TicketStatusClass, classify_ticket_status, order_key::OrderKey};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const LANE_COLLECTION: &str = "lanes";
pub const APP_ID: &str = "loom-lanes";

const PROSE_MAX_BYTES: usize = 16 * 1024;

pub fn provided_capabilities() -> &'static [&'static str] {
    &["lanes"]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaneStatus {
    Idle,
    Ready,
    Working,
    WaitingForReview,
    FeedbackAvailable,
    WaitingForDecision,
    Blocked,
    Paused,
    Closed,
}

impl LaneStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Ready => "ready",
            Self::Working => "working",
            Self::WaitingForReview => "waiting_for_review",
            Self::FeedbackAvailable => "feedback_available",
            Self::WaitingForDecision => "waiting_for_decision",
            Self::Blocked => "blocked",
            Self::Paused => "paused",
            Self::Closed => "closed",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "idle" => Ok(Self::Idle),
            "ready" => Ok(Self::Ready),
            "working" => Ok(Self::Working),
            "waiting_for_review" => Ok(Self::WaitingForReview),
            "feedback_available" => Ok(Self::FeedbackAvailable),
            "waiting_for_decision" => Ok(Self::WaitingForDecision),
            "blocked" => Ok(Self::Blocked),
            "paused" => Ok(Self::Paused),
            "closed" => Ok(Self::Closed),
            _ => Err(LoomError::invalid("unsupported lane status")),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaneKind {
    Assignment,
    Tracking,
}

impl LaneKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Assignment => "assignment",
            Self::Tracking => "tracking",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "assignment" => Ok(Self::Assignment),
            "tracking" => Ok(Self::Tracking),
            _ => Err(LoomError::invalid("unsupported lane kind")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaneTicket {
    pub ticket_id: String,
    pub order_key: String,
}

/// Placement verb for inserting a lane ticket. Ordering is driven by an internal opaque order key,
/// never by caller-supplied numeric ranks; callers choose only where a ticket lands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaneTicketPlacement<'a> {
    Append,
    First,
    Before(&'a str),
    After(&'a str),
}

impl<'a> LaneTicketPlacement<'a> {
    /// Parse a public placement verb + optional anchor ticket id. Empty/"append" -> Append,
    /// "first" -> First, "before"/"after" require a non-empty anchor. Unknown verbs are rejected.
    pub fn parse(placement: &str, anchor: Option<&'a str>) -> Result<Self> {
        match placement {
            "" | "append" => Ok(Self::Append),
            "first" => Ok(Self::First),
            "before" => anchor
                .filter(|a| !a.is_empty())
                .map(Self::Before)
                .ok_or_else(|| {
                    LoomError::invalid("placement 'before' requires an anchor ticket id")
                }),
            "after" => anchor
                .filter(|a| !a.is_empty())
                .map(Self::After)
                .ok_or_else(|| {
                    LoomError::invalid("placement 'after' requires an anchor ticket id")
                }),
            other => Err(LoomError::invalid(format!(
                "unknown lane ticket placement: {other}"
            ))),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LaneTicketView {
    pub ticket_id: String,
    pub status: Option<String>,
    pub priority: Option<String>,
    pub title: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LaneView {
    pub lane_id: String,
    pub lane_key: String,
    pub title: String,
    pub description: String,
    pub lane_kind: String,
    pub owner_principal: Option<String>,
    /// additive display alias for `owner_principal`. Holds the resolved handle for the
    /// canonical owner principal id; falls back to the id string when no handle is registered, and
    /// is `None` when there is no owner. `owner_principal` remains the canonical source of truth.
    /// loom-lanes has no access to the identity store, so this is populated by the projection layer
    /// (loom-mcp / loom-cli); `lane_view` leaves it `None`.
    pub owner_display: Option<String>,
    pub stored_lane_status: String,
    pub display_status: String,
    pub status_counts: LaneStatusCounts,
    pub status_warnings: Vec<LaneStatusTextWarning>,
    pub lane_tickets: Vec<LaneTicketView>,
    pub active_ticket_id: Option<String>,
    pub status_report: String,
    pub reviewer_feedback: String,
    pub updated_at: u64,
    pub updated_by: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PublicLane {
    pub lane_id: String,
    pub lane_key: String,
    pub title: String,
    pub description: String,
    pub lane_kind: String,
    pub owner_principal: Option<String>,
    pub lane_status: String,
    pub lane_tickets: Vec<String>,
    pub active_ticket_id: Option<String>,
    pub status_report: String,
    pub reviewer_feedback: String,
    pub updated_at: u64,
    pub updated_by: String,
}

/// Compact projection of a [`LaneView`] for list/overview surfaces: the display label, the derived
/// display status, and the ordered ticket ids only. Detailed inspection (owner, stored status,
/// coordination prose, per-ticket summaries) stays behind the full [`LaneView`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LaneCompactView {
    pub lane_id: String,
    pub lane_key: String,
    pub title: String,
    pub display_status: String,
    pub status_counts: LaneStatusCounts,
    pub status_warnings: Vec<LaneStatusTextWarning>,
    pub lane_tickets: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LaneStatusTextWarning {
    pub field: String,
    pub stated_status: String,
    pub ticket_id: Option<String>,
    pub ticket_status: Option<String>,
    pub message: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct LaneStatusCounts {
    pub blocked: usize,
    pub waiting_for_decision: usize,
    pub feedback_available: usize,
    pub waiting_for_review: usize,
    pub in_progress: usize,
    pub backlog: usize,
    pub planned: usize,
    pub ready: usize,
    pub accepted: usize,
    pub rejected: usize,
    pub closed: usize,
    pub missing: usize,
    pub total: usize,
    pub next_ticket_id: Option<String>,
}

impl LaneView {
    /// Project this detailed view down to its compact form (ordered ticket ids only).
    pub fn compact(&self) -> LaneCompactView {
        LaneCompactView {
            lane_id: self.lane_id.clone(),
            lane_key: self.lane_key.clone(),
            title: self.title.clone(),
            display_status: self.display_status.clone(),
            status_counts: self.status_counts.clone(),
            status_warnings: self.status_warnings.clone(),
            lane_tickets: self
                .lane_tickets
                .iter()
                .map(|ticket| ticket.ticket_id.clone())
                .collect(),
        }
    }
}

impl PublicLane {
    pub fn from_lane(lane: &Lane) -> Self {
        Self {
            lane_id: lane.lane_id.clone(),
            lane_key: lane.lane_key.clone(),
            title: lane.title.clone(),
            description: lane.description.clone(),
            lane_kind: lane.lane_kind.clone(),
            owner_principal: lane.owner_principal.clone(),
            lane_status: lane.lane_status.clone(),
            lane_tickets: lane
                .lane_tickets
                .iter()
                .map(|ticket| ticket.ticket_id.clone())
                .collect(),
            active_ticket_id: lane.active_ticket_id.clone(),
            status_report: lane.status_report.clone(),
            reviewer_feedback: lane.reviewer_feedback.clone(),
            updated_at: lane.updated_at,
            updated_by: lane.updated_by.clone(),
        }
    }
}

pub fn public_lane(lane: &Lane) -> PublicLane {
    PublicLane::from_lane(lane)
}

pub fn lane_display_status(lane: &Lane, counts: &LaneStatusCounts) -> String {
    match lane.lane_status.as_str() {
        "paused" | "closed" => lane.lane_status.clone(),
        _ if counts.in_progress > 0 => "working".to_string(),
        _ if counts.ready > 0 || counts.planned > 0 || counts.backlog > 0 => "ready".to_string(),
        _ if counts.feedback_available > 0 => "feedback_available".to_string(),
        _ if counts.waiting_for_review > 0 => "review_required".to_string(),
        _ if counts.waiting_for_decision > 0 => "waiting_for_decision".to_string(),
        _ if counts.blocked > 0 => "blocked".to_string(),
        _ => "ready".to_string(),
    }
}

pub fn lane_status_counts(lane_tickets: &[LaneTicketView]) -> LaneStatusCounts {
    let mut counts = LaneStatusCounts {
        total: lane_tickets.len(),
        ..LaneStatusCounts::default()
    };
    for ticket in lane_tickets {
        match classify_ticket_status(ticket.status.as_deref()) {
            TicketStatusClass::Blocked => counts.blocked += 1,
            TicketStatusClass::WaitingForDecision => counts.waiting_for_decision += 1,
            TicketStatusClass::FeedbackAvailable => counts.feedback_available += 1,
            TicketStatusClass::WaitingForReview => counts.waiting_for_review += 1,
            TicketStatusClass::InProgress => counts.in_progress += 1,
            TicketStatusClass::Backlog => counts.backlog += 1,
            TicketStatusClass::Planned => counts.planned += 1,
            TicketStatusClass::Ready => counts.ready += 1,
            TicketStatusClass::Accepted => counts.accepted += 1,
            TicketStatusClass::Rejected => counts.rejected += 1,
            TicketStatusClass::Closed => counts.closed += 1,
            TicketStatusClass::Missing => counts.missing += 1,
        }
        if counts.next_ticket_id.is_none()
            && !matches!(
                classify_ticket_status(ticket.status.as_deref()),
                TicketStatusClass::Accepted
                    | TicketStatusClass::Closed
                    | TicketStatusClass::Rejected
                    | TicketStatusClass::Missing
            )
        {
            counts.next_ticket_id = Some(ticket.ticket_id.clone());
        }
    }
    counts
}

pub fn lane_status_text_warnings(
    lane: &Lane,
    lane_tickets: &[LaneTicketView],
) -> Vec<LaneStatusTextWarning> {
    let active_subject = lane.active_ticket_id.as_deref().and_then(|active| {
        lane_tickets
            .iter()
            .find(|ticket| ticket.ticket_id == active)
    });
    let counts = lane_status_counts(lane_tickets);
    let next_subject = counts
        .next_ticket_id
        .as_deref()
        .and_then(|next| lane_tickets.iter().find(|ticket| ticket.ticket_id == next));
    let subject = active_subject.or(next_subject);
    let actual = subject.map(|ticket| classify_ticket_status(ticket.status.as_deref()));
    let mut warnings = Vec::new();
    for (field, text) in [
        ("status_report", lane.status_report.as_str()),
        ("reviewer_feedback", lane.reviewer_feedback.as_str()),
    ] {
        let Some(stated) = classify_lane_status_text(text) else {
            continue;
        };
        let disagrees = actual.is_some_and(|actual| !stated.matches(actual));
        if disagrees {
            let ticket_id = subject.map(|ticket| ticket.ticket_id.clone());
            let ticket_status = subject.and_then(|ticket| ticket.status.clone());
            warnings.push(LaneStatusTextWarning {
                field: field.to_string(),
                stated_status: stated.label().to_string(),
                ticket_id: ticket_id.clone(),
                ticket_status: ticket_status.clone(),
                message: stale_lane_status_text_message(
                    field,
                    stated.label(),
                    ticket_id.as_deref(),
                    ticket_status.as_deref(),
                ),
            });
        }
    }
    warnings
}

#[derive(Clone, Copy)]
enum LaneStatusTextClaim {
    Blocked,
    Done,
    ReadyForReview,
    CommandRequested,
    Waiting,
}

impl LaneStatusTextClaim {
    fn label(self) -> &'static str {
        match self {
            Self::Blocked => "blocked",
            Self::Done => "done",
            Self::ReadyForReview => "ready_for_review",
            Self::CommandRequested => "command_requested",
            Self::Waiting => "waiting",
        }
    }

    fn matches(self, actual: TicketStatusClass) -> bool {
        match self {
            Self::Blocked => matches!(actual, TicketStatusClass::Blocked),
            Self::Done => matches!(
                actual,
                TicketStatusClass::Accepted | TicketStatusClass::Closed
            ),
            Self::ReadyForReview => matches!(actual, TicketStatusClass::WaitingForReview),
            Self::CommandRequested => matches!(actual, TicketStatusClass::WaitingForDecision),
            Self::Waiting => matches!(
                actual,
                TicketStatusClass::WaitingForDecision | TicketStatusClass::WaitingForReview
            ),
        }
    }
}

fn classify_lane_status_text(text: &str) -> Option<LaneStatusTextClaim> {
    let lower = text.to_ascii_lowercase();
    if lower.contains("ready for review") || lower.contains("ready_for_review") {
        return Some(LaneStatusTextClaim::ReadyForReview);
    }
    if lower.contains("command requested") || lower.contains("command_requested") {
        return Some(LaneStatusTextClaim::CommandRequested);
    }
    if lower.contains("blocked") {
        return Some(LaneStatusTextClaim::Blocked);
    }
    if lower.contains("done") || lower.contains("complete") || lower.contains("completed") {
        return Some(LaneStatusTextClaim::Done);
    }
    if lower.contains("waiting") || lower.contains("awaiting") {
        return Some(LaneStatusTextClaim::Waiting);
    }
    None
}

fn stale_lane_status_text_message(
    field: &str,
    stated_status: &str,
    ticket_id: Option<&str>,
    ticket_status: Option<&str>,
) -> String {
    format!(
        "{field} says {stated_status}, but {} is {}",
        ticket_id.unwrap_or("the active ticket"),
        ticket_status.unwrap_or("missing")
    )
}

/// Detect a review/closeout handoff event described in lane text (`status_report` or
/// `reviewer_feedback`). These events must also be recorded as a typed ticket comment; the lane text
/// is only a summary. Returns the canonical event label when a phrase is present, else `None`. Pure
/// over the text so callers (MCP/CLI) can warn when a lane update carries one of these phrases without
/// a corresponding ticket comment.
pub fn lane_text_closeout_event(text: &str) -> Option<&'static str> {
    let lower = text.to_ascii_lowercase();
    if lower.contains("review request")
        || lower.contains("waiting for arbiter review")
        || lower.contains("ready for review")
        || lower.contains("ready_for_review")
    {
        return Some("review_request");
    }
    if lower.contains("command run requested")
        || lower.contains("command requested")
        || lower.contains("command_requested")
    {
        return Some("command_requested");
    }
    if lower.contains("feedback addressed") || lower.contains("feedback_addressed") {
        return Some("feedback_addressed");
    }
    if lower.contains("closeout") {
        return Some("closeout_evidence");
    }
    if lower.contains("blocker") || lower.contains("blocked") {
        return Some("blocker");
    }
    None
}

pub fn lane_view(lane: &Lane, lane_tickets: Vec<LaneTicketView>) -> LaneView {
    let status_counts = lane_status_counts(&lane_tickets);
    let display_status = lane_display_status(lane, &status_counts);
    let status_warnings = lane_status_text_warnings(lane, &lane_tickets);
    LaneView {
        lane_id: lane.lane_id.clone(),
        lane_key: lane.lane_key.clone(),
        title: lane.title.clone(),
        description: lane.description.clone(),
        lane_kind: lane.lane_kind.clone(),
        owner_principal: lane.owner_principal.clone(),
        owner_display: None,
        stored_lane_status: lane.lane_status.clone(),
        display_status,
        status_counts,
        status_warnings,
        lane_tickets,
        active_ticket_id: lane.active_ticket_id.clone(),
        status_report: lane.status_report.clone(),
        reviewer_feedback: lane.reviewer_feedback.clone(),
        updated_at: lane.updated_at,
        updated_by: lane.updated_by.clone(),
    }
}

impl LaneTicket {
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(|error| {
            LoomError::invalid(format!("lane ticket cbor decode failed: {error:?}"))
        })?)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(|error| {
            LoomError::invalid(format!("lane ticket cbor encode failed: {error:?}"))
        })
    }

    fn from_value(value: Value) -> Result<Self> {
        let Value::Array(items) = value else {
            return Err(LoomError::invalid("lane ticket must be an array"));
        };
        let [ticket_id, order_key]: [Value; 2] = items
            .try_into()
            .map_err(|_| LoomError::invalid("lane ticket field count is invalid"))?;
        let Value::Text(ticket_id) = ticket_id else {
            return Err(LoomError::invalid("lane ticket_id must be text"));
        };
        let Value::Text(order_key) = order_key else {
            return Err(LoomError::invalid("lane ticket order_key must be text"));
        };
        OrderKey::parse(&order_key)?;
        Ok(Self {
            ticket_id,
            order_key,
        })
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(self.ticket_id.clone()),
            Value::Text(self.order_key.clone()),
        ])
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lane {
    pub lane_id: String,
    pub lane_key: String,
    pub title: String,
    pub description: String,
    pub lane_kind: String,
    pub owner_principal: Option<String>,
    pub lane_status: String,
    pub lane_tickets: Vec<LaneTicket>,
    pub active_ticket_id: Option<String>,
    pub status_report: String,
    pub reviewer_feedback: String,
    pub updated_at: u64,
    pub updated_by: String,
}

pub struct LaneInput<'a> {
    pub lane_id: &'a str,
    pub lane_key: &'a str,
    pub title: &'a str,
    pub description: &'a str,
    pub lane_kind: LaneKind,
    pub owner_principal: Option<&'a str>,
    pub lane_status: LaneStatus,
    pub lane_tickets: &'a [LaneTicket],
    pub active_ticket_id: Option<&'a str>,
    pub status_report: &'a str,
    pub reviewer_feedback: &'a str,
    pub updated_at: u64,
    pub updated_by: &'a str,
}

impl Lane {
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(
            loom_codec::decode(bytes).map_err(|error| {
                LoomError::invalid(format!("lane cbor decode failed: {error:?}"))
            })?,
        )
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value())
            .map_err(|error| LoomError::invalid(format!("lane cbor encode failed: {error:?}")))
    }

    pub fn new(input: LaneInput<'_>) -> Result<Self> {
        let lane = Self {
            lane_id: input.lane_id.to_string(),
            lane_key: input.lane_key.to_string(),
            title: input.title.to_string(),
            description: input.description.to_string(),
            lane_kind: input.lane_kind.as_str().to_string(),
            owner_principal: input.owner_principal.map(str::to_string),
            lane_status: input.lane_status.as_str().to_string(),
            lane_tickets: input.lane_tickets.to_vec(),
            active_ticket_id: input.active_ticket_id.map(str::to_string),
            status_report: input.status_report.to_string(),
            reviewer_feedback: input.reviewer_feedback.to_string(),
            updated_at: input.updated_at,
            updated_by: input.updated_by.to_string(),
        };
        lane.validate()?;
        Ok(lane)
    }

    fn from_value(value: Value) -> Result<Self> {
        let Value::Array(items) = value else {
            return Err(LoomError::invalid("lane must be an array"));
        };
        let [
            lane_id,
            lane_key,
            title,
            description,
            lane_kind,
            owner_principal,
            lane_status,
            lane_tickets,
            active_ticket_id,
            status_report,
            reviewer_feedback,
            updated_at,
            updated_by,
        ]: [Value; 13] = items
            .try_into()
            .map_err(|_| LoomError::invalid("lane field count is invalid"))?;
        let Value::Text(lane_id) = lane_id else {
            return Err(LoomError::invalid("lane_id must be text"));
        };
        let Value::Text(lane_key) = lane_key else {
            return Err(LoomError::invalid("lane_key must be text"));
        };
        let Value::Text(title) = title else {
            return Err(LoomError::invalid("title must be text"));
        };
        let Value::Text(description) = description else {
            return Err(LoomError::invalid("description must be text"));
        };
        let Value::Text(lane_kind) = lane_kind else {
            return Err(LoomError::invalid("lane_kind must be text"));
        };
        let owner_principal = match owner_principal {
            Value::Null => None,
            Value::Text(value) => Some(value),
            _ => return Err(LoomError::invalid("owner_principal must be null or text")),
        };
        let Value::Text(lane_status) = lane_status else {
            return Err(LoomError::invalid("lane_status must be text"));
        };
        let Value::Array(ticket_values) = lane_tickets else {
            return Err(LoomError::invalid("lane_tickets must be an array"));
        };
        let lane_tickets = ticket_values
            .into_iter()
            .map(LaneTicket::from_value)
            .collect::<Result<Vec<_>>>()?;
        let active_ticket_id = match active_ticket_id {
            Value::Null => None,
            Value::Text(value) => Some(value),
            _ => return Err(LoomError::invalid("active_ticket_id must be null or text")),
        };
        let Value::Text(status_report) = status_report else {
            return Err(LoomError::invalid("status_report must be text"));
        };
        let Value::Text(reviewer_feedback) = reviewer_feedback else {
            return Err(LoomError::invalid("reviewer_feedback must be text"));
        };
        let Value::Uint(updated_at) = updated_at else {
            return Err(LoomError::invalid("updated_at must be unsigned integer"));
        };
        let Value::Text(updated_by) = updated_by else {
            return Err(LoomError::invalid("updated_by must be text"));
        };
        let lane = Self {
            lane_id,
            lane_key,
            title,
            description,
            lane_kind,
            owner_principal,
            lane_status,
            lane_tickets,
            active_ticket_id,
            status_report,
            reviewer_feedback,
            updated_at,
            updated_by,
        };
        lane.validate()?;
        Ok(lane)
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(self.lane_id.clone()),
            Value::Text(self.lane_key.clone()),
            Value::Text(self.title.clone()),
            Value::Text(self.description.clone()),
            Value::Text(self.lane_kind.clone()),
            self.owner_principal
                .clone()
                .map_or(Value::Null, Value::Text),
            Value::Text(self.lane_status.clone()),
            Value::Array(self.lane_tickets.iter().map(LaneTicket::to_value).collect()),
            self.active_ticket_id
                .clone()
                .map_or(Value::Null, Value::Text),
            Value::Text(self.status_report.clone()),
            Value::Text(self.reviewer_feedback.clone()),
            Value::Uint(self.updated_at),
            Value::Text(self.updated_by.clone()),
        ])
    }

    pub fn validate(&self) -> Result<()> {
        validate_id("lane_id", &self.lane_id)?;
        validate_id("lane_key", &self.lane_key)?;
        validate_prose("title", &self.title)?;
        validate_prose("description", &self.description)?;
        LaneKind::parse(&self.lane_kind)?;
        if let Some(owner_principal) = &self.owner_principal {
            validate_id("owner_principal", owner_principal)?;
        }
        LaneStatus::parse(&self.lane_status)?;
        validate_prose("status_report", &self.status_report)?;
        validate_prose("reviewer_feedback", &self.reviewer_feedback)?;
        validate_id("updated_by", &self.updated_by)?;
        validate_lane_tickets(&self.lane_tickets)?;
        if let Some(active_ticket_id) = &self.active_ticket_id {
            validate_id("active_ticket_id", active_ticket_id)?;
            if !self
                .lane_tickets
                .iter()
                .any(|lane_ticket| lane_ticket.ticket_id == *active_ticket_id)
            {
                return Err(LoomError::invalid(
                    "active_ticket_id must reference a lane ticket",
                ));
            }
        }
        Ok(())
    }

    fn from_json(text: &str) -> Result<Self> {
        let lane: Self = serde_json::from_str(text)
            .map_err(|error| LoomError::invalid(format!("lane document is invalid: {error}")))?;
        lane.validate()?;
        Ok(lane)
    }

    fn to_json(&self) -> Result<String> {
        self.validate()?;
        serde_json::to_string(self)
            .map_err(|error| LoomError::invalid(format!("lane document encode failed: {error}")))
    }
}

pub fn create_lane(loom: &mut Loom<FileStore>, workspace: WorkspaceId, lane: Lane) -> Result<Lane> {
    loom.authorize_domain(workspace, AclDomain::Tickets, AclRight::Write)?;
    lane.validate()?;
    if document_get_text(loom, workspace, LANE_COLLECTION, &lane.lane_id)?.is_some() {
        return Err(LoomError::new(Code::AlreadyExists, "lane already exists"));
    }
    put_lane(loom, workspace, lane)
}

pub fn put_lane(loom: &mut Loom<FileStore>, workspace: WorkspaceId, lane: Lane) -> Result<Lane> {
    loom.authorize_domain(workspace, AclDomain::Tickets, AclRight::Write)?;
    let text = lane.to_json()?;
    validate_assignment_lane_membership(loom, workspace, &lane)?;
    let prior = get_lane(loom, workspace, &lane.lane_id)?;
    document_put_text(loom, workspace, LANE_COLLECTION, &lane.lane_id, &text, None)?;
    put_lane_current_record(loom, workspace, &lane, prior.as_ref())?;
    Ok(lane)
}

pub fn put_lane_current_record(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    lane: &Lane,
    prior_lane: Option<&Lane>,
) -> Result<()> {
    let workspace_id = workspace.to_string();
    let lane_record = workflow_lane_current_record(
        &workspace_id,
        &lane.lane_id,
        lane.encode()?,
        loom.store().reference_root(),
    )?;
    let mut records = vec![lane_record];
    if lane.lane_kind == LaneKind::Assignment.as_str()
        && let Some(active_ticket_id) = &lane.active_ticket_id
    {
        records.push(workflow_active_assignment_record(
            &workspace_id,
            &lane.lane_id,
            active_ticket_id.as_bytes().to_vec(),
            loom.store().reference_root(),
        )?);
    } else if let Some(prior) = prior_lane
        && prior.active_ticket_id.is_some()
    {
        records.push(workflow_active_assignment_record(
            &workspace_id,
            &lane.lane_id,
            Vec::new(),
            loom.store().reference_root(),
        )?);
    }
    let snapshot = loom.mutable_overlay_snapshot();
    let mut writes = Vec::with_capacity(records.len());
    for (index, record) in records.iter().enumerate() {
        let key = record.overlay_key()?;
        let mut secondary_indexes = Vec::new();
        if index == 0 {
            secondary_indexes =
                lane_workflow_secondary_index_updates(&workspace_id, prior_lane, lane)?;
        }
        let op = if index == 1
            && lane.active_ticket_id.is_none()
            && prior_lane
                .and_then(|prior| prior.active_ticket_id.as_ref())
                .is_some()
        {
            FacetWriteOp::Delete
        } else {
            FacetWriteOp::Put {
                payload: record.encode()?,
            }
        };
        writes.push(FacetWrite {
            facet: FacetKind::Queue,
            target: key.clone(),
            op,
            secondary_indexes,
            expected: snapshot.owner_token(&key)?.map(CompareToken),
            durability: None,
            audit: Some(AuditIntent {
                operation: "lanes.current.put".to_string(),
            }),
            side_effects: FacetSideEffects::default(),
        });
    }
    loom.store()
        .commit_workflow_transaction(WorkflowTransaction {
            workspace,
            actor: loom.effective_principal()?.unwrap_or(workspace),
            expected_generation: Some(snapshot.generation()),
            writes,
            durability: OverlayDurabilityPolicy::Normal,
            boundary: AtomicityBoundary::Single,
            idempotency: None,
            owner_state: loom_core::WorkflowOwnerState::default(),
        })?;
    *loom.mutable_overlay_mut() =
        loom_core::MutableOverlay::import_entries(&loom.store().mutable_overlay_entries()?)?;
    Ok(())
}

fn lane_workflow_secondary_index_updates(
    workspace_id: &str,
    prior_lane: Option<&Lane>,
    lane: &Lane,
) -> Result<Vec<SecondaryIndexWrite>> {
    let mut indexes = lane_workflow_secondary_indexes(workspace_id, lane)?;
    if let Some(prior_lane) = prior_lane {
        let current_keys = indexes
            .iter()
            .map(|write| write.index.clone())
            .collect::<BTreeSet<_>>();
        for write in lane_workflow_secondary_index_deletes(workspace_id, prior_lane)? {
            if !current_keys.contains(&write.index) {
                indexes.push(write);
            }
        }
    }
    Ok(indexes)
}

fn lane_workflow_secondary_indexes(
    workspace_id: &str,
    lane: &Lane,
) -> Result<Vec<SecondaryIndexWrite>> {
    let mut indexes = vec![workflow_current_secondary_index_put(
        workspace_id,
        LANE_COLLECTION,
        "lane-kind",
        &lane.lane_kind,
        &lane.lane_id,
        lane.lane_id.as_bytes().to_vec(),
    )?];
    if let Some(owner_principal) = &lane.owner_principal {
        indexes.push(workflow_current_secondary_index_put(
            workspace_id,
            LANE_COLLECTION,
            "owner-principal",
            owner_principal,
            &lane.lane_id,
            lane.lane_id.as_bytes().to_vec(),
        )?);
    }
    for lane_ticket in &lane.lane_tickets {
        indexes.push(workflow_current_secondary_index_put(
            workspace_id,
            LANE_COLLECTION,
            "lane-ticket",
            &lane_ticket.ticket_id,
            &lane.lane_id,
            lane_ticket.order_key.as_bytes().to_vec(),
        )?);
    }
    Ok(indexes)
}

fn lane_workflow_secondary_index_deletes(
    workspace_id: &str,
    lane: &Lane,
) -> Result<Vec<SecondaryIndexWrite>> {
    let mut indexes = vec![workflow_current_secondary_index_delete(
        workspace_id,
        LANE_COLLECTION,
        "lane-kind",
        &lane.lane_kind,
        &lane.lane_id,
    )?];
    if let Some(owner_principal) = &lane.owner_principal {
        indexes.push(workflow_current_secondary_index_delete(
            workspace_id,
            LANE_COLLECTION,
            "owner-principal",
            owner_principal,
            &lane.lane_id,
        )?);
    }
    for lane_ticket in &lane.lane_tickets {
        indexes.push(workflow_current_secondary_index_delete(
            workspace_id,
            LANE_COLLECTION,
            "lane-ticket",
            &lane_ticket.ticket_id,
            &lane.lane_id,
        )?);
    }
    Ok(indexes)
}

pub fn append_lane_ticket(lane: &mut Lane, ticket_id: &str) -> Result<()> {
    place_lane_ticket(lane, ticket_id, LaneTicketPlacement::Append)
}

/// Insert `ticket_id` into `lane.lane_tickets` at the position selected by `placement`, minting an
/// internal opaque order key strictly between its new neighbours. Ordering is never caller-supplied.
pub fn place_lane_ticket(
    lane: &mut Lane,
    ticket_id: &str,
    placement: LaneTicketPlacement<'_>,
) -> Result<()> {
    validate_id("lane ticket_id", ticket_id)?;
    if lane
        .lane_tickets
        .iter()
        .any(|lane_ticket| lane_ticket.ticket_id == ticket_id)
    {
        return Err(LoomError::invalid("lane_tickets must not repeat ticket_id"));
    }
    let idx = match placement {
        LaneTicketPlacement::Append => lane.lane_tickets.len(),
        LaneTicketPlacement::First => 0,
        LaneTicketPlacement::Before(anchor) => lane
            .lane_tickets
            .iter()
            .position(|lane_ticket| lane_ticket.ticket_id == anchor)
            .ok_or_else(|| LoomError::not_found("lane placement anchor ticket not found"))?,
        LaneTicketPlacement::After(anchor) => {
            lane.lane_tickets
                .iter()
                .position(|lane_ticket| lane_ticket.ticket_id == anchor)
                .ok_or_else(|| LoomError::not_found("lane placement anchor ticket not found"))?
                + 1
        }
    };
    let lo_key = match idx.checked_sub(1).and_then(|i| lane.lane_tickets.get(i)) {
        Some(lane_ticket) => Some(OrderKey::parse(&lane_ticket.order_key)?),
        None => None,
    };
    let hi_key = match lane.lane_tickets.get(idx) {
        Some(lane_ticket) => Some(OrderKey::parse(&lane_ticket.order_key)?),
        None => None,
    };
    let key = OrderKey::between(lo_key.as_ref(), hi_key.as_ref());
    lane.lane_tickets.insert(
        idx,
        LaneTicket {
            ticket_id: ticket_id.to_string(),
            order_key: key.into_string(),
        },
    );
    Ok(())
}

pub fn replace_lane_ticket_order(lane: &mut Lane, ticket_ids: &[String]) -> Result<()> {
    let lane_tickets = lane_tickets_from_order(ticket_ids)?;
    lane.lane_tickets = lane_tickets;
    if let Some(active_ticket_id) = &lane.active_ticket_id
        && !lane
            .lane_tickets
            .iter()
            .any(|lane_ticket| lane_ticket.ticket_id == *active_ticket_id)
    {
        lane.active_ticket_id = None;
    }
    Ok(())
}

pub fn lane_tickets_from_order(ticket_ids: &[String]) -> Result<Vec<LaneTicket>> {
    let mut seen = BTreeSet::new();
    let mut prev: Option<OrderKey> = None;
    ticket_ids
        .iter()
        .map(|ticket_id| {
            validate_id("lane ticket_id", ticket_id)?;
            if !seen.insert(ticket_id.as_str()) {
                return Err(LoomError::invalid("lane_tickets must not repeat ticket_id"));
            }
            let key = OrderKey::between(prev.as_ref(), None);
            prev = Some(key.clone());
            Ok(LaneTicket {
                ticket_id: ticket_id.clone(),
                order_key: key.into_string(),
            })
        })
        .collect()
}

/// True when `status` classifies to a terminal ticket status class (Accepted, Rejected, or Closed).
/// Terminal tickets carry no remaining active/queued work, so first-class lane cleanup drops them
/// from lane membership while the ticket record and its history stay untouched.
pub fn is_terminal_ticket_status(status: Option<&str>) -> bool {
    matches!(
        classify_ticket_status(status),
        TicketStatusClass::Accepted | TicketStatusClass::Rejected | TicketStatusClass::Closed
    )
}

/// The ids of the lane member tickets whose resolved status is terminal, in current lane order.
/// Pure over the resolved [`LaneTicketView`]s, so callers can use it as a dry-run preview of what
/// [`remove_lane_tickets`] would drop without mutating anything.
pub fn terminal_lane_ticket_ids(lane_tickets: &[LaneTicketView]) -> Vec<String> {
    lane_tickets
        .iter()
        .filter(|ticket| is_terminal_ticket_status(ticket.status.as_deref()))
        .map(|ticket| ticket.ticket_id.clone())
        .collect()
}

/// Remove `remove_ids` from `lane.lane_tickets`, preserving the order of the members that remain and
/// re-minting valid, monotonic order keys via [`replace_lane_ticket_order`]. Clears `active_ticket_id`
/// when the active ticket was among those removed. Returns the ids that were actually present and
/// removed (ids not in the lane are ignored). This only edits lane membership: no ticket record is
/// touched, so ticket history and evidence are preserved on the tickets themselves.
pub fn remove_lane_tickets(lane: &mut Lane, remove_ids: &[String]) -> Vec<String> {
    let remove_set: BTreeSet<&str> = remove_ids.iter().map(String::as_str).collect();
    let mut removed = Vec::new();
    let mut kept = Vec::new();
    for lane_ticket in &lane.lane_tickets {
        if remove_set.contains(lane_ticket.ticket_id.as_str()) {
            removed.push(lane_ticket.ticket_id.clone());
        } else {
            kept.push(lane_ticket.ticket_id.clone());
        }
    }
    if removed.is_empty() {
        return removed;
    }
    // `kept` is a subset of the existing (already-validated) membership: valid ids with no repeats.
    // Rebuilding from it re-mints order keys and clears active_ticket_id when it is no longer present.
    replace_lane_ticket_order(lane, &kept)
        .expect("kept ids are a valid subset of existing lane membership");
    removed
}

pub fn transfer_assignment_lane_ticket(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    source_lane_id: &str,
    target_lane_id: &str,
    ticket_id: &str,
    updated_at: u64,
    updated_by: &str,
) -> Result<(Lane, Lane)> {
    loom.authorize_domain(workspace, AclDomain::Tickets, AclRight::Write)?;
    if source_lane_id == target_lane_id {
        return Err(LoomError::invalid("source and target lanes must differ"));
    }
    let mut source = get_lane(loom, workspace, source_lane_id)?
        .ok_or_else(|| LoomError::not_found("source lane not found"))?;
    let mut target = get_lane(loom, workspace, target_lane_id)?
        .ok_or_else(|| LoomError::not_found("target lane not found"))?;
    for lane in [&source, &target] {
        if lane.lane_kind != LaneKind::Assignment.as_str() {
            return Err(LoomError::invalid(
                "lane transfer requires assignment lanes",
            ));
        }
        if lane.lane_status == LaneStatus::Closed.as_str() {
            return Err(LoomError::invalid("closed lanes cannot transfer tickets"));
        }
    }
    let before = source.lane_tickets.len();
    source
        .lane_tickets
        .retain(|lane_ticket| lane_ticket.ticket_id != ticket_id);
    if source.lane_tickets.len() == before {
        return Err(LoomError::not_found("ticket is not in source lane"));
    }
    if source.active_ticket_id.as_deref() == Some(ticket_id) {
        source.active_ticket_id = None;
    }
    append_lane_ticket(&mut target, ticket_id)?;
    source.updated_at = updated_at;
    source.updated_by = updated_by.to_string();
    target.updated_at = updated_at;
    target.updated_by = updated_by.to_string();
    let source = put_lane(loom, workspace, source)?;
    let target = put_lane(loom, workspace, target)?;
    Ok((source, target))
}

pub fn delete_lane(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    lane_id: &str,
    updated_at: u64,
    updated_by: &str,
) -> Result<Lane> {
    loom.authorize_domain(workspace, AclDomain::Tickets, AclRight::Write)?;
    let mut lane = get_lane(loom, workspace, lane_id)?
        .ok_or_else(|| LoomError::not_found("lane not found"))?;
    if lane.lane_status != LaneStatus::Closed.as_str() {
        return Err(LoomError::invalid("only closed lanes can be deleted"));
    }
    lane.updated_at = updated_at;
    lane.updated_by = updated_by.to_string();
    emit_lane_change_notification(
        loom,
        workspace,
        &workspace.to_string(),
        &lane,
        "lane.deleted",
    )?;
    doc_delete(loom, workspace, LANE_COLLECTION, lane_id)?;
    Ok(lane)
}

/// A per-record decode failure surfaced by fail-soft lane listing: the offending lane document id
/// plus a human-readable reason. One malformed record must never make the whole coordination surface
/// unreadable, so `list_lanes_with_diagnostics` returns the lanes that decode alongside one
/// of these per record that does not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaneDecodeDiagnostic {
    pub lane_id: String,
    pub error: String,
}

pub fn get_lane(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    lane_id: &str,
) -> Result<Option<Lane>> {
    loom.authorize_domain(workspace, AclDomain::Tickets, AclRight::Read)?;
    let Some(document) = document_get_text(loom, workspace, LANE_COLLECTION, lane_id)? else {
        return Ok(None);
    };
    let base = Lane::from_json(&document.text)?;
    let snapshot = loom.open_mutable_overlay_read_snapshot(Some("lanes.get"))?;
    let current = read_workflow_current_record(
        &snapshot,
        &workspace.to_string(),
        "lanes",
        WorkflowCurrentRecordKind::Lane,
        lane_id,
        |_| Ok(Some(base.encode()?)),
    )?;
    current
        .map(|record| Lane::decode(&record.payload))
        .transpose()
        .map(|current| current.or(Some(base)))
}

pub fn list_lanes(loom: &Loom<FileStore>, workspace: WorkspaceId) -> Result<Vec<Lane>> {
    Ok(list_lanes_with_diagnostics(loom, workspace)?.0)
}

/// Fail-soft lane listing. Authorization and collection access still fail hard, but a
/// single malformed lane record no longer poisons the whole list: returns the lanes that decode
/// plus a diagnostic for each record that does not. Callers that only want the healthy lanes use
/// [`list_lanes`]; the MCP/CLI/dashboard readers use this to also surface the diagnostics.
pub fn list_lanes_with_diagnostics(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
) -> Result<(Vec<Lane>, Vec<LaneDecodeDiagnostic>)> {
    loom.authorize_domain(workspace, AclDomain::Tickets, AclRight::Read)?;
    let documents: Vec<(String, Vec<u8>)> = doc_list(loom, workspace, LANE_COLLECTION)?
        .iter()
        .map(|(id, bytes)| (id.to_string(), bytes.to_vec()))
        .collect();
    let (lanes, diagnostics) = partition_lane_documents(documents);
    let workspace_id = workspace.to_string();
    let snapshot = loom.open_mutable_overlay_read_snapshot(Some("lanes.list"))?;
    let mut lanes = lanes
        .into_iter()
        .map(|lane| {
            let current = read_workflow_current_record(
                &snapshot,
                &workspace_id,
                "lanes",
                WorkflowCurrentRecordKind::Lane,
                &lane.lane_id,
                |_| Ok(Some(lane.encode()?)),
            )?;
            current
                .map(|record| Lane::decode(&record.payload))
                .transpose()
                .map(|current| current.unwrap_or(lane))
        })
        .collect::<Result<Vec<_>>>()?;
    lanes.sort_by(|left, right| {
        left.lane_key
            .cmp(&right.lane_key)
            .then_with(|| left.lane_id.cmp(&right.lane_id))
    });
    Ok((lanes, diagnostics))
}

/// Decode lane documents into successfully-decoded lanes and per-record diagnostics without ever
/// returning early on a bad record. Pure over owned documents so it is unit-testable without a store.
fn partition_lane_documents(
    documents: Vec<(String, Vec<u8>)>,
) -> (Vec<Lane>, Vec<LaneDecodeDiagnostic>) {
    let mut lanes = Vec::new();
    let mut diagnostics = Vec::new();
    for (lane_id, bytes) in documents {
        let decoded = std::str::from_utf8(&bytes)
            .map_err(|_| LoomError::invalid("lane document is not utf-8"))
            .and_then(Lane::from_json);
        match decoded {
            Ok(lane) => lanes.push(lane),
            Err(error) => diagnostics.push(LaneDecodeDiagnostic {
                lane_id,
                error: error.to_string(),
            }),
        }
    }
    (lanes, diagnostics)
}

pub fn emit_lane_change_notification(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    lane: &Lane,
    event_kind: &str,
) -> Result<DeliveryEnvelope> {
    let payload = json!({
        "workspace_id": workspace_id,
        "lane_id": lane.lane_id,
        "event_kind": event_kind,
        "lane_status": lane.lane_status,
        "active_ticket_id": lane.active_ticket_id,
        "ticket_count": lane.lane_tickets.len(),
        "updated_at": lane.updated_at,
        "updated_by": lane.updated_by,
    });
    let payload = serde_json::to_vec(&payload)
        .map_err(|error| LoomError::invalid(format!("lane notification payload: {error}")))?;
    let cursor = lane_change_cursor(lane, event_kind)?;
    delivery_produce(
        loom,
        workspace,
        DeliveryProduceRequest {
            stream_id: &lane_change_stream(workspace_id),
            producer: APP_ID,
            subject: &format!("lane:{}", lane.lane_id),
            payload: &payload,
            created_at_ms: lane.updated_at,
            expires_at_ms: None,
            source_cursor: Some(cursor.as_bytes()),
        },
    )
}

pub fn lane_change_stream(workspace_id: &str) -> String {
    format!("lanes:{workspace_id}:changes")
}

fn lane_change_cursor(lane: &Lane, event_kind: &str) -> Result<String> {
    let payload = serde_json::to_vec(lane)
        .map_err(|error| LoomError::invalid(format!("lane cursor payload: {error}")))?;
    Ok(format!(
        "{}:{}:{}",
        lane.updated_at,
        event_kind,
        Digest::blake3(&payload)
    ))
}

fn validate_id(label: &str, value: &str) -> Result<()> {
    if value.is_empty() || value.len() > 512 {
        return Err(LoomError::invalid(format!("{label} is invalid")));
    }
    if value.chars().any(char::is_control) {
        return Err(LoomError::invalid(format!("{label} is invalid")));
    }
    Ok(())
}

fn validate_prose(label: &str, value: &str) -> Result<()> {
    if value.len() > PROSE_MAX_BYTES {
        return Err(LoomError::invalid(format!("{label} is too long")));
    }
    Ok(())
}

fn validate_lane_tickets(lane_tickets: &[LaneTicket]) -> Result<()> {
    let mut ticket_ids = BTreeSet::new();
    let mut previous_key: Option<OrderKey> = None;
    for lane_ticket in lane_tickets {
        validate_id("lane ticket_id", &lane_ticket.ticket_id)?;
        if !ticket_ids.insert(lane_ticket.ticket_id.as_str()) {
            return Err(LoomError::invalid("lane_tickets must not repeat ticket_id"));
        }
        let key = OrderKey::parse(&lane_ticket.order_key)?;
        if let Some(previous) = &previous_key
            && key <= *previous
        {
            return Err(LoomError::invalid(
                "lane_tickets must be ordered by order_key",
            ));
        }
        previous_key = Some(key);
    }
    Ok(())
}

fn validate_assignment_lane_membership(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    candidate: &Lane,
) -> Result<()> {
    if candidate.lane_kind != LaneKind::Assignment.as_str()
        || candidate.lane_status == LaneStatus::Closed.as_str()
    {
        return Ok(());
    }
    let candidate_tickets = candidate
        .lane_tickets
        .iter()
        .map(|lane_ticket| lane_ticket.ticket_id.as_str())
        .collect::<BTreeSet<_>>();
    if candidate_tickets.is_empty() {
        return Ok(());
    }
    for lane in list_lanes(loom, workspace)? {
        if lane.lane_id == candidate.lane_id
            || lane.lane_kind != LaneKind::Assignment.as_str()
            || lane.lane_status == LaneStatus::Closed.as_str()
        {
            continue;
        }
        if let Some(ticket_id) = lane
            .lane_tickets
            .iter()
            .map(|lane_ticket| lane_ticket.ticket_id.as_str())
            .find(|ticket_id| candidate_tickets.contains(ticket_id))
        {
            return Err(LoomError::invalid(format!(
                "ticket {ticket_id} already belongs to assignment lane {}",
                lane.lane_id
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use loom_core::{AclRight, AclSubject, FacetKind, Loom};
    use loom_store::{FileStore, MemoryBacking};

    use super::*;

    fn workspace() -> WorkspaceId {
        WorkspaceId::from_bytes([1; 16])
    }

    fn loom() -> Loom<FileStore> {
        let workspace = workspace();
        let mut loom =
            Loom::new(FileStore::with_backing(Box::new(MemoryBacking::new()), true).unwrap());
        loom.acl_store_mut()
            .allow(
                AclSubject::Everyone,
                Some(workspace),
                Some(FacetKind::Document),
                [AclRight::Read, AclRight::Write],
            )
            .unwrap();
        loom
    }

    fn sample_lane() -> Lane {
        Lane::new(LaneInput {
            lane_id: "review-lane-a",
            lane_key: "review-lane-a",
            title: "Review lane A",
            description: "Durable intention: land the sample ticket work end to end.",
            lane_kind: LaneKind::Assignment,
            owner_principal: Some("user:reviewer-a"),
            lane_status: LaneStatus::Working,
            lane_tickets: &[
                LaneTicket {
                    ticket_id: "DEMO-102".to_string(),
                    order_key: "F".to_string(),
                },
                LaneTicket {
                    ticket_id: "DEMO-103".to_string(),
                    order_key: "V".to_string(),
                },
            ],
            active_ticket_id: Some("DEMO-102"),
            status_report: "working lane model",
            reviewer_feedback: "",
            updated_at: 1,
            updated_by: "user:reviewer-a",
        })
        .unwrap()
    }

    #[test]
    fn lane_records_validate_and_round_trip_through_persistence_boundary() {
        let workspace = workspace();
        let mut loom = loom();
        let lane = create_lane(&mut loom, workspace, sample_lane()).unwrap();
        assert_eq!(lane.lane_status, "working");

        let read = get_lane(&loom, workspace, "review-lane-a")
            .unwrap()
            .unwrap();
        assert_eq!(read, lane);
        assert_eq!(read.lane_tickets[0].ticket_id, "DEMO-102");
        let lane_ticket_index = loom_tickets::workflow_current_secondary_index_key(
            &workspace.to_string(),
            "lanes",
            "lane-ticket",
            "DEMO-102",
            "review-lane-a",
        )
        .unwrap();
        assert_eq!(
            loom.store()
                .mutable_overlay_secondary_index_value(&lane_ticket_index)
                .unwrap()
                .as_deref(),
            Some(&b"F"[..])
        );

        let lanes = list_lanes(&loom, workspace).unwrap();
        assert_eq!(lanes, vec![lane]);
    }

    #[test]
    fn lane_stale_index_entries_are_tombstoned_on_owner_and_membership_change() {
        let workspace = workspace();
        let mut loom = loom();
        create_lane(&mut loom, workspace, sample_lane()).unwrap();

        let mut lane = get_lane(&loom, workspace, "review-lane-a")
            .unwrap()
            .unwrap();
        lane.owner_principal = Some("user:reviewer-b".to_string());
        lane.lane_tickets.remove(0);
        lane.active_ticket_id = Some("DEMO-103".to_string());
        put_lane(&mut loom, workspace, lane).unwrap();

        let workspace_id = workspace.to_string();
        let old_owner_index = loom_tickets::workflow_current_secondary_index_key(
            &workspace_id,
            "lanes",
            "owner-principal",
            "user:reviewer-a",
            "review-lane-a",
        )
        .unwrap();
        let new_owner_index = loom_tickets::workflow_current_secondary_index_key(
            &workspace_id,
            "lanes",
            "owner-principal",
            "user:reviewer-b",
            "review-lane-a",
        )
        .unwrap();
        let old_ticket_index = loom_tickets::workflow_current_secondary_index_key(
            &workspace_id,
            "lanes",
            "lane-ticket",
            "DEMO-102",
            "review-lane-a",
        )
        .unwrap();
        let kept_ticket_index = loom_tickets::workflow_current_secondary_index_key(
            &workspace_id,
            "lanes",
            "lane-ticket",
            "DEMO-103",
            "review-lane-a",
        )
        .unwrap();
        assert_eq!(
            loom.store()
                .mutable_overlay_secondary_index_value(&old_owner_index)
                .unwrap(),
            None
        );
        assert_eq!(
            loom.store()
                .mutable_overlay_secondary_index_value(&new_owner_index)
                .unwrap()
                .as_deref(),
            Some("review-lane-a".as_bytes())
        );
        assert_eq!(
            loom.store()
                .mutable_overlay_secondary_index_value(&old_ticket_index)
                .unwrap(),
            None
        );
        assert_eq!(
            loom.store()
                .mutable_overlay_secondary_index_value(&kept_ticket_index)
                .unwrap()
                .as_deref(),
            Some(&b"V"[..])
        );
    }

    #[test]
    fn lane_title_description_kind_and_owner_round_trip() {
        let lane = sample_lane();
        assert_eq!(lane.title, "Review lane A");
        assert_eq!(lane.lane_kind, "assignment");
        assert_eq!(lane.owner_principal.as_deref(), Some("user:reviewer-a"));
        let json = lane.to_json().unwrap();
        assert_eq!(Lane::from_json(&json).unwrap(), lane);
        let cbor = lane.encode().unwrap();
        assert_eq!(Lane::decode(&cbor).unwrap(), lane);
    }

    #[test]
    fn list_partition_is_fail_soft_with_per_record_diagnostics() {
        // One valid lane document plus two undecodable ones (malformed JSON and non-UTF-8) must
        // yield the valid lane and one diagnostic per bad record, never an all-or-nothing failure.
        let good = sample_lane();
        let good_json = good.to_json().unwrap();
        let documents = vec![
            (good.lane_id.clone(), good_json.into_bytes()),
            (
                "lane-broken".to_string(),
                b"{ this is not valid lane json".to_vec(),
            ),
            ("lane-nonutf8".to_string(), vec![0xff, 0xfe, 0xfd]),
        ];
        let (lanes, diagnostics) = partition_lane_documents(documents);
        assert_eq!(lanes.len(), 1);
        assert_eq!(lanes[0].lane_id, good.lane_id);
        let bad_ids: Vec<&str> = diagnostics.iter().map(|d| d.lane_id.as_str()).collect();
        assert_eq!(bad_ids, vec!["lane-broken", "lane-nonutf8"]);
        assert!(diagnostics.iter().all(|d| !d.error.is_empty()));
    }

    #[test]
    fn lane_text_closeout_event_classifies_review_and_closeout_phrases() {
        assert_eq!(
            lane_text_closeout_event("Waiting for arbiter review: MX-1"),
            Some("review_request")
        );
        assert_eq!(
            lane_text_closeout_event("Blocked: MX-1 command run requested"),
            Some("command_requested")
        );
        assert_eq!(
            lane_text_closeout_event("feedback addressed; re-verify"),
            Some("feedback_addressed")
        );
        assert_eq!(
            lane_text_closeout_event("closeout evidence recorded"),
            Some("closeout_evidence")
        );
        assert_eq!(
            lane_text_closeout_event("blocker: dependency not accepted"),
            Some("blocker")
        );
        assert_eq!(lane_text_closeout_event("working on the next ticket"), None);
    }

    #[test]
    fn lane_validation_rejects_invalid_status_and_membership_shape() {
        let mut lane = sample_lane();
        lane.lane_status = "in_review".to_string();
        assert_eq!(lane.validate().unwrap_err().code, Code::InvalidArgument);

        let mut lane = sample_lane();
        lane.lane_tickets = vec![
            LaneTicket {
                ticket_id: "DEMO-103".to_string(),
                order_key: "V".to_string(),
            },
            LaneTicket {
                ticket_id: "DEMO-102".to_string(),
                order_key: "F".to_string(),
            },
        ];
        assert_eq!(lane.validate().unwrap_err().code, Code::InvalidArgument);

        let mut lane = sample_lane();
        lane.active_ticket_id = Some("DEMO-999".to_string());
        assert_eq!(lane.validate().unwrap_err().code, Code::InvalidArgument);
    }

    #[test]
    fn assignment_lanes_reject_duplicate_open_ticket_membership() {
        let workspace = workspace();
        let mut loom = loom();
        create_lane(&mut loom, workspace, sample_lane()).unwrap();

        let duplicate = Lane::new(LaneInput {
            lane_id: "review-lane-b",
            lane_key: "review-lane-b",
            title: "",
            description: "",
            lane_kind: LaneKind::Assignment,
            owner_principal: None,
            lane_status: LaneStatus::Ready,
            lane_tickets: &[LaneTicket {
                ticket_id: "DEMO-102".to_string(),
                order_key: "F".to_string(),
            }],
            active_ticket_id: Some("DEMO-102"),
            status_report: "",
            reviewer_feedback: "",
            updated_at: 2,
            updated_by: "user:reviewer-b",
        })
        .unwrap();
        assert_eq!(
            create_lane(&mut loom, workspace, duplicate)
                .unwrap_err()
                .code,
            Code::InvalidArgument
        );

        let tracking = Lane::new(LaneInput {
            lane_id: "watch",
            lane_key: "watch",
            title: "",
            description: "",
            lane_kind: LaneKind::Tracking,
            owner_principal: None,
            lane_status: LaneStatus::Ready,
            lane_tickets: &[LaneTicket {
                ticket_id: "DEMO-102".to_string(),
                order_key: "F".to_string(),
            }],
            active_ticket_id: Some("DEMO-102"),
            status_report: "",
            reviewer_feedback: "",
            updated_at: 3,
            updated_by: "user:reviewer-b",
        })
        .unwrap();
        create_lane(&mut loom, workspace, tracking).unwrap();
    }

    #[test]
    fn lane_display_status_derives_from_aggregate_ticket_state_with_lane_overrides() {
        let mut lane = sample_lane();
        let review_counts = lane_status_counts(&[LaneTicketView {
            ticket_id: "DEMO-1".to_string(),
            status: Some("waiting_for_review".to_string()),
            priority: None,
            title: None,
        }]);
        assert_eq!(
            lane_display_status(&lane, &review_counts),
            "review_required"
        );
        let working_counts = lane_status_counts(&[LaneTicketView {
            ticket_id: "DEMO-1".to_string(),
            status: Some("in_progress".to_string()),
            priority: None,
            title: None,
        }]);
        assert_eq!(lane_display_status(&lane, &working_counts), "working");
        assert_eq!(
            lane_display_status(&lane, &LaneStatusCounts::default()),
            "ready"
        );

        lane.lane_status = LaneStatus::Blocked.as_str().to_string();
        assert_eq!(lane_display_status(&lane, &working_counts), "working");
    }

    #[test]
    fn lane_display_status_prefers_actionable_next_work_over_blocked_count() {
        let lane = sample_lane();
        let mixed_counts = lane_status_counts(&[
            LaneTicketView {
                ticket_id: "DEMO-1".to_string(),
                status: Some("accepted".to_string()),
                priority: None,
                title: None,
            },
            LaneTicketView {
                ticket_id: "DEMO-2".to_string(),
                status: Some("ready".to_string()),
                priority: None,
                title: None,
            },
            LaneTicketView {
                ticket_id: "DEMO-3".to_string(),
                status: Some("blocked".to_string()),
                priority: None,
                title: None,
            },
            LaneTicketView {
                ticket_id: "DEMO-4".to_string(),
                status: Some("waiting_for_review".to_string()),
                priority: None,
                title: None,
            },
        ]);

        assert_eq!(mixed_counts.accepted, 1);
        assert_eq!(mixed_counts.ready, 1);
        assert_eq!(mixed_counts.blocked, 1);
        assert_eq!(mixed_counts.waiting_for_review, 1);
        assert_eq!(mixed_counts.next_ticket_id.as_deref(), Some("DEMO-2"));
        assert_eq!(lane_display_status(&lane, &mixed_counts), "ready");
    }

    #[test]
    fn lane_status_text_warnings_flag_stale_guidance_against_ticket_truth() {
        let mut lane = sample_lane();
        lane.active_ticket_id = Some("DEMO-2".to_string());
        lane.status_report = "blocked waiting on owner".to_string();
        lane.reviewer_feedback = "done".to_string();
        let warnings = lane_status_text_warnings(
            &lane,
            &[
                LaneTicketView {
                    ticket_id: "DEMO-2".to_string(),
                    status: Some("ready".to_string()),
                    priority: None,
                    title: None,
                },
                LaneTicketView {
                    ticket_id: "DEMO-3".to_string(),
                    status: Some("blocked".to_string()),
                    priority: None,
                    title: None,
                },
            ],
        );

        assert_eq!(warnings.len(), 2);
        assert_eq!(warnings[0].field, "status_report");
        assert_eq!(warnings[0].stated_status, "blocked");
        assert_eq!(warnings[0].ticket_id.as_deref(), Some("DEMO-2"));
        assert_eq!(warnings[0].ticket_status.as_deref(), Some("ready"));
        assert_eq!(warnings[1].field, "reviewer_feedback");
        assert_eq!(warnings[1].stated_status, "done");
    }

    #[test]
    fn lane_status_counts_uses_shared_ticket_status_classification() {
        let ticket_views = [
            ("DEMO-1", Some("backlog")),
            ("DEMO-2", Some("planned")),
            ("DEMO-3", Some("ready")),
            ("DEMO-4", Some("in_progress")),
            ("DEMO-5", Some("blocked")),
            ("DEMO-6", Some("waiting_for_decision")),
            ("DEMO-7", Some("feedback_available")),
            ("DEMO-8", Some("waiting_for_review")),
            ("DEMO-9", Some("accepted")),
            ("DEMO-10", Some("rejected")),
            ("DEMO-11", Some("closed")),
            ("DEMO-12", Some("not_a_status")),
            ("DEMO-13", None),
        ]
        .into_iter()
        .map(|(ticket_id, status)| LaneTicketView {
            ticket_id: ticket_id.to_string(),
            status: status.map(str::to_string),
            priority: None,
            title: None,
        })
        .collect::<Vec<_>>();

        let counts = lane_status_counts(&ticket_views);
        assert_eq!(counts.backlog, 1);
        assert_eq!(counts.planned, 1);
        assert_eq!(counts.ready, 1);
        assert_eq!(counts.in_progress, 1);
        assert_eq!(counts.blocked, 1);
        assert_eq!(counts.waiting_for_decision, 1);
        assert_eq!(counts.feedback_available, 1);
        assert_eq!(counts.waiting_for_review, 1);
        assert_eq!(counts.accepted, 1);
        assert_eq!(counts.rejected, 1);
        assert_eq!(counts.closed, 1);
        assert_eq!(counts.missing, 2);
        assert_eq!(counts.next_ticket_id.as_deref(), Some("DEMO-1"));
    }

    #[test]
    fn lane_view_compact_keeps_label_status_and_ordered_ids_only() {
        let lane = sample_lane();
        let ticket_views = vec![
            LaneTicketView {
                ticket_id: "DEMO-102".to_string(),
                status: Some("in_progress".to_string()),
                priority: Some("P1".to_string()),
                title: Some("First".to_string()),
            },
            LaneTicketView {
                ticket_id: "DEMO-103".to_string(),
                status: Some("ready".to_string()),
                priority: None,
                title: Some("Second".to_string()),
            },
        ];
        let view = lane_view(&lane, ticket_views);
        // Detailed view derives display status from the first ticket (in_progress -> working).
        assert_eq!(view.display_status, "working");

        let compact = view.compact();
        assert_eq!(compact.lane_id, view.lane_id);
        assert_eq!(compact.lane_key, view.lane_key);
        assert_eq!(compact.title, view.title);
        assert_eq!(compact.display_status, "working");
        assert!(compact.status_warnings.is_empty());
        // Compact keeps only the ordered ticket ids and drops per-ticket status/priority/title.
        assert_eq!(
            compact.lane_tickets,
            vec!["DEMO-102".to_string(), "DEMO-103".to_string()]
        );
    }

    #[test]
    fn lane_decode_rejects_unknown_fields() {
        let text = r#"{"lane_id":"review-lane-a","lane_key":"review-lane-a","title":"Review lane A","description":"Review work","lane_kind":"assignment","owner_principal":"user:reviewer-a","lane_status":"ready","lane_tickets":[],"active_ticket_id":null,"status_report":"","reviewer_feedback":"","updated_at":1,"updated_by":"user:reviewer-a","added_by":"user:reviewer-a"}"#;
        assert_eq!(
            Lane::from_json(text).unwrap_err().code,
            Code::InvalidArgument
        );
    }

    #[test]
    fn lane_decode_rejects_ad_hoc_coordination_fields() {
        // decision_id / decision_resource are ticket-level coordination detail. The Lane
        // deny_unknown_fields contract must refuse to persist them through the Lane APIs;
        // equivalent decision detail is recorded on the active ticket instead.
        for ad_hoc in ["decision_id", "decision_resource"] {
            let text = format!(
                r#"{{"lane_id":"review-lane-a","lane_key":"review-lane-a","title":"Review lane A","description":"Review work","lane_kind":"assignment","owner_principal":"user:reviewer-a","lane_status":"ready","lane_tickets":[],"active_ticket_id":null,"status_report":"","reviewer_feedback":"","updated_at":1,"updated_by":"user:reviewer-a","{ad_hoc}":"x"}}"#
            );
            assert_eq!(
                Lane::from_json(&text).unwrap_err().code,
                Code::InvalidArgument,
                "ad-hoc Lane field {ad_hoc} must be rejected by the coordination-boundary contract"
            );
        }
        // The same document without the ad-hoc field decodes cleanly.
        let ok = r#"{"lane_id":"review-lane-a","lane_key":"review-lane-a","title":"Review lane A","description":"Review work","lane_kind":"assignment","owner_principal":"user:reviewer-a","lane_status":"ready","lane_tickets":[],"active_ticket_id":null,"status_report":"","reviewer_feedback":"","updated_at":1,"updated_by":"user:reviewer-a"}"#;
        assert!(Lane::from_json(ok).is_ok());
    }

    fn ticket_ids(lane: &Lane) -> Vec<&str> {
        lane.lane_tickets
            .iter()
            .map(|lane_ticket| lane_ticket.ticket_id.as_str())
            .collect()
    }

    fn empty_lane() -> Lane {
        Lane::new(LaneInput {
            lane_id: "agent-order",
            lane_key: "agent-order",
            title: "",
            description: "",
            lane_kind: LaneKind::Assignment,
            owner_principal: None,
            lane_status: LaneStatus::Ready,
            lane_tickets: &[],
            active_ticket_id: None,
            status_report: "",
            reviewer_feedback: "",
            updated_at: 1,
            updated_by: "agent:order",
        })
        .unwrap()
    }

    #[test]
    fn parse_placement_verbs_and_anchors() {
        assert_eq!(
            LaneTicketPlacement::parse("", None).unwrap(),
            LaneTicketPlacement::Append
        );
        assert_eq!(
            LaneTicketPlacement::parse("append", None).unwrap(),
            LaneTicketPlacement::Append
        );
        assert_eq!(
            LaneTicketPlacement::parse("first", None).unwrap(),
            LaneTicketPlacement::First
        );
        assert_eq!(
            LaneTicketPlacement::parse("before", Some("A")).unwrap(),
            LaneTicketPlacement::Before("A")
        );
        assert_eq!(
            LaneTicketPlacement::parse("after", Some("A")).unwrap(),
            LaneTicketPlacement::After("A")
        );
        // before/after require a non-empty anchor.
        assert_eq!(
            LaneTicketPlacement::parse("before", None).unwrap_err().code,
            Code::InvalidArgument
        );
        assert_eq!(
            LaneTicketPlacement::parse("before", Some(""))
                .unwrap_err()
                .code,
            Code::InvalidArgument
        );
        assert_eq!(
            LaneTicketPlacement::parse("after", None).unwrap_err().code,
            Code::InvalidArgument
        );
        assert_eq!(
            LaneTicketPlacement::parse("after", Some(""))
                .unwrap_err()
                .code,
            Code::InvalidArgument
        );
        // Unknown verbs are rejected.
        assert_eq!(
            LaneTicketPlacement::parse("sideways", None)
                .unwrap_err()
                .code,
            Code::InvalidArgument
        );
    }

    #[test]
    fn place_lane_ticket_append_first_before_after_produce_expected_order() {
        let mut lane = empty_lane();
        place_lane_ticket(&mut lane, "A", LaneTicketPlacement::Append).unwrap();
        place_lane_ticket(&mut lane, "B", LaneTicketPlacement::Append).unwrap();
        assert_eq!(ticket_ids(&lane), vec!["A", "B"]);

        place_lane_ticket(&mut lane, "C", LaneTicketPlacement::First).unwrap();
        assert_eq!(ticket_ids(&lane), vec!["C", "A", "B"]);

        place_lane_ticket(&mut lane, "D", LaneTicketPlacement::Before("A")).unwrap();
        assert_eq!(ticket_ids(&lane), vec!["C", "D", "A", "B"]);

        place_lane_ticket(&mut lane, "E", LaneTicketPlacement::After("A")).unwrap();
        assert_eq!(ticket_ids(&lane), vec!["C", "D", "A", "E", "B"]);

        // Order keys stay strictly increasing so the lane re-validates.
        lane.validate().unwrap();
    }

    #[test]
    fn place_lane_ticket_unknown_anchor_is_not_found() {
        let mut lane = empty_lane();
        place_lane_ticket(&mut lane, "A", LaneTicketPlacement::Append).unwrap();
        assert_eq!(
            place_lane_ticket(&mut lane, "B", LaneTicketPlacement::Before("Z"))
                .unwrap_err()
                .code,
            Code::NotFound
        );
        assert_eq!(
            place_lane_ticket(&mut lane, "B", LaneTicketPlacement::After("Z"))
                .unwrap_err()
                .code,
            Code::NotFound
        );
    }

    #[test]
    fn place_lane_ticket_rejects_duplicate_ticket_id() {
        let mut lane = empty_lane();
        place_lane_ticket(&mut lane, "A", LaneTicketPlacement::Append).unwrap();
        assert_eq!(
            place_lane_ticket(&mut lane, "A", LaneTicketPlacement::Append)
                .unwrap_err()
                .code,
            Code::InvalidArgument
        );
    }

    #[test]
    fn remove_preserves_order_and_reorder_by_ids_works() {
        let mut lane = empty_lane();
        for id in ["A", "B", "C"] {
            place_lane_ticket(&mut lane, id, LaneTicketPlacement::Append).unwrap();
        }
        lane.lane_tickets
            .retain(|lane_ticket| lane_ticket.ticket_id != "B");
        assert_eq!(ticket_ids(&lane), vec!["A", "C"]);
        lane.validate().unwrap();

        replace_lane_ticket_order(
            &mut lane,
            &["C".to_string(), "A".to_string(), "B".to_string()],
        )
        .unwrap();
        assert_eq!(ticket_ids(&lane), vec!["C", "A", "B"]);
        lane.validate().unwrap();
    }

    fn ticket_view(ticket_id: &str, status: &str) -> LaneTicketView {
        LaneTicketView {
            ticket_id: ticket_id.to_string(),
            status: Some(status.to_string()),
            priority: None,
            title: None,
        }
    }

    #[test]
    fn terminal_lane_ticket_ids_picks_terminal_members_in_order() {
        let views = vec![
            ticket_view("A", "accepted"),
            ticket_view("B", "in_progress"),
            ticket_view("C", "closed"),
            ticket_view("D", "rejected"),
            ticket_view("E", "ready"),
        ];
        // Accepted / Closed / Rejected are terminal; the rest are active/queued work.
        assert_eq!(
            terminal_lane_ticket_ids(&views),
            vec!["A".to_string(), "C".to_string(), "D".to_string()]
        );
        assert!(is_terminal_ticket_status(Some("accepted")));
        assert!(is_terminal_ticket_status(Some("rejected")));
        assert!(is_terminal_ticket_status(Some("closed")));
        assert!(!is_terminal_ticket_status(Some("in_progress")));
        assert!(!is_terminal_ticket_status(Some("ready")));
        assert!(!is_terminal_ticket_status(None));
    }

    #[test]
    fn terminal_lane_ticket_ids_dry_run_preview_does_not_mutate_membership() {
        let mut lane = empty_lane();
        for id in ["A", "B", "C"] {
            place_lane_ticket(&mut lane, id, LaneTicketPlacement::Append).unwrap();
        }
        let before = ticket_ids(&lane)
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>();
        let views = vec![
            ticket_view("A", "accepted"),
            ticket_view("B", "in_progress"),
            ticket_view("C", "closed"),
        ];
        // Computing the preview must leave lane membership untouched.
        let preview = terminal_lane_ticket_ids(&views);
        assert_eq!(preview, vec!["A".to_string(), "C".to_string()]);
        assert_eq!(
            ticket_ids(&lane)
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>(),
            before
        );
    }

    #[test]
    fn remove_lane_tickets_preserves_remaining_order_and_clears_active_when_removed() {
        let mut lane = empty_lane();
        for id in ["A", "B", "C", "D"] {
            place_lane_ticket(&mut lane, id, LaneTicketPlacement::Append).unwrap();
        }
        lane.active_ticket_id = Some("C".to_string());
        lane.validate().unwrap();

        // Remove two members (one of which is the active ticket) plus one id that is not present.
        let removed = remove_lane_tickets(
            &mut lane,
            &["A".to_string(), "C".to_string(), "Z".to_string()],
        );
        // Only the ids actually present are reported removed, in current order.
        assert_eq!(removed, vec!["A".to_string(), "C".to_string()]);
        // Remaining membership keeps its relative order and re-validates (monotonic order keys).
        assert_eq!(ticket_ids(&lane), vec!["B", "D"]);
        lane.validate().unwrap();
        // The active ticket was removed, so it is cleared.
        assert_eq!(lane.active_ticket_id, None);
    }

    #[test]
    fn remove_lane_tickets_preserves_non_terminal_and_active_when_untouched() {
        let mut lane = empty_lane();
        for id in ["A", "B", "C"] {
            place_lane_ticket(&mut lane, id, LaneTicketPlacement::Append).unwrap();
        }
        lane.active_ticket_id = Some("B".to_string());
        lane.validate().unwrap();

        // Terminal ids computed from resolved statuses: only "A" is terminal here.
        let views = vec![
            ticket_view("A", "closed"),
            ticket_view("B", "in_progress"),
            ticket_view("C", "ready"),
        ];
        let terminal = terminal_lane_ticket_ids(&views);
        let removed = remove_lane_tickets(&mut lane, &terminal);
        assert_eq!(removed, vec!["A".to_string()]);
        // Non-terminal members are preserved and the still-present active ticket is retained.
        assert_eq!(ticket_ids(&lane), vec!["B", "C"]);
        assert_eq!(lane.active_ticket_id.as_deref(), Some("B"));
        lane.validate().unwrap();
    }

    #[test]
    fn remove_lane_tickets_on_closed_lane_still_edits_membership() {
        let mut lane = empty_lane();
        for id in ["A", "B"] {
            place_lane_ticket(&mut lane, id, LaneTicketPlacement::Append).unwrap();
        }
        lane.lane_status = LaneStatus::Closed.as_str().to_string();
        lane.validate().unwrap();

        let removed = remove_lane_tickets(&mut lane, &["A".to_string()]);
        assert_eq!(removed, vec!["A".to_string()]);
        // Closed lanes still have their membership cleaned; the lane record stays valid.
        assert_eq!(ticket_ids(&lane), vec!["B"]);
        assert_eq!(lane.lane_status, "closed");
        lane.validate().unwrap();
    }
}
