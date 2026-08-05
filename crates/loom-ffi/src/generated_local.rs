//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;
use futures::executor::block_on;
use loom_client::generated_api::{
    Audit, Columnar, Exec, InferenceInstance, InterchangeProfiles, Lifecycle, Meetings, Refs,
    ServeConfig, Sql, StoreAdmin, StudioMaintenance, Vector,
};
use std::future::Future;

unsafe fn optional_str_arg<'a>(value: *const c_char, what: &str) -> LoomResult<Option<&'a str>> {
    if value.is_null() {
        return Ok(None);
    }
    let value = unsafe { CStr::from_ptr(value) }
        .to_str()
        .map_err(|_| LoomError::invalid(format!("{what}: invalid UTF-8")))?;
    Ok(Some(value))
}

fn parse_string_list(value: &str, what: &str) -> LoomResult<Vec<String>> {
    serde_json::from_str(value).map_err(|err| LoomError::invalid(format!("{what}: {err}")))
}

pub(crate) fn block_generated<F: Future>(future: F) -> F::Output {
    block_on(future)
}

fn generated_string(
    h: &LoomSession,
    f: impl FnOnce(&loom_client::LocalLoomClient, loom_client::types::LoomSession) -> LoomResult<String>,
) -> LoomResult<String> {
    with_generated_client(h, f)
}

fn generated_bytes(
    h: &LoomSession,
    f: impl FnOnce(
        &loom_client::LocalLoomClient,
        loom_client::types::LoomSession,
    ) -> LoomResult<Vec<u8>>,
) -> LoomResult<Vec<u8>> {
    with_generated_client(h, f)
}

/// Define a standard lifecycle and return deterministic JSON.
///
/// # Safety
/// `handle` must be from [`loom_open`]; string arguments must be valid C strings; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_lifecycle_define_standard_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    kind: *const c_char,
    version: *const c_char,
    completion_predicate_digest: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_lifecycle_define_standard_json");
    let workspace = arg_str!(workspace, "loom_lifecycle_define_standard_json");
    let kind = arg_str!(kind, "loom_lifecycle_define_standard_json");
    let version = arg_str!(version, "loom_lifecycle_define_standard_json");
    let completion_predicate_digest = arg_str!(
        completion_predicate_digest,
        "loom_lifecycle_define_standard_json"
    );
    match generated_string(h, |client, session| {
        block_generated(Lifecycle::lifecycle_define_standard_json(
            client,
            session,
            workspace.to_string(),
            kind.to_string(),
            version.to_string(),
            completion_predicate_digest.to_string(),
        ))
    }) {
        Ok(json) => unsafe { ok_str(out, &json) },
        Err(e) => fail(e),
    }
}

/// Define a lifecycle from its canonical byte definition and return deterministic JSON.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace` must be a valid C string; `definition` must be
/// null or readable for `definition_len` bytes; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_lifecycle_define_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    definition: *const c_uchar,
    definition_len: usize,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_lifecycle_define_json");
    let workspace = arg_str!(workspace, "loom_lifecycle_define_json");
    let definition = unsafe { byte_slice(definition, definition_len) }.to_vec();
    match generated_string(h, |client, session| {
        block_generated(Lifecycle::lifecycle_define_json(
            client,
            session,
            workspace.to_string(),
            definition,
        ))
    }) {
        Ok(json) => unsafe { ok_str(out, &json) },
        Err(e) => fail(e),
    }
}

/// Instantiate a lifecycle and return deterministic JSON.
///
/// # Safety
/// `handle` must be from [`loom_open`]; string arguments must be valid C strings;
/// `subject_refs_json` must be a JSON string array; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_lifecycle_instantiate_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    instance_id: *const c_char,
    definition_id: *const c_char,
    subject_refs_json: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_lifecycle_instantiate_json");
    let workspace = arg_str!(workspace, "loom_lifecycle_instantiate_json");
    let instance_id = arg_str!(instance_id, "loom_lifecycle_instantiate_json");
    let definition_id = arg_str!(definition_id, "loom_lifecycle_instantiate_json");
    let subject_refs_json = arg_str!(subject_refs_json, "loom_lifecycle_instantiate_json");
    let subject_refs = match parse_string_list(subject_refs_json, "loom_lifecycle_instantiate_json")
    {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    match generated_string(h, |client, session| {
        block_generated(Lifecycle::lifecycle_instantiate_json(
            client,
            session,
            workspace.to_string(),
            instance_id.to_string(),
            definition_id.to_string(),
            subject_refs,
        ))
    }) {
        Ok(json) => unsafe { ok_str(out, &json) },
        Err(e) => fail(e),
    }
}

/// Transition a lifecycle instance and return deterministic JSON.
///
/// # Safety
/// `handle` must be from [`loom_open`]; string arguments must be valid C strings; optional strings may
/// be null; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_lifecycle_transition_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    instance_id: *const c_char,
    transition_id: *const c_char,
    to_stage_id: *const c_char,
    actor_principal_id: *const c_char,
    gate_evaluations_json: *const c_char,
    snapshot_digest: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_lifecycle_transition_json");
    let workspace = arg_str!(workspace, "loom_lifecycle_transition_json");
    let instance_id = arg_str!(instance_id, "loom_lifecycle_transition_json");
    let transition_id = arg_str!(transition_id, "loom_lifecycle_transition_json");
    let to_stage_id = arg_str!(to_stage_id, "loom_lifecycle_transition_json");
    let actor_principal_id =
        match unsafe { optional_str_arg(actor_principal_id, "loom_lifecycle_transition_json") } {
            Ok(value) => value.map(str::to_string),
            Err(e) => return fail(e),
        };
    let gate_evaluations_json = arg_str!(gate_evaluations_json, "loom_lifecycle_transition_json");
    let snapshot_digest =
        match unsafe { optional_str_arg(snapshot_digest, "loom_lifecycle_transition_json") } {
            Ok(value) => value.map(str::to_string),
            Err(e) => return fail(e),
        };
    match generated_string(h, |client, session| {
        block_generated(Lifecycle::lifecycle_transition_json(
            client,
            session,
            workspace.to_string(),
            instance_id.to_string(),
            transition_id.to_string(),
            to_stage_id.to_string(),
            actor_principal_id,
            gate_evaluations_json.to_string(),
            snapshot_digest,
        ))
    }) {
        Ok(json) => unsafe { ok_str(out, &json) },
        Err(e) => fail(e),
    }
}

