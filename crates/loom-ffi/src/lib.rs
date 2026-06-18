//! C ABI for Uldren Loom - the stable contract every language binding wraps (Node, JVM, C/C++, WASM).
//!
//! Error contract: every fallible function returns an `int32` status - `0` on success,
//! else the stable [`loom_core::error::Code`] as an integer (see `Code::as_i32`). Results are written
//! through out-pointers. On error a thread-local record is set, retrievable via [`loom_last_error`]
//! (code + message) and [`loom_last_error_details`] (canonical CBOR details). Owned strings/handles
//! returned through out-pointers belong to the caller and must be freed with [`loom_string_free`] /
//! [`loom_sql_close`] / [`loom_close`]; buffers passed in are
//! borrowed for the duration of the call only.
//!
//! Structured result payloads cross as **Loom Canonical CBOR** bytes, returned through an
//! `(out_ptr, out_len)` pair and freed with [`loom_bytes_free`]. JSON is debug-only: [`loom_result_to_json`]
//! renders any result buffer to text on demand. Text scalars whose value is genuinely a string -
//! [`loom_version`], [`loom_last_error`], [`loom_blob_digest`], and the `algo:hex` commit addresses
//! from `loom_commit` / [`loom_sql_commit`] - remain C strings.
//!
//! Async model (poll/handle form). The engine is synchronous, so the async surface is the
//! **portable cooperative** primitive: an op like [`loom_sql_exec_async`] returns a pending
//! [`LoomTask`] that does no work until polled. [`loom_task_poll`] runs the op to completion - **the
//! first poll MAY block** on the calling thread under this backend (there is no core worker pool), so
//! a binding's async wrapper MUST drive `poll` / wait off the event loop or UI thread (its own worker:
//! libuv pool, executor, Web Worker). [`loom_task_cancel`] is guaranteed only while the task is still
//! pending (a polled task runs to completion). [`loom_task_result`] transfers exactly one owned
//! `(ptr, len)` buffer (free with [`loom_bytes_free`]) or reports the task's stored error. The task
//! handle is freed with [`loom_task_free`], which is separate from [`loom_bytes_free`] (a task owns its
//! handle; a transferred result buffer is owned by the caller).
//!
//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use core::ffi::{c_char, c_uchar};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::ffi::{CStr, CString};
use std::time::{SystemTime, UNIX_EPOCH};

use loom_codec::{Value as CborValue, encode as cbor_encode};
use loom_core::calendar::{CalendarEntry, CollectionMeta, Component};
use loom_core::contacts::{BookMeta, ContactEntry};
use loom_core::error::{Code, LoomError, Result as LoomResult};
use loom_core::keys::{EncryptionMeta, KeySpec, Suite};
use loom_core::mail::{MailMessage, MailboxMeta};
use loom_core::tabular::Value;
use loom_core::vcs::ChangeKind;
use loom_core::workspace::{FacetKind, WorkspaceId};
use loom_core::{
    AclDomain, AclEffect, AclGrant, AclPredicate, AclRight, AclScope, AclScopeKind, AclStore,
    AclSubject, Algo, Digest, DocumentFieldPath, DocumentIndexDef, EphemeralPutOptions,
    ExternalCredential, ExternalCredentialKind, ExternalCredentialSpec, IdentityPublicKeySpec,
    IdentityRole, IdentityStore, KvMapConfig, KvTier, Loom, Object, OpenMode, Principal,
    PrincipalKind, ProtectedRefPolicy, ReplayOutcome, Stream, WatchCursor, WatchSelector,
    WsSelector, cas_delete, cas_get, cas_has, cas_list, cas_put, doc_create_index, doc_delete,
    doc_drop_index, doc_find, doc_index_statuses, doc_query, doc_rebuild_index,
    document_get_binary, document_get_text, document_ids_json, document_index_statuses_json,
    document_index_value_from_json, document_list_binary, document_query_from_json,
    document_query_result_json, key_from_cbor, kv_delete, kv_get, kv_list, kv_put, kv_range,
    ledger_append, ledger_get, ledger_head, ledger_len, ledger_verify, ts_get, ts_latest, ts_put,
    ts_range, watch_batch_to_cbor,
};
use loom_result::result_view;
use loom_result::result_view::{Merge, Reader, ResultPayload, RowChange, ShowVariable, Statement};
use loom_sql::{LoomSqlStore, lookup_cbor, result_cbor};
use loom_store::{
    FileStore, LocalOpenAuth, attach_local_auth, daemon,
    open_loom_read_unlocked as store_open_loom_read_unlocked,
    open_loom_unlocked as store_open_loom_unlocked, save_loom,
};

#[macro_use]
mod macros;

/// Return the library version as a newly-allocated C string. Free with [`loom_string_free`].
/// Infallible, so it returns the string directly rather than a status.
#[unsafe(no_mangle)]
pub extern "C" fn loom_version() -> *mut c_char {
    to_c_string(loom_core::VERSION)
}

/// Compute the Blob content address (`"algo:hex"`) of `len` bytes at `data` and return it as a
/// newly-allocated C string (free with [`loom_string_free`]). Returns null on invalid input.
/// Effectively infallible (a hex address has no interior NUL), so it returns the string directly.
///
/// # Safety
/// `data` must point to at least `len` readable bytes, or be null when `len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_blob_digest(data: *const c_uchar, len: usize) -> *mut c_char {
    let bytes: &[u8] = if len == 0 {
        &[]
    } else if data.is_null() {
        return core::ptr::null_mut();
    } else {
        // SAFETY: caller guarantees `data` is valid for `len` bytes (see fn docs).
        unsafe { core::slice::from_raw_parts(data, len) }
    };
    to_c_string(&Object::Blob(bytes.to_vec()).digest().to_string())
}

