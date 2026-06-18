//! loom-sql - the SQL frontend (GlueSQL) over Loom.
//!
//! [`LoomSqlStore`] implements GlueSQL's `Store` / `StoreMut` (and the marker storage traits, which
//! have working defaults), so `gluesql_core::prelude::Glue` can run `CREATE TABLE` / `INSERT` /
//! `SELECT` / `DELETE` over it. The database **persists into a Loom and commits / branches /
//! checks-out** through the engine ([`LoomSqlStore::persist`] / [`LoomSqlStore::load`]), so a SQL
//! database is **versioned** like any other Loom data.
//!
//! GlueSQL's storage traits are `async`, but our bodies are synchronous (in-memory reads/writes), so
//! each returns immediately with no runtime work - no async runtime is needed inside the store, and
//! GlueSQL is pure-Rust + wasm-capable, so SQL runs identically on native and `wasm32`.
//!
//! **Row-level mapping.** Persistence is **per table**, not one whole-DB blob: a small catalog records
//! the GlueSQL schemas + auto-key counters, and each SQL table maps onto its own
//! [`loom_core::tabular::Table`]. Within that table each SQL row is keyed by its GlueSQL `Key` and
//! projected into typed tabular columns. Because `tabular::Table` encodes rows in primary-key order as
//! content-addressed data, this gives **row-level, per-table granularity**: unchanged tables dedup
//! across commits and a single-row edit is localized to the prolly-tree diff/merge substrate.

// The GlueSQL <-> tabular bridge: converts every SQL value/key/type to and from the tabular
// substrate. Consumed by `persist`/`load` below, which project SQL columns into typed tabular columns.
pub mod lookup_cbor;
pub mod result_cbor;
mod value_map;

use async_trait::async_trait;
use chrono::Utc;
use gluesql_core::ast::{DataType, Expr, IndexOperator, OrderByExpr};
use gluesql_core::data::{Key, Schema, SchemaIndex, SchemaIndexOrd, Value as GValue};
use gluesql_core::error::{
    AlterError, AlterTableError, DeleteError, Error as GError, EvaluateError, ExecuteError,
    FetchError, IndexError, InsertError, Result as GResult, UpdateError, ValidateError, ValueError,
};
use gluesql_core::sqlparser::dialect::PostgreSqlDialect;
use gluesql_core::sqlparser::tokenizer::{Token, Tokenizer};
use gluesql_core::store::{
    AlterTable, CustomFunction, CustomFunctionMut, DataRow, Index, IndexMut, Metadata, Planner,
    RowIter, Store, StoreMut, Transaction,
};
use loom_core::error::{Code, LoomError, Result};
use loom_core::workspace::facet_path;

/// The capability names (0010 section 5) this crate provides, for the capability-contribution overlay: a build
/// that links `loom-sql` supports the `sql` facet. The assembling layer overlays these onto
/// `loom_core::capability::registry()`.
pub fn provided_capabilities() -> &'static [&'static str] {
    &["sql"]
}
use loom_core::tabular::{
    ColumnType as LCol, IndexBound, Row as LRow, RowCursor, Schema as LSchema, Table as LTable,
    Value as LValue,
};
use loom_core::{AclRight, Digest, FacetKind, Loom, ObjectStore, WorkspaceId};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

/// A GlueSQL storage backed by Loom, structured as a **lazy base snapshot + a transaction overlay**
/// (the larger-than-RAM SELECT direction):
///
/// - The **base** is an owned, lock-free read view of the `.loom` captured when the store is opened.
///   Rows are never loaded eagerly; `fetch_data` point-fetches one and `scan_data` streams them on
///   demand from the durable per-table prolly row maps, so a table far larger than RAM can be queried
///   with memory bounded by the tree height, not the row count.
/// - The **overlay** holds only the rows (and schema/index changes) mutated since that snapshot:
///   `Some(row)` upserts, `None` tombstones a delete. Reads merge the overlay over the base; `persist`
///   flushes the overlay's deltas to a separate write loom; a transaction snapshots the (small) overlay,
///   not the whole table.
///
/// A store built with [`LoomSqlStore::default`] has no base (a pure in-memory store - everything lives
/// in the overlay); [`LoomSqlStore::open`] / [`LoomSqlStore::load`] attach a base read snapshot.
#[derive(Default, Clone)]
pub struct LoomSqlStore {
    schemas: BTreeMap<String, Schema>,
    /// The lazy base read snapshot (durable, previously-persisted rows), or `None` for a store with no
    /// backing. The platform acquires the snapshot (native: a lock-free `open_loom_read`; wasm: a byte
    /// snapshot) and hands the engine an owned, type-erased store - see [`LoomSqlStore::open`].
    base: Option<BaseSnapshot>,
    /// Per-table overlay of rows changed since the base snapshot: `Some(row)` is an insert/update,
    /// `None` is a delete tombstone (which shadows a base row). Reads consult this first, then the base.
    overlay: BTreeMap<String, BTreeMap<Key, Option<DataRow>>>,
    /// Per-table next auto-key for `append_data` (keyless tables).
    next_id: BTreeMap<String, i64>,
    /// Primary keys mutated since the last `persist` (per table). At persist, a key whose overlay slot
    /// is `Some` is an upsert (incremental `insert_row`); `None` is a delete (`delete_row`).
    row_dirty: BTreeMap<String, BTreeSet<Key>>,
    /// Tables whose schema or index set changed since the last `persist` (new tables, `CREATE`/`DROP
    /// INDEX`, `ALTER TABLE`); these are re-staged in full so the tabular schema and indexes are rebuilt.
    schema_dirty: BTreeSet<String>,
    /// The pre-`BEGIN` snapshot of the mutable state while an explicit SQL transaction is open (`None`
    /// outside a transaction). `COMMIT` drops it (promoting the working state); `ROLLBACK` restores it
    /// (discarding every change since `BEGIN`). The snapshot copies the **overlay** (the session's
    /// changes) plus the small base metadata - never the whole table's rows - so it is cheap even for a
    /// larger-than-RAM table. Transactions only make sense while the store lives across statements
    /// (inside a `LoomSqlBatch` or one multi-statement `exec`), so a transaction left open at the end of a
    /// per-op exec is rejected by the caller.
    txn: Option<Box<TxnSnapshot>>,
}

/// The lazy base read snapshot: an owned, type-erased read view of the `.loom` plus, per table, the
/// projected tabular schema and durable row-map root captured at open. Cheap to clone (an `Arc` plus
/// small per-table metadata maps - no row data), so a transaction can snapshot it.
#[derive(Clone)]
struct BaseSnapshot {
    /// Owned, lock-free read view of the object store (native: a `FileStore::open_read`; wasm: a byte
    /// snapshot). Type-erased so the SQL store is not generic over the backend; reads dispatch through
    /// the `Arc`.
    store: Arc<dyn ObjectStore + Send + Sync>,
    /// Per-table projected tabular schema at open (decodes base rows). Owned and addressable so a
    /// `scan_data` row stream can borrow it for the iterator's lifetime.
    schemas: BTreeMap<String, LSchema>,
    /// Per-table durable row-map root at open (`None` = the table existed but was empty). An absent key
    /// means the table is not in the base (created this session / never persisted).
    roots: BTreeMap<String, Option<Digest>>,
    /// Per-table durable secondary-index roots at open (`table -> index_name -> index/<name> root`).
    /// An indexed predicate is served from these prolly index trees; an absent entry falls back
    /// to a base row scan.
    index_roots: BTreeMap<String, BTreeMap<String, Digest>>,
}

impl std::fmt::Debug for BaseSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BaseSnapshot")
            .field("tables", &self.schemas.keys().collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for LoomSqlStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoomSqlStore")
            .field("schemas", &self.schemas.keys().collect::<Vec<_>>())
            .field("base", &self.base)
            .field("overlay_tables", &self.overlay.keys().collect::<Vec<_>>())
            .field("in_transaction", &self.txn.is_some())
            .finish_non_exhaustive()
    }
}

/// A copy of [`LoomSqlStore`]'s mutable state captured at `BEGIN` so `ROLLBACK` can restore it. Holds
/// the overlay (the session's changed rows) plus the base metadata and counters - never the base's
/// (possibly larger-than-RAM) row data, which is immutable through the snapshot.
#[derive(Debug, Clone)]
struct TxnSnapshot {
    schemas: BTreeMap<String, Schema>,
    base: Option<BaseSnapshot>,
    overlay: BTreeMap<String, BTreeMap<Key, Option<DataRow>>>,
    next_id: BTreeMap<String, i64>,
    row_dirty: BTreeMap<String, BTreeSet<Key>>,
    schema_dirty: BTreeSet<String>,
}

/// Catalog + per-table base metadata read once at open (no rows): the GlueSQL schemas and auto-key
/// counters, plus the projected tabular schema, row-map root, and durable index roots per table. Used
/// to assemble a [`BaseSnapshot`] (lazy paths) or to seed the eager path.
#[derive(Default)]
struct BaseMeta {
    schemas: BTreeMap<String, Schema>,
    next_id: BTreeMap<String, i64>,
    base_schemas: BTreeMap<String, LSchema>,
    base_roots: BTreeMap<String, Option<Digest>>,
    index_roots: BTreeMap<String, BTreeMap<String, Digest>>,
}

/// Map a Loom engine error into a GlueSQL storage error (used inside the row stream where only a
/// `gluesql_core::error::Error` can be yielded).
fn to_gerr(e: LoomError) -> GError {
    GError::StorageMsg(e.to_string())
}

