//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

// ---------------------------------------------------------------------------------------------------
// SQL session - the single path that exposes the whole tabular + SQL stack to any language.
// ---------------------------------------------------------------------------------------------------

/// An open SQL session: a reopenable handle to `(loom path, workspace name, database name)`, with the
/// workspace's SQL facet ensured. It does **not** hold the `.loom` (or its single-writer lock) between
/// calls - each [`loom_sql_exec`] / [`loom_sql_commit`] opens the loom for the duration of that op and
/// releases it (matching the engine's single-writer / lock-free-reader model and the `loom` CLI). So a
/// session is cheap, safe to keep around, and never blocks another session except during an actual write.
/// Opaque to C; create with [`loom_sql_open`] and free with [`loom_sql_close`].
pub struct LoomSqlSession {
    pub(crate) path: String,
    pub(crate) ns_name: String,
    pub(crate) db: String,
    pub(crate) auth: LocalOpenAuth,
}

/// Read a raw 256-bit KEK argument: `null`/`len == 0` -> `None`; otherwise exactly 32
/// bytes -> `Some(KeySpec::raw_kek(..))`. A wrong length records `INVALID_ARGUMENT` and returns its
/// status code for the caller to return.
///
/// # Safety
/// `ptr` must be null or point to `len` readable bytes.
pub(crate) unsafe fn kek_arg(
    ptr: *const c_uchar,
    len: usize,
    who: &str,
) -> core::result::Result<Option<KeySpec>, i32> {
    if ptr.is_null() || len == 0 {
        return Ok(None);
    }
    if len != 32 {
        return Err(fail_arg(&format!(
            "{who}: a raw KEK must be exactly 32 bytes (256 bits), got {len}"
        )));
    }
    // SAFETY: caller guarantees `ptr` points to `len` readable bytes (see fn docs).
    let bytes = unsafe { core::slice::from_raw_parts(ptr, len) };
    let mut kek = [0u8; 32];
    kek.copy_from_slice(bytes);
    Ok(Some(KeySpec::raw_kek(kek)))
}

/// Open the loom for write, resolve-or-create the session workspace with its SQL facet, and return both.
/// The caller drops the returned `Loom` to release the exclusive write lock.
fn open_for_write(session: &LoomSqlSession) -> LoomResult<(Loom<FileStore>, WorkspaceId)> {
    let loom = open_loom_unlocked(&session.path, session.auth.unlock_key.as_ref())?;
    let mut loom = attach_local_auth(loom, &session.auth)?;
    let id = derive_sql_ns_id(&session.ns_name);
    let ns = loom.registry_mut().ensure_for_write(
        &WsSelector::Typed {
            ty: FacetKind::Sql,
            name: session.ns_name.clone(),
        },
        id,
    )?;
    Ok((loom, ns))
}

fn open_session(
    path: &str,
    name: &str,
    db: &str,
    auth: LocalOpenAuth,
) -> LoomResult<LoomSqlSession> {
    let session = LoomSqlSession {
        path: path.to_string(),
        ns_name: name.to_string(),
        db: db.to_string(),
        auth,
    };
    // Fail-fast and create the workspace eagerly, then release the lock immediately.
    let (mut loom, _ns) = open_for_write(&session)?;
    save_loom(&mut loom)?;
    Ok(session)
}

/// Open the SQL store for database `db` over an **owned, lock-free read snapshot** of the loom at
/// `path` (the lazy base). The base owns its read view (via `FileStore::open_read`,
/// which takes no lock and coexists with a writer), distinct from the exclusive write loom that
/// [`LoomSqlStore::persist`] flushes into. Reads stream durable rows on demand; `open` yields an empty
/// (but snapshot-backed) store when no catalog is staged yet. Shared by the per-op session path and the
/// `LoomSqlBatch` scope.
pub(crate) fn load_store_read(
    path: &str,
    ns: WorkspaceId,
    db: &str,
    auth: &LocalOpenAuth,
) -> LoomResult<LoomSqlStore> {
    let read = open_loom_read_unlocked(path, auth.unlock_key.as_ref())?;
    let read = attach_local_auth(read, auth)?;
    LoomSqlStore::open_read(read, ns, db)
}

fn load_store_write(
    path: &str,
    ns: WorkspaceId,
    db: &str,
    auth: &LocalOpenAuth,
) -> LoomResult<LoomSqlStore> {
    let read = open_loom_read_unlocked(path, auth.unlock_key.as_ref())?;
    let read = attach_local_auth(read, auth)?;
    LoomSqlStore::open_write(read, ns, db)
}

pub(crate) fn exec_session(session: &LoomSqlSession, sql: &str) -> LoomResult<Vec<u8>> {
    let ns = derive_sql_ns_id(&session.ns_name);
    let mut store = load_store_write(&session.path, ns, &session.db, &session.auth)?;
    let payload = store.exec_cbor(sql)?;
    // A per-op exec is one atomic save: a transaction must open and resolve within this single call
    // (e.g. "BEGIN; ...; COMMIT"). A transaction left open would have its mid-flight state persisted and
    // then lost, so reject it - cross-statement transactions need a `LoomSqlBatch`.
    if store.in_transaction() {
        return Err(LoomError::invalid(
            "BEGIN without a matching COMMIT/ROLLBACK in one exec: open a LoomSqlBatch to run a transaction across statements",
        ));
    }
    // Take the exclusive write lock only when the statement actually changed something, then flush the
    // overlay's deltas and release. A pure `SELECT` never blocks a writer or another reader.
    if store.is_dirty() {
        let (mut loom, ns) = open_for_write(session)?;
        store.persist(&mut loom, ns, &session.db)?;
        save_loom(&mut loom)?;
    }
    Ok(payload)
}

