//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

// Result view - a decoded, immutable, indexed view over a canonical result buffer.
//
// The C-ABI bindings do NOT decode Loom Canonical CBOR or replicate the cell tag table. They open a
// result buffer once (`loom_result_open`), then read it with indexed, typed accessors backed by the one
// shared decoder (`loom_result::result_view::decode`) and the one faithful cell codec. Scalars come back in
// a compact [`LoomValue`] (one FFI call per cell, not per scalar byte); text/bytes are borrowed pointers
// into the view (valid until `loom_result_close`); 128-bit integers, decimal mantissas, and UUIDs come
// as 16-byte little-endian fields; floats carry both raw bits and a convenience `f64`.
// ---------------------------------------------------------------------------------------------------

/// `LoomValue.tag` values - identical to the shared cell codec's tags (loom-core `tabular::cell_value`).
pub const LOOM_VALUE_NULL: i32 = 0;
pub const LOOM_VALUE_BOOL: i32 = 1;
pub const LOOM_VALUE_INT: i32 = 2;
pub const LOOM_VALUE_FLOAT: i32 = 3;
pub const LOOM_VALUE_TEXT: i32 = 4;
pub const LOOM_VALUE_BYTES: i32 = 5;
pub const LOOM_VALUE_I8: i32 = 6;
pub const LOOM_VALUE_I16: i32 = 7;
pub const LOOM_VALUE_I32: i32 = 8;
pub const LOOM_VALUE_I128: i32 = 9;
pub const LOOM_VALUE_U8: i32 = 10;
pub const LOOM_VALUE_U16: i32 = 11;
pub const LOOM_VALUE_U32: i32 = 12;
pub const LOOM_VALUE_U64: i32 = 13;
pub const LOOM_VALUE_U128: i32 = 14;
pub const LOOM_VALUE_F32: i32 = 15;
pub const LOOM_VALUE_DECIMAL: i32 = 16;
pub const LOOM_VALUE_DATE: i32 = 17;
pub const LOOM_VALUE_TIME: i32 = 18;
pub const LOOM_VALUE_TIMESTAMP: i32 = 19;
pub const LOOM_VALUE_INTERVAL: i32 = 20;
pub const LOOM_VALUE_UUID: i32 = 21;
pub const LOOM_VALUE_INET: i32 = 22;
pub const LOOM_VALUE_POINT: i32 = 23;
pub const LOOM_VALUE_LIST: i32 = 24;
pub const LOOM_VALUE_MAP: i32 = 25;

/// `loom_result_item_kind` values - the kind of one result item (a SQL statement or a reader result).
pub const LOOM_RESULT_SELECT: i32 = 0;
pub const LOOM_RESULT_SELECT_MAP: i32 = 1;
pub const LOOM_RESULT_SHOW_COLUMNS: i32 = 2;
pub const LOOM_RESULT_INSERT: i32 = 3;
pub const LOOM_RESULT_DELETE: i32 = 4;
pub const LOOM_RESULT_UPDATE: i32 = 5;
pub const LOOM_RESULT_DROP_TABLE: i32 = 6;
pub const LOOM_RESULT_CREATE: i32 = 7;
pub const LOOM_RESULT_DROP_FUNCTION: i32 = 8;
pub const LOOM_RESULT_ALTER_TABLE: i32 = 9;
pub const LOOM_RESULT_CREATE_INDEX: i32 = 10;
pub const LOOM_RESULT_DROP_INDEX: i32 = 11;
pub const LOOM_RESULT_START_TRANSACTION: i32 = 12;
pub const LOOM_RESULT_COMMIT: i32 = 13;
pub const LOOM_RESULT_ROLLBACK: i32 = 14;
pub const LOOM_RESULT_SHOW_VARIABLE: i32 = 15;
pub const LOOM_RESULT_ROWS: i32 = 16;
pub const LOOM_RESULT_BLAME: i32 = 17;
pub const LOOM_RESULT_DIFF: i32 = 18;
pub const LOOM_RESULT_COMMIT_LOG: i32 = 19;
pub const LOOM_RESULT_MERGE: i32 = 20;

/// `loom_result_variable_kind` values.
pub const LOOM_VARIABLE_TABLES: i32 = 0;
pub const LOOM_VARIABLE_FUNCTIONS: i32 = 1;
pub const LOOM_VARIABLE_VERSION: i32 = 2;

/// `loom_result_merge_outcome` values.
pub const LOOM_MERGE_UP_TO_DATE: i32 = 0;
pub const LOOM_MERGE_FAST_FORWARD: i32 = 1;
pub const LOOM_MERGE_MERGED: i32 = 2;
pub const LOOM_MERGE_CONFLICTS: i32 = 3;

/// `loom_result_diff_change` values, and the `side` argument of `loom_result_diff_len`/`_cell`:
/// `SIDE_VALUES` is the values of an added/removed row or the **from** side of an update; `SIDE_TO` is
/// the **to** side of an update.
pub const LOOM_DIFF_ADDED: i32 = 0;
pub const LOOM_DIFF_REMOVED: i32 = 1;
pub const LOOM_DIFF_UPDATED: i32 = 2;
pub const LOOM_DIFF_SIDE_VALUES: i32 = 0;
pub const LOOM_DIFF_SIDE_TO: i32 = 1;