/// Free a string previously returned by this library. Passing null is a no-op.
///
/// # Safety
/// `s` must be a pointer returned by this library and not previously freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_string_free(s: *mut c_char) {
    if !s.is_null() {
        // SAFETY: `s` came from `CString::into_raw` in this library (see fn docs).
        unsafe {
            drop(CString::from_raw(s));
        }
    }
}

/// Allocate a C string from `s`, transferring ownership to the caller. Null on interior NUL.
fn to_c_string(s: &str) -> *mut c_char {
    match CString::new(s) {
        Ok(c) => c.into_raw(),
        Err(_) => core::ptr::null_mut(),
    }
}

/// Encode a canonical-CBOR array of per-record `encode()` byte strings (the list/search wire form).
/// Shared by the calendar/contacts/mail facet modules.
fn records_cbor(records: Vec<Vec<u8>>) -> LoomResult<Vec<u8>> {
    loom_wire::bytes_list_to_cbor(records)
}

// ---------------------------------------------------------------------------------------------------
// Error contract: int32 status + thread-local LoomError, surfaced by loom_last_error.
// ---------------------------------------------------------------------------------------------------

thread_local! {
    /// The most recent error on this thread.
    /// Every fallible entry point clears it on success and sets it before returning a non-zero status.
    static LAST_ERROR: RefCell<Option<LoomError>> = const { RefCell::new(None) };
}

fn clear_error() {
    LAST_ERROR.with(|e| *e.borrow_mut() = None);
}

/// Record an engine error and return its stable numeric status.
fn fail(e: LoomError) -> i32 {
    let code = e.code.as_i32();
    LAST_ERROR.with(|c| *c.borrow_mut() = Some(e));
    code
}

/// Record an ABI-layer argument error (null / non-UTF-8 pointer) as `INVALID_ARGUMENT`.
fn fail_arg(msg: &str) -> i32 {
    fail(LoomError::invalid(msg))
}

/// Write an owned C string for `s` into `*out` (when non-null) and return success (`0`).
///
/// # Safety
/// `out` must be null or a valid `*mut *mut c_char` the caller owns.
unsafe fn ok_str(out: *mut *mut c_char, s: &str) -> i32 {
    if !out.is_null() {
        // SAFETY: caller guarantees `out` is a valid writable pointer (see fn docs).
        unsafe {
            *out = to_c_string(s);
        }
    }
    0
}

/// Hand `bytes` to the caller as an owned `(ptr, len)` buffer (free with [`loom_bytes_free`]) and
/// return success (`0`). The canonical-CBOR result-payload return convention. If `out_ptr`
/// is null the buffer is reclaimed immediately rather than leaked.
///
/// # Safety
/// `out_ptr` / `out_len` must each be null or a valid writable pointer of the matching type.
unsafe fn ok_bytes(out_ptr: *mut *mut c_uchar, out_len: *mut usize, bytes: Vec<u8>) -> i32 {
    let boxed = bytes.into_boxed_slice();
    let len = boxed.len();
    let ptr = Box::into_raw(boxed).cast::<c_uchar>();
    if out_ptr.is_null() {
        // SAFETY: `ptr`/`len` came from the boxed slice just above; reclaim to avoid a leak.
        unsafe { drop(Box::from_raw(core::ptr::slice_from_raw_parts_mut(ptr, len))) };
    } else {
        // SAFETY: caller guarantees `out_ptr` is writable (see fn docs).
        unsafe { *out_ptr = ptr };
    }
    if !out_len.is_null() {
        // SAFETY: caller guarantees `out_len` is writable (see fn docs).
        unsafe { *out_len = len };
    }
    0
}

/// Free a byte buffer previously returned by a result function (its `out_ptr` + `out_len`). Passing a
/// null pointer is a no-op.
///
/// # Safety
/// `ptr`/`len` must be a buffer this library returned through an `(out_ptr, out_len)` pair, not
/// previously freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_bytes_free(ptr: *mut c_uchar, len: usize) {
    if !ptr.is_null() {
        // SAFETY: `ptr`/`len` came from `ok_bytes`'s boxed slice of exactly `len` bytes (see fn docs).
        unsafe { drop(Box::from_raw(core::ptr::slice_from_raw_parts_mut(ptr, len))) };
    }
}

/// Write the capability registry (0010 §5) as canonical CBOR to `(*out_ptr, *out_len)` (free with
/// [`loom_bytes_free`]): a `CapabilitySet` map with `schema_version` and `records`. Each record carries
/// `capability_id`, `current`, `minimum_compatible`, `owning_specs`, `owner_module`, `dimensions`,
/// `proof_status`, `operational_state`, `reason_code`, and `stable_error`. The report is build-aware: this build
/// links `loom-store`, `loom-sql`, and `loom-lanes`, so the capabilities those crates own are reported with
/// operational state `supported`. No handle is required because capabilities are a property of the
/// build, not of an open store.
///
/// # Safety
/// `out_ptr`/`out_len` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_capabilities(out_ptr: *mut *mut c_uchar, out_len: *mut usize) -> i32 {
    clear_error();
    let set = loom_core::capability::registry()
        .with_state_overlay(
            loom_store::provided_capabilities(),
            loom_core::CapabilityOperationalState::Supported,
        )
        .with_state_overlay(
            loom_sql::provided_capabilities(),
            loom_core::CapabilityOperationalState::Supported,
        )
        .with_state_overlay(
            loom_lanes::provided_capabilities(),
            loom_core::CapabilityOperationalState::Supported,
        );
    // SAFETY: `out_ptr`/`out_len` are writable per fn docs.
    unsafe { ok_bytes(out_ptr, out_len, set.to_cbor()) }
}