fn parse_sql_auth(
    principal: *const c_char,
    passphrase: *const c_uchar,
    passphrase_len: usize,
    who: &str,
) -> core::result::Result<(WorkspaceId, String), i32> {
    // SAFETY: the C ABI caller supplies a valid C string for `principal`.
    let Some(principal) = (unsafe { cstr(principal) }) else {
        return Err(fail_arg(&format!("{who}: null or non-UTF-8 principal")));
    };
    let principal = WorkspaceId::parse(principal).map_err(fail)?;
    // SAFETY: the C ABI caller supplies a readable passphrase buffer.
    let passphrase = match unsafe { passphrase_arg(passphrase, passphrase_len, who) } {
        Ok(Some(value)) => value,
        Ok(None) => return Err(fail_arg(&format!("{who}: passphrase is required"))),
        Err(code) => return Err(code),
    };
    Ok((principal, passphrase))
}

fn authenticated_sql_session(
    path: &str,
    name: &str,
    db: &str,
    unlock_key: Option<KeySpec>,
    principal: WorkspaceId,
    passphrase: String,
) -> LoomResult<LoomSqlSession> {
    let auth = with_sql_auth(
        LocalOpenAuth {
            unlock_key,
            ..LocalOpenAuth::default()
        },
        principal,
        passphrase,
    )?;
    open_session(path, name, db, auth)
}

fn with_sql_auth(
    mut auth: LocalOpenAuth,
    principal: WorkspaceId,
    passphrase: String,
) -> LoomResult<LocalOpenAuth> {
    auth.principal = Some(principal);
    auth.passphrase = Some(passphrase);
    auth.session_id = Some(random_workspace_id()?.to_string());
    Ok(auth)
}

fn commit_session(session: &LoomSqlSession, message: &str, author: &str) -> LoomResult<String> {
    let (mut loom, ns) = open_for_write(session)?;
    let digest = loom.commit(ns, author, message, now_ms())?;
    save_loom(&mut loom)?;
    Ok(digest.to_string())
}

/// Open `loom_path` and start a SQL session over workspace `ns_name` (created if absent, with its SQL
/// facet ensured), database `db`. On success writes an owned session pointer to `*out` (free with
/// [`loom_sql_close`]) and returns `0`; on failure returns a non-zero [`Code`] (see [`loom_last_error`]).
///
/// # Safety
/// `loom_path`, `ns_name`, `db` must be valid C strings; `out` a valid `*mut *mut LoomSqlSession`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_sql_open(
    loom_path: *const c_char,
    ns_name: *const c_char,
    db: *const c_char,
    out: *mut *mut LoomSqlSession,
) -> i32 {
    clear_error();
    // SAFETY: caller guarantees each argument is a valid C string (see fn docs).
    let (Some(path), Some(name), Some(db)) = (
        unsafe { cstr(loom_path) },
        unsafe { cstr(ns_name) },
        unsafe { cstr(db) },
    ) else {
        return fail_arg("loom_sql_open: null or non-UTF-8 argument");
    };
    match open_session(path, name, db, LocalOpenAuth::default()) {
        Ok(s) => {
            if !out.is_null() {
                // SAFETY: caller guarantees `out` is writable (see fn docs).
                unsafe { *out = Box::into_raw(Box::new(s)) };
            }
            0
        }
        Err(e) => fail(e),
    }
}

/// Like [`loom_sql_open`], but unlocks an **encrypted** loom with the passphrase bytes
/// at `(passphrase, passphrase_len)`, held for the session's lifetime. A null/empty passphrase behaves
/// exactly like [`loom_sql_open`] (and fails `E2eLocked` on an encrypted loom). The host acquires the
/// passphrase securely; the FFI never reads an environment variable.
///
/// # Safety
/// `loom_path`, `ns_name`, `db` must be valid C strings; `passphrase` null or `passphrase_len` readable
/// bytes; `out` a valid `*mut *mut LoomSqlSession`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_sql_open_keyed(
    loom_path: *const c_char,
    ns_name: *const c_char,
    db: *const c_char,
    passphrase: *const c_uchar,
    passphrase_len: usize,
    out: *mut *mut LoomSqlSession,
) -> i32 {
    clear_error();
    // SAFETY: caller guarantees each argument is a valid C string (see fn docs).
    let (Some(path), Some(name), Some(db)) = (
        unsafe { cstr(loom_path) },
        unsafe { cstr(ns_name) },
        unsafe { cstr(db) },
    ) else {
        return fail_arg("loom_sql_open_keyed: null or non-UTF-8 argument");
    };
    // SAFETY: caller guarantees the passphrase buffer (see fn docs).
    let key = match unsafe { passphrase_arg(passphrase, passphrase_len, "loom_sql_open_keyed") } {
        Ok(k) => k,
        Err(code) => return code,
    };
    let auth = LocalOpenAuth {
        unlock_key: key.map(KeySpec::passphrase),
        ..LocalOpenAuth::default()
    };
    match open_session(path, name, db, auth) {
        Ok(s) => {
            if !out.is_null() {
                // SAFETY: caller guarantees `out` is writable (see fn docs).
                unsafe { *out = Box::into_raw(Box::new(s)) };
            }
            0
        }
        Err(e) => fail(e),
    }
}