fn sql_error(e: GError) -> LoomError {
    let code = sql_error_code(&e);
    LoomError::new(code, format!("sql: {e}"))
}

fn sql_error_code(e: &GError) -> Code {
    match e {
        GError::Parser(_) => Code::SqlSyntax,
        GError::Fetch(FetchError::TableNotFound(_))
        | GError::Insert(InsertError::TableNotFound(_))
        | GError::Execute(ExecuteError::TableNotFound(_))
        | GError::AlterTable(AlterTableError::TableNotFound(_))
        | GError::Index(IndexError::TableNotFound(_) | IndexError::ConflictTableNotFound(_)) => {
            Code::SqlTableNotFound
        }
        GError::Alter(e) => match e.as_ref() {
            AlterError::TableNotFound(_)
            | AlterError::CtasSourceTableNotFound(_)
            | AlterError::ReferencedTableNotFound(_) => Code::SqlTableNotFound,
            AlterError::TableAlreadyExists(_)
            | AlterError::FunctionAlreadyExists(_)
            | AlterError::DuplicateColumnName(_)
            | AlterError::DuplicateArgName(_)
            | AlterError::ReferencingNonPKColumn { .. }
            | AlterError::CannotDropTableWithReferencing { .. }
            | AlterError::CannotAlterReferencedColumn { .. }
            | AlterError::CannotAlterReferencingColumn { .. } => Code::SqlConstraintViolation,
            AlterError::ForeignKeyDataTypeMismatch { .. }
            | AlterError::UnsupportedDataTypeForUniqueColumn(_, _) => Code::SqlTypeMismatch,
            _ => Code::SqlExecutionFailed,
        },
        GError::Validate(
            ValidateError::DuplicateEntryOnUniqueField(_, _)
            | ValidateError::DuplicateEntryOnPrimaryKeyField(_),
        )
        | GError::Insert(InsertError::CannotFindReferencedValue { .. })
        | GError::Update(UpdateError::CannotFindReferencedValue { .. })
        | GError::Delete(DeleteError::ReferencingColumnExists(_)) => Code::SqlConstraintViolation,
        GError::Value(e) => match e.as_ref() {
            ValueError::NullValueOnNotNullField => Code::SqlConstraintViolation,
            _ => Code::SqlTypeMismatch,
        },
        GError::Evaluate(
            EvaluateError::FunctionRequiresStringValue(_)
            | EvaluateError::FunctionRequiresIntegerOrStringValue(_)
            | EvaluateError::ArrowBaseRequiresMapOrList
            | EvaluateError::ArrowSelectorRequiresIntegerOrString(_)
            | EvaluateError::FunctionRequiresIntegerValue(_)
            | EvaluateError::FunctionRequiresFloatOrIntegerValue(_)
            | EvaluateError::FunctionRequiresUSizeValue(_)
            | EvaluateError::FunctionRequiresFloatValue(_)
            | EvaluateError::FunctionRequiresMapValue(_)
            | EvaluateError::FunctionRequiresPointValue(_)
            | EvaluateError::FunctionRequiresDateOrDateTimeValue(_)
            | EvaluateError::FunctionRequiresStrOrListOrMapValue(_)
            | EvaluateError::BooleanTypeRequired(_)
            | EvaluateError::MapOrListTypeRequired
            | EvaluateError::MapTypeRequired
            | EvaluateError::ListTypeRequired
            | EvaluateError::MapOrStringValueRequired(_)
            | EvaluateError::TextLiteralRequired(_)
            | EvaluateError::IncompatibleBitOperation(_, _)
            | EvaluateError::UnsupportedBinaryOperation { .. }
            | EvaluateError::LikeOnNonStringLiteral { .. }
            | EvaluateError::NumberParseFailed { .. }
            | EvaluateError::NumberCastFailed { .. }
            | EvaluateError::TextParseFailed { .. }
            | EvaluateError::TextCastFailed { .. },
        ) => Code::SqlTypeMismatch,
        _ => Code::SqlExecutionFailed,
    }
}

/// Reconstruct a GlueSQL [`DataRow`] from a stored tabular row (`__key` column first): a schemaless
/// table carries its payload in a single `__map` column, a normal table in the typed columns after the
/// key. The inverse of [`project_row`], shared by the lazy base reads and the persist full re-stage.
fn data_row_from_tabular(schemaless: bool, row: &[LValue]) -> Result<DataRow> {
    if schemaless {
        match row.get(1) {
            Some(LValue::Map(m)) => {
                let mut mm = BTreeMap::new();
                for (k, v) in m {
                    mm.insert(k.clone(), value_map::value_from_tabular(v)?);
                }
                Ok(DataRow::Map(mm))
            }
            _ => Err(LoomError::corrupt(
                "schemaless sql row lacks its map column",
            )),
        }
    } else {
        let mut values = Vec::with_capacity(row.len().saturating_sub(1));
        for v in &row[1..] {
            values.push(value_map::value_from_tabular(v)?);
        }
        Ok(DataRow::Vec(values))
    }
}

/// The forward state of a lazy `scan_data` row stream: drain the base cursor (skipping rows the overlay
/// shadows), then drain the overlay's live (non-tombstone) rows. Borrows the store and schema from the
/// base for the iterator's lifetime, so memory stays bounded by the tree height, not the row count.
struct ScanState<'a> {
    cursor: Option<RowCursor<'a, Arc<dyn ObjectStore + Send + Sync>>>,
    schemaless: bool,
    overlay: Option<&'a BTreeMap<Key, Option<DataRow>>>,
    overlay_iter: Option<std::collections::btree_map::Iter<'a, Key, Option<DataRow>>>,
}

#[async_trait]
impl Store for LoomSqlStore {
    async fn fetch_schema(&self, table_name: &str) -> GResult<Option<Schema>> {
        Ok(self.schemas.get(table_name).cloned())
    }
    async fn fetch_all_schemas(&self) -> GResult<Vec<Schema>> {
        Ok(self.schemas.values().cloned().collect())
    }
    async fn fetch_data(&self, table_name: &str, key: &Key) -> GResult<Option<DataRow>> {
        // Overlay first (an upsert or a delete tombstone), then the lazy base.
        if let Some(slot) = self.overlay.get(table_name).and_then(|t| t.get(key)) {
            return Ok(slot.clone());
        }
        self.base_fetch(table_name, key).map_err(to_gerr)
    }
    async fn scan_data<'a>(&'a self, table_name: &str) -> GResult<RowIter<'a>> {
        // A lazy two-phase stream: stream the base rows on demand (skipping any the overlay shadows),
        // then the overlay's own live rows. Never materializes the whole table.
        let overlay = self.overlay.get(table_name);
        let cursor = match &self.base {
            Some(base) => match (base.schemas.get(table_name), base.roots.get(table_name)) {
                (Some(schema), Some(Some(root))) => {
                    Some(RowCursor::open(&base.store, schema, root).map_err(to_gerr)?)
                }
                _ => None, // table not in the base, or staged-but-empty
            },
            None => None,
        };
        let schemaless = self
            .schemas
            .get(table_name)
            .map(|s| s.column_defs.is_none())
            .unwrap_or(false);
        let state = ScanState {
            cursor,
            schemaless,
            overlay,
            overlay_iter: None,
        };
        let stream = futures::stream::unfold(state, |mut st| async move {
            // Phase 1: drain the base cursor, skipping rows the overlay shadows. Each pull is scoped so
            // the cursor borrow ends before `st` is moved into the yielded tuple.
            loop {
                let pulled = st.cursor.as_mut().map(|cur| cur.next());
                match pulled {
                    None => break,
                    Some(Err(e)) => {
                        st.cursor = None;
                        return Some((Err(to_gerr(e)), st));
                    }
                    Some(Ok(None)) => {
                        st.cursor = None;
                        break;
                    }
                    Some(Ok(Some(row))) => {
                        let key = match value_map::key_from_tabular(&row[0]) {
                            Ok(k) => k,
                            Err(e) => return Some((Err(to_gerr(e)), st)),
                        };
                        if st.overlay.is_some_and(|o| o.contains_key(&key)) {
                            continue; // an overlay update/delete shadows this base row
                        }
                        match data_row_from_tabular(st.schemaless, &row) {
                            Ok(dr) => return Some((Ok((key, dr)), st)),
                            Err(e) => return Some((Err(to_gerr(e)), st)),
                        }
                    }
                }
            }
            // Phase 2: drain the overlay's live (non-tombstone) rows.
            if st.overlay_iter.is_none() {
                st.overlay_iter = st.overlay.map(|o| o.iter());
            }
            let next_item = {
                let mut found = None;
                if let Some(it) = st.overlay_iter.as_mut() {
                    for (k, slot) in it.by_ref() {
                        if let Some(row) = slot {
                            found = Some((k.clone(), row.clone()));
                            break;
                        }
                    }
                }
                found
            };
            next_item.map(|item| (Ok(item), st))
        });
        Ok(Box::pin(stream))
    }
}

