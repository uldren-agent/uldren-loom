use super::*;
// The C-ABI functions live in per-facet modules; pull them in so the tests, which
// exercise the whole surface, can call them unqualified as before.
use crate::{
    archive::*, calendar::*, cas::*, columnar::*, contacts::*, dataframe::*, document::*, drive::*,
    files::*, graph::*, kv::*, lanes::*, ledger::*, mail::*, meetings::*, metrics::*, queue::*,
    replay::*, restore::*, search::*, tags::*, tickets::*, timeseries::*, vector::*,
};
use std::io::{Read, Write};

fn cs(s: &str) -> CString {
    CString::new(s).unwrap()
}

/// The thread's last error as `(code, message)`, or `None` if the last call succeeded.
fn last_err() -> Option<(i32, String)> {
    let mut code = 0i32;
    let mut msg: *mut c_char = core::ptr::null_mut();
    let mut len = 0usize;
    unsafe { loom_last_error(&mut code, &mut msg, &mut len) };
    if msg.is_null() {
        return None;
    }
    // SAFETY: `msg` is a live library string from `loom_last_error`.
    let s = unsafe { CStr::from_ptr(msg) }.to_str().unwrap().to_string();
    assert_eq!(len, s.len(), "reported length matches message");
    unsafe { loom_string_free(msg) };
    Some((code, s))
}

fn last_error_details() -> Vec<u8> {
    let mut ptr: *mut c_uchar = core::ptr::null_mut();
    let mut len = 0usize;
    unsafe { loom_last_error_details(&mut ptr, &mut len) };
    if ptr.is_null() {
        return Vec::new();
    }
    let bytes = unsafe { core::slice::from_raw_parts(ptr, len) }.to_vec();
    unsafe { loom_bytes_free(ptr, len) };
    bytes
}

#[test]
fn last_error_details_returns_structured_cbor() {
    let status = fail(LoomError::invalid("bad status").with_detail(
        loom_core::error::ErrorDetail::invalid_field("target_status", Some("accepted"), ["ready"]),
    ));
    assert_eq!(status, Code::InvalidArgument.as_i32());
    let details = last_error_details();
    assert!(!details.is_empty());
    let decoded = LoomError::details_from_cbor(&details).unwrap();
    assert_eq!(
        decoded,
        vec![loom_core::error::ErrorDetail::invalid_field(
            "target_status",
            Some("accepted"),
            ["ready"]
        )]
    );
    clear_error();
    assert!(last_error_details().is_empty());
}

/// Assert a status is success and take ownership of the out-string.
unsafe fn ok_out(status: i32, out: *mut c_char) -> String {
    assert_eq!(status, 0, "status {status}, err {:?}", last_err());
    assert!(!out.is_null(), "success must write an out-string");
    // SAFETY: `out` is a live library string on success.
    let s = unsafe { CStr::from_ptr(out) }.to_str().unwrap().to_string();
    unsafe { loom_string_free(out) };
    s
}

unsafe fn ok_raw(status: i32, ptr: *mut c_uchar, len: usize) -> Vec<u8> {
    assert_eq!(status, 0, "status {status}, err {:?}", last_err());
    assert!(
        !ptr.is_null() && len > 0,
        "success must write a non-empty buffer"
    );
    // SAFETY: on success `(ptr, len)` is a live result buffer.
    let bytes = unsafe { core::slice::from_raw_parts(ptr, len) }.to_vec();
    unsafe { loom_bytes_free(ptr, len) };
    bytes
}

fn json_field(json: &str, field: &str) -> String {
    let needle = format!("\"{field}\":\"");
    let start = json.find(&needle).expect("field exists") + needle.len();
    let rest = &json[start..];
    rest[..rest.find('"').expect("field ends")].to_string()
}

/// Assert a status is success, then render the canonical-CBOR result buffer `(ptr, len)` to JSON
/// (via the debug renderer) for content assertions, freeing both the buffer and the JSON string.
unsafe fn ok_render(status: i32, ptr: *mut c_uchar, len: usize) -> String {
    assert_eq!(status, 0, "status {status}, err {:?}", last_err());
    assert!(
        !ptr.is_null() && len > 0,
        "success must write a non-empty buffer"
    );
    let mut json: *mut c_char = core::ptr::null_mut();
    // SAFETY: `(ptr, len)` is a live result buffer; `json` is a valid out-pointer.
    let st = unsafe { loom_result_to_json(ptr, len, &mut json) };
    let rendered = unsafe { ok_out(st, json) };
    // SAFETY: `(ptr, len)` came from a result function and has not been freed.
    unsafe { loom_bytes_free(ptr, len) };
    rendered
}

fn cbor_get<'a>(map: &'a [(CborValue, CborValue)], key: &str) -> &'a CborValue {
    map.iter()
        .find_map(|(k, v)| match k {
            CborValue::Text(found) if found == key => Some(v),
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing key {key}"))
}

fn temp_loom() -> std::path::PathBuf {
    // A process-wide counter guarantees a distinct path per call even when two tests run on
    // different threads within the same nanosecond.
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let uniq = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("loom-ffi-{}-{seq}-{uniq}.loom", std::process::id()))
}

#[test]
fn studio_surface_catalog_json_over_the_c_abi() {
    let workspace = cs("studio");
    let set = cs("core");
    let mut out = core::ptr::null_mut();
    let json = unsafe {
        ok_out(
            loom_studio_surface_catalog_json(workspace.as_ptr(), set.as_ptr(), &mut out),
            out,
        )
    };
    assert!(json.contains("\"workspace\":\"studio\""), "{json}");
    assert!(json.contains("\"set\":\"core\""), "{json}");
    assert!(json.contains("\"app_id\":\"ticket-details\""), "{json}");

    let bad = cs("bogus");
    let mut bad_out = core::ptr::null_mut();
    let status =
        unsafe { loom_studio_surface_catalog_json(workspace.as_ptr(), bad.as_ptr(), &mut bad_out) };
    assert_eq!(status, Code::InvalidArgument.as_i32(), "{:?}", last_err());
    assert!(bad_out.is_null());
    assert!(
        last_err()
            .unwrap()
            .1
            .contains("unsupported Studio surface catalog set"),
    );
}

#[test]
fn lock_acquire_returns_token_json_over_the_c_abi() {
    let path = temp_loom();
    std::fs::write(&path, b"not-a-store").unwrap();
    let paths = daemon::paths(&path).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    std::fs::write(&paths.addr_file, listener.local_addr().unwrap().to_string()).unwrap();
    let join = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = String::new();
        stream.read_to_string(&mut request).unwrap();
        assert!(
            request.starts_with("lock-acquire\tresource\talice\ts1\texclusive\t60000\t"),
            "{request}"
        );
        stream
            .write_all(b"lock\tresource\talice\ts1\texclusive\t7\t12345\n")
            .unwrap();
    });
    let cpath = cs(path.to_str().unwrap());
    let key = cs("resource");
    let principal = cs("alice");
    let session = cs("s1");
    let mode = cs("exclusive");
    let mut out = core::ptr::null_mut();
    let json = unsafe {
        ok_out(
            loom_lock_acquire_json(
                cpath.as_ptr(),
                key.as_ptr(),
                principal.as_ptr(),
                session.as_ptr(),
                mode.as_ptr(),
                1,
                1,
                60000,
                daemon::DEFAULT_LOCK_WAIT_MS,
                &mut out,
            ),
            out,
        )
    };
    join.join().unwrap();
    assert!(json.contains("\"mode\":\"EXCLUSIVE\""), "{json}");
    assert!(
        json.contains("\"fence\":{\"authority\":0,\"epoch\":0,\"sequence\":7}"),
        "{json}"
    );
    assert!(json.contains("\"lease_deadline_ms\":12345"), "{json}");
    let _ = std::fs::remove_file(paths.addr_file);
    let _ = std::fs::remove_file(path);
}

#[test]
fn lock_release_preserves_daemon_error_code_over_the_c_abi() {
    let path = temp_loom();
    std::fs::write(&path, b"not-a-store").unwrap();
    let paths = daemon::paths(&path).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    std::fs::write(&paths.addr_file, listener.local_addr().unwrap().to_string()).unwrap();
    let join = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = String::new();
        stream.read_to_string(&mut request).unwrap();
        assert!(
            request.starts_with("lock-release\tresource\talice\ts1\texclusive\t7\t"),
            "{request}"
        );
        stream
            .write_all(b"error\tLOCK_NOT_HELD: lock is not held by this token\n")
            .unwrap();
    });
    let cpath = cs(path.to_str().unwrap());
    let key = cs("resource");
    let principal = cs("alice");
    let session = cs("s1");
    let mode = cs("exclusive");
    let st = unsafe {
        loom_lock_release(
            cpath.as_ptr(),
            key.as_ptr(),
            principal.as_ptr(),
            session.as_ptr(),
            mode.as_ptr(),
            1,
            1,
            7,
            0,
        )
    };
    join.join().unwrap();
    assert_eq!(st, Code::LockNotHeld.as_i32(), "{:?}", last_err());
    assert_eq!(last_err().unwrap().0, Code::LockNotHeld.as_i32());
    let _ = std::fs::remove_file(paths.addr_file);
    let _ = std::fs::remove_file(path);
}

/// The in-progress merge lifecycle over the C ABI: a same-primary-key change on two branches
/// conflicts on merge, is introspected, resolved, and continued into a merge commit; plus the
/// no-merge error paths.
#[test]
fn merge_conflict_continue_over_the_c_abi() {
    let dir = temp_loom();
    let (path, db, ns) = (cs(dir.to_str().unwrap()), cs("main"), cs("app"));
    let table = cs(".loom/facets/sql/main/tables/t");
    let author = cs("seed");

    // Seed a one-row table and commit on the default branch.
    unsafe {
        let mut s: *mut LoomSqlSession = core::ptr::null_mut();
        assert_eq!(
            loom_sql_open(path.as_ptr(), ns.as_ptr(), db.as_ptr(), &mut s),
            0
        );
        for stmt in [
            "CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)",
            "INSERT INTO t VALUES (1, 'base')",
        ] {
            let c = cs(stmt);
            let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
            let _ = ok_render(loom_sql_exec(s, c.as_ptr(), &mut p, &mut n), p, n);
        }
        let (m, mut out) = (cs("c1"), core::ptr::null_mut());
        let _ = ok_out(
            loom_sql_commit(s, m.as_ptr(), author.as_ptr(), &mut out),
            out,
        );
        loom_sql_close(s);
    }

    let mut h: *mut LoomSession = core::ptr::null_mut();
    assert_eq!(unsafe { loom_open(path.as_ptr(), &mut h) }, 0);
    let feature = cs("feature");

    // No merge in progress on a fresh workspace: introspection and error paths.
    let mut flag = -1i32;
    assert_eq!(
        unsafe { loom_merge_in_progress(h, ns.as_ptr(), &mut flag) },
        0
    );
    assert_eq!(flag, 0, "no merge in progress initially");
    let mut conflicts_out: *mut c_char = core::ptr::null_mut();
    let conflicts = unsafe {
        ok_out(
            loom_merge_conflicts(h, ns.as_ptr(), &mut conflicts_out),
            conflicts_out,
        )
    };
    assert_eq!(conflicts, "[]", "no conflicts initially");
    assert_ne!(
        unsafe { loom_merge_abort(h, ns.as_ptr()) },
        0,
        "abort with no merge in progress fails"
    );

    // feature branch: change the row to 'theirs'.
    assert_eq!(unsafe { loom_branch(h, ns.as_ptr(), feature.as_ptr()) }, 0);
    assert_eq!(
        unsafe { loom_checkout(h, ns.as_ptr(), feature.as_ptr()) },
        0
    );
    unsafe {
        let mut s: *mut LoomSqlSession = core::ptr::null_mut();
        assert_eq!(
            loom_sql_open(path.as_ptr(), ns.as_ptr(), db.as_ptr(), &mut s),
            0
        );
        let c = cs("UPDATE t SET v = 'theirs' WHERE id = 1");
        let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
        let _ = ok_render(loom_sql_exec(s, c.as_ptr(), &mut p, &mut n), p, n);
        let (m, mut out) = (cs("c2"), core::ptr::null_mut());
        let _ = ok_out(
            loom_sql_commit(s, m.as_ptr(), author.as_ptr(), &mut out),
            out,
        );
        loom_sql_close(s);
    }

    // back on main: change the same row to 'ours'.
    let main = cs("main");
    assert_eq!(unsafe { loom_checkout(h, ns.as_ptr(), main.as_ptr()) }, 0);
    unsafe {
        let mut s: *mut LoomSqlSession = core::ptr::null_mut();
        assert_eq!(
            loom_sql_open(path.as_ptr(), ns.as_ptr(), db.as_ptr(), &mut s),
            0
        );
        let c = cs("UPDATE t SET v = 'ours' WHERE id = 1");
        let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
        let _ = ok_render(loom_sql_exec(s, c.as_ptr(), &mut p, &mut n), p, n);
        let (m, mut out) = (cs("c3"), core::ptr::null_mut());
        let _ = ok_out(
            loom_sql_commit(s, m.as_ptr(), author.as_ptr(), &mut out),
            out,
        );
        loom_sql_close(s);
    }

    // Merge feature: the same-row change conflicts and enters the in-progress state.
    let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
    let merged = unsafe {
        ok_render(
            loom_merge(
                h,
                ns.as_ptr(),
                feature.as_ptr(),
                author.as_ptr(),
                0,
                &mut p,
                &mut n,
            ),
            p,
            n,
        )
    };
    assert!(
        merged.contains("conflicts"),
        "merge reports conflicts: {merged}"
    );

    let mut flag = -1i32;
    assert_eq!(
        unsafe { loom_merge_in_progress(h, ns.as_ptr(), &mut flag) },
        0
    );
    assert_eq!(flag, 1, "merge is in progress after a conflict");
    let mut conflicts_out: *mut c_char = core::ptr::null_mut();
    let conflicts = unsafe {
        ok_out(
            loom_merge_conflicts(h, ns.as_ptr(), &mut conflicts_out),
            conflicts_out,
        )
    };
    assert!(
        conflicts.contains("tables/t"),
        "conflict path reported: {conflicts}"
    );

    // Resolve to theirs and continue into a two-parent merge commit.
    assert_eq!(
        unsafe { loom_merge_resolve(h, ns.as_ptr(), table.as_ptr(), 1) },
        0,
        "resolve theirs: {:?}",
        last_err()
    );
    let mut commit_out: *mut c_char = core::ptr::null_mut();
    let commit = unsafe {
        ok_out(
            loom_merge_continue(h, ns.as_ptr(), author.as_ptr(), &mut commit_out),
            commit_out,
        )
    };
    assert!(
        commit.starts_with("blake3:"),
        "merge commit address: {commit}"
    );

    let mut flag = -1i32;
    assert_eq!(
        unsafe { loom_merge_in_progress(h, ns.as_ptr(), &mut flag) },
        0
    );
    assert_eq!(flag, 0, "merge state cleared after continue");

    // The resolved table holds the 'theirs' value.
    let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
    let rt = unsafe {
        ok_render(
            loom_sql_read_table(h, ns.as_ptr(), table.as_ptr(), &mut p, &mut n),
            p,
            n,
        )
    };
    assert!(rt.contains("theirs"), "resolved row is theirs: {rt}");
    unsafe { loom_close(h) };
    let _ = std::fs::remove_file(&dir);
}

/// The staging surface over the C ABI: status reports working changes, `loom_stage` moves them into
/// the shared index, and `loom_commit_staged` records only the index.
#[test]
fn staging_index_over_the_c_abi() {
    let dir = temp_loom();
    let (path, db, ns) = (cs(dir.to_str().unwrap()), cs("main"), cs("app"));
    let table = cs(".loom/facets/sql/main/tables/t");
    let author = cs("seed");

    // Seed a one-row table and commit everything (clean state).
    unsafe {
        let mut s: *mut LoomSqlSession = core::ptr::null_mut();
        assert_eq!(
            loom_sql_open(path.as_ptr(), ns.as_ptr(), db.as_ptr(), &mut s),
            0
        );
        for stmt in [
            "CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)",
            "INSERT INTO t VALUES (1, 'a')",
        ] {
            let c = cs(stmt);
            let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
            let _ = ok_render(loom_sql_exec(s, c.as_ptr(), &mut p, &mut n), p, n);
        }
        let (m, mut out) = (cs("c1"), core::ptr::null_mut());
        let _ = ok_out(
            loom_sql_commit(s, m.as_ptr(), author.as_ptr(), &mut out),
            out,
        );
        loom_sql_close(s);
    }

    let mut h: *mut LoomSession = core::ptr::null_mut();
    assert_eq!(unsafe { loom_open(path.as_ptr(), &mut h) }, 0);

    // Clean: status arrays all empty.
    let mut out: *mut c_char = core::ptr::null_mut();
    let clean = unsafe { ok_out(loom_status(h, ns.as_ptr(), &mut out), out) };
    assert!(
        clean.contains("\"staged\":[]") && clean.contains("\"unstaged\":[]"),
        "clean status: {clean}"
    );

    // An uncommitted INSERT leaves the table modified in the working tree (unstaged).
    unsafe {
        let mut s: *mut LoomSqlSession = core::ptr::null_mut();
        assert_eq!(
            loom_sql_open(path.as_ptr(), ns.as_ptr(), db.as_ptr(), &mut s),
            0
        );
        let c = cs("INSERT INTO t VALUES (2, 'b')");
        let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
        let _ = ok_render(loom_sql_exec(s, c.as_ptr(), &mut p, &mut n), p, n);
        loom_sql_close(s);
    }
    let mut out: *mut c_char = core::ptr::null_mut();
    let modified = unsafe { ok_out(loom_status(h, ns.as_ptr(), &mut out), out) };
    assert!(
        modified.contains("\"unstaged\":[{") && modified.contains("tables/t"),
        "modified table is unstaged: {modified}"
    );

    // Stage the table path; it moves to staged.
    assert_eq!(
        unsafe { loom_stage(h, ns.as_ptr(), table.as_ptr()) },
        0,
        "stage: {:?}",
        last_err()
    );
    let mut out: *mut c_char = core::ptr::null_mut();
    let staged = unsafe { ok_out(loom_status(h, ns.as_ptr(), &mut out), out) };
    assert!(
        staged.contains("\"staged\":[{") && staged.contains("\"unstaged\":[]"),
        "staged status: {staged}"
    );

    // commit_staged records the index; afterward the workspace is clean.
    let msg = cs("staged insert");
    let mut out: *mut c_char = core::ptr::null_mut();
    let commit = unsafe {
        ok_out(
            loom_commit_staged(h, ns.as_ptr(), author.as_ptr(), msg.as_ptr(), &mut out),
            out,
        )
    };
    assert!(commit.starts_with("blake3:"), "commit address: {commit}");
    let mut out: *mut c_char = core::ptr::null_mut();
    let after = unsafe { ok_out(loom_status(h, ns.as_ptr(), &mut out), out) };
    assert!(
        after.contains("\"staged\":[]") && after.contains("\"unstaged\":[]"),
        "clean after commit_staged: {after}"
    );

    unsafe { loom_close(h) };
    let _ = std::fs::remove_file(&dir);
}

/// The whole-file working-tree ops over the C ABI: write, read, append, remove.
#[test]
fn file_ops_over_the_c_abi() {
    let dir = temp_loom();
    let path = cs(dir.to_str().unwrap());
    let profile = cs("default");
    assert_eq!(
        unsafe {
            loom_create(
                path.as_ptr(),
                profile.as_ptr(),
                core::ptr::null(),
                core::ptr::null(),
                0,
            )
        },
        0
    );

    let mut h: *mut LoomSession = core::ptr::null_mut();
    assert_eq!(unsafe { loom_open(path.as_ptr(), &mut h) }, 0);
    let (facet, ns, file) = (cs("files"), cs("docs"), cs("a.txt"));

    // Create the files workspace, then write/read a file.
    let mut nsout: *mut c_char = core::ptr::null_mut();
    let _ = unsafe {
        ok_out(
            loom_workspace_create(h, ns.as_ptr(), facet.as_ptr(), &mut nsout),
            nsout,
        )
    };
    let hello = b"hello";
    assert_eq!(
        unsafe {
            loom_write_file(
                h,
                ns.as_ptr(),
                file.as_ptr(),
                hello.as_ptr(),
                hello.len(),
                0,
            )
        },
        0,
        "write: {:?}",
        last_err()
    );
    let read_back = |h: *mut LoomSession| -> Option<Vec<u8>> {
        let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
        let st = unsafe { loom_read_file(h, ns.as_ptr(), file.as_ptr(), &mut p, &mut n) };
        if st != 0 {
            return None;
        }
        // SAFETY: on success `(p, n)` is a live result buffer.
        let bytes = if p.is_null() {
            Vec::new()
        } else {
            unsafe { core::slice::from_raw_parts(p, n) }.to_vec()
        };
        unsafe { loom_bytes_free(p, n) };
        Some(bytes)
    };
    assert_eq!(read_back(h).as_deref(), Some(&b"hello"[..]));

    // Append creates-or-concatenates.
    let bang = b"!";
    assert_eq!(
        unsafe { loom_append_file(h, ns.as_ptr(), file.as_ptr(), bang.as_ptr(), bang.len(),) },
        0
    );
    assert_eq!(read_back(h).as_deref(), Some(&b"hello!"[..]));

    // Remove deletes the file.
    assert_eq!(
        unsafe { loom_remove_file(h, ns.as_ptr(), file.as_ptr()) },
        0
    );
    assert_eq!(read_back(h), None, "removed file is absent");

    unsafe { loom_close(h) };
    let _ = std::fs::remove_file(&dir);
}

/// Byte-range I/O and a file handle over the C ABI: write_at/read_at/truncate plus the open file
/// description (open, positional write, read, stat, close).
#[test]
fn file_handles_over_the_c_abi() {
    let dir = temp_loom();
    let path = cs(dir.to_str().unwrap());
    let profile = cs("default");
    assert_eq!(
        unsafe {
            loom_create(
                path.as_ptr(),
                profile.as_ptr(),
                core::ptr::null(),
                core::ptr::null(),
                0,
            )
        },
        0
    );

    let mut h: *mut LoomSession = core::ptr::null_mut();
    assert_eq!(unsafe { loom_open(path.as_ptr(), &mut h) }, 0);
    let (facet, ns, file) = (cs("files"), cs("docs"), cs("a.txt"));
    let mut nsout: *mut c_char = core::ptr::null_mut();
    let _ = unsafe {
        ok_out(
            loom_workspace_create(h, ns.as_ptr(), facet.as_ptr(), &mut nsout),
            nsout,
        )
    };

    // write_at on a missing file zero-fills the gap; read_at clamps.
    let xy = b"XY";
    assert_eq!(
        unsafe { loom_write_at(h, ns.as_ptr(), file.as_ptr(), 5, xy.as_ptr(), xy.len(),) },
        0,
        "write_at: {:?}",
        last_err()
    );
    let read_range = |off: u64, len: u64| -> Vec<u8> {
        let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
        let st = unsafe { loom_read_at(h, ns.as_ptr(), file.as_ptr(), off, len, &mut p, &mut n) };
        assert_eq!(st, 0, "read_at: {:?}", last_err());
        let bytes = if p.is_null() {
            Vec::new()
        } else {
            unsafe { core::slice::from_raw_parts(p, n) }.to_vec()
        };
        unsafe { loom_bytes_free(p, n) };
        bytes
    };
    assert_eq!(read_range(0, 100), vec![0, 0, 0, 0, 0, b'X', b'Y']);
    // truncate shrinks.
    assert_eq!(
        unsafe { loom_truncate_file(h, ns.as_ptr(), file.as_ptr(), 6) },
        0
    );
    assert_eq!(read_range(0, 100), vec![0, 0, 0, 0, 0, b'X']);

    // Open a read-write handle, positionally write, stat, read back.
    let mut fh: u64 = 0;
    assert_eq!(
        unsafe { loom_file_open(h, ns.as_ptr(), file.as_ptr(), 2, &mut fh) },
        0,
        "file_open: {:?}",
        last_err()
    );
    let mut written: u64 = 0;
    let z = b"Z";
    assert_eq!(
        unsafe { loom_file_write_at(h, fh, 0, z.as_ptr(), z.len(), &mut written) },
        0
    );
    assert_eq!(written, 1);
    let (mut sz, mut md) = (0u64, 0u32);
    assert_eq!(unsafe { loom_file_stat(h, fh, &mut sz, &mut md) }, 0);
    assert_eq!(sz, 6, "size after positional write");
    // Positional read sees the edit.
    let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
    assert_eq!(
        unsafe { loom_file_read_at(h, fh, 0, 100, &mut p, &mut n) },
        0
    );
    let got = unsafe { core::slice::from_raw_parts(p, n) }.to_vec();
    unsafe { loom_bytes_free(p, n) };
    assert_eq!(got, vec![b'Z', 0, 0, 0, 0, b'X']);
    assert_eq!(unsafe { loom_file_close(h, fh) }, 0);

    unsafe { loom_close(h) };
    let _ = std::fs::remove_file(&dir);
}