/// One decoded scalar. Only the field(s) the `tag` selects are meaningful; the rest are zero. `data` is
/// a borrowed pointer (valid until [`loom_result_close`]): UTF-8 for `TEXT`, raw bytes for `BYTES`, and
/// the canonical CBOR of the value for `LIST`/`MAP` (decode with the shared cell codec if needed).
/// `bytes16` is little-endian for `I128`/`U128`/`UUID` and the decimal mantissa (with `scale`); for
/// `INET` it holds 4 or 16 octets with `uint_val` = 4 or 6. Floats fill `float_val` (convenience) and
/// `bits` (raw IEEE-754); `POINT` uses `float_val`/`bits` for x and `float_val2`/`bits2` for y.
#[repr(C)]
pub struct LoomValue {
    pub tag: i32,
    pub scale: u32,
    pub int_val: i64,
    pub int_val2: i64,
    pub uint_val: u64,
    pub float_val: f64,
    pub float_val2: f64,
    pub bits: u64,
    pub bits2: u64,
    pub bytes16: [u8; 16],
    pub data: *const c_uchar,
    pub data_len: usize,
}

impl LoomValue {
    pub(crate) fn zeroed() -> Self {
        LoomValue {
            tag: LOOM_VALUE_NULL,
            scale: 0,
            int_val: 0,
            int_val2: 0,
            uint_val: 0,
            float_val: 0.0,
            float_val2: 0.0,
            bits: 0,
            bits2: 0,
            bytes16: [0u8; 16],
            data: core::ptr::null(),
            data_len: 0,
        }
    }
}

/// A decoded result buffer. Opaque to C; create with [`loom_result_open`], free with
/// [`loom_result_close`]. Not safe to share across threads concurrently.
pub struct LoomResultView {
    payload: ResultPayload,
    /// Canonical bytes for `LIST`/`MAP` cells, kept alive so [`LoomValue::data`] stays borrowable.
    blobs: RefCell<Vec<Box<[u8]>>>,
}

impl LoomResultView {
    /// Encode a composite cell to canonical bytes, keep them alive in the view, and return a borrowed
    /// pointer + length (stable until the view is closed).
    fn stash(&self, v: &Value) -> (*const c_uchar, usize) {
        let boxed = loom_core::tabular::encode_cell(v).into_boxed_slice();
        let ptr = boxed.as_ptr();
        let len = boxed.len();
        self.blobs.borrow_mut().push(boxed);
        (ptr, len)
    }
}

/// One result item: a SQL statement or the single reader result.
enum Item<'a> {
    Stmt(&'a Statement),
    Reader(&'a Reader),
}

fn item_at(p: &ResultPayload, i: usize) -> Option<Item<'_>> {
    match p {
        ResultPayload::Statements(s) => s.get(i).map(Item::Stmt),
        ResultPayload::Reader(r) => (i == 0).then_some(Item::Reader(r)),
    }
}

fn kind_of(it: &Item) -> i32 {
    match it {
        Item::Stmt(s) => match s {
            Statement::Select { .. } => LOOM_RESULT_SELECT,
            Statement::SelectMap(_) => LOOM_RESULT_SELECT_MAP,
            Statement::ShowColumns(_) => LOOM_RESULT_SHOW_COLUMNS,
            Statement::Insert(_) => LOOM_RESULT_INSERT,
            Statement::Delete(_) => LOOM_RESULT_DELETE,
            Statement::Update(_) => LOOM_RESULT_UPDATE,
            Statement::DropTable(_) => LOOM_RESULT_DROP_TABLE,
            Statement::Create => LOOM_RESULT_CREATE,
            Statement::DropFunction => LOOM_RESULT_DROP_FUNCTION,
            Statement::AlterTable => LOOM_RESULT_ALTER_TABLE,
            Statement::CreateIndex => LOOM_RESULT_CREATE_INDEX,
            Statement::DropIndex => LOOM_RESULT_DROP_INDEX,
            Statement::StartTransaction => LOOM_RESULT_START_TRANSACTION,
            Statement::Commit => LOOM_RESULT_COMMIT,
            Statement::Rollback => LOOM_RESULT_ROLLBACK,
            Statement::ShowVariable(_) => LOOM_RESULT_SHOW_VARIABLE,
        },
        Item::Reader(r) => match r {
            Reader::Rows { .. } => LOOM_RESULT_ROWS,
            Reader::Blame(_) => LOOM_RESULT_BLAME,
            Reader::Diff(_) => LOOM_RESULT_DIFF,
            Reader::CommitLog(_) => LOOM_RESULT_COMMIT_LOG,
            Reader::Merge(_) => LOOM_RESULT_MERGE,
        },
    }
}

