//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

use loom_pages::{
    PageCreateRequest, StructureBindRequest, StructureCreateRequest, StructureDecomposeItem,
    StructureDecomposeRequest, StructureLinkRequest, StructureMoveRequest, StructureNodeRequest,
};
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value as JsonValue;

#[derive(Deserialize)]
struct StructureDecomposeItemJson {
    node_id: String,
    project_id: String,
    ticket_type: Option<String>,
    fields: Option<JsonValue>,
    #[serde(default)]
    policy_labels: Vec<String>,
}

unsafe fn optional_str_arg<'a>(value: *const c_char, what: &str) -> LoomResult<Option<&'a str>> {
    if value.is_null() {
        return Ok(None);
    }
    let value = unsafe { CStr::from_ptr(value) }
        .to_str()
        .map_err(|_| LoomError::invalid(format!("{what}: invalid UTF-8")))?;
    Ok((!value.is_empty()).then_some(value))
}

unsafe fn str_arg<'a>(value: *const c_char, what: &str) -> LoomResult<&'a str> {
    if value.is_null() {
        return Err(LoomError::invalid(format!("{what}: null string")));
    }
    unsafe { CStr::from_ptr(value) }
        .to_str()
        .map_err(|_| LoomError::invalid(format!("{what}: invalid UTF-8")))
}

fn json_result<T: Serialize>(result: LoomResult<T>) -> LoomResult<String> {
    serde_json::to_string(&result?).map_err(|err| LoomError::invalid(err.to_string()))
}

