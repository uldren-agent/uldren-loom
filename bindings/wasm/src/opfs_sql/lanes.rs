use loom_codec::{Value as CborValue, encode as cbor_encode};
use loom_lanes::{Lane, LaneStatus};
use wasm_bindgen::prelude::*;

use super::{LoomStore, le, resolve_workspace_arg, save_loom};

fn update_metadata(lane: &mut Lane, updated_by: &str) {
    lane.updated_by = updated_by.to_string();
    lane.updated_at = js_sys::Date::now() as u64;
}

#[wasm_bindgen]
impl LoomStore {
    pub fn lanes_create(&mut self, workspace: String, lane: Vec<u8>) -> Result<Vec<u8>, JsError> {
        let lane = Lane::decode(&lane).map_err(le)?;
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let lane = loom_lanes::create_lane(&mut self.loom, ns, lane).map_err(le)?;
        save_loom(&mut self.loom).map_err(le)?;
        lane.encode().map_err(le)
    }

    pub fn lanes_get(
        &self,
        workspace: String,
        lane_id: String,
    ) -> Result<Option<Vec<u8>>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        loom_lanes::get_lane(&self.loom, ns, &lane_id)
            .map_err(le)?
            .map(|lane| lane.encode().map_err(le))
            .transpose()
    }

    pub fn lanes_list(&self, workspace: String) -> Result<Vec<u8>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let lanes = loom_lanes::list_lanes(&self.loom, ns)
            .map_err(le)?
            .into_iter()
            .map(|lane| lane.encode().map(CborValue::Bytes).map_err(le))
            .collect::<Result<Vec<_>, _>>()?;
        cbor_encode(&CborValue::Array(lanes)).map_err(|e| JsError::new(&format!("cbor: {e}")))
    }

    pub fn lanes_update(
        &mut self,
        workspace: String,
        lane_id: String,
        title: Option<String>,
        description: Option<String>,
        lane_status: Option<String>,
        status_report: Option<String>,
        reviewer_feedback: Option<String>,
        updated_by: String,
    ) -> Result<Vec<u8>, JsError> {
        if title.is_none()
            && description.is_none()
            && lane_status.is_none()
            && status_report.is_none()
            && reviewer_feedback.is_none()
        {
            return Err(JsError::new("lane update requires at least one field"));
        }
        self.mutate_lane(workspace, lane_id, |lane| {
            if let Some(title) = title {
                lane.title = title;
            }
            if let Some(description) = description {
                lane.description = description;
            }
            if let Some(lane_status) = lane_status {
                lane.lane_status = LaneStatus::parse(&lane_status).map_err(le)?.as_str().to_string();
            }
            if let Some(status_report) = status_report {
                lane.status_report = status_report;
            }
            if let Some(reviewer_feedback) = reviewer_feedback {
                lane.reviewer_feedback = reviewer_feedback;
            }
            update_metadata(lane, &updated_by);
            Ok(())
        })
    }

    pub fn lanes_ticket_add(
        &mut self,
        workspace: String,
        lane_id: String,
        ticket_id: String,
        updated_by: String,
        placement: String,
        anchor: Option<String>,
    ) -> Result<Vec<u8>, JsError> {
        self.mutate_lane(workspace, lane_id, |lane| {
            let placement = loom_lanes::LaneTicketPlacement::parse(&placement, anchor.as_deref())
                .map_err(le)?;
            loom_lanes::place_lane_ticket(lane, &ticket_id, placement).map_err(le)?;
            update_metadata(lane, &updated_by);
            Ok(())
        })
    }

    pub fn lanes_ticket_remove(
        &mut self,
        workspace: String,
        lane_id: String,
        ticket_id: String,
        updated_by: String,
    ) -> Result<Vec<u8>, JsError> {
        self.mutate_lane(workspace, lane_id, |lane| {
            lane.lane_tickets
                .retain(|lane_ticket| lane_ticket.ticket_id != ticket_id);
            if lane.active_ticket_id.as_deref() == Some(ticket_id.as_str()) {
                lane.active_ticket_id = None;
            }
            update_metadata(lane, &updated_by);
            Ok(())
        })
    }

}

impl LoomStore {
    fn mutate_lane<F>(
        &mut self,
        workspace: String,
        lane_id: String,
        mutate: F,
    ) -> Result<Vec<u8>, JsError>
    where
        F: FnOnce(&mut Lane) -> Result<(), JsError>,
    {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let mut lane = loom_lanes::get_lane(&self.loom, ns, &lane_id)
            .map_err(le)?
            .ok_or_else(|| JsError::new("NOT_FOUND: lane not found"))?;
        mutate(&mut lane)?;
        let lane = loom_lanes::put_lane(&mut self.loom, ns, lane).map_err(le)?;
        save_loom(&mut self.loom).map_err(le)?;
        lane.encode().map_err(le)
    }
}