/// Fill `out` from a tabular value, borrowing text/bytes from the view (or stashing composite bytes).
fn fill_value(view: &LoomResultView, v: &Value, out: &mut LoomValue) {
    *out = LoomValue::zeroed();
    match v {
        Value::Null => out.tag = LOOM_VALUE_NULL,
        Value::Bool(b) => {
            out.tag = LOOM_VALUE_BOOL;
            out.int_val = i64::from(*b);
        }
        Value::Int(i) => {
            out.tag = LOOM_VALUE_INT;
            out.int_val = *i;
        }
        Value::Float(f) => {
            out.tag = LOOM_VALUE_FLOAT;
            out.float_val = *f;
            out.bits = f.to_bits();
        }
        Value::Text(s) => {
            out.tag = LOOM_VALUE_TEXT;
            out.data = s.as_ptr();
            out.data_len = s.len();
        }
        Value::Bytes(b) => {
            out.tag = LOOM_VALUE_BYTES;
            out.data = b.as_ptr();
            out.data_len = b.len();
        }
        Value::I8(x) => {
            out.tag = LOOM_VALUE_I8;
            out.int_val = i64::from(*x);
        }
        Value::I16(x) => {
            out.tag = LOOM_VALUE_I16;
            out.int_val = i64::from(*x);
        }
        Value::I32(x) => {
            out.tag = LOOM_VALUE_I32;
            out.int_val = i64::from(*x);
        }
        Value::I128(x) => {
            out.tag = LOOM_VALUE_I128;
            out.bytes16 = x.to_le_bytes();
        }
        Value::U8(x) => {
            out.tag = LOOM_VALUE_U8;
            out.uint_val = u64::from(*x);
        }
        Value::U16(x) => {
            out.tag = LOOM_VALUE_U16;
            out.uint_val = u64::from(*x);
        }
        Value::U32(x) => {
            out.tag = LOOM_VALUE_U32;
            out.uint_val = u64::from(*x);
        }
        Value::U64(x) => {
            out.tag = LOOM_VALUE_U64;
            out.uint_val = *x;
        }
        Value::U128(x) => {
            out.tag = LOOM_VALUE_U128;
            out.bytes16 = x.to_le_bytes();
        }
        Value::F32(x) => {
            out.tag = LOOM_VALUE_F32;
            out.float_val = f64::from(*x);
            out.bits = u64::from(x.to_bits());
        }
        Value::Decimal { mantissa, scale } => {
            out.tag = LOOM_VALUE_DECIMAL;
            out.bytes16 = mantissa.to_le_bytes();
            out.scale = *scale;
        }
        Value::Date(d) => {
            out.tag = LOOM_VALUE_DATE;
            out.int_val = i64::from(*d);
        }
        Value::Time(t) => {
            out.tag = LOOM_VALUE_TIME;
            out.uint_val = *t;
        }
        Value::Timestamp(t) => {
            out.tag = LOOM_VALUE_TIMESTAMP;
            out.int_val = *t;
        }
        Value::Interval { months, micros } => {
            out.tag = LOOM_VALUE_INTERVAL;
            out.int_val = i64::from(*months);
            out.int_val2 = *micros;
        }
        Value::Uuid(u) => {
            out.tag = LOOM_VALUE_UUID;
            out.bytes16 = u.to_le_bytes();
        }
        Value::Inet(ip) => {
            out.tag = LOOM_VALUE_INET;
            match ip {
                std::net::IpAddr::V4(a) => {
                    out.uint_val = 4;
                    out.bytes16[..4].copy_from_slice(&a.octets());
                }
                std::net::IpAddr::V6(a) => {
                    out.uint_val = 6;
                    out.bytes16 = a.octets();
                }
            }
        }
        Value::Point { x, y } => {
            out.tag = LOOM_VALUE_POINT;
            out.float_val = *x;
            out.bits = x.to_bits();
            out.float_val2 = *y;
            out.bits2 = y.to_bits();
        }
        Value::List(items) => {
            out.tag = LOOM_VALUE_LIST;
            out.uint_val = items.len() as u64;
            let (p, l) = view.stash(v);
            out.data = p;
            out.data_len = l;
        }
        Value::Map(m) => {
            out.tag = LOOM_VALUE_MAP;
            out.uint_val = m.len() as u64;
            let (p, l) = view.stash(v);
            out.data = p;
            out.data_len = l;
        }
    }
}

/// Borrow a result-view pointer as `&`, or `None` if null.
///
/// # Safety
/// `view` must be null or a pointer from [`loom_result_open`] that is still live.
unsafe fn view_ref<'a>(view: *const LoomResultView) -> Option<&'a LoomResultView> {
    // SAFETY: caller guarantees `view` is from `loom_result_open` and live (see each fn's docs).
    unsafe { view.as_ref() }
}

/// Write a borrowed `(ptr, len)` for a string into the out-pointers and return success.
unsafe fn ok_borrowed(s: &str, out_ptr: *mut *const c_uchar, out_len: *mut usize) -> i32 {
    if !out_ptr.is_null() {
        // SAFETY: caller guarantees `out_ptr` is writable (see each fn's docs).
        unsafe { *out_ptr = s.as_ptr() };
    }
    if !out_len.is_null() {
        // SAFETY: caller guarantees `out_len` is writable (see each fn's docs).
        unsafe { *out_len = s.len() };
    }
    0
}

