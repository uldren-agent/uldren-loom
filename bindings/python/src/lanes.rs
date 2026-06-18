//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;
use loom_lanes::{Lane, LaneStatus};

fn mutate_lane<F>(
    path: &str,
    workspace: &str,
    lane_id: &str,
    passphrase: Option<&str>,
    mutate: F,
) -> PyResult<Vec<u8>>
where
    F: FnOnce(&mut Lane) -> PyResult<()>,
{
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = ensure_lanes_ns(&mut loom, workspace)?;
    let mut lane = loom_lanes::get_lane(&loom, ns, lane_id)
        .map_err(py_err)?
        .ok_or_else(|| PyRuntimeError::new_err("NOT_FOUND: lane not found"))?;
    mutate(&mut lane)?;
    let lane = loom_lanes::put_lane(&mut loom, ns, lane).map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    lane.encode().map_err(py_err)
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

#[pyfunction]
#[pyo3(signature = (path, workspace, lane, passphrase=None))]
pub(crate) fn lanes_create<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    lane: &[u8],
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let lane = Lane::decode(lane).map_err(py_err)?;
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = ensure_lanes_ns(&mut loom, workspace)?;
    let lane = loom_lanes::create_lane(&mut loom, ns, lane).map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(PyBytes::new(py, &lane.encode().map_err(py_err)?))
}

#[pyfunction]
#[pyo3(signature = (path, workspace, lane_id, passphrase=None))]
pub(crate) fn lanes_get<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    lane_id: &str,
    passphrase: Option<&str>,
) -> PyResult<Option<Bound<'py, PyBytes>>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    loom_lanes::get_lane(&loom, ns, lane_id)
        .map_err(py_err)?
        .map(|lane| {
            lane.encode()
                .map(|bytes| PyBytes::new(py, &bytes))
                .map_err(py_err)
        })
        .transpose()
}

#[pyfunction]
#[pyo3(signature = (path, workspace, passphrase=None))]
pub(crate) fn lanes_list<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let lanes = loom_lanes::list_lanes(&loom, ns)
        .map_err(py_err)?
        .into_iter()
        .map(|lane| lane.encode().map(CborValue::Bytes).map_err(py_err))
        .collect::<PyResult<Vec<_>>>()?;
    let bytes = cbor_encode(&CborValue::Array(lanes))
        .map_err(|e| PyRuntimeError::new_err(format!("cbor: {e}")))?;
    Ok(PyBytes::new(py, &bytes))
}

#[pyfunction]
#[pyo3(signature = (path, workspace, lane_id, title=None, description=None, lane_status=None, status_report=None, reviewer_feedback=None, updated_by="", passphrase=None))]
pub(crate) fn lanes_update<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    lane_id: &str,
    title: Option<&str>,
    description: Option<&str>,
    lane_status: Option<&str>,
    status_report: Option<&str>,
    reviewer_feedback: Option<&str>,
    updated_by: &str,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    if title.is_none()
        && description.is_none()
        && lane_status.is_none()
        && status_report.is_none()
        && reviewer_feedback.is_none()
    {
        return Err(PyRuntimeError::new_err(
            "lane update requires at least one field",
        ));
    }
    let bytes = mutate_lane(path, workspace, lane_id, passphrase, |lane| {
        if let Some(title) = title {
            lane.title = title.to_string();
        }
        if let Some(description) = description {
            lane.description = description.to_string();
        }
        if let Some(lane_status) = lane_status {
            lane.lane_status = LaneStatus::parse(lane_status)
                .map_err(py_err)?
                .as_str()
                .to_string();
        }
        if let Some(status_report) = status_report {
            lane.status_report = status_report.to_string();
        }
        if let Some(reviewer_feedback) = reviewer_feedback {
            lane.reviewer_feedback = reviewer_feedback.to_string();
        }
        update_metadata(lane, updated_by);
        Ok(())
    })?;
    Ok(PyBytes::new(py, &bytes))
}

#[pyfunction]
#[pyo3(signature = (path, workspace, lane_id, ticket_id, updated_by, placement="append", anchor=None, passphrase=None))]
pub(crate) fn lanes_ticket_add<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    lane_id: &str,
    ticket_id: &str,
    updated_by: &str,
    placement: &str,
    anchor: Option<&str>,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let bytes = mutate_lane(path, workspace, lane_id, passphrase, |lane| {
        let placement = loom_lanes::LaneTicketPlacement::parse(placement, anchor).map_err(py_err)?;
        loom_lanes::place_lane_ticket(lane, ticket_id, placement).map_err(py_err)?;
        update_metadata(lane, updated_by);
        Ok(())
    })?;
    Ok(PyBytes::new(py, &bytes))
}

#[pyfunction]
#[pyo3(signature = (path, workspace, lane_id, ticket_id, updated_by, passphrase=None))]
pub(crate) fn lanes_ticket_remove<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    lane_id: &str,
    ticket_id: &str,
    updated_by: &str,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let bytes = mutate_lane(path, workspace, lane_id, passphrase, |lane| {
        lane.lane_tickets
            .retain(|lane_ticket| lane_ticket.ticket_id != ticket_id);
        if lane.active_ticket_id.as_deref() == Some(ticket_id) {
            lane.active_ticket_id = None;
        }
        update_metadata(lane, updated_by);
        Ok(())
    })?;
    Ok(PyBytes::new(py, &bytes))
}

#[pyfunction]
#[pyo3(signature = (path, workspace, source_lane_id, target_lane_id, ticket_id, updated_by, passphrase=None))]
pub(crate) fn lanes_ticket_transfer<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    source_lane_id: &str,
    target_lane_id: &str,
    ticket_id: &str,
    updated_by: &str,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = ensure_lanes_ns(&mut loom, workspace)?;
    let (_, target) = loom_lanes::transfer_assignment_lane_ticket(
        &mut loom,
        ns,
        source_lane_id,
        target_lane_id,
        ticket_id,
        now_ms(),
        updated_by,
    )
    .map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(PyBytes::new(py, &target.encode().map_err(py_err)?))
}

#[pyfunction]
#[pyo3(signature = (path, workspace, lane_id, updated_by, passphrase=None))]
pub(crate) fn lanes_delete<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    lane_id: &str,
    updated_by: &str,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = ensure_lanes_ns(&mut loom, workspace)?;
    let lane =
        loom_lanes::delete_lane(&mut loom, ns, lane_id, now_ms(), updated_by).map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(PyBytes::new(py, &lane.encode().map_err(py_err)?))
}