#[async_trait]
impl StoreMut for LoomSqlStore {
    async fn insert_schema(&mut self, schema: &Schema) -> GResult<()> {
        self.schemas
            .insert(schema.table_name.clone(), schema.clone());
        self.overlay.entry(schema.table_name.clone()).or_default();
        self.schema_dirty.insert(schema.table_name.clone()); // new/changed schema -> full re-stage
        Ok(())
    }
    async fn delete_schema(&mut self, table_name: &str) -> GResult<()> {
        self.schemas.remove(table_name);
        self.overlay.remove(table_name);
        self.next_id.remove(table_name);
        self.row_dirty.remove(table_name);
        self.schema_dirty.remove(table_name); // dropped tables are unstaged by the persist scan
        // Forget the base entry too, so a table re-created with the same name never reads stale base
        // rows (the base is otherwise immutable; this drop is captured by the transaction snapshot).
        if let Some(base) = &mut self.base {
            base.schemas.remove(table_name);
            base.roots.remove(table_name);
        }
        Ok(())
    }
    async fn append_data(&mut self, table_name: &str, rows: Vec<DataRow>) -> GResult<()> {
        let id = self.next_id.entry(table_name.to_string()).or_insert(0);
        let overlay = self.overlay.entry(table_name.to_string()).or_default();
        let dirty = self.row_dirty.entry(table_name.to_string()).or_default();
        for row in rows {
            let key = Key::I64(*id);
            overlay.insert(key.clone(), Some(row));
            dirty.insert(key);
            *id += 1;
        }
        Ok(())
    }
    async fn insert_data(&mut self, table_name: &str, rows: Vec<(Key, DataRow)>) -> GResult<()> {
        let overlay = self.overlay.entry(table_name.to_string()).or_default();
        let dirty = self.row_dirty.entry(table_name.to_string()).or_default();
        for (k, r) in rows {
            overlay.insert(k.clone(), Some(r));
            dirty.insert(k);
        }
        Ok(())
    }
    async fn delete_data(&mut self, table_name: &str, keys: Vec<Key>) -> GResult<()> {
        let overlay = self.overlay.entry(table_name.to_string()).or_default();
        let dirty = self.row_dirty.entry(table_name.to_string()).or_default();
        for k in keys {
            // A tombstone (not a bare removal) so it shadows any base row at this key.
            overlay.insert(k.clone(), None);
            dirty.insert(k);
        }
        Ok(())
    }
}

// Indexed read path: an indexed equality/prefix/range predicate the planner (below) routes here
// is served by scanning the **durable** prolly secondary index over the base read snapshot - the
// equality case prefix-scans the `index/<name>` tree (`O(matches * log n)`); other predicates walk the
// index in order and filter; a table with no durable index tree falls back to a base row scan. Overlay
// changes are merged in (a base row the overlay shadows is dropped; overlay rows are evaluated against
// the predicate), so the index result reflects uncommitted session writes.
#[async_trait]
impl Index for LoomSqlStore {
    async fn scan_indexed_data<'a>(
        &'a self,
        table_name: &str,
        index_name: &str,
        asc: Option<bool>,
        cmp_value: Option<(&IndexOperator, GValue)>,
    ) -> GResult<RowIter<'a>> {
        let mut rows = self
            .indexed_rows(table_name, index_name, cmp_value)
            .map_err(to_gerr)?;
        // Return rows in indexed-column order so any ordering the planner attributed to the index holds
        // (ascending unless the plan asked for descending). The sort key is the tabular indexed value,
        // which has a total order ([`loom_core::tabular::Value`] is `Ord`). GlueSQL re-sorts for an
        // explicit ORDER BY, so ties may be in any order.
        let descending = asc == Some(false);
        rows.sort_by(|a, b| {
            let ord = a.0.cmp(&b.0);
            if descending { ord.reverse() } else { ord }
        });
        let out: Vec<GResult<(Key, DataRow)>> =
            rows.into_iter().map(|(_, k, r)| Ok((k, r))).collect();
        Ok(Box::pin(futures::stream::iter(out)))
    }
}

/// The bare-identifier column an index keys on, or `None` for an expression index (which has no durable
/// single-column tree and so cannot be served here).
fn indexed_column_name<'a>(gschema: &'a Schema, index_name: &str) -> Option<&'a str> {
    let si = gschema.indexes.iter().find(|i| i.name == index_name)?;
    match &si.expr {
        Expr::Identifier(name) => Some(name.as_str()),
        _ => None,
    }
}

/// The tabular value of GlueSQL column `col` in `drow` (the indexed column's value), or `None` if absent.
/// Used for the small overlay side; the base side reads the value straight off the tabular row.
fn overlay_indexed_value(gschema: &Schema, col: &str, drow: &DataRow) -> Result<Option<LValue>> {
    let g = match drow {
        DataRow::Vec(values) => gschema
            .column_defs
            .as_ref()
            .and_then(|defs| defs.iter().position(|c| c.name == col))
            .and_then(|pos| values.get(pos)),
        DataRow::Map(m) => m.get(col),
    };
    match g {
        Some(v) => Ok(Some(value_map::value_to_tabular(v)?)),
        None => Ok(None),
    }
}

/// Whether `idxval` satisfies the indexed predicate `col OP target` (`cmp = None` = an unbounded full
/// index scan, which matches every row). Compared on the tabular value's total order.
fn index_cmp_matches(idxval: &LValue, cmp: &Option<(IndexOperator, LValue)>) -> bool {
    use std::cmp::Ordering::{Equal, Greater, Less};
    let Some((op, target)) = cmp else {
        return true;
    };
    let ord = idxval.cmp(target);
    match op {
        IndexOperator::Eq => ord == Equal,
        IndexOperator::Gt => ord == Greater,
        IndexOperator::GtEq => ord != Less,
        IndexOperator::Lt => ord == Less,
        IndexOperator::LtEq => ord != Greater,
    }
}

#[async_trait]
impl IndexMut for LoomSqlStore {
    async fn create_index(
        &mut self,
        table_name: &str,
        index_name: &str,
        column: &OrderByExpr,
    ) -> GResult<()> {
        let schema = self.schemas.get_mut(table_name).ok_or_else(|| {
            GError::StorageMsg(format!("create_index: table {table_name:?} not found"))
        })?;
        if schema.indexes.iter().any(|i| i.name == index_name) {
            return Err(GError::StorageMsg(format!(
                "create_index: index {index_name:?} already exists"
            )));
        }
        let order = match column.asc {
            Some(true) => SchemaIndexOrd::Asc,
            Some(false) => SchemaIndexOrd::Desc,
            None => SchemaIndexOrd::Both,
        };
        schema.indexes.push(SchemaIndex {
            name: index_name.to_owned(),
            expr: column.expr.clone(),
            order,
            created: Utc::now().naive_utc(),
        });
        self.schema_dirty.insert(table_name.to_owned()); // index set changed -> full re-stage
        Ok(())
    }

    async fn drop_index(&mut self, table_name: &str, index_name: &str) -> GResult<()> {
        let schema = self.schemas.get_mut(table_name).ok_or_else(|| {
            GError::StorageMsg(format!("drop_index: table {table_name:?} not found"))
        })?;
        let before = schema.indexes.len();
        schema.indexes.retain(|i| i.name != index_name);
        if schema.indexes.len() == before {
            return Err(GError::StorageMsg(format!(
                "drop_index: index {index_name:?} not found"
            )));
        }
        self.schema_dirty.insert(table_name.to_owned()); // index set changed -> full re-stage
        Ok(())
    }
}
impl Metadata for LoomSqlStore {}
impl AlterTable for LoomSqlStore {}

// Real SQL transaction semantics over the in-memory store (replacing GlueSQL's no-op default). GlueSQL
// calls `begin(true)` before every non-transaction statement, `begin(false)` for an explicit `BEGIN`,
// and routes `COMMIT`/`ROLLBACK` straight to `commit`/`rollback` (executor `execute`/`execute_inner`).
// `BEGIN` snapshots, `COMMIT` promotes, `ROLLBACK` restores. Nested transactions and a bare
// `COMMIT`/`ROLLBACK` are rejected, never silently accepted.
#[async_trait]
impl Transaction for LoomSqlStore {
    async fn begin(&mut self, autocommit: bool) -> GResult<bool> {
        if autocommit {
            // A normal statement. The store mutates in place and is made durable by the caller's
            // `persist` (per-op exec, or `LoomSqlBatch` commit), so GlueSQL must never auto-wrap the
            // statement in a commit/rollback - whether or not an explicit transaction is open. Always
            // report "not autocommitting" (Ok(false)), matching the prior behavior for normal SQL.
            return Ok(false);
        }
        if self.txn.is_some() {
            return Err(GError::StorageMsg(
                "nested transactions are not supported (a transaction is already open)".to_owned(),
            ));
        }
        self.txn = Some(Box::new(self.snapshot()));
        Ok(true)
    }

    async fn rollback(&mut self) -> GResult<()> {
        match self.txn.take() {
            Some(snap) => {
                self.restore(*snap);
                Ok(())
            }
            None => Err(GError::StorageMsg(
                "ROLLBACK without an active transaction".to_owned(),
            )),
        }
    }

    async fn commit(&mut self) -> GResult<()> {
        // `commit` is only reached via an explicit `COMMIT` statement (the autocommit path never calls
        // it, since `begin(true)` returns Ok(false)), so a `commit` with no open transaction is a bare
        // `COMMIT` and is rejected.
        if self.txn.take().is_some() {
            Ok(())
        } else {
            Err(GError::StorageMsg(
                "COMMIT without an active transaction".to_owned(),
            ))
        }
    }
}

impl CustomFunction for LoomSqlStore {}
impl CustomFunctionMut for LoomSqlStore {}

