use std::fmt::Write;

use loom_core::document::{doc_get, doc_list, doc_put};
use loom_core::log;
use loom_core::tabular::{ColumnType, Value};
use loom_core::vcs::Loom;
use loom_core::{
    BookMeta, CalendarEntry, CollectionMeta, ColumnarAggregate, Component, ContactEntry,
    DataframeBatch, DataframeMaterializationTarget, DataframePlan, DataframeSourceKind, Digest,
    DirEntry, Document, Edge, Hit, MailMessage, MailboxMeta, Mapping, MetaFilter, Metric,
    ObjectStore, Props, QueryRequest, QueryResponse, Series, VectorEntry, calendar, cas_delete,
    cas_get, cas_has, cas_list, cas_put, columnar_append, columnar_columns, columnar_create,
    columnar_rows, columnar_scan, columnar_select, contacts, dataframe_collect, dataframe_create,
    dataframe_materialize, dataframe_plan_digest, dataframe_preview, dataframe_source_digests,
    doc_delete, get_dataframe_plan, graph_get_edge, graph_get_node, graph_in_edges,
    graph_neighbors, graph_out_edges, graph_reachable, graph_remove_edge, graph_remove_node,
    graph_shortest_path, graph_upsert_edge, graph_upsert_node, kv_delete, kv_get, kv_list, kv_put,
    kv_range, ledger_append, ledger_get, ledger_head, ledger_len, ledger_verify, mail,
    put_dataframe_plan, search_create, search_delete, search_get, search_ids, search_index,
    search_query, search_remap, ts_get, ts_latest, ts_put, ts_range, vector_create, vector_delete,
    vector_get, vector_ids, vector_search, vector_source_text, vector_upsert,
};

use crate::authz::ExecContext;
use crate::capability::{Capability, Mode};
use crate::error::ExecError;

#[cfg(feature = "sql-state-access")]
use loom_sql::LoomSqlStore;

pub struct StateAccess<'a, S: ObjectStore> {
    loom: &'a mut Loom<S>,
    context: ExecContext,
}

impl<'a, S: ObjectStore> StateAccess<'a, S> {
    pub fn new(loom: &'a mut Loom<S>, context: ExecContext) -> Self {
        Self { loom, context }
    }

    pub fn file_read(&mut self, path: &str) -> Result<Vec<u8>, ExecError> {
        self.authorize(Capability::Files, Mode::Read, path)?;
        Ok(self.loom.read_file(self.context.workspace, path)?)
    }

    pub fn file_write(&mut self, path: &str, bytes: &[u8]) -> Result<(), ExecError> {
        self.authorize(Capability::Files, Mode::Write, path)?;
        if let Some((parent, _)) = path.trim_start_matches('/').rsplit_once('/') {
            self.loom
                .create_directory(self.context.workspace, parent, true)?;
        }
        Ok(self
            .loom
            .write_file(self.context.workspace, path, bytes, 0o100644)?)
    }

    pub fn file_remove(&mut self, path: &str) -> Result<(), ExecError> {
        self.authorize(Capability::Files, Mode::Write, path)?;
        Ok(self.loom.remove_file(self.context.workspace, path)?)
    }

    pub fn file_list(&mut self, path: &str) -> Result<Vec<DirEntry>, ExecError> {
        self.authorize(Capability::Files, Mode::Read, path)?;
        Ok(self.loom.list_directory(self.context.workspace, path)?)
    }

    pub fn kv_get(&mut self, collection: &str, key: &Value) -> Result<Option<Vec<u8>>, ExecError> {
        let target = kv_target(collection, key);
        self.authorize(Capability::Kv, Mode::Read, &target)?;
        Ok(kv_get(self.loom, self.context.workspace, collection, key)?)
    }

    pub fn kv_put(
        &mut self,
        collection: &str,
        key: Value,
        value: Vec<u8>,
    ) -> Result<(), ExecError> {
        let target = kv_target(collection, &key);
        self.authorize(Capability::Kv, Mode::Write, &target)?;
        Ok(kv_put(
            self.loom,
            self.context.workspace,
            collection,
            key,
            value,
        )?)
    }

    pub fn kv_delete(&mut self, collection: &str, key: &Value) -> Result<bool, ExecError> {
        let target = kv_target(collection, key);
        self.authorize(Capability::Kv, Mode::Write, &target)?;
        Ok(kv_delete(
            self.loom,
            self.context.workspace,
            collection,
            key,
        )?)
    }

    pub fn kv_list(&mut self, collection: &str) -> Result<Vec<(Value, Vec<u8>)>, ExecError> {
        let target = kv_collection_target(collection);
        self.authorize(Capability::Kv, Mode::Read, &target)?;
        Ok(kv_list(self.loom, self.context.workspace, collection)?
            .iter()
            .map(|(key, value)| (key.clone(), value.to_vec()))
            .collect())
    }

    /// Bounded, ordered scan of a KV collection over the half-open typed key range `[lo, hi)`, capped at
    /// `limit` entries. Keys sort in `Value` order (so `Int(2)` precedes `Int(10)`), the read is gated by
    /// the collection-scoped `Kv` grant, and there is no server-side cursor: a caller pages by advancing
    /// `lo` past the last returned key.
    pub fn kv_scan(
        &mut self,
        collection: &str,
        lo: &Value,
        hi: &Value,
        limit: usize,
    ) -> Result<Vec<(Value, Vec<u8>)>, ExecError> {
        let target = kv_collection_target(collection);
        self.authorize(Capability::Kv, Mode::Read, &target)?;
        Ok(
            kv_range(self.loom, self.context.workspace, collection, lo, hi)?
                .iter()
                .take(limit)
                .map(|(key, value)| (key.clone(), value.to_vec()))
                .collect(),
        )
    }

    #[cfg(feature = "sql-state-access")]
    pub fn sql_query_cbor(&mut self, db: &str, sql: &str) -> Result<Vec<u8>, ExecError> {
        let target = sql_db_target(db);
        self.authorize(Capability::Sql, Mode::Read, &target)?;
        let mut store = LoomSqlStore::load_eager(self.loom, self.context.workspace, db)?;
        let out = store.exec_cbor(sql)?;
        if store.in_transaction() {
            return Err(loom_core::LoomError::invalid(
                "BEGIN without a matching COMMIT/ROLLBACK in one query",
            )
            .into());
        }
        if store.is_dirty() {
            return Err(loom_core::LoomError::new(
                loom_core::Code::PermissionDenied,
                "sql.query is read-only; use sql.exec for statements that mutate state",
            )
            .into());
        }
        Ok(out)
    }

    #[cfg(feature = "sql-state-access")]
    pub fn sql_exec_cbor(&mut self, db: &str, sql: &str) -> Result<Vec<u8>, ExecError> {
        let target = sql_db_target(db);
        self.authorize(Capability::Sql, Mode::Write, &target)?;
        let mut store = LoomSqlStore::load_eager(self.loom, self.context.workspace, db)?;
        let out = store.exec_cbor(sql)?;
        if store.in_transaction() {
            return Err(loom_core::LoomError::invalid(
                "BEGIN without a matching COMMIT/ROLLBACK in one exec",
            )
            .into());
        }
        if store.is_dirty() {
            store.persist(self.loom, self.context.workspace, db)?;
        }
        Ok(out)
    }

    /// Recompute a derivation over the live KV `collection` and write its derived entry back, all
    /// grant-gated through `StateAccess`: the `kv_list` read authorizes `Kv` Read on the collection and
    /// the `kv_put` authorizes `Kv` Write on the derived key. The derivation's flat string keys map to
    /// the collection's `Text`-keyed KV entries (non-text keys are not part of the derivation view).
    /// Behind the `derivations` feature.
    #[cfg(feature = "derivations")]
    pub fn run_derivation(
        &mut self,
        collection: &str,
        derivation: &crate::derivation::Derivation,
    ) -> Result<(String, Vec<u8>), ExecError> {
        let mut view = crate::derivation::KvView::new();
        for (key, value) in self.kv_list(collection)? {
            if let Value::Text(k) = key {
                view.insert(k, value);
            }
        }
        let (into_key, derived) = derivation.recompute(&view);
        self.kv_put(collection, Value::Text(into_key.clone()), derived.clone())?;
        Ok((into_key, derived))
    }

    /// Fire a statechart event against the live KV `collection`: load its `Text`-keyed state (authorizes
    /// `Kv` Read), take the first matching guarded transition, and persist the new machine state and any
    /// action write back (each `kv_put` authorizes `Kv` Write) - grant-gated through `StateAccess`. A
    /// `NoTransition` maps to `Program`, a guard failure to `Denied`. Behind the `statecharts` feature.
    #[cfg(feature = "statecharts")]
    pub fn fire_machine(
        &mut self,
        collection: &str,
        machine: &crate::statechart::Machine,
        event: &str,
        inputs: &crate::guard::StateView,
        ledger_verified: bool,
    ) -> Result<crate::statechart::Step, ExecError> {
        let mut view = crate::guard::StateView::new();
        for (key, value) in self.kv_list(collection)? {
            if let Value::Text(k) = key {
                view.insert(k, value);
            }
        }
        let before = view.clone();
        let grants = self.context.grants.clone();
        let step = machine
            .fire(&mut view, event, inputs, &grants, ledger_verified)
            .map_err(|e| match e {
                crate::statechart::StepError::NoTransition { state, event } => ExecError::Program(
                    format!("statechart: no transition from {state:?} on event {event:?}"),
                ),
                crate::statechart::StepError::Guard(g) => {
                    ExecError::Denied(format!("statechart guard: {g:?}"))
                }
            })?;
        // Persist exactly the entries the transition changed or added, each Write-gated.
        for (k, v) in &view {
            if before.get(k) != Some(v) {
                self.kv_put(collection, Value::Text(k.clone()), v.clone())?;
            }
        }
        Ok(step)
    }

