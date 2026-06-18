//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

// ---- row-level diff & 3-way merge ----------------------------------------------------

/// One row-level change between two table revisions (the prolly row maps).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowDiff {
    /// A row present only in the new revision.
    Added(Row),
    /// A row present in both revisions, with a changed value.
    Updated {
        /// The row in the old revision.
        from: Row,
        /// The row in the new revision.
        to: Row,
    },
    /// A row present only in the old revision.
    Removed(Row),
}

/// One schema-aware table diff record between two table revisions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TableDiffRecord {
    /// The table schema changed, was created, or was removed.
    SchemaChanged {
        /// The old schema, absent when the table was created.
        from: Option<Schema>,
        /// The new schema, absent when the table was removed.
        to: Option<Schema>,
    },
    /// One row-level change under a stable schema.
    Row(RowDiff),
}

/// Row-level diff of two table row maps under `schema`, in `O(changed rows)` via prolly subtree
/// pruning (shared subtrees are skipped). `base`/`other` are row-map roots (`None` = empty table).
pub fn diff_rows<S: ObjectStore>(
    store: &S,
    schema: &Schema,
    base: Option<&crate::digest::Digest>,
    other: Option<&crate::digest::Digest>,
) -> Result<Vec<RowDiff>> {
    let mut out = Vec::new();
    for (_key, bv, ov) in crate::prolly::diff(store, base, other)? {
        match (bv, ov) {
            (None, Some(o)) => out.push(RowDiff::Added(decode_row(schema, &o)?)),
            (Some(b), None) => out.push(RowDiff::Removed(decode_row(schema, &b)?)),
            (Some(b), Some(o)) => out.push(RowDiff::Updated {
                from: decode_row(schema, &b)?,
                to: decode_row(schema, &o)?,
            }),
            (None, None) => {}
        }
    }
    Ok(out)
}

/// The outcome of a row-level 3-way table merge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TableMerge {
    /// A clean merge: the merged row-map root (`None` = the merged table is empty).
    Merged(Option<crate::digest::Digest>),
    /// Rows both sides changed differently from the base (reported with each conflicting row's value).
    Conflicts(Vec<Row>),
}

/// Three-way merge two table revisions (`ours`, `theirs`) against their common `base`, at **row
/// granularity**: a row only one side changed is taken from that side;
/// a row both sides changed identically is kept; a row both sides changed differently is a conflict.
/// On a clean merge the merged row map is built and its root returned. The roots are row-map roots
/// (`None` = empty). Only changed rows are examined (prolly diff), so cost is `O(changed rows)`.
pub fn merge_rows<S: ObjectStore>(
    store: &mut S,
    schema: &Schema,
    base: Option<&crate::digest::Digest>,
    ours: Option<&crate::digest::Digest>,
    theirs: Option<&crate::digest::Digest>,
) -> Result<TableMerge> {
    // What each side changed relative to the base, keyed by the encoded primary key.
    let ours_changed: BTreeMap<Vec<u8>, Option<Vec<u8>>> = crate::prolly::diff(store, base, ours)?
        .into_iter()
        .map(|(k, _b, o)| (k, o))
        .collect();
    let theirs_d = crate::prolly::diff(store, base, theirs)?;

    // Start from ours' full row set, then fold in theirs' changes.
    let mut merged: BTreeMap<Vec<u8>, Vec<u8>> = match ours {
        Some(r) => crate::prolly::entries(store, r)?.into_iter().collect(),
        None => BTreeMap::new(),
    };
    let mut conflicts: Vec<Row> = Vec::new();
    for (key, _base_v, their_v) in theirs_d {
        match ours_changed.get(&key) {
            // ours did not touch this key: take theirs (apply or delete).
            None => match their_v {
                Some(v) => {
                    merged.insert(key, v);
                }
                None => {
                    merged.remove(&key);
                }
            },
            // both sides changed this key: identical change is fine, otherwise a conflict.
            Some(our_v) => {
                if our_v.as_ref() != their_v.as_ref()
                    && let Some(rb) = our_v.clone().or(their_v)
                {
                    conflicts.push(decode_row(schema, &rb)?);
                }
            }
        }
    }
    if !conflicts.is_empty() {
        return Ok(TableMerge::Conflicts(conflicts));
    }
    let kv: Vec<(Vec<u8>, Vec<u8>)> = merged.into_iter().collect();
    Ok(TableMerge::Merged(crate::prolly::build(store, &kv)?))
}