// Query planning: the default `Planner::plan` runs only primary-key + join planning, so a
// secondary index is never selected. This override adds GlueSQL's `plan_index` pass, so a `WHERE`
// equality / range predicate on an indexed column is rewritten to a `NonClustered` index item the
// executor serves via `Index::scan_indexed_data` (above) - the durable prolly index, not a full scan.
// A predicate with no usable index still plans to a full `scan_data` (the correct fallback).
#[async_trait]
impl Planner for LoomSqlStore {
    async fn plan(
        &self,
        statement: gluesql_core::ast::Statement,
    ) -> GResult<gluesql_core::ast::Statement> {
        use gluesql_core::plan::{
            fetch_schema_map, plan_index, plan_join, plan_primary_key, validate,
        };
        let schema_map = fetch_schema_map(self, &statement).await?;
        validate(&schema_map, &statement)?;
        let statement = plan_primary_key(&schema_map, statement);
        let statement = plan_index(&schema_map, statement);
        let statement = plan_join(&schema_map, statement);
        Ok(statement)
    }
}

// ---- versioned persistence into Loom -----------------------------------------------------------

/// The per-database catalog: GlueSQL schemas + the keyless-append counters. Small and changes rarely,
/// so it is a separate blob from the (per-table) row data.
#[derive(serde::Serialize, serde::Deserialize)]
struct Catalog {
    schemas: Vec<(String, Schema)>,
    next_id: Vec<(String, i64)>,
}

fn sql_db_path(db: &str) -> String {
    facet_path(FacetKind::Sql, db)
}

/// The tabular schema a SQL table projects onto: a synthetic `__key` primary-key column (the GlueSQL
/// row key) followed by one **typed column per SQL column** (so each SQL column is a first-class
/// tabular column that secondary indexes, row diff/merge/blame, and structural sharing operate on
/// directly). A schemaless table (no column defs) projects to `__key` plus a single `__map` column.
fn tabular_schema(gschema: &Schema) -> Result<LSchema> {
    let mut columns = vec![("__key".to_string(), key_column_type(gschema))];
    match &gschema.column_defs {
        Some(defs) => {
            for c in defs {
                columns.push((c.name.clone(), value_map::coltype_to_tabular(&c.data_type)));
            }
        }
        None => columns.push(("__map".to_string(), LCol::Map)),
    }
    let mut schema = LSchema::new(columns, vec![0])?;
    // Project each single-column GlueSQL index onto a durable tabular secondary index (content-
    // addressed, synced, GC'd, queryable via `Loom::index_scan`). Expression indexes (non-identifier)
    // stay GlueSQL-side metadata only - they have no single tabular column to key.
    for idx in &gschema.indexes {
        if let Some(col) = index_column_name(&idx.expr) {
            schema = schema.with_index(&idx.name, &[col], false)?;
        }
    }
    Ok(schema)
}

/// Project a GlueSQL `(key, row)` into a tabular row: the `__key` column (the row key) followed by the
/// typed SQL columns (a `Vec` row) or a single `Map` column (a schemaless row). Matches the column
/// layout [`tabular_schema`] builds, used by both full staging and incremental row mutation.
fn project_row(key: &Key, drow: &DataRow) -> Result<Vec<LValue>> {
    let mut row = vec![value_map::key_to_tabular(key)?];
    match drow {
        DataRow::Vec(values) => {
            for v in values {
                row.push(value_map::value_to_tabular(v)?);
            }
        }
        DataRow::Map(m) => {
            let mut mm = BTreeMap::new();
            for (k, v) in m {
                mm.insert(k.clone(), value_map::value_to_tabular(v)?);
            }
            row.push(LValue::Map(mm));
        }
    }
    Ok(row)
}

/// The single column an index expression keys on, if it is a bare column reference; `None` for
/// expression indexes (which cannot map to one tabular column).
fn index_column_name(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Identifier(name) => Some(name),
        _ => None,
    }
}

/// The tabular type of a table's GlueSQL row key: the primary-key column's type, or `Int` (the
/// auto-increment i64 key GlueSQL assigns) when no primary key is declared.
fn key_column_type(gschema: &Schema) -> LCol {
    if let Some(defs) = &gschema.column_defs
        && let Some(pk) = defs
            .iter()
            .find(|c| c.unique.map(|u| u.is_primary).unwrap_or(false))
    {
        return value_map::coltype_to_tabular(&pk.data_type);
    }
    LCol::Int
}

fn infer_parameter_types_from_schemas(
    schemas: &BTreeMap<String, Schema>,
    sql: &str,
) -> Result<Vec<Option<DataType>>> {
    let tokens = parameter_tokens(sql)?;
    let mut inferred = vec![None; max_parameter_index(&tokens)];
    if inferred.is_empty() {
        return Ok(inferred);
    }

    infer_insert_parameters(schemas, &tokens, &mut inferred);
    infer_context_parameters(schemas, &tokens, &mut inferred);
    Ok(inferred)
}

fn parameter_tokens(sql: &str) -> Result<Vec<Token>> {
    let dialect = PostgreSqlDialect {};
    Tokenizer::new(&dialect, sql)
        .tokenize()
        .map(|tokens| {
            tokens
                .into_iter()
                .filter(|token| !matches!(token, Token::Whitespace(_) | Token::EOF))
                .collect()
        })
        .map_err(|e| {
            LoomError::new(
                Code::InvalidArgument,
                format!("SQL parameter inference parse: {e}"),
            )
        })
}

fn max_parameter_index(tokens: &[Token]) -> usize {
    tokens
        .iter()
        .filter_map(parameter_index)
        .map(|index| index + 1)
        .max()
        .unwrap_or(0)
}

fn parameter_index(token: &Token) -> Option<usize> {
    let Token::Placeholder(marker) = token else {
        return None;
    };
    marker
        .strip_prefix('$')?
        .parse::<usize>()
        .ok()
        .and_then(|index| index.checked_sub(1))
}

fn infer_insert_parameters(
    schemas: &BTreeMap<String, Schema>,
    tokens: &[Token],
    inferred: &mut [Option<DataType>],
) {
    let Some(into_index) = find_word(tokens, "INTO") else {
        return;
    };
    let Some(table_index) = next_word_index(tokens, into_index + 1) else {
        return;
    };
    let Some(schema) = schema_for_token(schemas, &tokens[table_index]) else {
        return;
    };
    let columns = insert_columns(tokens, table_index, schema);
    let Some(values_index) = find_word(tokens, "VALUES") else {
        return;
    };
    let Some(row) = first_parenthesized_span(tokens, values_index + 1) else {
        return;
    };
    for (position, span) in split_top_level_commas(tokens, row).into_iter().enumerate() {
        let Some(param_index) = single_placeholder(tokens, span) else {
            continue;
        };
        let Some(column) = columns.get(position) else {
            continue;
        };
        set_inferred(inferred, param_index, column.clone());
    }
}

fn infer_context_parameters(
    schemas: &BTreeMap<String, Schema>,
    tokens: &[Token],
    inferred: &mut [Option<DataType>],
) {
    let table = statement_table(schemas, tokens);
    for (index, token) in tokens.iter().enumerate() {
        let Some(param_index) = parameter_index(token) else {
            continue;
        };
        if let Some(data_type) = explicit_cast_type(tokens, index) {
            set_inferred(inferred, param_index, data_type);
            continue;
        }
        let Some(schema) = table else {
            continue;
        };
        if let Some(column) = comparison_column(tokens, index)
            && let Some(data_type) = column_type(schema, &column)
        {
            set_inferred(inferred, param_index, data_type.clone());
        }
    }
}

fn statement_table<'a>(
    schemas: &'a BTreeMap<String, Schema>,
    tokens: &[Token],
) -> Option<&'a Schema> {
    for keyword in ["FROM", "UPDATE", "INTO"] {
        if let Some(index) = find_word(tokens, keyword)
            && let Some(table_index) = next_word_index(tokens, index + 1)
            && let Some(schema) = schema_for_token(schemas, &tokens[table_index])
        {
            return Some(schema);
        }
    }
    None
}

fn schema_for_token<'a>(
    schemas: &'a BTreeMap<String, Schema>,
    token: &Token,
) -> Option<&'a Schema> {
    let name = token_ident(token)?;
    schemas
        .get(name)
        .or_else(|| schemas.get(&name.to_ascii_lowercase()))
}

fn insert_columns(tokens: &[Token], table_index: usize, schema: &Schema) -> Vec<DataType> {
    if matches!(tokens.get(table_index + 1), Some(Token::LParen))
        && let Some(span) = first_parenthesized_span(tokens, table_index + 1)
    {
        let mut columns = Vec::new();
        for token in &tokens[span.0..span.1] {
            if let Some(name) = token_ident(token)
                && let Some(data_type) = column_type(schema, name)
            {
                columns.push(data_type.clone());
            }
        }
        if !columns.is_empty() {
            return columns;
        }
    }
    schema
        .column_defs
        .as_ref()
        .map(|defs| defs.iter().map(|column| column.data_type.clone()).collect())
        .unwrap_or_default()
}

fn column_type<'a>(schema: &'a Schema, column: &str) -> Option<&'a DataType> {
    schema.column_defs.as_ref()?.iter().find_map(|def| {
        if def.name == column || def.name.eq_ignore_ascii_case(column) {
            Some(&def.data_type)
        } else {
            None
        }
    })
}

fn comparison_column(tokens: &[Token], param_index: usize) -> Option<String> {
    if param_index >= 2 && comparison_token(&tokens[param_index - 1]) {
        return token_ident(&tokens[param_index - 2]).map(str::to_string);
    }
    if param_index + 2 < tokens.len() && comparison_token(&tokens[param_index + 1]) {
        return token_ident(&tokens[param_index + 2]).map(str::to_string);
    }
    None
}