    /// Run one deterministic workflow pass over the live KV `collection`: load its `Text`-keyed state
    /// (authorizes `Kv` Read), cascade the trigger engine to a fixpoint (bounded by `budget`), and
    /// persist the derived entries the pass produced via `kv_put` (authorizes `Kv` Write). The pass
    /// treats the current state as newly-arrived (empty `before`) so every watched derivation
    /// materializes. Reactive firing (deciding *when* to run a pass) stays owned by spec 0029. A cascade
    /// that exceeds `budget` maps to `BudgetExceeded`. Behind the `workflows` feature.
    #[cfg(feature = "workflows")]
    pub fn run_workflow(
        &mut self,
        collection: &str,
        engine: &mut crate::workflow::TriggerEngine,
        budget: u64,
    ) -> Result<Vec<crate::workflow::FireReport>, ExecError> {
        let mut loaded = crate::derivation::KvView::new();
        for (key, value) in self.kv_list(collection)? {
            if let Value::Text(k) = key {
                loaded.insert(k, value);
            }
        }
        let before = crate::derivation::KvView::new();
        let mut after = loaded.clone();
        let reports = engine
            .on_change_to_fixpoint(&before, &mut after, budget)
            .map_err(|e| match e {
                crate::workflow::WorkflowError::Budget { budget } => {
                    ExecError::BudgetExceeded { budget }
                }
            })?;
        for (k, v) in &after {
            if loaded.get(k) != Some(v) {
                self.kv_put(collection, Value::Text(k.clone()), v.clone())?;
            }
        }
        Ok(reports)
    }

    pub fn cas_put(&mut self, bytes: &[u8]) -> Result<Digest, ExecError> {
        let digest = Digest::hash(self.loom.store().digest_algo(), bytes);
        self.authorize(Capability::Cas, Mode::Write, &digest.to_hex())?;
        Ok(cas_put(self.loom, self.context.workspace, bytes)?)
    }

    pub fn cas_get(&mut self, digest: &Digest) -> Result<Option<Vec<u8>>, ExecError> {
        self.authorize(Capability::Cas, Mode::Read, &digest.to_hex())?;
        Ok(cas_get(self.loom, self.context.workspace, digest)?)
    }

    pub fn cas_has(&mut self, digest: &Digest) -> Result<bool, ExecError> {
        self.authorize(Capability::Cas, Mode::Read, &digest.to_hex())?;
        Ok(cas_has(self.loom, self.context.workspace, digest)?)
    }

    pub fn cas_delete(&mut self, digest: &Digest) -> Result<bool, ExecError> {
        self.authorize(Capability::Cas, Mode::Write, &digest.to_hex())?;
        Ok(cas_delete(self.loom, self.context.workspace, digest)?)
    }

    pub fn cas_list(&mut self) -> Result<Vec<Digest>, ExecError> {
        self.authorize(Capability::Cas, Mode::Read, "")?;
        Ok(cas_list(self.loom, self.context.workspace)?)
    }

    /// Reconstruct a [`Digest`] from a raw 32-byte guest value, tagging it with the store's digest
    /// algorithm (the algorithm is a store-level property, not carried per digest).
    fn digest_from_raw(&self, raw: [u8; 32]) -> Digest {
        Digest::of(self.loom.store().digest_algo(), raw)
    }

    /// CAS read by raw 32-byte content address (the form the host ABI passes from the guest).
    pub fn cas_get_raw(&mut self, raw: [u8; 32]) -> Result<Option<Vec<u8>>, ExecError> {
        let digest = self.digest_from_raw(raw);
        self.cas_get(&digest)
    }

    /// CAS presence test by raw 32-byte content address.
    pub fn cas_has_raw(&mut self, raw: [u8; 32]) -> Result<bool, ExecError> {
        let digest = self.digest_from_raw(raw);
        self.cas_has(&digest)
    }

    /// CAS delete by raw 32-byte content address.
    pub fn cas_delete_raw(&mut self, raw: [u8; 32]) -> Result<bool, ExecError> {
        let digest = self.digest_from_raw(raw);
        self.cas_delete(&digest)
    }

    pub fn doc_put(&mut self, collection: &str, id: &str, doc: Vec<u8>) -> Result<(), ExecError> {
        let target = collection_item_target(collection, id);
        self.authorize(Capability::Document, Mode::Write, &target)?;
        Ok(doc_put(
            self.loom,
            self.context.workspace,
            collection,
            id,
            doc,
        )?)
    }

    pub fn doc_get(&mut self, collection: &str, id: &str) -> Result<Option<Vec<u8>>, ExecError> {
        let target = collection_item_target(collection, id);
        self.authorize(Capability::Document, Mode::Read, &target)?;
        Ok(doc_get(self.loom, self.context.workspace, collection, id)?)
    }

    pub fn doc_delete(&mut self, collection: &str, id: &str) -> Result<bool, ExecError> {
        let target = collection_item_target(collection, id);
        self.authorize(Capability::Document, Mode::Write, &target)?;
        Ok(doc_delete(
            self.loom,
            self.context.workspace,
            collection,
            id,
        )?)
    }

    pub fn doc_list(&mut self, collection: &str) -> Result<Vec<(String, Vec<u8>)>, ExecError> {
        let target = collection_target(collection);
        self.authorize(Capability::Document, Mode::Read, &target)?;
        Ok(doc_list(self.loom, self.context.workspace, collection)?
            .iter()
            .map(|(id, doc)| (id.to_string(), doc.to_vec()))
            .collect())
    }

    pub fn queue_append(&mut self, stream: &str, entry: &[u8]) -> Result<usize, ExecError> {
        self.authorize(Capability::Queue, Mode::Write, stream)?;
        Ok(log::append(
            self.loom,
            self.context.workspace,
            stream,
            entry,
        )?)
    }

    pub fn queue_get(&mut self, stream: &str, seq: usize) -> Result<Option<Vec<u8>>, ExecError> {
        let target = seq_target(stream, seq);
        self.authorize(Capability::Queue, Mode::Read, &target)?;
        Ok(log::get(self.loom, self.context.workspace, stream, seq)?)
    }

    pub fn queue_range(
        &mut self,
        stream: &str,
        lo: usize,
        hi: usize,
    ) -> Result<Vec<Vec<u8>>, ExecError> {
        self.authorize(Capability::Queue, Mode::Read, stream)?;
        Ok(log::range(
            self.loom,
            self.context.workspace,
            stream,
            lo,
            hi,
        )?)
    }

    pub fn queue_len(&mut self, stream: &str) -> Result<usize, ExecError> {
        self.authorize(Capability::Queue, Mode::Read, stream)?;
        Ok(log::len(self.loom, self.context.workspace, stream)?)
    }

    pub fn time_series_put(
        &mut self,
        collection: &str,
        timestamp: i64,
        value: Vec<u8>,
    ) -> Result<(), ExecError> {
        let target = timestamp_target(collection, timestamp);
        self.authorize(Capability::TimeSeries, Mode::Write, &target)?;
        Ok(ts_put(
            self.loom,
            self.context.workspace,
            collection,
            timestamp,
            value,
        )?)
    }

    pub fn time_series_get(
        &mut self,
        collection: &str,
        timestamp: i64,
    ) -> Result<Option<Vec<u8>>, ExecError> {
        let target = timestamp_target(collection, timestamp);
        self.authorize(Capability::TimeSeries, Mode::Read, &target)?;
        Ok(ts_get(
            self.loom,
            self.context.workspace,
            collection,
            timestamp,
        )?)
    }

    pub fn time_series_range(
        &mut self,
        collection: &str,
        from: i64,
        to: i64,
    ) -> Result<Series, ExecError> {
        self.authorize(Capability::TimeSeries, Mode::Read, collection)?;
        Ok(ts_range(
            self.loom,
            self.context.workspace,
            collection,
            from,
            to,
        )?)
    }

    pub fn time_series_latest(
        &mut self,
        collection: &str,
    ) -> Result<Option<(i64, Vec<u8>)>, ExecError> {
        self.authorize(Capability::TimeSeries, Mode::Read, collection)?;
        Ok(ts_latest(self.loom, self.context.workspace, collection)?)
    }

    pub fn ledger_append(&mut self, collection: &str, payload: Vec<u8>) -> Result<u64, ExecError> {
        self.authorize(Capability::Ledger, Mode::Write, collection)?;
        Ok(ledger_append(
            self.loom,
            self.context.workspace,
            collection,
            payload,
        )?)
    }

    pub fn ledger_get(&mut self, collection: &str, seq: u64) -> Result<Option<Vec<u8>>, ExecError> {
        let target = seq_target(collection, seq);
        self.authorize(Capability::Ledger, Mode::Read, &target)?;
        Ok(ledger_get(
            self.loom,
            self.context.workspace,
            collection,
            seq,
        )?)
    }

    pub fn ledger_head(&mut self, collection: &str) -> Result<Option<Digest>, ExecError> {
        self.authorize(Capability::Ledger, Mode::Read, collection)?;
        Ok(ledger_head(self.loom, self.context.workspace, collection)?)
    }

    pub fn ledger_len(&mut self, collection: &str) -> Result<u64, ExecError> {
        self.authorize(Capability::Ledger, Mode::Read, collection)?;
        Ok(ledger_len(self.loom, self.context.workspace, collection)?)
    }

    pub fn ledger_verify(&mut self, collection: &str) -> Result<(), ExecError> {
        self.authorize(Capability::Ledger, Mode::Read, collection)?;
        Ok(ledger_verify(
            self.loom,
            self.context.workspace,
            collection,
        )?)
    }

    pub fn graph_upsert_node(
        &mut self,
        graph: &str,
        id: &str,
        props: Props,
    ) -> Result<(), ExecError> {
        let target = collection_item_target(graph, id);
        self.authorize(Capability::Graph, Mode::Write, &target)?;
        Ok(graph_upsert_node(
            self.loom,
            self.context.workspace,
            graph,
            id,
            props,
        )?)
    }

    pub fn graph_get_node(&mut self, graph: &str, id: &str) -> Result<Option<Props>, ExecError> {
        let target = collection_item_target(graph, id);
        self.authorize(Capability::Graph, Mode::Read, &target)?;
        Ok(graph_get_node(
            self.loom,
            self.context.workspace,
            graph,
            id,
        )?)
    }