/// Reconcile references and return deterministic JSON.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace` must be a valid C string; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_refs_reconcile_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    max: u64,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_refs_reconcile_json");
    let workspace = arg_str!(workspace, "loom_refs_reconcile_json");
    match generated_string(h, |client, session| {
        block_generated(Refs::refs_reconcile_json(
            client,
            session,
            workspace.to_string(),
            max,
        ))
    }) {
        Ok(json) => unsafe { ok_str(out, &json) },
        Err(e) => fail(e),
    }
}

/// Apply an Exec request and return canonical `loom.exec.apply.result.v1` CBOR.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `request` must be null or readable for `request_len` bytes;
/// `out_ptr` and `out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_apply_cbor(
    handle: *mut LoomSession,
    request: *const c_uchar,
    request_len: usize,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_apply_cbor");
    let request = unsafe { byte_slice(request, request_len) }.to_vec();
    match generated_bytes(h, |client, session| {
        block_generated(Exec::apply_cbor(client, session, request))
    }) {
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Import a normalized Meetings snapshot into an existing workspace and return the report JSON.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace` and `input_profile` must be valid C strings;
/// `snapshot` must be null or readable for `snapshot_len` bytes; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_meetings_import_snapshot(
    handle: *mut LoomSession,
    workspace: *const c_char,
    input_profile: *const c_char,
    snapshot: *const c_uchar,
    snapshot_len: usize,
    dry_run: i32,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_meetings_import_snapshot");
    let workspace = arg_str!(workspace, "loom_meetings_import_snapshot");
    let input_profile = arg_str!(input_profile, "loom_meetings_import_snapshot");
    let snapshot = unsafe { byte_slice(snapshot, snapshot_len) }.to_vec();
    match generated_string(h, |client, session| {
        block_generated(Meetings::meetings_import_snapshot(
            client,
            session,
            workspace.to_string(),
            input_profile.to_string(),
            snapshot,
            dry_run != 0,
        ))
    }) {
        Ok(json) => unsafe { ok_str(out, &json) },
        Err(e) => fail(e),
    }
}

/// Execute SQL through the generated `Sql.sql_exec_result` owner and return canonical CBOR.
///
/// # Safety
/// `handle` must be from [`loom_open`]; string arguments must be valid C strings; `out_ptr` and
/// `out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_sql_exec_result(
    handle: *mut LoomSession,
    workspace: *const c_char,
    db: *const c_char,
    sql: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_sql_exec_result");
    let workspace = arg_str!(workspace, "loom_sql_exec_result");
    let db = arg_str!(db, "loom_sql_exec_result");
    let sql = arg_str!(sql, "loom_sql_exec_result");
    match generated_bytes(h, |client, session| {
        block_generated(Sql::sql_exec_result(
            client,
            session,
            workspace.to_string(),
            db.to_string(),
            sql.to_string(),
        ))
    }) {
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Configure a served listener through the generated ServeConfig owner and return JSON.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `request_json` must be a valid C string; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_serve_listener_configure_json(
    handle: *mut LoomSession,
    request_json: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_serve_listener_configure_json");
    let request_json = arg_str!(request_json, "loom_serve_listener_configure_json");
    match generated_string(h, |client, session| {
        block_generated(ServeConfig::serve_listener_configure_json(
            client,
            session,
            request_json.to_string(),
        ))
    }) {
        Ok(json) => unsafe { ok_str(out, &json) },
        Err(e) => fail(e),
    }
}

/// List served listeners through the generated ServeConfig owner and return JSON.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_serve_listener_list_json(
    handle: *mut LoomSession,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_serve_listener_list_json");
    match generated_string(h, |client, session| {
        block_generated(ServeConfig::serve_listener_list_json(client, session))
    }) {
        Ok(json) => unsafe { ok_str(out, &json) },
        Err(e) => fail(e),
    }
}

/// Enable or disable a served listener through the generated ServeConfig owner and return JSON.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `listener_id` must be a valid C string; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_serve_listener_set_enabled_json(
    handle: *mut LoomSession,
    listener_id: *const c_char,
    enabled: i32,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_serve_listener_set_enabled_json");
    let listener_id = arg_str!(listener_id, "loom_serve_listener_set_enabled_json");
    match generated_string(h, |client, session| {
        block_generated(ServeConfig::serve_listener_set_enabled_json(
            client,
            session,
            listener_id.to_string(),
            enabled != 0,
        ))
    }) {
        Ok(json) => unsafe { ok_str(out, &json) },
        Err(e) => fail(e),
    }
}

/// Remove a served listener through the generated ServeConfig owner and return JSON.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `listener_id` must be a valid C string; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_serve_listener_remove_json(
    handle: *mut LoomSession,
    listener_id: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_serve_listener_remove_json");
    let listener_id = arg_str!(listener_id, "loom_serve_listener_remove_json");
    match generated_string(h, |client, session| {
        block_generated(ServeConfig::serve_listener_remove_json(
            client,
            session,
            listener_id.to_string(),
        ))
    }) {
        Ok(json) => unsafe { ok_str(out, &json) },
        Err(e) => fail(e),
    }
}

