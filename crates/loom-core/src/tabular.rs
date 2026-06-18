//! The tabular facet substrate: a minimal, dependency-free, `wasm32`-clean versioned-table
//! core: a typed schema, rows keyed by primary key, a metadata **pre-filter** scan, and a canonical
//! encoding so a table is a content-addressable object and therefore versions/syncs like any
//! other Loom state.
//!
//! This is the **scan substrate**: the always-on, browser-capable path. The SQL frontend
//! (GlueSQL's `Store`/`StoreMut` over these tables) sits on top of it; nothing here pulls a
//! non-portable dependency.
//!
//! Two storage forms coexist. [`Table::encode`] writes the whole table as one canonical blob for
//! result payloads, tests, and compact in-process uses. Committed table slots use
//! [`Table::build_rows`]/[`Table::load_rows`]/[`Table::get_row`] to store rows in a prolly tree keyed
//! by primary key, giving row-level structural sharing and point lookups.

use crate::cbor;
use crate::digest::Digest;
use crate::error::{Code, LoomError, Result};
use crate::object::{EntryKind, Object, TreeEntry};
use crate::provider::ObjectStore;
use crate::vcs::{Loom, StagedEntry, normalize_path};
use crate::workspace::WorkspaceId;
use crate::{AclRight, FacetKind};
pub use loom_types::tabular::{
    CmpOp, ColumnType, Row, Value, cell_from, cell_value, encode_cell, encode_cells, key_bytes,
};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

/// The pieces of a staged table the incremental row mutators read.
type TableMutationParts = (Digest, Schema, Option<Digest>, Vec<Option<Digest>>);

/// A declared secondary index: a name and the indexed column
/// indices in key order. The index is realized as a prolly tree (an `index/<name>` entry of the
/// table's `TABLE` Tree) keyed by `(indexed-cols, pk)`, maintained transactionally with the row map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexDef {
    /// Index name (also the `index/<name>` Tree-entry suffix).
    pub name: String,
    /// Indexed column indices, in key order.
    pub columns: Vec<usize>,
    /// Whether the indexed columns must be unique across rows.
    pub unique: bool,
}

/// A table schema: ordered `(name, type)` columns, the primary-key column indices, and any declared
/// secondary indexes. The schema is canonical-encoded (with its indexes) as the `schema` Blob of a
/// table's `TABLE`-entry Tree, so the index set is part of the table's content identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schema {
    /// Ordered columns.
    pub columns: Vec<(String, ColumnType)>,
    /// Primary-key column indices (one or more), in key order.
    pub primary_key: Vec<usize>,
    /// Declared secondary indexes, sorted by name (canonical order).
    pub indexes: Vec<IndexDef>,
}

impl Schema {
    /// Build a schema, validating it is non-empty with an in-range primary key. The schema starts
    /// with no secondary indexes; declare them with [`Schema::with_index`].
    pub fn new(columns: Vec<(String, ColumnType)>, primary_key: Vec<usize>) -> Result<Self> {
        if columns.is_empty() {
            return Err(LoomError::invalid("schema has no columns"));
        }
        if primary_key.is_empty() {
            return Err(LoomError::invalid("schema has no primary key"));
        }
        for &i in &primary_key {
            if i >= columns.len() {
                return Err(LoomError::invalid(format!(
                    "primary-key index {i} out of range"
                )));
            }
        }
        Ok(Self {
            columns,
            primary_key,
            indexes: Vec::new(),
        })
    }

    /// Declare a secondary index named `name` over `columns` (by column name), returning the extended
    /// schema. Indexes are kept sorted by name so the schema has one canonical form. `unique` enforces
    /// that no two rows share the indexed values.
    pub fn with_index(mut self, name: &str, columns: &[&str], unique: bool) -> Result<Self> {
        if name.is_empty() {
            return Err(LoomError::invalid("index name is empty"));
        }
        if self.indexes.iter().any(|i| i.name == name) {
            return Err(LoomError::invalid(format!("duplicate index {name:?}")));
        }
        if columns.is_empty() {
            return Err(LoomError::invalid(format!("index {name:?} has no columns")));
        }
        let cols = columns
            .iter()
            .map(|c| {
                self.column(c)
                    .ok_or_else(|| LoomError::invalid(format!("index {name:?}: no column {c:?}")))
            })
            .collect::<Result<Vec<_>>>()?;
        self.indexes.push(IndexDef {
            name: name.to_string(),
            columns: cols,
            unique,
        });
        self.indexes.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(self)
    }

    /// Number of columns.
    pub fn arity(&self) -> usize {
        self.columns.len()
    }