    pub fn graph_remove_node(
        &mut self,
        graph: &str,
        id: &str,
        cascade: bool,
    ) -> Result<(), ExecError> {
        let target = collection_item_target(graph, id);
        self.authorize(Capability::Graph, Mode::Write, &target)?;
        Ok(graph_remove_node(
            self.loom,
            self.context.workspace,
            graph,
            id,
            cascade,
        )?)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn graph_upsert_edge(
        &mut self,
        graph: &str,
        id: &str,
        src: &str,
        dst: &str,
        label: &str,
        props: Props,
    ) -> Result<(), ExecError> {
        let target = collection_item_target(graph, id);
        self.authorize(Capability::Graph, Mode::Write, &target)?;
        Ok(graph_upsert_edge(
            self.loom,
            self.context.workspace,
            graph,
            id,
            src,
            dst,
            label,
            props,
        )?)
    }

    pub fn graph_get_edge(&mut self, graph: &str, id: &str) -> Result<Option<Edge>, ExecError> {
        let target = collection_item_target(graph, id);
        self.authorize(Capability::Graph, Mode::Read, &target)?;
        Ok(graph_get_edge(
            self.loom,
            self.context.workspace,
            graph,
            id,
        )?)
    }

    pub fn graph_remove_edge(&mut self, graph: &str, id: &str) -> Result<bool, ExecError> {
        let target = collection_item_target(graph, id);
        self.authorize(Capability::Graph, Mode::Write, &target)?;
        Ok(graph_remove_edge(
            self.loom,
            self.context.workspace,
            graph,
            id,
        )?)
    }

    pub fn graph_neighbors(&mut self, graph: &str, id: &str) -> Result<Vec<String>, ExecError> {
        let target = collection_item_target(graph, id);
        self.authorize(Capability::Graph, Mode::Read, &target)?;
        Ok(graph_neighbors(
            self.loom,
            self.context.workspace,
            graph,
            id,
        )?)
    }

    pub fn graph_out_edges(
        &mut self,
        graph: &str,
        id: &str,
    ) -> Result<Vec<(String, Edge)>, ExecError> {
        let target = collection_item_target(graph, id);
        self.authorize(Capability::Graph, Mode::Read, &target)?;
        Ok(graph_out_edges(
            self.loom,
            self.context.workspace,
            graph,
            id,
        )?)
    }

    pub fn graph_in_edges(
        &mut self,
        graph: &str,
        id: &str,
    ) -> Result<Vec<(String, Edge)>, ExecError> {
        let target = collection_item_target(graph, id);
        self.authorize(Capability::Graph, Mode::Read, &target)?;
        Ok(graph_in_edges(
            self.loom,
            self.context.workspace,
            graph,
            id,
        )?)
    }

    pub fn graph_reachable(
        &mut self,
        graph: &str,
        start: &str,
        max_depth: Option<usize>,
        via_label: Option<&str>,
    ) -> Result<Vec<String>, ExecError> {
        let target = collection_item_target(graph, start);
        self.authorize(Capability::Graph, Mode::Read, &target)?;
        Ok(graph_reachable(
            self.loom,
            self.context.workspace,
            graph,
            start,
            max_depth,
            via_label,
        )?)
    }

    pub fn graph_shortest_path(
        &mut self,
        graph: &str,
        from: &str,
        to: &str,
        via_label: Option<&str>,
    ) -> Result<Option<Vec<String>>, ExecError> {
        let target = collection_item_target(graph, from);
        self.authorize(Capability::Graph, Mode::Read, &target)?;
        Ok(graph_shortest_path(
            self.loom,
            self.context.workspace,
            graph,
            from,
            to,
            via_label,
        )?)
    }

    pub fn columnar_create(
        &mut self,
        dataset: &str,
        columns: Vec<(String, ColumnType)>,
        target_segment_rows: usize,
    ) -> Result<(), ExecError> {
        self.authorize(Capability::Columnar, Mode::Write, dataset)?;
        Ok(columnar_create(
            self.loom,
            self.context.workspace,
            dataset,
            columns,
            target_segment_rows,
        )?)
    }

    pub fn columnar_append(&mut self, dataset: &str, row: Vec<Value>) -> Result<(), ExecError> {
        self.authorize(Capability::Columnar, Mode::Write, dataset)?;
        Ok(columnar_append(
            self.loom,
            self.context.workspace,
            dataset,
            row,
        )?)
    }

    pub fn columnar_scan(&mut self, dataset: &str) -> Result<Vec<Vec<Value>>, ExecError> {
        self.authorize(Capability::Columnar, Mode::Read, dataset)?;
        Ok(columnar_scan(self.loom, self.context.workspace, dataset)?)
    }

    pub fn columnar_columns(
        &mut self,
        dataset: &str,
    ) -> Result<Vec<(String, ColumnType)>, ExecError> {
        self.authorize(Capability::Columnar, Mode::Read, dataset)?;
        Ok(columnar_columns(
            self.loom,
            self.context.workspace,
            dataset,
        )?)
    }

    pub fn columnar_rows(&mut self, dataset: &str) -> Result<usize, ExecError> {
        self.authorize(Capability::Columnar, Mode::Read, dataset)?;
        Ok(columnar_rows(self.loom, self.context.workspace, dataset)?)
    }

    pub fn columnar_select(
        &mut self,
        dataset: &str,
        columns: &[&str],
        filter: Option<(&str, loom_core::CmpOp, &Value)>,
    ) -> Result<Vec<Vec<Value>>, ExecError> {
        self.authorize(Capability::Columnar, Mode::Read, dataset)?;
        Ok(columnar_select(
            self.loom,
            self.context.workspace,
            dataset,
            columns,
            filter,
        )?)
    }

    pub fn columnar_aggregate(
        &mut self,
        dataset: &str,
        aggregates: &[ColumnarAggregate],
        filter: Option<(&str, loom_core::CmpOp, &Value)>,
    ) -> Result<Vec<Value>, ExecError> {
        self.authorize(Capability::Columnar, Mode::Read, dataset)?;
        Ok(loom_core::columnar_aggregate(
            self.loom,
            self.context.workspace,
            dataset,
            aggregates,
            filter,
        )?)
    }

    pub fn search_create(&mut self, collection: &str, mapping: Mapping) -> Result<(), ExecError> {
        self.authorize(Capability::Search, Mode::Write, collection)?;
        Ok(search_create(
            self.loom,
            self.context.workspace,
            collection,
            mapping,
        )?)
    }

    pub fn search_index(
        &mut self,
        collection: &str,
        id: Vec<u8>,
        doc: Document,
    ) -> Result<(), ExecError> {
        let target = bytes_item_target(collection, &id);
        self.authorize(Capability::Search, Mode::Write, &target)?;
        Ok(search_index(
            self.loom,
            self.context.workspace,
            collection,
            id,
            doc,
        )?)
    }

    pub fn search_get(
        &mut self,
        collection: &str,
        id: &[u8],
    ) -> Result<Option<Document>, ExecError> {
        let target = bytes_item_target(collection, id);
        self.authorize(Capability::Search, Mode::Read, &target)?;
        Ok(search_get(
            self.loom,
            self.context.workspace,
            collection,
            id,
        )?)
    }

    pub fn search_delete(&mut self, collection: &str, id: &[u8]) -> Result<bool, ExecError> {
        let target = bytes_item_target(collection, id);
        self.authorize(Capability::Search, Mode::Write, &target)?;
        Ok(search_delete(
            self.loom,
            self.context.workspace,
            collection,
            id,
        )?)
    }

    pub fn search_ids(
        &mut self,
        collection: &str,
        prefix: Option<&[u8]>,
    ) -> Result<Vec<Vec<u8>>, ExecError> {
        self.authorize(Capability::Search, Mode::Read, collection)?;
        Ok(search_ids(
            self.loom,
            self.context.workspace,
            collection,
            prefix,
        )?)
    }

    pub fn search_remap(&mut self, collection: &str, mapping: Mapping) -> Result<(), ExecError> {
        self.authorize(Capability::Search, Mode::Write, collection)?;
        Ok(search_remap(
            self.loom,
            self.context.workspace,
            collection,
            mapping,
        )?)
    }

    pub fn search_query(
        &mut self,
        collection: &str,
        request: &QueryRequest,
    ) -> Result<QueryResponse, ExecError> {
        self.authorize(Capability::Search, Mode::Read, collection)?;
        Ok(search_query(
            self.loom,
            self.context.workspace,
            collection,
            request,
        )?)
    }

    pub fn vector_create(
        &mut self,
        set: &str,
        dim: usize,
        metric: Metric,
    ) -> Result<(), ExecError> {
        self.authorize(Capability::Vector, Mode::Write, set)?;
        Ok(vector_create(
            self.loom,
            self.context.workspace,
            set,
            dim,
            metric,
        )?)
    }

    pub fn vector_upsert(
        &mut self,
        set: &str,
        id: &str,
        vector: Vec<f32>,
        metadata: std::collections::BTreeMap<String, Value>,
    ) -> Result<(), ExecError> {
        let target = collection_item_target(set, id);
        self.authorize(Capability::Vector, Mode::Write, &target)?;
        Ok(vector_upsert(
            self.loom,
            self.context.workspace,
            set,
            id,
            vector,
            metadata,
        )?)
    }

    pub fn vector_get(&mut self, set: &str, id: &str) -> Result<Option<VectorEntry>, ExecError> {
        let target = collection_item_target(set, id);
        self.authorize(Capability::Vector, Mode::Read, &target)?;
        Ok(vector_get(self.loom, self.context.workspace, set, id)?)
    }

    pub fn vector_source_text(&mut self, set: &str, id: &str) -> Result<Option<String>, ExecError> {
        let target = collection_item_target(set, id);
        self.authorize(Capability::Vector, Mode::Read, &target)?;
        Ok(vector_source_text(
            self.loom,
            self.context.workspace,
            set,
            id,
        )?)
    }

    pub fn vector_ids(
        &mut self,
        set: &str,
        prefix: Option<&str>,
    ) -> Result<Vec<String>, ExecError> {
        self.authorize(Capability::Vector, Mode::Read, set)?;
        Ok(vector_ids(self.loom, self.context.workspace, set, prefix)?)
    }

    pub fn vector_delete(&mut self, set: &str, id: &str) -> Result<bool, ExecError> {
        let target = collection_item_target(set, id);
        self.authorize(Capability::Vector, Mode::Write, &target)?;
        Ok(vector_delete(self.loom, self.context.workspace, set, id)?)
    }

    pub fn vector_search(
        &mut self,
        set: &str,
        query: &[f32],
        k: usize,
        filter: &MetaFilter,
    ) -> Result<Vec<Hit>, ExecError> {
        self.authorize(Capability::Vector, Mode::Read, set)?;
        Ok(vector_search(
            self.loom,
            self.context.workspace,
            set,
            query,
            k,
            filter,
        )?)
    }

    pub fn dataframe_create(&mut self, frame: &str, plan: &DataframePlan) -> Result<(), ExecError> {
        self.authorize(Capability::Dataframe, Mode::Write, frame)?;
        Ok(dataframe_create(
            self.loom,
            self.context.workspace,
            frame,
            plan,
        )?)
    }

    pub fn dataframe_put_plan(
        &mut self,
        frame: &str,
        plan: &DataframePlan,
    ) -> Result<(), ExecError> {
        self.authorize(Capability::Dataframe, Mode::Write, frame)?;
        Ok(put_dataframe_plan(
            self.loom,
            self.context.workspace,
            frame,
            plan,
        )?)
    }

    pub fn dataframe_get_plan(&mut self, frame: &str) -> Result<DataframePlan, ExecError> {
        self.authorize(Capability::Dataframe, Mode::Read, frame)?;
        Ok(get_dataframe_plan(
            self.loom,
            self.context.workspace,
            frame,
        )?)
    }

    pub fn dataframe_collect(&mut self, frame: &str) -> Result<DataframeBatch, ExecError> {
        self.authorize(Capability::Dataframe, Mode::Read, frame)?;
        let plan = get_dataframe_plan(self.loom, self.context.workspace, frame)?;
        self.authorize_dataframe_sources(&plan)?;
        Ok(dataframe_collect(self.loom, self.context.workspace, frame)?)
    }

    pub fn dataframe_preview(
        &mut self,
        frame: &str,
        rows: u64,
    ) -> Result<DataframeBatch, ExecError> {
        self.authorize(Capability::Dataframe, Mode::Read, frame)?;
        let plan = get_dataframe_plan(self.loom, self.context.workspace, frame)?;
        self.authorize_dataframe_sources(&plan)?;
        Ok(dataframe_preview(
            self.loom,
            self.context.workspace,
            frame,
            rows,
        )?)
    }

    pub fn dataframe_materialize(&mut self, frame: &str) -> Result<Option<Digest>, ExecError> {
        self.authorize(Capability::Dataframe, Mode::Write, frame)?;
        let plan = get_dataframe_plan(self.loom, self.context.workspace, frame)?;
        self.authorize_dataframe_sources(&plan)?;
        self.authorize_dataframe_materialization(&plan)?;
        Ok(dataframe_materialize(
            self.loom,
            self.context.workspace,
            frame,
        )?)
    }

    pub fn dataframe_plan_digest(&mut self, frame: &str) -> Result<Digest, ExecError> {
        self.authorize(Capability::Dataframe, Mode::Read, frame)?;
        Ok(dataframe_plan_digest(
            self.loom,
            self.context.workspace,
            frame,
        )?)
    }

    pub fn dataframe_source_digests(&mut self, frame: &str) -> Result<Vec<Digest>, ExecError> {
        self.authorize(Capability::Dataframe, Mode::Read, frame)?;
        Ok(dataframe_source_digests(
            self.loom,
            self.context.workspace,
            frame,
        )?)
    }

    pub fn calendar_create_collection(
        &mut self,
        principal: &str,
        collection: &str,
        meta: &CollectionMeta,
    ) -> Result<(), ExecError> {
        let target = principal_collection_target(principal, collection);
        self.authorize(Capability::Calendar, Mode::Write, &target)?;
        Ok(calendar::create_collection(
            self.loom,
            self.context.workspace,
            principal,
            collection,
            meta,
        )?)
    }

    pub fn calendar_get_collection(
        &mut self,
        principal: &str,
        collection: &str,
    ) -> Result<Option<CollectionMeta>, ExecError> {
        let target = principal_collection_target(principal, collection);
        self.authorize(Capability::Calendar, Mode::Read, &target)?;
        Ok(calendar::get_collection(
            self.loom,
            self.context.workspace,
            principal,
            collection,
        )?)
    }

    pub fn calendar_list_collections(&mut self, principal: &str) -> Result<Vec<String>, ExecError> {
        let target = principal_target(principal);
        self.authorize(Capability::Calendar, Mode::Read, &target)?;
        Ok(calendar::list_collections(
            self.loom,
            self.context.workspace,
            principal,
        )?)
    }

    pub fn calendar_delete_collection(
        &mut self,
        principal: &str,
        collection: &str,
    ) -> Result<bool, ExecError> {
        let target = principal_collection_target(principal, collection);
        self.authorize(Capability::Calendar, Mode::Write, &target)?;
        Ok(calendar::delete_collection(
            self.loom,
            self.context.workspace,
            principal,
            collection,
        )?)
    }

    pub fn calendar_put_entry(
        &mut self,
        principal: &str,
        collection: &str,
        entry: &CalendarEntry,
    ) -> Result<Digest, ExecError> {
        let target = principal_item_target(principal, collection, &entry.uid);
        self.authorize(Capability::Calendar, Mode::Write, &target)?;
        Ok(calendar::put_entry(
            self.loom,
            self.context.workspace,
            principal,
            collection,
            entry,
        )?)
    }

    pub fn calendar_get_entry(
        &mut self,
        principal: &str,
        collection: &str,
        uid: &str,
    ) -> Result<Option<CalendarEntry>, ExecError> {
        let target = principal_item_target(principal, collection, uid);
        self.authorize(Capability::Calendar, Mode::Read, &target)?;
        Ok(calendar::get_entry(
            self.loom,
            self.context.workspace,
            principal,
            collection,
            uid,
        )?)
    }

    pub fn calendar_delete_entry(
        &mut self,
        principal: &str,
        collection: &str,
        uid: &str,
    ) -> Result<bool, ExecError> {
        let target = principal_item_target(principal, collection, uid);
        self.authorize(Capability::Calendar, Mode::Write, &target)?;
        Ok(calendar::delete_entry(
            self.loom,
            self.context.workspace,
            principal,
            collection,
            uid,
        )?)
    }

    pub fn calendar_list_entries(
        &mut self,
        principal: &str,
        collection: &str,
    ) -> Result<Vec<CalendarEntry>, ExecError> {
        let target = principal_collection_target(principal, collection);
        self.authorize(Capability::Calendar, Mode::Read, &target)?;
        Ok(calendar::list_entries(
            self.loom,
            self.context.workspace,
            principal,
            collection,
        )?)
    }

    pub fn calendar_search(
        &mut self,
        principal: &str,
        collection: &str,
        component: Option<Component>,
        text: Option<&str>,
    ) -> Result<Vec<CalendarEntry>, ExecError> {
        let target = principal_collection_target(principal, collection);
        self.authorize(Capability::Calendar, Mode::Read, &target)?;
        Ok(calendar::search(
            self.loom,
            self.context.workspace,
            principal,
            collection,
            component,
            text,
        )?)
    }

    pub fn contacts_create_book(
        &mut self,
        principal: &str,
        book: &str,
        meta: &BookMeta,
    ) -> Result<(), ExecError> {
        let target = principal_collection_target(principal, book);
        self.authorize(Capability::Contacts, Mode::Write, &target)?;
        Ok(contacts::create_book(
            self.loom,
            self.context.workspace,
            principal,
            book,
            meta,
        )?)
    }

    pub fn contacts_get_book(
        &mut self,
        principal: &str,
        book: &str,
    ) -> Result<Option<BookMeta>, ExecError> {
        let target = principal_collection_target(principal, book);
        self.authorize(Capability::Contacts, Mode::Read, &target)?;
        Ok(contacts::get_book(
            self.loom,
            self.context.workspace,
            principal,
            book,
        )?)
    }

    pub fn contacts_list_books(&mut self, principal: &str) -> Result<Vec<String>, ExecError> {
        let target = principal_target(principal);
        self.authorize(Capability::Contacts, Mode::Read, &target)?;
        Ok(contacts::list_books(
            self.loom,
            self.context.workspace,
            principal,
        )?)
    }

    pub fn contacts_delete_book(&mut self, principal: &str, book: &str) -> Result<bool, ExecError> {
        let target = principal_collection_target(principal, book);
        self.authorize(Capability::Contacts, Mode::Write, &target)?;
        Ok(contacts::delete_book(
            self.loom,
            self.context.workspace,
            principal,
            book,
        )?)
    }

    pub fn contacts_put_entry(
        &mut self,
        principal: &str,
        book: &str,
        entry: &ContactEntry,
    ) -> Result<Digest, ExecError> {
        let target = principal_item_target(principal, book, &entry.uid);
        self.authorize(Capability::Contacts, Mode::Write, &target)?;
        Ok(contacts::put_entry(
            self.loom,
            self.context.workspace,
            principal,
            book,
            entry,
        )?)
    }

    pub fn contacts_get_entry(
        &mut self,
        principal: &str,
        book: &str,
        uid: &str,
    ) -> Result<Option<ContactEntry>, ExecError> {
        let target = principal_item_target(principal, book, uid);
        self.authorize(Capability::Contacts, Mode::Read, &target)?;
        Ok(contacts::get_entry(
            self.loom,
            self.context.workspace,
            principal,
            book,
            uid,
        )?)
    }

    pub fn contacts_delete_entry(
        &mut self,
        principal: &str,
        book: &str,
        uid: &str,
    ) -> Result<bool, ExecError> {
        let target = principal_item_target(principal, book, uid);
        self.authorize(Capability::Contacts, Mode::Write, &target)?;
        Ok(contacts::delete_entry(
            self.loom,
            self.context.workspace,
            principal,
            book,
            uid,
        )?)
    }

    pub fn contacts_list_entries(
        &mut self,
        principal: &str,
        book: &str,
    ) -> Result<Vec<ContactEntry>, ExecError> {
        let target = principal_collection_target(principal, book);
        self.authorize(Capability::Contacts, Mode::Read, &target)?;
        Ok(contacts::list_entries(
            self.loom,
            self.context.workspace,
            principal,
            book,
        )?)
    }

    pub fn contacts_search(
        &mut self,
        principal: &str,
        book: &str,
        text: &str,
    ) -> Result<Vec<ContactEntry>, ExecError> {
        let target = principal_collection_target(principal, book);
        self.authorize(Capability::Contacts, Mode::Read, &target)?;
        Ok(contacts::search(
            self.loom,
            self.context.workspace,
            principal,
            book,
            text,
        )?)
    }

    pub fn mail_create_mailbox(
        &mut self,
        principal: &str,
        mailbox: &str,
        meta: &MailboxMeta,
    ) -> Result<(), ExecError> {
        let target = principal_collection_target(principal, mailbox);
        self.authorize(Capability::Mail, Mode::Write, &target)?;
        Ok(mail::create_mailbox(
            self.loom,
            self.context.workspace,
            principal,
            mailbox,
            meta,
        )?)
    }

    pub fn mail_get_mailbox(
        &mut self,
        principal: &str,
        mailbox: &str,
    ) -> Result<Option<MailboxMeta>, ExecError> {
        let target = principal_collection_target(principal, mailbox);
        self.authorize(Capability::Mail, Mode::Read, &target)?;
        Ok(mail::get_mailbox(
            self.loom,
            self.context.workspace,
            principal,
            mailbox,
        )?)
    }

    pub fn mail_list_mailboxes(&mut self, principal: &str) -> Result<Vec<String>, ExecError> {
        let target = principal_target(principal);
        self.authorize(Capability::Mail, Mode::Read, &target)?;
        Ok(mail::list_mailboxes(
            self.loom,
            self.context.workspace,
            principal,
        )?)
    }

    pub fn mail_delete_mailbox(
        &mut self,
        principal: &str,
        mailbox: &str,
    ) -> Result<bool, ExecError> {
        let target = principal_collection_target(principal, mailbox);
        self.authorize(Capability::Mail, Mode::Write, &target)?;
        Ok(mail::delete_mailbox(
            self.loom,
            self.context.workspace,
            principal,
            mailbox,
        )?)
    }

    pub fn mail_ingest_message(
        &mut self,
        principal: &str,
        mailbox: &str,
        uid: &str,
        raw: &[u8],
    ) -> Result<Digest, ExecError> {
        let target = principal_item_target(principal, mailbox, uid);
        self.authorize(Capability::Mail, Mode::Write, &target)?;
        Ok(mail::ingest_message(
            self.loom,
            self.context.workspace,
            principal,
            mailbox,
            uid,
            raw,
        )?)
    }

    pub fn mail_get_message(
        &mut self,
        principal: &str,
        mailbox: &str,
        uid: &str,
    ) -> Result<Option<MailMessage>, ExecError> {
        let target = principal_item_target(principal, mailbox, uid);
        self.authorize(Capability::Mail, Mode::Read, &target)?;
        Ok(mail::get_message(
            self.loom,
            self.context.workspace,
            principal,
            mailbox,
            uid,
        )?)
    }

    pub fn mail_to_eml(
        &mut self,
        principal: &str,
        mailbox: &str,
        uid: &str,
    ) -> Result<Option<Vec<u8>>, ExecError> {
        let target = principal_item_target(principal, mailbox, uid);
        self.authorize(Capability::Mail, Mode::Read, &target)?;
        Ok(mail::to_eml(
            self.loom,
            self.context.workspace,
            principal,
            mailbox,
            uid,
        )?)
    }

    pub fn mail_delete_message(
        &mut self,
        principal: &str,
        mailbox: &str,
        uid: &str,
    ) -> Result<bool, ExecError> {
        let target = principal_item_target(principal, mailbox, uid);
        self.authorize(Capability::Mail, Mode::Write, &target)?;
        Ok(mail::delete_message(
            self.loom,
            self.context.workspace,
            principal,
            mailbox,
            uid,
        )?)
    }

    pub fn mail_list_messages(
        &mut self,
        principal: &str,
        mailbox: &str,
    ) -> Result<Vec<MailMessage>, ExecError> {
        let target = principal_collection_target(principal, mailbox);
        self.authorize(Capability::Mail, Mode::Read, &target)?;
        Ok(mail::list_messages(
            self.loom,
            self.context.workspace,
            principal,
            mailbox,
        )?)
    }

    pub fn mail_get_flags(
        &mut self,
        principal: &str,
        mailbox: &str,
        uid: &str,
    ) -> Result<Vec<String>, ExecError> {
        let target = principal_item_target(principal, mailbox, uid);
        self.authorize(Capability::Mail, Mode::Read, &target)?;
        Ok(mail::get_flags(
            self.loom,
            self.context.workspace,
            principal,
            mailbox,
            uid,
        )?)
    }

    pub fn mail_set_flags(
        &mut self,
        principal: &str,
        mailbox: &str,
        uid: &str,
        flags: &[String],
    ) -> Result<(), ExecError> {
        let target = principal_item_target(principal, mailbox, uid);
        self.authorize(Capability::Mail, Mode::Write, &target)?;
        Ok(mail::set_flags(
            self.loom,
            self.context.workspace,
            principal,
            mailbox,
            uid,
            flags,
        )?)
    }

    pub fn mail_search(
        &mut self,
        principal: &str,
        mailbox: &str,
        text: &str,
    ) -> Result<Vec<MailMessage>, ExecError> {
        let target = principal_collection_target(principal, mailbox);
        self.authorize(Capability::Mail, Mode::Read, &target)?;
        Ok(mail::search(
            self.loom,
            self.context.workspace,
            principal,
            mailbox,
            text,
        )?)
    }

    pub fn commit(
        &mut self,
        author: &str,
        message: &str,
        timestamp_ms: u64,
    ) -> Result<Digest, ExecError> {
        Ok(self
            .loom
            .commit(self.context.workspace, author, message, timestamp_ms)?)
    }

    fn authorize(&self, facet: Capability, mode: Mode, target: &str) -> Result<(), ExecError> {
        self.context
            .authorize_operation(self.loom.acl_store(), facet, mode, target)
    }

    fn authorize_dataframe_sources(&self, plan: &DataframePlan) -> Result<(), ExecError> {
        for source in &plan.sources {
            match source.kind {
                DataframeSourceKind::Files => {
                    self.authorize(Capability::Files, Mode::Read, &source.target)?;
                }
                DataframeSourceKind::Cas | DataframeSourceKind::SqlResult => {
                    let digest = Digest::parse(&source.target)?;
                    self.authorize(Capability::Cas, Mode::Read, &digest.to_hex())?;
                }
                DataframeSourceKind::Columnar => {
                    self.authorize(Capability::Columnar, Mode::Read, &source.target)?;
                }
            }
        }
        Ok(())
    }

    fn authorize_dataframe_materialization(&self, plan: &DataframePlan) -> Result<(), ExecError> {
        let Some(materialization) = &plan.materialization else {
            return Ok(());
        };
        match materialization.target {
            DataframeMaterializationTarget::Columnar => {
                let destination = materialization.destination.as_deref().ok_or_else(|| {
                    loom_core::LoomError::invalid("columnar materialization requires destination")
                })?;
                self.authorize(Capability::Columnar, Mode::Write, destination)?;
            }
            DataframeMaterializationTarget::Files => {
                let destination = materialization.destination.as_deref().ok_or_else(|| {
                    loom_core::LoomError::invalid("file materialization requires destination")
                })?;
                self.authorize(Capability::Files, Mode::Write, destination)?;
            }
            DataframeMaterializationTarget::Cas => {
                self.authorize(Capability::Cas, Mode::Write, "")?;
            }
            DataframeMaterializationTarget::EphemeralPreview => {}
        }
        Ok(())
    }
}

fn kv_collection_target(collection: &str) -> String {
    format!("{collection}/")
}

#[cfg(feature = "sql-state-access")]
fn sql_db_target(db: &str) -> String {
    format!("{db}/")
}

fn collection_target(collection: &str) -> String {
    format!("{collection}/")
}

fn collection_item_target(collection: &str, id: &str) -> String {
    format!("{collection}/{id}")
}

fn principal_target(principal: &str) -> String {
    format!("{principal}/")
}

fn principal_collection_target(principal: &str, collection: &str) -> String {
    format!("{principal}/{collection}")
}

fn principal_item_target(principal: &str, collection: &str, id: &str) -> String {
    format!("{principal}/{collection}/{id}")
}

fn seq_target(collection: &str, seq: impl std::fmt::Display) -> String {
    format!("{collection}/{seq}")
}

fn timestamp_target(collection: &str, timestamp: i64) -> String {
    format!("{collection}/{timestamp}")
}

fn bytes_item_target(collection: &str, id: &[u8]) -> String {
    let mut out = collection_target(collection);
    for byte in id {
        write!(&mut out, "{byte:02x}").expect("write to string");
    }
    out
}

fn kv_target(collection: &str, key: &Value) -> String {
    let mut out = kv_collection_target(collection);
    for byte in loom_core::key_to_cbor(key) {
        write!(&mut out, "{byte:02x}").expect("write to string");
    }
    out
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use loom_core::{
        AclEffect, AclGrant, AclRight, AclScope, AclScopeKind, AclSubject, DataframeInputFormat,
        DataframeMaterialization, DataframeOperation, DataframeSourceBinding, FacetKind,
        MemoryStore, WorkspaceId,
    };

    use super::*;
    use crate::capability::{Grant, GrantSet, Scope};

    fn nid(seed: u8) -> WorkspaceId {
        WorkspaceId::from_bytes([seed; 16])
    }

    fn pid(seed: u8) -> loom_core::PrincipalId {
        loom_core::PrincipalId::from_bytes([seed; 16])
    }

    fn context(ns: WorkspaceId, grants: GrantSet) -> ExecContext {
        ExecContext {
            workspace: ns,
            principal: pid(9),
            roles: Vec::new(),
            authenticated: true,
            base_branch: "main".to_string(),
            grants,
        }
    }

    fn allow_all_exec(loom: &mut Loom<MemoryStore>, ns: WorkspaceId) {
        loom.acl_store_mut()
            .allow(
                AclSubject::Principal(pid(9)),
                Some(ns),
                None,
                [AclRight::Execute],
            )
            .unwrap();
    }

    fn all_facet_grants() -> GrantSet {
        GrantSet::all_facets()
    }

    fn state_loom(seed: u8) -> (Loom<MemoryStore>, WorkspaceId) {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Files, None, nid(seed))
            .unwrap();
        (loom, ns)
    }

