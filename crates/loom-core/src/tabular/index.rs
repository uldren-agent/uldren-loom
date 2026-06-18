//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

// ---- secondary indexes --------------------------------------------------------------------------

/// Order-preserving index key for `row` under index `idx`: the indexed column values then the
/// primary-key values, each [`encode_key_value`]-encoded (self-delimiting). Equal indexed values sort
/// together, and the trailing primary key both disambiguates duplicates and is recoverable as the
/// row-map key (see [`index_row_map_key`]). This is the `(indexed-cols, pk)` index key.
pub(crate) fn encode_index_key(schema: &Schema, idx: &IndexDef, row: &Row) -> Vec<u8> {
    let mut b = Vec::new();
    for &c in &idx.columns {
        encode_key_value(&mut b, &row[c]);
    }
    for &c in &schema.primary_key {
        encode_key_value(&mut b, &row[c]);
    }
    b
}

/// The order-preserving byte prefix that every index key for `values` shares: the leading indexed
/// columns, [`encode_key_value`]-encoded. A scan over an index tree for this prefix returns exactly the
/// entries whose leading indexed columns equal `values` (`values` may be shorter than the index for a
/// leading-column range scan).
pub(crate) fn encode_index_prefix(values: &[Value]) -> Vec<u8> {
    let mut b = Vec::new();
    for v in values {
        encode_key_value(&mut b, v);
    }
    b
}

/// Recover the row-map key (the encoded primary key) embedded in an index key: skip the `nidx` leading
/// indexed-column values, and the remaining bytes are the primary-key encoding the row map is keyed by
/// (both use [`encode_key_value`]), so the matched row can be fetched without decoding the index key.
pub(crate) fn index_row_map_key(index_key: &[u8], nidx: usize) -> Result<Vec<u8>> {
    let mut c = Cur::new(index_key);
    for _ in 0..nidx {
        skip_key_value(&mut c)?;
    }
    Ok(index_key[c.pos..].to_vec())
}