fn pages_read_loom<T>(
    h: &LoomSession,
    workspace: &str,
    f: impl FnOnce(&Loom<FileStore>, WorkspaceId) -> LoomResult<T>,
) -> LoomResult<T> {
    let loom = open_h_read(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    f(&loom, ns)
}

fn pages_write_loom<T>(
    h: &LoomSession,
    workspace: &str,
    f: impl FnOnce(&mut Loom<FileStore>, WorkspaceId) -> LoomResult<T>,
) -> LoomResult<T> {
    let mut loom = open_h_write(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let result = f(&mut loom, ns)?;
    save_loom(&mut loom)?;
    Ok(result)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

macro_rules! out_json {
    ($out:ident, $result:expr) => {
        match $result {
            Ok(s) => unsafe { ok_str($out, &s) },
            Err(e) => fail(e),
        }
    };
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_spaces_create_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    page_workspace_id: *const c_char,
    space_id: *const c_char,
    title: *const c_char,
    expected_root: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_spaces_create_json");
    let workspace = arg_str!(workspace, "loom_spaces_create_json");
    let page_workspace_id = arg_str!(page_workspace_id, "loom_spaces_create_json");
    let space_id = arg_str!(space_id, "loom_spaces_create_json");
    let title = arg_str!(title, "loom_spaces_create_json");
    let expected_root = match unsafe { optional_str_arg(expected_root, "loom_spaces_create_json") }
    {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    out_json!(
        out,
        pages_write_loom(h, workspace, |loom, ns| {
            json_result(loom_pages::create_space(
                loom,
                ns,
                page_workspace_id,
                space_id,
                title,
                expected_root,
            ))
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_spaces_list_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    page_workspace_id: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_spaces_list_json");
    let workspace = arg_str!(workspace, "loom_spaces_list_json");
    let page_workspace_id = arg_str!(page_workspace_id, "loom_spaces_list_json");
    out_json!(
        out,
        pages_read_loom(h, workspace, |loom, ns| {
            json_result(loom_pages::list_spaces(loom, ns, page_workspace_id))
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_spaces_get_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    page_workspace_id: *const c_char,
    space_id: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_spaces_get_json");
    let workspace = arg_str!(workspace, "loom_spaces_get_json");
    let page_workspace_id = arg_str!(page_workspace_id, "loom_spaces_get_json");
    let space_id = arg_str!(space_id, "loom_spaces_get_json");
    out_json!(
        out,
        pages_read_loom(h, workspace, |loom, ns| {
            json_result(loom_pages::get_space(loom, ns, page_workspace_id, space_id))
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_pages_create_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    page_workspace_id: *const c_char,
    page_id: *const c_char,
    space_id: *const c_char,
    parent_page_id: *const c_char,
    title: *const c_char,
    expected_root: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_pages_create_json");
    let workspace = arg_str!(workspace, "loom_pages_create_json");
    let page_workspace_id = arg_str!(page_workspace_id, "loom_pages_create_json");
    let page_id = arg_str!(page_id, "loom_pages_create_json");
    let space_id = arg_str!(space_id, "loom_pages_create_json");
    let parent_page_id = match unsafe { optional_str_arg(parent_page_id, "loom_pages_create_json") }
    {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    let title = arg_str!(title, "loom_pages_create_json");
    let expected_root = match unsafe { optional_str_arg(expected_root, "loom_pages_create_json") } {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    out_json!(
        out,
        pages_write_loom(h, workspace, |loom, ns| {
            json_result(loom_pages::create_page(
                loom,
                ns,
                PageCreateRequest {
                    workspace_id: page_workspace_id,
                    page_id,
                    space_id,
                    parent_page_id,
                    title,
                    expected_root,
                },
            ))
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_pages_update_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    page_workspace_id: *const c_char,
    page_id: *const c_char,
    body_text: *const c_char,
    expected_root: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_pages_update_json");
    let workspace = arg_str!(workspace, "loom_pages_update_json");
    let page_workspace_id = arg_str!(page_workspace_id, "loom_pages_update_json");
    let page_id = arg_str!(page_id, "loom_pages_update_json");
    let body_text = arg_str!(body_text, "loom_pages_update_json");
    let expected_root = match unsafe { optional_str_arg(expected_root, "loom_pages_update_json") } {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    out_json!(
        out,
        pages_write_loom(h, workspace, |loom, ns| {
            json_result(loom_pages::update_page_text(
                loom,
                ns,
                page_workspace_id,
                page_id,
                body_text,
                now_ms(),
                expected_root,
            ))
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_pages_publish_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    page_workspace_id: *const c_char,
    page_id: *const c_char,
    expected_root: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_pages_publish_json");
    let workspace = arg_str!(workspace, "loom_pages_publish_json");
    let page_workspace_id = arg_str!(page_workspace_id, "loom_pages_publish_json");
    let page_id = arg_str!(page_id, "loom_pages_publish_json");
    let expected_root = match unsafe { optional_str_arg(expected_root, "loom_pages_publish_json") }
    {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    out_json!(
        out,
        pages_write_loom(h, workspace, |loom, ns| {
            json_result(loom_pages::publish_page(
                loom,
                ns,
                page_workspace_id,
                page_id,
                now_ms(),
                expected_root,
            ))
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_pages_get_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    page_workspace_id: *const c_char,
    page_id: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_pages_get_json");
    let workspace = arg_str!(workspace, "loom_pages_get_json");
    let page_workspace_id = arg_str!(page_workspace_id, "loom_pages_get_json");
    let page_id = arg_str!(page_id, "loom_pages_get_json");
    out_json!(
        out,
        pages_read_loom(h, workspace, |loom, ns| {
            json_result(loom_pages::get_page(loom, ns, page_workspace_id, page_id))
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_pages_list_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    page_workspace_id: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_pages_list_json");
    let workspace = arg_str!(workspace, "loom_pages_list_json");
    let page_workspace_id = arg_str!(page_workspace_id, "loom_pages_list_json");
    out_json!(
        out,
        pages_read_loom(h, workspace, |loom, ns| {
            json_result(loom_pages::list_pages(loom, ns, page_workspace_id))
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_pages_history_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    page_workspace_id: *const c_char,
    page_id: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_pages_history_json");
    let workspace = arg_str!(workspace, "loom_pages_history_json");
    let page_workspace_id = arg_str!(page_workspace_id, "loom_pages_history_json");
    let page_id = arg_str!(page_id, "loom_pages_history_json");
    out_json!(
        out,
        pages_read_loom(h, workspace, |loom, ns| {
            json_result(loom_pages::page_history(
                loom,
                ns,
                page_workspace_id,
                page_id,
            ))
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_structures_create_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    page_workspace_id: *const c_char,
    structure_id: *const c_char,
    space_id: *const c_char,
    kind: *const c_char,
    title: *const c_char,
    expected_root: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_structures_create_json");
    let workspace = arg_str!(workspace, "loom_structures_create_json");
    let page_workspace_id = arg_str!(page_workspace_id, "loom_structures_create_json");
    let structure_id = arg_str!(structure_id, "loom_structures_create_json");
    let space_id = arg_str!(space_id, "loom_structures_create_json");
    let kind = arg_str!(kind, "loom_structures_create_json");
    let title = arg_str!(title, "loom_structures_create_json");
    let expected_root =
        match unsafe { optional_str_arg(expected_root, "loom_structures_create_json") } {
            Ok(value) => value,
            Err(e) => return fail(e),
        };
    out_json!(
        out,
        pages_write_loom(h, workspace, |loom, ns| {
            json_result(loom_pages::create_structure(
                loom,
                ns,
                StructureCreateRequest {
                    workspace_id: page_workspace_id,
                    structure_id,
                    space_id,
                    kind,
                    title,
                    expected_root,
                },
            ))
        })
    )
}

unsafe fn structure_node_request<'a>(
    what: &str,
    page_workspace_id: *const c_char,
    structure_id: *const c_char,
    node_id: *const c_char,
    kind: *const c_char,
    label: *const c_char,
    body_digest: *const c_char,
    entity_ref: *const c_char,
    expected_root: *const c_char,
) -> LoomResult<StructureNodeRequest<'a>> {
    Ok(StructureNodeRequest {
        workspace_id: unsafe { str_arg(page_workspace_id, what) }?,
        structure_id: unsafe { str_arg(structure_id, what) }?,
        node_id: unsafe { str_arg(node_id, what) }?,
        kind: unsafe { str_arg(kind, what) }?,
        label: unsafe { str_arg(label, what) }?,
        body_digest: unsafe { optional_str_arg(body_digest, what) }?,
        entity_ref: unsafe { optional_str_arg(entity_ref, what) }?.map(str::to_string),
        expected_root: unsafe { optional_str_arg(expected_root, what) }?,
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_structures_add_node_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    page_workspace_id: *const c_char,
    structure_id: *const c_char,
    node_id: *const c_char,
    kind: *const c_char,
    label: *const c_char,
    body_digest: *const c_char,
    entity_ref: *const c_char,
    expected_root: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_structures_add_node_json");
    let workspace = arg_str!(workspace, "loom_structures_add_node_json");
    let request = match unsafe {
        structure_node_request(
            "loom_structures_add_node_json",
            page_workspace_id,
            structure_id,
            node_id,
            kind,
            label,
            body_digest,
            entity_ref,
            expected_root,
        )
    } {
        Ok(request) => request,
        Err(e) => return fail(e),
    };
    out_json!(
        out,
        pages_write_loom(h, workspace, |loom, ns| {
            json_result(loom_pages::add_structure_node(loom, ns, request))
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_structures_update_node_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    page_workspace_id: *const c_char,
    structure_id: *const c_char,
    node_id: *const c_char,
    kind: *const c_char,
    label: *const c_char,
    body_digest: *const c_char,
    entity_ref: *const c_char,
    expected_root: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_structures_update_node_json");
    let workspace = arg_str!(workspace, "loom_structures_update_node_json");
    let request = match unsafe {
        structure_node_request(
            "loom_structures_update_node_json",
            page_workspace_id,
            structure_id,
            node_id,
            kind,
            label,
            body_digest,
            entity_ref,
            expected_root,
        )
    } {
        Ok(request) => request,
        Err(e) => return fail(e),
    };
    out_json!(
        out,
        pages_write_loom(h, workspace, |loom, ns| {
            json_result(loom_pages::update_structure_node(loom, ns, request))
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_structures_bind_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    page_workspace_id: *const c_char,
    structure_id: *const c_char,
    node_id: *const c_char,
    entity_ref: *const c_char,
    expected_root: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_structures_bind_json");
    let workspace = arg_str!(workspace, "loom_structures_bind_json");
    let page_workspace_id = arg_str!(page_workspace_id, "loom_structures_bind_json");
    let structure_id = arg_str!(structure_id, "loom_structures_bind_json");
    let node_id = arg_str!(node_id, "loom_structures_bind_json");
    let entity_ref = match unsafe { optional_str_arg(entity_ref, "loom_structures_bind_json") } {
        Ok(value) => value.map(str::to_string),
        Err(e) => return fail(e),
    };
    let expected_root =
        match unsafe { optional_str_arg(expected_root, "loom_structures_bind_json") } {
            Ok(value) => value,
            Err(e) => return fail(e),
        };
    out_json!(
        out,
        pages_write_loom(h, workspace, |loom, ns| {
            json_result(loom_pages::bind_structure_node(
                loom,
                ns,
                StructureBindRequest {
                    workspace_id: page_workspace_id,
                    structure_id,
                    node_id,
                    entity_ref,
                    expected_root,
                },
            ))
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_structures_move_node_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    page_workspace_id: *const c_char,
    structure_id: *const c_char,
    node_id: *const c_char,
    parent_node_id: *const c_char,
    label: *const c_char,
    expected_root: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_structures_move_node_json");
    let workspace = arg_str!(workspace, "loom_structures_move_node_json");
    let page_workspace_id = arg_str!(page_workspace_id, "loom_structures_move_node_json");
    let structure_id = arg_str!(structure_id, "loom_structures_move_node_json");
    let node_id = arg_str!(node_id, "loom_structures_move_node_json");
    let parent_node_id =
        match unsafe { optional_str_arg(parent_node_id, "loom_structures_move_node_json") } {
            Ok(value) => value,
            Err(e) => return fail(e),
        };
    let label = match unsafe { optional_str_arg(label, "loom_structures_move_node_json") } {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    let expected_root =
        match unsafe { optional_str_arg(expected_root, "loom_structures_move_node_json") } {
            Ok(value) => value,
            Err(e) => return fail(e),
        };
    out_json!(
        out,
        pages_write_loom(h, workspace, |loom, ns| {
            json_result(loom_pages::move_structure_node(
                loom,
                ns,
                StructureMoveRequest {
                    workspace_id: page_workspace_id,
                    structure_id,
                    node_id,
                    parent_node_id,
                    label,
                    expected_root,
                },
            ))
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_structures_link_node_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    page_workspace_id: *const c_char,
    structure_id: *const c_char,
    edge_id: *const c_char,
    src_node_id: *const c_char,
    dst_node_id: *const c_char,
    label: *const c_char,
    target_ref: *const c_char,
    expected_root: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_structures_link_node_json");
    let workspace = arg_str!(workspace, "loom_structures_link_node_json");
    let page_workspace_id = arg_str!(page_workspace_id, "loom_structures_link_node_json");
    let structure_id = arg_str!(structure_id, "loom_structures_link_node_json");
    let edge_id = arg_str!(edge_id, "loom_structures_link_node_json");
    let src_node_id = arg_str!(src_node_id, "loom_structures_link_node_json");
    let dst_node_id = arg_str!(dst_node_id, "loom_structures_link_node_json");
    let label = arg_str!(label, "loom_structures_link_node_json");
    let target_ref = match unsafe { optional_str_arg(target_ref, "loom_structures_link_node_json") }
    {
        Ok(value) => value.map(str::to_string),
        Err(e) => return fail(e),
    };
    let expected_root =
        match unsafe { optional_str_arg(expected_root, "loom_structures_link_node_json") } {
            Ok(value) => value,
            Err(e) => return fail(e),
        };
    out_json!(
        out,
        pages_write_loom(h, workspace, |loom, ns| {
            json_result(loom_pages::link_structure_node(
                loom,
                ns,
                StructureLinkRequest {
                    workspace_id: page_workspace_id,
                    structure_id,
                    edge_id,
                    src_node_id,
                    dst_node_id,
                    label,
                    target_ref,
                    expected_root,
                },
            ))
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_structures_decompose_to_tickets_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    page_workspace_id: *const c_char,
    structure_id: *const c_char,
    items_json: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_structures_decompose_to_tickets_json");
    let workspace = arg_str!(workspace, "loom_structures_decompose_to_tickets_json");
    let page_workspace_id = arg_str!(
        page_workspace_id,
        "loom_structures_decompose_to_tickets_json"
    );
    let structure_id = arg_str!(structure_id, "loom_structures_decompose_to_tickets_json");
    let items_json = arg_str!(items_json, "loom_structures_decompose_to_tickets_json");
    let items: Vec<StructureDecomposeItemJson> = match serde_json::from_str(items_json) {
        Ok(items) => items,
        Err(err) => {
            return fail(LoomError::invalid(format!(
                "structure decompose items: {err}"
            )));
        }
    };
    let borrowed = items
        .iter()
        .map(|item| StructureDecomposeItem {
            node_id: item.node_id.as_str(),
            project_id: item.project_id.as_str(),
            ticket_type: item.ticket_type.as_deref(),
            fields: item.fields.as_ref(),
            policy_labels: &item.policy_labels,
        })
        .collect::<Vec<_>>();
    out_json!(
        out,
        pages_write_loom(h, workspace, |loom, ns| {
            json_result(loom_pages::decompose_to_tickets(
                loom,
                ns,
                StructureDecomposeRequest {
                    workspace_id: page_workspace_id,
                    structure_id,
                    items: &borrowed,
                },
            ))
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_structures_get_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    page_workspace_id: *const c_char,
    structure_id: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_structures_get_json");
    let workspace = arg_str!(workspace, "loom_structures_get_json");
    let page_workspace_id = arg_str!(page_workspace_id, "loom_structures_get_json");
    let structure_id = arg_str!(structure_id, "loom_structures_get_json");
    out_json!(
        out,
        pages_read_loom(h, workspace, |loom, ns| {
            json_result(loom_pages::get_structure(
                loom,
                ns,
                page_workspace_id,
                structure_id,
            ))
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_structures_list_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    page_workspace_id: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_structures_list_json");
    let workspace = arg_str!(workspace, "loom_structures_list_json");
    let page_workspace_id = arg_str!(page_workspace_id, "loom_structures_list_json");
    out_json!(
        out,
        pages_read_loom(h, workspace, |loom, ns| {
            json_result(loom_pages::list_structures(loom, ns, page_workspace_id))
        })
    )
}