    #[test]
    fn files_and_kv_are_real_committed_state() {
        let (mut loom, ns) = state_loom(1);
        allow_all_exec(&mut loom, ns);
        let grants = GrantSet::new(vec![
            Grant {
                facet: Capability::Files,
                mode: Mode::ReadWrite,
                scopes: vec![Scope::All],
            },
            Grant {
                facet: Capability::Kv,
                mode: Mode::ReadWrite,
                scopes: vec![Scope::All],
            },
        ]);
        let key = Value::Text("k1".to_string());
        let commit = {
            let mut state = StateAccess::new(&mut loom, context(ns, grants));
            state.file_write("/reports/q1.txt", b"report").unwrap();
            state
                .kv_put("cache", key.clone(), b"value".to_vec())
                .unwrap();
            assert_eq!(state.file_read("/reports/q1.txt").unwrap(), b"report");
            assert_eq!(
                state.kv_get("cache", &key).unwrap(),
                Some(b"value".to_vec())
            );
            state.commit("program", "state access", 1).unwrap()
        };

        loom.remove_file(ns, "/reports/q1.txt").unwrap();
        kv_delete(&mut loom, ns, "cache", &key).unwrap();
        loom.checkout_commit(ns, commit).unwrap();

        assert_eq!(loom.read_file(ns, "/reports/q1.txt").unwrap(), b"report");
        assert_eq!(
            kv_get(&loom, ns, "cache", &key).unwrap(),
            Some(b"value".to_vec())
        );
    }