/// Opt-in **cell-level** three-way merge: like [`merge_rows`], but a row both sides modified
/// is reconciled column by column rather than conflicting whole-row - if each column was changed by at
/// most one side (relative to the base), the row auto-merges; only a column both sides changed
/// differently is a conflict. Add/add and delete/modify divergences still conflict (no base row to
/// reconcile against). The default merge is row-level; callers opt in.
pub fn merge_rows_cells<S: ObjectStore>(
    store: &mut S,
    schema: &Schema,
    base: Option<&crate::digest::Digest>,
    ours: Option<&crate::digest::Digest>,
    theirs: Option<&crate::digest::Digest>,
) -> Result<TableMerge> {
    let ours_changed: BTreeMap<Vec<u8>, Option<Vec<u8>>> = crate::prolly::diff(store, base, ours)?
        .into_iter()
        .map(|(k, _b, o)| (k, o))
        .collect();
    let theirs_d = crate::prolly::diff(store, base, theirs)?;
    let mut merged: BTreeMap<Vec<u8>, Vec<u8>> = match ours {
        Some(r) => crate::prolly::entries(store, r)?.into_iter().collect(),
        None => BTreeMap::new(),
    };
    let mut conflicts: Vec<Row> = Vec::new();
    for (key, base_v, their_v) in theirs_d {
        match ours_changed.get(&key) {
            None => match their_v {
                Some(v) => {
                    merged.insert(key, v);
                }
                None => {
                    merged.remove(&key);
                }
            },
            Some(our_v) => {
                if our_v.as_ref() == their_v.as_ref() {
                    continue; // identical change on both sides
                }
                match (&base_v, our_v, &their_v) {
                    // Both modified an existing row: reconcile per column.
                    (Some(b), Some(o), Some(t)) => {
                        let base_row = decode_row(schema, b)?;
                        let our_row = decode_row(schema, o)?;
                        let their_row = decode_row(schema, t)?;
                        match merge_cells(schema, &base_row, &our_row, &their_row) {
                            Some(row) => {
                                merged.insert(key, encode_row(&row));
                            }
                            None => conflicts.push(their_row),
                        }
                    }
                    // Add/add or delete/modify divergence: no base row to reconcile -> conflict.
                    _ => {
                        if let Some(rb) = our_v.clone().or(their_v) {
                            conflicts.push(decode_row(schema, &rb)?);
                        }
                    }
                }
            }
        }
    }
    if !conflicts.is_empty() {
        return Ok(TableMerge::Conflicts(conflicts));
    }
    let kv: Vec<(Vec<u8>, Vec<u8>)> = merged.into_iter().collect();
    Ok(TableMerge::Merged(crate::prolly::build(store, &kv)?))
}

/// Three-way merge two revisions of one row against `base`, column by column: a column only one side
/// changed takes that side; a column both sides changed identically is kept; a column both sides
/// changed differently makes the whole row a conflict (`None`).
fn merge_cells(schema: &Schema, base: &Row, ours: &Row, theirs: &Row) -> Option<Row> {
    let mut out = Vec::with_capacity(schema.arity());
    for i in 0..schema.arity() {
        let (b, o, t) = (&base[i], &ours[i], &theirs[i]);
        let cell = if o == t {
            o
        } else if o == b {
            t
        } else if t == b {
            o
        } else {
            return None; // both sides changed this column differently
        };
        out.push(cell.clone());
    }
    Some(out)
}