/// Symlink create + read over the C ABI.
#[test]
fn symlink_over_the_c_abi() {
    let dir = temp_loom();
    let path = cs(dir.to_str().unwrap());
    let profile = cs("default");
    assert_eq!(
        unsafe {
            loom_create(
                path.as_ptr(),
                profile.as_ptr(),
                core::ptr::null(),
                core::ptr::null(),
                0,
            )
        },
        0
    );
    let mut h: *mut LoomSession = core::ptr::null_mut();
    assert_eq!(unsafe { loom_open(path.as_ptr(), &mut h) }, 0);
    let (facet, ns) = (cs("files"), cs("docs"));
    let mut nsout: *mut c_char = core::ptr::null_mut();
    let _ = unsafe {
        ok_out(
            loom_workspace_create(h, ns.as_ptr(), facet.as_ptr(), &mut nsout),
            nsout,
        )
    };
    let (target, link) = (cs("some/target"), cs("link"));
    assert_eq!(
        unsafe { loom_symlink(h, ns.as_ptr(), target.as_ptr(), link.as_ptr(),) },
        0,
        "symlink: {:?}",
        last_err()
    );
    let mut out: *mut c_char = core::ptr::null_mut();
    let read = unsafe { ok_out(loom_read_link(h, ns.as_ptr(), link.as_ptr(), &mut out), out) };
    assert_eq!(read, "some/target");
    unsafe { loom_close(h) };
    let _ = std::fs::remove_file(&dir);
}

/// Tag verbs over the C ABI: create (lightweight + annotated), list, target, rename, delete.
#[test]
fn tags_over_the_c_abi() {
    let dir = temp_loom();
    let path = cs(dir.to_str().unwrap());
    let profile = cs("default");
    assert_eq!(
        unsafe {
            loom_create(
                path.as_ptr(),
                profile.as_ptr(),
                core::ptr::null(),
                core::ptr::null(),
                0,
            )
        },
        0
    );
    let mut h: *mut LoomSession = core::ptr::null_mut();
    assert_eq!(unsafe { loom_open(path.as_ptr(), &mut h) }, 0);
    let (facet, ns) = (cs("files"), cs("docs"));
    let mut nsout: *mut c_char = core::ptr::null_mut();
    let _ = unsafe {
        ok_out(
            loom_workspace_create(h, ns.as_ptr(), facet.as_ptr(), &mut nsout),
            nsout,
        )
    };
    // A commit to tag.
    let file = cs("a.txt");
    let hello = b"hello";
    assert_eq!(
        unsafe {
            loom_write_file(
                h,
                ns.as_ptr(),
                file.as_ptr(),
                hello.as_ptr(),
                hello.len(),
                0,
            )
        },
        0
    );
    let (author, msg) = (cs("nas"), cs("init"));
    let mut out: *mut c_char = core::ptr::null_mut();
    let commit = unsafe {
        ok_out(
            loom_commit(h, ns.as_ptr(), author.as_ptr(), msg.as_ptr(), &mut out),
            out,
        )
    };

    // Lightweight tag at HEAD returns the commit digest.
    let (v1, head, empty, tagger) = (cs("v1"), cs("HEAD"), cs(""), cs("nas"));
    let mut out: *mut c_char = core::ptr::null_mut();
    let target = unsafe {
        ok_out(
            loom_tag_create(
                h,
                ns.as_ptr(),
                v1.as_ptr(),
                head.as_ptr(),
                tagger.as_ptr(),
                empty.as_ptr(),
                &mut out,
            ),
            out,
        )
    };
    assert_eq!(target, commit, "lightweight tag points at the commit");

    // Annotated tag returns the tag object digest (not the commit).
    let (v1ann, message) = (cs("v1-ann"), cs("release 1"));
    let mut out: *mut c_char = core::ptr::null_mut();
    let ann = unsafe {
        ok_out(
            loom_tag_create(
                h,
                ns.as_ptr(),
                v1ann.as_ptr(),
                head.as_ptr(),
                tagger.as_ptr(),
                message.as_ptr(),
                &mut out,
            ),
            out,
        )
    };
    assert_ne!(ann, commit, "annotated tag points at the tag object");

    // List is JSON and sorted.
    let mut out: *mut c_char = core::ptr::null_mut();
    let list = unsafe { ok_out(loom_tag_list(h, ns.as_ptr(), &mut out), out) };
    assert_eq!(list, "[\"v1\",\"v1-ann\"]", "tag list JSON");

    // Target reads back the commit; found flag set.
    let (mut tp, mut found) = (core::ptr::null_mut(), 0i32);
    assert_eq!(
        unsafe { loom_tag_target(h, ns.as_ptr(), v1.as_ptr(), &mut tp, &mut found,) },
        0
    );
    assert_eq!(found, 1);
    let read = unsafe { CStr::from_ptr(tp) }.to_str().unwrap().to_string();
    unsafe { loom_string_free(tp) };
    assert_eq!(read, commit);

    // Rename then delete.
    let v2 = cs("v2");
    assert_eq!(
        unsafe { loom_tag_rename(h, ns.as_ptr(), v1.as_ptr(), v2.as_ptr()) },
        0
    );
    assert_eq!(unsafe { loom_tag_delete(h, ns.as_ptr(), v2.as_ptr()) }, 0);
    // A missing tag's target reports not-found via the flag.
    let (mut tp2, mut found2) = (core::ptr::null_mut(), 9i32);
    assert_eq!(
        unsafe { loom_tag_target(h, ns.as_ptr(), v2.as_ptr(), &mut tp2, &mut found2,) },
        0
    );
    assert_eq!(found2, 0, "deleted tag is absent");
    assert!(tp2.is_null());

    unsafe { loom_close(h) };
    let _ = std::fs::remove_file(&dir);
}

/// restore_file + restore_path over the C ABI.
#[test]
fn restore_over_the_c_abi() {
    let dir = temp_loom();
    let path = cs(dir.to_str().unwrap());
    let profile = cs("default");
    assert_eq!(
        unsafe {
            loom_create(
                path.as_ptr(),
                profile.as_ptr(),
                core::ptr::null(),
                core::ptr::null(),
                0,
            )
        },
        0
    );
    let mut h: *mut LoomSession = core::ptr::null_mut();
    assert_eq!(unsafe { loom_open(path.as_ptr(), &mut h) }, 0);
    let (facet, ns, file) = (cs("files"), cs("docs"), cs("a.txt"));
    let mut nsout: *mut c_char = core::ptr::null_mut();
    let _ = unsafe {
        ok_out(
            loom_workspace_create(h, ns.as_ptr(), facet.as_ptr(), &mut nsout),
            nsout,
        )
    };
    let v1 = b"v1";
    assert_eq!(
        unsafe { loom_write_file(h, ns.as_ptr(), file.as_ptr(), v1.as_ptr(), v1.len(), 0,) },
        0
    );
    let (author, msg) = (cs("nas"), cs("init"));
    let mut out: *mut c_char = core::ptr::null_mut();
    let _ = unsafe {
        ok_out(
            loom_commit(h, ns.as_ptr(), author.as_ptr(), msg.as_ptr(), &mut out),
            out,
        )
    };
    // Edit the working tree, then restore the path from HEAD.
    let v2 = b"v2";
    assert_eq!(
        unsafe { loom_write_file(h, ns.as_ptr(), file.as_ptr(), v2.as_ptr(), v2.len(), 0,) },
        0
    );
    let (head, root) = (cs("HEAD"), cs(""));
    assert_eq!(
        unsafe { loom_restore_file(h, ns.as_ptr(), head.as_ptr(), file.as_ptr()) },
        0,
        "restore_file: {:?}",
        last_err()
    );
    let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
    assert_eq!(
        unsafe { loom_read_file(h, ns.as_ptr(), file.as_ptr(), &mut p, &mut n,) },
        0
    );
    let got = unsafe { core::slice::from_raw_parts(p, n) }.to_vec();
    unsafe { loom_bytes_free(p, n) };
    assert_eq!(got, b"v1", "restore_file reverted the edit");
    // restore_path over the whole tree also succeeds.
    assert_eq!(
        unsafe { loom_restore_path(h, ns.as_ptr(), head.as_ptr(), root.as_ptr()) },
        0,
        "restore_path: {:?}",
        last_err()
    );

    unsafe { loom_close(h) };
    let _ = std::fs::remove_file(&dir);
}

/// History replay over the C ABI: revert (replayed), empty cherry-pick, and a no-op rebase onto HEAD.
#[test]
fn replay_over_the_c_abi() {
    let dir = temp_loom();
    let path = cs(dir.to_str().unwrap());
    let profile = cs("default");
    assert_eq!(
        unsafe {
            loom_create(
                path.as_ptr(),
                profile.as_ptr(),
                core::ptr::null(),
                core::ptr::null(),
                0,
            )
        },
        0
    );
    let mut h: *mut LoomSession = core::ptr::null_mut();
    assert_eq!(unsafe { loom_open(path.as_ptr(), &mut h) }, 0);
    let (facet, ns, file, author) = (cs("files"), cs("docs"), cs("a.txt"), cs("nas"));
    let mut nsout: *mut c_char = core::ptr::null_mut();
    let _ = unsafe {
        ok_out(
            loom_workspace_create(h, ns.as_ptr(), facet.as_ptr(), &mut nsout),
            nsout,
        )
    };
    let write = |bytes: &[u8]| {
        assert_eq!(
            unsafe {
                loom_write_file(
                    h,
                    ns.as_ptr(),
                    file.as_ptr(),
                    bytes.as_ptr(),
                    bytes.len(),
                    0,
                )
            },
            0
        );
    };
    let commit = |m: &str| -> String {
        let msg = cs(m);
        let mut out: *mut c_char = core::ptr::null_mut();
        unsafe {
            ok_out(
                loom_commit(h, ns.as_ptr(), author.as_ptr(), msg.as_ptr(), &mut out),
                out,
            )
        }
    };
    write(b"v1");
    commit("init");
    write(b"v2");
    let c1 = commit("bump");

    // Revert c1: the outcome is "replayed" and a.txt reverts to v1.
    let commits = cs(&c1);
    let mut out: *mut c_char = core::ptr::null_mut();
    let json = unsafe {
        ok_out(
            loom_revert(
                h,
                ns.as_ptr(),
                commits.as_ptr(),
                author.as_ptr(),
                0,
                &mut out,
            ),
            out,
        )
    };
    assert!(json.contains("\"replayed\""), "revert outcome: {json}");
    let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
    assert_eq!(
        unsafe { loom_read_file(h, ns.as_ptr(), file.as_ptr(), &mut p, &mut n,) },
        0
    );
    let got = unsafe { core::slice::from_raw_parts(p, n) }.to_vec();
    unsafe { loom_bytes_free(p, n) };
    assert_eq!(got, b"v1", "revert undid the bump");

    // An empty cherry-pick is "empty"; a rebase onto HEAD is "empty".
    let (empty, head) = (cs(""), cs("HEAD"));
    let mut out: *mut c_char = core::ptr::null_mut();
    let cp = unsafe {
        ok_out(
            loom_cherry_pick(h, ns.as_ptr(), empty.as_ptr(), 0, &mut out),
            out,
        )
    };
    assert!(cp.contains("\"empty\""), "empty cherry-pick: {cp}");
    let mut out: *mut c_char = core::ptr::null_mut();
    let rb = unsafe { ok_out(loom_rebase(h, ns.as_ptr(), head.as_ptr(), 0, &mut out), out) };
    assert!(rb.contains("\"empty\""), "rebase onto HEAD: {rb}");

    // Squash everything after c1 (the bump) into one commit; returns a digest.
    let onto = cs(&c1);
    let sqmsg = cs("squashed");
    let mut out: *mut c_char = core::ptr::null_mut();
    let sq = unsafe {
        ok_out(
            loom_squash(
                h,
                ns.as_ptr(),
                onto.as_ptr(),
                author.as_ptr(),
                sqmsg.as_ptr(),
                &mut out,
            ),
            out,
        )
    };
    assert!(sq.starts_with("blake3:"), "squash digest: {sq}");

    unsafe { loom_close(h) };
    let _ = std::fs::remove_file(&dir);
}

/// `loom_create` chooses the identity profile (default vs FIPS) and sets up at-rest encryption, and
/// the keyed openers unlock an encrypted store.
#[test]
fn create_chooses_profile_and_encryption_over_the_c_abi() {
    let (ns, db) = (cs("app"), cs("main"));

    // 1. A FIPS, unencrypted store: profile "fips", no passphrase.
    let fips = temp_loom();
    let fpath = cs(fips.to_str().unwrap());
    let profile = cs("fips");
    let st = unsafe {
        loom_create(
            fpath.as_ptr(),
            profile.as_ptr(),
            core::ptr::null(),
            core::ptr::null(),
            0,
        )
    };
    assert_eq!(st, 0, "loom_create fips failed: {:?}", last_err());
    // The store's identity profile is SHA-256 (the FIPS digest).
    let store = FileStore::open(&fips).unwrap();
    assert_eq!(store.digest_algo(), Algo::Sha256);
    drop(store);
    // It opens and round-trips with no key.
    let mut session: *mut LoomSqlSession = core::ptr::null_mut();
    let st = unsafe { loom_sql_open(fpath.as_ptr(), ns.as_ptr(), db.as_ptr(), &mut session) };
    assert_eq!(st, 0, "open fips failed: {:?}", last_err());
    unsafe { loom_sql_close(session) };

    // 2. An encrypted store under the default profile: a passphrase is supplied.
    let enc = temp_loom();
    let epath = cs(enc.to_str().unwrap());
    let dflt = cs("default");
    let pass = b"correct horse battery staple";
    let st = unsafe {
        loom_create(
            epath.as_ptr(),
            dflt.as_ptr(),
            core::ptr::null(),
            pass.as_ptr(),
            pass.len(),
        )
    };
    assert_eq!(st, 0, "loom_create encrypted failed: {:?}", last_err());

    // Opening without the key fails loudly (E2eLocked), never a silent plaintext open.
    let mut locked: *mut LoomSqlSession = core::ptr::null_mut();
    let st = unsafe { loom_sql_open(epath.as_ptr(), ns.as_ptr(), db.as_ptr(), &mut locked) };
    assert_ne!(st, 0, "an encrypted store must not open without a key");
    assert_eq!(last_err().unwrap().0, Code::E2eLocked.as_i32());

    // Opening keyed succeeds and round-trips a write through the sealed store.
    let mut session: *mut LoomSqlSession = core::ptr::null_mut();
    let st = unsafe {
        loom_sql_open_keyed(
            epath.as_ptr(),
            ns.as_ptr(),
            db.as_ptr(),
            pass.as_ptr(),
            pass.len(),
            &mut session,
        )
    };
    assert_eq!(st, 0, "keyed open failed: {:?}", last_err());
    unsafe {
        let create = cs("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)");
        let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
        let _ = ok_render(
            loom_sql_exec(session, create.as_ptr(), &mut p, &mut n),
            p,
            n,
        );
        let insert = cs("INSERT INTO t VALUES (1, 'secret')");
        let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
        let _ = ok_render(
            loom_sql_exec(session, insert.as_ptr(), &mut p, &mut n),
            p,
            n,
        );
        let select = cs("SELECT v FROM t");
        let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
        let json = ok_render(
            loom_sql_exec(session, select.as_ptr(), &mut p, &mut n),
            p,
            n,
        );
        assert!(json.contains("secret"), "encrypted round-trip: {json}");
        loom_sql_close(session);
    }

    // A wrong key fails the AEAD (E2eKeyInvalid), not a panic or a silent miss.
    let wrong = b"nope";
    let mut bad: *mut LoomSqlSession = core::ptr::null_mut();
    let st = unsafe {
        loom_sql_open_keyed(
            epath.as_ptr(),
            ns.as_ptr(),
            db.as_ptr(),
            wrong.as_ptr(),
            wrong.len(),
            &mut bad,
        )
    };
    assert_ne!(st, 0, "a wrong key must fail");
    assert_eq!(last_err().unwrap().0, Code::E2eKeyInvalid.as_i32());
}

/// Create a store wrapped under a caller 256-bit KEK and open it
/// with the KEK; a wrong KEK fails `E2E_KEY_INVALID`, and a passphrase open fails `E2E_LOCKED`.
#[test]
fn create_and_open_with_kek_over_the_c_abi() {
    let (ns, db) = (cs("app"), cs("main"));
    let enc = temp_loom();
    let epath = cs(enc.to_str().unwrap());
    let dflt = cs("default");
    let kek = [0x5au8; 32];
    let st = unsafe {
        loom_create_with_kek(
            epath.as_ptr(),
            dflt.as_ptr(),
            core::ptr::null(),
            kek.as_ptr(),
            kek.len(),
        )
    };
    assert_eq!(st, 0, "loom_create_with_kek failed: {:?}", last_err());

    // Opening with a passphrase fails: the only wrap is a RawKek entry (E2eLocked - no passphrase
    // entry to even attempt).
    let pass = b"not the kek";
    let mut locked: *mut LoomSqlSession = core::ptr::null_mut();
    let st = unsafe {
        loom_sql_open_keyed(
            epath.as_ptr(),
            ns.as_ptr(),
            db.as_ptr(),
            pass.as_ptr(),
            pass.len(),
            &mut locked,
        )
    };
    assert_ne!(st, 0, "a passphrase must not open a KEK-only store");

    // The right KEK opens it and round-trips a write.
    let mut session: *mut LoomSqlSession = core::ptr::null_mut();
    let st = unsafe {
        loom_sql_open_with_kek(
            epath.as_ptr(),
            ns.as_ptr(),
            db.as_ptr(),
            kek.as_ptr(),
            kek.len(),
            &mut session,
        )
    };
    assert_eq!(st, 0, "keyed-with-kek open failed: {:?}", last_err());
    unsafe {
        let create = cs("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)");
        let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
        let _ = ok_render(
            loom_sql_exec(session, create.as_ptr(), &mut p, &mut n),
            p,
            n,
        );
        let insert = cs("INSERT INTO t VALUES (1, 'kek-secret')");
        let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
        let _ = ok_render(
            loom_sql_exec(session, insert.as_ptr(), &mut p, &mut n),
            p,
            n,
        );
        let select = cs("SELECT v FROM t");
        let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
        let json = ok_render(
            loom_sql_exec(session, select.as_ptr(), &mut p, &mut n),
            p,
            n,
        );
        assert!(json.contains("kek-secret"), "kek round-trip: {json}");
        loom_sql_close(session);
    }

    // A wrong KEK fails the AEAD.
    let wrong = [0u8; 32];
    let mut bad: *mut LoomSqlSession = core::ptr::null_mut();
    let st = unsafe {
        loom_sql_open_with_kek(
            epath.as_ptr(),
            ns.as_ptr(),
            db.as_ptr(),
            wrong.as_ptr(),
            wrong.len(),
            &mut bad,
        )
    };
    assert_ne!(st, 0, "a wrong KEK must fail");
    assert_eq!(last_err().unwrap().0, Code::E2eKeyInvalid.as_i32());

    // A wrong-length KEK is an argument error.
    let short = [1u8; 16];
    let mut bad2: *mut LoomSession = core::ptr::null_mut();
    let st = unsafe { loom_open_with_kek(epath.as_ptr(), short.as_ptr(), short.len(), &mut bad2) };
    assert_eq!(
        st,
        Code::InvalidArgument.as_i32(),
        "a 16-byte KEK must be rejected"
    );
}

#[test]
fn add_and_remove_wraps_over_the_c_abi() {
    let enc = temp_loom();
    let epath = cs(enc.to_str().unwrap());
    let dflt = cs("default");
    let pass = b"recovery-passphrase";
    let st = unsafe {
        loom_create(
            epath.as_ptr(),
            dflt.as_ptr(),
            core::ptr::null(),
            pass.as_ptr(),
            pass.len(),
        )
    };
    assert_eq!(st, 0, "loom_create encrypted failed: {:?}", last_err());

    let mut h: *mut LoomSession = core::ptr::null_mut();
    let st = unsafe { loom_open_keyed(epath.as_ptr(), pass.as_ptr(), pass.len(), &mut h) };
    assert_eq!(st, 0, "loom_open_keyed failed: {:?}", last_err());
    let kek = [0x5au8; 32];
    let st = unsafe { loom_key_add_wrap_with_kek(h, kek.as_ptr(), kek.len(), false) };
    assert_eq!(st, 0, "add KEK wrap failed: {:?}", last_err());
    unsafe { loom_close(h) };

    let mut by_kek: *mut LoomSession = core::ptr::null_mut();
    let st = unsafe { loom_open_with_kek(epath.as_ptr(), kek.as_ptr(), kek.len(), &mut by_kek) };
    assert_eq!(st, 0, "open with added KEK failed: {:?}", last_err());
    let st = unsafe { loom_key_remove_wrap(by_kek, 0, false) };
    assert_ne!(st, 0, "removing recovery wrap should need override");
    assert_eq!(last_err().unwrap().0, Code::InvalidArgument.as_i32());
    let st = unsafe { loom_key_remove_wrap(by_kek, 0, true) };
    assert_eq!(
        st,
        0,
        "remove recovery wrap with override failed: {:?}",
        last_err()
    );
    unsafe { loom_close(by_kek) };

    let mut by_pass: *mut LoomSession = core::ptr::null_mut();
    let st = unsafe { loom_open_keyed(epath.as_ptr(), pass.as_ptr(), pass.len(), &mut by_pass) };
    assert_ne!(st, 0, "removed passphrase wrap must not open");
    assert_eq!(last_err().unwrap().0, Code::E2eKeyInvalid.as_i32());

    let mut by_kek_again: *mut LoomSession = core::ptr::null_mut();
    let st =
        unsafe { loom_open_with_kek(epath.as_ptr(), kek.as_ptr(), kek.len(), &mut by_kek_again) };
    assert_eq!(st, 0, "remaining KEK wrap must open: {:?}", last_err());
    unsafe { loom_close(by_kek_again) };
}

#[test]
fn duplicate_add_wrap_is_already_exists_over_the_c_abi() {
    let enc = temp_loom();
    let epath = cs(enc.to_str().unwrap());
    let dflt = cs("default");
    let pass = b"recovery-passphrase";
    let st = unsafe {
        loom_create(
            epath.as_ptr(),
            dflt.as_ptr(),
            core::ptr::null(),
            pass.as_ptr(),
            pass.len(),
        )
    };
    assert_eq!(st, 0, "loom_create encrypted failed: {:?}", last_err());

    let mut h: *mut LoomSession = core::ptr::null_mut();
    let st = unsafe { loom_open_keyed(epath.as_ptr(), pass.as_ptr(), pass.len(), &mut h) };
    assert_eq!(st, 0, "loom_open_keyed failed: {:?}", last_err());

    let st = unsafe { loom_key_add_wrap_keyed(h, pass.as_ptr(), pass.len(), false) };
    assert_eq!(
        st,
        Code::AlreadyExists.as_i32(),
        "re-adding the same passphrase must be AlreadyExists"
    );
    assert_eq!(last_err().unwrap().0, Code::AlreadyExists.as_i32());

    let kek = [0x5au8; 32];
    let st = unsafe { loom_key_add_wrap_with_kek(h, kek.as_ptr(), kek.len(), false) };
    assert_eq!(st, 0, "first KEK wrap failed: {:?}", last_err());
    let st = unsafe { loom_key_add_wrap_with_kek(h, kek.as_ptr(), kek.len(), false) };
    assert_eq!(
        st,
        Code::AlreadyExists.as_i32(),
        "re-adding the same KEK must be AlreadyExists"
    );
    assert_eq!(last_err().unwrap().0, Code::AlreadyExists.as_i32());

    unsafe { loom_close(h) };
}