    #[test]
    fn scoped_exec_acl_limits_kv_access() {
        let (mut loom, ns) = state_loom(2);
        loom.acl_store_mut()
            .grant(AclGrant {
                subject: AclSubject::Principal(pid(9)),
                workspace: Some(ns),
                domain: Some(Capability::Kv.into()),
                ref_glob: None,
                scopes: vec![AclScope::Prefix {
                    kind: AclScopeKind::Exec,
                    prefix: b"allowed/".to_vec(),
                }],
                rights: [AclRight::Execute].into_iter().collect(),
                effect: AclEffect::Allow,
                predicate: None,
            })
            .unwrap();
        let grants = GrantSet::new(vec![Grant {
            facet: Capability::Kv,
            mode: Mode::ReadWrite,
            scopes: vec![Scope::Prefix("allowed/".to_string())],
        }]);
        let mut state = StateAccess::new(&mut loom, context(ns, grants));
        state
            .kv_put("allowed", Value::Text("k".to_string()), b"ok".to_vec())
            .unwrap();
        assert!(matches!(
            state.kv_put("blocked", Value::Text("k".to_string()), b"no".to_vec()),
            Err(ExecError::Core(_))
        ));
    }

    #[test]
    fn manifest_grants_limit_files_before_core_write() {
        let (mut loom, ns) = state_loom(3);
        allow_all_exec(&mut loom, ns);
        let grants = GrantSet::new(vec![Grant {
            facet: Capability::Files,
            mode: Mode::Write,
            scopes: vec![Scope::Prefix("/allowed/".to_string())],
        }]);
        let mut state = StateAccess::new(&mut loom, context(ns, grants));
        assert!(matches!(
            state.file_write("/blocked/file.txt", b"no"),
            Err(ExecError::Denied(_))
        ));
        assert!(loom.read_file(ns, "/blocked/file.txt").is_err());
    }