fn comparison_token(token: &Token) -> bool {
    matches!(
        token,
        Token::Eq | Token::Neq | Token::Lt | Token::Gt | Token::LtEq | Token::GtEq
    )
}

fn explicit_cast_type(tokens: &[Token], param_index: usize) -> Option<DataType> {
    if !matches!(tokens.get(param_index + 1), Some(Token::DoubleColon)) {
        return None;
    }
    let name = token_ident(tokens.get(param_index + 2)?)?;
    postgres_type_name(name)
}

fn postgres_type_name(name: &str) -> Option<DataType> {
    match name.to_ascii_lowercase().as_str() {
        "bool" | "boolean" => Some(DataType::Boolean),
        "int2" | "smallint" => Some(DataType::Int16),
        "int4" | "integer" | "int" => Some(DataType::Int32),
        "int8" | "bigint" => Some(DataType::Int),
        "float4" | "real" => Some(DataType::Float32),
        "float8" | "double" => Some(DataType::Float),
        "text" | "varchar" | "char" | "bpchar" => Some(DataType::Text),
        "bytea" => Some(DataType::Bytea),
        "date" => Some(DataType::Date),
        "timestamp" | "timestamptz" => Some(DataType::Timestamp),
        "time" | "timetz" => Some(DataType::Time),
        "uuid" => Some(DataType::Uuid),
        "numeric" | "decimal" => Some(DataType::Decimal),
        "inet" | "cidr" => Some(DataType::Inet),
        _ => None,
    }
}

fn single_placeholder(tokens: &[Token], span: (usize, usize)) -> Option<usize> {
    if span.1 == span.0 + 1 {
        parameter_index(&tokens[span.0])
    } else {
        None
    }
}

fn first_parenthesized_span(tokens: &[Token], start: usize) -> Option<(usize, usize)> {
    let open = tokens
        .iter()
        .enumerate()
        .skip(start)
        .find_map(|(index, token)| matches!(token, Token::LParen).then_some(index))?;
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(open) {
        match token {
            Token::LParen => depth += 1,
            Token::RParen => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some((open + 1, index));
                }
            }
            _ => {}
        }
    }
    None
}

fn split_top_level_commas(tokens: &[Token], span: (usize, usize)) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut start = span.0;
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().take(span.1).skip(span.0) {
        match token {
            Token::LParen => depth += 1,
            Token::RParen => depth = depth.saturating_sub(1),
            Token::Comma if depth == 0 => {
                spans.push((start, index));
                start = index + 1;
            }
            _ => {}
        }
    }
    spans.push((start, span.1));
    spans
}

fn find_word(tokens: &[Token], word: &str) -> Option<usize> {
    tokens
        .iter()
        .position(|token| token_ident(token).is_some_and(|value| value.eq_ignore_ascii_case(word)))
}

fn next_word_index(tokens: &[Token], start: usize) -> Option<usize> {
    tokens
        .iter()
        .enumerate()
        .skip(start)
        .find_map(|(index, token)| token_ident(token).map(|_| index))
}

fn token_ident(token: &Token) -> Option<&str> {
    match token {
        Token::Word(word) => Some(&word.value),
        Token::DoubleQuotedString(value) => Some(value),
        _ => None,
    }
}

fn set_inferred(inferred: &mut [Option<DataType>], index: usize, data_type: DataType) {
    if let Some(slot) = inferred.get_mut(index)
        && slot.is_none()
    {
        *slot = Some(data_type);
    }
}

/// A GlueSQL result [`Payload`](gluesql_core::prelude::Payload) as a canonical-CBOR statement
/// envelope. Each statement is a `{ "kind": <variant>, ... }` map; result rows carry their cells
/// through the faithful cell codec so every scalar survives bit-exact.
fn payload_cbor(p: &gluesql_core::prelude::Payload) -> Result<loom_codec::Value> {
    use gluesql_core::prelude::Payload;
    use loom_codec::Value::{Array, Map};
    Ok(match p {
        Payload::Select { labels, rows } => Map(vec![
            (cbor_text("kind"), cbor_text("Select")),
            (
                cbor_text("labels"),
                Array(labels.iter().map(|l| cbor_text(l.clone())).collect()),
            ),
            (
                cbor_text("rows"),
                Array(rows.iter().map(|r| grow(r)).collect::<Result<Vec<_>>>()?),
            ),
        ]),
        Payload::SelectMap(maps) => Map(vec![
            (cbor_text("kind"), cbor_text("SelectMap")),
            (
                cbor_text("rows"),
                Array(
                    maps.iter()
                        .map(|m| {
                            Ok(Map(m
                                .iter()
                                .map(|(k, v)| Ok((cbor_text(k.clone()), gcell(v)?)))
                                .collect::<Result<Vec<_>>>()?))
                        })
                        .collect::<Result<Vec<_>>>()?,
                ),
            ),
        ]),
        Payload::ShowColumns(cols) => Map(vec![
            (cbor_text("kind"), cbor_text("ShowColumns")),
            (
                cbor_text("columns"),
                Array(
                    cols.iter()
                        .map(|(name, dt)| {
                            Map(vec![
                                (cbor_text("name"), cbor_text(name.clone())),
                                (cbor_text("type"), cbor_text(format!("{dt:?}"))),
                            ])
                        })
                        .collect(),
                ),
            ),
        ]),
        Payload::Insert(n) => payload_count("Insert", *n),
        Payload::Delete(n) => payload_count("Delete", *n),
        Payload::Update(n) => payload_count("Update", *n),
        Payload::DropTable(n) => payload_count("DropTable", *n),
        Payload::Create => payload_kind("Create"),
        Payload::DropFunction => payload_kind("DropFunction"),
        Payload::AlterTable => payload_kind("AlterTable"),
        Payload::CreateIndex => payload_kind("CreateIndex"),
        Payload::DropIndex => payload_kind("DropIndex"),
        Payload::StartTransaction => payload_kind("StartTransaction"),
        Payload::Commit => payload_kind("Commit"),
        Payload::Rollback => payload_kind("Rollback"),
        Payload::ShowVariable(var) => show_variable(var),
    })
}

/// A CBOR text node from anything string-like.
fn cbor_text(s: impl Into<String>) -> loom_codec::Value {
    loom_codec::Value::Text(s.into())
}

/// One GlueSQL value as a faithful CBOR cell (GlueSQL value -> tabular value -> [`cell_value`]).
fn gcell(v: &gluesql_core::data::Value) -> Result<loom_codec::Value> {
    Ok(loom_core::tabular::cell_value(
        &value_map::value_to_tabular(v)?,
    ))
}

/// One GlueSQL row as a CBOR array of faithful cells.
fn grow(row: &[gluesql_core::data::Value]) -> Result<loom_codec::Value> {
    Ok(loom_codec::Value::Array(
        row.iter().map(gcell).collect::<Result<Vec<_>>>()?,
    ))
}

/// A `{ "kind": <kind> }` statement envelope for a unit payload (no data).
fn payload_kind(kind: &str) -> loom_codec::Value {
    loom_codec::Value::Map(vec![(cbor_text("kind"), cbor_text(kind))])
}

/// A `{ "kind": <kind>, "count": n }` envelope for a row-count payload (Insert/Delete/Update/Drop).
fn payload_count(kind: &str, n: usize) -> loom_codec::Value {
    loom_codec::Value::Map(vec![
        (cbor_text("kind"), cbor_text(kind)),
        (cbor_text("count"), loom_codec::Value::Uint(n as u64)),
    ])
}

/// A `SHOW` variable payload as a `{ "kind": "ShowVariable", "variable", ... }` envelope.
fn show_variable(var: &gluesql_core::prelude::PayloadVariable) -> loom_codec::Value {
    use gluesql_core::prelude::PayloadVariable;
    use loom_codec::Value::{Array, Map};
    let strings = |items: &[String]| Array(items.iter().map(|s| cbor_text(s.clone())).collect());
    match var {
        PayloadVariable::Tables(items) => Map(vec![
            (cbor_text("kind"), cbor_text("ShowVariable")),
            (cbor_text("variable"), cbor_text("Tables")),
            (cbor_text("values"), strings(items)),
        ]),
        PayloadVariable::Functions(items) => Map(vec![
            (cbor_text("kind"), cbor_text("ShowVariable")),
            (cbor_text("variable"), cbor_text("Functions")),
            (cbor_text("values"), strings(items)),
        ]),
        PayloadVariable::Version(v) => Map(vec![
            (cbor_text("kind"), cbor_text("ShowVariable")),
            (cbor_text("variable"), cbor_text("Version")),
            (cbor_text("value"), cbor_text(v.clone())),
        ]),
    }
}

impl LoomSqlStore {
    /// Whether an explicit SQL transaction (`BEGIN` without a matching `COMMIT`/`ROLLBACK`) is open.
    /// The `LoomSqlBatch` scope checks this so it never persists a half-finished transaction, and the
    /// per-op exec path rejects a transaction left dangling at the end of a single call.
    pub fn in_transaction(&self) -> bool {
        self.txn.is_some()
    }

    /// Whether anything was mutated since the last `persist` (so the caller can skip persisting - and so
    /// avoid taking the exclusive write lock - for a read-only `exec`, keeping `SELECT` lock-free).
    pub fn is_dirty(&self) -> bool {
        !self.row_dirty.is_empty() || !self.schema_dirty.is_empty()
    }