    /// The index of a column by name.
    pub fn column(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|(n, _)| n == name)
    }

    /// A declared secondary index by name.
    pub fn index(&self, name: &str) -> Option<&IndexDef> {
        self.indexes.iter().find(|i| i.name == name)
    }

    /// Validate a row against this schema (arity and per-column type), the same check
    /// [`Table::insert`] applies. Used by the incremental row-mutation path.
    pub(crate) fn check_row(&self, row: &Row) -> Result<()> {
        if row.len() != self.arity() {
            return Err(LoomError::invalid(format!(
                "row has {} values, schema has {}",
                row.len(),
                self.arity()
            )));
        }
        for (v, (name, ty)) in row.iter().zip(&self.columns) {
            if !v.matches(*ty) {
                return Err(LoomError::invalid(format!(
                    "column {name:?} expects {ty:?}"
                )));
            }
        }
        Ok(())
    }

    /// Canonical schema bytes: the Loom Canonical CBOR array `[columns, primary_key, indexes]`, with
    /// `columns` a list of `[name, type-tag]`, `primary_key` a list of column indices, and `indexes` a
    /// list of `[name, [cols...], unique]`. This is the form stored as the `schema` Blob of a table's
    /// `TABLE`-entry Tree.
    pub fn encode(&self) -> Vec<u8> {
        cbor::encode(&self.to_cbor())
    }

    /// This schema as a Loom Canonical CBOR value (the `schema` Blob payload and the head of a
    /// whole-table frame).
    pub(crate) fn to_cbor(&self) -> cbor::Value {
        use cbor::Value::{Array, Bool, Text, Uint};
        let columns = self
            .columns
            .iter()
            .map(|(name, ty)| Array(vec![Text(name.clone()), Uint(u64::from(ty.tag()))]))
            .collect();
        let primary_key = self.primary_key.iter().map(|&i| Uint(i as u64)).collect();
        // Secondary-index definitions, in canonical (name-sorted) order.
        let indexes = self
            .indexes
            .iter()
            .map(|idx| {
                Array(vec![
                    Text(idx.name.clone()),
                    Array(idx.columns.iter().map(|&c| Uint(c as u64)).collect()),
                    Bool(idx.unique),
                ])
            })
            .collect();
        Array(vec![Array(columns), Array(primary_key), Array(indexes)])
    }

    /// Rebuild a schema from [`Schema::to_cbor`].
    pub(crate) fn from_cbor(value: cbor::Value) -> Result<Self> {
        let mut f = cbor::Fields::new(cbor::as_array(value)?);
        let columns_raw = f.array()?;
        let pk_raw = f.array()?;
        let indexes_raw = f.array()?;
        f.end()?;

        let mut columns = Vec::with_capacity(columns_raw.len());
        for col in columns_raw {
            let mut cf = cbor::Fields::new(cbor::as_array(col)?);
            let name = cf.text()?;
            let ty = ColumnType::from_tag(as_u8(cf.uint()?)?)?;
            cf.end()?;
            columns.push((name, ty));
        }
        let mut primary_key = Vec::with_capacity(pk_raw.len());
        for i in pk_raw {
            primary_key.push(as_usize(cbor::as_uint(i)?)?);
        }
        let mut schema = Schema::new(columns, primary_key)?;
        for idx in indexes_raw {
            let mut xf = cbor::Fields::new(cbor::as_array(idx)?);
            let name = xf.text()?;
            let cols_raw = xf.array()?;
            let unique = xf.bool()?;
            xf.end()?;
            let mut idx_cols = Vec::with_capacity(cols_raw.len());
            for c in cols_raw {
                let col = as_usize(cbor::as_uint(c)?)?;
                if col >= schema.columns.len() {
                    return Err(LoomError::corrupt("index column out of range"));
                }
                idx_cols.push(col);
            }
            schema.indexes.push(IndexDef {
                name,
                columns: idx_cols,
                unique,
            });
        }
        Ok(schema)
    }

    /// Parse a schema from standalone [`Schema::encode`] bytes (a table's `schema` Blob).
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_cbor(cbor::decode(bytes)?)
    }
}

/// A scan pre-filter: evaluated before results are returned.
#[derive(Debug, Clone)]
pub enum Predicate {
    /// Matches every row.
    All,
    /// `column <op> value`.
    Compare {
        /// Column index.
        col: usize,
        /// Operator.
        op: CmpOp,
        /// Right-hand value.
        value: Value,
    },
    /// Both sub-predicates must hold.
    And(Box<Predicate>, Box<Predicate>),
}

impl Predicate {
    fn eval(&self, row: &Row) -> bool {
        match self {
            Predicate::All => true,
            Predicate::And(a, b) => a.eval(row) && b.eval(row),
            Predicate::Compare { col, op, value } => {
                let Some(cell) = row.get(*col) else {
                    return false;
                };
                let ord = cell.cmp(value);
                match op {
                    CmpOp::Eq => ord == Ordering::Equal,
                    CmpOp::Ne => ord != Ordering::Equal,
                    CmpOp::Lt => ord == Ordering::Less,
                    CmpOp::Le => ord != Ordering::Greater,
                    CmpOp::Gt => ord == Ordering::Greater,
                    CmpOp::Ge => ord != Ordering::Less,
                }
            }
        }
    }
}