    #[test]
    fn kv_delete_and_list_use_real_map() {
        let (mut loom, ns) = state_loom(4);
        allow_all_exec(&mut loom, ns);
        let grants = GrantSet::new(vec![Grant {
            facet: Capability::Kv,
            mode: Mode::ReadWrite,
            scopes: vec![Scope::All],
        }]);
        let key_a = Value::Text("a".to_string());
        let key_b = Value::Text("b".to_string());
        let mut state = StateAccess::new(&mut loom, context(ns, grants));
        state.kv_put("cache", key_a.clone(), b"a".to_vec()).unwrap();
        state.kv_put("cache", key_b.clone(), b"b".to_vec()).unwrap();
        assert!(state.kv_delete("cache", &key_a).unwrap());
        assert_eq!(
            state.kv_list("cache").unwrap(),
            vec![(key_b, b"b".to_vec())]
        );
    }

    #[test]
    fn kv_scan_returns_bounded_ordered_range() {
        let (mut loom, ns) = state_loom(9);
        allow_all_exec(&mut loom, ns);
        let grants = GrantSet::new(vec![Grant {
            facet: Capability::Kv,
            mode: Mode::ReadWrite,
            scopes: vec![Scope::All],
        }]);
        let mut state = StateAccess::new(&mut loom, context(ns, grants));
        // Typed integer keys inserted out of order: the scan must return them in Value order (1, 2, 10),
        // not lexical order, so `[Int(2), Int(11))` is exactly {2, 10}.
        state.kv_put("m", Value::Int(10), b"ten".to_vec()).unwrap();
        state.kv_put("m", Value::Int(2), b"two".to_vec()).unwrap();
        state.kv_put("m", Value::Int(1), b"one".to_vec()).unwrap();

        let ranged = state
            .kv_scan("m", &Value::Int(2), &Value::Int(11), 100)
            .unwrap();
        assert_eq!(
            ranged,
            vec![
                (Value::Int(2), b"two".to_vec()),
                (Value::Int(10), b"ten".to_vec()),
            ],
            "the half-open range excludes keys below lo and orders numerically"
        );

        // The limit caps the entry count while preserving order (the first key in range).
        let capped = state
            .kv_scan("m", &Value::Int(1), &Value::Int(100), 1)
            .unwrap();
        assert_eq!(capped, vec![(Value::Int(1), b"one".to_vec())]);
    }

    #[test]
    fn kv_scan_requires_the_kv_grant() {
        let (mut loom, ns) = state_loom(10);
        allow_all_exec(&mut loom, ns);
        let grants = GrantSet::new(vec![Grant {
            facet: Capability::Files,
            mode: Mode::ReadWrite,
            scopes: vec![Scope::All],
        }]);
        let mut state = StateAccess::new(&mut loom, context(ns, grants));
        assert!(matches!(
            state.kv_scan("m", &Value::Int(0), &Value::Int(9), 10),
            Err(ExecError::Denied(_))
        ));
    }

    #[cfg(feature = "derivations")]
    #[test]
    fn run_derivation_recomputes_over_live_kv_and_writes_back() {
        let (mut loom, ns) = state_loom(11);
        allow_all_exec(&mut loom, ns);
        let grants = GrantSet::new(vec![Grant {
            facet: Capability::Kv,
            mode: Mode::ReadWrite,
            scopes: vec![Scope::All],
        }]);
        let mut state = StateAccess::new(&mut loom, context(ns, grants));
        state
            .kv_put("m", Value::Text("item:1".to_string()), b"a".to_vec())
            .unwrap();
        state
            .kv_put("m", Value::Text("item:2".to_string()), b"b".to_vec())
            .unwrap();

        let derivation = crate::derivation::Derivation::CountUnderPrefix {
            source_prefix: "item:".to_string(),
            into_key: "derived:count".to_string(),
        };
        let (key, value) = state.run_derivation("m", &derivation).unwrap();
        assert_eq!(
            (key.as_str(), value.as_slice()),
            ("derived:count", b"2".as_slice())
        );
        // The derived entry is committed to the live collection.
        assert_eq!(
            state
                .kv_get("m", &Value::Text("derived:count".to_string()))
                .unwrap()
                .as_deref(),
            Some(b"2".as_slice())
        );
    }