/// A bound for [`index_scan_rows`] over a secondary index's leading column. `Eq` prefix-scans the index
/// (order-independent, any column type); the four range variants use **encoded byte bounds** over the
/// index tree, exact for scalar columns because `encode_key_value` is order-preserving (sign-flipped
/// integers, `total_cmp` floats, byte-stuffed text/bytes, decimal/temporal keys); `All` walks the whole
/// index in order (used for an unbounded scan, or a composite-typed `List`/`Map` column whose encoded
/// order is not its semantic order - the caller filters those).
pub enum IndexBound<'a> {
    /// Exact match on the leading column(s).
    Eq(&'a [Value]),
    /// Leading column strictly greater than the value.
    Gt(&'a Value),
    /// Leading column greater than or equal to the value.
    GtEq(&'a Value),
    /// Leading column strictly less than the value.
    Lt(&'a Value),
    /// Leading column less than or equal to the value.
    LtEq(&'a Value),
    /// Every row, in index order.
    All,
}

/// The smallest byte string strictly greater than every string with `prefix` (its successor): increment
/// the last byte below `0xFF`, dropping trailing `0xFF`s; `None` if `prefix` is all `0xFF` (it is the
/// maximum, so "unbounded above").
fn prefix_successor(prefix: &[u8]) -> Option<Vec<u8>> {
    let mut out = prefix.to_vec();
    while let Some(last) = out.last_mut() {
        if *last < 0xFF {
            *last += 1;
            return Some(out);
        }
        out.pop();
    }
    None
}

/// Scan rows of a table through a durable secondary index **given its raw roots** (no [`crate::Loom`]):
/// the lazy SQL base serves `scan_indexed_data` over its owned read snapshot through this.
/// Equality prefix-scans `index_root`; a range uses encoded `[start, upper)` byte bounds over the index
/// tree (visiting only the matching span, `O(matches * log n)`, not the whole index); `All` walks the
/// index. Each matched index entry's primary key is recovered (`index_row_map_key`) and the row
/// fetched from `rows_root` and decoded, so rows come back in index (then primary-key) order.
pub fn index_scan_rows<S: ObjectStore>(
    store: &S,
    schema: &Schema,
    index_name: &str,
    rows_root: &crate::digest::Digest,
    index_root: &crate::digest::Digest,
    bound: IndexBound<'_>,
) -> Result<Vec<Row>> {
    let idx = schema
        .index(index_name)
        .ok_or_else(|| LoomError::not_found(format!("no index {index_name:?}")))?;
    let nidx = idx.columns.len();

    // Gather the matching index entries (key, _) in index order.
    let entries: Vec<(Vec<u8>, Vec<u8>)> = match bound {
        IndexBound::Eq(values) => {
            crate::prolly::scan_prefix(store, index_root, &encode_index_prefix(values))?
        }
        IndexBound::All => crate::prolly::entries(store, index_root)?,
        // Range: translate the operator to an encoded `[start, upper)` span over the index tree.
        // `start` is the inclusive lower bound, `upper` the exclusive upper bound; either side `None`
        // is unbounded.
        range => {
            let one = |v: &Value| encode_index_prefix(std::slice::from_ref(v));
            let (start, upper) = match range {
                IndexBound::GtEq(v) => (Some(one(v)), None),
                // Strictly greater: skip the whole `== v` span by starting at its successor; no
                // successor (the value's key is the maximum) means nothing is greater - empty.
                IndexBound::Gt(v) => match prefix_successor(&one(v)) {
                    Some(s) => (Some(s), None),
                    None => return Ok(Vec::new()),
                },
                IndexBound::Lt(v) => (None, Some(one(v))),
                // Less-or-equal: exclusive upper at the successor of the `== v` span; no successor
                // means `v` is the maximum, so the upper bound is unbounded (every row qualifies).
                IndexBound::LtEq(v) => (None, prefix_successor(&one(v))),
                IndexBound::Eq(_) | IndexBound::All => unreachable!("handled above"),
            };
            let mut cur = crate::prolly::ProllyCursor::open_range(
                store,
                index_root,
                start.as_deref(),
                upper,
            )?;
            let mut v = Vec::new();
            while let Some(e) = cur.next()? {
                v.push(e);
            }
            v
        }
    };

    let mut out = Vec::new();
    for (index_key, _) in entries {
        let pk_key = index_row_map_key(&index_key, nidx)?;
        if let Some(value) = crate::prolly::get(store, rows_root, &pk_key)? {
            out.push(decode_row(schema, &value)?);
        }
    }
    Ok(out)
}

/// Advance `c` past one [`encode_key_value`]-encoded value without materializing it.
pub(crate) fn skip_key_value(c: &mut Cur) -> Result<()> {
    match c.u8()? {
        0 => {} // Null: rank only
        1 | 6 | 10 => {
            c.u8()?; // Bool / I8 / U8: one body byte
        }
        7 | 11 => {
            c.take(2)?; // I16 / U16
        }
        8 | 12 | 15 | 17 => {
            c.take(4)?; // I32 / U32 / F32 / Date
        }
        2 | 3 | 13 | 18 | 19 => {
            c.take(8)?; // Int / Float / U64 / Time / Timestamp
        }
        9 | 14 | 21 => {
            c.take(16)?; // I128 / U128 / Uuid
        }
        20 => {
            c.take(12)?; // Interval: 4 (months) + 8 (micros)
        }
        23 => {
            c.take(16)?; // Point: two f64
        }
        4 | 5 => skip_orderpreserving_bytes(c)?, // Text / Bytes
        16 => skip_decimal_key(c)?,              // Decimal
        22 => match c.u8()? {
            0 => {
                c.take(4)?; // Inet v4
            }
            1 => {
                c.take(16)?; // Inet v6
            }
            other => return Err(LoomError::corrupt(format!("bad inet family {other}"))),
        },
        24 => {
            let n = c.uvarint()?; // List: length-prefixed key encodings
            for _ in 0..n {
                skip_key_value(c)?;
            }
        }
        25 => {
            let n = c.uvarint()?; // Map: length-prefixed (key bytes, value key)
            for _ in 0..n {
                skip_orderpreserving_bytes(c)?;
                skip_key_value(c)?;
            }
        }
        other => return Err(LoomError::corrupt(format!("bad key value rank {other:#x}"))),
    }
    Ok(())
}

/// Skip an [`super::codec::encode_decimal_key`] body: the sign marker, then (for non-zero) the 8 exponent bytes and
/// the coefficient digits up to their terminator (`0x00` for positive values, `0xFF` for negative).
fn skip_decimal_key(c: &mut Cur) -> Result<()> {
    let term = match c.u8()? {
        0x80 => return Ok(()), // zero
        0x81 => 0x00,          // positive
        0x7F => 0xFF,          // negative
        other => return Err(LoomError::corrupt(format!("bad decimal marker {other:#x}"))),
    };
    c.take(8)?; // exponent
    while c.u8()? != term {} // coefficient digits up to the terminator
    Ok(())
}

/// Skip a byte-stuffed, NUL-terminated body written by [`super::codec::encode_orderpreserving_bytes`]: a lone `0x00`
/// followed by `0x00` ends the body; `0x00` followed by `0xFF` is an escaped data NUL.
fn skip_orderpreserving_bytes(c: &mut Cur) -> Result<()> {
    loop {
        if c.u8()? == 0x00 {
            match c.u8()? {
                0x00 => return Ok(()), // terminator
                0xFF => continue,      // escaped NUL
                _ => return Err(LoomError::corrupt("bad order-preserving byte escape")),
            }
        }
    }
}

/// Build each declared secondary index of `schema` over `rows` as a prolly tree keyed by
/// `encode_index_key` with an empty (non-covering) value, returning `(name, root)` for each non-empty
/// index in schema (name) order. A `unique` index errors if two rows share the indexed values. The
/// resulting trees live in the table's `TABLE` Tree alongside the row map, so they commit,
/// sync, diff, and GC with it transactionally.
pub fn build_indexes<S: ObjectStore>(
    store: &mut S,
    schema: &Schema,
    rows: &[Row],
) -> Result<Vec<(String, crate::digest::Digest)>> {
    if let Some(name) = unique_conflict(schema, rows) {
        return Err(LoomError::invalid(format!(
            "unique index {name:?} violated"
        )));
    }
    let mut out = Vec::new();
    for idx in &schema.indexes {
        let mut kv: Vec<(Vec<u8>, Vec<u8>)> = rows
            .iter()
            .map(|row| (encode_index_key(schema, idx, row), Vec::new()))
            .collect();
        kv.sort_by(|a, b| a.0.cmp(&b.0));
        if let Some(root) = crate::prolly::build(store, &kv)? {
            out.push((idx.name.clone(), root));
        }
    }
    Ok(out)
}

/// The name of the first `unique` index that `rows` violate (two rows sharing the indexed values), or
/// `None`. Shared by the index build (a violation is an error there) and the merge check (a violation
/// is a conflict there).
fn unique_conflict(schema: &Schema, rows: &[Row]) -> Option<String> {
    for idx in schema.indexes.iter().filter(|i| i.unique) {
        let mut seen: BTreeSet<Vec<u8>> = BTreeSet::new();
        for row in rows {
            let prefix = encode_index_prefix(
                &idx.columns
                    .iter()
                    .map(|&c| row[c].clone())
                    .collect::<Vec<_>>(),
            );
            if !seen.insert(prefix) {
                return Some(idx.name.clone());
            }
        }
    }
    None
}

/// Whether the row set at `rows_root` violates any `unique` index. A clean row-level merge can produce
/// such a violation (two branches independently adding rows with the same unique value); the merge
/// reports that as a conflict rather than a hard error.
pub fn rows_violate_unique<S: ObjectStore>(
    store: &S,
    schema: &Schema,
    rows_root: Option<&crate::digest::Digest>,
) -> Result<bool> {
    if !schema.indexes.iter().any(|i| i.unique) {
        return Ok(false);
    }
    let rows: Vec<Row> = match rows_root {
        Some(r) => crate::prolly::entries(store, r)?
            .into_iter()
            .map(|(_, v)| decode_row(schema, &v))
            .collect::<Result<_>>()?,
        None => Vec::new(),
    };
    Ok(unique_conflict(schema, &rows).is_some())
}

/// Build the declared secondary indexes from a stored row-map `root`, used after a row-level merge
/// where the merged rows exist only as a prolly tree (not an in-memory [`Table`]). Equivalent to
/// loading the rows and calling [`build_indexes`].
pub fn build_indexes_from_rows<S: ObjectStore>(
    store: &mut S,
    schema: &Schema,
    rows_root: Option<&crate::digest::Digest>,
) -> Result<Vec<(String, crate::digest::Digest)>> {
    let rows: Vec<Row> = match rows_root {
        Some(r) => crate::prolly::entries(store, r)?
            .into_iter()
            .map(|(_, v)| decode_row(schema, &v))
            .collect::<Result<Vec<_>>>()?,
        None => Vec::new(),
    };
    build_indexes(store, schema, &rows)
}

/// The row-map and per-index roots of a table after an incremental mutation (index roots aligned to
/// `Schema::indexes`; `None` = that structure is empty).
pub type TableRoots = (
    Option<crate::digest::Digest>,
    Vec<Option<crate::digest::Digest>>,
);

/// Incrementally insert or replace `row` in a table's prolly structures, returning the updated row-map
/// root and per-declared-index roots (aligned to `schema.indexes`). Each structure is mutated in
/// `O(log n)` via [`crate::prolly::insert`]/[`crate::prolly::remove`] rather than rebuilt, and the
/// result is identical to staging the equivalent full row set. A `unique` index rejects a colliding
/// indexed value held by a different primary key.
pub fn insert_row<S: ObjectStore>(
    store: &mut S,
    schema: &Schema,
    rows_root: Option<crate::digest::Digest>,
    index_roots: &[Option<crate::digest::Digest>],
    row: &Row,
) -> Result<TableRoots> {
    schema.check_row(row)?;
    let pk: Vec<Value> = schema.primary_key.iter().map(|&i| row[i].clone()).collect();
    let pk_key = encode_pk_values(&pk);
    let row_val = encode_row(row);
    // The row this primary key currently holds (if any): replacing it must retract its old index keys.
    let old = match &rows_root {
        Some(r) => crate::prolly::get(store, r, &pk_key)?,
        None => None,
    };
    let new_rows = Some(crate::prolly::insert(
        store,
        rows_root.as_ref(),
        &pk_key,
        &row_val,
    )?);
    let mut new_index = Vec::with_capacity(schema.indexes.len());
    for (idx, cur) in schema.indexes.iter().zip(index_roots) {
        let mut root = *cur;
        if let Some(old_val) = &old {
            let old_row = decode_row(schema, old_val)?;
            let old_key = encode_index_key(schema, idx, &old_row);
            if let Some(r) = root {
                root = crate::prolly::remove(store, &r, &old_key)?;
            }
        }
        if idx.unique
            && let Some(r) = root
        {
            // After retracting this pk's own entry, any remaining entry with the same indexed-column
            // prefix belongs to a different primary key - a uniqueness violation.
            let prefix = encode_index_prefix(
                &idx.columns
                    .iter()
                    .map(|&c| row[c].clone())
                    .collect::<Vec<_>>(),
            );
            if !crate::prolly::scan_prefix(store, &r, &prefix)?.is_empty() {
                return Err(LoomError::invalid(format!(
                    "unique index {:?} violated",
                    idx.name
                )));
            }
        }
        let new_key = encode_index_key(schema, idx, row);
        root = Some(crate::prolly::insert(store, root.as_ref(), &new_key, &[])?);
        new_index.push(root);
    }
    Ok((new_rows, new_index))
}

/// Incrementally delete the row with primary key `pk` from a table's prolly structures, returning the
/// updated row-map root, per-index roots, and whether a row was present. The inverse of [`insert_row`].
pub fn delete_row<S: ObjectStore>(
    store: &mut S,
    schema: &Schema,
    rows_root: Option<crate::digest::Digest>,
    index_roots: &[Option<crate::digest::Digest>],
    pk: &[Value],
) -> Result<(
    Option<crate::digest::Digest>,
    Vec<Option<crate::digest::Digest>>,
    bool,
)> {
    let pk_key = encode_pk_values(pk);
    let old = match &rows_root {
        Some(r) => crate::prolly::get(store, r, &pk_key)?,
        None => None,
    };
    let Some(old_val) = old else {
        return Ok((rows_root, index_roots.to_vec(), false));
    };
    let new_rows = match &rows_root {
        Some(r) => crate::prolly::remove(store, r, &pk_key)?,
        None => None,
    };
    let old_row = decode_row(schema, &old_val)?;
    let mut new_index = Vec::with_capacity(schema.indexes.len());
    for (idx, cur) in schema.indexes.iter().zip(index_roots) {
        let mut root = *cur;
        let old_key = encode_index_key(schema, idx, &old_row);
        if let Some(r) = root {
            root = crate::prolly::remove(store, &r, &old_key)?;
        }
        new_index.push(root);
    }
    Ok((new_rows, new_index, true))
}