/// List Web routes for a served listener through the generated ServeConfig owner and return JSON.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `listener_id` must be a valid C string; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_serve_web_route_list_json(
    handle: *mut LoomSession,
    listener_id: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_serve_web_route_list_json");
    let listener_id = arg_str!(listener_id, "loom_serve_web_route_list_json");
    match generated_string(h, |client, session| {
        block_generated(ServeConfig::serve_web_route_list_json(
            client,
            session,
            listener_id.to_string(),
        ))
    }) {
        Ok(json) => unsafe { ok_str(out, &json) },
        Err(e) => fail(e),
    }
}

/// Set a Web route through the generated ServeConfig owner and return JSON.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `request_json` must be a valid C string; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_serve_web_route_set_json(
    handle: *mut LoomSession,
    request_json: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_serve_web_route_set_json");
    let request_json = arg_str!(request_json, "loom_serve_web_route_set_json");
    match generated_string(h, |client, session| {
        block_generated(ServeConfig::serve_web_route_set_json(
            client,
            session,
            request_json.to_string(),
        ))
    }) {
        Ok(json) => unsafe { ok_str(out, &json) },
        Err(e) => fail(e),
    }
}

/// Remove a Web route through the generated ServeConfig owner and return JSON.
///
/// # Safety
/// `handle` must be from [`loom_open`]; string arguments must be valid C strings; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_serve_web_route_remove_json(
    handle: *mut LoomSession,
    listener_id: *const c_char,
    route_id: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_serve_web_route_remove_json");
    let listener_id = arg_str!(listener_id, "loom_serve_web_route_remove_json");
    let route_id = arg_str!(route_id, "loom_serve_web_route_remove_json");
    match generated_string(h, |client, session| {
        block_generated(ServeConfig::serve_web_route_remove_json(
            client,
            session,
            listener_id.to_string(),
            route_id.to_string(),
        ))
    }) {
        Ok(json) => unsafe { ok_str(out, &json) },
        Err(e) => fail(e),
    }
}

