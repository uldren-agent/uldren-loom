#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TicketStatusClass {
    Backlog,
    Planned,
    Ready,
    InProgress,
    Blocked,
    WaitingForDecision,
    FeedbackAvailable,
    WaitingForReview,
    Accepted,
    Rejected,
    Closed,
    Missing,
}

impl TicketStatusClass {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Backlog => "backlog",
            Self::Planned => "planned",
            Self::Ready => "ready",
            Self::InProgress => "in_progress",
            Self::Blocked => "blocked",
            Self::WaitingForDecision => "waiting_for_decision",
            Self::FeedbackAvailable => "feedback_available",
            Self::WaitingForReview => "waiting_for_review",
            Self::Accepted => "accepted",
            Self::Rejected => "rejected",
            Self::Closed => "closed",
            Self::Missing => "missing",
        }
    }
}

pub const NORMALIZED_TICKET_STATUSES: [&str; 11] = [
    "backlog",
    "planned",
    "ready",
    "in_progress",
    "blocked",
    "waiting_for_decision",
    "feedback_available",
    "waiting_for_review",
    "accepted",
    "rejected",
    "closed",
];

pub const LANE_TICKET_STATUS_COUNT_FIELDS: [&str; 12] = [
    "blocked",
    "waiting_for_decision",
    "feedback_available",
    "waiting_for_review",
    "in_progress",
    "backlog",
    "planned",
    "ready",
    "accepted",
    "rejected",
    "closed",
    "missing",
];

pub fn classify_ticket_status(status: Option<&str>) -> TicketStatusClass {
    match status {
        Some("backlog") => TicketStatusClass::Backlog,
        Some("planned") => TicketStatusClass::Planned,
        Some("ready") => TicketStatusClass::Ready,
        Some("in_progress") | Some("working") => TicketStatusClass::InProgress,
        Some("blocked") => TicketStatusClass::Blocked,
        Some("waiting_for_decision") | Some("awaiting_decision") => {
            TicketStatusClass::WaitingForDecision
        }
        Some("feedback_available") => TicketStatusClass::FeedbackAvailable,
        Some("waiting_for_review") | Some("review_required") => TicketStatusClass::WaitingForReview,
        Some("accepted") => TicketStatusClass::Accepted,
        Some("rejected") => TicketStatusClass::Rejected,
        Some("closed") => TicketStatusClass::Closed,
        Some("missing") | None => TicketStatusClass::Missing,
        Some(_) => TicketStatusClass::Missing,
    }
}