    #[cfg(feature = "statecharts")]
    #[test]
    fn fire_machine_persists_state_and_action_through_state_access() {
        let (mut loom, ns) = state_loom(12);
        allow_all_exec(&mut loom, ns);
        let grants = GrantSet::new(vec![Grant {
            facet: Capability::Kv,
            mode: Mode::ReadWrite,
            scopes: vec![Scope::All],
        }]);
        let mut state = StateAccess::new(&mut loom, context(ns, grants));
        state
            .kv_put("m", Value::Text("reviewer".to_string()), b"alice".to_vec())
            .unwrap();

        let machine = crate::statechart::Machine {
            state_key: "doc:state".to_string(),
            initial: "draft".to_string(),
            transitions: vec![
                crate::statechart::Transition {
                    from: "draft".to_string(),
                    event: "submit".to_string(),
                    guard: None,
                    to: "review".to_string(),
                    action: None,
                },
                crate::statechart::Transition {
                    from: "review".to_string(),
                    event: "approve".to_string(),
                    guard: Some(r#"kv.reviewer == "alice""#.to_string()),
                    to: "published".to_string(),
                    action: Some(("published_by".to_string(), b"alice".to_vec())),
                },
            ],
        };
        let inputs = crate::guard::StateView::new();

        let s1 = state
            .fire_machine("m", &machine, "submit", &inputs, false)
            .unwrap();
        assert_eq!(s1.to, "review");
        assert_eq!(
            state
                .kv_get("m", &Value::Text("doc:state".to_string()))
                .unwrap()
                .as_deref(),
            Some(b"review".as_slice())
        );

        let s2 = state
            .fire_machine("m", &machine, "approve", &inputs, false)
            .unwrap();
        assert_eq!(s2.to, "published");
        assert_eq!(
            state
                .kv_get("m", &Value::Text("published_by".to_string()))
                .unwrap()
                .as_deref(),
            Some(b"alice".as_slice())
        );
    }

    #[cfg(feature = "workflows")]
    #[test]
    fn run_workflow_materializes_derived_views_through_state_access() {
        let (mut loom, ns) = state_loom(13);
        allow_all_exec(&mut loom, ns);
        let grants = GrantSet::new(vec![Grant {
            facet: Capability::Kv,
            mode: Mode::ReadWrite,
            scopes: vec![Scope::All],
        }]);
        let mut state = StateAccess::new(&mut loom, context(ns, grants));
        state
            .kv_put("m", Value::Text("item:1".to_string()), b"a".to_vec())
            .unwrap();
        state
            .kv_put("m", Value::Text("item:2".to_string()), b"b".to_vec())
            .unwrap();

        let mut engine = crate::workflow::TriggerEngine::new();
        engine.register(crate::workflow::Trigger::kv(
            crate::derivation::Derivation::CountUnderPrefix {
                source_prefix: "item:".to_string(),
                into_key: "derived:count".to_string(),
            },
        ));

        let reports = state.run_workflow("m", &mut engine, 16).unwrap();
        assert!(reports.iter().any(|r| r.fired));
        assert_eq!(
            state
                .kv_get("m", &Value::Text("derived:count".to_string()))
                .unwrap()
                .as_deref(),
            Some(b"2".as_slice())
        );
    }

    #[test]
    fn promoted_byte_facets_are_real_committed_state() {
        let (mut loom, ns) = state_loom(5);
        allow_all_exec(&mut loom, ns);
        let doc_value = br#"{"name":"Ada"}"#.to_vec();
        let digest;
        let commit = {
            let mut state = StateAccess::new(&mut loom, context(ns, all_facet_grants()));
            digest = state.cas_put(b"cas bytes").unwrap();
            assert_eq!(state.cas_get(&digest).unwrap(), Some(b"cas bytes".to_vec()));
            assert!(state.cas_has(&digest).unwrap());

            state.doc_put("people", "ada", doc_value.clone()).unwrap();
            assert_eq!(state.doc_get("people", "ada").unwrap(), Some(doc_value));
            assert_eq!(state.doc_list("people").unwrap().len(), 1);

            assert_eq!(state.queue_append("events", b"e1").unwrap(), 0);
            assert_eq!(state.queue_append("events", b"e2").unwrap(), 1);
            assert_eq!(state.queue_get("events", 1).unwrap(), Some(b"e2".to_vec()));
            assert_eq!(
                state.queue_range("events", 0, 2).unwrap(),
                vec![b"e1".to_vec(), b"e2".to_vec()]
            );
            assert_eq!(state.queue_len("events").unwrap(), 2);

            state.time_series_put("cpu", 100, b"low".to_vec()).unwrap();
            state.time_series_put("cpu", 200, b"high".to_vec()).unwrap();
            assert_eq!(
                state.time_series_get("cpu", 100).unwrap(),
                Some(b"low".to_vec())
            );
            assert_eq!(
                state.time_series_latest("cpu").unwrap(),
                Some((200, b"high".to_vec()))
            );
            assert_eq!(state.time_series_range("cpu", 0, 150).unwrap().len(), 1);

            assert_eq!(state.ledger_append("audit", b"a1".to_vec()).unwrap(), 0);
            assert_eq!(state.ledger_append("audit", b"a2".to_vec()).unwrap(), 1);
            assert_eq!(state.ledger_len("audit").unwrap(), 2);
            assert_eq!(state.ledger_get("audit", 0).unwrap(), Some(b"a1".to_vec()));
            assert!(state.ledger_head("audit").unwrap().is_some());
            state.ledger_verify("audit").unwrap();

            state.commit("program", "promoted facets", 2).unwrap()
        };

        assert_eq!(
            loom.read_file(ns, "/no-such-file").unwrap_err().code,
            loom_core::Code::NotFound
        );
        loom.checkout_commit(ns, commit).unwrap();
        let mut state = StateAccess::new(&mut loom, context(ns, all_facet_grants()));
        assert_eq!(state.cas_list().unwrap(), vec![digest]);
        assert_eq!(
            state.doc_get("people", "ada").unwrap(),
            Some(br#"{"name":"Ada"}"#.to_vec())
        );
        assert_eq!(state.queue_len("events").unwrap(), 2);
        assert_eq!(state.time_series_latest("cpu").unwrap().unwrap().0, 200);
        assert_eq!(state.ledger_len("audit").unwrap(), 2);
    }

    #[test]
    fn promoted_structured_facets_are_real_state() {
        let (mut loom, ns) = state_loom(6);
        allow_all_exec(&mut loom, ns);
        let mut state = StateAccess::new(&mut loom, context(ns, all_facet_grants()));

        state
            .graph_upsert_node(
                "deps",
                "root",
                BTreeMap::from([("kind".into(), loom_core::GraphValue::Text("pkg".into()))]),
            )
            .unwrap();
        state
            .graph_upsert_node("deps", "leaf", BTreeMap::new())
            .unwrap();
        state
            .graph_upsert_edge("deps", "e1", "root", "leaf", "uses", BTreeMap::new())
            .unwrap();
        assert_eq!(
            state.graph_get_node("deps", "root").unwrap().unwrap()["kind"],
            loom_core::GraphValue::Text("pkg".into())
        );
        assert_eq!(
            state.graph_neighbors("deps", "root").unwrap(),
            vec!["leaf".to_string()]
        );
        assert!(state.graph_get_edge("deps", "e1").unwrap().is_some());
        assert_eq!(
            state
                .graph_shortest_path("deps", "root", "leaf", Some("uses"))
                .unwrap(),
            Some(vec!["root".to_string(), "leaf".to_string()])
        );

        state
            .columnar_create(
                "metrics",
                vec![
                    ("id".to_string(), loom_core::ColumnType::Int),
                    ("name".to_string(), loom_core::ColumnType::Text),
                ],
                0,
            )
            .unwrap();
        state
            .columnar_append(
                "metrics",
                vec![Value::Int(1), Value::Text("latency".to_string())],
            )
            .unwrap();
        assert_eq!(state.columnar_rows("metrics").unwrap(), 1);
        assert_eq!(
            state.columnar_scan("metrics").unwrap(),
            vec![vec![Value::Int(1), Value::Text("latency".to_string())]]
        );

        state
            .search_create(
                "docs",
                BTreeMap::from([("body".to_string(), loom_core::FieldMapping::text())]),
            )
            .unwrap();
        state
            .search_index(
                "docs",
                b"doc-1".to_vec(),
                BTreeMap::from([(
                    "body".to_string(),
                    loom_core::FieldValue::Text("hello world".to_string()),
                )]),
            )
            .unwrap();
        assert_eq!(
            state.search_ids("docs", None).unwrap(),
            vec![b"doc-1".to_vec()]
        );
        assert!(state.search_get("docs", b"doc-1").unwrap().is_some());

        state.vector_create("emb", 2, Metric::Cosine).unwrap();
        state
            .vector_upsert("emb", "a", vec![1.0, 0.0], BTreeMap::new())
            .unwrap();
        assert!(state.vector_get("emb", "a").unwrap().is_some());
        assert_eq!(
            state.vector_ids("emb", None).unwrap(),
            vec!["a".to_string()]
        );
        assert_eq!(
            state
                .vector_search("emb", &[1.0, 0.0], 1, &MetaFilter::All)
                .unwrap()[0]
                .id,
            "a"
        );
    }

    #[test]
    fn pim_state_access_is_domain_shaped_committed_state() {
        let (mut loom, ns) = state_loom(16);
        allow_all_exec(&mut loom, ns);
        let commit = {
            let mut state = StateAccess::new(&mut loom, context(ns, all_facet_grants()));
            state
                .calendar_create_collection(
                    "alice",
                    "work",
                    &CollectionMeta {
                        display_name: "Work".to_string(),
                        component_set: vec![Component::Event],
                    },
                )
                .unwrap();
            state
                .calendar_put_entry(
                    "alice",
                    "work",
                    &CalendarEntry::event("u1", "Standup", "20240101T090000"),
                )
                .unwrap();
            assert_eq!(
                state
                    .calendar_get_entry("alice", "work", "u1")
                    .unwrap()
                    .unwrap()
                    .summary,
                "Standup"
            );
            assert_eq!(
                state
                    .calendar_search("alice", "work", Some(Component::Event), Some("stand"))
                    .unwrap()
                    .len(),
                1
            );

            state
                .contacts_create_book(
                    "alice",
                    "people",
                    &BookMeta {
                        display_name: "People".to_string(),
                    },
                )
                .unwrap();
            state
                .contacts_put_entry("alice", "people", &ContactEntry::new("c1", "Ada Lovelace"))
                .unwrap();
            assert_eq!(
                state
                    .contacts_get_entry("alice", "people", "c1")
                    .unwrap()
                    .unwrap()
                    .full_name,
                "Ada Lovelace"
            );
            assert_eq!(
                state
                    .contacts_search("alice", "people", "lovelace")
                    .unwrap()
                    .len(),
                1
            );

            let raw = b"From: bob@example.test\r\nTo: alice@example.test\r\nSubject: Hello\r\nMessage-ID: <m1@example.test>\r\n\r\nbody";
            state
                .mail_create_mailbox(
                    "alice",
                    "inbox",
                    &MailboxMeta {
                        display_name: "Inbox".to_string(),
                    },
                )
                .unwrap();
            state
                .mail_ingest_message("alice", "inbox", "m1", raw)
                .unwrap();
            assert_eq!(
                state
                    .mail_get_message("alice", "inbox", "m1")
                    .unwrap()
                    .unwrap()
                    .subject,
                "Hello"
            );
            state
                .mail_set_flags(
                    "alice",
                    "inbox",
                    "m1",
                    &["\\Seen".to_string(), "Important".to_string()],
                )
                .unwrap();
            assert_eq!(
                state.mail_get_flags("alice", "inbox", "m1").unwrap(),
                vec!["Important".to_string(), "\\Seen".to_string()]
            );
            assert_eq!(
                state
                    .mail_to_eml("alice", "inbox", "m1")
                    .unwrap()
                    .as_deref(),
                Some(raw.as_slice())
            );
            state.commit("program", "pim state access", 3).unwrap()
        };

        loom.checkout_commit(ns, commit).unwrap();
        let mut state = StateAccess::new(&mut loom, context(ns, all_facet_grants()));
        assert_eq!(
            state.calendar_list_collections("alice").unwrap(),
            vec!["work".to_string()]
        );
        assert_eq!(
            state.calendar_list_entries("alice", "work").unwrap().len(),
            1
        );
        assert_eq!(
            state.contacts_list_books("alice").unwrap(),
            vec!["people".to_string()]
        );
        assert_eq!(
            state
                .contacts_list_entries("alice", "people")
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            state.mail_list_mailboxes("alice").unwrap(),
            vec!["inbox".to_string()]
        );
        assert_eq!(state.mail_list_messages("alice", "inbox").unwrap().len(), 1);
        assert_eq!(
            state.mail_search("alice", "inbox", "hello").unwrap().len(),
            1
        );
    }

    #[test]
    fn pim_state_access_scopes_prevent_cross_collection_mutation() {
        let (mut loom, ns) = state_loom(17);
        allow_all_exec(&mut loom, ns);
        let grants = GrantSet::new(vec![Grant {
            facet: Capability::Calendar,
            mode: Mode::ReadWrite,
            scopes: vec![Scope::Prefix("alice/work".to_string())],
        }]);
        let mut state = StateAccess::new(&mut loom, context(ns, grants));
        state
            .calendar_create_collection(
                "alice",
                "work",
                &CollectionMeta {
                    display_name: "Work".to_string(),
                    component_set: vec![Component::Event],
                },
            )
            .unwrap();
        assert!(matches!(
            state.calendar_create_collection(
                "alice",
                "private",
                &CollectionMeta {
                    display_name: "Private".to_string(),
                    component_set: vec![Component::Event],
                },
            ),
            Err(ExecError::Denied(_))
        ));
        assert!(matches!(
            state.calendar_create_collection(
                "bob",
                "work",
                &CollectionMeta {
                    display_name: "Bob Work".to_string(),
                    component_set: vec![Component::Event],
                },
            ),
            Err(ExecError::Denied(_))
        ));
    }

    #[test]
    fn pim_state_access_denies_modes_facets_and_missing_collections() {
        let (mut loom, ns) = state_loom(18);
        allow_all_exec(&mut loom, ns);
        let grants = GrantSet::new(vec![
            Grant {
                facet: Capability::Calendar,
                mode: Mode::Read,
                scopes: vec![Scope::All],
            },
            Grant {
                facet: Capability::Contacts,
                mode: Mode::ReadWrite,
                scopes: vec![Scope::Prefix("alice/people".to_string())],
            },
            Grant {
                facet: Capability::Mail,
                mode: Mode::ReadWrite,
                scopes: vec![Scope::Prefix("alice/inbox".to_string())],
            },
        ]);
        let mut state = StateAccess::new(&mut loom, context(ns, grants));
        assert!(
            state
                .calendar_get_collection("alice", "work")
                .unwrap()
                .is_none()
        );
        let denied = state
            .calendar_create_collection(
                "alice",
                "work",
                &CollectionMeta {
                    display_name: "Work".to_string(),
                    component_set: vec![Component::Event],
                },
            )
            .unwrap_err();
        assert_eq!(denied.code(), loom_core::Code::PermissionDenied);
        state
            .contacts_create_book(
                "alice",
                "people",
                &BookMeta {
                    display_name: "People".to_string(),
                },
            )
            .unwrap();
        assert_eq!(
            state
                .contacts_create_book(
                    "alice",
                    "private",
                    &BookMeta {
                        display_name: "Private".to_string(),
                    },
                )
                .unwrap_err()
                .code(),
            loom_core::Code::PermissionDenied
        );
        assert_eq!(
            state
                .contacts_create_book(
                    "bob",
                    "people",
                    &BookMeta {
                        display_name: "Bob".to_string(),
                    },
                )
                .unwrap_err()
                .code(),
            loom_core::Code::PermissionDenied
        );
        state
            .mail_create_mailbox(
                "alice",
                "inbox",
                &MailboxMeta {
                    display_name: "Inbox".to_string(),
                },
            )
            .unwrap();
        assert_eq!(
            state
                .mail_create_mailbox(
                    "alice",
                    "archive",
                    &MailboxMeta {
                        display_name: "Archive".to_string(),
                    },
                )
                .unwrap_err()
                .code(),
            loom_core::Code::PermissionDenied
        );
        assert_eq!(
            state
                .mail_create_mailbox(
                    "bob",
                    "inbox",
                    &MailboxMeta {
                        display_name: "Bob Inbox".to_string(),
                    },
                )
                .unwrap_err()
                .code(),
            loom_core::Code::PermissionDenied
        );

        let (mut loom, ns) = state_loom(19);
        allow_all_exec(&mut loom, ns);
        let mut state = StateAccess::new(&mut loom, context(ns, all_facet_grants()));
        assert_eq!(
            state
                .calendar_put_entry(
                    "alice",
                    "missing",
                    &CalendarEntry::event("u1", "Standup", "20240101T090000"),
                )
                .unwrap_err()
                .code(),
            loom_core::Code::NotFound
        );
        assert_eq!(
            state
                .contacts_put_entry("alice", "missing", &ContactEntry::new("c1", "Ada"))
                .unwrap_err()
                .code(),
            loom_core::Code::NotFound
        );
        assert_eq!(
            state
                .mail_ingest_message("alice", "missing", "m1", b"Subject: Hi\r\n\r\nbody")
                .unwrap_err()
                .code(),
            loom_core::Code::NotFound
        );
    }

    #[test]
    fn dataframe_state_access_checks_sources_and_materialization_targets() {
        let (mut loom, ns) = state_loom(14);
        allow_all_exec(&mut loom, ns);
        let mut state = StateAccess::new(&mut loom, context(ns, all_facet_grants()));
        state
            .file_write(
                "inputs/events.csv",
                b"id,kind\n1,purchase\n2,view\n3,purchase\n",
            )
            .unwrap();

        let plan = DataframePlan::new(vec![DataframeSourceBinding::new(
            "events",
            DataframeSourceKind::Files,
            "inputs/events.csv",
            DataframeInputFormat::Csv,
        )])
        .unwrap()
        .with_operations(vec![
            DataframeOperation::Scan {
                source: "events".to_string(),
            },
            DataframeOperation::Filter {
                expression: "kind == \"purchase\"".to_string(),
            },
        ])
        .unwrap()
        .with_materialization(DataframeMaterialization::new(
            DataframeMaterializationTarget::Columnar,
            Some("analytics/events".to_string()),
            DataframeInputFormat::Parquet,
        ))
        .unwrap();

        state.dataframe_create("etl/events", &plan).unwrap();
        assert_eq!(state.dataframe_get_plan("etl/events").unwrap(), plan);
        assert_eq!(
            state
                .dataframe_preview("etl/events", 1)
                .unwrap()
                .row_count(),
            1
        );
        assert_eq!(
            state.dataframe_collect("etl/events").unwrap().row_count(),
            2
        );
        assert!(
            !state
                .dataframe_plan_digest("etl/events")
                .unwrap()
                .to_hex()
                .is_empty()
        );
        assert_eq!(
            state.dataframe_source_digests("etl/events").unwrap(),
            vec![]
        );

        state.dataframe_materialize("etl/events").unwrap();
        assert_eq!(state.columnar_rows("analytics/events").unwrap(), 2);
    }

    #[test]
    fn dataframe_state_access_does_not_bypass_source_grants() {
        let (mut loom, ns) = state_loom(15);
        allow_all_exec(&mut loom, ns);
        loom.create_directory(ns, "inputs", true).unwrap();
        loom.write_file(ns, "inputs/events.csv", b"id\n1\n", 0o100644)
            .unwrap();
        let plan = DataframePlan::new(vec![DataframeSourceBinding::new(
            "events",
            DataframeSourceKind::Files,
            "inputs/events.csv",
            DataframeInputFormat::Csv,
        )])
        .unwrap()
        .with_operations(vec![DataframeOperation::Scan {
            source: "events".to_string(),
        }])
        .unwrap();
        dataframe_create(&mut loom, ns, "etl/events", &plan).unwrap();

        let grants = GrantSet::new(vec![Grant {
            facet: Capability::Dataframe,
            mode: Mode::Read,
            scopes: vec![Scope::All],
        }]);
        let mut state = StateAccess::new(&mut loom, context(ns, grants));
        assert!(matches!(
            state.dataframe_collect("etl/events"),
            Err(ExecError::Denied(_))
        ));
    }

    #[cfg(feature = "sql-state-access")]
    #[test]
    fn sql_state_access_exec_persists_and_query_is_read_only() {
        let (mut loom, ns) = state_loom(17);
        loom.registry_mut().add_facet(ns, FacetKind::Sql).unwrap();
        allow_all_exec(&mut loom, ns);
        let grants = GrantSet::new(vec![Grant {
            facet: Capability::Sql,
            mode: Mode::ReadWrite,
            scopes: vec![Scope::Prefix("app/".to_string())],
        }]);
        let mut state = StateAccess::new(&mut loom, context(ns, grants));
        state
            .sql_exec_cbor(
                "app",
                "CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT); INSERT INTO t VALUES (1, 'a')",
            )
            .unwrap();
        let result = state
            .sql_query_cbor("app", "SELECT id, v FROM t ORDER BY id")
            .unwrap();
        let json = loom_result::result_to_json(&result).unwrap();
        assert!(json.contains("\"id\""));
        assert!(json.contains("\"v\""));
        assert!(json.contains("\"a\""));
        let err = state
            .sql_query_cbor("app", "INSERT INTO t VALUES (2, 'b')")
            .unwrap_err();
        assert_eq!(err.code(), loom_core::Code::PermissionDenied);
        assert!(
            state.sql_query_cbor("other", "SELECT id FROM t").is_err(),
            "db scope is enforced by the SQL capability target"
        );
    }

    #[cfg(feature = "sql-state-access")]
    #[test]
    fn sql_state_access_respects_manifest_mode_grants() {
        let (mut loom, ns) = state_loom(18);
        loom.registry_mut().add_facet(ns, FacetKind::Sql).unwrap();
        allow_all_exec(&mut loom, ns);
        let read_only = GrantSet::new(vec![Grant {
            facet: Capability::Sql,
            mode: Mode::Read,
            scopes: vec![Scope::All],
        }]);
        let mut state = StateAccess::new(&mut loom, context(ns, read_only));
        assert!(matches!(
            state.sql_exec_cbor("app", "CREATE TABLE t (id INTEGER PRIMARY KEY)"),
            Err(ExecError::Denied(_))
        ));

        let write_only = GrantSet::new(vec![Grant {
            facet: Capability::Sql,
            mode: Mode::Write,
            scopes: vec![Scope::All],
        }]);
        let mut state = StateAccess::new(&mut loom, context(ns, write_only));
        assert!(matches!(
            state.sql_query_cbor("app", "SELECT 1"),
            Err(ExecError::Denied(_))
        ));
    }

    #[test]
    fn promoted_facet_manifest_denial_prevents_core_mutation() {
        let (mut loom, ns) = state_loom(7);
        allow_all_exec(&mut loom, ns);
        let grants = GrantSet::new(vec![Grant {
            facet: Capability::Document,
            mode: Mode::Write,
            scopes: vec![Scope::Prefix("allowed/".to_string())],
        }]);
        let mut state = StateAccess::new(&mut loom, context(ns, grants));
        assert!(matches!(
            state.doc_put("blocked", "a", b"no".to_vec()),
            Err(ExecError::Denied(_))
        ));

        let grants = GrantSet::new(vec![Grant {
            facet: Capability::Document,
            mode: Mode::Read,
            scopes: vec![Scope::All],
        }]);
        let mut state = StateAccess::new(&mut loom, context(ns, grants));
        assert_eq!(state.doc_get("blocked", "a").unwrap(), None);
    }
}