/// Import table CSV bytes and return the canonical import report CBOR.
///
/// # Safety
/// `handle` must be from [`loom_open`]; string arguments must be valid C strings; optional strings
/// may be null; `csv_payload` must be null or readable for `csv_payload_len` bytes; `out_ptr` and
/// `out_len` writable.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn loom_import_table_csv(
    handle: *mut LoomSession,
    workspace: *const c_char,
    source_scope: *const c_char,
    csv_payload: *const c_uchar,
    csv_payload_len: usize,
    database: *const c_char,
    table: *const c_char,
    schema: *const c_char,
    primary_key: *const c_char,
    mode: *const c_char,
    commit: i32,
    author: *const c_char,
    message: *const c_char,
    dry_run: i32,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_import_table_csv");
    let workspace = arg_str!(workspace, "loom_import_table_csv");
    let source_scope = arg_str!(source_scope, "loom_import_table_csv");
    let csv_payload = unsafe { byte_slice(csv_payload, csv_payload_len) }.to_vec();
    let database = arg_str!(database, "loom_import_table_csv");
    let table = arg_str!(table, "loom_import_table_csv");
    let schema = arg_str!(schema, "loom_import_table_csv");
    let primary_key = arg_str!(primary_key, "loom_import_table_csv");
    let mode = arg_str!(mode, "loom_import_table_csv");
    let author = match unsafe { optional_str_arg(author, "loom_import_table_csv") } {
        Ok(value) => value.map(str::to_string),
        Err(e) => return fail(e),
    };
    let message = match unsafe { optional_str_arg(message, "loom_import_table_csv") } {
        Ok(value) => value.map(str::to_string),
        Err(e) => return fail(e),
    };
    match generated_bytes(h, |client, session| {
        block_generated(InterchangeProfiles::import_table_csv(
            client,
            session,
            workspace.to_string(),
            source_scope.to_string(),
            csv_payload,
            database.to_string(),
            table.to_string(),
            schema.to_string(),
            primary_key.to_string(),
            mode.to_string(),
            commit != 0,
            author,
            message,
            dry_run != 0,
        ))
    }) {
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Import Redmine snapshot bytes and return the canonical import report CBOR.
///
/// # Safety
/// `handle` must be from [`loom_open`]; string arguments must be valid C strings; `snapshot_payload`
/// must be null or readable for `snapshot_payload_len` bytes; `out_ptr` and `out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_import_redmine(
    handle: *mut LoomSession,
    workspace: *const c_char,
    profile: *const c_char,
    source_scope: *const c_char,
    snapshot_payload: *const c_uchar,
    snapshot_payload_len: usize,
    field_policy: *const c_char,
    dry_run: i32,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_import_redmine");
    let workspace = arg_str!(workspace, "loom_import_redmine");
    let profile = arg_str!(profile, "loom_import_redmine");
    let source_scope = arg_str!(source_scope, "loom_import_redmine");
    let snapshot_payload = unsafe { byte_slice(snapshot_payload, snapshot_payload_len) }.to_vec();
    let field_policy = arg_str!(field_policy, "loom_import_redmine");
    match generated_bytes(h, |client, session| {
        block_generated(InterchangeProfiles::import_redmine(
            client,
            session,
            workspace.to_string(),
            profile.to_string(),
            source_scope.to_string(),
            snapshot_payload,
            field_policy.to_string(),
            dry_run != 0,
        ))
    }) {
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Import Asana snapshot bytes and return the canonical import report CBOR.
///
/// # Safety
/// `handle` must be from [`loom_open`]; string arguments must be valid C strings; `snapshot_payload`
/// must be null or readable for `snapshot_payload_len` bytes; `out_ptr` and `out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_import_asana(
    handle: *mut LoomSession,
    workspace: *const c_char,
    profile: *const c_char,
    source_scope: *const c_char,
    snapshot_payload: *const c_uchar,
    snapshot_payload_len: usize,
    field_policy: *const c_char,
    dry_run: i32,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_import_asana");
    let workspace = arg_str!(workspace, "loom_import_asana");
    let profile = arg_str!(profile, "loom_import_asana");
    let source_scope = arg_str!(source_scope, "loom_import_asana");
    let snapshot_payload = unsafe { byte_slice(snapshot_payload, snapshot_payload_len) }.to_vec();
    let field_policy = arg_str!(field_policy, "loom_import_asana");
    match generated_bytes(h, |client, session| {
        block_generated(InterchangeProfiles::import_asana(
            client,
            session,
            workspace.to_string(),
            profile.to_string(),
            source_scope.to_string(),
            snapshot_payload,
            field_policy.to_string(),
            dry_run != 0,
        ))
    }) {
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Import Jira snapshot bytes and return the canonical import report CBOR.
///
/// # Safety
/// `handle` must be from [`loom_open`]; string arguments must be valid C strings; `snapshot_payload`
/// must be null or readable for `snapshot_payload_len` bytes; `out_ptr` and `out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_import_jira(
    handle: *mut LoomSession,
    workspace: *const c_char,
    profile: *const c_char,
    source_scope: *const c_char,
    snapshot_payload: *const c_uchar,
    snapshot_payload_len: usize,
    field_policy: *const c_char,
    dry_run: i32,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_import_jira");
    let workspace = arg_str!(workspace, "loom_import_jira");
    let profile = arg_str!(profile, "loom_import_jira");
    let source_scope = arg_str!(source_scope, "loom_import_jira");
    let snapshot_payload = unsafe { byte_slice(snapshot_payload, snapshot_payload_len) }.to_vec();
    let field_policy = arg_str!(field_policy, "loom_import_jira");
    match generated_bytes(h, |client, session| {
        block_generated(InterchangeProfiles::import_jira(
            client,
            session,
            workspace.to_string(),
            profile.to_string(),
            source_scope.to_string(),
            snapshot_payload,
            field_policy.to_string(),
            dry_run != 0,
        ))
    }) {
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Import Confluence snapshot bytes and return the canonical import report CBOR.
///
/// # Safety
/// `handle` must be from [`loom_open`]; string arguments must be valid C strings; `snapshot_payload`
/// must be null or readable for `snapshot_payload_len` bytes; `out_ptr` and `out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_import_confluence(
    handle: *mut LoomSession,
    workspace: *const c_char,
    profile: *const c_char,
    source_scope: *const c_char,
    snapshot_payload: *const c_uchar,
    snapshot_payload_len: usize,
    default_space: *const c_char,
    dry_run: i32,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_import_confluence");
    let workspace = arg_str!(workspace, "loom_import_confluence");
    let profile = arg_str!(profile, "loom_import_confluence");
    let source_scope = arg_str!(source_scope, "loom_import_confluence");
    let snapshot_payload = unsafe { byte_slice(snapshot_payload, snapshot_payload_len) }.to_vec();
    let default_space = arg_str!(default_space, "loom_import_confluence");
    match generated_bytes(h, |client, session| {
        block_generated(InterchangeProfiles::import_confluence(
            client,
            session,
            workspace.to_string(),
            profile.to_string(),
            source_scope.to_string(),
            snapshot_payload,
            default_space.to_string(),
            dry_run != 0,
        ))
    }) {
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Import Slack snapshot bytes and return the canonical import report CBOR.
///
/// # Safety
/// `handle` must be from [`loom_open`]; string arguments must be valid C strings; `snapshot_payload`
/// must be null or readable for `snapshot_payload_len` bytes; `out_ptr` and `out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_import_slack(
    handle: *mut LoomSession,
    workspace: *const c_char,
    profile: *const c_char,
    source_scope: *const c_char,
    snapshot_payload: *const c_uchar,
    snapshot_payload_len: usize,
    dry_run: i32,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_import_slack");
    let workspace = arg_str!(workspace, "loom_import_slack");
    let profile = arg_str!(profile, "loom_import_slack");
    let source_scope = arg_str!(source_scope, "loom_import_slack");
    let snapshot_payload = unsafe { byte_slice(snapshot_payload, snapshot_payload_len) }.to_vec();
    match generated_bytes(h, |client, session| {
        block_generated(InterchangeProfiles::import_slack(
            client,
            session,
            workspace.to_string(),
            profile.to_string(),
            source_scope.to_string(),
            snapshot_payload,
            dry_run != 0,
        ))
    }) {
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Import Drive archive bytes and return the canonical import report CBOR.
///
/// # Safety
/// `handle` must be from [`loom_open`]; string arguments must be valid C strings; `archive_payload`
/// must be null or readable for `archive_payload_len` bytes; `out_ptr` and `out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_import_drive(
    handle: *mut LoomSession,
    workspace: *const c_char,
    profile: *const c_char,
    source_scope: *const c_char,
    archive_payload: *const c_uchar,
    archive_payload_len: usize,
    dry_run: i32,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_import_drive");
    let workspace = arg_str!(workspace, "loom_import_drive");
    let profile = arg_str!(profile, "loom_import_drive");
    let source_scope = arg_str!(source_scope, "loom_import_drive");
    let archive_payload = unsafe { byte_slice(archive_payload, archive_payload_len) }.to_vec();
    match generated_bytes(h, |client, session| {
        block_generated(InterchangeProfiles::import_drive(
            client,
            session,
            workspace.to_string(),
            profile.to_string(),
            source_scope.to_string(),
            archive_payload,
            dry_run != 0,
        ))
    }) {
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Import Markdown archive bytes and return the canonical import report CBOR.
///
/// # Safety
/// `handle` must be from [`loom_open`]; string arguments must be valid C strings; `archive_payload`
/// must be null or readable for `archive_payload_len` bytes; `out_ptr` and `out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_import_markdown(
    handle: *mut LoomSession,
    workspace: *const c_char,
    profile: *const c_char,
    source_scope: *const c_char,
    archive_payload: *const c_uchar,
    archive_payload_len: usize,
    space: *const c_char,
    dry_run: i32,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_import_markdown");
    let workspace = arg_str!(workspace, "loom_import_markdown");
    let profile = arg_str!(profile, "loom_import_markdown");
    let source_scope = arg_str!(source_scope, "loom_import_markdown");
    let archive_payload = unsafe { byte_slice(archive_payload, archive_payload_len) }.to_vec();
    let space = arg_str!(space, "loom_import_markdown");
    match generated_bytes(h, |client, session| {
        block_generated(InterchangeProfiles::import_markdown(
            client,
            session,
            workspace.to_string(),
            profile.to_string(),
            source_scope.to_string(),
            archive_payload,
            space.to_string(),
            dry_run != 0,
        ))
    }) {
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Import Notion snapshot bytes and return the canonical import report CBOR.
///
/// # Safety
/// `handle` must be from [`loom_open`]; string arguments must be valid C strings; `snapshot_payload`
/// must be null or readable for `snapshot_payload_len` bytes; `out_ptr` and `out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_import_notion(
    handle: *mut LoomSession,
    workspace: *const c_char,
    profile: *const c_char,
    source_scope: *const c_char,
    snapshot_payload: *const c_uchar,
    snapshot_payload_len: usize,
    default_space: *const c_char,
    dry_run: i32,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_import_notion");
    let workspace = arg_str!(workspace, "loom_import_notion");
    let profile = arg_str!(profile, "loom_import_notion");
    let source_scope = arg_str!(source_scope, "loom_import_notion");
    let snapshot_payload = unsafe { byte_slice(snapshot_payload, snapshot_payload_len) }.to_vec();
    let default_space = arg_str!(default_space, "loom_import_notion");
    match generated_bytes(h, |client, session| {
        block_generated(InterchangeProfiles::import_notion(
            client,
            session,
            workspace.to_string(),
            profile.to_string(),
            source_scope.to_string(),
            snapshot_payload,
            default_space.to_string(),
            dry_run != 0,
        ))
    }) {
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Import Arrow IPC bytes through the generated Columnar owner and return the canonical report CBOR.
///
/// # Safety
/// `handle` must be from [`loom_open`]; string arguments must be valid C strings; `payload` must be
/// null or readable for `payload_len` bytes; `out_ptr` and `out_len` writable.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn loom_columnar_import_arrow(
    handle: *mut LoomSession,
    workspace: *const c_char,
    name: *const c_char,
    payload: *const c_uchar,
    payload_len: usize,
    target_segment_rows: u64,
    replace: i32,
    dry_run: i32,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_columnar_import_arrow");
    let workspace = arg_str!(workspace, "loom_columnar_import_arrow");
    let name = arg_str!(name, "loom_columnar_import_arrow");
    let payload = unsafe { byte_slice(payload, payload_len) }.to_vec();
    match generated_bytes(h, |client, session| {
        block_generated(Columnar::columnar_import_arrow(
            client,
            session,
            workspace.to_string(),
            name.to_string(),
            payload,
            target_segment_rows,
            replace != 0,
            dry_run != 0,
        ))
    }) {
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Import Parquet bytes through the generated Columnar owner and return the canonical report CBOR.
///
/// # Safety
/// `handle` must be from [`loom_open`]; string arguments must be valid C strings; `payload` must be
/// null or readable for `payload_len` bytes; `out_ptr` and `out_len` writable.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn loom_columnar_import_parquet(
    handle: *mut LoomSession,
    workspace: *const c_char,
    name: *const c_char,
    payload: *const c_uchar,
    payload_len: usize,
    target_segment_rows: u64,
    replace: i32,
    dry_run: i32,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_columnar_import_parquet");
    let workspace = arg_str!(workspace, "loom_columnar_import_parquet");
    let name = arg_str!(name, "loom_columnar_import_parquet");
    let payload = unsafe { byte_slice(payload, payload_len) }.to_vec();
    match generated_bytes(h, |client, session| {
        block_generated(Columnar::columnar_import_parquet(
            client,
            session,
            workspace.to_string(),
            name.to_string(),
            payload,
            target_segment_rows,
            replace != 0,
            dry_run != 0,
        ))
    }) {
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Upsert vector text through the generated Vector owner and return canonical report CBOR.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `request` must be null or readable for `request_len` bytes;
/// `out_ptr` and `out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_vector_text_upsert(
    handle: *mut LoomSession,
    request: *const c_uchar,
    request_len: usize,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_vector_text_upsert");
    let request = unsafe { byte_slice(request, request_len) }.to_vec();
    match generated_bytes(h, |client, session| {
        block_generated(Vector::vector_text_upsert(client, session, request))
    }) {
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Configure vector workspace binding through the generated Vector owner and return JSON.
///
/// # Safety
/// `handle` must be from [`loom_open`]; string arguments must be valid C strings; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_vector_workspace_configure_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    request_json: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_vector_workspace_configure_json");
    let workspace = arg_str!(workspace, "loom_vector_workspace_configure_json");
    let request_json = arg_str!(request_json, "loom_vector_workspace_configure_json");
    match generated_string(h, |client, session| {
        block_generated(Vector::vector_workspace_configure_json(
            client,
            session,
            workspace.to_string(),
            request_json.to_string(),
        ))
    }) {
        Ok(json) => unsafe { ok_str(out, &json) },
        Err(e) => fail(e),
    }
}

/// Reindex Studio projections through the generated StudioMaintenance owner and return JSON.
///
/// # Safety
/// `handle` must be from [`loom_open`]; string arguments must be valid C strings; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_studio_reindex_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    profile: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_studio_reindex_json");
    let workspace = arg_str!(workspace, "loom_studio_reindex_json");
    let profile = arg_str!(profile, "loom_studio_reindex_json");
    match generated_string(h, |client, session| {
        block_generated(StudioMaintenance::studio_reindex_json(
            client,
            session,
            workspace.to_string(),
            profile.to_string(),
        ))
    }) {
        Ok(json) => unsafe { ok_str(out, &json) },
        Err(e) => fail(e),
    }
}

/// Rebuild Studio revision indexes through the generated StudioMaintenance owner and return JSON.
///
/// # Safety
/// `handle` must be from [`loom_open`]; string arguments must be valid C strings; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_studio_revisions_rebuild_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    profile: *const c_char,
    dry_run: i32,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_studio_revisions_rebuild_json");
    let workspace = arg_str!(workspace, "loom_studio_revisions_rebuild_json");
    let profile = arg_str!(profile, "loom_studio_revisions_rebuild_json");
    match generated_string(h, |client, session| {
        block_generated(StudioMaintenance::studio_revisions_rebuild_json(
            client,
            session,
            workspace.to_string(),
            profile.to_string(),
            dry_run != 0,
        ))
    }) {
        Ok(json) => unsafe { ok_str(out, &json) },
        Err(e) => fail(e),
    }
}

/// Import a store bundle through the generated StoreAdmin owner and return canonical report CBOR.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `bundle` must be null or readable for `bundle_len` bytes;
/// `out_ptr` and `out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_store_bundle_import(
    handle: *mut LoomSession,
    bundle: *const c_uchar,
    bundle_len: usize,
    dry_run: i32,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_store_bundle_import");
    let bundle = unsafe { byte_slice(bundle, bundle_len) }.to_vec();
    match generated_bytes(h, |client, session| {
        block_generated(StoreAdmin::store_bundle_import(
            client,
            session,
            bundle,
            dry_run != 0,
        ))
    }) {
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Compact the audit retention log through the generated Audit owner and return canonical CBOR.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `out_ptr` and `out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_audit_compact(
    handle: *mut LoomSession,
    through_seq: u64,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_audit_compact");
    match generated_bytes(h, |client, session| {
        block_generated(Audit::audit_compact(client, session, through_seq))
    }) {
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Read store maintenance status through the generated StoreAdmin owner and return canonical CBOR.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `request` must be null or readable for `request_len` bytes;
/// `out_ptr` and `out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_store_maintenance_status(
    handle: *mut LoomSession,
    request: *const c_uchar,
    request_len: usize,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_store_maintenance_status");
    let request = unsafe { byte_slice(request, request_len) }.to_vec();
    match generated_bytes(h, |client, session| {
        block_generated(StoreAdmin::store_maintenance_status(
            client, session, request,
        ))
    }) {
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Set store maintenance policy through the generated StoreAdmin owner and return canonical CBOR.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `update` must be null or readable for `update_len` bytes;
/// `out_ptr` and `out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_store_maintenance_policy_set(
    handle: *mut LoomSession,
    update: *const c_uchar,
    update_len: usize,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_store_maintenance_policy_set");
    let update = unsafe { byte_slice(update, update_len) }.to_vec();
    match generated_bytes(h, |client, session| {
        block_generated(StoreAdmin::store_maintenance_policy_set(
            client, session, update,
        ))
    }) {
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Run bounded store maintenance through the generated StoreAdmin owner and return canonical CBOR.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `request` must be null or readable for `request_len` bytes;
/// `out_ptr` and `out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_store_maintenance_run(
    handle: *mut LoomSession,
    request: *const c_uchar,
    request_len: usize,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_store_maintenance_run");
    let request = unsafe { byte_slice(request, request_len) }.to_vec();
    match generated_bytes(h, |client, session| {
        block_generated(StoreAdmin::store_maintenance_run(client, session, request))
    }) {
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Create an inference instance through the generated InferenceInstance owner and return JSON.
///
/// # Safety
/// `handle` must be from [`loom_open`]; string arguments must be valid C strings; optional strings
/// may be null; `out` writable.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn loom_inference_instance_create_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    name: *const c_char,
    model: *const c_char,
    kind: *const c_char,
    runtime: *const c_char,
    preset: *const c_char,
    settings_json: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_inference_instance_create_json");
    let workspace = arg_str!(workspace, "loom_inference_instance_create_json");
    let name = arg_str!(name, "loom_inference_instance_create_json");
    let model = arg_str!(model, "loom_inference_instance_create_json");
    let kind = arg_str!(kind, "loom_inference_instance_create_json");
    let runtime = arg_str!(runtime, "loom_inference_instance_create_json");
    let preset = match unsafe { optional_str_arg(preset, "loom_inference_instance_create_json") } {
        Ok(value) => value.map(str::to_string),
        Err(e) => return fail(e),
    };
    let settings_json = arg_str!(settings_json, "loom_inference_instance_create_json");
    match generated_string(h, |client, session| {
        block_generated(InferenceInstance::inference_instance_create_json(
            client,
            session,
            workspace.to_string(),
            name.to_string(),
            model.to_string(),
            kind.to_string(),
            runtime.to_string(),
            preset,
            settings_json.to_string(),
        ))
    }) {
        Ok(json) => unsafe { ok_str(out, &json) },
        Err(e) => fail(e),
    }
}

/// Update an inference instance through the generated InferenceInstance owner and return JSON.
///
/// # Safety
/// `handle` must be from [`loom_open`]; string arguments must be valid C strings; optional strings
/// may be null; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_inference_instance_update_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    name: *const c_char,
    preset: *const c_char,
    settings_json: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_inference_instance_update_json");
    let workspace = arg_str!(workspace, "loom_inference_instance_update_json");
    let name = arg_str!(name, "loom_inference_instance_update_json");
    let preset = match unsafe { optional_str_arg(preset, "loom_inference_instance_update_json") } {
        Ok(value) => value.map(str::to_string),
        Err(e) => return fail(e),
    };
    let settings_json = arg_str!(settings_json, "loom_inference_instance_update_json");
    match generated_string(h, |client, session| {
        block_generated(InferenceInstance::inference_instance_update_json(
            client,
            session,
            workspace.to_string(),
            name.to_string(),
            preset,
            settings_json.to_string(),
        ))
    }) {
        Ok(json) => unsafe { ok_str(out, &json) },
        Err(e) => fail(e),
    }
}

/// Delete an inference instance through the generated InferenceInstance owner and return JSON.
///
/// # Safety
/// `handle` must be from [`loom_open`]; string arguments must be valid C strings; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_inference_instance_delete_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    name: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_inference_instance_delete_json");
    let workspace = arg_str!(workspace, "loom_inference_instance_delete_json");
    let name = arg_str!(name, "loom_inference_instance_delete_json");
    match generated_string(h, |client, session| {
        block_generated(InferenceInstance::inference_instance_delete_json(
            client,
            session,
            workspace.to_string(),
            name.to_string(),
        ))
    }) {
        Ok(json) => unsafe { ok_str(out, &json) },
        Err(e) => fail(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn cs(s: &str) -> CString {
        CString::new(s).unwrap()
    }

    fn last_err() -> Option<(i32, String)> {
        let mut code = 0i32;
        let mut msg: *mut c_char = core::ptr::null_mut();
        let mut len = 0usize;
        unsafe { crate::loom_last_error(&mut code, &mut msg, &mut len) };
        if msg.is_null() {
            return None;
        }
        let s = unsafe { CStr::from_ptr(msg) }.to_str().unwrap().to_string();
        assert_eq!(len, s.len());
        unsafe { crate::loom_string_free(msg) };
        Some((code, s))
    }

    fn temp_loom() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let uniq = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "loom-ffi-mu6i-d6-{}-{seq}-{uniq}.loom",
            std::process::id()
        ))
    }

    fn open_fresh() -> (std::path::PathBuf, *mut LoomSession) {
        let dir = temp_loom();
        let path = cs(dir.to_str().unwrap());
        let dflt = cs("default");
        assert_eq!(
            unsafe {
                crate::loom_create(
                    path.as_ptr(),
                    dflt.as_ptr(),
                    core::ptr::null(),
                    core::ptr::null(),
                    0,
                )
            },
            0,
            "create failed: {:?}",
            last_err()
        );
        let mut handle: *mut LoomSession = core::ptr::null_mut();
        assert_eq!(unsafe { crate::loom_open(path.as_ptr(), &mut handle) }, 0);
        (dir, handle)
    }

    unsafe fn take_buf(status: i32, p: *mut c_uchar, n: usize) -> Vec<u8> {
        assert_eq!(status, 0, "status {status}, err {:?}", last_err());
        let v = unsafe { std::slice::from_raw_parts(p, n) }.to_vec();
        unsafe { crate::loom_bytes_free(p, n) };
        v
    }

    fn binding_contract_methods() -> Vec<(String, String)> {
        let contract: serde_json::Value =
            serde_json::from_str(include_str!("../../../idl/binding-targets.json"))
                .expect("binding target contract is valid JSON");
        let idl = include_str!("../../../idl/loom.idl");
        assert_eq!(contract["schema_version"], 1);
        assert_eq!(
            contract["native_targets"],
            serde_json::json!([
                "c_abi",
                "cpp",
                "jvm",
                "android",
                "ios",
                "react_native",
                "nodejs",
                "python"
            ])
        );
        assert_eq!(
            contract["wasm_capability_gated"]["reason"],
            "profile_unsupported"
        );
        assert_eq!(contract["wasm_capability_gated"]["code"], "UNSUPPORTED");
        contract["methods"]
            .as_array()
            .expect("contract method array")
            .iter()
            .map(|entry| {
                let name = entry["name"]
                    .as_str()
                    .expect("contract method has a qualified name");
                let (interface, method) =
                    name.split_once('.').expect("contract method has interface");
                assert!(idl_has_method(idl, interface, method), "{name} is in IDL");
                let wasm = entry["wasm"]
                    .as_str()
                    .expect("contract method has WASM disposition");
                assert!(wasm == "supported" || wasm == "capability_gated");
                (interface.to_string(), method.to_string())
            })
            .collect()
    }

    fn idl_has_method(idl: &str, interface: &str, method: &str) -> bool {
        let Some(interface_start) = idl.find(&format!("interface {interface} {{")) else {
            return false;
        };
        let interface_tail = &idl[interface_start..];
        let Some(interface_end) = interface_tail.find("\n}") else {
            return false;
        };
        interface_tail[..interface_end].contains(&format!(" {method}("))
    }

    fn promoted_c_name(interface: &str, method: &str) -> String {
        match (interface, method) {
            ("InterchangeProfiles", _) => format!("loom_{method}"),
            ("Exec", "apply_cbor") => "loom_apply_cbor".to_string(),
            ("Meetings", "meetings_import_snapshot") => "loom_meetings_import_snapshot".to_string(),
            ("Drive", "drive_read_file") => "loom_drive_read".to_string(),
            _ => format!("loom_{method}"),
        }
    }

    #[test]
    fn binding_target_contract_matches_promoted_c_abi_inventory() {
        let generated_local = include_str!("generated_local.rs");
        let drive = include_str!("drive.rs");
        let chat = include_str!("chat.rs");
        let generated = include_str!("../../loom-remote-protocol/src/generated_api.rs");
        let header = include_str!("../../../include/loom.h");
        let methods = binding_contract_methods();
        assert_eq!(methods.len(), 84);
        let mut unique_methods = std::collections::BTreeSet::new();
        let mut unique_c_names = std::collections::BTreeSet::new();
        for (interface, method) in &methods {
            assert!(unique_methods.insert(format!("{interface}.{method}")));
            let c_name = promoted_c_name(interface, method);
            assert!(unique_c_names.insert(c_name.clone()));
            let source = if c_name.starts_with("loom_drive_") || c_name == "loom_drive_read" {
                drive
            } else if c_name.starts_with("loom_chat_") {
                chat
            } else {
                generated_local
            };
            assert_eq!(
                source.matches(&format!("fn {c_name}(")).count(),
                1,
                "{c_name} Rust C export"
            );
            assert_eq!(
                header.matches(&format!("{c_name}(")).count(),
                1,
                "{c_name} header declaration"
            );
            assert_eq!(
                generated.matches(&format!("fn {method}(")).count(),
                1,
                "{interface}.{method} generated trait method"
            );
        }
    }

    #[test]
    fn mu6i_d6_audit_and_store_admin_generated_c_abi_bytes() {
        let (dir, handle) = open_fresh();
        let mut out_ptr: *mut c_uchar = core::ptr::null_mut();
        let mut out_len = 0usize;

        let compact = unsafe {
            take_buf(
                loom_audit_compact(handle, 0, &mut out_ptr, &mut out_len),
                out_ptr,
                out_len,
            )
        };
        let compact = loom_wire::audit::audit_compact_result_from_cbor(&compact)
            .expect("audit compact result cbor");
        assert_eq!(compact.pruned, 0);
        assert_eq!(compact.checkpoint_seq, None);

        let status_request = loom_wire::store_admin::store_maintenance_status_request_to_cbor(
            &loom_wire::store_admin::StoreMaintenanceStatusRequest {
                include_live_root_diagnostics: false,
            },
        );
        let status = unsafe {
            take_buf(
                loom_store_maintenance_status(
                    handle,
                    status_request.as_ptr(),
                    status_request.len(),
                    &mut out_ptr,
                    &mut out_len,
                ),
                out_ptr,
                out_len,
            )
        };
        let status = loom_wire::store_admin::store_maintenance_status_result_from_cbor(&status)
            .expect("maintenance status result cbor");
        let default_policy = status.report.policy;
        assert!(status.live_root_diagnostics.is_none());

        let update = loom_wire::store_admin::store_maintenance_policy_update_to_cbor(
            &loom_wire::store_admin::StoreMaintenancePolicyUpdate {
                min_candidate_pages: None,
                min_reusable_pages: None,
                interval_ms: None,
                backoff_ms: None,
                max_segments: None,
                max_pages: Some(77),
                full_compaction_enabled: None,
                tail_trim_enabled: Some(false),
                tail_compaction_enabled: None,
                tail_compaction_max_pages: None,
                tail_compaction_max_objects: None,
                tail_compaction_max_bytes: None,
                tail_compaction_interval_ms: None,
                tail_compaction_backoff_ms: None,
            },
        );
        let updated = unsafe {
            take_buf(
                loom_store_maintenance_policy_set(
                    handle,
                    update.as_ptr(),
                    update.len(),
                    &mut out_ptr,
                    &mut out_len,
                ),
                out_ptr,
                out_len,
            )
        };
        let updated = loom_wire::store_admin::store_maintenance_status_result_from_cbor(&updated)
            .expect("maintenance policy-set result cbor");
        assert_eq!(updated.report.policy.max_pages, 77);
        assert!(!updated.report.policy.tail_trim_enabled);
        assert_eq!(
            updated.report.policy.min_candidate_pages,
            default_policy.min_candidate_pages
        );
        assert_eq!(
            updated.report.policy.min_reusable_pages,
            default_policy.min_reusable_pages
        );
        assert_eq!(
            updated.report.policy.interval_ms,
            default_policy.interval_ms
        );

        let run_request = loom_wire::store_admin::store_maintenance_run_request_to_cbor(
            &loom_wire::store_admin::StoreMaintenanceRunRequest {
                max_segments: Some(1),
                max_pages: Some(1),
            },
        );
        let run = unsafe {
            take_buf(
                loom_store_maintenance_run(
                    handle,
                    run_request.as_ptr(),
                    run_request.len(),
                    &mut out_ptr,
                    &mut out_len,
                ),
                out_ptr,
                out_len,
            )
        };
        let run = loom_wire::store_admin::store_maintenance_run_result_from_cbor(&run)
            .expect("maintenance run result cbor");
        assert!(matches!(
            run.kind,
            loom_wire::store_admin::StoreMaintenanceRunKind::Skipped
                | loom_wire::store_admin::StoreMaintenanceRunKind::Marked
                | loom_wire::store_admin::StoreMaintenanceRunKind::Compacted
                | loom_wire::store_admin::StoreMaintenanceRunKind::Reclaimed
        ));

        let malformed = [0xffu8];
        out_ptr = core::ptr::null_mut();
        out_len = 0;
        let malformed_status = unsafe {
            loom_store_maintenance_status(
                handle,
                malformed.as_ptr(),
                malformed.len(),
                &mut out_ptr,
                &mut out_len,
            )
        };
        assert_eq!(malformed_status, Code::InvalidArgument.as_i32());
        assert!(out_ptr.is_null());
        assert_eq!(out_len, 0);
        let err = last_err().expect("malformed status error");
        assert_eq!(err.0, Code::InvalidArgument.as_i32());

        out_ptr = core::ptr::null_mut();
        out_len = 0;
        let null_status = unsafe {
            loom_store_maintenance_run(
                core::ptr::null_mut(),
                run_request.as_ptr(),
                run_request.len(),
                &mut out_ptr,
                &mut out_len,
            )
        };
        assert_eq!(null_status, Code::InvalidArgument.as_i32());
        assert!(out_ptr.is_null());
        assert_eq!(out_len, 0);
        assert!(
            last_err()
                .expect("null handle error")
                .1
                .contains("null handle")
        );

        unsafe { crate::loom_close(handle) };
        let _ = std::fs::remove_file(&dir);
    }
}