/// Write the runtime provider/profile report as canonical CBOR to `(*out_ptr, *out_len)` (free with
/// [`loom_bytes_free`]). The report describes the linked native artifact: binary channel, runtime
/// policy, default identity profile, crypto provider, TLS provider, FIPS capability, and FIPS TLS
/// claim.
///
/// # Safety
/// `out_ptr`/`out_len` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_runtime_profile(
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    // SAFETY: `out_ptr`/`out_len` are writable per fn docs.
    unsafe { ok_bytes(out_ptr, out_len, loom_core::runtime_profile().to_cbor()) }
}

/// Write the Studio app catalog as JSON to `*out` (free with [`loom_string_free`]).
///
/// # Safety
/// `workspace` and `set` must be valid C strings; `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_studio_surface_catalog_json(
    workspace: *const c_char,
    set: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let workspace = arg_str!(workspace, "loom_studio_surface_catalog_json");
    let set = arg_str!(set, "loom_studio_surface_catalog_json");
    match loom_substrate::surfaces::surface_catalog_json(workspace, set) {
        // SAFETY: `out` is writable per fn docs.
        Ok(json) => unsafe { ok_str(out, &json) },
        Err(error) => fail(error),
    }
}

/// Render a canonical-CBOR result buffer to JSON text (debug only); writes an owned C string to `*out`
/// (free with [`loom_string_free`]) and returns `0`. Bindings consume the CBOR directly; this is for
/// inspecting a payload by eye.
///
/// # Safety
/// `ptr` must point to `len` readable bytes (or be null when `len == 0`); `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_result_to_json(
    ptr: *const c_uchar,
    len: usize,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let bytes: &[u8] = if len == 0 {
        &[]
    } else if ptr.is_null() {
        return fail_arg("loom_result_to_json: null buffer");
    } else {
        // SAFETY: caller guarantees `ptr` is valid for `len` bytes (see fn docs).
        unsafe { core::slice::from_raw_parts(ptr, len) }
    };
    match loom_result::result_to_json(bytes) {
        // SAFETY: `out` is writable per fn docs.
        Ok(s) => unsafe { ok_str(out, &s) },
        Err(e) => fail(e),
    }
}

/// Render a canonical result buffer to **lossless bridge JSON** (a React Native bridge projection -
/// NOT the normative wire form, and NOT the general typed binding API; bindings other than RN use the
/// result-view). Big ints, decimals, uuids, inet, bytes (base64), `f32`, non-finite/`-0.0` `f64`, and
/// points cross as tagged `$`-prefixed objects so a JS bridge with no `BigInt`/`Uint8Array` can
/// reconstruct them losslessly; the RN native layer returns this string and TS `JSON.parse`s it. Built
/// on the one shared `result_view` decoder, so CBOR decoding and the cell tag table stay in Rust.
/// Writes an owned C string to `*out` (free with [`loom_string_free`]).
///
/// # Safety
/// `ptr` must point to `len` readable bytes (or be null when `len == 0`); `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_result_to_bridge_json(
    ptr: *const c_uchar,
    len: usize,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let bytes: &[u8] = if len == 0 {
        &[]
    } else if ptr.is_null() {
        return fail_arg("loom_result_to_bridge_json: null buffer");
    } else {
        // SAFETY: caller guarantees `ptr` is valid for `len` bytes (see fn docs).
        unsafe { core::slice::from_raw_parts(ptr, len) }
    };
    match loom_result::to_bridge_json(bytes) {
        // SAFETY: `out` is writable per fn docs.
        Ok(s) => unsafe { ok_str(out, &s) },
        Err(e) => fail(e),
    }
}

/// Borrow a C string argument as `&str`, or `None` if null or not valid UTF-8.
///
/// # Safety
/// `p` must be null or point to a NUL-terminated C string valid for the duration of the call.
unsafe fn cstr<'a>(p: *const c_char) -> Option<&'a str> {
    if p.is_null() {
        return None;
    }
    // SAFETY: caller guarantees `p` is a valid NUL-terminated C string (see fn docs).
    unsafe { CStr::from_ptr(p) }.to_str().ok()
}

/// Borrow a `(ptr, len)` byte buffer as `&[u8]`; a null pointer or zero length yields an empty slice.
///
/// # Safety
/// When non-null with `len > 0`, `(ptr, len)` must be a readable buffer valid for the borrow.
unsafe fn byte_slice<'a>(ptr: *const c_uchar, len: usize) -> &'a [u8] {
    if ptr.is_null() || len == 0 {
        &[]
    } else {
        // SAFETY: caller guarantees `(ptr, len)` is a readable buffer (see each fn's docs).
        unsafe { core::slice::from_raw_parts(ptr, len) }
    }
}

/// Retrieve this thread's most recent error: writes the stable numeric `code` (`0` if the last call
/// succeeded), an owned message C string (free with [`loom_string_free`]; null when no error), and the
/// message byte length. Any out-pointer may be null to skip that field.
///
/// # Safety
/// Each non-null out-pointer must be a valid, writable pointer of the matching type.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_last_error(
    out_code: *mut i32,
    out_msg: *mut *mut c_char,
    out_len: *mut usize,
) {
    LAST_ERROR.with(|e| match e.borrow().as_ref() {
        Some(err) => {
            // SAFETY: caller guarantees each non-null out-pointer is writable (see fn docs).
            unsafe {
                if !out_code.is_null() {
                    *out_code = err.code.as_i32();
                }
                if !out_len.is_null() {
                    *out_len = err.to_string().len();
                }
                if !out_msg.is_null() {
                    *out_msg = to_c_string(&err.to_string());
                }
            }
        }
        None => {
            // SAFETY: caller guarantees each non-null out-pointer is writable (see fn docs).
            unsafe {
                if !out_code.is_null() {
                    *out_code = 0;
                }
                if !out_len.is_null() {
                    *out_len = 0;
                }
                if !out_msg.is_null() {
                    *out_msg = core::ptr::null_mut();
                }
            }
        }
    });
}