/// Decode a canonical result buffer into an indexed view; writes an owned handle to `*out`
/// (free with [`loom_result_close`]) and returns `0`.
///
/// # Safety
/// `ptr` must point to `len` readable bytes (or be null when `len == 0`); `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_result_open(
    ptr: *const c_uchar,
    len: usize,
    out: *mut *mut LoomResultView,
) -> i32 {
    clear_error();
    let bytes: &[u8] = if len == 0 {
        &[]
    } else if ptr.is_null() {
        return fail_arg("loom_result_open: null buffer");
    } else {
        // SAFETY: caller guarantees `ptr` is valid for `len` bytes (see fn docs).
        unsafe { core::slice::from_raw_parts(ptr, len) }
    };
    match result_view::decode(bytes) {
        Ok(payload) => {
            if !out.is_null() {
                // SAFETY: caller guarantees `out` is writable (see fn docs).
                unsafe {
                    *out = Box::into_raw(Box::new(LoomResultView {
                        payload,
                        blobs: RefCell::new(Vec::new()),
                    }));
                }
            }
            0
        }
        Err(e) => fail(e),
    }
}

/// Decode a single streamed row - a canonical-CBOR cell array, as yielded by [`loom_iter_next`] - into
/// a [`LoomResultView`] holding exactly that one row (item 0, row 0). The binding then reads its typed
/// cells through the same `loom_result_row_len` / `loom_result_cell` accessors and frees it with
/// [`loom_result_close`]; this is the typed bridge for the streaming iterator, so no binding parses
/// CBOR or replicates the cell tag table. Reuses the one shared cell decoder (`loom-sql`
/// `lookup_cbor::values_from_cbor`).
///
/// # Safety
/// `(ptr, len)` must be a readable buffer (a row from [`loom_iter_next`]); `out` a writable `*mut *mut LoomResultView`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_row_open(
    ptr: *const c_uchar,
    len: usize,
    out: *mut *mut LoomResultView,
) -> i32 {
    clear_error();
    // SAFETY: caller guarantees `(ptr, len)` is a readable buffer (see fn docs).
    let bytes = unsafe { byte_slice(ptr, len) };
    match lookup_cbor::values_from_cbor(bytes) {
        Ok(cells) => {
            if !out.is_null() {
                // A one-row reader result: columns are unnamed (the iterator yields values only), so the
                // binding reads cells positionally via `loom_result_cell(view, 0, 0, col)`.
                let payload = result_view::ResultPayload::Reader(result_view::Reader::Rows {
                    columns: Vec::new(),
                    rows: vec![cells],
                });
                // SAFETY: caller guarantees `out` is writable (see fn docs).
                unsafe {
                    *out = Box::into_raw(Box::new(LoomResultView {
                        payload,
                        blobs: RefCell::new(Vec::new()),
                    }));
                }
            }
            0
        }
        Err(e) => fail(e),
    }
}

/// Free a view from [`loom_result_open`]. Passing null is a no-op.
///
/// # Safety
/// `view` must be a pointer from [`loom_result_open`], not previously freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_result_close(view: *mut LoomResultView) {
    if !view.is_null() {
        // SAFETY: `view` came from `Box::into_raw` in `loom_result_open` (see fn docs).
        drop(unsafe { Box::from_raw(view) });
    }
}

/// The number of result items (SQL statements, or 1 for a reader result). 0 if `view` is null.
///
/// # Safety
/// `view` must be null or from [`loom_result_open`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_result_len(view: *const LoomResultView) -> usize {
    // SAFETY: see fn docs.
    match unsafe { view_ref(view) } {
        Some(v) => match &v.payload {
            ResultPayload::Statements(s) => s.len(),
            ResultPayload::Reader(_) => 1,
        },
        None => 0,
    }
}

/// `1` if this result is a list of SQL statements, `0` if it is a single reader result, `-1` if null.
///
/// # Safety
/// `view` must be null or from [`loom_result_open`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_result_is_statements(view: *const LoomResultView) -> i32 {
    // SAFETY: see fn docs.
    match unsafe { view_ref(view) } {
        Some(v) => i32::from(matches!(v.payload, ResultPayload::Statements(_))),
        None => -1,
    }
}

/// The kind of item `item` (a `LOOM_RESULT_*` value), or `-1` if null / out of range.
///
/// # Safety
/// `view` must be null or from [`loom_result_open`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_result_item_kind(view: *const LoomResultView, item: usize) -> i32 {
    // SAFETY: see fn docs.
    match unsafe { view_ref(view) }.and_then(|v| item_at(&v.payload, item)) {
        Some(it) => kind_of(&it),
        None => -1,
    }
}

/// The column count of item `item` (Select labels, Rows columns, or ShowColumns), else 0.
///
/// # Safety
/// `view` must be null or from [`loom_result_open`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_result_column_count(
    view: *const LoomResultView,
    item: usize,
) -> usize {
    // SAFETY: see fn docs.
    match unsafe { view_ref(view) }.and_then(|v| item_at(&v.payload, item)) {
        Some(Item::Stmt(Statement::Select { labels, .. })) => labels.len(),
        Some(Item::Stmt(Statement::ShowColumns(c))) => c.len(),
        Some(Item::Reader(Reader::Rows { columns, .. })) => columns.len(),
        _ => 0,
    }
}