/// A versioned table: rows keyed by primary key, kept in key order (the prolly-tree stand-in).
#[derive(Debug, Clone)]
pub struct Table {
    schema: Schema,
    rows: BTreeMap<Vec<Value>, Row>,
}

impl Table {
    /// An empty table over `schema`.
    pub fn new(schema: Schema) -> Self {
        Self {
            schema,
            rows: BTreeMap::new(),
        }
    }

    /// The schema.
    pub fn schema(&self) -> &Schema {
        &self.schema
    }

    /// Row count.
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether the table has no rows.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    fn key_of(&self, row: &Row) -> Vec<Value> {
        self.schema
            .primary_key
            .iter()
            .map(|&i| row[i].clone())
            .collect()
    }

    /// Insert (or replace, by primary key) a row, validating arity and column types.
    pub fn insert(&mut self, row: Row) -> Result<()> {
        if row.len() != self.schema.arity() {
            return Err(LoomError::invalid(format!(
                "row has {} values, schema has {}",
                row.len(),
                self.schema.arity()
            )));
        }
        for (v, (name, ty)) in row.iter().zip(&self.schema.columns) {
            if !v.matches(*ty) {
                return Err(LoomError::invalid(format!(
                    "column {name:?} expects {ty:?}"
                )));
            }
        }
        let key = self.key_of(&row);
        self.rows.insert(key, row);
        Ok(())
    }

    /// Fetch a row by its primary-key values.
    pub fn get(&self, pk: &[Value]) -> Option<&Row> {
        self.rows.get(pk)
    }

    /// Remove a row by primary key; returns whether a row was present.
    pub fn delete(&mut self, pk: &[Value]) -> bool {
        self.rows.remove(pk).is_some()
    }

    /// Scan rows matching `filter`, returned in primary-key order (deterministic).
    pub fn scan(&self, filter: &Predicate) -> Vec<&Row> {
        self.rows.values().filter(|r| filter.eval(r)).collect()
    }

    /// Canonical bytes for the whole table: the Loom Canonical CBOR array `[schema, rows]`, where
    /// `rows` lists rows (each a row array of cell values) in primary-key order. Deterministic, so a
    /// table has one byte form and addresses stably.
    pub fn encode(&self) -> Vec<u8> {
        let rows = self
            .rows
            .values()
            .map(|row| cbor::Value::Array(row.iter().map(cell_value).collect()))
            .collect();
        cbor::encode(&cbor::Value::Array(vec![
            self.schema.to_cbor(),
            cbor::Value::Array(rows),
        ]))
    }

    /// Parse a table from [`Table::encode`] output.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let mut f = cbor::Fields::new(cbor::decode_array(bytes)?);
        let schema = Schema::from_cbor(f.next_field()?)?;
        let rows = f.array()?;
        f.end()?;
        let mut table = Table::new(schema);
        let arity = table.schema.arity();
        for r in rows {
            let items = cbor::as_array(r)?;
            if items.len() != arity {
                return Err(LoomError::corrupt("row arity mismatch"));
            }
            let row = items
                .into_iter()
                .map(cell_from)
                .collect::<Result<Vec<_>>>()?;
            table.insert(row)?;
        }
        Ok(table)
    }

    // ---- row-level prolly storage -----------------------------------------------------------------

    /// Store the table's rows as a prolly tree keyed by the encoded primary key, returning the root,
    /// or `None` if the table is empty. Changing one row re-chunks only its leaf and the spine to the
    /// root, so a diff/sync moves `O(changed rows)` nodes. The root is a pure function of the row set.
    pub fn build_rows<S: ObjectStore>(
        &self,
        store: &mut S,
    ) -> Result<Option<crate::digest::Digest>> {
        let mut kv: Vec<(Vec<u8>, Vec<u8>)> = self
            .rows
            .iter()
            .map(|(pk, row)| (encode_pk_values(pk), encode_row(row)))
            .collect();
        // Prolly requires keys sorted ascending by bytes, which is not the BTreeMap value-order.
        kv.sort_by(|a, b| a.0.cmp(&b.0));
        crate::prolly::build(store, &kv)
    }

    /// Rebuild a table from a prolly `root` produced by [`Table::build_rows`] under `schema`.
    pub fn load_rows<S: ObjectStore>(
        store: &S,
        schema: Schema,
        root: &crate::digest::Digest,
    ) -> Result<Self> {
        let mut table = Table::new(schema);
        for (_, value) in crate::prolly::entries(store, root)? {
            let row = decode_row(&table.schema, &value)?;
            table.insert(row)?;
        }
        Ok(table)
    }

    /// Build the table's declared secondary indexes (see [`build_indexes`]). The returned `(name,
    /// root)` pairs become the `index/<name>` entries of the table's `TABLE` Tree, maintained
    /// transactionally with the row map.
    pub fn build_index_roots<S: ObjectStore>(
        &self,
        store: &mut S,
    ) -> Result<Vec<(String, crate::digest::Digest)>> {
        let rows: Vec<Row> = self.rows.values().cloned().collect();
        build_indexes(store, &self.schema, &rows)
    }

    /// Fetch a single row by primary key from a prolly `root` without loading the whole table.
    pub fn get_row<S: ObjectStore>(
        store: &S,
        schema: &Schema,
        root: &crate::digest::Digest,
        pk: &[Value],
    ) -> Result<Option<Row>> {
        match crate::prolly::get(store, root, &encode_pk_values(pk))? {
            Some(value) => Ok(Some(decode_row(schema, &value)?)),
            None => Ok(None),
        }
    }
}