/// Retrieve this thread's most recent structured error details as canonical CBOR bytes. The returned
/// buffer is null with length `0` when there is no last error or the last error has no details.
///
/// # Safety
/// `out_ptr` and `out_len` must each be null or a valid writable pointer of the matching type.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_last_error_details(out_ptr: *mut *mut c_uchar, out_len: *mut usize) {
    let details = LAST_ERROR.with(|e| e.borrow().as_ref().and_then(LoomError::details_cbor));
    match details {
        Some(bytes) => {
            // SAFETY: caller guarantees `out_ptr`/`out_len` are writable (see fn docs).
            unsafe {
                let _ = ok_bytes(out_ptr, out_len, bytes);
            }
        }
        None => unsafe {
            if !out_ptr.is_null() {
                *out_ptr = core::ptr::null_mut();
            }
            if !out_len.is_null() {
                *out_len = 0;
            }
        },
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn install_acl_predicate_evaluator(loom: &mut Loom<FileStore>) {
    loom.set_acl_predicate_evaluator(std::sync::Arc::new(loom_compute::CelAclPredicateEvaluator));
}

/// Classify a Loom locator for the C ABI's local-vs-remote split.
///
/// A local filesystem path is returned unchanged; a `file://` URL is stripped to its local path. An
/// `http(s)://` URL (or any remote locator) routes to the remote branch, which the C ABI does not wire:
/// with the `remote` feature off (default) it returns a stable "remote requires the remote feature" error;
/// with `remote` on it returns a "not yet wired" error. Alias TOML is not consulted; a bare non-path
/// string stays a local path, so no alias config is read.
fn normalize_locator(locator: &str) -> LoomResult<String> {
    use loom_core::error::{Code, LoomError};
    if let Some(rest) = locator.strip_prefix("file://") {
        return Ok(rest.to_string());
    }
    if locator.starts_with("https://") || locator.starts_with("http://") {
        #[cfg(not(feature = "remote"))]
        {
            return Err(LoomError::new(
                Code::Unsupported,
                "remote Loom locators require the remote feature in this binding",
            ));
        }
        #[cfg(feature = "remote")]
        {
            return Err(LoomError::new(
                Code::Unsupported,
                "remote Loom locators are not yet wired in this binding (constructor surface only)",
            ));
        }
    }
    Ok(locator.to_string())
}

fn open_loom_unlocked(path: &str, key: Option<&KeySpec>) -> LoomResult<Loom<FileStore>> {
    let path = normalize_locator(path)?;
    let mut loom = store_open_loom_unlocked(&path, key)?;
    install_acl_predicate_evaluator(&mut loom);
    Ok(loom)
}

fn open_loom_read_unlocked(path: &str, key: Option<&KeySpec>) -> LoomResult<Loom<FileStore>> {
    let path = normalize_locator(path)?;
    let mut loom = store_open_loom_read_unlocked(&path, key)?;
    install_acl_predicate_evaluator(&mut loom);
    Ok(loom)
}

fn daemon_status_json(path: &str) -> LoomResult<String> {
    let paths = daemon::paths(path)?;
    Ok(daemon::status_json(&paths))
}

fn lock_token_json(response: &str) -> LoomResult<String> {
    daemon::lock_response_json(response)
}

/// A deterministic workspace id from the SQL session workspace name, matching the `loom` CLI so the
/// same name resolves to the same workspace across the CLI and every binding.
fn derive_sql_ns_id(name: &str) -> WorkspaceId {
    let d = Digest::blake3(format!("{}:{name}", FacetKind::Sql.as_str()).as_bytes());
    let mut id = [0u8; 16];
    id.copy_from_slice(&d.bytes()[..16]);
    WorkspaceId::from_bytes(id)
}

// ---------------------------------------------------------------------------------------------------
// SQL session + batch surface (sync verbs). The
// streaming/async SQL forms below reuse `LoomSqlSession` via this re-export.
// ---------------------------------------------------------------------------------------------------
mod sql_session;
pub use sql_session::*;
// The streaming/async SQL forms and direct's keyed opens reach these `pub(crate)` SQL helpers here.
use sql_session::{exec_session, kek_arg, load_store_read};

// ---------------------------------------------------------------------------------------------------
// Streaming iterators - the row-at-a-time reader surface bindings wrap as async iterators.
// ---------------------------------------------------------------------------------------------------

/// A forward-only iterator over result items, each a self-describing canonical-CBOR buffer.
/// A binding wraps it as an `AsyncIterable` / `Stream` and pulls one item per `loom_iter_next`, so a
/// large result never has to materialize in the foreign runtime. The rows are computed eagerly into
/// this handle. Opaque to C; create with `loom_sql_query`, free with `loom_iter_free`.
pub struct LoomIter {
    items: Vec<Vec<u8>>,
    pos: usize,
}

fn query_session(session: &LoomSqlSession, sql: &str) -> LoomResult<Vec<Vec<u8>>> {
    let ns = derive_sql_ns_id(&session.ns_name);
    let mut store = load_store_read(&session.path, ns, &session.db, &session.auth)?;
    let rows = store.select_rows_cbor(sql)?;
    if store.in_transaction() {
        return Err(LoomError::invalid(
            "BEGIN without a matching COMMIT/ROLLBACK in one query: open a LoomSqlBatch to run a transaction across statements",
        ));
    }
    if store.is_dirty() {
        return Err(LoomError::new(
            Code::PermissionDenied,
            "sql_query is read-only; use sql_exec for statements that mutate state",
        ));
    }
    Ok(rows)
}

/// Run SQL and open a streaming iterator over the rows of its first `SELECT` (each row a canonical-CBOR
/// cell array). On success writes an owned iterator to `*out` (free with [`loom_iter_free`]) and returns
/// `0`; on failure a non-zero [`Code`] (see [`loom_last_error`]). Statements that mutate state are
/// rejected with `PERMISSION_DENIED`.
///
/// # Safety
/// `session` must be from [`loom_sql_open`] and live; `sql` a valid C string; `out` a writable `*mut *mut LoomIter`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_sql_query(
    session: *mut LoomSqlSession,
    sql: *const c_char,
    out: *mut *mut LoomIter,
) -> i32 {
    clear_error();
    // SAFETY: caller guarantees `session` is from `loom_sql_open` and live (see fn docs).
    let Some(session) = (unsafe { session.as_ref() }) else {
        return fail_arg("loom_sql_query: null session");
    };
    // SAFETY: caller guarantees `sql` is a valid C string (see fn docs).
    let Some(sql) = (unsafe { cstr(sql) }) else {
        return fail_arg("loom_sql_query: null or non-UTF-8 sql");
    };
    match query_session(session, sql) {
        Ok(items) => {
            if !out.is_null() {
                // SAFETY: caller guarantees `out` is writable (see fn docs).
                unsafe { *out = Box::into_raw(Box::new(LoomIter { items, pos: 0 })) };
            }
            0
        }
        Err(e) => fail(e),
    }
}