/// Borrow the name of column `col` of item `item` into `*out_ptr`/`*out_len` (UTF-8, valid until close).
///
/// # Safety
/// `view` must be null or from [`loom_result_open`]; out-pointers writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_result_column_name(
    view: *const LoomResultView,
    item: usize,
    col: usize,
    out_ptr: *mut *const c_uchar,
    out_len: *mut usize,
) -> i32 {
    // SAFETY: see fn docs.
    let Some(it) = (unsafe { view_ref(view) }).and_then(|v| item_at(&v.payload, item)) else {
        return fail_arg("loom_result_column_name: bad view or item");
    };
    let name = match it {
        Item::Stmt(Statement::Select { labels, .. }) => labels.get(col).map(String::as_str),
        Item::Stmt(Statement::ShowColumns(c)) => c.get(col).map(|c| c.name.as_str()),
        Item::Reader(Reader::Rows { columns, .. }) => columns.get(col).map(|c| c.name.as_str()),
        _ => None,
    };
    match name {
        // SAFETY: out-pointers writable per fn docs; pointer borrows the view.
        Some(s) => unsafe { ok_borrowed(s, out_ptr, out_len) },
        None => fail_arg("loom_result_column_name: column out of range"),
    }
}

/// Borrow the type label of column `col` (empty for a Select label, which carries no type).
///
/// # Safety
/// `view` must be null or from [`loom_result_open`]; out-pointers writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_result_column_type(
    view: *const LoomResultView,
    item: usize,
    col: usize,
    out_ptr: *mut *const c_uchar,
    out_len: *mut usize,
) -> i32 {
    // SAFETY: see fn docs.
    let Some(it) = (unsafe { view_ref(view) }).and_then(|v| item_at(&v.payload, item)) else {
        return fail_arg("loom_result_column_type: bad view or item");
    };
    let ty = match it {
        Item::Stmt(Statement::Select { labels, .. }) => labels.get(col).map(|_| ""),
        Item::Stmt(Statement::ShowColumns(c)) => c.get(col).map(|c| c.type_name.as_str()),
        Item::Reader(Reader::Rows { columns, .. }) => {
            columns.get(col).map(|c| c.type_name.as_str())
        }
        _ => None,
    };
    match ty {
        // SAFETY: out-pointers writable per fn docs.
        Some(s) => unsafe { ok_borrowed(s, out_ptr, out_len) },
        None => fail_arg("loom_result_column_type: column out of range"),
    }
}

/// The row count of item `item` (Select / Rows / Blame / SelectMap), else 0. For Diff use
/// [`loom_result_diff_count`].
///
/// # Safety
/// `view` must be null or from [`loom_result_open`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_result_row_count(view: *const LoomResultView, item: usize) -> usize {
    // SAFETY: see fn docs.
    match unsafe { view_ref(view) }.and_then(|v| item_at(&v.payload, item)) {
        Some(Item::Stmt(Statement::Select { rows, .. })) => rows.len(),
        Some(Item::Stmt(Statement::SelectMap(rows))) => rows.len(),
        Some(Item::Reader(Reader::Rows { rows, .. })) => rows.len(),
        Some(Item::Reader(Reader::Blame(rows))) => rows.len(),
        _ => 0,
    }
}

/// The number of cells in row `row` of item `item` (Select / Rows / Blame), else 0.
///
/// # Safety
/// `view` must be null or from [`loom_result_open`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_result_row_len(
    view: *const LoomResultView,
    item: usize,
    row: usize,
) -> usize {
    // SAFETY: see fn docs.
    match unsafe { view_ref(view) }.and_then(|v| item_at(&v.payload, item)) {
        Some(Item::Stmt(Statement::Select { rows, .. })) => rows.get(row).map_or(0, Vec::len),
        Some(Item::Reader(Reader::Rows { rows, .. })) => rows.get(row).map_or(0, Vec::len),
        Some(Item::Reader(Reader::Blame(rows))) => rows.get(row).map_or(0, |r| r.values.len()),
        _ => 0,
    }
}

/// Read cell `(row, col)` of item `item` (Select / Rows / Blame) into `*out`.
///
/// # Safety
/// `view` must be null or from [`loom_result_open`]; `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_result_cell(
    view: *const LoomResultView,
    item: usize,
    row: usize,
    col: usize,
    out: *mut LoomValue,
) -> i32 {
    // SAFETY: see fn docs.
    let Some(v) = (unsafe { view_ref(view) }) else {
        return fail_arg("loom_result_cell: null view");
    };
    let cell = item_at(&v.payload, item).and_then(|it| match it {
        Item::Stmt(Statement::Select { rows, .. }) => rows.get(row).and_then(|r| r.get(col)),
        Item::Reader(Reader::Rows { rows, .. }) => rows.get(row).and_then(|r| r.get(col)),
        Item::Reader(Reader::Blame(rows)) => rows.get(row).and_then(|r| r.values.get(col)),
        _ => None,
    });
    match cell {
        Some(value) => {
            if out.is_null() {
                return fail_arg("loom_result_cell: null out");
            }
            // SAFETY: `out` is writable per fn docs.
            fill_value(v, value, unsafe { &mut *out });
            0
        }
        None => fail_arg("loom_result_cell: row/col out of range"),
    }
}

/// Borrow the commit address of blame row `row` of item `item` into `*out_ptr`/`*out_len`.
///
/// # Safety
/// `view` must be null or from [`loom_result_open`]; out-pointers writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_result_row_commit(
    view: *const LoomResultView,
    item: usize,
    row: usize,
    out_ptr: *mut *const c_uchar,
    out_len: *mut usize,
) -> i32 {
    // SAFETY: see fn docs.
    let commit = (unsafe { view_ref(view) })
        .and_then(|v| item_at(&v.payload, item))
        .and_then(|it| match it {
            Item::Reader(Reader::Blame(rows)) => rows.get(row).map(|r| r.commit.as_str()),
            _ => None,
        });
    match commit {
        // SAFETY: out-pointers writable per fn docs.
        Some(s) => unsafe { ok_borrowed(s, out_ptr, out_len) },
        None => fail_arg("loom_result_row_commit: not a blame row"),
    }
}