/// Like [`loom_sql_open`], but unlocks an encrypted loom with a caller-supplied 256-bit **KEK**
/// at `(kek, kek_len)` (= 32 bytes), held for the session's lifetime. A null/empty KEK behaves
/// like [`loom_sql_open`]. The host computed the KEK from its provider; a wrong KEK fails
/// `E2E_KEY_INVALID`.
///
/// # Safety
/// `loom_path`, `ns_name`, `db` valid C strings; `kek` null or `kek_len` readable bytes; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_sql_open_with_kek(
    loom_path: *const c_char,
    ns_name: *const c_char,
    db: *const c_char,
    kek: *const c_uchar,
    kek_len: usize,
    out: *mut *mut LoomSqlSession,
) -> i32 {
    clear_error();
    // SAFETY: caller guarantees each argument is a valid C string (see fn docs).
    let (Some(path), Some(name), Some(db)) = (
        unsafe { cstr(loom_path) },
        unsafe { cstr(ns_name) },
        unsafe { cstr(db) },
    ) else {
        return fail_arg("loom_sql_open_with_kek: null or non-UTF-8 argument");
    };
    // SAFETY: caller guarantees the KEK buffer (see fn docs).
    let key = match unsafe { kek_arg(kek, kek_len, "loom_sql_open_with_kek") } {
        Ok(k) => k,
        Err(code) => return code,
    };
    let auth = LocalOpenAuth {
        unlock_key: key,
        ..LocalOpenAuth::default()
    };
    match open_session(path, name, db, auth) {
        Ok(s) => {
            if !out.is_null() {
                // SAFETY: caller guarantees `out` is writable (see fn docs).
                unsafe { *out = Box::into_raw(Box::new(s)) };
            }
            0
        }
        Err(e) => fail(e),
    }
}

/// Like [`loom_sql_open`], but binds a principal passphrase to the SQL handle.
///
/// # Safety
/// `loom_path`, `ns_name`, `db`, `auth_principal` must be valid C strings; `auth_passphrase` must be
/// null or point to `auth_passphrase_len` readable bytes; `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_sql_open_authenticated(
    loom_path: *const c_char,
    ns_name: *const c_char,
    db: *const c_char,
    auth_principal: *const c_char,
    auth_passphrase: *const c_uchar,
    auth_passphrase_len: usize,
    out: *mut *mut LoomSqlSession,
) -> i32 {
    clear_error();
    // SAFETY: caller guarantees each argument is a valid C string (see fn docs).
    let (Some(path), Some(name), Some(db)) = (
        unsafe { cstr(loom_path) },
        unsafe { cstr(ns_name) },
        unsafe { cstr(db) },
    ) else {
        return fail_arg("loom_sql_open_authenticated: null or non-UTF-8 argument");
    };
    let (principal, passphrase) = match parse_sql_auth(
        auth_principal,
        auth_passphrase,
        auth_passphrase_len,
        "loom_sql_open_authenticated",
    ) {
        Ok(value) => value,
        Err(code) => return code,
    };
    match authenticated_sql_session(path, name, db, None, principal, passphrase) {
        Ok(s) => {
            if !out.is_null() {
                // SAFETY: caller guarantees `out` is writable (see fn docs).
                unsafe { *out = Box::into_raw(Box::new(s)) };
            }
            0
        }
        Err(e) => fail(e),
    }
}

/// Like [`loom_sql_open_keyed`], but binds a principal passphrase to the SQL handle.
///
/// # Safety
/// `loom_path`, `ns_name`, `db`, `auth_principal` must be valid C strings; byte buffers must be null or
/// readable for their lengths; `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_sql_open_keyed_authenticated(
    loom_path: *const c_char,
    ns_name: *const c_char,
    db: *const c_char,
    passphrase: *const c_uchar,
    passphrase_len: usize,
    auth_principal: *const c_char,
    auth_passphrase: *const c_uchar,
    auth_passphrase_len: usize,
    out: *mut *mut LoomSqlSession,
) -> i32 {
    clear_error();
    // SAFETY: caller guarantees each argument is a valid C string (see fn docs).
    let (Some(path), Some(name), Some(db)) = (
        unsafe { cstr(loom_path) },
        unsafe { cstr(ns_name) },
        unsafe { cstr(db) },
    ) else {
        return fail_arg("loom_sql_open_keyed_authenticated: null or non-UTF-8 argument");
    };
    // SAFETY: caller guarantees the passphrase buffer (see fn docs).
    let key = match unsafe {
        passphrase_arg(
            passphrase,
            passphrase_len,
            "loom_sql_open_keyed_authenticated",
        )
    } {
        Ok(value) => value,
        Err(code) => return code,
    };
    let (principal, auth_passphrase) = match parse_sql_auth(
        auth_principal,
        auth_passphrase,
        auth_passphrase_len,
        "loom_sql_open_keyed_authenticated",
    ) {
        Ok(value) => value,
        Err(code) => return code,
    };
    match authenticated_sql_session(
        path,
        name,
        db,
        key.map(KeySpec::passphrase),
        principal,
        auth_passphrase,
    ) {
        Ok(s) => {
            if !out.is_null() {
                // SAFETY: caller guarantees `out` is writable (see fn docs).
                unsafe { *out = Box::into_raw(Box::new(s)) };
            }
            0
        }
        Err(e) => fail(e),
    }
}