/// Advance a streaming iterator. On success returns `0` and sets `*done`: when there is a next item,
/// `*done = 0` and the item's canonical-CBOR bytes are written to `(*out_ptr, *out_len)` (free with
/// [`loom_bytes_free`]); when the stream is exhausted, `*done = 1` and `(*out_ptr, *out_len)` are
/// `(null, 0)`. Returns a non-zero [`Code`] only on a usage error (e.g. a null iterator).
///
/// # Safety
/// `it` must be from [`loom_sql_query`] and live; `out_ptr`/`out_len`/`done` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_iter_next(
    it: *mut LoomIter,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
    done: *mut i32,
) -> i32 {
    clear_error();
    // SAFETY: caller guarantees `it` is from `loom_sql_query` and live (see fn docs).
    let Some(it) = (unsafe { it.as_mut() }) else {
        return fail_arg("loom_iter_next: null iterator");
    };
    if it.pos >= it.items.len() {
        if !done.is_null() {
            // SAFETY: `done` is writable per fn docs.
            unsafe { *done = 1 };
        }
        if !out_ptr.is_null() {
            // SAFETY: `out_ptr` is writable per fn docs.
            unsafe { *out_ptr = core::ptr::null_mut() };
        }
        if !out_len.is_null() {
            // SAFETY: `out_len` is writable per fn docs.
            unsafe { *out_len = 0 };
        }
        return 0;
    }
    let bytes = it.items[it.pos].clone();
    it.pos += 1;
    if !done.is_null() {
        // SAFETY: `done` is writable per fn docs.
        unsafe { *done = 0 };
    }
    // SAFETY: `out_ptr`/`out_len` are writable per fn docs.
    unsafe { ok_bytes(out_ptr, out_len, bytes) }
}

/// Free a streaming iterator from [`loom_sql_query`]. Passing null is a no-op.
///
/// # Safety
/// `it` must be a pointer returned by [`loom_sql_query`] and not previously freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_iter_free(it: *mut LoomIter) {
    if !it.is_null() {
        // SAFETY: `it` came from `Box::into_raw` in `loom_sql_query` (see fn docs).
        drop(unsafe { Box::from_raw(it) });
    }
}

// ---------------------------------------------------------------------------------------------------
// Async task primitive (poll/handle form) - the portable cooperative backend.
// ---------------------------------------------------------------------------------------------------

/// A deferred op (the work to run on first poll), produced by an `..._async` constructor.
type TaskWork = Box<dyn FnOnce() -> LoomResult<Vec<u8>> + Send>;

/// The lifecycle state of a [`LoomTask`] under the portable cooperative backend.
enum TaskState {
    /// Not yet polled; holds the work to run. Cancellable.
    Pending(TaskWork),
    /// Completed successfully; the canonical-CBOR result is held until [`loom_task_result`] takes it.
    Ready(Vec<u8>),
    /// Completed with an error, held so [`loom_task_result`] can report it repeatably.
    Errored { error: LoomError },
    /// Cancelled while pending (never ran).
    Cancelled,
    /// The successful result buffer has been transferred to the caller.
    Taken,
}

/// Stable [`loom_task_status`] codes (part of the ABI surface).
pub const LOOM_TASK_PENDING: i32 = 0;
pub const LOOM_TASK_READY: i32 = 1;
pub const LOOM_TASK_ERROR: i32 = 2;
pub const LOOM_TASK_CANCELLED: i32 = 3;
pub const LOOM_TASK_TAKEN: i32 = 4;

/// A pollable asynchronous task: the poll/handle async form. Opaque to C; created by an
/// `..._async` constructor (e.g. [`loom_sql_exec_async`]), driven by [`loom_task_poll`], read by
/// [`loom_task_result`], and freed with [`loom_task_free`]. See the module docs for the cooperative
/// backend's blocking-poll contract.
pub struct LoomTask {
    state: TaskState,
}

/// Box a pending task's work and write the owned handle to `*out_task` (free with [`loom_task_free`]).
/// The shared tail of every `..._async` constructor.
///
/// # Safety
/// `out_task` must be null or a valid writable `*mut *mut LoomTask`.
unsafe fn spawn_task(out_task: *mut *mut LoomTask, work: TaskWork) -> i32 {
    let task = LoomTask {
        state: TaskState::Pending(work),
    };
    if !out_task.is_null() {
        // SAFETY: caller guarantees `out_task` is writable (see fn docs).
        unsafe { *out_task = Box::into_raw(Box::new(task)) };
    }
    0
}