/// Write the row count of a count payload (Insert / Delete / Update / DropTable) to `*out`.
///
/// # Safety
/// `view` must be null or from [`loom_result_open`]; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_result_count(
    view: *const LoomResultView,
    item: usize,
    out: *mut u64,
) -> i32 {
    // SAFETY: see fn docs.
    let n = (unsafe { view_ref(view) })
        .and_then(|v| item_at(&v.payload, item))
        .and_then(|it| match it {
            Item::Stmt(Statement::Insert(n) | Statement::Delete(n) | Statement::Update(n)) => {
                Some(*n)
            }
            Item::Stmt(Statement::DropTable(n)) => Some(*n),
            _ => None,
        });
    match n {
        Some(n) => {
            if !out.is_null() {
                // SAFETY: `out` is writable per fn docs.
                unsafe { *out = n };
            }
            0
        }
        None => fail_arg("loom_result_count: not a count payload"),
    }
}

/// The string-list length of item `item`: CommitLog commits; ShowVariable values (1 for Version);
/// Merge paths (conflicts) or the single commit (fast-forward / merged), 0 for up-to-date. Else 0.
///
/// # Safety
/// `view` must be null or from [`loom_result_open`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_result_string_count(
    view: *const LoomResultView,
    item: usize,
) -> usize {
    // SAFETY: see fn docs.
    match unsafe { view_ref(view) }.and_then(|v| item_at(&v.payload, item)) {
        Some(Item::Reader(Reader::CommitLog(c))) => c.len(),
        Some(Item::Stmt(Statement::ShowVariable(sv))) => match sv {
            ShowVariable::Tables(v) | ShowVariable::Functions(v) => v.len(),
            ShowVariable::Version(_) => 1,
        },
        Some(Item::Reader(Reader::Merge(m))) => match m {
            Merge::UpToDate => 0,
            Merge::FastForward(_) | Merge::Merged(_) => 1,
            Merge::Conflicts(p) => p.len(),
        },
        _ => 0,
    }
}

/// Borrow string `i` of item `item` (see [`loom_result_string_count`]) into `*out_ptr`/`*out_len`.
///
/// # Safety
/// `view` must be null or from [`loom_result_open`]; out-pointers writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_result_string(
    view: *const LoomResultView,
    item: usize,
    i: usize,
    out_ptr: *mut *const c_uchar,
    out_len: *mut usize,
) -> i32 {
    // SAFETY: see fn docs.
    let Some(it) = (unsafe { view_ref(view) }).and_then(|v| item_at(&v.payload, item)) else {
        return fail_arg("loom_result_string: bad view or item");
    };
    let s = match it {
        Item::Reader(Reader::CommitLog(c)) => c.get(i).map(String::as_str),
        Item::Stmt(Statement::ShowVariable(sv)) => match sv {
            ShowVariable::Tables(v) | ShowVariable::Functions(v) => v.get(i).map(String::as_str),
            ShowVariable::Version(s) => (i == 0).then_some(s.as_str()),
        },
        Item::Reader(Reader::Merge(m)) => match m {
            Merge::FastForward(c) | Merge::Merged(c) => (i == 0).then_some(c.as_str()),
            Merge::Conflicts(p) => p.get(i).map(String::as_str),
            Merge::UpToDate => None,
        },
        _ => None,
    };
    match s {
        // SAFETY: out-pointers writable per fn docs.
        Some(s) => unsafe { ok_borrowed(s, out_ptr, out_len) },
        None => fail_arg("loom_result_string: index out of range"),
    }
}

/// Write the ShowVariable variable kind (`LOOM_VARIABLE_*`) of item `item` to `*out`.
///
/// # Safety
/// `view` must be null or from [`loom_result_open`]; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_result_variable_kind(
    view: *const LoomResultView,
    item: usize,
    out: *mut i32,
) -> i32 {
    // SAFETY: see fn docs.
    let kind = (unsafe { view_ref(view) })
        .and_then(|v| item_at(&v.payload, item))
        .and_then(|it| match it {
            Item::Stmt(Statement::ShowVariable(sv)) => Some(match sv {
                ShowVariable::Tables(_) => LOOM_VARIABLE_TABLES,
                ShowVariable::Functions(_) => LOOM_VARIABLE_FUNCTIONS,
                ShowVariable::Version(_) => LOOM_VARIABLE_VERSION,
            }),
            _ => None,
        });
    match kind {
        Some(k) => {
            if !out.is_null() {
                // SAFETY: `out` is writable per fn docs.
                unsafe { *out = k };
            }
            0
        }
        None => fail_arg("loom_result_variable_kind: not a ShowVariable"),
    }
}