    /// Point-fetch one row from the base read snapshot (the overlay having already been consulted).
    /// `None` when the table is not in the base, is empty, or has no such key.
    fn base_fetch(&self, table_name: &str, key: &Key) -> Result<Option<DataRow>> {
        let Some(base) = &self.base else {
            return Ok(None);
        };
        let (Some(schema), Some(Some(root))) =
            (base.schemas.get(table_name), base.roots.get(table_name))
        else {
            return Ok(None);
        };
        let pk = [value_map::key_to_tabular(key)?];
        match LTable::get_row(&base.store, schema, root, &pk)? {
            Some(row) => {
                let schemaless = self
                    .schemas
                    .get(table_name)
                    .map(|s| s.column_defs.is_none())
                    .unwrap_or(false);
                Ok(Some(data_row_from_tabular(schemaless, &row)?))
            }
            None => Ok(None),
        }
    }

    /// Visit every live row of `table` in the merged base+overlay view (base rows not shadowed by the
    /// overlay, then the overlay's upserts), reconstructed as GlueSQL `(key, row)`. Used by the persist
    /// full re-stage (a new table / `ALTER` / index change), the one inherently `O(rows)` path; the
    /// common case applies only the row deltas.
    fn for_each_row(
        &self,
        table: &str,
        mut visit: impl FnMut(&Key, &DataRow) -> Result<()>,
    ) -> Result<()> {
        let overlay = self.overlay.get(table);
        let schemaless = self
            .schemas
            .get(table)
            .map(|s| s.column_defs.is_none())
            .unwrap_or(false);
        if let Some(base) = &self.base
            && let (Some(schema), Some(Some(root))) =
                (base.schemas.get(table), base.roots.get(table))
        {
            let mut cur = RowCursor::open(&base.store, schema, root)?;
            while let Some(row) = cur.next()? {
                let key = value_map::key_from_tabular(&row[0])?;
                if overlay.is_some_and(|o| o.contains_key(&key)) {
                    continue; // shadowed by an overlay update/delete
                }
                let drow = data_row_from_tabular(schemaless, &row)?;
                visit(&key, &drow)?;
            }
        }
        if let Some(ov) = overlay {
            for (k, slot) in ov {
                if let Some(drow) = slot {
                    visit(k, drow)?;
                }
            }
        }
        Ok(())
    }

    /// Collect the rows matching an indexed predicate `col OP value` (`cmp = None` is an unbounded index
    /// scan), each as `(indexed value, key, row)`. The base side reads through the durable prolly index
    /// (equality prefix-scans it; other predicates walk it in order; a missing index tree falls back to a
    /// base row scan) and skips rows the overlay shadows; the overlay side is scanned in full and
    /// filtered. Backs [`Index::scan_indexed_data`].
    fn indexed_rows(
        &self,
        table: &str,
        index_name: &str,
        cmp: Option<(&IndexOperator, GValue)>,
    ) -> Result<Vec<(LValue, Key, DataRow)>> {
        let gschema = self
            .schemas
            .get(table)
            .ok_or_else(|| LoomError::not_found(format!("table {table:?} not found")))?;
        let col = indexed_column_name(gschema, index_name).ok_or_else(|| {
            LoomError::invalid(format!(
                "index {index_name:?} on {table:?} is not a single-column index"
            ))
        })?;
        let schemaless = gschema.column_defs.is_none();
        // The predicate as a tabular value (total-ordered), converted once.
        let target: Option<(IndexOperator, LValue)> = match cmp {
            Some((op, v)) => Some((op.clone(), value_map::value_to_tabular(&v)?)),
            None => None,
        };
        let mut out: Vec<(LValue, Key, DataRow)> = Vec::new();

        // Base, through the durable index (or a base row scan if there is no index tree yet).
        if let Some(base) = &self.base
            && let (Some(tab_schema), Some(Some(rows_root))) =
                (base.schemas.get(table), base.roots.get(table))
        {
            let overlay = self.overlay.get(table);
            // The indexed column's position in the tabular row (the `__key`-offset column index).
            let col_pos = tab_schema
                .index(index_name)
                .and_then(|i| i.columns.first().copied())
                .ok_or_else(|| LoomError::invalid(format!("index {index_name:?} has no column")))?;
            // The index bound from the predicate: equality prefix-scans; a range uses exact byte bounds
            // on a scalar column. A range over a composite (`List`/`Map`) column - whose encoded order is
            // not its semantic order - walks the whole index instead, leaving `index_cmp_matches` below
            // as the authoritative filter.
            let composite = matches!(
                tab_schema.columns.get(col_pos).map(|(_, t)| t),
                Some(LCol::List | LCol::Map)
            );
            let bound = match &target {
                Some((IndexOperator::Eq, v)) => IndexBound::Eq(std::slice::from_ref(v)),
                Some((op, v)) if !composite => match op {
                    IndexOperator::Gt => IndexBound::Gt(v),
                    IndexOperator::GtEq => IndexBound::GtEq(v),
                    IndexOperator::Lt => IndexBound::Lt(v),
                    IndexOperator::LtEq => IndexBound::LtEq(v),
                    IndexOperator::Eq => unreachable!("handled above"),
                },
                _ => IndexBound::All, // unbounded scan, or a range on a composite-typed column
            };
            let base_rows: Vec<LRow> =
                match base.index_roots.get(table).and_then(|m| m.get(index_name)) {
                    Some(index_root) => loom_core::tabular::index_scan_rows(
                        &base.store,
                        tab_schema,
                        index_name,
                        rows_root,
                        index_root,
                        bound,
                    )?,
                    None => {
                        // No durable index tree yet: correct fallback over the base row map.
                        let mut cur = RowCursor::open(&base.store, tab_schema, rows_root)?;
                        let mut rows = Vec::new();
                        while let Some(r) = cur.next()? {
                            rows.push(r);
                        }
                        rows
                    }
                };
            for row in base_rows {
                let key = value_map::key_from_tabular(&row[0])?;
                if overlay.is_some_and(|o| o.contains_key(&key)) {
                    continue; // an overlay update/delete shadows this base row
                }
                let Some(idxval) = row.get(col_pos).cloned() else {
                    continue;
                };
                if index_cmp_matches(&idxval, &target) {
                    out.push((idxval, key, data_row_from_tabular(schemaless, &row)?));
                }
            }
        }

        // Overlay (small): evaluate the predicate directly over the session's live rows.
        if let Some(ov) = self.overlay.get(table) {
            for (key, slot) in ov {
                if let Some(drow) = slot
                    && let Some(idxval) = overlay_indexed_value(gschema, col, drow)?
                    && index_cmp_matches(&idxval, &target)
                {
                    out.push((idxval, key.clone(), drow.clone()));
                }
            }
        }
        Ok(out)
    }

    /// Capture the current mutable state for transaction rollback. Copies the overlay (the session's
    /// changes) and the small base metadata - never the base's row data (immutable through the snapshot),
    /// so this is cheap even for a larger-than-RAM table.
    fn snapshot(&self) -> TxnSnapshot {
        TxnSnapshot {
            schemas: self.schemas.clone(),
            base: self.base.clone(),
            overlay: self.overlay.clone(),
            next_id: self.next_id.clone(),
            row_dirty: self.row_dirty.clone(),
            schema_dirty: self.schema_dirty.clone(),
        }
    }

    /// Restore a snapshot captured by [`snapshot`](Self::snapshot), discarding every change since.
    fn restore(&mut self, snap: TxnSnapshot) {
        self.schemas = snap.schemas;
        self.base = snap.base;
        self.overlay = snap.overlay;
        self.next_id = snap.next_id;
        self.row_dirty = snap.row_dirty;
        self.schema_dirty = snap.schema_dirty;
    }

    /// Run one or more `;`-separated SQL statements against this in-memory store and return the GlueSQL
    /// result payloads. Mutations update the store (call [`LoomSqlStore::persist`] to stage them into a
    /// Loom). This is the single SQL entry point the FFI and language bindings call (via [`exec_cbor`]),
    /// so GlueSQL stays contained in this crate.
    ///
    /// [`exec_cbor`]: LoomSqlStore::exec_cbor
    fn run(&mut self, sql: &str) -> Result<Vec<gluesql_core::prelude::Payload>> {
        let mut glue = gluesql_core::prelude::Glue::new(std::mem::take(self));
        let result = futures::executor::block_on(glue.execute(sql));
        *self = glue.storage; // move the (mutated) store back out of Glue
        result.map_err(sql_error)
    }

    /// Infer prepared-statement parameter types from schema-backed SQL contexts.
    ///
    /// The result is indexed by PostgreSQL parameter number minus one. `None` means the SQL contains
    /// that marker, but this inference pass did not find a table-column or explicit cast context for
    /// it.
    pub fn infer_parameter_types(&self, sql: &str) -> Result<Vec<Option<DataType>>> {
        infer_parameter_types_from_schemas(&self.schemas, sql)
    }

    /// Run one or more `;`-separated SQL statements; return the result payloads as **canonical CBOR**,
    /// the normative wire form the FFI and bindings return. Each payload is built directly
    /// with the faithful cell codec ([`loom_core::tabular::cell_value`]), so every scalar (including
    /// 128-bit integers, non-finite floats, exact `f32`, decimals, and byte strings) crosses the
    /// boundary bit-exact (no serde_json route). Call [`LoomSqlStore::persist`] to stage mutations.
    pub fn exec_cbor(&mut self, sql: &str) -> Result<Vec<u8>> {
        let payloads = self.run(sql)?;
        let mut out = Vec::with_capacity(payloads.len());
        for p in &payloads {
            out.push(payload_cbor(p)?);
        }
        loom_codec::encode(&loom_codec::Value::Array(out))
            .map_err(|e| LoomError::corrupt(format!("result cbor: {e}")))
    }