/// Start a SQL exec as a [`LoomTask`] (the async-first op). Returns `0` and writes a pending task to
/// `*out_task` (free with [`loom_task_free`]); the statement(s) run on the first [`loom_task_poll`],
/// which yields the result payloads as canonical CBOR via [`loom_task_result`]. Same semantics as
/// [`loom_sql_exec`], deferred.
///
/// # Safety
/// `session` must be from [`loom_sql_open`] and live until the task is polled; `sql` a valid C string;
/// `out_task` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_sql_exec_async(
    session: *mut LoomSqlSession,
    sql: *const c_char,
    out_task: *mut *mut LoomTask,
) -> i32 {
    clear_error();
    // SAFETY: caller guarantees `session` is from `loom_sql_open` and live (see fn docs).
    let Some(session) = (unsafe { session.as_ref() }) else {
        return fail_arg("loom_sql_exec_async: null session");
    };
    // SAFETY: caller guarantees `sql` is a valid C string (see fn docs).
    let Some(sql) = (unsafe { cstr(sql) }) else {
        return fail_arg("loom_sql_exec_async: null or non-UTF-8 sql");
    };
    // Snapshot the session fields + sql into an owned `'static` closure: the task may outlive this call
    // and reopens the loom per the engine's per-op lock model, so it borrows nothing.
    let owned = LoomSqlSession {
        path: session.path.clone(),
        ns_name: session.ns_name.clone(),
        db: session.db.clone(),
        auth: session.auth.clone(),
    };
    let sql = sql.to_string();
    // SAFETY: `out_task` is writable per fn docs.
    unsafe { spawn_task(out_task, Box::new(move || exec_session(&owned, &sql))) }
}

/// Drive a task toward completion. Under the portable backend a pending task is run to completion now,
/// so **the first poll MAY block** the calling thread (drive it off the event loop / UI thread). Writes
/// `1` to `*out_done` once the task is terminal (ready / error / cancelled), else `0`. Returns `0`
/// unless the call is misused (null task). The op's own success/failure is read via [`loom_task_result`].
///
/// # Safety
/// `task` must be from an `..._async` constructor and live; `out_done` null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_task_poll(task: *mut LoomTask, out_done: *mut i32) -> i32 {
    clear_error();
    // SAFETY: caller guarantees `task` is a live task handle (see fn docs).
    let Some(task) = (unsafe { task.as_mut() }) else {
        return fail_arg("loom_task_poll: null task");
    };
    if matches!(task.state, TaskState::Pending(_)) {
        // Move pending work into a terminal state before running it.
        if let TaskState::Pending(work) = std::mem::replace(&mut task.state, TaskState::Taken) {
            task.state = match work() {
                Ok(bytes) => TaskState::Ready(bytes),
                Err(error) => TaskState::Errored { error },
            };
        }
    }
    let done = !matches!(task.state, TaskState::Pending(_));
    if !out_done.is_null() {
        // SAFETY: caller guarantees `out_done` is writable (see fn docs).
        unsafe { *out_done = i32::from(done) };
    }
    0
}

/// The task's [`LOOM_TASK_PENDING`]/`READY`/`ERROR`/`CANCELLED`/`TAKEN` status, or `-1` for a null task.
///
/// # Safety
/// `task` must be null or a live task handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_task_status(task: *const LoomTask) -> i32 {
    // SAFETY: caller guarantees `task` is null or a live task handle (see fn docs).
    match unsafe { task.as_ref() } {
        None => -1,
        Some(t) => match t.state {
            TaskState::Pending(_) => LOOM_TASK_PENDING,
            TaskState::Ready(_) => LOOM_TASK_READY,
            TaskState::Errored { .. } => LOOM_TASK_ERROR,
            TaskState::Cancelled => LOOM_TASK_CANCELLED,
            TaskState::Taken => LOOM_TASK_TAKEN,
        },
    }
}

/// Take a completed task's result. On success transfers **exactly one** owned `(ptr, len)` buffer
/// (free with [`loom_bytes_free`]) and returns `0`, leaving the task `TAKEN`. If the task errored,
/// re-publishes its stored error to [`loom_last_error`] and returns the stable code (repeatably). A
/// pending, cancelled, or already-taken task is `INVALID_ARGUMENT`.
///
/// # Safety
/// `task` must be from an `..._async` constructor and live; `out_ptr`/`out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_task_result(
    task: *mut LoomTask,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    // SAFETY: caller guarantees `task` is a live task handle (see fn docs).
    let Some(task) = (unsafe { task.as_mut() }) else {
        return fail_arg("loom_task_result: null task");
    };
    match &task.state {
        TaskState::Ready(_) => {
            // Transfer the buffer exactly once; the task becomes `TAKEN`.
            if let TaskState::Ready(bytes) = std::mem::replace(&mut task.state, TaskState::Taken) {
                // SAFETY: `out_ptr`/`out_len` are writable per fn docs.
                unsafe { ok_bytes(out_ptr, out_len, bytes) }
            } else {
                unreachable!("state checked as Ready")
            }
        }
        TaskState::Errored { error } => {
            // Report the stored error without consuming it (status stays ERROR; repeatable).
            fail(error.clone())
        }
        TaskState::Pending(_) => fail_arg("loom_task_result: task not polled to completion"),
        TaskState::Cancelled => fail_arg("loom_task_result: task was cancelled"),
        TaskState::Taken => fail_arg("loom_task_result: result already taken"),
    }
}

/// Cancel a task. Guaranteed only while the task is still **pending** (it then becomes `CANCELLED` and
/// never runs); a task already polled to completion is unaffected. Passing null is a no-op.
///
/// # Safety
/// `task` must be null or a live task handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_task_cancel(task: *mut LoomTask) {
    // SAFETY: caller guarantees `task` is null or a live task handle (see fn docs).
    if let Some(task) = unsafe { task.as_mut() }
        && matches!(task.state, TaskState::Pending(_))
    {
        task.state = TaskState::Cancelled;
    }
}