/// The canonical row-map key for a primary key (the prolly key under which [`Table::build_rows`] stores
/// a row). Lets a streaming caller build `[start, upper)` bounds for [`RowCursor::open_range`] from PK
/// values without reaching into the encoding.
pub fn row_map_key(pk: &[Value]) -> Vec<u8> {
    encode_pk_values(pk)
}

/// A lazy, forward cursor over a stored table's rows, ascending by primary key, decoding each row on
/// demand from the prolly row map - the streaming read primitive for larger-than-RAM tables.
/// Memory is bounded by the tree height plus the current leaf, not the table size. Read-only.
pub struct RowCursor<'a, S: ObjectStore> {
    inner: crate::prolly::ProllyCursor<'a, S>,
    schema: &'a Schema,
}

impl<'a, S: ObjectStore> RowCursor<'a, S> {
    /// Every row of the table rooted at `root` (a digest from [`Table::build_rows`] or the engine's
    /// staged table root), ascending by primary key.
    pub fn open(store: &'a S, schema: &'a Schema, root: &crate::digest::Digest) -> Result<Self> {
        Ok(Self {
            inner: crate::prolly::ProllyCursor::open(store, root)?,
            schema,
        })
    }

    /// Rows whose row-map key is in `[start, upper)` (build bounds with [`row_map_key`]); either side
    /// `None` is unbounded. Backs a streaming primary-key range scan.
    pub fn open_range(
        store: &'a S,
        schema: &'a Schema,
        root: &crate::digest::Digest,
        start: Option<&[u8]>,
        upper: Option<Vec<u8>>,
    ) -> Result<Self> {
        Ok(Self {
            inner: crate::prolly::ProllyCursor::open_range(store, root, start, upper)?,
            schema,
        })
    }

    /// The next row in ascending primary-key order, or `None` at the end of the range.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Result<Option<Row>> {
        match self.inner.next()? {
            Some((_, value)) => Ok(Some(decode_row(self.schema, &value)?)),
            None => Ok(None),
        }
    }
}

// ---- row-level diff / merge and secondary indexes ----
mod merge;
pub use merge::*;
mod index;
pub use index::*;

// ---- versioned-table facade over the engine ----------------------------------------------------

/// Stage `table` under `name` in `ns`'s working tree SQL facet as a structured table slot:
/// a `TABLE`-entry `Tree` of `{ schema, rows, index/<name> }`, so the engine versions,
/// branches, merges, and syncs it through the object graph with row-level structural sharing. Call
/// [`Loom::commit`] to snapshot it.
pub fn put_table<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    name: &str,
    table: &Table,
) -> Result<()> {
    loom.stage_table(ns, name, table)
}

/// Load the table named `name` from `ns`'s current working tree, or `NOT_FOUND`.
pub fn get_table<S: ObjectStore>(loom: &Loom<S>, ns: WorkspaceId, name: &str) -> Result<Table> {
    loom.read_table(ns, name)
}

/// Names of the tables staged in `ns`'s working tree, sorted.
pub fn list_tables<S: ObjectStore>(loom: &Loom<S>, ns: WorkspaceId) -> Vec<String> {
    loom.staged_tables(ns)
}

/// Drop the table named `name` from `ns`'s working tree (a staged deletion).
pub fn drop_table<S: ObjectStore>(loom: &mut Loom<S>, ns: WorkspaceId, name: &str) -> Result<()> {
    loom.remove_file(ns, name)
}

// ---- canonical codec helpers --------------------------------------------------------------------

// ---- value/cell/key serialization + `Cur` ----
mod codec;
pub(crate) use codec::{
    Cur, as_u8, as_usize, decode_row, encode_key_value, encode_pk_values, encode_row,
};

// ---- the Loom<S> table engine surface ----
mod engine;

#[cfg(test)]
mod tests;