    /// Run one or more `;`-separated SQL statements, returning the result payloads as a JSON string.
    /// **Debug only**, rendered from the canonical CBOR of [`LoomSqlStore::exec_cbor`] (the normative
    /// surface) - never a separate serialization, so the two can never drift.
    pub fn exec_json(&mut self, sql: &str) -> Result<String> {
        loom_result::result_to_json(&self.exec_cbor(sql)?)
    }

    /// Run SQL and return the rows of the first `SELECT` result, **each row encoded on its own** as
    /// canonical CBOR (a cell array). This is the per-item form a streaming iterator yields one
    /// at a time (`loom_iter_next`): a binding wraps the iterator as an `AsyncIterable` /
    /// `Stream` and pulls rows without ever materializing the whole result in the foreign runtime. The
    /// engine still computes the result eagerly here; the iterator surface is stable enough for a
    /// lazy/streaming backend to slot in behind it without an ABI change. A statement that is not a
    /// `SELECT` (or a reader `Rows` result) yields an empty row list.
    pub fn select_rows_cbor(&mut self, sql: &str) -> Result<Vec<Vec<u8>>> {
        use loom_result::result_view::{Reader, ResultPayload, Statement};
        let rows = match loom_result::result_view::decode(&self.exec_cbor(sql)?)? {
            ResultPayload::Statements(stmts) => stmts
                .into_iter()
                .find_map(|s| match s {
                    Statement::Select { rows, .. } => Some(rows),
                    _ => None,
                })
                .unwrap_or_default(),
            ResultPayload::Reader(Reader::Rows { rows, .. }) => rows,
            ResultPayload::Reader(_) => Vec::new(),
        };
        rows.into_iter()
            .map(|row| {
                let cells: Vec<loom_codec::Value> =
                    row.iter().map(loom_core::tabular::cell_value).collect();
                loom_codec::encode(&loom_codec::Value::Array(cells))
                    .map_err(|e| LoomError::corrupt(format!("row cbor: {e}")))
            })
            .collect()
    }

    /// Run SQL and return the rows of the first `SELECT` result as **decoded** tabular values - the form
    /// the in-process (Rust-native) bindings iterate, mapping each row's cells to their native types.
    /// Mirrors [`select_rows_cbor`](Self::select_rows_cbor) but skips the per-row re-encode, since the
    /// caller is in the same address space. A non-`SELECT` yields an empty row list and may leave the
    /// store dirty; read-only public query surfaces reject that dirty state.
    pub fn select_rows(&mut self, sql: &str) -> Result<Vec<Vec<LValue>>> {
        use loom_result::result_view::{Reader, ResultPayload, Statement};
        Ok(
            match loom_result::result_view::decode(&self.exec_cbor(sql)?)? {
                ResultPayload::Statements(stmts) => stmts
                    .into_iter()
                    .find_map(|s| match s {
                        Statement::Select { rows, .. } => Some(rows),
                        _ => None,
                    })
                    .unwrap_or_default(),
                ResultPayload::Reader(Reader::Rows { rows, .. }) => rows,
                ResultPayload::Reader(_) => Vec::new(),
            },
        )
    }

    /// Persist the database `db` into workspace `ns` for the engine to version: the catalog and each
    /// SQL table are separate working-tree entries inside the SQL facet. The caller commits; then it
    /// branches, syncs, and checks out like any Loom data.
    pub fn persist<S: ObjectStore>(
        &mut self,
        loom: &mut Loom<S>,
        ns: WorkspaceId,
        db: &str,
    ) -> Result<()> {
        loom.authorize(ns, FacetKind::Sql, AclRight::Write)?;
        let base = sql_db_path(db);
        // SQL is a facet implementation, so it writes its own reserved `.loom/facets/sql/...` storage
        // through the privileged facet-writers; the public fs facade rejects user writes there (0014a),
        // exactly as for the other facets.
        loom.create_directory_reserved(ns, &format!("{base}/tables"), true)?;

        let catalog = Catalog {
            schemas: self
                .schemas
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            next_id: self.next_id.iter().map(|(k, v)| (k.clone(), *v)).collect(),
        };
        let cat_bytes = serde_json::to_vec(&catalog).expect("catalog serialize");
        loom.write_file_reserved(ns, &format!("{base}/catalog"), &cat_bytes, 0o100644)?;

        // Each SQL table is its own tabular::Table, with SQL columns projected to typed tabular columns
        // and the `__key` column carrying the GlueSQL row key. A table whose schema or index set changed
        // is staged in full; otherwise only its dirty rows are applied incrementally.
        for (name, gschema) in &self.schemas {
            let path = format!("{base}/tables/{name}");
            let needs_full =
                self.schema_dirty.contains(name) || loom.staged_table_root(ns, &path).is_none();
            if needs_full {
                // Full re-stage (new table / ALTER / index change): the one inherently O(rows) path -
                // re-project the merged base+overlay view through the (possibly new) tabular schema.
                let mut t = LTable::new(tabular_schema(gschema)?);
                self.for_each_row(name, |key, drow| {
                    t.insert(project_row(key, drow)?)?;
                    Ok(())
                })?;
                loom.stage_table(ns, &path, &t)?;
            } else if let Some(dirty) = self.row_dirty.get(name) {
                // Common path: apply only the row deltas the overlay recorded (upsert or tombstone), so
                // commit cost scales with the change, not the table size.
                for key in dirty {
                    match self.overlay.get(name).and_then(|o| o.get(key)) {
                        Some(Some(drow)) => loom.insert_row(ns, &path, project_row(key, drow)?)?,
                        Some(None) => {
                            loom.delete_row(ns, &path, &[value_map::key_to_tabular(key)?])?
                        }
                        None => {} // dirty without an overlay slot: nothing to apply
                    }
                }
            }
        }

        // Drop data files for tables removed since the last persist (so DROP TABLE is reflected).
        let live: BTreeSet<String> = self.schemas.keys().cloned().collect();
        let prefix = format!("{base}/tables/");
        for path in loom.staged_paths(ns) {
            if let Some(tn) = path.strip_prefix(&prefix)
                && !live.contains(tn)
            {
                loom.remove_file_reserved(ns, &path)?;
            }
        }

        // The staged tree now matches the in-memory state: clear the change log.
        self.row_dirty.clear();
        self.schema_dirty.clear();
        Ok(())
    }

    /// Read the database's catalog and per-table base metadata (schema + row-map root) from `loom`'s
    /// working tree **without** loading any rows - the cheap open-time read the lazy base needs. Returns
    /// empty maps if no catalog is staged (a fresh, never-written database). Shared by [`load`] and
    /// [`open`].
    ///
    /// [`load`]: LoomSqlStore::load
    /// [`open`]: LoomSqlStore::open
    fn read_meta<S: ObjectStore>(loom: &Loom<S>, ns: WorkspaceId, db: &str) -> Result<BaseMeta> {
        let base = sql_db_path(db);
        let catalog_path = format!("{base}/catalog");
        if !loom.staged_paths(ns).contains(&catalog_path) {
            return Ok(BaseMeta::default()); // fresh database: no catalog, no base
        }
        let cat_bytes = loom.read_file_reserved(ns, &catalog_path)?;
        let catalog: Catalog = serde_json::from_slice(&cat_bytes)
            .map_err(|e| LoomError::corrupt(format!("sql catalog: {e}")))?;
        let mut meta = BaseMeta {
            schemas: catalog.schemas.into_iter().collect(),
            next_id: catalog.next_id.into_iter().collect(),
            ..BaseMeta::default()
        };
        for name in meta.schemas.keys() {
            let path = format!("{base}/tables/{name}");
            // A freshly-schema'd, never-persisted table has no table slot yet (`Ok(None)`); skip it.
            let Some((tab_schema, root)) = loom.table_reader_reserved(ns, &path)? else {
                continue;
            };
            // Capture each declared index's durable `index/<name>` root (absent until the index has
            // entries), so an indexed predicate can scan it without a Loom.
            let mut roots = BTreeMap::new();
            for idx in &tab_schema.indexes {
                if let Some(ir) = loom.table_index_reader_reserved(ns, &path, &idx.name)? {
                    roots.insert(idx.name.clone(), ir);
                }
            }
            if !roots.is_empty() {
                meta.index_roots.insert(name.clone(), roots);
            }
            meta.base_schemas.insert(name.clone(), tab_schema);
            meta.base_roots.insert(name.clone(), root);
        }
        Ok(meta)
    }

    /// Assemble a store from catalog metadata plus an owned base read store.
    fn with_base(meta: BaseMeta, store: Arc<dyn ObjectStore + Send + Sync>) -> Self {
        Self {
            schemas: meta.schemas,
            base: Some(BaseSnapshot {
                store,
                schemas: meta.base_schemas,
                roots: meta.base_roots,
                index_roots: meta.index_roots,
            }),
            overlay: BTreeMap::new(),
            next_id: meta.next_id,
            row_dirty: BTreeMap::new(),
            schema_dirty: BTreeSet::new(),
            txn: None,
        }
    }