/// Like [`loom_sql_open_with_kek`], but binds a principal passphrase to the SQL handle.
///
/// # Safety
/// `loom_path`, `ns_name`, `db`, `auth_principal` must be valid C strings; byte buffers must be null or
/// readable for their lengths; `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_sql_open_with_kek_authenticated(
    loom_path: *const c_char,
    ns_name: *const c_char,
    db: *const c_char,
    kek: *const c_uchar,
    kek_len: usize,
    auth_principal: *const c_char,
    auth_passphrase: *const c_uchar,
    auth_passphrase_len: usize,
    out: *mut *mut LoomSqlSession,
) -> i32 {
    clear_error();
    // SAFETY: caller guarantees each argument is a valid C string (see fn docs).
    let (Some(path), Some(name), Some(db)) = (
        unsafe { cstr(loom_path) },
        unsafe { cstr(ns_name) },
        unsafe { cstr(db) },
    ) else {
        return fail_arg("loom_sql_open_with_kek_authenticated: null or non-UTF-8 argument");
    };
    // SAFETY: caller guarantees the KEK buffer (see fn docs).
    let key = match unsafe { kek_arg(kek, kek_len, "loom_sql_open_with_kek_authenticated") } {
        Ok(value) => value,
        Err(code) => return code,
    };
    let (principal, passphrase) = match parse_sql_auth(
        auth_principal,
        auth_passphrase,
        auth_passphrase_len,
        "loom_sql_open_with_kek_authenticated",
    ) {
        Ok(value) => value,
        Err(code) => return code,
    };
    match authenticated_sql_session(path, name, db, key, principal, passphrase) {
        Ok(s) => {
            if !out.is_null() {
                // SAFETY: caller guarantees `out` is writable (see fn docs).
                unsafe { *out = Box::into_raw(Box::new(s)) };
            }
            0
        }
        Err(e) => fail(e),
    }
}

