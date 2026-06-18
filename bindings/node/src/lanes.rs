//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;
use loom_lanes::{Lane, LaneStatus};

fn mutate_lane<F>(
    loom_path: &str,
    workspace: &str,
    lane_id: &str,
    passphrase: Option<&str>,
    mutate: F,
) -> napi::Result<Uint8Array>
where
    F: FnOnce(&mut Lane) -> napi::Result<()>,
{
    let mut loom = open_loom_unlocked(loom_path, key_spec(passphrase).as_ref()).map_err(reason)?;
    let ns = ensure_lanes_ns(&mut loom, workspace)?;
    let mut lane = loom_lanes::get_lane(&loom, ns, lane_id)
        .map_err(reason)?
        .ok_or_else(|| napi::Error::from_reason("NOT_FOUND: lane not found".to_string()))?;
    mutate(&mut lane)?;
    let lane = loom_lanes::put_lane(&mut loom, ns, lane).map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(Uint8Array::from(lane.encode().map_err(reason)?))
}

fn update_metadata(lane: &mut Lane, updated_by: &str) {
    lane.updated_by = updated_by.to_string();
    lane.updated_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX);
}

#[napi]
pub fn lanes_create(
    loom_path: String,
    workspace: String,
    lane: Uint8Array,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let lane = Lane::decode(&lane).map_err(reason)?;
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = ensure_lanes_ns(&mut loom, &workspace)?;
    let lane = loom_lanes::create_lane(&mut loom, ns, lane).map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(Uint8Array::from(lane.encode().map_err(reason)?))
}

#[napi]
pub fn lanes_get(
    loom_path: String,
    workspace: String,
    lane_id: String,
    passphrase: Option<String>,
) -> napi::Result<Option<Uint8Array>> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    loom_lanes::get_lane(&loom, ns, &lane_id)
        .map_err(reason)?
        .map(|lane| lane.encode().map(Uint8Array::from).map_err(reason))
        .transpose()
}

#[napi]
pub fn lanes_list(
    loom_path: String,
    workspace: String,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let lanes = loom_lanes::list_lanes(&loom, ns)
        .map_err(reason)?
        .into_iter()
        .map(|lane| lane.encode().map(CborValue::Bytes).map_err(reason))
        .collect::<napi::Result<Vec<_>>>()?;
    let bytes = cbor_encode(&CborValue::Array(lanes))
        .map_err(|e| napi::Error::from_reason(format!("cbor: {e}")))?;
    Ok(Uint8Array::from(bytes))
}

#[napi]
pub fn lanes_update(
    loom_path: String,
    workspace: String,
    lane_id: String,
    title: Option<String>,
    description: Option<String>,
    lane_status: Option<String>,
    status_report: Option<String>,
    reviewer_feedback: Option<String>,
    updated_by: String,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    if title.is_none()
        && description.is_none()
        && lane_status.is_none()
        && status_report.is_none()
        && reviewer_feedback.is_none()
    {
        return Err(napi::Error::from_reason(
            "lane update requires at least one field".to_string(),
        ));
    }
    mutate_lane(
        &loom_path,
        &workspace,
        &lane_id,
        passphrase.as_deref(),
        |lane| {
            if let Some(title) = title {
                lane.title = title;
            }
            if let Some(description) = description {
                lane.description = description;
            }
            if let Some(lane_status) = lane_status {
                lane.lane_status = LaneStatus::parse(&lane_status).map_err(reason)?.as_str().to_string();
            }
            if let Some(status_report) = status_report {
                lane.status_report = status_report;
            }
            if let Some(reviewer_feedback) = reviewer_feedback {
                lane.reviewer_feedback = reviewer_feedback;
            }
            update_metadata(lane, &updated_by);
            Ok(())
        },
    )
}

#[napi]
pub fn lanes_ticket_add(
    loom_path: String,
    workspace: String,
    lane_id: String,
    ticket_id: String,
    updated_by: String,
    placement: String,
    anchor: Option<String>,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    mutate_lane(
        &loom_path,
        &workspace,
        &lane_id,
        passphrase.as_deref(),
        |lane| {
            let placement = loom_lanes::LaneTicketPlacement::parse(&placement, anchor.as_deref())
                .map_err(reason)?;
            loom_lanes::place_lane_ticket(lane, &ticket_id, placement).map_err(reason)?;
            update_metadata(lane, &updated_by);
            Ok(())
        },
    )
}

#[napi]
pub fn lanes_ticket_remove(
    loom_path: String,
    workspace: String,
    lane_id: String,
    ticket_id: String,
    updated_by: String,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    mutate_lane(
        &loom_path,
        &workspace,
        &lane_id,
        passphrase.as_deref(),
        |lane| {
            lane.lane_tickets
                .retain(|lane_ticket| lane_ticket.ticket_id != ticket_id);
            if lane.active_ticket_id.as_deref() == Some(&ticket_id) {
                lane.active_ticket_id = None;
            }
            update_metadata(lane, &updated_by);
            Ok(())
        },
    )
}

#[napi]
pub fn lanes_ticket_transfer(
    loom_path: String,
    workspace: String,
    source_lane_id: String,
    target_lane_id: String,
    ticket_id: String,
    updated_by: String,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = ensure_lanes_ns(&mut loom, &workspace)?;
    let (_, target) = loom_lanes::transfer_assignment_lane_ticket(
        &mut loom,
        ns,
        &source_lane_id,
        &target_lane_id,
        &ticket_id,
        now_ms(),
        &updated_by,
    )
    .map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(Uint8Array::from(target.encode().map_err(reason)?))
}

#[napi]
pub fn lanes_delete(
    loom_path: String,
    workspace: String,
    lane_id: String,
    updated_by: String,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = ensure_lanes_ns(&mut loom, &workspace)?;
    let lane = loom_lanes::delete_lane(&mut loom, ns, &lane_id, now_ms(), &updated_by)
        .map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(Uint8Array::from(lane.encode().map_err(reason)?))
}