    /// Open the database `db` from an **owned** read view of the loom (the lazy base):
    /// the catalog and per-table schemas/roots are read once, then the read view's store is moved into an
    /// owned `Arc` so the engine can stream durable rows for the session's lifetime. The platform supplies
    /// the read view as its own lock-free snapshot (native: `open_loom_read`; wasm: a byte snapshot) -
    /// distinct from the write loom that [`persist`](Self::persist) flushes into. An absent catalog yields
    /// an empty (but base-backed) store.
    pub fn open<S: ObjectStore + Send + Sync + 'static>(
        read_loom: Loom<S>,
        ns: WorkspaceId,
        db: &str,
    ) -> Result<Self> {
        let meta = Self::read_meta(&read_loom, ns, db)?;
        let store: Arc<dyn ObjectStore + Send + Sync> = Arc::new(read_loom.into_store());
        Ok(Self::with_base(meta, store))
    }

    /// Open a SQL database for a read-only public query.
    pub fn open_read<S: ObjectStore + Send + Sync + 'static>(
        read_loom: Loom<S>,
        ns: WorkspaceId,
        db: &str,
    ) -> Result<Self> {
        read_loom.authorize(ns, FacetKind::Sql, AclRight::Read)?;
        Self::open(read_loom, ns, db)
    }

    /// Open a SQL database for a mutation-capable public exec.
    pub fn open_write<S: ObjectStore + Send + Sync + 'static>(
        read_loom: Loom<S>,
        ns: WorkspaceId,
        db: &str,
    ) -> Result<Self> {
        read_loom.authorize(ns, FacetKind::Sql, AclRight::Write)?;
        Self::open(read_loom, ns, db)
    }

    /// Load the database `db` from a **borrowed** loom whose store is cheap to clone (in-memory tests and
    /// any `Clone` backend): the base read view shares the store via an `Arc` clone. The on-disk
    /// (`FileStore`) path uses [`open`](Self::open) with a separate read snapshot instead, since a write
    /// `FileStore` is not `Clone`. An absent catalog yields an empty store.
    pub fn load<S: ObjectStore + Clone + Send + Sync + 'static>(
        loom: &Loom<S>,
        ns: WorkspaceId,
        db: &str,
    ) -> Result<Self> {
        let meta = Self::read_meta(loom, ns, db)?;
        let store: Arc<dyn ObjectStore + Send + Sync> = Arc::new(loom.store().clone());
        Ok(Self::with_base(meta, store))
    }

    /// Load the database **eagerly** into the overlay (no lazy base): read every row of every table up
    /// front, so the read path is in-memory and `Send` regardless of the backend. Used where a streaming
    /// base is unavailable - notably the wasm32 `FileStore`, which is not `Send` (its backing erases to a
    /// `Box<dyn BackingIo>` without `+ Send`) and so cannot back a GlueSQL `RowIter` (which must be
    /// `Send`). RAM-bound by design; larger-than-RAM is the native requirement, where [`open`](Self::open)
    /// streams from a lazy disk snapshot instead. Preloaded rows are **not** marked dirty, so `persist`
    /// still writes only what later changes.
    pub fn load_eager<S: ObjectStore>(loom: &Loom<S>, ns: WorkspaceId, db: &str) -> Result<Self> {
        let BaseMeta {
            schemas, next_id, ..
        } = Self::read_meta(loom, ns, db)?;
        let base_path = sql_db_path(db);
        let mut overlay: BTreeMap<String, BTreeMap<Key, Option<DataRow>>> = BTreeMap::new();
        for name in schemas.keys() {
            let table = match loom.read_table_reserved(ns, &format!("{base_path}/tables/{name}")) {
                Ok(t) => t,
                Err(_) => continue, // a freshly-schema'd, never-persisted table has no table slot yet
            };
            let schemaless = schemas[name].column_defs.is_none();
            let dest = overlay.entry(name.clone()).or_default();
            for row in table.scan(&loom_core::tabular::Predicate::All) {
                let key = value_map::key_from_tabular(&row[0])?;
                dest.insert(key, Some(data_row_from_tabular(schemaless, row)?));
            }
        }
        Ok(Self {
            schemas,
            base: None,
            overlay,
            next_id,
            row_dirty: BTreeMap::new(),
            schema_dirty: BTreeSet::new(),
            txn: None,
        })
    }

    /// Eagerly load a SQL database for a read-only public query.
    pub fn load_eager_read<S: ObjectStore>(
        loom: &Loom<S>,
        ns: WorkspaceId,
        db: &str,
    ) -> Result<Self> {
        loom.authorize(ns, FacetKind::Sql, AclRight::Read)?;
        Self::load_eager(loom, ns, db)
    }

    /// Eagerly load a SQL database for a mutation-capable public exec.
    pub fn load_eager_write<S: ObjectStore>(
        loom: &Loom<S>,
        ns: WorkspaceId,
        db: &str,
    ) -> Result<Self> {
        loom.authorize(ns, FacetKind::Sql, AclRight::Write)?;
        Self::load_eager(loom, ns, db)
    }
}

/// The crate version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Deterministic conformance vector for the whole SQL-over-Loom stack. Runs a fixed
/// create/insert/commit script (fixed workspace id, author, message, and timestamp) against a fresh
/// `store` and returns the resulting commit's content address. Backing- and target-agnostic: native
/// (`std::fs` / in-memory, 64-bit) and the wasm32 OPFS build (32-bit) MUST all return
/// [`CONFORMANCE_COMMIT`]. This pins the canonical object encoding across object stores and, crucially,
/// across pointer widths - 32-bit wasm vs 64-bit native.
pub fn conformance_commit_digest<S: ObjectStore>(store: S) -> Result<String> {
    use loom_core::{FacetKind, WsSelector};
    let mut loom = Loom::new(store);
    let ns = loom.registry_mut().ensure_for_write(
        &WsSelector::Default(FacetKind::Sql),
        WorkspaceId::from_bytes([0x11; 16]),
    )?;
    let mut sql = LoomSqlStore::default();
    sql.exec_json("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)")?;
    sql.exec_json("INSERT INTO t VALUES (1, 'alpha'), (2, 'beta')")?;
    sql.persist(&mut loom, ns, "cv")?;
    Ok(loom.commit(ns, "conformance", "cv-1", 0)?.to_string())
}

/// The content address [`conformance_commit_digest`] returns on every conforming backend and target.
/// The browser conformance check recomputes it live on wasm32 and asserts equality with this.
pub const CONFORMANCE_COMMIT: &str =
    "blake3:c5bf636410db63b7cf88107ec439c4a960347bab8f7ee1865445641668d38b8b"; // pinned by the test below

// ---------------------------------------------------------------------------------------------------
// Result-payload conformance vectors: pin the canonical result bytes the FFI and every language
// binding decode, so the one shared decoder (`result_view`) and the RN bridge projection (`bridge_json`)
// are proven to round-trip the same typed values everywhere. The bytes are deterministic, so a binding
// running the same input MUST get the same bytes and therefore the same typed result.
// ---------------------------------------------------------------------------------------------------

/// A fixed, hard-typed single-row table whose canonical result payload exercises cells that are easy
/// to lose through lossy bridges: a 128-bit integer beyond `u64`, a NaN float, an exact `f32`, a decimal,
/// raw bytes, plus the easy scalars. Returns the canonical CBOR of its `Rows` result.
pub fn result_vector_payload() -> Vec<u8> {
    use loom_core::tabular::{ColumnType, Schema, Table, Value};
    let schema = Schema::new(
        vec![
            ("id".into(), ColumnType::Int),
            ("name".into(), ColumnType::Text),
            ("flag".into(), ColumnType::Bool),
            ("big".into(), ColumnType::U128),
            ("amount".into(), ColumnType::Decimal),
            ("raw".into(), ColumnType::Bytes),
            ("ratio".into(), ColumnType::F32),
            ("missing".into(), ColumnType::Float),
        ],
        vec![0],
    )
    .expect("result vector schema");
    let mut table = Table::new(schema);
    table
        .insert(vec![
            Value::Int(1),
            Value::Text("hi".into()),
            Value::Bool(true),
            Value::U128(u128::from(u64::MAX) + 1),
            Value::Decimal {
                mantissa: 12_345,
                scale: 2,
            },
            Value::Bytes(vec![0, 1, 2, 255]),
            Value::F32(0.1f32),
            Value::Float(f64::NAN),
        ])
        .expect("result vector row");
    crate::result_cbor::table_cbor(&table).expect("result vector encode")
}

/// The pinned content address of [`result_vector_payload`]'s bytes - the result-codec proof bar. Every
/// backend and target MUST reproduce it; every binding's typed decoder MUST round-trip these bytes.
pub const RESULT_VECTOR_DIGEST: &str =
    "blake3:ca617610f7dbcc19baa690d54df723532529a075f2eb0c611b6dff360520feef"; // pinned by the test

/// A fixed portable SQL exec whose result payload every binding can reproduce through its own typed
/// `exec` (Int + Text + NULL - types every binding and GlueSQL handle identically). Returns the
/// canonical CBOR of the `Select` statement result.
pub fn result_exec_vector() -> Vec<u8> {
    let mut store = LoomSqlStore::default();
    store
        .exec_cbor("CREATE TABLE t (id INTEGER PRIMARY KEY, n TEXT)")
        .expect("exec vector create");
    store
        .exec_cbor("INSERT INTO t VALUES (1, 'hi'), (2, NULL)")
        .expect("exec vector insert");
    store
        .exec_cbor("SELECT id, n FROM t ORDER BY id")
        .expect("exec vector select")
}

/// The pinned content address of [`result_exec_vector`]'s bytes.
pub const RESULT_EXEC_VECTOR_DIGEST: &str =
    "blake3:170202a820f490f842846cb876d6ec29d0cbb45c7e2ff89cb54f0262ee613c52"; // pinned by the test

#[cfg(test)]
mod tests;