/// Write the merge outcome (`LOOM_MERGE_*`) of item `item` to `*out`. Read the commit (fast-forward /
/// merged) or conflict paths via [`loom_result_string`].
///
/// # Safety
/// `view` must be null or from [`loom_result_open`]; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_result_merge_outcome(
    view: *const LoomResultView,
    item: usize,
    out: *mut i32,
) -> i32 {
    // SAFETY: see fn docs.
    let outcome = (unsafe { view_ref(view) })
        .and_then(|v| item_at(&v.payload, item))
        .and_then(|it| match it {
            Item::Reader(Reader::Merge(m)) => Some(match m {
                Merge::UpToDate => LOOM_MERGE_UP_TO_DATE,
                Merge::FastForward(_) => LOOM_MERGE_FAST_FORWARD,
                Merge::Merged(_) => LOOM_MERGE_MERGED,
                Merge::Conflicts(_) => LOOM_MERGE_CONFLICTS,
            }),
            _ => None,
        });
    match outcome {
        Some(o) => {
            if !out.is_null() {
                // SAFETY: `out` is writable per fn docs.
                unsafe { *out = o };
            }
            0
        }
        None => fail_arg("loom_result_merge_outcome: not a merge"),
    }
}

/// The number of change entries in a Diff item, else 0.
///
/// # Safety
/// `view` must be null or from [`loom_result_open`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_result_diff_count(view: *const LoomResultView, item: usize) -> usize {
    // SAFETY: see fn docs.
    match unsafe { view_ref(view) }.and_then(|v| item_at(&v.payload, item)) {
        Some(Item::Reader(Reader::Diff(d))) => d.len(),
        _ => 0,
    }
}

/// Write the change kind (`LOOM_DIFF_*`) of diff entry `entry` of item `item` to `*out`.
///
/// # Safety
/// `view` must be null or from [`loom_result_open`]; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_result_diff_change(
    view: *const LoomResultView,
    item: usize,
    entry: usize,
    out: *mut i32,
) -> i32 {
    // SAFETY: see fn docs.
    let change = (unsafe { view_ref(view) })
        .and_then(|v| item_at(&v.payload, item))
        .and_then(|it| match it {
            Item::Reader(Reader::Diff(d)) => d.get(entry).map(|c| match c {
                RowChange::Added(_) => LOOM_DIFF_ADDED,
                RowChange::Removed(_) => LOOM_DIFF_REMOVED,
                RowChange::Updated { .. } => LOOM_DIFF_UPDATED,
            }),
            _ => None,
        });
    match change {
        Some(c) => {
            if !out.is_null() {
                // SAFETY: `out` is writable per fn docs.
                unsafe { *out = c };
            }
            0
        }
        None => fail_arg("loom_result_diff_change: bad diff entry"),
    }
}

/// The cell count on one side of a diff entry: `LOOM_DIFF_SIDE_VALUES` is the added/removed values or
/// the update's **from**; `LOOM_DIFF_SIDE_TO` is the update's **to** (0 for added/removed). Else 0.
///
/// # Safety
/// `view` must be null or from [`loom_result_open`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_result_diff_len(
    view: *const LoomResultView,
    item: usize,
    entry: usize,
    side: i32,
) -> usize {
    // SAFETY: see fn docs.
    let row = (unsafe { view_ref(view) })
        .and_then(|v| item_at(&v.payload, item))
        .and_then(|it| diff_side(it, entry, side));
    row.map_or(0, <[Value]>::len)
}

/// Read cell `col` on `side` of diff entry `entry` of item `item` into `*out`.
///
/// # Safety
/// `view` must be null or from [`loom_result_open`]; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_result_diff_cell(
    view: *const LoomResultView,
    item: usize,
    entry: usize,
    side: i32,
    col: usize,
    out: *mut LoomValue,
) -> i32 {
    // SAFETY: see fn docs.
    let Some(v) = (unsafe { view_ref(view) }) else {
        return fail_arg("loom_result_diff_cell: null view");
    };
    let cell = item_at(&v.payload, item)
        .and_then(|it| diff_side(it, entry, side))
        .and_then(|r| r.get(col));
    match cell {
        Some(value) => {
            if out.is_null() {
                return fail_arg("loom_result_diff_cell: null out");
            }
            // SAFETY: `out` is writable per fn docs.
            fill_value(v, value, unsafe { &mut *out });
            0
        }
        None => fail_arg("loom_result_diff_cell: out of range"),
    }
}

/// The values slice on one side of a diff entry (see the side constants).
fn diff_side<'a>(it: Item<'a>, entry: usize, side: i32) -> Option<&'a [Value]> {
    let Item::Reader(Reader::Diff(d)) = it else {
        return None;
    };
    match (d.get(entry)?, side) {
        (RowChange::Added(vs) | RowChange::Removed(vs), LOOM_DIFF_SIDE_VALUES) => Some(vs),
        (RowChange::Updated { from, .. }, LOOM_DIFF_SIDE_VALUES) => Some(from),
        (RowChange::Updated { to, .. }, LOOM_DIFF_SIDE_TO) => Some(to),
        _ => None,
    }
}

/// The entry count of row `row` of a SelectMap item, else 0.
///
/// # Safety
/// `view` must be null or from [`loom_result_open`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_result_map_len(
    view: *const LoomResultView,
    item: usize,
    row: usize,
) -> usize {
    // SAFETY: see fn docs.
    match unsafe { view_ref(view) }.and_then(|v| item_at(&v.payload, item)) {
        Some(Item::Stmt(Statement::SelectMap(rows))) => rows.get(row).map_or(0, BTreeMap::len),
        _ => 0,
    }
}

