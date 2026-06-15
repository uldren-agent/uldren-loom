//! C ABI for Uldren Loom - the stable contract every language binding wraps (Node, JVM, C/C++, WASM).
//!
//! Ownership: every pointer returned by this library is owned by the library and must be freed with
//! [`loom_string_free`]; buffers passed in are borrowed for the duration of the call only.
//!
//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). © Uldren Technologies LLC.

use core::ffi::{c_char, c_uchar};
use std::ffi::CString;

use loom_core::Object;

/// Return the library version as a newly-allocated C string. Free with [`loom_string_free`].
#[unsafe(no_mangle)]
pub extern "C" fn loom_version() -> *mut c_char {
    to_c_string(loom_core::VERSION)
}

/// Compute the Blob content address (`"algo:hex"`) of `len` bytes at `data` and return it as a
/// newly-allocated C string (free with [`loom_string_free`]). Returns null on invalid input.
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