/// Free a task handle (and any un-taken result buffer it still owns). Separate from [`loom_bytes_free`]:
/// a result buffer transferred by [`loom_task_result`] is owned by the caller and outlives the task.
/// Passing null is a no-op.
///
/// # Safety
/// `task` must be a pointer from an `..._async` constructor and not previously freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_task_free(task: *mut LoomTask) {
    if !task.is_null() {
        // SAFETY: `task` came from `Box::into_raw` in an `..._async` constructor (see fn docs).
        drop(unsafe { Box::from_raw(task) });
    }
}

/// Drive `task` to completion and take its result in one call - the synchronous convenience over the
/// poll/handle form. Polls until terminal (the **wait MAY block**; see the module docs), then behaves
/// like [`loom_task_result`]: on success transfers exactly one owned `(ptr, len)` buffer (free with
/// [`loom_bytes_free`]) and returns `0`; on a task error republishes it and returns the stable code.
/// Does **not** free the task (call [`loom_task_free`] after).
///
/// # Safety
/// `task` must be from an `..._async` constructor and live; `out_ptr`/`out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_task_wait(
    task: *mut LoomTask,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    loop {
        let mut done = 0i32;
        // SAFETY: `task` is a live task handle per fn docs; `done` is a local.
        let status = unsafe { loom_task_poll(task, &mut done) };
        if status != 0 {
            return status; // poll only fails on misuse (null task)
        }
        if done != 0 {
            break;
        }
    }
    // SAFETY: `task`/`out_ptr`/`out_len` are valid per fn docs.
    unsafe { loom_task_result(task, out_ptr, out_len) }
}

// ---------------------------------------------------------------------------------------------------
// ---------------------------------------------------------------------------------------------------
// LoomSession - the direct, non-SQL engine surface (B2).
// ---------------------------------------------------------------------------------------------------
mod direct;
pub use direct::*;
// The engine-helper layer lives in `direct.rs` (the non-SQL engine surface); the per-facet C-ABI
// modules and the async forms below reach these `pub(crate)` helpers through this crate-root re-import.
use direct::{
    append_file_ns, blame_table_ns, branch_ns, checkout_ns, cherry_pick_ns, commit_ns,
    commit_staged_ns, diff_table_ns, file_close_ns, file_flush_ns, file_open_ns, file_read_at_ns,
    file_read_ns, file_stat_ns, file_truncate_ns, file_write_at_ns, file_write_ns, index_scan_ns,
    json_string, log_ns, merge_abort_ns, merge_conflicts_ns, merge_continue_ns,
    merge_in_progress_ns, merge_ns, merge_resolve_ns, open_h_read, open_h_write, passphrase_arg,
    random_workspace_id, read_at_ns, read_file_ns, read_link_ns, read_table_ns, rebase_ns,
    remove_file_ns, resolve_workspace_arg, restore_file_ns, restore_path_ns, revert_ns, squash_ns,
    stage_all_ns, stage_ns, status_json_ns, symlink_ns, tag_create_ns, tag_delete_ns, tag_list_ns,
    tag_rename_ns, tag_target_ns, task_handle, truncate_ns, unstage_ns, vcs_blame_ns, vcs_diff_ns,
    watch_poll_ns, write_at_ns, write_file_ns,
};
// ---------------------------------------------------------------------------------------------------
// Per-facet C ABI surface, split into modules. Each module does `use super::*` to pull the
// crate-root types and private helpers; the shared arg/handle macros arrive via `#[macro_use] mod macros`.
// ---------------------------------------------------------------------------------------------------
mod archive;
mod calendar;
mod cas;
mod chat;
mod columnar;
mod contacts;
mod dataframe;
mod document;
mod drive;
mod files;
mod fsdir;
mod graph;
mod kv;
mod lanes;
mod ledger;
mod logs;
mod mail;
mod meetings;
mod metrics;
mod pages;
mod program;
mod queue;
mod replay;
mod restore;
mod search;
mod tags;
mod tickets;
mod timeseries;
mod traces;
mod triggers;
mod vector;

// ---------------------------------------------------------------------------------------------------
// Async task forms of the direct readers. Each defers the same `*_ns` work into a `LoomTask`;
// drive with `loom_task_poll`/`loom_task_wait`, read with `loom_task_result`. The handle
// is reopenable, so each closure owns its arguments and a snapshot of the session.
// ---------------------------------------------------------------------------------------------------

/// Async form of `loom_log`; writes a pending [`LoomTask`] to `*out_task`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; all string arguments valid C strings; `out_task` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_log_async(
    handle: *mut LoomSession,
    ns_name: *const c_char,
    branch: *const c_char,
    out_task: *mut *mut LoomTask,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_log_async");
    let n = arg_str!(ns_name, "loom_log_async").to_string();
    let b = arg_str!(branch, "loom_log_async").to_string();
    let task_handle = task_handle(h);
    // SAFETY: `out_task` is writable per fn docs.
    unsafe { spawn_task(out_task, Box::new(move || log_ns(&task_handle, &n, &b))) }
}

/// Async form of `loom_merge`; writes a pending [`LoomTask`] to `*out_task`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; all string arguments valid C strings; `out_task` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_merge_async(
    handle: *mut LoomSession,
    ns_name: *const c_char,
    from_branch: *const c_char,
    author: *const c_char,
    cell_level: i32,
    out_task: *mut *mut LoomTask,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_merge_async");
    let n = arg_str!(ns_name, "loom_merge_async").to_string();
    let f = arg_str!(from_branch, "loom_merge_async").to_string();
    let a = arg_str!(author, "loom_merge_async").to_string();
    let task_handle = task_handle(h);
    let cells = cell_level != 0;
    // SAFETY: `out_task` is writable per fn docs.
    unsafe {
        spawn_task(
            out_task,
            Box::new(move || merge_ns(&task_handle, &n, &f, &a, cells)),
        )
    }
}

