//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;
use loom_lanes::{Lane, LaneStatus};

fn ensure_lanes_ns(loom: &mut Loom<FileStore>, workspace: &str) -> LoomResult<WorkspaceId> {
    let selector = match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Typed {
            ty: FacetKind::Document,
            name: workspace.to_string(),
        },
    };
    let ns = loom
        .registry_mut()
        .ensure_for_write(&selector, random_workspace_id()?)?;
    loom.registry_mut().add_facet(ns, FacetKind::Document)?;
    Ok(ns)
}

fn lanes_create_ns(h: &LoomSession, workspace: &str, lane_bytes: &[u8]) -> LoomResult<Vec<u8>> {
    let lane = Lane::decode(lane_bytes)?;
    let mut loom = open_h_write(h)?;
    let ns = ensure_lanes_ns(&mut loom, workspace)?;
    let lane = loom_lanes::create_lane(&mut loom, ns, lane)?;
    save_loom(&mut loom)?;
    lane.encode()
}

fn lanes_get_ns(h: &LoomSession, workspace: &str, lane_id: &str) -> LoomResult<Option<Vec<u8>>> {
    let loom = open_h_read(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    loom_lanes::get_lane(&loom, ns, lane_id)?
        .map(|lane| lane.encode())
        .transpose()
}

fn lanes_list_cbor_ns(h: &LoomSession, workspace: &str) -> LoomResult<Vec<u8>> {
    let loom = open_h_read(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let lane_values = loom_lanes::list_lanes(&loom, ns)?
        .iter()
        .map(|lane| lane.encode().map(CborValue::Bytes))
        .collect::<LoomResult<Vec<_>>>()?;
    cbor_encode(&CborValue::Array(lane_values))
        .map_err(|error| LoomError::invalid(format!("lane list cbor encode failed: {error:?}")))
}

fn lanes_mutate_ns<F>(
    h: &LoomSession,
    workspace: &str,
    lane_id: &str,
    mutate: F,
) -> LoomResult<Vec<u8>>
where
    F: FnOnce(&mut Lane) -> LoomResult<()>,
{
    let mut loom = open_h_write(h)?;
    let ns = ensure_lanes_ns(&mut loom, workspace)?;
    let mut lane = loom_lanes::get_lane(&loom, ns, lane_id)?
        .ok_or_else(|| LoomError::new(Code::NotFound, "lane not found"))?;
    mutate(&mut lane)?;
    let lane = loom_lanes::put_lane(&mut loom, ns, lane)?;
    save_loom(&mut loom)?;
    lane.encode()
}

fn lanes_ticket_transfer_ns(
    h: &LoomSession,
    workspace: &str,
    source_lane_id: &str,
    target_lane_id: &str,
    ticket_id: &str,
    updated_by: &str,
) -> LoomResult<Vec<u8>> {
    let mut loom = open_h_write(h)?;
    let ns = ensure_lanes_ns(&mut loom, workspace)?;
    let (_, target) = loom_lanes::transfer_assignment_lane_ticket(
        &mut loom,
        ns,
        source_lane_id,
        target_lane_id,
        ticket_id,
        now_ms(),
        updated_by,
    )?;
    save_loom(&mut loom)?;
    target.encode()
}

fn lanes_delete_ns(
    h: &LoomSession,
    workspace: &str,
    lane_id: &str,
    updated_by: &str,
) -> LoomResult<Vec<u8>> {
    let mut loom = open_h_write(h)?;
    let ns = ensure_lanes_ns(&mut loom, workspace)?;
    let lane = loom_lanes::delete_lane(&mut loom, ns, lane_id, now_ms(), updated_by)?;
    save_loom(&mut loom)?;
    lane.encode()
}

fn update_lane_metadata(lane: &mut Lane, updated_by: &str) {
    lane.updated_by = updated_by.to_string();
    lane.updated_at = now_ms();
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn ffi_ticket_text_field(ticket: &loom_tickets::TicketSummary, field: &str) -> Option<String> {
    match ticket.fields.get(field)? {
        serde_json::Value::String(value) => Some(value.clone()),
        serde_json::Value::Object(map) => map
            .get("String")
            .or_else(|| map.get("Text"))
            .or_else(|| map.get("EnumOption"))
            .or_else(|| map.get("Principal"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
        _ => None,
    }
}

fn ffi_ticket_status(ticket: &loom_tickets::TicketSummary) -> String {
    ffi_ticket_text_field(ticket, "status").unwrap_or_else(|| "unknown".to_string())
}

fn ffi_ticket_title(ticket: &loom_tickets::TicketSummary) -> String {
    ffi_ticket_text_field(ticket, "title")
        .or_else(|| ffi_ticket_text_field(ticket, "summary"))
        .unwrap_or_default()
}

fn build_lane_view_ffi(
    loom: &Loom<FileStore>,
    ns: WorkspaceId,
    ticket_workspace_id: &str,
    lane: &Lane,
) -> loom_lanes::LaneView {
    let lane_tickets = lane
        .lane_tickets
        .iter()
        .map(|lane_ticket| {
            let ticket =
                loom_tickets::get_ticket(loom, ns, ticket_workspace_id, &lane_ticket.ticket_id)
                    .ok()
                    .flatten();
            loom_lanes::LaneTicketView {
                ticket_id: lane_ticket.ticket_id.clone(),
                status: ticket.as_ref().map(ffi_ticket_status),
                priority: ticket
                    .as_ref()
                    .and_then(|ticket| ffi_ticket_text_field(ticket, "priority")),
                title: ticket.as_ref().map(ffi_ticket_title),
            }
        })
        .collect();
    let mut view = loom_lanes::lane_view(lane, lane_tickets);
    // resolve the lane owner's display alias via the shared ticket-service resolver.
    view.owner_display = view
        .owner_principal
        .as_deref()
        .map(|id| loom_tickets::resolve_principal_display(loom.identity_store(), id));
    view
}

fn lane_view_json(view: &loom_lanes::LaneView, detailed: bool) -> LoomResult<String> {
    let json = if detailed {
        serde_json::to_string(view)
    } else {
        serde_json::to_string(&view.compact())
    };
    json.map_err(|err| LoomError::invalid(format!("lane view json encode failed: {err}")))
}

fn lanes_get_view_json_ns(
    h: &LoomSession,
    workspace: &str,
    ticket_workspace_id: &str,
    lane_id: &str,
    detailed: bool,
) -> LoomResult<String> {
    let loom = open_h_read(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    match loom_lanes::get_lane(&loom, ns, lane_id)? {
        Some(lane) => {
            let view = build_lane_view_ffi(&loom, ns, ticket_workspace_id, &lane);
            lane_view_json(&view, detailed)
        }
        None => Ok("null".to_string()),
    }
}

fn lanes_list_views_json_ns(
    h: &LoomSession,
    workspace: &str,
    ticket_workspace_id: &str,
    detailed: bool,
) -> LoomResult<String> {
    let loom = open_h_read(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let lanes = loom_lanes::list_lanes(&loom, ns)?;
    let json = if detailed {
        let views = lanes
            .iter()
            .map(|lane| build_lane_view_ffi(&loom, ns, ticket_workspace_id, lane))
            .collect::<Vec<_>>();
        serde_json::to_string(&views)
    } else {
        let views = lanes
            .iter()
            .map(|lane| build_lane_view_ffi(&loom, ns, ticket_workspace_id, lane).compact())
            .collect::<Vec<_>>();
        serde_json::to_string(&views)
    };
    json.map_err(|err| LoomError::invalid(format!("lane views json encode failed: {err}")))
}

unsafe fn ok_optional_lane(
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
    out_found: *mut i32,
    bytes: Option<Vec<u8>>,
) -> i32 {
    match bytes {
        Some(bytes) => {
            if !out_found.is_null() {
                // SAFETY: `out_found` is writable per caller contract.
                unsafe { *out_found = 1 };
            }
            // SAFETY: `out_ptr` and `out_len` are writable per caller contract.
            unsafe { ok_bytes(out_ptr, out_len, bytes) }
        }
        None => {
            // SAFETY: each non-null out-pointer is writable per caller contract.
            unsafe {
                if !out_found.is_null() {
                    *out_found = 0;
                }
                if !out_ptr.is_null() {
                    *out_ptr = core::ptr::null_mut();
                }
                if !out_len.is_null() {
                    *out_len = 0;
                }
            }
            0
        }
    }
}

/// Create a lane in `workspace` from a canonical CBOR `Lane` record and return the stored `Lane` record.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace` must be a valid C string; `lane` must point to
/// `lane_len` readable bytes; `out_ptr` and `out_len` must be writable when non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_lanes_create_cbor(
    handle: *mut LoomSession,
    workspace: *const c_char,
    lane: *const c_uchar,
    lane_len: usize,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_lanes_create_cbor");
    let workspace = arg_str!(workspace, "loom_lanes_create_cbor");
    // SAFETY: caller guarantees `(lane, lane_len)` is readable or null when empty.
    let lane = unsafe { byte_slice(lane, lane_len) };
    match lanes_create_ns(h, workspace, lane) {
        // SAFETY: `out_ptr` and `out_len` are writable per docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Fetch one lane as canonical CBOR. `*out_found` is `0` when the lane is absent.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace` and `lane_id` must be valid C strings;
/// `out_ptr`, `out_len`, and `out_found` must be writable when non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_lanes_get_cbor(
    handle: *mut LoomSession,
    workspace: *const c_char,
    lane_id: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
    out_found: *mut i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_lanes_get_cbor");
    let workspace = arg_str!(workspace, "loom_lanes_get_cbor");
    let lane_id = arg_str!(lane_id, "loom_lanes_get_cbor");
    match lanes_get_ns(h, workspace, lane_id) {
        // SAFETY: output pointers are writable per docs.
        Ok(bytes) => unsafe { ok_optional_lane(out_ptr, out_len, out_found, bytes) },
        Err(e) => fail(e),
    }
}

/// List lanes as a canonical CBOR array of per-lane canonical CBOR byte strings.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace` must be a valid C string; `out_ptr` and `out_len`
/// must be writable when non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_lanes_list_cbor(
    handle: *mut LoomSession,
    workspace: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_lanes_list_cbor");
    let workspace = arg_str!(workspace, "loom_lanes_list_cbor");
    match lanes_list_cbor_ns(h, workspace) {
        // SAFETY: `out_ptr` and `out_len` are writable per docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Fetch one lane as a JSON view resolved against `ticket_workspace_id`: the full `LaneView` when
/// `detailed`, otherwise the compact projection (label, derived display status, ordered ticket ids).
/// Returns the JSON literal `null` when the lane is absent. Per-ticket status, priority, and title
/// are read from the ticket store; derived display status honors paused/closed/blocked overrides.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`, `ticket_workspace_id`, and `lane_id` must be
/// valid C strings; `out` must be writable when non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_lanes_get_view_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    ticket_workspace_id: *const c_char,
    lane_id: *const c_char,
    detailed: bool,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_lanes_get_view_json");
    let workspace = arg_str!(workspace, "loom_lanes_get_view_json");
    let ticket_workspace_id = arg_str!(ticket_workspace_id, "loom_lanes_get_view_json");
    let lane_id = arg_str!(lane_id, "loom_lanes_get_view_json");
    match lanes_get_view_json_ns(h, workspace, ticket_workspace_id, lane_id, detailed) {
        // SAFETY: `out` is writable per docs.
        Ok(s) => unsafe { ok_str(out, &s) },
        Err(e) => fail(e),
    }
}

/// List lanes as a JSON array of views resolved against `ticket_workspace_id`: full `LaneView`
/// records when `detailed`, otherwise compact projections (label, derived display status, ordered
/// ticket ids). Per-ticket status, priority, and title are read from the ticket store.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace` and `ticket_workspace_id` must be valid C
/// strings; `out` must be writable when non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_lanes_list_views_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    ticket_workspace_id: *const c_char,
    detailed: bool,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_lanes_list_views_json");
    let workspace = arg_str!(workspace, "loom_lanes_list_views_json");
    let ticket_workspace_id = arg_str!(ticket_workspace_id, "loom_lanes_list_views_json");
    match lanes_list_views_json_ns(h, workspace, ticket_workspace_id, detailed) {
        // SAFETY: `out` is writable per docs.
        Ok(s) => unsafe { ok_str(out, &s) },
        Err(e) => fail(e),
    }
}

/// Distinguish an omitted optional string (null pointer -> `None`, leave the field unchanged) from
/// an explicit clear (empty C string -> `Some("")`). This differs from the shared `optional_str_arg`
/// helpers used elsewhere, which collapse empty strings to `None`.
///
/// # Safety
/// `value` must be null or a valid C string.
unsafe fn lane_optional_field<'a>(value: *const c_char, what: &str) -> LoomResult<Option<&'a str>> {
    if value.is_null() {
        return Ok(None);
    }
    let value = unsafe { CStr::from_ptr(value) }
        .to_str()
        .map_err(|_| LoomError::invalid(format!("{what}: invalid UTF-8")))?;
    Ok(Some(value))
}

/// Atomically update one or more Lane fields and return the updated lane as canonical CBOR.
///
/// At least one optional field must be non-null. A null pointer leaves the field unchanged; a
/// non-null string sets it, so clearing text stays distinct from omission.
///
/// # Safety
/// `workspace`, `lane_id`, and `updated_by` must be valid C strings; optional fields must be null
/// or valid C strings; `out_ptr` and `out_len` must be writable when non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_lanes_update_cbor(
    handle: *mut LoomSession,
    workspace: *const c_char,
    lane_id: *const c_char,
    title: *const c_char,
    description: *const c_char,
    lane_status: *const c_char,
    status_report: *const c_char,
    reviewer_feedback: *const c_char,
    updated_by: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_lanes_update_cbor");
    let workspace = arg_str!(workspace, "loom_lanes_update_cbor");
    let lane_id = arg_str!(lane_id, "loom_lanes_update_cbor");
    let title = match unsafe { lane_optional_field(title, "loom_lanes_update_cbor") } {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    let description = match unsafe { lane_optional_field(description, "loom_lanes_update_cbor") } {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    let lane_status = match unsafe { lane_optional_field(lane_status, "loom_lanes_update_cbor") } {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    let status_report =
        match unsafe { lane_optional_field(status_report, "loom_lanes_update_cbor") } {
            Ok(value) => value,
            Err(e) => return fail(e),
        };
    let reviewer_feedback =
        match unsafe { lane_optional_field(reviewer_feedback, "loom_lanes_update_cbor") } {
            Ok(value) => value,
            Err(e) => return fail(e),
        };
    let updated_by = arg_str!(updated_by, "loom_lanes_update_cbor");
    if title.is_none()
        && description.is_none()
        && lane_status.is_none()
        && status_report.is_none()
        && reviewer_feedback.is_none()
    {
        return fail(LoomError::invalid(
            "lane update requires at least one field",
        ));
    }
    match lanes_mutate_ns(h, workspace, lane_id, |lane| {
        if let Some(title) = title {
            lane.title = title.to_string();
        }
        if let Some(description) = description {
            lane.description = description.to_string();
        }
        if let Some(lane_status) = lane_status {
            lane.lane_status = LaneStatus::parse(lane_status)?.as_str().to_string();
        }
        if let Some(status_report) = status_report {
            lane.status_report = status_report.to_string();
        }
        if let Some(reviewer_feedback) = reviewer_feedback {
            lane.reviewer_feedback = reviewer_feedback.to_string();
        }
        update_lane_metadata(lane, updated_by);
        Ok(())
    }) {
        // SAFETY: `out_ptr` and `out_len` are writable per docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Add a ticket to the lane at the position selected by `placement` and return the updated lane as
/// canonical CBOR.
///
/// `placement` is a public placement verb: null or empty => `append`; `first`; `before`/`after`
/// require a non-empty `anchor` ticket id. A null `anchor` is treated as absent. Ordering is never
/// caller-supplied; callers choose only where the ticket lands.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`, `lane_id`, `ticket_id`, and `updated_by` must be
/// valid C strings; `placement` and `anchor` must be null or valid C strings; `out_ptr` and `out_len`
/// must be writable when non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_lanes_ticket_add_cbor(
    handle: *mut LoomSession,
    workspace: *const c_char,
    lane_id: *const c_char,
    ticket_id: *const c_char,
    updated_by: *const c_char,
    placement: *const c_char,
    anchor: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_lanes_ticket_add_cbor");
    let workspace = arg_str!(workspace, "loom_lanes_ticket_add_cbor");
    let lane_id = arg_str!(lane_id, "loom_lanes_ticket_add_cbor");
    let ticket_id = arg_str!(ticket_id, "loom_lanes_ticket_add_cbor");
    let updated_by = arg_str!(updated_by, "loom_lanes_ticket_add_cbor");
    // A null placement pointer defaults to "append"; `LaneTicketPlacement::parse` maps "" -> Append.
    let placement = match unsafe { lane_optional_field(placement, "loom_lanes_ticket_add_cbor") } {
        Ok(value) => value.unwrap_or(""),
        Err(e) => return fail(e),
    };
    // A null anchor pointer is treated as absent (None).
    let anchor = match unsafe { lane_optional_field(anchor, "loom_lanes_ticket_add_cbor") } {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    let placement = match loom_lanes::LaneTicketPlacement::parse(placement, anchor) {
        Ok(placement) => placement,
        Err(e) => return fail(e),
    };
    match lanes_mutate_ns(h, workspace, lane_id, |lane| {
        loom_lanes::place_lane_ticket(lane, ticket_id, placement)?;
        update_lane_metadata(lane, updated_by);
        Ok(())
    }) {
        // SAFETY: `out_ptr` and `out_len` are writable per docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Remove a ticket from the lane and return the updated lane as canonical CBOR.
///
/// # Safety
/// String pointers must be valid C strings; `out_ptr` and `out_len` must be writable when non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_lanes_ticket_remove_cbor(
    handle: *mut LoomSession,
    workspace: *const c_char,
    lane_id: *const c_char,
    ticket_id: *const c_char,
    updated_by: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_lanes_ticket_remove_cbor");
    let workspace = arg_str!(workspace, "loom_lanes_ticket_remove_cbor");
    let lane_id = arg_str!(lane_id, "loom_lanes_ticket_remove_cbor");
    let ticket_id = arg_str!(ticket_id, "loom_lanes_ticket_remove_cbor");
    let updated_by = arg_str!(updated_by, "loom_lanes_ticket_remove_cbor");
    match lanes_mutate_ns(h, workspace, lane_id, |lane| {
        lane.lane_tickets
            .retain(|lane_ticket| lane_ticket.ticket_id != ticket_id);
        if lane.active_ticket_id.as_deref() == Some(ticket_id) {
            lane.active_ticket_id = None;
        }
        update_lane_metadata(lane, updated_by);
        Ok(())
    }) {
        // SAFETY: `out_ptr` and `out_len` are writable per docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Transfer a ticket between assignment lanes and return the target lane as canonical CBOR.
///
/// # Safety
/// String pointers must be valid C strings; `out_ptr` and `out_len` must be writable when non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_lanes_ticket_transfer_cbor(
    handle: *mut LoomSession,
    workspace: *const c_char,
    source_lane_id: *const c_char,
    target_lane_id: *const c_char,
    ticket_id: *const c_char,
    updated_by: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_lanes_ticket_transfer_cbor");
    let workspace = arg_str!(workspace, "loom_lanes_ticket_transfer_cbor");
    let source_lane_id = arg_str!(source_lane_id, "loom_lanes_ticket_transfer_cbor");
    let target_lane_id = arg_str!(target_lane_id, "loom_lanes_ticket_transfer_cbor");
    let ticket_id = arg_str!(ticket_id, "loom_lanes_ticket_transfer_cbor");
    let updated_by = arg_str!(updated_by, "loom_lanes_ticket_transfer_cbor");
    match lanes_ticket_transfer_ns(
        h,
        workspace,
        source_lane_id,
        target_lane_id,
        ticket_id,
        updated_by,
    ) {
        // SAFETY: `out_ptr` and `out_len` are writable per docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Delete a closed lane and return the deleted lane tombstone as canonical CBOR.
///
/// # Safety
/// String pointers must be valid C strings; `out_ptr` and `out_len` must be writable when non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_lanes_delete_cbor(
    handle: *mut LoomSession,
    workspace: *const c_char,
    lane_id: *const c_char,
    updated_by: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_lanes_delete_cbor");
    let workspace = arg_str!(workspace, "loom_lanes_delete_cbor");
    let lane_id = arg_str!(lane_id, "loom_lanes_delete_cbor");
    let updated_by = arg_str!(updated_by, "loom_lanes_delete_cbor");
    match lanes_delete_ns(h, workspace, lane_id, updated_by) {
        // SAFETY: `out_ptr` and `out_len` are writable per docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}
