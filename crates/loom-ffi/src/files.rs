//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

// ---- byte-range file I/O and file handles -------------------------------------------------------

/// Read `len` bytes starting at byte `offset` of file `path`(workspace `ns_name`)
/// into `(*out_ptr, *out_len)` (free with [`loom_bytes_free`]). Reads past the end return fewer bytes
/// (empty at or beyond the end). A missing file is `NOT_FOUND`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `ns_name`/`path` valid C strings; `out_ptr`/`out_len`
/// writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_read_at(
    handle: *mut LoomSession,
    ns_name: *const c_char,
    path: *const c_char,
    offset: u64,
    len: u64,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_read_at");
    let (n, p) = (
        arg_str!(ns_name, "loom_read_at"),
        arg_str!(path, "loom_read_at"),
    );
    match read_at_ns(h, n, p, offset, len) {
        // SAFETY: `out_ptr`/`out_len` are writable per fn docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Write `len` bytes at `content` to byte `offset` of file `path` (workspace `ns_name`, required
/// `facet`), creating the file if absent and zero-filling any gap before `offset`. Returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `ns_name`/`path` valid C strings; `content` null or
/// `len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_write_at(
    handle: *mut LoomSession,
    ns_name: *const c_char,
    path: *const c_char,
    offset: u64,
    content: *const c_uchar,
    len: usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_write_at");
    let (n, p) = (
        arg_str!(ns_name, "loom_write_at"),
        arg_str!(path, "loom_write_at"),
    );
    // SAFETY: caller guarantees `(content, len)` is a readable buffer (see fn docs).
    let bytes = unsafe { byte_slice(content, len) };
    match write_at_ns(h, n, p, offset, bytes) {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// Resize file `path`(workspace `ns_name`) to `size`, zero-extending or dropping
/// bytes. A missing file is created zero-filled to `size`. Returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `ns_name`/`path` valid C strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_truncate_file(
    handle: *mut LoomSession,
    ns_name: *const c_char,
    path: *const c_char,
    size: u64,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_truncate_file");
    let (n, p) = (
        arg_str!(ns_name, "loom_truncate_file"),
        arg_str!(path, "loom_truncate_file"),
    );
    match truncate_ns(h, n, p, size) {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// Open a file handle on `path`(workspace `ns_name`) with `mode` (0 read, 1 write,
/// 2 read-write, 3 append), writing the handle id to `*out_handle`. Returns `0`. The handle binds to an
/// inode and stays valid (across reopens) until [`loom_file_close`].
///
/// # Safety
/// `handle` must be from [`loom_open`]; `ns_name`/`path` valid C strings; `out_handle` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_file_open(
    handle: *mut LoomSession,
    ns_name: *const c_char,
    path: *const c_char,
    mode: u8,
    out_handle: *mut u64,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_file_open");
    let (n, p) = (
        arg_str!(ns_name, "loom_file_open"),
        arg_str!(path, "loom_file_open"),
    );
    match file_open_ns(h, n, p, mode) {
        Ok(id) => {
            if !out_handle.is_null() {
                // SAFETY: `out_handle` is writable per fn docs.
                unsafe { *out_handle = id };
            }
            0
        }
        Err(e) => fail(e),
    }
}

/// Sequentially read up to `len` bytes from file handle `file` at its cursor, advancing it, into
/// `(*out_ptr, *out_len)` (free with [`loom_bytes_free`]). Returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `out_ptr`/`out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_file_read(
    handle: *mut LoomSession,
    file: u64,
    len: u64,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_file_read");
    match file_read_ns(h, file, len) {
        // SAFETY: `out_ptr`/`out_len` are writable per fn docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Positionally read up to `len` bytes at `offset` from file handle `file` without moving its cursor,
/// into `(*out_ptr, *out_len)` (free with [`loom_bytes_free`]). Returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `out_ptr`/`out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_file_read_at(
    handle: *mut LoomSession,
    file: u64,
    offset: u64,
    len: u64,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_file_read_at");
    match file_read_at_ns(h, file, offset, len) {
        // SAFETY: `out_ptr`/`out_len` are writable per fn docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Sequentially write `len` bytes at `content` to file handle `file` at its cursor (or end of file for
/// an append handle), advancing it; writes the byte count to `*out_written`. Returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `content` null or `len` readable bytes; `out_written` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_file_write(
    handle: *mut LoomSession,
    file: u64,
    content: *const c_uchar,
    len: usize,
    out_written: *mut u64,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_file_write");
    // SAFETY: caller guarantees `(content, len)` is a readable buffer (see fn docs).
    let bytes = unsafe { byte_slice(content, len) };
    match file_write_ns(h, file, bytes) {
        Ok(n) => {
            if !out_written.is_null() {
                // SAFETY: `out_written` is writable per fn docs.
                unsafe { *out_written = n };
            }
            0
        }
        Err(e) => fail(e),
    }
}

/// Positionally write `len` bytes at `content` to byte `offset` of file handle `file` without moving its
/// cursor, zero-filling any gap; writes the byte count to `*out_written`. Returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `content` null or `len` readable bytes; `out_written` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_file_write_at(
    handle: *mut LoomSession,
    file: u64,
    offset: u64,
    content: *const c_uchar,
    len: usize,
    out_written: *mut u64,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_file_write_at");
    // SAFETY: caller guarantees `(content, len)` is a readable buffer (see fn docs).
    let bytes = unsafe { byte_slice(content, len) };
    match file_write_at_ns(h, file, offset, bytes) {
        Ok(n) => {
            if !out_written.is_null() {
                // SAFETY: `out_written` is writable per fn docs.
                unsafe { *out_written = n };
            }
            0
        }
        Err(e) => fail(e),
    }
}

/// Resize file handle `file` to `size` bytes (zero-extend or drop). Returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_file_truncate(handle: *mut LoomSession, file: u64, size: u64) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_file_truncate");
    match file_truncate_ns(h, file, size) {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// Flush file handle `file` (a no-op beyond validating the handle; writes already apply per operation).
/// Returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_file_flush(handle: *mut LoomSession, file: u64) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_file_flush");
    match file_flush_ns(h, file) {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// Write file handle `file`'s live size to `*out_size` and POSIX mode to `*out_mode`. Returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `out_size`/`out_mode` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_file_stat(
    handle: *mut LoomSession,
    file: u64,
    out_size: *mut u64,
    out_mode: *mut u32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_file_stat");
    match file_stat_ns(h, file) {
        Ok((size, mode)) => {
            if !out_size.is_null() {
                // SAFETY: `out_size` is writable per fn docs.
                unsafe { *out_size = size };
            }
            if !out_mode.is_null() {
                // SAFETY: `out_mode` is writable per fn docs.
                unsafe { *out_mode = mode };
            }
            0
        }
        Err(e) => fail(e),
    }
}

/// Close file handle `file`, releasing it. When the last handle on an unlinked inode closes, its bytes
/// are reclaimed (delete-on-last-close). Returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_file_close(handle: *mut LoomSession, file: u64) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_file_close");
    match file_close_ns(h, file) {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}