/// Bind a principal passphrase to an existing SQL session.
///
/// # Safety
/// `session` must be from [`loom_sql_open`]; `principal` must be a valid C string; `passphrase` must
/// be null or point to `passphrase_len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_sql_authenticate_passphrase(
    session: *mut LoomSqlSession,
    principal: *const c_char,
    passphrase: *const c_uchar,
    passphrase_len: usize,
) -> i32 {
    clear_error();
    // SAFETY: caller guarantees `session` is from `loom_sql_open` and live (see fn docs).
    let Some(session) = (unsafe { session.as_mut() }) else {
        return fail_arg("loom_sql_authenticate_passphrase: null session");
    };
    let (principal, passphrase) = match parse_sql_auth(
        principal,
        passphrase,
        passphrase_len,
        "loom_sql_authenticate_passphrase",
    ) {
        Ok(value) => value,
        Err(code) => return code,
    };
    match with_sql_auth(session.auth.clone(), principal, passphrase).and_then(|auth| {
        let read = open_loom_read_unlocked(&session.path, auth.unlock_key.as_ref())?;
        attach_local_auth(read, &auth)?;
        session.auth = auth;
        Ok(())
    }) {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// Run one or more `;`-separated SQL statements; on success writes the result payloads as canonical
/// CBOR to `(*out_ptr, *out_len)` (free with [`loom_bytes_free`]) and returns `0`. Mutations
/// are staged and persisted to the working tree; call [`loom_sql_commit`] to record a commit. Use
/// [`loom_result_to_json`] to inspect the buffer as text.
///
/// # Safety
/// `session` must be from [`loom_sql_open`] and live; `sql` a valid C string; `out_ptr`/`out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_sql_exec(
    session: *mut LoomSqlSession,
    sql: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    // SAFETY: caller guarantees `session` is from `loom_sql_open` and live (see fn docs).
    let Some(session) = (unsafe { session.as_ref() }) else {
        return fail_arg("loom_sql_exec: null session");
    };
    // SAFETY: caller guarantees `sql` is a valid C string (see fn docs).
    let Some(sql) = (unsafe { cstr(sql) }) else {
        return fail_arg("loom_sql_exec: null or non-UTF-8 sql");
    };
    match exec_session(session, sql) {
        // SAFETY: `out_ptr`/`out_len` are writable per fn docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Commit the session's staged database state; on success writes the new commit's content address to
/// `*out` (free with [`loom_string_free`]) and returns `0`.
///
/// # Safety
/// `session` must be from [`loom_sql_open`] and live; `message`/`author` valid C strings; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_sql_commit(
    session: *mut LoomSqlSession,
    message: *const c_char,
    author: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    // SAFETY: caller guarantees `session` is from `loom_sql_open` and live (see fn docs).
    let Some(session) = (unsafe { session.as_ref() }) else {
        return fail_arg("loom_sql_commit: null session");
    };
    // SAFETY: caller guarantees both arguments are valid C strings (see fn docs).
    let (Some(message), Some(author)) = (unsafe { cstr(message) }, unsafe { cstr(author) }) else {
        return fail_arg("loom_sql_commit: null or non-UTF-8 argument");
    };
    match commit_session(session, message, author) {
        // SAFETY: `out` is writable per fn docs.
        Ok(hex) => unsafe { ok_str(out, &hex) },
        Err(e) => fail(e),
    }
}

/// Free a session from [`loom_sql_open`]. Passing null is a no-op.
///
/// # Safety
/// `session` must be a pointer returned by [`loom_sql_open`] and not previously freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_sql_close(session: *mut LoomSqlSession) {
    if !session.is_null() {
        // SAFETY: `session` came from `Box::into_raw` in `loom_sql_open` (see fn docs).
        drop(unsafe { Box::from_raw(session) });
    }
}

// ---------------------------------------------------------------------------------------------------
// Transaction/batch scope - the opt-in held-open writer.
// ---------------------------------------------------------------------------------------------------

/// An explicit transaction/batch scope. Unlike a session, a batch holds the `.loom` open - and its
/// exclusive single-writer lock - for its whole lifetime, and loads the SQL store **once**. Statements
/// run against that one in-memory store, so an explicit SQL transaction (`BEGIN` / `COMMIT` /
/// `ROLLBACK`) can span calls, and changes become durable through a **single atomic save** at
/// [`loom_sql_batch_commit`] (or [`loom_sql_batch_commit_vcs`]) - the engine's true atomic persistence
/// boundary, so a crash mid-batch leaves the pre-batch state intact. Because it holds the write lock,
/// only one batch (or single writing op) per `.loom` runs at a time; keep batches short. The SQL
/// `COMMIT` (ending a transaction) is distinct from the VCS commit (recording a history entry). Opaque
/// to C; create with [`loom_sql_batch_begin`] and free with [`loom_sql_batch_close`] (closing without a
/// commit discards every un-persisted change).
pub struct LoomSqlBatch {
    loom: Loom<FileStore>,
    ns: WorkspaceId,
    db: String,
    path: String,
    store: LoomSqlStore,
    auth: LocalOpenAuth,
}

fn begin_batch(path: &str, name: &str, db: &str, auth: LocalOpenAuth) -> LoomResult<LoomSqlBatch> {
    let session = LoomSqlSession {
        path: path.to_string(),
        ns_name: name.to_string(),
        db: db.to_string(),
        auth,
    };
    // Hold the exclusive write loom for the batch's lifetime (persist flushes into it), and snapshot a
    // separate lock-free read view for the lazy base.
    let (loom, ns) = open_for_write(&session)?;
    let store = load_store_write(path, ns, db, &session.auth)?;
    let auth = session.auth;
    Ok(LoomSqlBatch {
        loom,
        ns,
        db: db.to_string(),
        path: path.to_string(),
        store,
        auth,
    })
}

fn authenticated_sql_batch(
    path: &str,
    name: &str,
    db: &str,
    unlock_key: Option<KeySpec>,
    principal: WorkspaceId,
    passphrase: String,
) -> LoomResult<LoomSqlBatch> {
    let auth = with_sql_auth(
        LocalOpenAuth {
            unlock_key,
            ..LocalOpenAuth::default()
        },
        principal,
        passphrase,
    )?;
    begin_batch(path, name, db, auth)
}

/// Reject finalizing a batch while an explicit SQL transaction is still open (the in-memory state is
/// mid-flight and must not be persisted); the caller must `COMMIT` or `ROLLBACK` first.
fn ensure_no_open_txn(batch: &LoomSqlBatch) -> LoomResult<()> {
    if batch.store.in_transaction() {
        return Err(LoomError::invalid(
            "the batch has an open SQL transaction; COMMIT or ROLLBACK before committing the batch",
        ));
    }
    Ok(())
}

fn flush_batch(batch: &mut LoomSqlBatch) -> LoomResult<()> {
    ensure_no_open_txn(batch)?;
    batch.store.persist(&mut batch.loom, batch.ns, &batch.db)?;
    save_loom(&mut batch.loom)?;
    Ok(())
}

fn commit_vcs_batch(batch: &mut LoomSqlBatch, message: &str, author: &str) -> LoomResult<String> {
    ensure_no_open_txn(batch)?;
    batch.store.persist(&mut batch.loom, batch.ns, &batch.db)?;
    let digest = batch.loom.commit(batch.ns, author, message, now_ms())?;
    save_loom(&mut batch.loom)?;
    Ok(digest.to_string())
}

/// Begin a transaction/batch scope over workspace `ns_name` (created if absent, with its SQL facet
/// ensured), database `db`, in `loom_path`. Holds the write lock until [`loom_sql_batch_close`]. On
/// success writes an owned batch pointer to `*out` and returns `0`; on failure a non-zero [`Code`] (see
/// [`loom_last_error`]).
///
/// # Safety
/// `loom_path`, `ns_name`, `db` must be valid C strings; `out` a valid `*mut *mut LoomSqlBatch`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_sql_batch_begin(
    loom_path: *const c_char,
    ns_name: *const c_char,
    db: *const c_char,
    out: *mut *mut LoomSqlBatch,
) -> i32 {
    clear_error();
    // SAFETY: caller guarantees each argument is a valid C string (see fn docs).
    let (Some(path), Some(name), Some(db)) = (
        unsafe { cstr(loom_path) },
        unsafe { cstr(ns_name) },
        unsafe { cstr(db) },
    ) else {
        return fail_arg("loom_sql_batch_begin: null or non-UTF-8 argument");
    };
    match begin_batch(path, name, db, LocalOpenAuth::default()) {
        Ok(b) => {
            if !out.is_null() {
                // SAFETY: caller guarantees `out` is writable (see fn docs).
                unsafe { *out = Box::into_raw(Box::new(b)) };
            }
            0
        }
        Err(e) => fail(e),
    }
}

