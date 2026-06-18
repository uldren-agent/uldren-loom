//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

// ---- tags ---------------------------------------------------------------------------------------

/// Create tag `tag_name` in workspace `ns_name` at the commit `rev` resolves to
/// (`HEAD`, a branch name, or a digest). A non-empty `message` makes an annotated tag (with `tagger`);
/// an empty `message` makes a lightweight tag. Writes the ref target digest (the commit, or the tag
/// object) to `(*out_ptr)` (free with [`loom_string_free`]) and returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; all string arguments valid C strings; `out_ptr` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_tag_create(
    handle: *mut LoomSession,
    ns_name: *const c_char,
    tag_name: *const c_char,
    rev: *const c_char,
    tagger: *const c_char,
    message: *const c_char,
    out_ptr: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_tag_create");
    let (n, tag, r, tg, m) = (
        arg_str!(ns_name, "loom_tag_create"),
        arg_str!(tag_name, "loom_tag_create"),
        arg_str!(rev, "loom_tag_create"),
        arg_str!(tagger, "loom_tag_create"),
        arg_str!(message, "loom_tag_create"),
    );
    match tag_create_ns(h, n, tag, r, tg, m) {
        // SAFETY: `out_ptr` is writable per fn docs.
        Ok(s) => unsafe { ok_str(out_ptr, &s) },
        Err(e) => fail(e),
    }
}

/// List tag names in workspace `ns_name` as a JSON string array into `(*out_ptr)`
/// (free with [`loom_string_free`]). Returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `ns_name` valid C strings; `out_ptr` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_tag_list(
    handle: *mut LoomSession,
    ns_name: *const c_char,
    out_ptr: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_tag_list");
    let n = arg_str!(ns_name, "loom_tag_list");
    match tag_list_ns(h, n) {
        // SAFETY: `out_ptr` is writable per fn docs.
        Ok(s) => unsafe { ok_str(out_ptr, &s) },
        Err(e) => fail(e),
    }
}

/// Read the raw ref target digest of tag `tag_name`. On success returns `0` and sets `*out_found`: when
/// present, `*out_found = 1` and the digest string is written to `(*out_ptr)` (free with
/// [`loom_string_free`]); when absent, `*out_found = 0` and `*out_ptr` is null.
///
/// # Safety
/// `handle` must be from [`loom_open`]; all string arguments valid C strings; `out_ptr`/`out_found`
/// writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_tag_target(
    handle: *mut LoomSession,
    ns_name: *const c_char,
    tag_name: *const c_char,
    out_ptr: *mut *mut c_char,
    out_found: *mut i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_tag_target");
    let (n, tag) = (
        arg_str!(ns_name, "loom_tag_target"),
        arg_str!(tag_name, "loom_tag_target"),
    );
    match tag_target_ns(h, n, tag) {
        Ok(Some(s)) => {
            if !out_found.is_null() {
                // SAFETY: `out_found` is writable per fn docs.
                unsafe { *out_found = 1 };
            }
            // SAFETY: `out_ptr` is writable per fn docs.
            unsafe { ok_str(out_ptr, &s) }
        }
        Ok(None) => {
            if !out_found.is_null() {
                // SAFETY: `out_found` is writable per fn docs.
                unsafe { *out_found = 0 };
            }
            if !out_ptr.is_null() {
                // SAFETY: `out_ptr` is writable per fn docs.
                unsafe { *out_ptr = core::ptr::null_mut() };
            }
            0
        }
        Err(e) => fail(e),
    }
}

/// Delete tag `tag_name` from workspace `ns_name`. NOT_FOUND if absent. Returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; all string arguments valid C strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_tag_delete(
    handle: *mut LoomSession,
    ns_name: *const c_char,
    tag_name: *const c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_tag_delete");
    let (n, tag) = (
        arg_str!(ns_name, "loom_tag_delete"),
        arg_str!(tag_name, "loom_tag_delete"),
    );
    match tag_delete_ns(h, n, tag) {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// Rename tag `old_name` to `new_name`, preserving its target. NOT_FOUND if `old_name` is absent;
/// ALREADY_EXISTS if `new_name` is taken. Returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; all string arguments valid C strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_tag_rename(
    handle: *mut LoomSession,
    ns_name: *const c_char,
    old_name: *const c_char,
    new_name: *const c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_tag_rename");
    let (n, o, nw) = (
        arg_str!(ns_name, "loom_tag_rename"),
        arg_str!(old_name, "loom_tag_rename"),
        arg_str!(new_name, "loom_tag_rename"),
    );
    match tag_rename_ns(h, n, o, nw) {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}
