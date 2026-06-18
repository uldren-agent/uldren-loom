use loom_core::Loom;
use loom_core::error::{Code, LoomError, Result};
use loom_core::workspace::WorkspaceId;
use loom_lanes::{Lane, LaneInput, LaneKind, LaneStatus, LaneTicket, LaneTicketPlacement};
use loom_store::FileStore;

pub struct HostedLaneCreate<'a> {
    pub lane_id: &'a str,
    pub lane_key: &'a str,
    pub title: &'a str,
    pub description: &'a str,
    pub lane_kind: &'a str,
    pub owner_principal: Option<&'a str>,
    pub lane_status: &'a str,
    pub lane_tickets: &'a [LaneTicket],
    pub active_ticket_id: Option<&'a str>,
    pub status_report: &'a str,
    pub reviewer_feedback: &'a str,
    pub updated_by: &'a str,
}

pub struct HostedLaneTicketUpdate<'a> {
    pub lane_id: &'a str,
    pub ticket_id: &'a str,
    /// where the ticket lands. Ignored by remove; defaults to append for add callers.
    pub placement: LaneTicketPlacement<'a>,
    pub updated_by: &'a str,
}

pub struct HostedLaneUpdate<'a> {
    pub lane_id: &'a str,
    pub title: Option<&'a str>,
    pub description: Option<&'a str>,
    pub lane_status: Option<&'a str>,
    pub status_report: Option<&'a str>,
    pub reviewer_feedback: Option<&'a str>,
    pub updated_by: &'a str,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

pub fn create(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    input: HostedLaneCreate<'_>,
) -> Result<Lane> {
    let lane = Lane::new(LaneInput {
        lane_id: input.lane_id,
        lane_key: input.lane_key,
        title: input.title,
        description: input.description,
        lane_kind: LaneKind::parse(input.lane_kind)?,
        owner_principal: input.owner_principal,
        lane_status: LaneStatus::parse(input.lane_status)?,
        lane_tickets: input.lane_tickets,
        active_ticket_id: input.active_ticket_id,
        status_report: input.status_report,
        reviewer_feedback: input.reviewer_feedback,
        updated_at: now_ms(),
        updated_by: input.updated_by,
    })?;
    let lane = loom_lanes::create_lane(loom, workspace, lane)?;
    loom_lanes::emit_lane_change_notification(
        loom,
        workspace,
        &workspace.to_string(),
        &lane,
        "lane.created",
    )?;
    Ok(lane)
}

pub fn get(loom: &Loom<FileStore>, workspace: WorkspaceId, lane_id: &str) -> Result<Option<Lane>> {
    loom_lanes::get_lane(loom, workspace, lane_id)
}

pub fn list(loom: &Loom<FileStore>, workspace: WorkspaceId) -> Result<Vec<Lane>> {
    loom_lanes::list_lanes(loom, workspace)
}

pub fn update(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    input: HostedLaneUpdate<'_>,
) -> Result<Lane> {
    if input.title.is_none()
        && input.description.is_none()
        && input.lane_status.is_none()
        && input.status_report.is_none()
        && input.reviewer_feedback.is_none()
    {
        return Err(LoomError::invalid(
            "lane update requires at least one field",
        ));
    }
    mutate(loom, workspace, input.lane_id, "lane.updated", |lane| {
        if let Some(title) = input.title {
            lane.title = title.to_string();
        }
        if let Some(description) = input.description {
            lane.description = description.to_string();
        }
        if let Some(lane_status) = input.lane_status {
            lane.lane_status = LaneStatus::parse(lane_status)?.as_str().to_string();
        }
        if let Some(status_report) = input.status_report {
            lane.status_report = status_report.to_string();
        }
        if let Some(reviewer_feedback) = input.reviewer_feedback {
            lane.reviewer_feedback = reviewer_feedback.to_string();
        }
        update_metadata(lane, input.updated_by);
        Ok(())
    })
}

pub fn add_ticket(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    input: HostedLaneTicketUpdate<'_>,
) -> Result<Lane> {
    mutate(
        loom,
        workspace,
        input.lane_id,
        "lane.ticket_added",
        |lane| {
            loom_lanes::place_lane_ticket(lane, input.ticket_id, input.placement)?;
            update_metadata(lane, input.updated_by);
            Ok(())
        },
    )
}

pub fn remove_ticket(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    input: HostedLaneTicketUpdate<'_>,
) -> Result<Lane> {
    mutate(
        loom,
        workspace,
        input.lane_id,
        "lane.ticket_removed",
        |lane| {
            lane.lane_tickets
                .retain(|lane_ticket| lane_ticket.ticket_id != input.ticket_id);
            if lane.active_ticket_id.as_deref() == Some(input.ticket_id) {
                lane.active_ticket_id = None;
            }
            update_metadata(lane, input.updated_by);
            Ok(())
        },
    )
}

pub fn transfer_ticket(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    source_lane_id: &str,
    target_lane_id: &str,
    ticket_id: &str,
    updated_by: &str,
) -> Result<Lane> {
    let (_, target) = loom_lanes::transfer_assignment_lane_ticket(
        loom,
        workspace,
        source_lane_id,
        target_lane_id,
        ticket_id,
        now_ms(),
        updated_by,
    )?;
    loom_lanes::emit_lane_change_notification(
        loom,
        workspace,
        &workspace.to_string(),
        &target,
        "lane.ticket_transferred",
    )?;
    Ok(target)
}

pub fn delete(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    lane_id: &str,
    updated_by: &str,
) -> Result<Lane> {
    loom_lanes::delete_lane(loom, workspace, lane_id, now_ms(), updated_by)
}

fn mutate<F>(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    lane_id: &str,
    event_kind: &str,
    mutation: F,
) -> Result<Lane>
where
    F: FnOnce(&mut Lane) -> Result<()>,
{
    let mut lane = loom_lanes::get_lane(loom, workspace, lane_id)?
        .ok_or_else(|| LoomError::new(Code::NotFound, "lane not found"))?;
    mutation(&mut lane)?;
    let lane = loom_lanes::put_lane(loom, workspace, lane)?;
    loom_lanes::emit_lane_change_notification(
        loom,
        workspace,
        &workspace.to_string(),
        &lane,
        event_kind,
    )?;
    Ok(lane)
}

fn update_metadata(lane: &mut Lane, updated_by: &str) {
    lane.updated_at = now_ms();
    lane.updated_by = updated_by.to_string();
}