/// Like [`loom_sql_batch_begin`], but unlocks an **encrypted** loom with the passphrase
/// bytes at `(passphrase, passphrase_len)` for the batch's lifetime. A null/empty passphrase behaves
/// like [`loom_sql_batch_begin`]. The host acquires the passphrase securely; no environment variable is
/// consulted.
///
/// # Safety
/// `loom_path`, `ns_name`, `db` must be valid C strings; `passphrase` null or `passphrase_len` readable
/// bytes; `out` a valid `*mut *mut LoomSqlBatch`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_sql_batch_begin_keyed(
    loom_path: *const c_char,
    ns_name: *const c_char,
    db: *const c_char,
    passphrase: *const c_uchar,
    passphrase_len: usize,
    out: *mut *mut LoomSqlBatch,
) -> i32 {
    clear_error();
    // SAFETY: caller guarantees each argument is a valid C string (see fn docs).
    let (Some(path), Some(name), Some(db)) = (
        unsafe { cstr(loom_path) },
        unsafe { cstr(ns_name) },
        unsafe { cstr(db) },
    ) else {
        return fail_arg("loom_sql_batch_begin_keyed: null or non-UTF-8 argument");
    };
    // SAFETY: caller guarantees the passphrase buffer (see fn docs).
    let key =
        match unsafe { passphrase_arg(passphrase, passphrase_len, "loom_sql_batch_begin_keyed") } {
            Ok(k) => k,
            Err(code) => return code,
        };
    let auth = LocalOpenAuth {
        unlock_key: key.map(KeySpec::passphrase),
        ..LocalOpenAuth::default()
    };
    match begin_batch(path, name, db, auth) {
        Ok(b) => {
            if !out.is_null() {
                // SAFETY: caller guarantees `out` is writable (see fn docs).
                unsafe { *out = Box::into_raw(Box::new(b)) };
            }
            0
        }
        Err(e) => fail(e),
    }
}

/// Like [`loom_sql_batch_begin`], but unlocks an encrypted loom with a caller-supplied 256-bit **KEK**
/// at `(kek, kek_len)` (= 32 bytes) for the batch's lifetime. A null/empty KEK behaves
/// like [`loom_sql_batch_begin`].
///
/// # Safety
/// `loom_path`, `ns_name`, `db` valid C strings; `kek` null or `kek_len` readable bytes; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_sql_batch_begin_with_kek(
    loom_path: *const c_char,
    ns_name: *const c_char,
    db: *const c_char,
    kek: *const c_uchar,
    kek_len: usize,
    out: *mut *mut LoomSqlBatch,
) -> i32 {
    clear_error();
    // SAFETY: caller guarantees each argument is a valid C string (see fn docs).
    let (Some(path), Some(name), Some(db)) = (
        unsafe { cstr(loom_path) },
        unsafe { cstr(ns_name) },
        unsafe { cstr(db) },
    ) else {
        return fail_arg("loom_sql_batch_begin_with_kek: null or non-UTF-8 argument");
    };
    // SAFETY: caller guarantees the KEK buffer (see fn docs).
    let key = match unsafe { kek_arg(kek, kek_len, "loom_sql_batch_begin_with_kek") } {
        Ok(k) => k,
        Err(code) => return code,
    };
    let auth = LocalOpenAuth {
        unlock_key: key,
        ..LocalOpenAuth::default()
    };
    match begin_batch(path, name, db, auth) {
        Ok(b) => {
            if !out.is_null() {
                // SAFETY: caller guarantees `out` is writable (see fn docs).
                unsafe { *out = Box::into_raw(Box::new(b)) };
            }
            0
        }
        Err(e) => fail(e),
    }
}