/// Read entry `idx` of SelectMap row `row`: borrow the key into `*key_ptr`/`*key_len` and the value
/// into `*out`.
///
/// # Safety
/// `view` must be null or from [`loom_result_open`]; out-pointers writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_result_map_entry(
    view: *const LoomResultView,
    item: usize,
    row: usize,
    idx: usize,
    key_ptr: *mut *const c_uchar,
    key_len: *mut usize,
    out: *mut LoomValue,
) -> i32 {
    // SAFETY: see fn docs.
    let Some(v) = (unsafe { view_ref(view) }) else {
        return fail_arg("loom_result_map_entry: null view");
    };
    let entry = match item_at(&v.payload, item) {
        Some(Item::Stmt(Statement::SelectMap(rows))) => {
            rows.get(row).and_then(|m| m.iter().nth(idx))
        }
        _ => None,
    };
    match entry {
        Some((k, value)) => {
            if out.is_null() {
                return fail_arg("loom_result_map_entry: null out");
            }
            // SAFETY: `out` is writable per fn docs.
            fill_value(v, value, unsafe { &mut *out });
            // SAFETY: key out-pointers writable per fn docs; pointer borrows the view.
            unsafe { ok_borrowed(k, key_ptr, key_len) }
        }
        None => fail_arg("loom_result_map_entry: out of range"),
    }
}

#[cfg(test)]
mod result_view_abi_tests {
    use super::*;
    use loom_core::tabular::{ColumnType, Schema, Table};

    #[test]
    fn open_navigate_and_read_select_cells() {
        let mut s = LoomSqlStore::default();
        s.exec_cbor("CREATE TABLE t (id INTEGER PRIMARY KEY, n TEXT)")
            .unwrap();
        s.exec_cbor("INSERT INTO t VALUES (1,'hi')").unwrap();
        let bytes = s.exec_cbor("SELECT id, n FROM t").unwrap();

        let mut view: *mut LoomResultView = core::ptr::null_mut();
        assert_eq!(
            unsafe { loom_result_open(bytes.as_ptr(), bytes.len(), &mut view) },
            0
        );
        assert_eq!(unsafe { loom_result_len(view) }, 1);
        assert_eq!(unsafe { loom_result_is_statements(view) }, 1);
        assert_eq!(
            unsafe { loom_result_item_kind(view, 0) },
            LOOM_RESULT_SELECT
        );
        assert_eq!(unsafe { loom_result_column_count(view, 0) }, 2);
        assert_eq!(unsafe { loom_result_row_count(view, 0) }, 1);

        let mut val = LoomValue::zeroed();
        assert_eq!(unsafe { loom_result_cell(view, 0, 0, 0, &mut val) }, 0);
        assert_eq!(val.tag, LOOM_VALUE_INT);
        assert_eq!(val.int_val, 1);

        assert_eq!(unsafe { loom_result_cell(view, 0, 0, 1, &mut val) }, 0);
        assert_eq!(val.tag, LOOM_VALUE_TEXT);
        let txt = unsafe { core::slice::from_raw_parts(val.data, val.data_len) };
        assert_eq!(txt, b"hi");

        unsafe { loom_result_close(view) };
    }

    #[test]
    fn u128_decimal_bytes_cross_faithfully() {
        let schema = Schema::new(
            vec![
                ("id".into(), ColumnType::Int),
                ("big".into(), ColumnType::U128),
                ("d".into(), ColumnType::Decimal),
                ("b".into(), ColumnType::Bytes),
            ],
            vec![0],
        )
        .unwrap();
        let mut t = Table::new(schema);
        let big = u128::from(u64::MAX) + 1;
        t.insert(vec![
            Value::Int(1),
            Value::U128(big),
            Value::Decimal {
                mantissa: 12_345,
                scale: 2,
            },
            Value::Bytes(vec![0, 1, 2, 255]),
        ])
        .unwrap();
        let bytes = result_cbor::table_cbor(&t).unwrap();

        let mut view: *mut LoomResultView = core::ptr::null_mut();
        assert_eq!(
            unsafe { loom_result_open(bytes.as_ptr(), bytes.len(), &mut view) },
            0
        );
        assert_eq!(unsafe { loom_result_item_kind(view, 0) }, LOOM_RESULT_ROWS);

        let mut val = LoomValue::zeroed();
        assert_eq!(unsafe { loom_result_cell(view, 0, 0, 1, &mut val) }, 0);
        assert_eq!(val.tag, LOOM_VALUE_U128);
        assert_eq!(u128::from_le_bytes(val.bytes16), big);

        assert_eq!(unsafe { loom_result_cell(view, 0, 0, 2, &mut val) }, 0);
        assert_eq!(val.tag, LOOM_VALUE_DECIMAL);
        assert_eq!(i128::from_le_bytes(val.bytes16), 12_345);
        assert_eq!(val.scale, 2);

        assert_eq!(unsafe { loom_result_cell(view, 0, 0, 3, &mut val) }, 0);
        assert_eq!(val.tag, LOOM_VALUE_BYTES);
        let b = unsafe { core::slice::from_raw_parts(val.data, val.data_len) };
        assert_eq!(b, &[0, 1, 2, 255]);

        unsafe { loom_result_close(view) };
    }
}