/// Async form of `loom_sql_read_table`; writes a pending [`LoomTask`] to `*out_task`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; all string arguments valid C strings; `out_task` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_sql_read_table_async(
    handle: *mut LoomSession,
    ns_name: *const c_char,
    table: *const c_char,
    out_task: *mut *mut LoomTask,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_sql_read_table_async");
    let n = arg_str!(ns_name, "loom_sql_read_table_async").to_string();
    let tbl = arg_str!(table, "loom_sql_read_table_async").to_string();
    let task_handle = task_handle(h);
    // SAFETY: `out_task` is writable per fn docs.
    unsafe {
        spawn_task(
            out_task,
            Box::new(move || read_table_ns(&task_handle, &n, &tbl)),
        )
    }
}

/// Async form of `loom_sql_index_scan`; writes a pending [`LoomTask`] to `*out_task`. The lookup
/// prefix is canonical CBOR (a cell array), copied into the task before it is spawned.
///
/// # Safety
/// `handle` must be from [`loom_open`]; the name/table/index arguments valid C strings; `(prefix_ptr,
/// prefix_len)` a readable buffer; `out_task` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_sql_index_scan_async(
    handle: *mut LoomSession,
    ns_name: *const c_char,
    table: *const c_char,
    index: *const c_char,
    prefix_ptr: *const c_uchar,
    prefix_len: usize,
    out_task: *mut *mut LoomTask,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_sql_index_scan_async");
    let n = arg_str!(ns_name, "loom_sql_index_scan_async").to_string();
    let tbl = arg_str!(table, "loom_sql_index_scan_async").to_string();
    let idx = arg_str!(index, "loom_sql_index_scan_async").to_string();
    // SAFETY: caller guarantees `(prefix_ptr, prefix_len)` is a readable buffer (see fn docs).
    let prefix = unsafe { byte_slice(prefix_ptr, prefix_len) }.to_vec();
    let task_handle = task_handle(h);
    // SAFETY: `out_task` is writable per fn docs.
    unsafe {
        spawn_task(
            out_task,
            Box::new(move || index_scan_ns(&task_handle, &n, &tbl, &idx, &prefix)),
        )
    }
}

/// Async form of `loom_sql_blame`; writes a pending [`LoomTask`] to `*out_task`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; all string arguments valid C strings; `out_task` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_sql_blame_async(
    handle: *mut LoomSession,
    ns_name: *const c_char,
    branch: *const c_char,
    table: *const c_char,
    out_task: *mut *mut LoomTask,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_sql_blame_async");
    let n = arg_str!(ns_name, "loom_sql_blame_async").to_string();
    let b = arg_str!(branch, "loom_sql_blame_async").to_string();
    let tbl = arg_str!(table, "loom_sql_blame_async").to_string();
    let task_handle = task_handle(h);
    // SAFETY: `out_task` is writable per fn docs.
    unsafe {
        spawn_task(
            out_task,
            Box::new(move || blame_table_ns(&task_handle, &n, &b, &tbl)),
        )
    }
}

/// Async form of `loom_sql_diff`; writes a pending [`LoomTask`] to `*out_task`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; all string arguments valid C strings; `out_task` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_sql_diff_async(
    handle: *mut LoomSession,
    ns_name: *const c_char,
    table: *const c_char,
    from_hex: *const c_char,
    to_hex: *const c_char,
    out_task: *mut *mut LoomTask,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_sql_diff_async");
    let n = arg_str!(ns_name, "loom_sql_diff_async").to_string();
    let tbl = arg_str!(table, "loom_sql_diff_async").to_string();
    let f = arg_str!(from_hex, "loom_sql_diff_async").to_string();
    let to = arg_str!(to_hex, "loom_sql_diff_async").to_string();
    let task_handle = task_handle(h);
    // SAFETY: `out_task` is writable per fn docs.
    unsafe {
        spawn_task(
            out_task,
            Box::new(move || diff_table_ns(&task_handle, &n, &tbl, &f, &to)),
        )
    }
}

/// Async form of `loom_watch_poll`; writes a pending [`LoomTask`] to `*out_task`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `cursor` a valid C string; `out_task` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_watch_poll_async(
    handle: *mut LoomSession,
    cursor: *const c_char,
    max: u32,
    out_task: *mut *mut LoomTask,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_watch_poll_async");
    let c = arg_str!(cursor, "loom_watch_poll_async").to_string();
    let task_handle = task_handle(h);
    // SAFETY: `out_task` is writable per fn docs.
    unsafe {
        spawn_task(
            out_task,
            Box::new(move || watch_poll_ns(&task_handle, &c, max)),
        )
    }
}

// ---------------------------------------------------------------------------------------------------
// ---- result-view decode surface lives in result_render.rs ----
mod result_render;
pub use result_render::*;

#[cfg(all(test, feature = "integration-tests"))]
mod tests;

#[cfg(test)]
mod locator_tests {
    use super::normalize_locator;

    #[test]
    fn local_paths_pass_through() {
        assert_eq!(normalize_locator("./app.loom").unwrap(), "./app.loom");
        assert_eq!(normalize_locator("/abs/app.loom").unwrap(), "/abs/app.loom");
        // A bare non-path string stays local (the C ABI reads no alias TOML).
        assert_eq!(normalize_locator("prod").unwrap(), "prod");
    }

    #[test]
    fn file_url_is_stripped_to_local_path() {
        assert_eq!(
            normalize_locator("file:///abs/app.loom").unwrap(),
            "/abs/app.loom"
        );
    }

    #[test]
    fn remote_url_is_rejected_without_remote_feature() {
        let err = normalize_locator("https://loom.example.com/prod").unwrap_err();
        assert!(
            err.to_string().contains("remote feature"),
            "unexpected error: {err}"
        );
        assert!(normalize_locator("http://loom.example.com/prod").is_err());
    }
}