/// Like [`loom_sql_batch_begin`], but binds a principal passphrase before the SQL store is opened.
///
/// # Safety
/// `loom_path`, `ns_name`, `db`, `auth_principal` must be valid C strings; `auth_passphrase` must be
/// null or point to `auth_passphrase_len` readable bytes; `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_sql_batch_begin_authenticated(
    loom_path: *const c_char,
    ns_name: *const c_char,
    db: *const c_char,
    auth_principal: *const c_char,
    auth_passphrase: *const c_uchar,
    auth_passphrase_len: usize,
    out: *mut *mut LoomSqlBatch,
) -> i32 {
    clear_error();
    // SAFETY: caller guarantees each argument is a valid C string (see fn docs).
    let (Some(path), Some(name), Some(db)) = (
        unsafe { cstr(loom_path) },
        unsafe { cstr(ns_name) },
        unsafe { cstr(db) },
    ) else {
        return fail_arg("loom_sql_batch_begin_authenticated: null or non-UTF-8 argument");
    };
    let (principal, passphrase) = match parse_sql_auth(
        auth_principal,
        auth_passphrase,
        auth_passphrase_len,
        "loom_sql_batch_begin_authenticated",
    ) {
        Ok(value) => value,
        Err(code) => return code,
    };
    match authenticated_sql_batch(path, name, db, None, principal, passphrase) {
        Ok(b) => {
            if !out.is_null() {
                // SAFETY: caller guarantees `out` is writable (see fn docs).
                unsafe { *out = Box::into_raw(Box::new(b)) };
            }
            0
        }
        Err(e) => fail(e),
    }
}

/// Like [`loom_sql_batch_begin_keyed`], but binds a principal passphrase before the SQL store is opened.
///
/// # Safety
/// `loom_path`, `ns_name`, `db`, `auth_principal` must be valid C strings; byte buffers must be null or
/// readable for their lengths; `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_sql_batch_begin_keyed_authenticated(
    loom_path: *const c_char,
    ns_name: *const c_char,
    db: *const c_char,
    passphrase: *const c_uchar,
    passphrase_len: usize,
    auth_principal: *const c_char,
    auth_passphrase: *const c_uchar,
    auth_passphrase_len: usize,
    out: *mut *mut LoomSqlBatch,
) -> i32 {
    clear_error();
    // SAFETY: caller guarantees each argument is a valid C string (see fn docs).
    let (Some(path), Some(name), Some(db)) = (
        unsafe { cstr(loom_path) },
        unsafe { cstr(ns_name) },
        unsafe { cstr(db) },
    ) else {
        return fail_arg("loom_sql_batch_begin_keyed_authenticated: null or non-UTF-8 argument");
    };
    // SAFETY: caller guarantees the passphrase buffer (see fn docs).
    let key = match unsafe {
        passphrase_arg(
            passphrase,
            passphrase_len,
            "loom_sql_batch_begin_keyed_authenticated",
        )
    } {
        Ok(value) => value,
        Err(code) => return code,
    };
    let (principal, auth_passphrase) = match parse_sql_auth(
        auth_principal,
        auth_passphrase,
        auth_passphrase_len,
        "loom_sql_batch_begin_keyed_authenticated",
    ) {
        Ok(value) => value,
        Err(code) => return code,
    };
    match authenticated_sql_batch(
        path,
        name,
        db,
        key.map(KeySpec::passphrase),
        principal,
        auth_passphrase,
    ) {
        Ok(b) => {
            if !out.is_null() {
                // SAFETY: caller guarantees `out` is writable (see fn docs).
                unsafe { *out = Box::into_raw(Box::new(b)) };
            }
            0
        }
        Err(e) => fail(e),
    }
}

/// Like [`loom_sql_batch_begin_with_kek`], but binds a principal passphrase before the SQL store is opened.
///
/// # Safety
/// `loom_path`, `ns_name`, `db`, `auth_principal` must be valid C strings; byte buffers must be null or
/// readable for their lengths; `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_sql_batch_begin_with_kek_authenticated(
    loom_path: *const c_char,
    ns_name: *const c_char,
    db: *const c_char,
    kek: *const c_uchar,
    kek_len: usize,
    auth_principal: *const c_char,
    auth_passphrase: *const c_uchar,
    auth_passphrase_len: usize,
    out: *mut *mut LoomSqlBatch,
) -> i32 {
    clear_error();
    // SAFETY: caller guarantees each argument is a valid C string (see fn docs).
    let (Some(path), Some(name), Some(db)) = (
        unsafe { cstr(loom_path) },
        unsafe { cstr(ns_name) },
        unsafe { cstr(db) },
    ) else {
        return fail_arg("loom_sql_batch_begin_with_kek_authenticated: null or non-UTF-8 argument");
    };
    // SAFETY: caller guarantees the KEK buffer (see fn docs).
    let key = match unsafe { kek_arg(kek, kek_len, "loom_sql_batch_begin_with_kek_authenticated") }
    {
        Ok(value) => value,
        Err(code) => return code,
    };
    let (principal, passphrase) = match parse_sql_auth(
        auth_principal,
        auth_passphrase,
        auth_passphrase_len,
        "loom_sql_batch_begin_with_kek_authenticated",
    ) {
        Ok(value) => value,
        Err(code) => return code,
    };
    match authenticated_sql_batch(path, name, db, key, principal, passphrase) {
        Ok(b) => {
            if !out.is_null() {
                // SAFETY: caller guarantees `out` is writable (see fn docs).
                unsafe { *out = Box::into_raw(Box::new(b)) };
            }
            0
        }
        Err(e) => fail(e),
    }
}

