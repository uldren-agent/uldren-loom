//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

/// Borrow a handle pointer as `&`, recording an argument error and returning the error status when
/// null.
macro_rules! handle_ref {
    ($handle:expr, $what:literal) => {
        // SAFETY: caller guarantees `$handle` came from `loom_open` and is live (see each fn's docs).
        match unsafe { $handle.as_ref() } {
            Some(h) => h,
            None => return fail_arg(concat!($what, ": null handle")),
        }
    };
}

macro_rules! handle_mut {
    ($handle:expr, $what:literal) => {
        // SAFETY: caller guarantees `$handle` came from `loom_open` and is live (see each fn's docs).
        match unsafe { $handle.as_mut() } {
            Some(h) => h,
            None => return fail_arg(concat!($what, ": null handle")),
        }
    };
}

/// Borrow a C-string argument as `&str`, recording an argument error and returning when null/invalid.
macro_rules! arg_str {
    ($p:expr, $what:literal) => {
        // SAFETY: caller guarantees `$p` is a valid C string (see each fn's docs).
        match unsafe { cstr($p) } {
            Some(s) => s,
            None => return fail_arg(concat!($what, ": null or non-UTF-8 argument")),
        }
    };
}