#[test]
fn identity_and_acl_management_over_the_c_abi() {
    let dir = temp_loom();
    let path = cs(dir.to_str().unwrap());
    let dflt = cs("default");
    assert_eq!(
        unsafe {
            loom_create(
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
    assert_eq!(unsafe { loom_open(path.as_ptr(), &mut handle) }, 0);

    let mut identity_out = core::ptr::null_mut();
    let identity_json = unsafe {
        ok_out(
            loom_identity_list_json(handle, &mut identity_out),
            identity_out,
        )
    };
    assert!(identity_json.contains("\"authenticated_mode\":false"));
    assert!(identity_json.contains("\"kind\":\"root\""));
    assert!(identity_json.contains("\"roles\""));
    assert!(identity_json.contains("\"name\":\"admin\""));
    let root = json_field(&identity_json, "root");

    let mut acl_out = core::ptr::null_mut();
    let acl_json = unsafe { ok_out(loom_acl_list_json(handle, &mut acl_out), acl_out) };
    assert!(acl_json.contains("\"rights\":[\"admin\"]"), "{acl_json}");
    assert!(acl_json.contains(&root), "{acl_json}");

    let root_c = cs(&root);
    let passphrase = b"root-pass";
    assert_eq!(
        unsafe {
            loom_identity_set_passphrase(
                handle,
                root_c.as_ptr(),
                passphrase.as_ptr(),
                passphrase.len(),
            )
        },
        0,
        "set passphrase failed: {:?}",
        last_err()
    );

    let user = cs("alice");
    let kind = cs("user");
    let mut denied_user = core::ptr::null_mut();
    let denied = unsafe {
        loom_identity_add_principal(
            handle,
            user.as_ptr(),
            user.as_ptr(),
            kind.as_ptr(),
            &mut denied_user,
        )
    };
    assert_eq!(denied, Code::AuthenticationFailed.as_i32());
    assert!(denied_user.is_null());

    assert_eq!(
        unsafe {
            loom_authenticate_passphrase(
                handle,
                root_c.as_ptr(),
                passphrase.as_ptr(),
                passphrase.len(),
            )
        },
        0,
        "auth failed: {:?}",
        last_err()
    );

    let mut user_out = core::ptr::null_mut();
    let user_id = unsafe {
        ok_out(
            loom_identity_add_principal(
                handle,
                user.as_ptr(),
                user.as_ptr(),
                kind.as_ptr(),
                &mut user_out,
            ),
            user_out,
        )
    };
    assert!(user_id.contains('-'));

    let user_c = cs(&user_id);
    let admin_role = loom_core::ROLE_ADMIN_ID.to_string();
    let admin_role_c = cs(&admin_role);
    assert_eq!(
        unsafe { loom_identity_assign_role(handle, user_c.as_ptr(), admin_role_c.as_ptr()) },
        0,
        "assign role failed: {:?}",
        last_err()
    );
    let mut identity_out2 = core::ptr::null_mut();
    let identity_json2 = unsafe {
        ok_out(
            loom_identity_list_json(handle, &mut identity_out2),
            identity_out2,
        )
    };
    assert!(identity_json2.contains(&user_id), "{identity_json2}");
    assert!(identity_json2.contains(&admin_role), "{identity_json2}");
    assert!(
        identity_json2.contains("\"external_credentials\""),
        "{identity_json2}"
    );
    assert!(
        identity_json2.contains("\"public_keys\""),
        "{identity_json2}"
    );

    let external_kind = cs("oidc-subject");
    let external_label = cs("okta-prod");
    let external_issuer = cs("https://issuer.example");
    let external_subject = cs("00u123");
    let external_material = cs("sha256:metadata");
    let mut external_out = core::ptr::null_mut();
    let external_id = unsafe {
        ok_out(
            loom_identity_create_external_credential(
                handle,
                user_c.as_ptr(),
                external_kind.as_ptr(),
                external_label.as_ptr(),
                external_issuer.as_ptr(),
                external_subject.as_ptr(),
                external_material.as_ptr(),
                &mut external_out,
            ),
            external_out,
        )
    };
    assert!(external_id.contains('-'));
    let mut identity_out3 = core::ptr::null_mut();
    let identity_json3 = unsafe {
        ok_out(
            loom_identity_list_json(handle, &mut identity_out3),
            identity_out3,
        )
    };
    assert!(identity_json3.contains(&external_id), "{identity_json3}");
    assert!(
        identity_json3.contains("\"kind\":\"oidc_subject\""),
        "{identity_json3}"
    );
    assert!(
        identity_json3.contains("\"issuer\":\"https://issuer.example\""),
        "{identity_json3}"
    );
    let external_id_c = cs(&external_id);
    assert_eq!(
        unsafe { loom_identity_revoke_external_credential(handle, external_id_c.as_ptr()) },
        0,
        "revoke external credential failed: {:?}",
        last_err()
    );
    let mut identity_out4 = core::ptr::null_mut();
    let identity_json4 = unsafe {
        ok_out(
            loom_identity_list_json(handle, &mut identity_out4),
            identity_out4,
        )
    };
    assert!(!identity_json4.contains(&external_id), "{identity_json4}");

    let key_label = cs("authority-laptop");
    let key_algorithm = cs("ES256");
    let key_hex = cs(
        "046b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c296\
         4fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5",
    );
    let mut key_out = core::ptr::null_mut();
    let key_id = unsafe {
        ok_out(
            loom_identity_add_public_key(
                handle,
                user_c.as_ptr(),
                key_label.as_ptr(),
                key_algorithm.as_ptr(),
                key_hex.as_ptr(),
                &mut key_out,
            ),
            key_out,
        )
    };
    assert!(key_id.contains('-'));
    let mut identity_out5 = core::ptr::null_mut();
    let identity_json5 = unsafe {
        ok_out(
            loom_identity_list_json(handle, &mut identity_out5),
            identity_out5,
        )
    };
    assert!(identity_json5.contains(&key_id), "{identity_json5}");
    assert!(
        identity_json5.contains("\"label\":\"authority-laptop\""),
        "{identity_json5}"
    );
    let key_id_c = cs(&key_id);
    assert_eq!(
        unsafe { loom_identity_revoke_public_key(handle, key_id_c.as_ptr()) },
        0,
        "revoke public key failed: {:?}",
        last_err()
    );
    let mut identity_out6 = core::ptr::null_mut();
    let identity_json6 = unsafe {
        ok_out(
            loom_identity_list_json(handle, &mut identity_out6),
            identity_out6,
        )
    };
    assert!(!identity_json6.contains(&key_id), "{identity_json6}");

    let ns = cs("");
    let facet = cs("");
    assert_eq!(
        unsafe {
            loom_acl_grant(
                handle,
                0,
                user_c.as_ptr(),
                ns.as_ptr(),
                facet.as_ptr(),
                0x03,
            )
        },
        0,
        "grant failed: {:?}",
        last_err()
    );

    let mut acl_out2 = core::ptr::null_mut();
    let acl_json2 = unsafe { ok_out(loom_acl_list_json(handle, &mut acl_out2), acl_out2) };
    assert!(acl_json2.contains(&user_id), "{acl_json2}");
    assert!(acl_json2.contains("\"read\"") && acl_json2.contains("\"write\""));
    let role_subject = cs(&format!("role:{admin_role}"));
    assert_eq!(
        unsafe {
            loom_acl_grant(
                handle,
                0,
                role_subject.as_ptr(),
                ns.as_ptr(),
                facet.as_ptr(),
                0x01,
            )
        },
        0,
        "role grant failed: {:?}",
        last_err()
    );
    let mut acl_out3 = core::ptr::null_mut();
    let acl_json3 = unsafe { ok_out(loom_acl_list_json(handle, &mut acl_out3), acl_out3) };
    assert!(
        acl_json3.contains("\"subject_kind\":\"role\""),
        "{acl_json3}"
    );
    let kv_facet = cs("kv");
    let ref_glob = cs("branch/main");
    let scope_kinds = [3i32, 3i32];
    let scope_a = b"tenant/a/";
    let scope_b = b"tenant/b/";
    let scope_prefixes = [scope_a.as_ptr(), scope_b.as_ptr()];
    let scope_lens = [scope_a.len(), scope_b.len()];
    assert_eq!(
        unsafe {
            loom_acl_grant_scoped(
                handle,
                0,
                user_c.as_ptr(),
                ns.as_ptr(),
                kv_facet.as_ptr(),
                0x03,
                ref_glob.as_ptr(),
                scope_kinds.len(),
                scope_kinds.as_ptr(),
                scope_prefixes.as_ptr(),
                scope_lens.as_ptr(),
            )
        },
        0,
        "scoped grant failed: {:?}",
        last_err()
    );
    let mut acl_out4 = core::ptr::null_mut();
    let acl_json4 = unsafe { ok_out(loom_acl_list_json(handle, &mut acl_out4), acl_out4) };
    assert!(
        acl_json4.contains("\"ref_glob\":\"branch/main\""),
        "{acl_json4}"
    );
    assert!(
        acl_json4.contains("\"prefix_hex\":\"74656e616e742f612f\""),
        "{acl_json4}"
    );
    let files_facet = cs("files");
    let path_kind = [2i32];
    let path_prefix = b"reports/";
    let path_prefixes = [path_prefix.as_ptr()];
    let path_lens = [path_prefix.len()];
    let predicate_language = cs("cel");
    let predicate_expression = cs("principal == 'alice'");
    assert_eq!(
        unsafe {
            loom_acl_grant_scoped_predicate(
                handle,
                0,
                user_c.as_ptr(),
                ns.as_ptr(),
                files_facet.as_ptr(),
                0x01,
                ref_glob.as_ptr(),
                path_kind.len(),
                path_kind.as_ptr(),
                path_prefixes.as_ptr(),
                path_lens.as_ptr(),
                predicate_language.as_ptr(),
                predicate_expression.as_ptr(),
            )
        },
        0,
        "predicate grant failed: {:?}",
        last_err()
    );
    let mut acl_out5 = core::ptr::null_mut();
    let acl_json5 = unsafe { ok_out(loom_acl_list_json(handle, &mut acl_out5), acl_out5) };
    assert!(acl_json5.contains("\"language\":\"cel\""), "{acl_json5}");
    assert!(
        acl_json5.contains("\"expression\":\"principal == 'alice'\""),
        "{acl_json5}"
    );

    let mut removed = 0;
    assert_eq!(
        unsafe {
            loom_acl_revoke(
                handle,
                0,
                user_c.as_ptr(),
                ns.as_ptr(),
                facet.as_ptr(),
                0x03,
                &mut removed,
            )
        },
        0,
        "revoke failed: {:?}",
        last_err()
    );
    assert_eq!(removed, 1);
    let mut predicate_removed = 0;
    assert_eq!(
        unsafe {
            loom_acl_revoke_scoped_predicate(
                handle,
                0,
                user_c.as_ptr(),
                ns.as_ptr(),
                files_facet.as_ptr(),
                0x01,
                ref_glob.as_ptr(),
                path_kind.len(),
                path_kind.as_ptr(),
                path_prefixes.as_ptr(),
                path_lens.as_ptr(),
                predicate_language.as_ptr(),
                predicate_expression.as_ptr(),
                &mut predicate_removed,
            )
        },
        0,
        "predicate revoke failed: {:?}",
        last_err()
    );
    assert_eq!(predicate_removed, 1);
    let mut scoped_removed = 0;
    assert_eq!(
        unsafe {
            loom_acl_revoke_scoped(
                handle,
                0,
                user_c.as_ptr(),
                ns.as_ptr(),
                kv_facet.as_ptr(),
                0x03,
                ref_glob.as_ptr(),
                scope_kinds.len(),
                scope_kinds.as_ptr(),
                scope_prefixes.as_ptr(),
                scope_lens.as_ptr(),
                &mut scoped_removed,
            )
        },
        0,
        "scoped revoke failed: {:?}",
        last_err()
    );
    assert_eq!(scoped_removed, 1);
    let mut ns_out = core::ptr::null_mut();
    let policy_ns = unsafe {
        ok_out(
            loom_workspace_create(handle, core::ptr::null(), core::ptr::null(), &mut ns_out),
            ns_out,
        )
    };
    let policy_ns_c = cs(&policy_ns);
    let protected_ref = cs("branch/main");
    assert_eq!(
        unsafe {
            loom_protected_ref_set(
                handle,
                policy_ns_c.as_ptr(),
                protected_ref.as_ptr(),
                true,
                false,
                false,
                0,
                true,
                false,
            )
        },
        0,
        "protected-ref set failed: {:?}",
        last_err()
    );
    let mut protected_out = core::ptr::null_mut();
    let protected_json = unsafe {
        ok_out(
            loom_protected_ref_get_json(
                handle,
                policy_ns_c.as_ptr(),
                protected_ref.as_ptr(),
                &mut protected_out,
            ),
            protected_out,
        )
    };
    assert!(
        protected_json.contains("\"fast_forward_only\":true"),
        "{protected_json}"
    );
    assert!(
        protected_json.contains("\"retention_lock\":true"),
        "{protected_json}"
    );
    let mut protected_list_out = core::ptr::null_mut();
    let protected_list_json = unsafe {
        ok_out(
            loom_protected_ref_list_json(handle, policy_ns_c.as_ptr(), &mut protected_list_out),
            protected_list_out,
        )
    };
    assert!(
        protected_list_json.contains("\"ref\":\"branch/main\""),
        "{protected_list_json}"
    );
    let mut protected_removed = 0;
    assert_eq!(
        unsafe {
            loom_protected_ref_remove(
                handle,
                policy_ns_c.as_ptr(),
                protected_ref.as_ptr(),
                &mut protected_removed,
            )
        },
        0,
        "protected-ref remove failed: {:?}",
        last_err()
    );
    assert_eq!(protected_removed, 1);
    let mut role_removed = 0;
    assert_eq!(
        unsafe {
            loom_identity_revoke_role(
                handle,
                user_c.as_ptr(),
                admin_role_c.as_ptr(),
                &mut role_removed,
            )
        },
        0,
        "revoke role failed: {:?}",
        last_err()
    );
    assert_eq!(role_removed, 1);

    unsafe { loom_close(handle) };
    let store = FileStore::open(&dir).unwrap();
    let audit = store.audit_records().unwrap();
    let actions: Vec<&str> = audit.iter().map(|record| record.action.as_str()).collect();
    assert_eq!(
        actions,
        vec![
            "identity.set_passphrase",
            "identity.add_principal",
            "identity.assign_role",
            "identity.external_credential.create",
            "identity.external_credential.revoke",
            "identity.public_key.add",
            "identity.public_key.revoke",
            "acl.grant",
            "acl.grant",
            "acl.grant",
            "acl.grant",
            "acl.revoke",
            "acl.revoke",
            "acl.revoke",
            "protected_ref.set",
            "protected_ref.remove",
            "identity.revoke_role",
        ]
    );
    assert!(
        audit
            .iter()
            .all(|record| record.principal == Some(WorkspaceId::parse(&root).unwrap()))
    );
    assert_eq!(audit[1].target.as_deref(), Some(user_id.as_str()));
    let role_target = format!("principal={user_id};role={admin_role}");
    assert_eq!(audit[2].target.as_deref(), Some(role_target.as_str()));
    for window in audit.windows(2) {
        assert_eq!(window[1].prev_hash, Some(window[0].hash));
    }
    let _ = std::fs::remove_file(&dir);
}

#[test]
fn handle_session_authenticates_persisted_acl_state() {
    let dir = temp_loom();
    let path = cs(dir.to_str().unwrap());
    let dflt = cs("default");
    assert_eq!(
        unsafe {
            loom_create(
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

    let root = WorkspaceId::from_bytes([7; 16]);
    {
        let fs = FileStore::open(dir.to_str().unwrap()).unwrap();
        let mut identity = loom_core::IdentityStore::new(root);
        identity
            .set_passphrase(root, "root-pass", b"12345678")
            .unwrap();
        fs.save_identity_store(&identity).unwrap();
        let mut acl = loom_core::AclStore::new();
        acl.allow(
            loom_core::AclSubject::Principal(root),
            None,
            Some(FacetKind::Cas),
            [loom_core::AclRight::Read, loom_core::AclRight::Write],
        )
        .unwrap();
        fs.save_acl_store(&acl).unwrap();
    }

    let mut handle: *mut LoomSession = core::ptr::null_mut();
    assert_eq!(unsafe { loom_open(path.as_ptr(), &mut handle) }, 0);
    let ns = cs("blobs");
    let content = b"session cas";
    let mut denied_out = core::ptr::null_mut();
    let denied = unsafe {
        loom_cas_put(
            handle,
            ns.as_ptr(),
            content.as_ptr(),
            content.len(),
            &mut denied_out,
        )
    };
    assert_eq!(
        denied,
        Code::AuthenticationFailed.as_i32(),
        "unauthenticated call failed with {:?}",
        last_err()
    );
    assert!(denied_out.is_null());

    let principal = cs(&root.to_string());
    let passphrase = b"root-pass";
    assert_eq!(
        unsafe {
            loom_authenticate_passphrase(
                handle,
                principal.as_ptr(),
                passphrase.as_ptr(),
                passphrase.len(),
            )
        },
        0,
        "session auth failed: {:?}",
        last_err()
    );
    let mut digest_out = core::ptr::null_mut();
    let digest = unsafe {
        ok_out(
            loom_cas_put(
                handle,
                ns.as_ptr(),
                content.as_ptr(),
                content.len(),
                &mut digest_out,
            ),
            digest_out,
        )
    };
    assert!(digest.starts_with("blake3:"), "{digest}");

    assert_eq!(unsafe { loom_clear_authentication(handle) }, 0);
    let mut list = core::ptr::null_mut();
    let st = unsafe { loom_cas_list_json(handle, ns.as_ptr(), &mut list) };
    assert_eq!(st, Code::AuthenticationFailed.as_i32());
    assert!(list.is_null());

    unsafe { loom_close(handle) };
    let _ = std::fs::remove_file(&dir);
}

#[test]
fn sql_session_authenticates_persisted_acl_state() {
    let dir = temp_loom();
    let path = cs(dir.to_str().unwrap());
    let dflt = cs("default");
    assert_eq!(
        unsafe {
            loom_create(
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

    let ns = cs("app");
    let db = cs("main");
    let mut bootstrap: *mut LoomSqlSession = core::ptr::null_mut();
    assert_eq!(
        unsafe { loom_sql_open(path.as_ptr(), ns.as_ptr(), db.as_ptr(), &mut bootstrap) },
        0,
        "bootstrap sql open failed: {:?}",
        last_err()
    );
    let setup =
        cs("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT); INSERT INTO t VALUES (1, 'a')");
    let mut setup_ptr = core::ptr::null_mut();
    let mut setup_len = 0usize;
    assert_eq!(
        unsafe { loom_sql_exec(bootstrap, setup.as_ptr(), &mut setup_ptr, &mut setup_len) },
        0,
        "bootstrap sql exec failed: {:?}",
        last_err()
    );
    unsafe {
        loom_bytes_free(setup_ptr, setup_len);
        loom_sql_close(bootstrap);
    }

    let root = WorkspaceId::from_bytes([8; 16]);
    {
        let fs = FileStore::open(dir.to_str().unwrap()).unwrap();
        let mut identity = loom_core::IdentityStore::new(root);
        identity
            .set_passphrase(root, "root-pass", b"12345678")
            .unwrap();
        fs.save_identity_store(&identity).unwrap();
        let mut acl = loom_core::AclStore::new();
        acl.allow(
            loom_core::AclSubject::Principal(root),
            None,
            Some(FacetKind::Sql),
            [loom_core::AclRight::Read],
        )
        .unwrap();
        fs.save_acl_store(&acl).unwrap();
    }

    let mut denied: *mut LoomSqlSession = core::ptr::null_mut();
    let st = unsafe { loom_sql_open(path.as_ptr(), ns.as_ptr(), db.as_ptr(), &mut denied) };
    assert_eq!(st, 0, "{:?}", last_err());
    let select = cs("SELECT id, v FROM t ORDER BY id");
    let mut denied_iter: *mut LoomIter = core::ptr::null_mut();
    let st = unsafe { loom_sql_query(denied, select.as_ptr(), &mut denied_iter) };
    assert_eq!(st, Code::AuthenticationFailed.as_i32(), "{:?}", last_err());
    assert!(denied_iter.is_null());
    unsafe { loom_sql_close(denied) };

    let principal = cs(&root.to_string());
    let root_pass = b"root-pass";
    let mut read_session: *mut LoomSqlSession = core::ptr::null_mut();
    assert_eq!(
        unsafe {
            loom_sql_open_authenticated(
                path.as_ptr(),
                ns.as_ptr(),
                db.as_ptr(),
                principal.as_ptr(),
                root_pass.as_ptr(),
                root_pass.len(),
                &mut read_session,
            )
        },
        0,
        "authenticated sql open failed: {:?}",
        last_err()
    );
    let mut iter: *mut LoomIter = core::ptr::null_mut();
    assert_eq!(
        unsafe { loom_sql_query(read_session, select.as_ptr(), &mut iter) },
        0,
        "authenticated query failed: {:?}",
        last_err()
    );
    unsafe { loom_iter_free(iter) };
    let insert = cs("INSERT INTO t VALUES (2, 'b')");
    let mut denied_ptr = core::ptr::null_mut();
    let mut denied_len = 0usize;
    let st = unsafe {
        loom_sql_exec(
            read_session,
            insert.as_ptr(),
            &mut denied_ptr,
            &mut denied_len,
        )
    };
    assert_eq!(st, Code::PermissionDenied.as_i32(), "{:?}", last_err());
    assert!(denied_ptr.is_null());
    unsafe { loom_sql_close(read_session) };

    {
        let fs = FileStore::open(dir.to_str().unwrap()).unwrap();
        let mut acl = fs.acl_store().unwrap().unwrap();
        acl.allow(
            loom_core::AclSubject::Principal(root),
            None,
            Some(FacetKind::Sql),
            [loom_core::AclRight::Write],
        )
        .unwrap();
        fs.save_acl_store(&acl).unwrap();
    }

    let mut write_session: *mut LoomSqlSession = core::ptr::null_mut();
    assert_eq!(
        unsafe {
            loom_sql_open_authenticated(
                path.as_ptr(),
                ns.as_ptr(),
                db.as_ptr(),
                principal.as_ptr(),
                root_pass.as_ptr(),
                root_pass.len(),
                &mut write_session,
            )
        },
        0,
        "authenticated write sql open failed: {:?}",
        last_err()
    );
    let mut insert_ptr = core::ptr::null_mut();
    let mut insert_len = 0usize;
    assert_eq!(
        unsafe {
            loom_sql_exec(
                write_session,
                insert.as_ptr(),
                &mut insert_ptr,
                &mut insert_len,
            )
        },
        0,
        "authenticated exec failed: {:?}",
        last_err()
    );
    unsafe {
        loom_bytes_free(insert_ptr, insert_len);
        loom_sql_close(write_session);
    }

    let mut batch: *mut LoomSqlBatch = core::ptr::null_mut();
    assert_eq!(
        unsafe {
            loom_sql_batch_begin_authenticated(
                path.as_ptr(),
                ns.as_ptr(),
                db.as_ptr(),
                principal.as_ptr(),
                root_pass.as_ptr(),
                root_pass.len(),
                &mut batch,
            )
        },
        0,
        "authenticated batch begin failed: {:?}",
        last_err()
    );
    unsafe {
        batch_exec_ok(batch, "INSERT INTO t VALUES (3, 'c')");
        assert_eq!(loom_sql_batch_commit(batch), 0, "{:?}", last_err());
        loom_sql_batch_close(batch);
    }
    let _ = std::fs::remove_file(&dir);
}

#[test]
fn kv_config_over_the_c_abi_persists() {
    let dir = temp_loom();
    let path = cs(dir.to_str().unwrap());
    let dflt = cs("default");
    assert_eq!(
        unsafe {
            loom_create(
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
    assert_eq!(unsafe { loom_open(path.as_ptr(), &mut handle) }, 0);

    let ns = cs("kv");
    let map = cs("cache");
    assert_eq!(
        unsafe {
            loom_management_kv_set_config(
                handle,
                ns.as_ptr(),
                map.as_ptr(),
                1,   // tier: ephemeral
                100, // default_ttl_ms
                25,  // default_idle_ttl_ms
                1,   // read_through
                1,   // write_through
                10,  // max_entries
                0,   // max_bytes (unbounded)
                1,   // eviction: lru
                0,   // on_evict: drop
                1,   // write_behind
                0,   // write_around
                1,   // back_pressure: pressure
                80,  // flush_high_water_pct
                4,   // flush_batch
            )
        },
        0,
        "configure failed: {:?}",
        last_err()
    );
    let mut out = core::ptr::null_mut();
    let json = unsafe {
        ok_out(
            loom_management_kv_get_config_json(handle, ns.as_ptr(), map.as_ptr(), &mut out),
            out,
        )
    };
    assert!(json.contains("\"tier\":\"ephemeral\""), "{json}");
    assert!(json.contains("\"default_ttl_ms\":100"), "{json}");
    assert!(json.contains("\"default_idle_ttl_ms\":25"), "{json}");
    assert!(json.contains("\"read_through\":true"), "{json}");
    assert!(json.contains("\"write_through\":true"), "{json}");
    assert!(json.contains("\"max_entries\":10"), "{json}");
    assert!(json.contains("\"eviction\":\"lru\""), "{json}");
    assert!(json.contains("\"write_behind\":true"), "{json}");
    assert!(json.contains("\"back_pressure\":\"pressure\""), "{json}");
    assert!(json.contains("\"flush_high_water_pct\":80"), "{json}");
    assert!(json.contains("\"flush_batch\":4"), "{json}");
    unsafe { loom_close(handle) };

    let mut reopened: *mut LoomSession = core::ptr::null_mut();
    assert_eq!(unsafe { loom_open(path.as_ptr(), &mut reopened) }, 0);
    let mut out = core::ptr::null_mut();
    let json = unsafe {
        ok_out(
            loom_management_kv_get_config_json(reopened, ns.as_ptr(), map.as_ptr(), &mut out),
            out,
        )
    };
    assert!(json.contains("\"tier\":\"ephemeral\""), "{json}");
    assert_eq!(
        unsafe {
            loom_management_kv_set_config(
                reopened,
                ns.as_ptr(),
                map.as_ptr(),
                0,
                0,
                0,
                0,
                0, // versioned, no ttls/flags
                0,
                0, // max_entries, max_bytes
                0,
                0, // eviction: none, on_evict: drop
                0,
                0,  // write_behind, write_around
                0,  // back_pressure: block
                -1, // flush_high_water_pct: absent
                0,  // flush_batch
            )
        },
        0,
        "reset failed: {:?}",
        last_err()
    );
    let mut out = core::ptr::null_mut();
    let json = unsafe {
        ok_out(
            loom_management_kv_get_config_json(reopened, ns.as_ptr(), map.as_ptr(), &mut out),
            out,
        )
    };
    assert!(json.contains("\"tier\":\"versioned\""), "{json}");
    assert!(json.contains("\"default_ttl_ms\":null"), "{json}");

    unsafe { loom_close(reopened) };
    let _ = std::fs::remove_file(&dir);
}

#[test]
fn kv_round_trip_over_the_c_abi() {
    let dir = temp_loom();
    let path = cs(dir.to_str().unwrap());
    let dflt = cs("default");
    assert_eq!(
        unsafe {
            loom_create(
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
    assert_eq!(unsafe { loom_open(path.as_ptr(), &mut handle) }, 0);

    let ns = cs("kv");
    let map = cs("main");
    let k1 = loom_core::key_to_cbor(&Value::Int(1));
    let k2 = loom_core::key_to_cbor(&Value::Int(2));

    // Put by workspace name: the kv facet is created on first write.
    for (k, v) in [(&k1, &b"one"[..]), (&k2, &b"two"[..])] {
        let st = unsafe {
            loom_kv_put(
                handle,
                ns.as_ptr(),
                map.as_ptr(),
                k.as_ptr(),
                k.len(),
                v.as_ptr(),
                v.len(),
            )
        };
        assert_eq!(st, 0, "put failed: {:?}", last_err());
    }

    // Get round trip: found, with the exact bytes back.
    let (mut p, mut n, mut found) = (core::ptr::null_mut(), 0usize, -1i32);
    let st = unsafe {
        loom_kv_get(
            handle,
            ns.as_ptr(),
            map.as_ptr(),
            k1.as_ptr(),
            k1.len(),
            &mut p,
            &mut n,
            &mut found,
        )
    };
    assert_eq!(st, 0, "get failed: {:?}", last_err());
    assert_eq!(found, 1, "a stored key must be found");
    // SAFETY: on `found`, `(p, n)` is a live buffer this library returned.
    let got = unsafe { std::slice::from_raw_parts(p, n) }.to_vec();
    unsafe { loom_bytes_free(p, n) };
    assert_eq!(got, b"one");

    // List returns the canonical-CBOR pair array; decode and count.
    let mut lp = core::ptr::null_mut();
    let mut ln = 0usize;
    let st = unsafe { loom_kv_list_cbor(handle, ns.as_ptr(), map.as_ptr(), &mut lp, &mut ln) };
    assert_eq!(st, 0, "list failed: {:?}", last_err());
    let list_bytes = unsafe { std::slice::from_raw_parts(lp, ln) }.to_vec();
    unsafe { loom_bytes_free(lp, ln) };
    assert_eq!(
        loom_core::KvMap::decode(&list_bytes).unwrap().len(),
        2,
        "list must hold both keys"
    );

    // Range [1, 2) returns only key 1.
    let mut rp = core::ptr::null_mut();
    let mut rn = 0usize;
    let st = unsafe {
        loom_kv_range_cbor(
            handle,
            ns.as_ptr(),
            map.as_ptr(),
            k1.as_ptr(),
            k1.len(),
            k2.as_ptr(),
            k2.len(),
            &mut rp,
            &mut rn,
        )
    };
    assert_eq!(st, 0, "range failed: {:?}", last_err());
    let range_bytes = unsafe { std::slice::from_raw_parts(rp, rn) }.to_vec();
    unsafe { loom_bytes_free(rp, rn) };
    assert_eq!(
        loom_core::KvMap::decode(&range_bytes).unwrap().len(),
        1,
        "half-open range [1,2) holds only key 1"
    );

    // Delete reports presence, then is a no-op.
    let mut d1 = -1i32;
    assert_eq!(
        unsafe {
            loom_kv_delete(
                handle,
                ns.as_ptr(),
                map.as_ptr(),
                k1.as_ptr(),
                k1.len(),
                &mut d1,
            )
        },
        0
    );
    assert_eq!(d1, 1, "deleting a present key reports 1");
    let mut d2 = -1i32;
    assert_eq!(
        unsafe {
            loom_kv_delete(
                handle,
                ns.as_ptr(),
                map.as_ptr(),
                k1.as_ptr(),
                k1.len(),
                &mut d2,
            )
        },
        0
    );
    assert_eq!(d2, 0, "deleting an absent key reports 0");

    // Absent get after delete: found = 0, no bytes.
    let (mut p3, mut n3, mut found3) = (core::ptr::null_mut(), 0usize, -1i32);
    let st = unsafe {
        loom_kv_get(
            handle,
            ns.as_ptr(),
            map.as_ptr(),
            k1.as_ptr(),
            k1.len(),
            &mut p3,
            &mut n3,
            &mut found3,
        )
    };
    assert_eq!(st, 0, "absent get must succeed: {:?}", last_err());
    assert_eq!(found3, 0, "a deleted key must be absent");
    assert!(p3.is_null() && n3 == 0, "an absent get must write no bytes");

    unsafe { loom_close(handle) };
    let _ = std::fs::remove_file(&dir);
}

#[test]
fn document_timeseries_ledger_round_trip_over_the_c_abi() {
    let dir = temp_loom();
    let path = cs(dir.to_str().unwrap());
    let dflt = cs("default");
    assert_eq!(
        unsafe {
            loom_create(
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
    assert_eq!(unsafe { loom_open(path.as_ptr(), &mut handle) }, 0);

    // --- Document ---
    let dns = cs("docs");
    let coll = cs("people");
    let id = cs("u1");
    let docv = b"{\"n\":1}";
    let mut digest: *mut c_char = core::ptr::null_mut();
    let mut entity_tag: *mut c_char = core::ptr::null_mut();
    assert_eq!(
        unsafe {
            loom_doc_put_binary(
                handle,
                dns.as_ptr(),
                coll.as_ptr(),
                id.as_ptr(),
                docv.as_ptr(),
                docv.len(),
                core::ptr::null(),
                &mut digest,
                &mut entity_tag,
            )
        },
        0,
        "doc_put_binary: {:?}",
        last_err()
    );
    unsafe { loom_string_free(digest) };
    unsafe { loom_string_free(entity_tag) };
    let (mut p, mut n, mut got_digest, mut got_entity_tag, mut found) = (
        core::ptr::null_mut(),
        0usize,
        core::ptr::null_mut(),
        core::ptr::null_mut(),
        -1i32,
    );
    assert_eq!(
        unsafe {
            loom_doc_get_binary(
                handle,
                dns.as_ptr(),
                coll.as_ptr(),
                id.as_ptr(),
                &mut p,
                &mut n,
                &mut got_digest,
                &mut got_entity_tag,
                &mut found,
            )
        },
        0
    );
    assert_eq!(found, 1, "doc must be found");
    let got = unsafe { std::slice::from_raw_parts(p, n) }.to_vec();
    unsafe { loom_bytes_free(p, n) };
    unsafe { loom_string_free(got_digest) };
    unsafe { loom_string_free(got_entity_tag) };
    assert_eq!(got, docv);
    let mut dfound = -1i32;
    assert_eq!(
        unsafe {
            loom_doc_delete(
                handle,
                dns.as_ptr(),
                coll.as_ptr(),
                id.as_ptr(),
                &mut dfound,
            )
        },
        0
    );
    assert_eq!(dfound, 1, "deleting a present doc reports 1");

    // --- Time-series ---
    let tns = cs("metrics");
    let series = cs("cpu");
    for (t, v) in [(100i64, &b"a"[..]), (200, &b"b"[..])] {
        assert_eq!(
            unsafe {
                loom_ts_put(
                    handle,
                    tns.as_ptr(),
                    series.as_ptr(),
                    t,
                    v.as_ptr(),
                    v.len(),
                )
            },
            0,
            "ts_put: {:?}",
            last_err()
        );
    }
    let (mut tp, mut tn, mut tfound, mut ts_out) = (core::ptr::null_mut(), 0usize, -1i32, 0i64);
    assert_eq!(
        unsafe {
            loom_ts_latest(
                handle,
                tns.as_ptr(),
                series.as_ptr(),
                &mut ts_out,
                &mut tp,
                &mut tn,
                &mut tfound,
            )
        },
        0
    );
    assert_eq!(tfound, 1);
    assert_eq!(ts_out, 200, "latest timestamp is 200");
    let tv = unsafe { std::slice::from_raw_parts(tp, tn) }.to_vec();
    unsafe { loom_bytes_free(tp, tn) };
    assert_eq!(tv, b"b");

    // --- Ledger ---
    let lns = cs("audit");
    let log = cs("log");
    let e0 = b"e0";
    let mut seq = u64::MAX;
    assert_eq!(
        unsafe {
            loom_ledger_append(
                handle,
                lns.as_ptr(),
                log.as_ptr(),
                e0.as_ptr(),
                e0.len(),
                &mut seq,
            )
        },
        0,
        "ledger_append: {:?}",
        last_err()
    );
    assert_eq!(seq, 0, "first entry is sequence 0");
    let mut llen = u64::MAX;
    assert_eq!(
        unsafe { loom_ledger_len(handle, lns.as_ptr(), log.as_ptr(), &mut llen) },
        0
    );
    assert_eq!(llen, 1);
    let mut hout = core::ptr::null_mut();
    let mut hfound = -1i32;
    assert_eq!(
        unsafe { loom_ledger_head(handle, lns.as_ptr(), log.as_ptr(), &mut hout, &mut hfound) },
        0
    );
    assert_eq!(hfound, 1, "a non-empty ledger has a head");
    let head = unsafe { CStr::from_ptr(hout) }
        .to_str()
        .unwrap()
        .to_string();
    unsafe { loom_string_free(hout) };
    assert!(
        head.starts_with("blake3:"),
        "head is profile-tagged: {head}"
    );
    assert_eq!(
        unsafe { loom_ledger_verify(handle, lns.as_ptr(), log.as_ptr()) },
        0,
        "an intact chain verifies: {:?}",
        last_err()
    );

    unsafe { loom_close(handle) };
    let _ = std::fs::remove_file(&dir);
}

#[test]
fn document_text_and_binary_contract_over_the_c_abi() {
    let dir = temp_loom();
    let path = cs(dir.to_str().unwrap());
    let dflt = cs("default");
    assert_eq!(
        unsafe {
            loom_create(
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
    assert_eq!(unsafe { loom_open(path.as_ptr(), &mut handle) }, 0);

    let workspace = cs("docs-text");
    let collection = cs("notes");
    let id = cs("a");
    let text = cs("hello");
    let mut digest_ptr: *mut c_char = core::ptr::null_mut();
    let mut entity_tag_ptr: *mut c_char = core::ptr::null_mut();
    assert_eq!(
        unsafe {
            loom_doc_put_text(
                handle,
                workspace.as_ptr(),
                collection.as_ptr(),
                id.as_ptr(),
                text.as_ptr(),
                core::ptr::null(),
                &mut digest_ptr,
                &mut entity_tag_ptr,
            )
        },
        0,
        "put_text failed: {:?}",
        last_err()
    );
    let digest = unsafe { ok_out(0, digest_ptr) };
    unsafe { loom_string_free(entity_tag_ptr) };
    assert!(digest.starts_with("blake3:"));

    let mut out_text: *mut c_char = core::ptr::null_mut();
    let mut out_digest: *mut c_char = core::ptr::null_mut();
    let mut out_entity_tag: *mut c_char = core::ptr::null_mut();
    let mut found = -1;
    assert_eq!(
        unsafe {
            loom_doc_get_text(
                handle,
                workspace.as_ptr(),
                collection.as_ptr(),
                id.as_ptr(),
                &mut out_text,
                &mut out_digest,
                &mut out_entity_tag,
                &mut found,
            )
        },
        0,
        "get_text failed: {:?}",
        last_err()
    );
    assert_eq!(found, 1);
    let got_text = unsafe { ok_out(0, out_text) };
    let got_digest = unsafe { ok_out(0, out_digest) };
    unsafe { loom_string_free(out_entity_tag) };
    assert_eq!(got_text, "hello");
    assert_eq!(got_digest, digest);

    let stale = cs("blake3:0000000000000000000000000000000000000000000000000000000000000000");
    let replacement = cs("stale");
    assert_eq!(
        unsafe {
            loom_doc_put_text(
                handle,
                workspace.as_ptr(),
                collection.as_ptr(),
                id.as_ptr(),
                replacement.as_ptr(),
                stale.as_ptr(),
                &mut digest_ptr,
                &mut entity_tag_ptr,
            )
        },
        Code::CasMismatch.as_i32(),
        "stale guard must fail"
    );

    let digest_arg = cs(&digest);
    let replacement = cs("updated");
    assert_eq!(
        unsafe {
            loom_doc_put_text(
                handle,
                workspace.as_ptr(),
                collection.as_ptr(),
                id.as_ptr(),
                replacement.as_ptr(),
                digest_arg.as_ptr(),
                &mut digest_ptr,
                &mut entity_tag_ptr,
            )
        },
        0,
        "guarded put_text failed: {:?}",
        last_err()
    );
    let updated_digest = unsafe { ok_out(0, digest_ptr) };
    unsafe { loom_string_free(entity_tag_ptr) };
    assert_ne!(updated_digest, digest);

    let bin_collection = cs("bin");
    let bin_id = cs("raw");
    let raw = [0xffu8, 0xfe];
    assert_eq!(
        unsafe {
            loom_doc_put_binary(
                handle,
                workspace.as_ptr(),
                bin_collection.as_ptr(),
                bin_id.as_ptr(),
                raw.as_ptr(),
                raw.len(),
                core::ptr::null(),
                &mut digest_ptr,
                &mut entity_tag_ptr,
            )
        },
        0,
        "put_binary failed: {:?}",
        last_err()
    );
    let binary_digest = unsafe { ok_out(0, digest_ptr) };
    unsafe { loom_string_free(entity_tag_ptr) };
    assert!(binary_digest.starts_with("blake3:"));

    let (mut bytes_ptr, mut bytes_len, mut bytes_found) = (core::ptr::null_mut(), 0usize, -1);
    assert_eq!(
        unsafe {
            loom_doc_get_binary(
                handle,
                workspace.as_ptr(),
                bin_collection.as_ptr(),
                bin_id.as_ptr(),
                &mut bytes_ptr,
                &mut bytes_len,
                &mut out_digest,
                &mut out_entity_tag,
                &mut bytes_found,
            )
        },
        0,
        "get_binary failed: {:?}",
        last_err()
    );
    assert_eq!(bytes_found, 1);
    let got = unsafe { std::slice::from_raw_parts(bytes_ptr, bytes_len) }.to_vec();
    unsafe { loom_bytes_free(bytes_ptr, bytes_len) };
    let got_binary_digest = unsafe { ok_out(0, out_digest) };
    unsafe { loom_string_free(out_entity_tag) };
    assert_eq!(got, raw);
    assert_eq!(got_binary_digest, binary_digest);

    assert_eq!(
        unsafe {
            loom_doc_get_text(
                handle,
                workspace.as_ptr(),
                bin_collection.as_ptr(),
                bin_id.as_ptr(),
                &mut out_text,
                &mut out_digest,
                &mut out_entity_tag,
                &mut found,
            )
        },
        Code::DocumentNotText.as_i32(),
        "binary bytes must not project as text"
    );

    unsafe { loom_close(handle) };
}

#[test]
fn lanes_contract_over_the_c_abi() {
    let dir = temp_loom();
    let path = cs(dir.to_str().unwrap());
    let dflt = cs("default");
    assert_eq!(
        unsafe {
            loom_create(
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
    let mut handle = core::ptr::null_mut();
    assert_eq!(unsafe { loom_open(path.as_ptr(), &mut handle) }, 0);

    let workspace = cs("lane-work");
    let lane_id = cs("agent-3");
    let updated_by = cs("agent");
    let lane = loom_lanes::Lane::new(loom_lanes::LaneInput {
        lane_id: "agent-3",
        lane_key: "agent-3",
        title: "",
        description: "",
        lane_kind: loom_lanes::LaneKind::Assignment,
        owner_principal: Some("agent"),
        lane_status: loom_lanes::LaneStatus::Ready,
        lane_tickets: &[],
        active_ticket_id: None,
        status_report: "",
        reviewer_feedback: "",
        updated_at: 1,
        updated_by: "agent",
    })
    .unwrap();
    let lane_bytes = lane.encode().unwrap();
    let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
    let created = unsafe {
        ok_raw(
            loom_lanes_create_cbor(
                handle,
                workspace.as_ptr(),
                lane_bytes.as_ptr(),
                lane_bytes.len(),
                &mut p,
                &mut n,
            ),
            p,
            n,
        )
    };
    assert_eq!(
        loom_lanes::Lane::decode(&created).unwrap().lane_id,
        "agent-3"
    );

    let ticket_id = cs("MX-110");
    let added = unsafe {
        ok_raw(
            loom_lanes_ticket_add_cbor(
                handle,
                workspace.as_ptr(),
                lane_id.as_ptr(),
                ticket_id.as_ptr(),
                updated_by.as_ptr(),
                core::ptr::null(),
                core::ptr::null(),
                &mut p,
                &mut n,
            ),
            p,
            n,
        )
    };
    let added = loom_lanes::Lane::decode(&added).unwrap();
    assert_eq!(added.lane_tickets[0].ticket_id, "MX-110");

    let report = cs("running focused ABI checks");
    let feedback = cs("review available");
    let updated = unsafe {
        ok_raw(
            loom_lanes_update_cbor(
                handle,
                workspace.as_ptr(),
                lane_id.as_ptr(),
                core::ptr::null(),
                core::ptr::null(),
                core::ptr::null(),
                report.as_ptr(),
                feedback.as_ptr(),
                updated_by.as_ptr(),
                &mut p,
                &mut n,
            ),
            p,
            n,
        )
    };
    let updated = loom_lanes::Lane::decode(&updated).unwrap();
    assert_eq!(updated.status_report, "running focused ABI checks");
    assert_eq!(updated.reviewer_feedback, "review available");

    let ticket_two = cs("MX-111");
    let first = cs("first");
    let added_first = unsafe {
        ok_raw(
            loom_lanes_ticket_add_cbor(
                handle,
                workspace.as_ptr(),
                lane_id.as_ptr(),
                ticket_two.as_ptr(),
                updated_by.as_ptr(),
                first.as_ptr(),
                core::ptr::null(),
                &mut p,
                &mut n,
            ),
            p,
            n,
        )
    };
    assert_eq!(
        loom_lanes::Lane::decode(&added_first).unwrap().lane_tickets[0].ticket_id,
        "MX-111"
    );

    let mut found = -1i32;
    let fetched = unsafe {
        ok_raw(
            loom_lanes_get_cbor(
                handle,
                workspace.as_ptr(),
                lane_id.as_ptr(),
                &mut p,
                &mut n,
                &mut found,
            ),
            p,
            n,
        )
    };
    assert_eq!(found, 1);
    assert_eq!(
        loom_lanes::Lane::decode(&fetched).unwrap().lane_id,
        "agent-3"
    );

    let listed = unsafe {
        ok_raw(
            loom_lanes_list_cbor(handle, workspace.as_ptr(), &mut p, &mut n),
            p,
            n,
        )
    };
    let CborValue::Array(lanes) = loom_codec::decode(&listed).unwrap() else {
        panic!("lane list must be an array");
    };
    assert_eq!(lanes.len(), 1);

    let removed = unsafe {
        ok_raw(
            loom_lanes_ticket_remove_cbor(
                handle,
                workspace.as_ptr(),
                lane_id.as_ptr(),
                ticket_id.as_ptr(),
                updated_by.as_ptr(),
                &mut p,
                &mut n,
            ),
            p,
            n,
        )
    };
    let removed = loom_lanes::Lane::decode(&removed).unwrap();
    assert!(removed.active_ticket_id.is_none());
    assert_eq!(removed.lane_tickets.len(), 1);

    unsafe { loom_close(handle) };
    let _ = std::fs::remove_file(&dir);
}

#[test]
fn lane_view_json_over_the_c_abi_is_compact_by_default_and_detailed_on_flag() {
    let dir = temp_loom();
    let path = cs(dir.to_str().unwrap());
    let dflt = cs("default");
    assert_eq!(
        unsafe {
            loom_create(
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
    assert_eq!(unsafe { loom_open(path.as_ptr(), &mut handle) }, 0);

    let workspace = cs("lane-view-work");
    let ticket_workspace_id = cs("tickets");
    let lane_id = cs("agent-9");
    let updated_by = cs("agent");
    let lane = loom_lanes::Lane::new(loom_lanes::LaneInput {
        lane_id: "agent-9",
        lane_key: "agent-9",
        title: "",
        description: "",
        lane_kind: loom_lanes::LaneKind::Assignment,
        owner_principal: Some("agent"),
        lane_status: loom_lanes::LaneStatus::Ready,
        lane_tickets: &[],
        active_ticket_id: None,
        status_report: "",
        reviewer_feedback: "",
        updated_at: 1,
        updated_by: "agent",
    })
    .unwrap();
    let lane_bytes = lane.encode().unwrap();
    let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
    unsafe {
        ok_raw(
            loom_lanes_create_cbor(
                handle,
                workspace.as_ptr(),
                lane_bytes.as_ptr(),
                lane_bytes.len(),
                &mut p,
                &mut n,
            ),
            p,
            n,
        )
    };
    let ticket_id = cs("MX-500");
    unsafe {
        ok_raw(
            loom_lanes_ticket_add_cbor(
                handle,
                workspace.as_ptr(),
                lane_id.as_ptr(),
                ticket_id.as_ptr(),
                updated_by.as_ptr(),
                core::ptr::null(),
                core::ptr::null(),
                &mut p,
                &mut n,
            ),
            p,
            n,
        )
    };

    // Compact list: label, derived display status, and ordered ticket ids only.
    let mut out: *mut c_char = core::ptr::null_mut();
    let compact = unsafe {
        ok_out(
            loom_lanes_list_views_json(
                handle,
                workspace.as_ptr(),
                ticket_workspace_id.as_ptr(),
                false,
                &mut out,
            ),
            out,
        )
    };
    assert!(compact.contains("\"agent-9\""), "compact list: {compact}");
    assert!(compact.contains("MX-500"), "compact list ids: {compact}");
    assert!(
        compact.contains("\"display_status\""),
        "compact display status: {compact}"
    );
    assert!(
        !compact.contains("stored_lane_status"),
        "compact omits stored status: {compact}"
    );

    // Detailed get: stored status, owner, and per-ticket summaries.
    out = core::ptr::null_mut();
    let detailed = unsafe {
        ok_out(
            loom_lanes_get_view_json(
                handle,
                workspace.as_ptr(),
                ticket_workspace_id.as_ptr(),
                lane_id.as_ptr(),
                true,
                &mut out,
            ),
            out,
        )
    };
    assert!(
        detailed.contains("\"stored_lane_status\""),
        "detailed stored status: {detailed}"
    );
    assert!(
        detailed.contains("\"owner_principal\":\"agent\""),
        "detailed owner: {detailed}"
    );
    assert!(detailed.contains("MX-500"), "detailed ids: {detailed}");

    // Absent lane reads as the JSON literal null.
    out = core::ptr::null_mut();
    let missing_id = cs("nope");
    let absent = unsafe {
        ok_out(
            loom_lanes_get_view_json(
                handle,
                workspace.as_ptr(),
                ticket_workspace_id.as_ptr(),
                missing_id.as_ptr(),
                false,
                &mut out,
            ),
            out,
        )
    };
    assert_eq!(absent, "null", "absent lane view");

    unsafe { loom_close(handle) };
    let _ = std::fs::remove_file(&dir);
}

#[test]
fn calendar_contacts_mail_round_trip_over_the_c_abi() {
    let dir = temp_loom();
    let path = cs(dir.to_str().unwrap());
    let dflt = cs("default");
    assert_eq!(
        unsafe {
            loom_create(
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
    assert_eq!(unsafe { loom_open(path.as_ptr(), &mut handle) }, 0);

    // --- Calendar ---
    let cns = cs("cal");
    let principal = cs("alice");
    let collection = cs("work");
    let display = cs("Work");
    let comps = cs("event,todo");
    assert_eq!(
        unsafe {
            loom_cal_create_collection(
                handle,
                cns.as_ptr(),
                principal.as_ptr(),
                collection.as_ptr(),
                display.as_ptr(),
                comps.as_ptr(),
            )
        },
        0,
        "cal_create_collection: {:?}",
        last_err()
    );
    // Put an entry as its canonical CBOR.
    let entry = CalendarEntry::event("uid-1", "Standup", "20260101T090000");
    let entry_bytes = entry.encode();
    let bad_entry = [0xffu8];
    let bad_cal = unsafe {
        loom_cal_put_entry(
            handle,
            cns.as_ptr(),
            principal.as_ptr(),
            collection.as_ptr(),
            bad_entry.as_ptr(),
            bad_entry.len(),
        )
    };
    assert_eq!(bad_cal, Code::CorruptObject.as_i32(), "{:?}", last_err());
    assert_eq!(
        unsafe {
            loom_cal_put_entry(
                handle,
                cns.as_ptr(),
                principal.as_ptr(),
                collection.as_ptr(),
                entry_bytes.as_ptr(),
                entry_bytes.len(),
            )
        },
        0,
        "cal_put_entry: {:?}",
        last_err()
    );
    let missing_collection = cs("missing");
    let missing_cal = unsafe {
        loom_cal_put_entry(
            handle,
            cns.as_ptr(),
            principal.as_ptr(),
            missing_collection.as_ptr(),
            entry_bytes.as_ptr(),
            entry_bytes.len(),
        )
    };
    assert_eq!(missing_cal, Code::NotFound.as_i32(), "{:?}", last_err());
    // Get it back, byte-exact, and decodable to the same record.
    let uid = cs("uid-1");
    let (mut p, mut n, mut found) = (core::ptr::null_mut(), 0usize, -1i32);
    assert_eq!(
        unsafe {
            loom_cal_get_entry(
                handle,
                cns.as_ptr(),
                principal.as_ptr(),
                collection.as_ptr(),
                uid.as_ptr(),
                &mut p,
                &mut n,
                &mut found,
            )
        },
        0,
        "cal_get_entry: {:?}",
        last_err()
    );
    assert_eq!(found, 1, "the stored calendar entry must be found");
    let got = unsafe { std::slice::from_raw_parts(p, n) }.to_vec();
    unsafe { loom_bytes_free(p, n) };
    assert_eq!(got, entry_bytes, "calendar entry round-trips byte-exact");
    assert_eq!(CalendarEntry::decode(&got).unwrap(), entry);
    // List returns the canonical-CBOR array of one record byte string.
    let (mut lp, mut ln) = (core::ptr::null_mut(), 0usize);
    assert_eq!(
        unsafe {
            loom_cal_list_entries(
                handle,
                cns.as_ptr(),
                principal.as_ptr(),
                collection.as_ptr(),
                &mut lp,
                &mut ln,
            )
        },
        0,
        "cal_list_entries: {:?}",
        last_err()
    );
    let list_bytes = unsafe { std::slice::from_raw_parts(lp, ln) }.to_vec();
    unsafe { loom_bytes_free(lp, ln) };
    let CborValue::Array(items) = loom_codec::decode(&list_bytes).unwrap() else {
        panic!("list must be a CBOR array");
    };
    assert_eq!(items.len(), 1, "list holds the one entry");
    let CborValue::Bytes(rec) = &items[0] else {
        panic!("list element must be CBOR bytes");
    };
    assert_eq!(CalendarEntry::decode(rec).unwrap(), entry);

    // --- Contacts ---
    let bns = cs("card");
    let book = cs("personal");
    let book_display = cs("Personal");
    assert_eq!(
        unsafe {
            loom_card_create_book(
                handle,
                bns.as_ptr(),
                principal.as_ptr(),
                book.as_ptr(),
                book_display.as_ptr(),
            )
        },
        0,
        "card_create_book: {:?}",
        last_err()
    );
    let contact = ContactEntry::new("c-1", "Bob Jones");
    let contact_bytes = contact.encode();
    assert_eq!(
        unsafe {
            loom_card_put_entry(
                handle,
                bns.as_ptr(),
                principal.as_ptr(),
                book.as_ptr(),
                contact_bytes.as_ptr(),
                contact_bytes.len(),
            )
        },
        0,
        "card_put_entry: {:?}",
        last_err()
    );
    let missing_book = cs("missing");
    let missing_card = unsafe {
        loom_card_put_entry(
            handle,
            bns.as_ptr(),
            principal.as_ptr(),
            missing_book.as_ptr(),
            contact_bytes.as_ptr(),
            contact_bytes.len(),
        )
    };
    assert_eq!(missing_card, Code::NotFound.as_i32(), "{:?}", last_err());
    let cuid = cs("c-1");
    let (mut cp, mut cn, mut cfound) = (core::ptr::null_mut(), 0usize, -1i32);
    assert_eq!(
        unsafe {
            loom_card_get_entry(
                handle,
                bns.as_ptr(),
                principal.as_ptr(),
                book.as_ptr(),
                cuid.as_ptr(),
                &mut cp,
                &mut cn,
                &mut cfound,
            )
        },
        0,
        "card_get_entry: {:?}",
        last_err()
    );
    assert_eq!(cfound, 1, "the stored contact must be found");
    let cgot = unsafe { std::slice::from_raw_parts(cp, cn) }.to_vec();
    unsafe { loom_bytes_free(cp, cn) };
    assert_eq!(ContactEntry::decode(&cgot).unwrap(), contact);

    // --- Mail ---
    let mns = cs("mail");
    let mailbox = cs("inbox");
    let mb_display = cs("Inbox");
    assert_eq!(
        unsafe {
            loom_mail_create_mailbox(
                handle,
                mns.as_ptr(),
                principal.as_ptr(),
                mailbox.as_ptr(),
                mb_display.as_ptr(),
            )
        },
        0,
        "mail_create_mailbox: {:?}",
        last_err()
    );
    let raw =
        b"From: bob@example.com\r\nTo: alice@example.com\r\nSubject: Hi\r\n\r\nHello there.\r\n";
    let muid = cs("m-1");
    let missing_mailbox = cs("missing");
    let mut missing_addr_out = core::ptr::null_mut();
    let missing_mail = unsafe {
        loom_mail_ingest_message(
            handle,
            mns.as_ptr(),
            principal.as_ptr(),
            missing_mailbox.as_ptr(),
            muid.as_ptr(),
            raw.as_ptr(),
            raw.len(),
            &mut missing_addr_out,
        )
    };
    assert_eq!(missing_mail, Code::NotFound.as_i32(), "{:?}", last_err());
    assert!(missing_addr_out.is_null());
    let mut addr_out = core::ptr::null_mut();
    let addr = unsafe {
        ok_out(
            loom_mail_ingest_message(
                handle,
                mns.as_ptr(),
                principal.as_ptr(),
                mailbox.as_ptr(),
                muid.as_ptr(),
                raw.as_ptr(),
                raw.len(),
                &mut addr_out,
            ),
            addr_out,
        )
    };
    assert!(
        addr.starts_with("blake3:"),
        "body address is tagged: {addr}"
    );
    // Get the raw .eml back byte-exact.
    let (mut bp, mut bn, mut bfound) = (core::ptr::null_mut(), 0usize, -1i32);
    assert_eq!(
        unsafe {
            loom_mail_to_eml(
                handle,
                mns.as_ptr(),
                principal.as_ptr(),
                mailbox.as_ptr(),
                muid.as_ptr(),
                &mut bp,
                &mut bn,
                &mut bfound,
            )
        },
        0,
        "mail_to_eml: {:?}",
        last_err()
    );
    assert_eq!(bfound, 1, "an ingested message must be found");
    let body = unsafe { std::slice::from_raw_parts(bp, bn) }.to_vec();
    unsafe { loom_bytes_free(bp, bn) };
    assert_eq!(body, raw, "mail body round-trips byte-exact");
    // Set flags (CBOR Array(Text)) then read them back.
    let flags_in = cbor_encode(&CborValue::Array(vec![
        CborValue::Text("\\Seen".into()),
        CborValue::Text("important".into()),
    ]))
    .unwrap();
    let bad_flags = [0xffu8];
    let bad_flags_status = unsafe {
        loom_mail_set_flags(
            handle,
            mns.as_ptr(),
            principal.as_ptr(),
            mailbox.as_ptr(),
            muid.as_ptr(),
            bad_flags.as_ptr(),
            bad_flags.len(),
        )
    };
    assert_eq!(
        bad_flags_status,
        Code::InvalidArgument.as_i32(),
        "{:?}",
        last_err()
    );
    assert_eq!(
        unsafe {
            loom_mail_set_flags(
                handle,
                mns.as_ptr(),
                principal.as_ptr(),
                mailbox.as_ptr(),
                muid.as_ptr(),
                flags_in.as_ptr(),
                flags_in.len(),
            )
        },
        0,
        "mail_set_flags: {:?}",
        last_err()
    );
    let (mut fp, mut fn_) = (core::ptr::null_mut(), 0usize);
    assert_eq!(
        unsafe {
            loom_mail_get_flags(
                handle,
                mns.as_ptr(),
                principal.as_ptr(),
                mailbox.as_ptr(),
                muid.as_ptr(),
                &mut fp,
                &mut fn_,
            )
        },
        0,
        "mail_get_flags: {:?}",
        last_err()
    );
    let flags_bytes = unsafe { std::slice::from_raw_parts(fp, fn_) }.to_vec();
    unsafe { loom_bytes_free(fp, fn_) };
    let CborValue::Array(flag_items) = loom_codec::decode(&flags_bytes).unwrap() else {
        panic!("flags must be a CBOR array");
    };
    let flags: Vec<String> = flag_items
        .into_iter()
        .map(|v| match v {
            CborValue::Text(s) => s,
            _ => panic!("flag must be CBOR text"),
        })
        .collect();
    // get_flags returns the sorted, deduplicated set.
    assert_eq!(flags, vec!["\\Seen".to_string(), "important".to_string()]);

    unsafe { loom_close(handle) };
    let _ = std::fs::remove_file(&dir);
}

#[test]
fn cas_workspace_by_uuid_over_the_c_abi() {
    let dir = temp_loom();
    let path = cs(dir.to_str().unwrap());
    let dflt = cs("default");
    assert_eq!(
        unsafe {
            loom_create(
                path.as_ptr(),
                dflt.as_ptr(),
                core::ptr::null(),
                core::ptr::null(),
                0,
            )
        },
        0
    );
    let mut handle: *mut LoomSession = core::ptr::null_mut();
    assert_eq!(unsafe { loom_open(path.as_ptr(), &mut handle) }, 0);

    // Create a cas workspace and capture its UUID.
    let (name, facet) = (cs("vault"), cs("cas"));
    let mut id_out = core::ptr::null_mut();
    let id = unsafe {
        ok_out(
            loom_workspace_create(handle, name.as_ptr(), facet.as_ptr(), &mut id_out),
            id_out,
        )
    };
    assert!(id.contains('-'), "uuid: {id}");

    // Put then has, addressing the workspace by its UUID.
    let id_arg = cs(&id);
    let content = b"by uuid";
    let mut digest_out = core::ptr::null_mut();
    let digest = unsafe {
        ok_out(
            loom_cas_put(
                handle,
                id_arg.as_ptr(),
                content.as_ptr(),
                content.len(),
                &mut digest_out,
            ),
            digest_out,
        )
    };
    let dg = cs(&digest);
    let mut has = -1i32;
    let st = unsafe { loom_cas_has(handle, id_arg.as_ptr(), dg.as_ptr(), &mut has) };
    assert_eq!(st, 0, "has by uuid failed: {:?}", last_err());
    assert_eq!(has, 1);

    unsafe { loom_close(handle) };
    let _ = std::fs::remove_file(&dir);
}

#[test]
fn queue_round_trip_over_the_c_abi() {
    let dir = temp_loom();
    let path = cs(dir.to_str().unwrap());
    let dflt = cs("default");
    assert_eq!(
        unsafe {
            loom_create(
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
    assert_eq!(unsafe { loom_open(path.as_ptr(), &mut handle) }, 0);

    let ns = cs("events");
    let stream = cs("orders");
    // append by name: the queue facet is created on first write; seqs are 0 then 1.
    let append = |h: *mut LoomSession, payload: &[u8]| -> u64 {
        let mut seq = u64::MAX;
        let st = unsafe {
            loom_queue_append(
                h,
                ns.as_ptr(),
                stream.as_ptr(),
                payload.as_ptr(),
                payload.len(),
                &mut seq,
            )
        };
        assert_eq!(st, 0, "append failed: {:?}", last_err());
        seq
    };
    assert_eq!(append(handle, b"a"), 0);
    assert_eq!(append(handle, b"b"), 1);
    assert_eq!(append(handle, b"c"), 2);

    // len reflects the appends.
    let mut n = 0u64;
    assert_eq!(
        unsafe { loom_queue_len(handle, ns.as_ptr(), stream.as_ptr(), &mut n) },
        0
    );
    assert_eq!(n, 3);

    // get returns the payload at a seq.
    let (mut p, mut blen, mut found) = (core::ptr::null_mut(), 0usize, -1i32);
    let st = unsafe {
        loom_queue_get(
            handle,
            ns.as_ptr(),
            stream.as_ptr(),
            1,
            &mut p,
            &mut blen,
            &mut found,
        )
    };
    assert_eq!(st, 0, "get failed: {:?}", last_err());
    assert_eq!(found, 1);
    let got = unsafe { std::slice::from_raw_parts(p, blen) }.to_vec();
    unsafe { loom_bytes_free(p, blen) };
    assert_eq!(got, b"b");

    // absent get: out_found = 0, null, len 0.
    let (mut p2, mut blen2, mut found2) = (core::ptr::null_mut(), 0usize, -1i32);
    let st = unsafe {
        loom_queue_get(
            handle,
            ns.as_ptr(),
            stream.as_ptr(),
            9,
            &mut p2,
            &mut blen2,
            &mut found2,
        )
    };
    assert_eq!(st, 0, "absent get must succeed: {:?}", last_err());
    assert_eq!(found2, 0);
    assert!(p2.is_null() && blen2 == 0);

    // range is half-open and ordered, returned as CBOR array of byte strings.
    let (mut rp, mut rn) = (core::ptr::null_mut(), 0usize);
    let st =
        unsafe { loom_queue_range(handle, ns.as_ptr(), stream.as_ptr(), 1, 3, &mut rp, &mut rn) };
    assert_eq!(st, 0, "range failed: {:?}", last_err());
    let cbor = unsafe { std::slice::from_raw_parts(rp, rn) }.to_vec();
    unsafe { loom_bytes_free(rp, rn) };
    let decoded = Stream::decode(&cbor).unwrap();
    assert_eq!(decoded.len(), 2);
    assert_eq!(decoded.get(0), Some(&b"b"[..]));
    assert_eq!(decoded.get(1), Some(&b"c"[..]));

    // an invalid stream name is INVALID_ARGUMENT.
    let bad = cs("../escape");
    let mut seq = 0u64;
    let st = unsafe {
        loom_queue_append(
            handle,
            ns.as_ptr(),
            bad.as_ptr(),
            b"x".as_ptr(),
            1,
            &mut seq,
        )
    };
    assert_eq!(
        st,
        Code::InvalidArgument.as_i32(),
        "traversal name must be rejected"
    );
    let empty = cs("");
    let st = unsafe {
        loom_queue_append(
            handle,
            ns.as_ptr(),
            empty.as_ptr(),
            b"x".as_ptr(),
            1,
            &mut seq,
        )
    };
    assert_eq!(
        st,
        Code::InvalidArgument.as_i32(),
        "empty name must be rejected"
    );

    unsafe { loom_close(handle) };
    let _ = std::fs::remove_file(&dir);
}

#[test]
fn queue_workspace_by_uuid_over_the_c_abi() {
    let dir = temp_loom();
    let path = cs(dir.to_str().unwrap());
    let dflt = cs("default");
    assert_eq!(
        unsafe {
            loom_create(
                path.as_ptr(),
                dflt.as_ptr(),
                core::ptr::null(),
                core::ptr::null(),
                0,
            )
        },
        0
    );
    let mut handle: *mut LoomSession = core::ptr::null_mut();
    assert_eq!(unsafe { loom_open(path.as_ptr(), &mut handle) }, 0);

    let (name, facet) = (cs("bus"), cs("queue"));
    let mut id_out = core::ptr::null_mut();
    let id = unsafe {
        ok_out(
            loom_workspace_create(handle, name.as_ptr(), facet.as_ptr(), &mut id_out),
            id_out,
        )
    };
    assert!(id.contains('-'), "uuid: {id}");

    let id_arg = cs(&id);
    let stream = cs("log");
    let mut seq = u64::MAX;
    let st = unsafe {
        loom_queue_append(
            handle,
            id_arg.as_ptr(),
            stream.as_ptr(),
            b"hi".as_ptr(),
            2,
            &mut seq,
        )
    };
    assert_eq!(st, 0, "append by uuid failed: {:?}", last_err());
    assert_eq!(seq, 0);
    let mut n = 0u64;
    assert_eq!(
        unsafe { loom_queue_len(handle, id_arg.as_ptr(), stream.as_ptr(), &mut n) },
        0
    );
    assert_eq!(n, 1);

    unsafe { loom_close(handle) };
    let _ = std::fs::remove_file(&dir);
}

#[test]
fn queue_consumer_offsets_over_the_c_abi() {
    let dir = temp_loom();
    let path = cs(dir.to_str().unwrap());
    let dflt = cs("default");
    assert_eq!(
        unsafe {
            loom_create(
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
    assert_eq!(unsafe { loom_open(path.as_ptr(), &mut handle) }, 0);

    let ns = cs("events");
    let stream = cs("orders");
    let worker = cs("worker");
    for payload in [b"a", b"b", b"c"] {
        let mut seq = u64::MAX;
        assert_eq!(
            unsafe {
                loom_queue_append(
                    handle,
                    ns.as_ptr(),
                    stream.as_ptr(),
                    payload.as_ptr(),
                    1,
                    &mut seq,
                )
            },
            0,
            "append failed: {:?}",
            last_err()
        );
    }

    // Missing offset reads as 0; read does not advance it.
    let mut pos = u64::MAX;
    assert_eq!(
        unsafe {
            loom_queue_consumer_position(
                handle,
                ns.as_ptr(),
                stream.as_ptr(),
                worker.as_ptr(),
                &mut pos,
            )
        },
        0
    );
    assert_eq!(pos, 0);

    let read_batch = |h: *mut LoomSession| -> Vec<Vec<u8>> {
        let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
        let st = unsafe {
            loom_queue_consumer_read(
                h,
                ns.as_ptr(),
                stream.as_ptr(),
                worker.as_ptr(),
                2,
                &mut p,
                &mut n,
            )
        };
        assert_eq!(st, 0, "consumer read failed: {:?}", last_err());
        let cbor = unsafe { std::slice::from_raw_parts(p, n) }.to_vec();
        unsafe { loom_bytes_free(p, n) };
        let s = Stream::decode(&cbor).unwrap();
        (0..s.len()).map(|i| s.get(i).unwrap().to_vec()).collect()
    };
    assert_eq!(read_batch(handle), vec![b"a".to_vec(), b"b".to_vec()]);
    // Read again without advancing redelivers the same entries.
    assert_eq!(read_batch(handle), vec![b"a".to_vec(), b"b".to_vec()]);

    // Advance persists across reopen.
    assert_eq!(
        unsafe {
            loom_queue_consumer_advance(handle, ns.as_ptr(), stream.as_ptr(), worker.as_ptr(), 2)
        },
        0,
        "advance failed: {:?}",
        last_err()
    );
    unsafe { loom_close(handle) };
    let mut reopened: *mut LoomSession = core::ptr::null_mut();
    assert_eq!(unsafe { loom_open(path.as_ptr(), &mut reopened) }, 0);
    let mut pos2 = u64::MAX;
    assert_eq!(
        unsafe {
            loom_queue_consumer_position(
                reopened,
                ns.as_ptr(),
                stream.as_ptr(),
                worker.as_ptr(),
                &mut pos2,
            )
        },
        0
    );
    assert_eq!(pos2, 2, "advance must persist across reopen");

    // Backward advance is rejected; reset can move backward.
    assert_eq!(
        unsafe {
            loom_queue_consumer_advance(reopened, ns.as_ptr(), stream.as_ptr(), worker.as_ptr(), 1)
        },
        Code::InvalidArgument.as_i32()
    );
    assert_eq!(
        unsafe {
            loom_queue_consumer_reset(reopened, ns.as_ptr(), stream.as_ptr(), worker.as_ptr(), 0)
        },
        0,
        "reset failed: {:?}",
        last_err()
    );
    let mut pos3 = u64::MAX;
    unsafe {
        loom_queue_consumer_position(
            reopened,
            ns.as_ptr(),
            stream.as_ptr(),
            worker.as_ptr(),
            &mut pos3,
        )
    };
    assert_eq!(pos3, 0);

    // Invalid consumer id is rejected.
    let bad = cs("a/b");
    assert_eq!(
        unsafe {
            loom_queue_consumer_position(
                reopened,
                ns.as_ptr(),
                stream.as_ptr(),
                bad.as_ptr(),
                &mut pos3,
            )
        },
        Code::InvalidArgument.as_i32()
    );

    unsafe { loom_close(reopened) };
    let _ = std::fs::remove_file(&dir);
}

#[test]
fn c_abi_reproduces_the_cross_language_exec_vector() {
    // The C ABI is the path cpp/jvm/android/react-native take. Run the shared exec vector
    // through loom_sql_exec and assert the raw canonical bytes are byte-for-byte the
    // engine-pinned vector - the same bytes every binding's test asserts against the fixture, so a
    // C-ABI consumer decodes them to identical typed values through the one shared result-view.
    let dir = temp_loom();
    let (path, ns, db) = (cs(dir.to_str().unwrap()), cs("app"), cs("main"));
    let mut session: *mut LoomSqlSession = core::ptr::null_mut();
    let st = unsafe { loom_sql_open(path.as_ptr(), ns.as_ptr(), db.as_ptr(), &mut session) };
    assert_eq!(st, 0, "open failed: {:?}", last_err());

    for stmt in [
        "CREATE TABLE t (id INTEGER PRIMARY KEY, n TEXT)",
        "INSERT INTO t VALUES (1, 'hi'), (2, NULL)",
    ] {
        let c = cs(stmt);
        let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
        let st = unsafe { loom_sql_exec(session, c.as_ptr(), &mut p, &mut n) };
        assert_eq!(st, 0, "exec failed: {:?}", last_err());
        unsafe { loom_bytes_free(p, n) };
    }

    let select = cs("SELECT id, n FROM t ORDER BY id");
    let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
    let st = unsafe { loom_sql_exec(session, select.as_ptr(), &mut p, &mut n) };
    assert_eq!(st, 0, "select failed: {:?}", last_err());
    // SAFETY: on success `(p, n)` is a live result buffer.
    let bytes = unsafe { std::slice::from_raw_parts(p, n) }.to_vec();
    unsafe { loom_bytes_free(p, n) };
    unsafe { loom_sql_close(session) };

    assert_eq!(
        bytes,
        loom_sql::result_exec_vector(),
        "C-ABI exec bytes diverged from the pinned cross-language vector"
    );
    assert_eq!(
        loom_core::Digest::blake3(&bytes).to_string(),
        loom_sql::RESULT_EXEC_VECTOR_DIGEST
    );
    let _ = std::fs::remove_file(&dir);
}

/// Run one batch statement, asserting success and freeing the result buffer.
unsafe fn batch_exec_ok(batch: *mut LoomSqlBatch, sql: &str) {
    let c = cs(sql);
    let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
    let st = unsafe { loom_sql_batch_exec(batch, c.as_ptr(), &mut p, &mut n) };
    assert_eq!(st, 0, "batch exec '{sql}' failed: {:?}", last_err());
    unsafe { loom_bytes_free(p, n) };
}

#[test]
fn batch_transaction_commit_and_rollback_over_the_c_abi() {
    // A batch holds one store across statements, so a SQL transaction spans calls: an in-transaction
    // insert that is ROLLed BACK must vanish, while a row committed before BEGIN survives, and a
    // single batch commit makes it all durable (verified across a fresh open).
    let dir = temp_loom();
    let (path, ns, db) = (cs(dir.to_str().unwrap()), cs("app"), cs("main"));
    let mut batch: *mut LoomSqlBatch = core::ptr::null_mut();
    let st = unsafe { loom_sql_batch_begin(path.as_ptr(), ns.as_ptr(), db.as_ptr(), &mut batch) };
    assert_eq!(st, 0, "batch begin failed: {:?}", last_err());

    unsafe {
        batch_exec_ok(batch, "CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)");
        batch_exec_ok(batch, "INSERT INTO t VALUES (1, 'a')");
        batch_exec_ok(batch, "BEGIN");
        batch_exec_ok(batch, "INSERT INTO t VALUES (2, 'b')");
        batch_exec_ok(batch, "ROLLBACK");
        assert_eq!(loom_sql_batch_commit(batch), 0, "commit: {:?}", last_err());
        loom_sql_batch_close(batch);
    }

    // Reopen a session and confirm the rolled-back row is absent.
    let mut session: *mut LoomSqlSession = core::ptr::null_mut();
    let _ = unsafe { loom_sql_open(path.as_ptr(), ns.as_ptr(), db.as_ptr(), &mut session) };
    let select = cs("SELECT v FROM t ORDER BY id");
    let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
    let json = unsafe {
        ok_render(
            loom_sql_exec(session, select.as_ptr(), &mut p, &mut n),
            p,
            n,
        )
    };
    assert!(
        json.contains("\"Text\":\"a\""),
        "committed row missing: {json}"
    );
    assert!(
        !json.contains("\"Text\":\"b\""),
        "rolled-back row present: {json}"
    );
    unsafe { loom_sql_close(session) };
    let _ = std::fs::remove_file(&dir);
}

#[test]
fn batch_commit_rejects_open_transaction() {
    let dir = temp_loom();
    let (path, ns, db) = (cs(dir.to_str().unwrap()), cs("app"), cs("main"));
    let mut batch: *mut LoomSqlBatch = core::ptr::null_mut();
    let _ = unsafe { loom_sql_batch_begin(path.as_ptr(), ns.as_ptr(), db.as_ptr(), &mut batch) };
    unsafe {
        batch_exec_ok(batch, "CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)");
        batch_exec_ok(batch, "BEGIN");
        let st = loom_sql_batch_commit(batch);
        assert_ne!(st, 0, "commit with open txn must fail");
        assert!(
            last_err().unwrap().1.contains("open SQL transaction"),
            "unexpected error"
        );
        // Resolve and then commit succeeds.
        batch_exec_ok(batch, "ROLLBACK");
        assert_eq!(loom_sql_batch_commit(batch), 0, "{:?}", last_err());
        loom_sql_batch_close(batch);
    }
    let _ = std::fs::remove_file(&dir);
}

#[test]
fn direct_ops_over_the_c_abi() {
    let dir = temp_loom();
    let (path, db, ns) = (cs(dir.to_str().unwrap()), cs("main"), cs("app"));
    let table = cs(".loom/facets/sql/main/tables/t");

    // Seed via the SQL session: table + index + rows, across two commits.
    unsafe {
        let mut s: *mut LoomSqlSession = core::ptr::null_mut();
        assert_eq!(
            loom_sql_open(path.as_ptr(), ns.as_ptr(), db.as_ptr(), &mut s),
            0
        );
        for stmt in [
            "CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)",
            "CREATE INDEX idx_v ON t (v)",
            "INSERT INTO t VALUES (1,'a'),(2,'b')",
        ] {
            let c = cs(stmt);
            let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
            let _ = ok_render(loom_sql_exec(s, c.as_ptr(), &mut p, &mut n), p, n);
        }
        let (m1, author) = (cs("c1"), cs("seed"));
        let mut out = core::ptr::null_mut();
        let _ = ok_out(
            loom_sql_commit(s, m1.as_ptr(), author.as_ptr(), &mut out),
            out,
        );
        let ins = cs("INSERT INTO t VALUES (3,'c')");
        let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
        let _ = ok_render(loom_sql_exec(s, ins.as_ptr(), &mut p, &mut n), p, n);
        let m2 = cs("c2");
        out = core::ptr::null_mut();
        let _ = ok_out(
            loom_sql_commit(s, m2.as_ptr(), author.as_ptr(), &mut out),
            out,
        );
        loom_sql_close(s);
    }

    let mut h: *mut LoomSession = core::ptr::null_mut();
    assert_eq!(
        unsafe { loom_open(path.as_ptr(), &mut h) },
        0,
        "open: {:?}",
        last_err()
    );
    let branch = cs("main");

    // log: two commits, newest first.
    let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
    let log = unsafe {
        ok_render(
            loom_log(h, ns.as_ptr(), branch.as_ptr(), &mut p, &mut n),
            p,
            n,
        )
    };
    let hexes: Vec<&str> = log
        .split('"')
        .filter(|s| s.starts_with("blake3:"))
        .collect();
    assert_eq!(hexes.len(), 2, "log: {log}");

    // read_table: columns + the three rows.
    let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
    let rt = unsafe {
        ok_render(
            loom_sql_read_table(h, ns.as_ptr(), table.as_ptr(), &mut p, &mut n),
            p,
            n,
        )
    };
    assert!(
        rt.contains("\"columns\"") && rt.contains("\"a\"") && rt.contains("\"c\""),
        "{rt}"
    );

    let (from, to) = (cs(hexes[1]), cs(hexes[0]));

    // read_table_at: c1 has two rows, so the later "c" row is absent and current state is unchanged.
    let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
    let old_rt = unsafe {
        ok_render(
            loom_sql_read_table_at(
                h,
                ns.as_ptr(),
                table.as_ptr(),
                from.as_ptr(),
                &mut p,
                &mut n,
            ),
            p,
            n,
        )
    };
    assert!(
        old_rt.contains("\"a\"") && old_rt.contains("\"b\""),
        "{old_rt}"
    );
    assert!(!old_rt.contains("\"c\""), "{old_rt}");

    // index_scan idx_v for v='b'. The lookup prefix is canonical CBOR (a cell array), built with
    // the shared cell codec - the same form as a result row.
    let idx = cs("idx_v");
    let lookup = loom_core::tabular::encode_cells(&[loom_core::tabular::Value::Text("b".into())]);
    let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
    let scan = unsafe {
        ok_render(
            loom_sql_index_scan(
                h,
                ns.as_ptr(),
                table.as_ptr(),
                idx.as_ptr(),
                lookup.as_ptr(),
                lookup.len(),
                &mut p,
                &mut n,
            ),
            p,
            n,
        )
    };
    assert!(scan.contains("\"b\""), "index_scan: {scan}");

    let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
    let old_scan = unsafe {
        ok_render(
            loom_sql_index_scan_at(
                h,
                ns.as_ptr(),
                table.as_ptr(),
                idx.as_ptr(),
                lookup.as_ptr(),
                lookup.len(),
                from.as_ptr(),
                &mut p,
                &mut n,
            ),
            p,
            n,
        )
    };
    assert!(old_scan.contains("\"b\""), "index_scan_at: {old_scan}");

    // blame.
    let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
    let blame = unsafe {
        ok_render(
            loom_sql_blame(
                h,
                ns.as_ptr(),
                branch.as_ptr(),
                table.as_ptr(),
                &mut p,
                &mut n,
            ),
            p,
            n,
        )
    };
    assert!(
        blame.contains("\"commit\"") && blame.contains("blake3:"),
        "blame: {blame}"
    );

    // diff c1 -> c2: the third row is added.
    let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
    let diff = unsafe {
        ok_render(
            loom_sql_diff(
                h,
                ns.as_ptr(),
                table.as_ptr(),
                from.as_ptr(),
                to.as_ptr(),
                &mut p,
                &mut n,
            ),
            p,
            n,
        )
    };
    assert!(diff.contains("\"added\""), "diff: {diff}");

    let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
    let table_diff = unsafe {
        ok_render(
            loom_sql_table_diff(
                h,
                ns.as_ptr(),
                table.as_ptr(),
                from.as_ptr(),
                to.as_ptr(),
                &mut p,
                &mut n,
            ),
            p,
            n,
        )
    };
    assert!(
        table_diff.contains("\"kind\":\"TableDiff\"") && table_diff.contains("\"added\""),
        "table_diff: {table_diff}"
    );

    // branch + checkout (status-only).
    let feature = cs("feature");
    assert_eq!(unsafe { loom_branch(h, ns.as_ptr(), feature.as_ptr()) }, 0);
    assert_eq!(
        unsafe { loom_checkout(h, ns.as_ptr(), feature.as_ptr()) },
        0
    );

    // A null handle is INVALID_ARGUMENT.
    let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
    let st = unsafe {
        loom_log(
            core::ptr::null_mut(),
            ns.as_ptr(),
            branch.as_ptr(),
            &mut p,
            &mut n,
        )
    };
    assert_eq!(st, Code::InvalidArgument.as_i32());
    assert!(last_err().unwrap().1.contains("null handle"));

    unsafe { loom_close(h) };
    unsafe { loom_close(core::ptr::null_mut()) }; // no-op
    let _ = std::fs::remove_file(&dir);
}

/// Run one statement through the async task primitive to completion and render the result to JSON.
fn run_async(session: *mut LoomSqlSession, sql: &str) -> String {
    let c = cs(sql);
    let mut task: *mut LoomTask = core::ptr::null_mut();
    assert_eq!(
        unsafe { loom_sql_exec_async(session, c.as_ptr(), &mut task) },
        0,
        "exec_async: {:?}",
        last_err()
    );
    assert_eq!(unsafe { loom_task_status(task) }, LOOM_TASK_PENDING);
    let mut done = 0i32;
    assert_eq!(unsafe { loom_task_poll(task, &mut done) }, 0);
    assert_eq!(done, 1, "first poll completes under the portable backend");
    assert_eq!(unsafe { loom_task_status(task) }, LOOM_TASK_READY);
    let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
    let st = unsafe { loom_task_result(task, &mut p, &mut n) };
    let json = unsafe { ok_render(st, p, n) };
    assert_eq!(unsafe { loom_task_status(task) }, LOOM_TASK_TAKEN);
    // A second take is invalid (the buffer transferred exactly once).
    let (mut p2, mut n2) = (core::ptr::null_mut(), 0usize);
    assert_eq!(
        unsafe { loom_task_result(task, &mut p2, &mut n2) },
        Code::InvalidArgument.as_i32()
    );
    unsafe { loom_task_free(task) };
    json
}

#[test]
fn async_task_poll_result_cancel_and_error() {
    let dir = temp_loom();
    let (path, ns, db) = (cs(dir.to_str().unwrap()), cs("app"), cs("main"));
    let mut session: *mut LoomSqlSession = core::ptr::null_mut();
    assert_eq!(
        unsafe { loom_sql_open(path.as_ptr(), ns.as_ptr(), db.as_ptr(), &mut session) },
        0
    );

    let _ = run_async(session, "CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)");
    let _ = run_async(session, "INSERT INTO t VALUES (1,'hello')");
    let json = run_async(session, "SELECT id, v FROM t");
    assert!(json.contains("hello"), "{json}");

    // Cancel while pending: the op never runs, the task is terminal, result is INVALID_ARGUMENT.
    let c = cs("SELECT 1");
    let mut task: *mut LoomTask = core::ptr::null_mut();
    assert_eq!(
        unsafe { loom_sql_exec_async(session, c.as_ptr(), &mut task) },
        0
    );
    unsafe { loom_task_cancel(task) };
    assert_eq!(unsafe { loom_task_status(task) }, LOOM_TASK_CANCELLED);
    let mut done = 0i32;
    assert_eq!(unsafe { loom_task_poll(task, &mut done) }, 0);
    assert_eq!(done, 1, "a cancelled task is terminal");
    assert_eq!(unsafe { loom_task_status(task) }, LOOM_TASK_CANCELLED);
    let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
    assert_eq!(
        unsafe { loom_task_result(task, &mut p, &mut n) },
        Code::InvalidArgument.as_i32()
    );
    unsafe { loom_task_free(task) };

    // An errored op surfaces its stable code through result, repeatably.
    let bad = cs("SELECT * FROM does_not_exist");
    let mut task: *mut LoomTask = core::ptr::null_mut();
    assert_eq!(
        unsafe { loom_sql_exec_async(session, bad.as_ptr(), &mut task) },
        0
    );
    let mut done = 0i32;
    let _ = unsafe { loom_task_poll(task, &mut done) };
    assert_eq!(done, 1);
    assert_eq!(unsafe { loom_task_status(task) }, LOOM_TASK_ERROR);
    let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
    let st = unsafe { loom_task_result(task, &mut p, &mut n) };
    assert_eq!(st, Code::SqlTableNotFound.as_i32());
    assert_eq!(
        last_err().unwrap().0,
        st,
        "result republishes the stored error"
    );
    // Reporting the error is repeatable (status stays ERROR, not consumed).
    let st2 = unsafe { loom_task_result(task, &mut p, &mut n) };
    assert_eq!(st2, st);
    assert_eq!(unsafe { loom_task_status(task) }, LOOM_TASK_ERROR);
    unsafe { loom_task_free(task) };

    // A null task is INVALID_ARGUMENT on poll/result; cancel/free of null are no-ops.
    let mut done = 0i32;
    assert_eq!(
        unsafe { loom_task_poll(core::ptr::null_mut(), &mut done) },
        Code::InvalidArgument.as_i32()
    );
    assert_eq!(unsafe { loom_task_status(core::ptr::null()) }, -1);
    unsafe { loom_task_cancel(core::ptr::null_mut()) };
    unsafe { loom_task_free(core::ptr::null_mut()) };

    unsafe { loom_sql_close(session) };
    let _ = std::fs::remove_file(&dir);
}

#[test]
fn vcs_diff_and_blame_over_the_c_abi() {
    let dir = temp_loom();
    let path = cs(dir.to_str().unwrap());
    let dflt = cs("default");
    assert_eq!(
        unsafe {
            loom_create(
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
    let mut h: *mut LoomSession = core::ptr::null_mut();
    assert_eq!(unsafe { loom_open(path.as_ptr(), &mut h) }, 0);
    let (facet, ns, branch, author) = (cs("files"), cs("docs"), cs("main"), cs("nas"));
    let mut nsout: *mut c_char = core::ptr::null_mut();
    let _ = unsafe {
        ok_out(
            loom_workspace_create(h, ns.as_ptr(), facet.as_ptr(), &mut nsout),
            nsout,
        )
    };

    let write = |name: &str, body: &[u8]| {
        let f = cs(name);
        assert_eq!(
            unsafe { loom_write_file(h, ns.as_ptr(), f.as_ptr(), body.as_ptr(), body.len(), 0) },
            0,
            "write {name} failed: {:?}",
            last_err()
        );
    };
    let commit = |msg: &str| -> String {
        let m = cs(msg);
        let mut out: *mut c_char = core::ptr::null_mut();
        unsafe {
            ok_out(
                loom_commit(h, ns.as_ptr(), author.as_ptr(), m.as_ptr(), &mut out),
                out,
            )
        }
    };

    write("a.txt", b"a0");
    write("b.txt", b"b0");
    let c0 = commit("c0");
    write("b.txt", b"b1");
    write("c.txt", b"c1");
    let c1 = commit("c1");

    // vcs.diff c0..c1: structural cross-facet diff.
    let (c0c, c1c) = (cs(&c0), cs(&c1));
    let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
    assert_eq!(
        unsafe { loom_vcs_diff(h, ns.as_ptr(), c0c.as_ptr(), c1c.as_ptr(), &mut p, &mut n,) },
        0,
        "diff failed: {:?}",
        last_err()
    );
    let diff = unsafe { std::slice::from_raw_parts(p, n) }.to_vec();
    unsafe { loom_bytes_free(p, n) };
    assert!(
        diff.windows(b"LMDIFF".len()).any(|w| w == b"LMDIFF"),
        "{diff:?}"
    );

    // vcs.blame: a attributed to c0, b and c to c1.
    let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
    assert_eq!(
        unsafe { loom_vcs_blame(h, ns.as_ptr(), branch.as_ptr(), &mut p, &mut n,) },
        0,
        "blame failed: {:?}",
        last_err()
    );
    let blame_s = String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(p, n) }).into_owned();
    unsafe { loom_bytes_free(p, n) };
    assert!(blame_s.contains("PathBlame"), "{blame_s}");
    assert!(blame_s.contains(&c0) && blame_s.contains(&c1), "{blame_s}");

    unsafe { loom_close(h) };
    let _ = std::fs::remove_file(&dir);
}

#[test]
fn watch_subscribe_and_poll_over_the_c_abi() {
    let dir = temp_loom();
    let path = cs(dir.to_str().unwrap());
    let dflt = cs("default");
    assert_eq!(
        unsafe {
            loom_create(
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
    let mut h: *mut LoomSession = core::ptr::null_mut();
    assert_eq!(unsafe { loom_open(path.as_ptr(), &mut h) }, 0);
    let (facet, ns, branch, author) = (cs("files"), cs("docs"), cs("main"), cs("nas"));
    let mut nsout: *mut c_char = core::ptr::null_mut();
    let _ = unsafe {
        ok_out(
            loom_workspace_create(h, ns.as_ptr(), facet.as_ptr(), &mut nsout),
            nsout,
        )
    };

    let mut cursor: *mut c_char = core::ptr::null_mut();
    let files = cs("files");
    assert_eq!(
        unsafe {
            loom_watch_subscribe(
                h,
                ns.as_ptr(),
                branch.as_ptr(),
                files.as_ptr(),
                core::ptr::null(),
                core::ptr::null(),
                core::ptr::null(),
                &mut cursor,
            )
        },
        0,
        "subscribe failed: {:?}",
        last_err()
    );
    let cursor = unsafe { ok_out(0, cursor) };

    let name = cs("a.txt");
    assert_eq!(
        unsafe { loom_write_file(h, ns.as_ptr(), name.as_ptr(), b"a1".as_ptr(), 2, 0) },
        0,
        "write failed: {:?}",
        last_err()
    );
    let message = cs("c1");
    let mut commit: *mut c_char = core::ptr::null_mut();
    let commit = unsafe {
        ok_out(
            loom_commit(
                h,
                ns.as_ptr(),
                author.as_ptr(),
                message.as_ptr(),
                &mut commit,
            ),
            commit,
        )
    };

    let cursor_c = cs(&cursor);
    let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
    assert_eq!(
        unsafe { loom_watch_poll(h, cursor_c.as_ptr(), 10, &mut p, &mut n) },
        0,
        "poll failed: {:?}",
        last_err()
    );
    let batch = unsafe { ok_raw(0, p, n) };
    let CborValue::Map(fields) = loom_codec::decode(&batch).expect("watch batch cbor") else {
        panic!("watch batch must be a map");
    };
    assert_eq!(
        cbor_get(&fields, "schema"),
        &CborValue::Text("loom.watch.batch.v1".to_string())
    );
    let CborValue::Array(events) = cbor_get(&fields, "events") else {
        panic!("events must be an array");
    };
    assert_eq!(events.len(), 1, "{events:?}");
    let CborValue::Map(event) = &events[0] else {
        panic!("event must be a map");
    };
    assert_eq!(cbor_get(event, "commit"), &CborValue::Text(commit));
    let CborValue::Array(changes) = cbor_get(event, "changes") else {
        panic!("changes must be an array");
    };
    assert_eq!(changes.len(), 1, "{changes:?}");
    let CborValue::Map(change) = &changes[0] else {
        panic!("change must be a map");
    };
    assert_eq!(
        cbor_get(change, "domain"),
        &CborValue::Text("files".to_string())
    );
    assert_eq!(
        cbor_get(change, "kind"),
        &CborValue::Text("added".to_string())
    );

    let mut task: *mut LoomTask = core::ptr::null_mut();
    assert_eq!(
        unsafe { loom_watch_poll_async(h, cursor_c.as_ptr(), 10, &mut task) },
        0,
        "poll async failed: {:?}",
        last_err()
    );
    let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
    let async_batch = unsafe { ok_raw(loom_task_wait(task, &mut p, &mut n), p, n) };
    unsafe { loom_task_free(task) };
    assert_eq!(batch, async_batch);

    unsafe { loom_close(h) };
    let _ = std::fs::remove_file(&dir);
}

#[test]
fn async_readers_via_task_wait() {
    let dir = temp_loom();
    let (path, ns, db) = (cs(dir.to_str().unwrap()), cs("app"), cs("main"));

    // Seed a committed table through a SQL session.
    let mut session: *mut LoomSqlSession = core::ptr::null_mut();
    assert_eq!(
        unsafe { loom_sql_open(path.as_ptr(), ns.as_ptr(), db.as_ptr(), &mut session) },
        0
    );
    let _ = run_async(session, "CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)");
    let _ = run_async(session, "INSERT INTO t VALUES (1,'a')");
    let (m, a) = (cs("seed"), cs("nas"));
    let mut out = core::ptr::null_mut();
    let _ = unsafe {
        ok_out(
            loom_sql_commit(session, m.as_ptr(), a.as_ptr(), &mut out),
            out,
        )
    };
    unsafe { loom_sql_close(session) };

    let mut h: *mut LoomSession = core::ptr::null_mut();
    assert_eq!(unsafe { loom_open(path.as_ptr(), &mut h) }, 0);
    let (branch, table) = (cs("main"), cs(".loom/facets/sql/main/tables/t"));

    // loom_log_async + loom_task_wait yields the commit log.
    let mut task: *mut LoomTask = core::ptr::null_mut();
    assert_eq!(
        unsafe { loom_log_async(h, ns.as_ptr(), branch.as_ptr(), &mut task) },
        0
    );
    let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
    let log = unsafe { ok_render(loom_task_wait(task, &mut p, &mut n), p, n) };
    unsafe { loom_task_free(task) };
    assert!(log.contains("blake3:"), "log: {log}");

    // loom_sql_read_table_async + loom_task_wait yields the table.
    let mut task2: *mut LoomTask = core::ptr::null_mut();
    assert_eq!(
        unsafe { loom_sql_read_table_async(h, ns.as_ptr(), table.as_ptr(), &mut task2) },
        0
    );
    let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
    let rt = unsafe { ok_render(loom_task_wait(task2, &mut p, &mut n), p, n) };
    unsafe { loom_task_free(task2) };
    assert!(
        rt.contains("\"columns\"") && rt.contains("\"a\""),
        "table: {rt}"
    );

    // A null handle is INVALID_ARGUMENT at the async constructor.
    let mut bad: *mut LoomTask = core::ptr::null_mut();
    assert_eq!(
        unsafe {
            loom_log_async(
                core::ptr::null_mut(),
                ns.as_ptr(),
                branch.as_ptr(),
                &mut bad,
            )
        },
        Code::InvalidArgument.as_i32()
    );

    unsafe { loom_close(h) };
    let _ = std::fs::remove_file(&dir);
}

/// Open a fresh store at a unique temp path and return `(path, handle)`.
fn open_fresh() -> (std::path::PathBuf, *mut LoomSession) {
    let dir = temp_loom();
    let path = cs(dir.to_str().unwrap());
    let dflt = cs("default");
    assert_eq!(
        unsafe {
            loom_create(
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
    assert_eq!(unsafe { loom_open(path.as_ptr(), &mut handle) }, 0);
    (dir, handle)
}

fn create_workspace(handle: *mut LoomSession, name: &str, facet: &str) {
    let name = cs(name);
    let facet = cs(facet);
    let mut out: *mut c_char = core::ptr::null_mut();
    unsafe {
        let _ = ok_out(
            loom_workspace_create(handle, name.as_ptr(), facet.as_ptr(), &mut out),
            out,
        );
    }
}

#[test]
fn meetings_import_and_source_read_over_the_c_abi() {
    let (dir, handle) = open_fresh();
    create_workspace(handle, "studio", "vcs");
    let digest = Digest::hash(Algo::Blake3, b"source").to_string();
    let snapshot = serde_json::json!({
        "snapshot_version": 1,
        "profile": "granola-app",
        "source_system": "granola-app",
        "source_scope": "local-cache",
        "observed_at": 500,
        "coverage": "complete",
        "items": [{
            "source_entity_id": "note-1",
            "source_digest": digest,
            "source_sidecar": {"id": "note-1", "raw": true},
            "title": "Planning",
            "summary_text": "Planning summary",
            "transcript_spans": [{"text": "Capture decisions."}],
            "decisions": [{"label": "Use normalized meeting imports."}]
        }]
    });
    let bytes = serde_json::to_vec(&snapshot).unwrap();
    let workspace = cs("studio");
    let profile = cs("granola-app");
    let mut out: *mut c_char = core::ptr::null_mut();
    let report = unsafe {
        ok_out(
            loom_meetings_import_snapshot(
                handle,
                workspace.as_ptr(),
                profile.as_ptr(),
                bytes.as_ptr(),
                bytes.len(),
                0,
                &mut out,
            ),
            out,
        )
    };
    let report_json: serde_json::Value = serde_json::from_str(&report).unwrap();
    assert_eq!(report_json["profile"], "meetings");
    assert_eq!(report_json["source_scope"], "local-cache");
    assert_eq!(report_json["rows_imported"], 1);
    assert_eq!(
        report_json["operations_applied"],
        report_json["operations_planned"]
    );

    let source = cs("note-1");
    let leaf = cs("summary.txt");
    let mut ptr: *mut c_uchar = core::ptr::null_mut();
    let mut len = 0usize;
    let summary = unsafe {
        take_buf(
            loom_meetings_source_read(
                handle,
                workspace.as_ptr(),
                source.as_ptr(),
                leaf.as_ptr(),
                &mut ptr,
                &mut len,
            ),
            ptr,
            len,
        )
    };
    assert_eq!(summary, b"Planning summary");

    unsafe { loom_close(handle) };
    let _ = std::fs::remove_file(&dir);
}

#[test]
fn drive_round_trip_over_the_c_abi() {
    let (dir, handle) = open_fresh();
    create_workspace(handle, "repo", "vcs");
    let workspace = cs("repo");
    let profile = cs("main");
    let root_folder = cs("root");
    let mut out: *mut c_char = core::ptr::null_mut();
    let root_json = unsafe {
        ok_out(
            loom_drive_list_json(
                handle,
                workspace.as_ptr(),
                profile.as_ptr(),
                root_folder.as_ptr(),
                &mut out,
            ),
            out,
        )
    };
    let root: serde_json::Value = serde_json::from_str(&root_json).unwrap();
    assert_eq!(root["workspace_id"], "main");
    assert_eq!(root["folder_id"], "root");
    assert_eq!(root["entries"].as_array().unwrap().len(), 0);
    let root_digest = root["profile_root"].as_str().unwrap();

    let folder = cs("folder-1");
    let folder_name = cs("Specs");
    let created = unsafe {
        ok_out(
            loom_drive_create_folder_json(
                handle,
                workspace.as_ptr(),
                profile.as_ptr(),
                root_folder.as_ptr(),
                folder.as_ptr(),
                folder_name.as_ptr(),
                cs(root_digest).as_ptr(),
                &mut out,
            ),
            out,
        )
    };
    let created: serde_json::Value = serde_json::from_str(&created).unwrap();
    assert_eq!(created["operation_kind"], "folder.created");
    let after_folder_root = created["profile_root"].as_str().unwrap();

    let upload = cs("upload-1");
    let file = cs("file-1");
    let file_name = cs("readme.txt");
    let upload_json = unsafe {
        ok_out(
            loom_drive_create_upload_json(
                handle,
                workspace.as_ptr(),
                profile.as_ptr(),
                upload.as_ptr(),
                folder.as_ptr(),
                file_name.as_ptr(),
                file.as_ptr(),
                cs(after_folder_root).as_ptr(),
                100,
                0,
                &mut out,
            ),
            out,
        )
    };
    let upload_value: serde_json::Value = serde_json::from_str(&upload_json).unwrap();
    assert_eq!(upload_value["chunk_count"], 0);

    let bytes = b"drive bytes";
    let chunk_json = unsafe {
        ok_out(
            loom_drive_upload_chunk_json(
                handle,
                workspace.as_ptr(),
                profile.as_ptr(),
                upload.as_ptr(),
                bytes.as_ptr(),
                bytes.len(),
                &mut out,
            ),
            out,
        )
    };
    let chunk_value: serde_json::Value = serde_json::from_str(&chunk_json).unwrap();
    assert_eq!(chunk_value["chunk_count"], 1);
    assert_eq!(chunk_value["total_size"], bytes.len() as u64);

    let committed = unsafe {
        ok_out(
            loom_drive_commit_upload_json(
                handle,
                workspace.as_ptr(),
                profile.as_ptr(),
                upload.as_ptr(),
                &mut out,
            ),
            out,
        )
    };
    let committed: serde_json::Value = serde_json::from_str(&committed).unwrap();
    assert_eq!(committed["operation_kind"], "file.upload_committed");

    let mut ptr: *mut c_uchar = core::ptr::null_mut();
    let mut len = 0usize;
    let read = unsafe {
        take_buf(
            loom_drive_read(
                handle,
                workspace.as_ptr(),
                profile.as_ptr(),
                file.as_ptr(),
                &mut ptr,
                &mut len,
            ),
            ptr,
            len,
        )
    };
    assert_eq!(read, bytes);

    let versions = unsafe {
        ok_out(
            loom_drive_list_versions_json(
                handle,
                workspace.as_ptr(),
                profile.as_ptr(),
                file.as_ptr(),
                &mut out,
            ),
            out,
        )
    };
    let versions: serde_json::Value = serde_json::from_str(&versions).unwrap();
    assert_eq!(versions.as_array().unwrap().len(), 1);
    assert_eq!(versions[0]["size"], bytes.len() as u64);

    let grant = cs("grant-1");
    let principal = cs("05050505-0505-4505-8505-050505050505");
    let role = cs("editor");
    let target_kind = cs("folder");
    let shared = unsafe {
        ok_out(
            loom_drive_grant_share_json(
                handle,
                workspace.as_ptr(),
                profile.as_ptr(),
                grant.as_ptr(),
                target_kind.as_ptr(),
                folder.as_ptr(),
                principal.as_ptr(),
                role.as_ptr(),
                200,
                0,
                0,
                &mut out,
            ),
            out,
        )
    };
    let shared: serde_json::Value = serde_json::from_str(&shared).unwrap();
    assert_eq!(shared["operation_kind"], "share.granted");
    let shares = unsafe {
        ok_out(
            loom_drive_list_shares_json(handle, workspace.as_ptr(), profile.as_ptr(), &mut out),
            out,
        )
    };
    let shares: serde_json::Value = serde_json::from_str(&shares).unwrap();
    assert_eq!(shares.as_array().unwrap().len(), 1);
    assert_eq!(shares[0]["role"], "editor");

    let revoked = unsafe {
        ok_out(
            loom_drive_revoke_share_json(
                handle,
                workspace.as_ptr(),
                profile.as_ptr(),
                grant.as_ptr(),
                &mut out,
            ),
            out,
        )
    };
    let revoked: serde_json::Value = serde_json::from_str(&revoked).unwrap();
    assert_eq!(revoked["operation_kind"], "share.revoked");

    let pin = cs("hold-1");
    let hold_kind = cs("legal_hold");
    let target = cs("folder:root");
    let pinned = unsafe {
        ok_out(
            loom_drive_pin_retention_json(
                handle,
                workspace.as_ptr(),
                profile.as_ptr(),
                pin.as_ptr(),
                hold_kind.as_ptr(),
                cs(committed["profile_root"].as_str().unwrap()).as_ptr(),
                target.as_ptr(),
                300,
                0,
                0,
                &mut out,
            ),
            out,
        )
    };
    let pinned: serde_json::Value = serde_json::from_str(&pinned).unwrap();
    assert_eq!(pinned["operation_kind"], "retention.pinned");
    let pins = unsafe {
        ok_out(
            loom_drive_list_retention_json(handle, workspace.as_ptr(), profile.as_ptr(), &mut out),
            out,
        )
    };
    let pins: serde_json::Value = serde_json::from_str(&pins).unwrap();
    assert_eq!(pins.as_array().unwrap().len(), 1);
    assert_eq!(pins[0]["kind"], "legal_hold");

    let unpinned = unsafe {
        ok_out(
            loom_drive_unpin_retention_json(
                handle,
                workspace.as_ptr(),
                profile.as_ptr(),
                pin.as_ptr(),
                &mut out,
            ),
            out,
        )
    };
    let unpinned: serde_json::Value = serde_json::from_str(&unpinned).unwrap();
    assert_eq!(unpinned["operation_kind"], "retention.unpinned");

    unsafe { loom_close(handle) };
    let _ = std::fs::remove_file(&dir);
}

/// Take ownership of a successful `(out_ptr, out_len)` result buffer as a `Vec<u8>`.
unsafe fn take_buf(status: i32, p: *mut c_uchar, n: usize) -> Vec<u8> {
    assert_eq!(status, 0, "status {status}, err {:?}", last_err());
    // SAFETY: on success `(p, n)` is a live library buffer.
    let v = unsafe { std::slice::from_raw_parts(p, n) }.to_vec();
    unsafe { loom_bytes_free(p, n) };
    v
}

fn cbor_text_array(bytes: &[u8]) -> Vec<String> {
    match loom_codec::decode(bytes).unwrap() {
        loom_codec::Value::Array(items) => items
            .into_iter()
            .map(|v| match v {
                loom_codec::Value::Text(s) => s,
                other => panic!("expected text, got {other:?}"),
            })
            .collect(),
        other => panic!("expected array, got {other:?}"),
    }
}

#[test]
fn metrics_c_abi_round_trips_canonical_records() {
    let (dir, handle) = open_fresh();
    let workspace = cs("native-metrics");
    let name = cs("request.duration");
    let descriptor = loom_core::MetricDescriptor::with_policy(
        "request.duration".into(),
        "Request duration".into(),
        "ms".into(),
        loom_core::MetricInstrumentKind::Histogram,
        loom_core::MetricTemporality::Delta,
        vec!["route".into()],
        loom_core::MetricDescriptorPolicy::new(
            std::collections::BTreeMap::from([("route".into(), 128)]),
            8,
            30_000,
            86_400_000,
            loom_core::MetricDistribution::explicit_histogram(vec![10.0, 100.0], 1).unwrap(),
        ),
    )
    .unwrap();
    let observation = loom_core::MetricObservation::with_value(
        descriptor.digest().unwrap(),
        std::collections::BTreeMap::new(),
        std::collections::BTreeMap::new(),
        std::collections::BTreeMap::from([("route".into(), "/v1/items".into())]),
        Some(1),
        2,
        loom_core::MetricValue::Histogram(loom_core::MetricHistogram {
            buckets: vec![1, 2, 3],
            count: 6,
            sum: 75.0,
        }),
        vec![
            loom_core::MetricExemplar::new(
                std::collections::BTreeMap::from([("trace".into(), "abc".into())]),
                2,
                12.5,
            )
            .unwrap(),
        ],
    )
    .unwrap();
    let descriptor_bytes = descriptor.encode().unwrap();
    let observation_bytes = observation.encode().unwrap();

    assert_eq!(
        unsafe {
            loom_metrics_put_descriptor(
                handle,
                workspace.as_ptr(),
                descriptor_bytes.as_ptr(),
                descriptor_bytes.len(),
            )
        },
        0,
        "put descriptor failed: {:?}",
        last_err()
    );
    assert_eq!(
        unsafe {
            loom_metrics_put_observation(
                handle,
                workspace.as_ptr(),
                name.as_ptr(),
                observation_bytes.as_ptr(),
                observation_bytes.len(),
            )
        },
        0,
        "put observation failed: {:?}",
        last_err()
    );

    let mut found = -1i32;
    let mut p: *mut c_uchar = core::ptr::null_mut();
    let mut n = 0usize;
    let got_descriptor = unsafe {
        let status = loom_metrics_get_descriptor(
            handle,
            workspace.as_ptr(),
            name.as_ptr(),
            &mut p,
            &mut n,
            &mut found,
        );
        take_buf(status, p, n)
    };
    assert_eq!(found, 1);
    assert_eq!(got_descriptor, descriptor_bytes);

    p = core::ptr::null_mut();
    n = 0;
    let query = unsafe {
        let status = loom_metrics_query_cbor(
            handle,
            workspace.as_ptr(),
            name.as_ptr(),
            0,
            10,
            1,
            1,
            1,
            1024,
            60_000,
            &mut p,
            &mut n,
        );
        take_buf(status, p, n)
    };
    let loom_codec::Value::Array(fields) = loom_codec::decode(&query).unwrap() else {
        panic!("metrics query must return an array");
    };
    assert_eq!(fields.len(), 3);
    let loom_codec::Value::Array(observations) = &fields[0] else {
        panic!("metrics query observations must be an array");
    };
    assert_eq!(observations.len(), 1);
    assert_eq!(observations[0], loom_codec::Value::Bytes(observation_bytes));
    assert_eq!(fields[1], loom_codec::Value::Bool(true));
    assert_eq!(fields[2], loom_codec::Value::Bool(true));

    unsafe { loom_close(handle) };
    let _ = std::fs::remove_file(&dir);
}

#[test]
fn filesystem_import_export_over_the_c_abi() {
    let (store_path, handle) = open_fresh();
    let workspace = cs("default");
    create_workspace(handle, "default", "files");

    let import_dir = temp_loom().with_extension("fs-import");
    let nested_dir = import_dir.join("nested");
    std::fs::create_dir_all(&nested_dir).unwrap();
    std::fs::write(nested_dir.join("a.txt"), b"filesystem body").unwrap();

    let import_path = cs(import_dir.to_str().unwrap());
    let mut p: *mut c_uchar = core::ptr::null_mut();
    let mut n = 0usize;
    let import_bytes = unsafe {
        take_buf(
            loom_fs_import(
                handle,
                workspace.as_ptr(),
                import_path.as_ptr(),
                0,
                0,
                &mut p,
                &mut n,
            ),
            p,
            n,
        )
    };
    let import_report = loom_interchange::ImportReport::decode(&import_bytes).unwrap();
    assert_eq!(import_report.operations_applied, 2);

    let file = cs("nested/a.txt");
    let mut rp: *mut c_uchar = core::ptr::null_mut();
    let mut rn = 0usize;
    let read = unsafe {
        take_buf(
            loom_read_file(handle, workspace.as_ptr(), file.as_ptr(), &mut rp, &mut rn),
            rp,
            rn,
        )
    };
    assert_eq!(read, b"filesystem body");

    let export_dir = temp_loom().with_extension("fs-export");
    let export_path = cs(export_dir.to_str().unwrap());
    let export_bytes = unsafe {
        take_buf(
            loom_fs_export(
                handle,
                workspace.as_ptr(),
                export_path.as_ptr(),
                core::ptr::null(),
                0,
                &mut p,
                &mut n,
            ),
            p,
            n,
        )
    };
    let export_report = loom_interchange::ExportReport::decode(&export_bytes).unwrap();
    assert_eq!(export_report.files_written, 1);
    assert_eq!(export_report.bytes_out, b"filesystem body".len() as u64);
    assert_eq!(
        std::fs::read(export_dir.join("nested").join("a.txt")).unwrap(),
        b"filesystem body"
    );

    unsafe { loom_close(handle) };
    let _ = std::fs::remove_file(store_path);
    let _ = std::fs::remove_dir_all(import_dir);
    let _ = std::fs::remove_dir_all(export_dir);
}

#[test]
fn archive_round_trip_over_the_c_abi() {
    let (src_path, src) = open_fresh();
    let workspace = cs("default");
    create_workspace(src, "default", "files");
    let file = cs("a.txt");
    let body = b"archive body";
    assert_eq!(
        unsafe {
            loom_write_file(
                src,
                workspace.as_ptr(),
                file.as_ptr(),
                body.as_ptr(),
                body.len(),
                0,
            )
        },
        0,
        "write_file: {:?}",
        last_err()
    );

    let archive_path = temp_loom().with_extension("zip");
    let archive = cs(archive_path.to_str().unwrap());
    let kind = cs("zip");
    let mut p: *mut c_uchar = core::ptr::null_mut();
    let mut n = 0usize;
    let export_bytes = unsafe {
        take_buf(
            loom_archive_export(
                src,
                workspace.as_ptr(),
                archive.as_ptr(),
                kind.as_ptr(),
                core::ptr::null(),
                0,
                &mut p,
                &mut n,
            ),
            p,
            n,
        )
    };
    let export_report = loom_interchange::ExportReport::decode(&export_bytes).unwrap();
    assert_eq!(export_report.files_written, 1);
    assert!(archive_path.exists());

    let (dst_path, dst) = open_fresh();
    create_workspace(dst, "default", "files");
    let mut task: *mut LoomTask = core::ptr::null_mut();
    assert_eq!(
        unsafe {
            loom_archive_import_async(
                dst,
                workspace.as_ptr(),
                archive.as_ptr(),
                kind.as_ptr(),
                0,
                &mut task,
            )
        },
        0,
        "archive_import_async: {:?}",
        last_err()
    );
    let mut ip: *mut c_uchar = core::ptr::null_mut();
    let mut ilen = 0usize;
    let import_bytes = unsafe { take_buf(loom_task_wait(task, &mut ip, &mut ilen), ip, ilen) };
    unsafe { loom_task_free(task) };
    let import_report = loom_interchange::ImportReport::decode(&import_bytes).unwrap();
    assert_eq!(import_report.operations_applied, 1);

    let mut rp: *mut c_uchar = core::ptr::null_mut();
    let mut rn = 0usize;
    let read = unsafe {
        take_buf(
            loom_read_file(dst, workspace.as_ptr(), file.as_ptr(), &mut rp, &mut rn),
            rp,
            rn,
        )
    };
    assert_eq!(read, body);

    unsafe {
        loom_close(src);
        loom_close(dst);
    }
    let _ = std::fs::remove_file(src_path);
    let _ = std::fs::remove_file(dst_path);
    let _ = std::fs::remove_file(archive_path);
}

#[test]
fn car_round_trip_over_the_c_abi() {
    let (src_path, src) = open_fresh();
    let workspace = cs("default");
    create_workspace(src, "default", "files");
    let file = cs("car.txt");
    let body = b"car body";
    assert_eq!(
        unsafe {
            loom_write_file(
                src,
                workspace.as_ptr(),
                file.as_ptr(),
                body.as_ptr(),
                body.len(),
                0,
            )
        },
        0,
        "write_file: {:?}",
        last_err()
    );
    let author = cs("ffi");
    let message = cs("car export");
    let mut commit_out: *mut c_char = core::ptr::null_mut();
    unsafe {
        let _ = ok_out(
            loom_commit(
                src,
                workspace.as_ptr(),
                author.as_ptr(),
                message.as_ptr(),
                &mut commit_out,
            ),
            commit_out,
        );
    }

    let car_path = temp_loom().with_extension("car");
    let car = cs(car_path.to_str().unwrap());
    let mut task: *mut LoomTask = core::ptr::null_mut();
    assert_eq!(
        unsafe { loom_car_export_async(src, workspace.as_ptr(), car.as_ptr(), 0, &mut task) },
        0,
        "car_export_async: {:?}",
        last_err()
    );
    let mut ep: *mut c_uchar = core::ptr::null_mut();
    let mut elen = 0usize;
    let export_bytes = unsafe { take_buf(loom_task_wait(task, &mut ep, &mut elen), ep, elen) };
    unsafe { loom_task_free(task) };
    let export_report = loom_interchange::ExportReport::decode(&export_bytes).unwrap();
    assert!(export_report.rows_written > 1);
    assert!(car_path.exists());

    let (dst_path, dst) = open_fresh();
    let mut ip: *mut c_uchar = core::ptr::null_mut();
    let mut ilen = 0usize;
    let import_bytes = unsafe {
        take_buf(
            loom_car_import(dst, car.as_ptr(), 0, &mut ip, &mut ilen),
            ip,
            ilen,
        )
    };
    let import_report = loom_interchange::ImportReport::decode(&import_bytes).unwrap();
    assert!(import_report.operations_applied > 0);

    unsafe {
        loom_close(src);
        loom_close(dst);
    }
    let _ = std::fs::remove_file(src_path);
    let _ = std::fs::remove_file(dst_path);
    let _ = std::fs::remove_file(car_path);
}

#[test]
fn graph_round_trip_over_the_c_abi() {
    let (dir, handle) = open_fresh();
    let ns = cs("graph");
    let g = cs("g");
    let (a, b, e1) = (cs("a"), cs("b"), cs("e1"));

    for id in [&a, &b] {
        let st = unsafe {
            loom_graph_upsert_node(
                handle,
                ns.as_ptr(),
                g.as_ptr(),
                id.as_ptr(),
                core::ptr::null(),
                0,
            )
        };
        assert_eq!(st, 0, "upsert_node failed: {:?}", last_err());
    }
    let rel = cs("rel");
    let st = unsafe {
        loom_graph_upsert_edge(
            handle,
            ns.as_ptr(),
            g.as_ptr(),
            e1.as_ptr(),
            a.as_ptr(),
            b.as_ptr(),
            rel.as_ptr(),
            core::ptr::null(),
            0,
        )
    };
    assert_eq!(st, 0, "upsert_edge failed: {:?}", last_err());

    let (mut p, mut n, mut found) = (core::ptr::null_mut(), 0usize, -1i32);
    let st = unsafe {
        loom_graph_get_node(
            handle,
            ns.as_ptr(),
            g.as_ptr(),
            a.as_ptr(),
            &mut p,
            &mut n,
            &mut found,
        )
    };
    assert_eq!(st, 0, "get_node failed: {:?}", last_err());
    assert_eq!(found, 1, "an upserted node is found");
    let _ = unsafe { take_buf(st, p, n) };

    let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
    let st = unsafe {
        loom_graph_neighbors_cbor(handle, ns.as_ptr(), g.as_ptr(), a.as_ptr(), &mut p, &mut n)
    };
    let bytes = unsafe { take_buf(st, p, n) };
    assert_eq!(
        cbor_text_array(&bytes),
        vec!["b".to_string()],
        "a's neighbour is b"
    );

    let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
    let st = unsafe {
        loom_graph_reachable_cbor(
            handle,
            ns.as_ptr(),
            g.as_ptr(),
            a.as_ptr(),
            -1,
            core::ptr::null(),
            &mut p,
            &mut n,
        )
    };
    let bytes = unsafe { take_buf(st, p, n) };
    assert_eq!(
        cbor_text_array(&bytes),
        vec!["b".to_string()],
        "b is reachable from a"
    );

    let mut removed = -1i32;
    let st = unsafe {
        loom_graph_remove_edge(handle, ns.as_ptr(), g.as_ptr(), e1.as_ptr(), &mut removed)
    };
    assert_eq!(st, 0, "remove_edge failed: {:?}", last_err());
    assert_eq!(removed, 1, "a present edge reports removed");

    unsafe { loom_close(handle) };
    let _ = std::fs::remove_file(&dir);
}

#[test]
fn vector_round_trip_over_the_c_abi() {
    let (dir, handle) = open_fresh();
    let ns = cs("vec");
    let set = cs("emb");
    let floats = |xs: &[f32]| -> Vec<u8> { xs.iter().flat_map(|f| f.to_le_bytes()).collect() };

    assert_eq!(
        unsafe { loom_vector_create(handle, ns.as_ptr(), set.as_ptr(), 2, 1) },
        0,
        "create failed: {:?}",
        last_err()
    );
    for (id, v) in [("a", floats(&[1.0, 0.0])), ("c", floats(&[0.9, 0.1]))] {
        let id = cs(id);
        let st = unsafe {
            loom_vector_upsert(
                handle,
                ns.as_ptr(),
                set.as_ptr(),
                id.as_ptr(),
                v.as_ptr(),
                v.len(),
                core::ptr::null(),
                0,
            )
        };
        assert_eq!(st, 0, "upsert failed: {:?}", last_err());
    }

    let a = cs("a");
    let (mut p, mut n, mut found) = (core::ptr::null_mut(), 0usize, -1i32);
    let st = unsafe {
        loom_vector_get(
            handle,
            ns.as_ptr(),
            set.as_ptr(),
            a.as_ptr(),
            &mut p,
            &mut n,
            &mut found,
        )
    };
    assert_eq!(st, 0, "get failed: {:?}", last_err());
    assert_eq!(found, 1, "an upserted vector is found");
    let _ = unsafe { take_buf(st, p, n) };

    let source = b"alpha source";
    let model_id = cs("test-embedding");
    let weights_digest = cs("sha256:test-weights");
    let a_vec = floats(&[1.0, 0.0]);
    let st = unsafe {
        loom_vector_upsert_source(
            handle,
            ns.as_ptr(),
            set.as_ptr(),
            a.as_ptr(),
            a_vec.as_ptr(),
            a_vec.len(),
            core::ptr::null(),
            0,
            source.as_ptr(),
            source.len(),
            model_id.as_ptr(),
            1,
            weights_digest.as_ptr(),
            1,
        )
    };
    assert_eq!(st, 0, "upsert_source failed: {:?}", last_err());

    let (mut p, mut n, mut found) = (core::ptr::null_mut(), 0usize, -1i32);
    let st = unsafe {
        loom_vector_source_text(
            handle,
            ns.as_ptr(),
            set.as_ptr(),
            a.as_ptr(),
            &mut p,
            &mut n,
            &mut found,
        )
    };
    assert_eq!(st, 0, "source_text failed: {:?}", last_err());
    assert_eq!(found, 1, "source text is found");
    let bytes = unsafe { take_buf(st, p, n) };
    assert_eq!(bytes, source);

    let (mut p, mut n, mut found) = (core::ptr::null_mut(), 0usize, -1i32);
    let st = unsafe {
        loom_vector_embedding_model_cbor(
            handle,
            ns.as_ptr(),
            set.as_ptr(),
            &mut p,
            &mut n,
            &mut found,
        )
    };
    assert_eq!(st, 0, "embedding_model failed: {:?}", last_err());
    assert_eq!(found, 1, "embedding model is found");
    let bytes = unsafe { take_buf(st, p, n) };
    let model = match loom_codec::decode(&bytes).unwrap() {
        loom_codec::Value::Array(items) => items,
        other => panic!("expected model array, got {other:?}"),
    };
    assert_eq!(model[0], loom_codec::Value::Uint(1));
    assert_eq!(model[1], loom_codec::Value::Text("test-embedding".into()));
    assert_eq!(model[2], loom_codec::Value::Uint(2));
    assert_eq!(
        model[3],
        loom_codec::Value::Text("sha256:test-weights".into())
    );

    let st = unsafe {
        loom_vector_upsert(
            handle,
            ns.as_ptr(),
            set.as_ptr(),
            a.as_ptr(),
            a_vec.as_ptr(),
            a_vec.len(),
            core::ptr::null(),
            0,
        )
    };
    assert_eq!(st, 0, "raw upsert after source failed: {:?}", last_err());
    let (mut p, mut n, mut found) = (core::ptr::null_mut(), 0usize, -1i32);
    let st = unsafe {
        loom_vector_source_text(
            handle,
            ns.as_ptr(),
            set.as_ptr(),
            a.as_ptr(),
            &mut p,
            &mut n,
            &mut found,
        )
    };
    assert_eq!(
        st,
        0,
        "source_text after raw upsert failed: {:?}",
        last_err()
    );
    assert_eq!(found, 0, "raw upsert clears stale source text");
    assert!(p.is_null());
    assert_eq!(n, 0);

    let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
    let st = unsafe {
        loom_vector_ids_cbor(
            handle,
            ns.as_ptr(),
            set.as_ptr(),
            core::ptr::null(),
            0,
            &mut p,
            &mut n,
        )
    };
    let bytes = unsafe { take_buf(st, p, n) };
    let ids = match loom_codec::decode(&bytes).unwrap() {
        loom_codec::Value::Array(items) => items
            .into_iter()
            .map(|item| match item {
                loom_codec::Value::Text(s) => s,
                other => panic!("expected id text, got {other:?}"),
            })
            .collect::<Vec<_>>(),
        other => panic!("expected array, got {other:?}"),
    };
    assert_eq!(ids, vec!["a".to_string(), "c".to_string()]);

    let lang = cs("lang");
    let mut changed = -1i32;
    let st = unsafe {
        loom_vector_create_metadata_index(
            handle,
            ns.as_ptr(),
            set.as_ptr(),
            lang.as_ptr(),
            &mut changed,
        )
    };
    assert_eq!(st, 0, "create metadata index failed: {:?}", last_err());
    assert_eq!(changed, 1, "new metadata index reports changed");
    let st = unsafe {
        loom_vector_create_metadata_index(
            handle,
            ns.as_ptr(),
            set.as_ptr(),
            lang.as_ptr(),
            &mut changed,
        )
    };
    assert_eq!(
        st,
        0,
        "repeat create metadata index failed: {:?}",
        last_err()
    );
    assert_eq!(changed, 0, "repeat metadata index create is idempotent");

    let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
    let st = unsafe {
        loom_vector_metadata_index_keys_cbor(handle, ns.as_ptr(), set.as_ptr(), &mut p, &mut n)
    };
    let bytes = unsafe { take_buf(st, p, n) };
    let keys = match loom_codec::decode(&bytes).unwrap() {
        loom_codec::Value::Array(items) => items
            .into_iter()
            .map(|item| match item {
                loom_codec::Value::Text(s) => s,
                other => panic!("expected metadata index key text, got {other:?}"),
            })
            .collect::<Vec<_>>(),
        other => panic!("expected array, got {other:?}"),
    };
    assert_eq!(keys, vec!["lang".to_string()]);

    let st = unsafe {
        loom_vector_drop_metadata_index(
            handle,
            ns.as_ptr(),
            set.as_ptr(),
            lang.as_ptr(),
            &mut changed,
        )
    };
    assert_eq!(st, 0, "drop metadata index failed: {:?}", last_err());
    assert_eq!(
        changed, 1,
        "dropping present metadata index reports changed"
    );
    let st = unsafe {
        loom_vector_drop_metadata_index(
            handle,
            ns.as_ptr(),
            set.as_ptr(),
            lang.as_ptr(),
            &mut changed,
        )
    };
    assert_eq!(st, 0, "repeat drop metadata index failed: {:?}", last_err());
    assert_eq!(changed, 0, "repeat metadata index drop is idempotent");

    let q = floats(&[1.0, 0.0]);
    let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
    let st = unsafe {
        loom_vector_search_cbor(
            handle,
            ns.as_ptr(),
            set.as_ptr(),
            q.as_ptr(),
            q.len(),
            2,
            core::ptr::null(),
            0,
            &mut p,
            &mut n,
        )
    };
    let bytes = unsafe { take_buf(st, p, n) };
    let first_id = match loom_codec::decode(&bytes).unwrap() {
        loom_codec::Value::Array(items) => match &items[0] {
            loom_codec::Value::Array(hit) => match &hit[0] {
                loom_codec::Value::Text(s) => s.clone(),
                other => panic!("expected hit id text, got {other:?}"),
            },
            other => panic!("expected hit array, got {other:?}"),
        },
        other => panic!("expected array, got {other:?}"),
    };
    assert_eq!(first_id, "a", "the nearest neighbour to [1,0] is a");

    let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
    let st = unsafe {
        loom_vector_search_policy_cbor(
            handle,
            ns.as_ptr(),
            set.as_ptr(),
            q.as_ptr(),
            q.len(),
            2,
            core::ptr::null(),
            0,
            1,
            0,
            0,
            1,
            16,
            8,
            &mut p,
            &mut n,
        )
    };
    let bytes = unsafe { take_buf(st, p, n) };
    let first_id = match loom_codec::decode(&bytes).unwrap() {
        loom_codec::Value::Array(items) => match &items[0] {
            loom_codec::Value::Array(hit) => match &hit[0] {
                loom_codec::Value::Text(s) => s.clone(),
                other => panic!("expected policy hit id text, got {other:?}"),
            },
            other => panic!("expected policy hit array, got {other:?}"),
        },
        other => panic!("expected policy array, got {other:?}"),
    };
    assert_eq!(first_id, "a", "policy PQ search returns the nearest hit");

    let mut removed = -1i32;
    let st =
        unsafe { loom_vector_delete(handle, ns.as_ptr(), set.as_ptr(), a.as_ptr(), &mut removed) };
    assert_eq!(st, 0, "delete failed: {:?}", last_err());
    assert_eq!(removed, 1, "a present vector reports removed");

    unsafe { loom_close(handle) };
    let _ = std::fs::remove_file(&dir);
}

#[test]
fn columnar_round_trip_over_the_c_abi() {
    use loom_core::tabular::cell_value;
    let (dir, handle) = open_fresh();
    let ns = cs("col");
    let t = cs("t");

    // Columns [id Int(tag 1), price Text(tag 3)].
    let columns = loom_codec::encode(&loom_codec::Value::Array(vec![
        loom_codec::Value::Array(vec![
            loom_codec::Value::Text("id".into()),
            loom_codec::Value::Uint(1),
        ]),
        loom_codec::Value::Array(vec![
            loom_codec::Value::Text("price".into()),
            loom_codec::Value::Uint(3),
        ]),
    ]))
    .unwrap();
    assert_eq!(
        unsafe {
            loom_columnar_create(
                handle,
                ns.as_ptr(),
                t.as_ptr(),
                columns.as_ptr(),
                columns.len(),
                0,
            )
        },
        0,
        "create failed: {:?}",
        last_err()
    );

    for (id, price) in [(1i64, "10"), (2, "20")] {
        let row = loom_core::tabular::encode_cells(&[Value::Int(id), Value::Text(price.into())]);
        let st = unsafe {
            loom_columnar_append(handle, ns.as_ptr(), t.as_ptr(), row.as_ptr(), row.len())
        };
        assert_eq!(st, 0, "append failed: {:?}", last_err());
    }

    let mut count = 0u64;
    let st = unsafe { loom_columnar_rows(handle, ns.as_ptr(), t.as_ptr(), &mut count) };
    assert_eq!(st, 0, "rows failed: {:?}", last_err());
    assert_eq!(count, 2, "two appended rows");

    let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
    let st = unsafe { loom_columnar_inspect_cbor(handle, ns.as_ptr(), t.as_ptr(), &mut p, &mut n) };
    let bytes = unsafe { take_buf(st, p, n) };
    let inspect = match loom_codec::decode(&bytes).unwrap() {
        loom_codec::Value::Array(items) => items,
        other => panic!("expected inspect array, got {other:?}"),
    };
    assert_eq!(inspect[1], loom_codec::Value::Uint(2));

    let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
    let st = unsafe {
        loom_columnar_source_digest_cbor(handle, ns.as_ptr(), t.as_ptr(), &mut p, &mut n)
    };
    let bytes = unsafe { take_buf(st, p, n) };
    let digest = match loom_codec::decode(&bytes).unwrap() {
        loom_codec::Value::Text(text) => text,
        other => panic!("expected source digest text, got {other:?}"),
    };
    assert!(digest.starts_with("blake3:"));

    let st = unsafe { loom_columnar_compact(handle, ns.as_ptr(), t.as_ptr()) };
    assert_eq!(st, 0, "compact failed: {:?}", last_err());

    // select price where id >= 2 (op tag 5) -> one row.
    let select_cols = loom_codec::encode(&loom_codec::Value::Array(vec![loom_codec::Value::Text(
        "price".into(),
    )]))
    .unwrap();
    let filter = loom_codec::encode(&loom_codec::Value::Array(vec![
        loom_codec::Value::Text("id".into()),
        loom_codec::Value::Uint(5),
        cell_value(&Value::Int(2)),
    ]))
    .unwrap();
    let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
    let st = unsafe {
        loom_columnar_select_cbor(
            handle,
            ns.as_ptr(),
            t.as_ptr(),
            select_cols.as_ptr(),
            select_cols.len(),
            filter.as_ptr(),
            filter.len(),
            &mut p,
            &mut n,
        )
    };
    let bytes = unsafe { take_buf(st, p, n) };
    let row_count = match loom_codec::decode(&bytes).unwrap() {
        loom_codec::Value::Array(rows) => rows.len(),
        other => panic!("expected array, got {other:?}"),
    };
    assert_eq!(row_count, 1, "id >= 2 matches exactly one row");

    let aggregates = loom_codec::encode(&loom_codec::Value::Array(vec![
        loom_codec::Value::Array(vec![loom_codec::Value::Uint(0), loom_codec::Value::Null]),
        loom_codec::Value::Array(vec![
            loom_codec::Value::Uint(2),
            loom_codec::Value::Text("id".into()),
        ]),
        loom_codec::Value::Array(vec![
            loom_codec::Value::Uint(4),
            loom_codec::Value::Text("id".into()),
        ]),
    ]))
    .unwrap();
    let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
    let st = unsafe {
        loom_columnar_aggregate_cbor(
            handle,
            ns.as_ptr(),
            t.as_ptr(),
            aggregates.as_ptr(),
            aggregates.len(),
            core::ptr::null(),
            0,
            &mut p,
            &mut n,
        )
    };
    let bytes = unsafe { take_buf(st, p, n) };
    let values = match loom_codec::decode(&bytes).unwrap() {
        loom_codec::Value::Array(values) => values,
        other => panic!("expected aggregate value array, got {other:?}"),
    };
    assert_eq!(
        values,
        vec![
            cell_value(&Value::U64(2)),
            cell_value(&Value::Int(1)),
            cell_value(&Value::Int(3))
        ]
    );

    unsafe { loom_close(handle) };
    let _ = std::fs::remove_file(&dir);
}

#[test]
fn dataframe_round_trip_over_the_c_abi() {
    let (dir, handle) = open_fresh();
    let ns = cs("df");
    let frame = cs("etl");
    let source_path = cs("events.csv");
    let output_path = cs("out.csv");
    let source = b"id,total\n1,10\n2,20\n";

    let facet = cs("files");
    let mut ns_out: *mut c_char = core::ptr::null_mut();
    let created = unsafe {
        ok_out(
            loom_workspace_create(handle, ns.as_ptr(), facet.as_ptr(), &mut ns_out),
            ns_out,
        )
    };
    assert!(!created.is_empty(), "workspace id returned");

    let st = unsafe {
        loom_write_file(
            handle,
            ns.as_ptr(),
            source_path.as_ptr(),
            source.as_ptr(),
            source.len(),
            0,
        )
    };
    assert_eq!(st, 0, "source write failed: {:?}", last_err());

    let plan = loom_core::DataframePlan::new(vec![
        loom_core::DataframeSourceBinding::new(
            "events",
            loom_core::DataframeSourceKind::Files,
            "events.csv",
            loom_core::DataframeInputFormat::Csv,
        )
        .with_source_digest(Digest::blake3(source)),
    ])
    .unwrap()
    .with_operations(vec![
        loom_core::DataframeOperation::Scan {
            source: "events".to_string(),
        },
        loom_core::DataframeOperation::Select {
            columns: vec!["id".to_string(), "total".to_string()],
        },
    ])
    .unwrap()
    .with_materialization(loom_core::DataframeMaterialization::new(
        loom_core::DataframeMaterializationTarget::Files,
        Some("out.csv".to_string()),
        loom_core::DataframeInputFormat::Csv,
    ))
    .unwrap();
    let plan_bytes = plan.encode();

    let st = unsafe {
        loom_dataframe_create(
            handle,
            ns.as_ptr(),
            frame.as_ptr(),
            plan_bytes.as_ptr(),
            plan_bytes.len(),
        )
    };
    assert_eq!(st, 0, "dataframe create failed: {:?}", last_err());

    let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
    let st =
        unsafe { loom_dataframe_collect_cbor(handle, ns.as_ptr(), frame.as_ptr(), &mut p, &mut n) };
    let bytes = unsafe { take_buf(st, p, n) };
    let batch = loom_codec::decode(&bytes).unwrap();
    let loom_codec::Value::Array(batch_items) = batch else {
        panic!("dataframe batch shape");
    };
    let loom_codec::Value::Array(rows) = &batch_items[1] else {
        panic!("dataframe rows shape");
    };
    assert_eq!(rows.len(), 2, "collect returns two rows");

    let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
    let st = unsafe {
        loom_dataframe_preview_cbor(handle, ns.as_ptr(), frame.as_ptr(), 1, &mut p, &mut n)
    };
    let bytes = unsafe { take_buf(st, p, n) };
    let batch = loom_codec::decode(&bytes).unwrap();
    let loom_codec::Value::Array(batch_items) = batch else {
        panic!("dataframe preview batch shape");
    };
    let loom_codec::Value::Array(rows) = &batch_items[1] else {
        panic!("dataframe preview rows shape");
    };
    assert_eq!(rows.len(), 1, "preview returns one row");

    let mut out: *mut c_char = core::ptr::null_mut();
    let digest = unsafe {
        ok_out(
            loom_dataframe_plan_digest(handle, ns.as_ptr(), frame.as_ptr(), &mut out),
            out,
        )
    };
    assert!(digest.starts_with("blake3:"), "plan digest: {digest}");

    let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
    let st = unsafe {
        loom_dataframe_source_digests_cbor(handle, ns.as_ptr(), frame.as_ptr(), &mut p, &mut n)
    };
    let bytes = unsafe { take_buf(st, p, n) };
    let loom_codec::Value::Array(digests) = loom_codec::decode(&bytes).unwrap() else {
        panic!("source digest shape");
    };
    assert_eq!(digests.len(), 1, "one pinned source digest");

    let mut digest_out: *mut c_char = core::ptr::null_mut();
    let mut has_digest = 0i32;
    let st = unsafe {
        loom_dataframe_materialize(
            handle,
            ns.as_ptr(),
            frame.as_ptr(),
            &mut digest_out,
            &mut has_digest,
        )
    };
    assert_eq!(st, 0, "materialize failed: {:?}", last_err());
    assert_eq!(has_digest, 0, "files materialization has no CAS digest");
    assert!(
        digest_out.is_null(),
        "no digest string for files materialization"
    );

    let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
    let st = unsafe { loom_read_file(handle, ns.as_ptr(), output_path.as_ptr(), &mut p, &mut n) };
    let bytes = unsafe { take_buf(st, p, n) };
    assert_eq!(bytes, b"id,total\n1,10\n2,20\n");

    unsafe { loom_close(handle) };
    let _ = std::fs::remove_file(&dir);
}

#[test]
fn search_round_trip_over_the_c_abi() {
    use loom_codec::Value as Cbor;
    let (dir, handle) = open_fresh();
    let ns = cs("search");
    let name = cs("docs");

    // mapping {"title": [0 text, true stored, false faceted]}.
    let mapping = loom_codec::encode(&Cbor::Map(vec![(
        Cbor::Text("title".into()),
        Cbor::Array(vec![Cbor::Uint(0), Cbor::Bool(true), Cbor::Bool(false)]),
    )]))
    .unwrap();
    assert_eq!(
        unsafe {
            loom_search_create(
                handle,
                ns.as_ptr(),
                name.as_ptr(),
                mapping.as_ptr(),
                mapping.len(),
            )
        },
        0,
        "create failed: {:?}",
        last_err()
    );

    // index d1 with title "hello world".
    let doc = loom_codec::encode(&Cbor::Map(vec![(
        Cbor::Text("title".into()),
        Cbor::Text("hello world".into()),
    )]))
    .unwrap();
    let id = b"d1";
    let st = unsafe {
        loom_search_index(
            handle,
            ns.as_ptr(),
            name.as_ptr(),
            id.as_ptr(),
            id.len(),
            doc.as_ptr(),
            doc.len(),
        )
    };
    assert_eq!(st, 0, "index failed: {:?}", last_err());

    let (mut p, mut n, mut found) = (core::ptr::null_mut(), 0usize, -1i32);
    let st = unsafe {
        loom_search_get(
            handle,
            ns.as_ptr(),
            name.as_ptr(),
            id.as_ptr(),
            id.len(),
            &mut p,
            &mut n,
            &mut found,
        )
    };
    assert_eq!(st, 0, "get failed: {:?}", last_err());
    assert_eq!(found, 1, "an indexed document is found");
    let _ = unsafe { take_buf(st, p, n) };

    // query Match(title, "hello") -> reduced response with one hit.
    let request = loom_codec::encode(&Cbor::Array(vec![
        Cbor::Array(vec![
            Cbor::Uint(0),
            Cbor::Text("title".into()),
            Cbor::Text("hello".into()),
        ]),
        Cbor::Uint(10),
        Cbor::Uint(0),
    ]))
    .unwrap();
    let (mut p, mut n) = (core::ptr::null_mut(), 0usize);
    let st = unsafe {
        loom_search_query_cbor(
            handle,
            ns.as_ptr(),
            name.as_ptr(),
            request.as_ptr(),
            request.len(),
            &mut p,
            &mut n,
        )
    };
    let bytes = unsafe { take_buf(st, p, n) };
    match loom_codec::decode(&bytes).unwrap() {
        Cbor::Array(parts) => {
            assert_eq!(parts[0], Cbor::Bool(true), "the fallback marks reduced");
            match &parts[1] {
                Cbor::Array(hits) => assert_eq!(hits.len(), 1, "one matching document"),
                other => panic!("expected hits array, got {other:?}"),
            }
        }
        other => panic!("expected response array, got {other:?}"),
    }

    let mut removed = -1i32;
    let st = unsafe {
        loom_search_delete(
            handle,
            ns.as_ptr(),
            name.as_ptr(),
            id.as_ptr(),
            id.len(),
            &mut removed,
        )
    };
    assert_eq!(st, 0, "delete failed: {:?}", last_err());
    assert_eq!(removed, 1, "a present document reports removed");

    unsafe { loom_close(handle) };
    let _ = std::fs::remove_file(&dir);
}