/// Run one or more `;`-separated SQL statements in the batch (including `BEGIN`/`COMMIT`/`ROLLBACK`),
/// accumulating changes in the batch's in-memory store. On success writes the result payloads as
/// canonical CBOR to `(*out_ptr, *out_len)` (free with [`loom_bytes_free`]) and returns `0`. Nothing is
/// made durable until [`loom_sql_batch_commit`].
///
/// # Safety
/// `batch` must be from [`loom_sql_batch_begin`] and live; `sql` a valid C string; `out_ptr`/`out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_sql_batch_exec(
    batch: *mut LoomSqlBatch,
    sql: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    // SAFETY: caller guarantees `batch` is from `loom_sql_batch_begin` and live (see fn docs).
    let Some(batch) = (unsafe { batch.as_mut() }) else {
        return fail_arg("loom_sql_batch_exec: null batch");
    };
    // SAFETY: caller guarantees `sql` is a valid C string (see fn docs).
    let Some(sql) = (unsafe { cstr(sql) }) else {
        return fail_arg("loom_sql_batch_exec: null or non-UTF-8 sql");
    };
    match batch.store.exec_cbor(sql) {
        // SAFETY: `out_ptr`/`out_len` are writable per fn docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Make the batch's accumulated changes durable with a single atomic save (no history entry). Rejected
/// while an explicit SQL transaction is still open. The batch stays open and may run more statements.
///
/// # Safety
/// `batch` must be from [`loom_sql_batch_begin`] and live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_sql_batch_commit(batch: *mut LoomSqlBatch) -> i32 {
    clear_error();
    // SAFETY: caller guarantees `batch` is from `loom_sql_batch_begin` and live (see fn docs).
    let Some(batch) = (unsafe { batch.as_mut() }) else {
        return fail_arg("loom_sql_batch_commit: null batch");
    };
    match flush_batch(batch) {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// Like [`loom_sql_batch_commit`], but also records a VCS commit over the persisted state; writes the
/// new commit's content address to `*out` (free with [`loom_string_free`]). The VCS commit is distinct
/// from a SQL `COMMIT`. Rejected while an explicit SQL transaction is still open.
///
/// # Safety
/// `batch` must be from [`loom_sql_batch_begin`] and live; `message`/`author` valid C strings; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_sql_batch_commit_vcs(
    batch: *mut LoomSqlBatch,
    message: *const c_char,
    author: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    // SAFETY: caller guarantees `batch` is from `loom_sql_batch_begin` and live (see fn docs).
    let Some(batch) = (unsafe { batch.as_mut() }) else {
        return fail_arg("loom_sql_batch_commit_vcs: null batch");
    };
    // SAFETY: caller guarantees both arguments are valid C strings (see fn docs).
    let (Some(message), Some(author)) = (unsafe { cstr(message) }, unsafe { cstr(author) }) else {
        return fail_arg("loom_sql_batch_commit_vcs: null or non-UTF-8 argument");
    };
    match commit_vcs_batch(batch, message, author) {
        // SAFETY: `out` is writable per fn docs.
        Ok(hex) => unsafe { ok_str(out, &hex) },
        Err(e) => fail(e),
    }
}

/// Discard the batch's un-persisted in-memory changes (and any open SQL transaction), reloading the
/// store from the last durable state. The batch stays open. Returns `0`, or a non-zero [`Code`].
///
/// # Safety
/// `batch` must be from [`loom_sql_batch_begin`] and live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_sql_batch_abort(batch: *mut LoomSqlBatch) -> i32 {
    clear_error();
    // SAFETY: caller guarantees `batch` is from `loom_sql_batch_begin` and live (see fn docs).
    let Some(batch) = (unsafe { batch.as_mut() }) else {
        return fail_arg("loom_sql_batch_abort: null batch");
    };
    // Discard the overlay (and any open transaction) by re-snapshotting the durable state: a fresh
    // lock-free read view reflects everything flushed so far, so abort reverts only the un-persisted
    // changes since the last commit, not the whole batch.
    match load_store_write(&batch.path, batch.ns, &batch.db, &batch.auth) {
        Ok(store) => {
            batch.store = store;
            0
        }
        Err(e) => fail(e),
    }
}

/// Free a batch from [`loom_sql_batch_begin`], releasing the write lock. Passing null is a no-op.
/// **Closing without a commit discards every un-persisted change** (only [`loom_sql_batch_commit`] /
/// [`loom_sql_batch_commit_vcs`] make changes durable).
///
/// # Safety
/// `batch` must be a pointer returned by [`loom_sql_batch_begin`] and not previously freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_sql_batch_close(batch: *mut LoomSqlBatch) {
    if !batch.is_null() {
        // SAFETY: `batch` came from `Box::into_raw` in `loom_sql_batch_begin` (see fn docs); dropping it
        // drops the held `Loom` and releases the write lock.
        drop(unsafe { Box::from_raw(batch) });
    }
}
