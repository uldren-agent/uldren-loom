use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU32, Ordering};

use loom_coordination::CoordinationScope;
use loom_core::error::{Code, LoomError, Result};
use loom_core::fs::FileKind;
use loom_core::workspace::facet_path;
use loom_core::{
    AclRight, CmpOp, ColumnType, ColumnarAggregate, ColumnarInspect, Digest, FacetKind, Hit, Loom,
    Mapping, Metric, QueryRequest, QueryResponse, StructuredPoint, TimeSeriesAggregation,
    TimeSeriesPolicy, TimeSeriesRollup, TimeSeriesValue, Value, WorkspaceId, WsSelector, dataframe,
    document, graph, kv, ledger, log, search, timeseries, vector,
};
use loom_store::FileStore;
use sha2::{Digest as ShaDigest, Sha256};

use crate::{HostedAuth, HostedKernel, HostedOutcome, hosted_outcome};

pub struct HostedDataAdapter<'a> {
    kernel: &'a HostedKernel,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KvEntry {
    pub key_cbor: Vec<u8>,
    pub value: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostedDocumentText {
    pub text: String,
    pub digest: String,
    pub entity_tag: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostedDocumentBinary {
    pub bytes: Vec<u8>,
    pub digest: String,
    pub entity_tag: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostedDocumentPutResult {
    pub digest: String,
    pub entity_tag: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostedEtcdKv {
    pub key: Vec<u8>,
    pub value: Vec<u8>,
    pub create_revision: i64,
    pub mod_revision: i64,
    pub version: i64,
    pub lease: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostedEtcdRangeResult {
    pub revision: i64,
    pub kvs: Vec<HostedEtcdKv>,
    pub count: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostedEtcdCompactResult {
    pub revision: i64,
    pub compacted_revision: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostedEtcdEventKind {
    Put,
    Delete,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostedEtcdWatchEvent {
    pub revision: i64,
    pub kind: HostedEtcdEventKind,
    pub kv: HostedEtcdKv,
    pub prev_kv: Option<HostedEtcdKv>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostedEtcdWatchResult {
    pub revision: i64,
    pub compacted_revision: i64,
    pub events: Vec<HostedEtcdWatchEvent>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostedEtcdPutResult {
    pub revision: i64,
    pub prev_kv: Option<HostedEtcdKv>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostedEtcdDeleteRangeResult {
    pub revision: i64,
    pub deleted: i64,
    pub prev_kvs: Vec<HostedEtcdKv>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostedEtcdLeaseGrantResult {
    pub id: i64,
    pub ttl: i64,
    pub revision: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostedEtcdLeaseKeepAliveResult {
    pub id: i64,
    pub ttl: i64,
    pub revision: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostedEtcdCompareTarget {
    Value(Vec<u8>),
    Version(i64),
    CreateRevision(i64),
    ModRevision(i64),
    Lease(i64),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostedEtcdCompareResult {
    Equal,
    Greater,
    Less,
    NotEqual,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostedEtcdCompare {
    pub key: Vec<u8>,
    pub result: HostedEtcdCompareResult,
    pub target: HostedEtcdCompareTarget,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostedEtcdRequestOp {
    Range {
        key: Vec<u8>,
        range_end: Vec<u8>,
        limit: i64,
        revision: i64,
    },
    Put {
        key: Vec<u8>,
        value: Vec<u8>,
        lease: i64,
        prev_kv: bool,
    },
    DeleteRange {
        key: Vec<u8>,
        range_end: Vec<u8>,
        prev_kv: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostedEtcdResponseOp {
    Range(HostedEtcdRangeResult),
    Put(HostedEtcdPutResult),
    DeleteRange(HostedEtcdDeleteRangeResult),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostedEtcdTxnResult {
    pub revision: i64,
    pub succeeded: bool,
    pub responses: Vec<HostedEtcdResponseOp>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentEntry {
    pub id: String,
    pub document: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostedDocumentIndex {
    pub index_id: String,
    pub name: String,
    pub path: String,
    pub extractor: String,
    pub key_codec: String,
    pub comparator: String,
    pub uniqueness: String,
    pub unique: bool,
    pub failure_policy: String,
    pub declaration_version: u64,
    pub analyzer_profile: Option<String>,
    pub projection: Option<String>,
    pub partial_filter: Option<String>,
    pub metadata: serde_json::Value,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueueEntry {
    pub seq: usize,
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostedKafkaTopicMetadata {
    pub topic: String,
    pub topic_id: [u8; 16],
    pub metadata_version: u64,
    pub created_at_ms: u64,
    pub partitions: Vec<HostedKafkaPartitionMetadata>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostedKafkaPartitionMetadata {
    pub partition: i32,
    pub next_offset: u64,
    pub leader_id: i32,
    pub leader_epoch: u64,
    pub high_watermark: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostedKafkaProduceResult {
    pub base_offset: u64,
    pub high_watermark: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostedKafkaProducerAppend {
    pub producer_id: i64,
    pub producer_epoch: i16,
    pub first_sequence: i32,
    pub record_count: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostedKafkaFetchResult {
    pub high_watermark: u64,
    pub last_stable_offset: u64,
    pub aborted_transactions: Vec<HostedKafkaAbortedTransaction>,
    pub records: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostedKafkaProducerState {
    pub producer_id: i64,
    pub producer_epoch: i16,
    pub transactional_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostedKafkaTransactionTopic {
    pub topic: String,
    pub partitions: Vec<i32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostedKafkaPendingOffsetCommit {
    pub group_id: String,
    pub topic: String,
    pub partition: i32,
    pub offset: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostedKafkaAbortedTransaction {
    pub producer_id: i64,
    pub first_offset: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct HostedKafkaPendingProducedRange {
    topic: String,
    partition: i32,
    first_offset: u64,
    record_count: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum HostedKafkaTransactionStatus {
    Active,
    Committed,
    Aborted,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct HostedKafkaTransactionState {
    transactional_id: String,
    producer_id: i64,
    producer_epoch: i16,
    transaction_epoch: u64,
    status: HostedKafkaTransactionStatus,
    topics: Vec<HostedKafkaTransactionTopic>,
    groups: Vec<String>,
    pending_offset_commits: Vec<HostedKafkaPendingOffsetCommit>,
    pending_produced_ranges: Vec<HostedKafkaPendingProducedRange>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct HostedKafkaProducerSequenceState {
    producer_id: i64,
    producer_epoch: i16,
    next_sequence: i32,
    last_base_sequence: i32,
    last_record_count: u32,
    last_base_offset: u64,
    last_high_watermark: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimeSeriesPoint {
    pub timestamp: i64,
    pub value: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StructuredTimeSeriesPoint {
    pub measurement: String,
    pub tags: BTreeMap<String, String>,
    pub timestamp_ns: i64,
    pub fields: BTreeMap<String, TimeSeriesValue>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HostedVectorEntry {
    pub vector: Vec<f32>,
    pub metadata: std::collections::BTreeMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HostedVectorInfo {
    pub dim: usize,
    pub metric: Metric,
    pub count: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HostedPineconeWorkspaceStats {
    pub workspace: String,
    pub dim: usize,
    pub metric: Metric,
    pub count: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HostedSearchQueryDocuments {
    pub result: QueryResponse,
    pub documents: Vec<(Vec<u8>, search::Document)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostedSearchAliasAction {
    Add {
        index: String,
        alias: String,
        is_write_index: bool,
    },
    Remove {
        alias: String,
    },
}

impl HostedKernel {
    pub fn data(&self) -> HostedDataAdapter<'_> {
        HostedDataAdapter { kernel: self }
    }
}

impl HostedDataAdapter<'_> {
    pub fn columnar_create(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        dataset: &str,
        columns: Vec<(String, ColumnType)>,
        target_segment_rows: usize,
    ) -> HostedOutcome<()> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = ensure_facet_ns(loom, FacetKind::Columnar, workspace)?;
            loom_core::columnar_create(loom, ns, dataset, columns, target_segment_rows)
        }))
    }

    pub fn columnar_append(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        dataset: &str,
        row: Vec<Value>,
    ) -> HostedOutcome<()> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = ensure_facet_ns(loom, FacetKind::Columnar, workspace)?;
            loom_core::columnar_append(loom, ns, dataset, row)
        }))
    }

    pub fn columnar_scan(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        dataset: &str,
    ) -> HostedOutcome<Vec<Vec<Value>>> {
        self.read_facet(auth, FacetKind::Columnar, workspace, |loom, ns| {
            loom_core::columnar_scan(loom, ns, dataset)
        })
    }

    pub fn columnar_columns(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        dataset: &str,
    ) -> HostedOutcome<Vec<(String, ColumnType)>> {
        self.read_facet(auth, FacetKind::Columnar, workspace, |loom, ns| {
            loom_core::columnar_columns(loom, ns, dataset)
        })
    }

    pub fn columnar_rows(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        dataset: &str,
    ) -> HostedOutcome<usize> {
        self.read_facet(auth, FacetKind::Columnar, workspace, |loom, ns| {
            loom_core::columnar_rows(loom, ns, dataset)
        })
    }

    pub fn columnar_compact(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        dataset: &str,
    ) -> HostedOutcome<()> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = ensure_facet_ns(loom, FacetKind::Columnar, workspace)?;
            loom_core::columnar_compact(loom, ns, dataset)
        }))
    }

    pub fn columnar_inspect(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        dataset: &str,
    ) -> HostedOutcome<ColumnarInspect> {
        self.read_facet(auth, FacetKind::Columnar, workspace, |loom, ns| {
            loom_core::columnar_inspect(loom, ns, dataset)
        })
    }

    pub fn columnar_source_digest(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        dataset: &str,
    ) -> HostedOutcome<Digest> {
        self.read_facet(auth, FacetKind::Columnar, workspace, |loom, ns| {
            loom_core::columnar_source_digest(loom, ns, dataset)
        })
    }

    pub fn columnar_select(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        dataset: &str,
        columns: &[&str],
        filter: Option<(String, CmpOp, Value)>,
    ) -> HostedOutcome<Vec<Vec<Value>>> {
        self.read_facet(auth, FacetKind::Columnar, workspace, |loom, ns| {
            let filter = filter
                .as_ref()
                .map(|(field, op, value)| (field.as_str(), *op, value));
            loom_core::columnar_select(loom, ns, dataset, columns, filter)
        })
    }

    pub fn columnar_aggregate(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        dataset: &str,
        aggregates: &[ColumnarAggregate],
        filter: Option<(String, CmpOp, Value)>,
    ) -> HostedOutcome<Vec<Value>> {
        self.read_facet(auth, FacetKind::Columnar, workspace, |loom, ns| {
            let filter = filter
                .as_ref()
                .map(|(field, op, value)| (field.as_str(), *op, value));
            loom_core::columnar_aggregate(loom, ns, dataset, aggregates, filter)
        })
    }

    #[cfg(feature = "columnar-arrow")]
    pub fn columnar_export_arrow_ipc(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        dataset: &str,
    ) -> HostedOutcome<Vec<u8>> {
        self.read_facet(auth, FacetKind::Columnar, workspace, |loom, ns| {
            let set = loom_core::get_columnar(loom, ns, dataset)?;
            loom_core::columnar_to_arrow_ipc(&set)
        })
    }

    #[cfg(not(feature = "columnar-arrow"))]
    pub fn columnar_export_arrow_ipc(
        &self,
        _auth: &HostedAuth,
        _workspace: &str,
        _dataset: &str,
    ) -> HostedOutcome<Vec<u8>> {
        Err(crate::HostedError::from_error(
            loom_core::LoomError::unsupported(
                "hosted Arrow IPC transfer requires the columnar-arrow feature",
            ),
        ))
    }

    #[cfg(feature = "columnar-arrow")]
    pub fn columnar_import_arrow_ipc(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        dataset: &str,
        bytes: &[u8],
        target_segment_rows: usize,
    ) -> HostedOutcome<()> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = ensure_facet_ns(loom, FacetKind::Columnar, workspace)?;
            let set = loom_core::columnar_from_arrow_ipc(bytes, target_segment_rows)?;
            loom_core::put_columnar(loom, ns, dataset, &set)
        }))
    }

    #[cfg(not(feature = "columnar-arrow"))]
    pub fn columnar_import_arrow_ipc(
        &self,
        _auth: &HostedAuth,
        _workspace: &str,
        _dataset: &str,
        _bytes: &[u8],
        _target_segment_rows: usize,
    ) -> HostedOutcome<()> {
        Err(crate::HostedError::from_error(
            loom_core::LoomError::unsupported(
                "hosted Arrow IPC transfer requires the columnar-arrow feature",
            ),
        ))
    }

    #[cfg(feature = "columnar-arrow")]
    pub fn columnar_export_parquet(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        dataset: &str,
    ) -> HostedOutcome<Vec<u8>> {
        self.read_facet(auth, FacetKind::Columnar, workspace, |loom, ns| {
            let set = loom_core::get_columnar(loom, ns, dataset)?;
            loom_core::columnar_to_parquet(&set)
        })
    }

    #[cfg(not(feature = "columnar-arrow"))]
    pub fn columnar_export_parquet(
        &self,
        _auth: &HostedAuth,
        _workspace: &str,
        _dataset: &str,
    ) -> HostedOutcome<Vec<u8>> {
        Err(crate::HostedError::from_error(
            loom_core::LoomError::unsupported(
                "hosted Parquet transfer requires the columnar-arrow feature",
            ),
        ))
    }

    #[cfg(feature = "columnar-arrow")]
    pub fn columnar_import_parquet(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        dataset: &str,
        bytes: &[u8],
        target_segment_rows: usize,
    ) -> HostedOutcome<()> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = ensure_facet_ns(loom, FacetKind::Columnar, workspace)?;
            let set = loom_core::columnar_from_parquet(bytes, target_segment_rows)?;
            loom_core::put_columnar(loom, ns, dataset, &set)
        }))
    }

    #[cfg(not(feature = "columnar-arrow"))]
    pub fn columnar_import_parquet(
        &self,
        _auth: &HostedAuth,
        _workspace: &str,
        _dataset: &str,
        _bytes: &[u8],
        _target_segment_rows: usize,
    ) -> HostedOutcome<()> {
        Err(crate::HostedError::from_error(
            loom_core::LoomError::unsupported(
                "hosted Parquet transfer requires the columnar-arrow feature",
            ),
        ))
    }

    pub fn kv_put(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
        key_cbor: &[u8],
        value: Vec<u8>,
    ) -> HostedOutcome<()> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = ensure_facet_ns(loom, FacetKind::Kv, workspace)?;
            let key = kv::key_from_cbor(key_cbor)?;
            loom.kv_put_configured(ns, collection, key, value, None, 0)
        }))
    }

    pub fn kv_get(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
        key_cbor: &[u8],
    ) -> HostedOutcome<Option<Vec<u8>>> {
        hosted_outcome(self.kernel.read_mut(auth, |loom| {
            let ns = resolve_facet_ns(loom, FacetKind::Kv, workspace)?;
            let key = kv::key_from_cbor(key_cbor)?;
            loom.kv_get_configured(ns, collection, &key, 0)
        }))
    }

    pub fn kv_delete(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
        key_cbor: &[u8],
    ) -> HostedOutcome<bool> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = resolve_facet_ns(loom, FacetKind::Kv, workspace)?;
            let key = kv::key_from_cbor(key_cbor)?;
            loom.kv_delete_configured(ns, collection, &key)
        }))
    }

    pub fn kv_list(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
    ) -> HostedOutcome<Vec<KvEntry>> {
        hosted_outcome(self.kernel.read_mut(auth, |loom| {
            let ns = resolve_facet_ns(loom, FacetKind::Kv, workspace)?;
            let map = loom.kv_list_configured(ns, collection, 0)?;
            Ok(kv_entries(map.iter()))
        }))
    }

    pub fn kv_range(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
        lo_cbor: &[u8],
        hi_cbor: &[u8],
    ) -> HostedOutcome<Vec<KvEntry>> {
        hosted_outcome(self.kernel.read_mut(auth, |loom| {
            let ns = resolve_facet_ns(loom, FacetKind::Kv, workspace)?;
            let lo = kv::key_from_cbor(lo_cbor)?;
            let hi = kv::key_from_cbor(hi_cbor)?;
            let map = loom.kv_range_configured(ns, collection, &lo, &hi, 0)?;
            Ok(kv_entries(map.iter()))
        }))
    }

    pub fn etcd_range(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
        key: Vec<u8>,
        range_end: Vec<u8>,
        limit: i64,
        revision: i64,
    ) -> HostedOutcome<HostedEtcdRangeResult> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = resolve_facet_ns(loom, FacetKind::Kv, workspace)?;
            let state = EtcdState::load(loom, ns, collection)?;
            let result = state.range(key, range_end, limit, revision)?;
            state.save(loom, ns, collection)?;
            Ok(result)
        }))
    }

    pub fn etcd_compact(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
        revision: i64,
        physical: bool,
    ) -> HostedOutcome<HostedEtcdCompactResult> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = resolve_facet_ns(loom, FacetKind::Kv, workspace)?;
            let mut state = EtcdState::load(loom, ns, collection)?;
            let result = state.compact(revision, physical)?;
            state.save(loom, ns, collection)?;
            Ok(result)
        }))
    }

    pub fn etcd_watch_events(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
        key: Vec<u8>,
        range_end: Vec<u8>,
        start_revision: i64,
    ) -> HostedOutcome<HostedEtcdWatchResult> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = resolve_facet_ns(loom, FacetKind::Kv, workspace)?;
            let state = EtcdState::load(loom, ns, collection)?;
            let result = state.watch_events(key, range_end, start_revision)?;
            state.save(loom, ns, collection)?;
            Ok(result)
        }))
    }

    pub fn etcd_put(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
        key: Vec<u8>,
        value: Vec<u8>,
        lease: i64,
        prev_kv: bool,
    ) -> HostedOutcome<HostedEtcdPutResult> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = ensure_facet_ns(loom, FacetKind::Kv, workspace)?;
            let mut state = EtcdState::load(loom, ns, collection)?;
            let result = state.put(key, value, lease, prev_kv)?;
            state.save(loom, ns, collection)?;
            Ok(result)
        }))
    }

    pub fn etcd_delete_range(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
        key: Vec<u8>,
        range_end: Vec<u8>,
        prev_kv: bool,
    ) -> HostedOutcome<HostedEtcdDeleteRangeResult> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = resolve_facet_ns(loom, FacetKind::Kv, workspace)?;
            let mut state = EtcdState::load(loom, ns, collection)?;
            let result = state.delete_range(key, range_end, prev_kv);
            state.save(loom, ns, collection)?;
            Ok(result)
        }))
    }

    pub fn etcd_txn(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
        compare: Vec<HostedEtcdCompare>,
        success: Vec<HostedEtcdRequestOp>,
        failure: Vec<HostedEtcdRequestOp>,
    ) -> HostedOutcome<HostedEtcdTxnResult> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = ensure_facet_ns(loom, FacetKind::Kv, workspace)?;
            let mut state = EtcdState::load(loom, ns, collection)?;
            let result = state.txn(compare, success, failure)?;
            state.save(loom, ns, collection)?;
            Ok(result)
        }))
    }

    pub fn etcd_lease_grant(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
        requested_id: i64,
        ttl: i64,
    ) -> HostedOutcome<HostedEtcdLeaseGrantResult> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = ensure_facet_ns(loom, FacetKind::Kv, workspace)?;
            let mut state = EtcdState::load(loom, ns, collection)?;
            let result = state.lease_grant(requested_id, ttl)?;
            state.save(loom, ns, collection)?;
            Ok(result)
        }))
    }

    pub fn etcd_lease_keep_alive(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
        id: i64,
    ) -> HostedOutcome<HostedEtcdLeaseKeepAliveResult> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = resolve_facet_ns(loom, FacetKind::Kv, workspace)?;
            let mut state = EtcdState::load(loom, ns, collection)?;
            let result = state.lease_keep_alive(id)?;
            state.save(loom, ns, collection)?;
            Ok(result)
        }))
    }

    pub fn etcd_lease_revoke(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
        id: i64,
    ) -> HostedOutcome<HostedEtcdDeleteRangeResult> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = resolve_facet_ns(loom, FacetKind::Kv, workspace)?;
            let mut state = EtcdState::load(loom, ns, collection)?;
            let result = state.lease_revoke(id);
            state.save(loom, ns, collection)?;
            Ok(result)
        }))
    }

    pub fn document_put(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
        id: &str,
        document: Vec<u8>,
    ) -> HostedOutcome<()> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = ensure_facet_ns(loom, FacetKind::Document, workspace)?;
            document::doc_put(loom, ns, collection, id, document)
        }))
    }

    pub fn document_get(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
        id: &str,
    ) -> HostedOutcome<Option<Vec<u8>>> {
        self.read_facet(auth, FacetKind::Document, workspace, |loom, ns| {
            document::doc_get(loom, ns, collection, id)
        })
    }

    pub fn document_put_text(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
        id: &str,
        text: &str,
        expected_entity_tag: Option<&str>,
    ) -> HostedOutcome<HostedDocumentPutResult> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = ensure_facet_ns(loom, FacetKind::Document, workspace)?;
            document::document_put_text_with_entity_tag(
                loom,
                ns,
                collection,
                id,
                text,
                expected_entity_tag,
            )
            .map(|result| HostedDocumentPutResult {
                digest: result.digest.to_string(),
                entity_tag: result.entity_tag,
            })
        }))
    }

    pub fn document_get_text(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
        id: &str,
    ) -> HostedOutcome<Option<HostedDocumentText>> {
        self.read_facet(auth, FacetKind::Document, workspace, |loom, ns| {
            document::document_get_text(loom, ns, collection, id).map(|document| {
                document.map(|document| HostedDocumentText {
                    text: document.text,
                    digest: document.digest.to_string(),
                    entity_tag: document.entity_tag,
                })
            })
        })
    }

    pub fn document_put_binary(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
        id: &str,
        bytes: Vec<u8>,
        expected_entity_tag: Option<&str>,
    ) -> HostedOutcome<HostedDocumentPutResult> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = ensure_facet_ns(loom, FacetKind::Document, workspace)?;
            document::document_put_binary_with_entity_tag(
                loom,
                ns,
                collection,
                id,
                bytes,
                expected_entity_tag,
            )
            .map(|result| HostedDocumentPutResult {
                digest: result.digest.to_string(),
                entity_tag: result.entity_tag,
            })
        }))
    }

    pub fn document_get_binary(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
        id: &str,
    ) -> HostedOutcome<Option<HostedDocumentBinary>> {
        self.read_facet(auth, FacetKind::Document, workspace, |loom, ns| {
            document::document_get_binary(loom, ns, collection, id).map(|document| {
                document.map(|document| HostedDocumentBinary {
                    bytes: document.bytes,
                    digest: document.digest.to_string(),
                    entity_tag: document.entity_tag,
                })
            })
        })
    }

    pub fn document_delete(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
        id: &str,
    ) -> HostedOutcome<bool> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = resolve_facet_ns(loom, FacetKind::Document, workspace)?;
            document::doc_delete(loom, ns, collection, id)
        }))
    }

    pub fn document_list_binary(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
    ) -> HostedOutcome<Vec<u8>> {
        self.read_facet(auth, FacetKind::Document, workspace, |loom, ns| {
            document::document_list_binary(loom, ns, collection)
        })
    }

    pub fn document_list(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
    ) -> HostedOutcome<Vec<DocumentEntry>> {
        self.read_facet(auth, FacetKind::Document, workspace, |loom, ns| {
            let collection = document::doc_list(loom, ns, collection)?;
            Ok(collection
                .iter()
                .map(|(id, doc)| DocumentEntry {
                    id: id.to_string(),
                    document: doc.to_vec(),
                })
                .collect())
        })
    }

    pub fn document_create_index(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
        name: &str,
        path: &str,
        unique: bool,
    ) -> HostedOutcome<()> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = ensure_facet_ns(loom, FacetKind::Document, workspace)?;
            let index = document::DocumentIndexDef::new(
                name,
                document::DocumentFieldPath::dotted(path)?,
                unique,
            )?;
            document::doc_create_index(loom, ns, collection, index)
        }))
    }

    pub fn document_create_index_declaration(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
        declaration: document::DocumentIndexDeclaration,
    ) -> HostedOutcome<()> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = ensure_facet_ns(loom, FacetKind::Document, workspace)?;
            document::doc_create_index_declaration(loom, ns, collection, declaration)
        }))
    }

    pub fn document_drop_index(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
        name: &str,
    ) -> HostedOutcome<bool> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = resolve_facet_ns(loom, FacetKind::Document, workspace)?;
            document::doc_drop_index(loom, ns, collection, name)
        }))
    }

    pub fn document_rebuild_index(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
        name: &str,
    ) -> HostedOutcome<()> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = resolve_facet_ns(loom, FacetKind::Document, workspace)?;
            document::doc_rebuild_index(loom, ns, collection, name)
        }))
    }

    pub fn document_list_indexes(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
    ) -> HostedOutcome<Vec<HostedDocumentIndex>> {
        self.read_facet(auth, FacetKind::Document, workspace, |loom, ns| {
            document::doc_list_index_declarations(loom, ns, collection).map(|indexes| {
                indexes
                    .into_iter()
                    .map(|index| HostedDocumentIndex {
                        metadata: document::document_index_declaration_json(index.clone())
                            .get("metadata")
                            .cloned()
                            .unwrap_or(serde_json::Value::Object(Default::default())),
                        index_id: index.index_id,
                        name: index.index_name,
                        path: document_field_path_string(&index.source_selector),
                        extractor: index.extractor,
                        key_codec: index.key_codec,
                        comparator: index.comparator,
                        uniqueness: index.uniqueness.as_str().to_string(),
                        unique: matches!(
                            index.uniqueness,
                            document::DocumentIndexUniqueness::Unique
                        ),
                        failure_policy: index.failure_policy,
                        declaration_version: index.declaration_version,
                        analyzer_profile: index.analyzer_profile,
                        projection: index.projection.as_ref().map(document_field_path_string),
                        partial_filter: index.partial_filter,
                    })
                    .collect()
            })
        })
    }

    pub fn document_index_statuses(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
    ) -> HostedOutcome<Vec<document::DocumentIndexStatus>> {
        self.read_facet(auth, FacetKind::Document, workspace, |loom, ns| {
            document::doc_index_statuses(loom, ns, collection)
        })
    }

    pub fn document_find(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
        index: &str,
        value: &Value,
    ) -> HostedOutcome<Vec<String>> {
        self.read_facet(auth, FacetKind::Document, workspace, |loom, ns| {
            document::doc_find(loom, ns, collection, index, value)
        })
    }

    pub fn document_query(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
        query: &document::DocumentQuery,
    ) -> HostedOutcome<document::DocumentQueryResult> {
        self.read_facet(auth, FacetKind::Document, workspace, |loom, ns| {
            document::doc_query(loom, ns, collection, query)
        })
    }

    pub fn queue_append(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        stream: &str,
        payload: &[u8],
    ) -> HostedOutcome<usize> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = ensure_facet_ns(loom, FacetKind::Queue, workspace)?;
            log::append(loom, ns, stream, payload)
        }))
    }

    pub fn queue_create(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        stream: &str,
    ) -> HostedOutcome<bool> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = ensure_facet_ns(loom, FacetKind::Queue, workspace)?;
            match log::len(loom, ns, stream) {
                Ok(_) => Ok(false),
                Err(err) if err.code == Code::NotFound => {
                    log::put_stream(loom, ns, stream, &log::Stream::new())?;
                    Ok(true)
                }
                Err(err) => Err(err),
            }
        }))
    }

    pub fn queue_delete(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        stream: &str,
    ) -> HostedOutcome<bool> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = resolve_facet_ns(loom, FacetKind::Queue, workspace)?;
            loom.authorize(ns, FacetKind::Queue, AclRight::Write)?;
            match log::len(loom, ns, stream) {
                Ok(_) => {
                    loom.remove_file_reserved(ns, &facet_path(FacetKind::Queue, stream))?;
                    Ok(true)
                }
                Err(err) if err.code == Code::NotFound => Ok(false),
                Err(err) => Err(err),
            }
        }))
    }

    pub fn queue_streams(&self, auth: &HostedAuth, workspace: &str) -> HostedOutcome<Vec<String>> {
        self.read_facet(auth, FacetKind::Queue, workspace, |loom, ns| {
            let root = facet_path(FacetKind::Queue, "");
            match loom.list_directory(ns, &root) {
                Ok(entries) => Ok(entries
                    .into_iter()
                    .filter(|entry| entry.kind == FileKind::File)
                    .map(|entry| entry.name)
                    .collect()),
                Err(err) if err.code == Code::NotFound => Ok(Vec::new()),
                Err(err) => Err(err),
            }
        })
    }

    pub fn kafka_topic_create(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        topic: &str,
        partition_count: i32,
    ) -> HostedOutcome<Option<HostedKafkaTopicMetadata>> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            if partition_count <= 0 {
                return Err(LoomError::invalid(
                    "Kafka topic partition count must be positive",
                ));
            }
            let ns = ensure_facet_ns(loom, FacetKind::Queue, workspace)?;
            match read_kafka_topic_metadata(loom, ns, topic) {
                Ok(_) => return Ok(None),
                Err(err) if err.code == Code::NotFound => {}
                Err(err) => return Err(err),
            }
            match log::len(loom, ns, topic) {
                Ok(_) => return Ok(None),
                Err(err) if err.code == Code::NotFound => {}
                Err(err) => return Err(err),
            }
            log::put_stream(loom, ns, topic, &log::Stream::new())?;
            let metadata_version = allocate_kafka_metadata_version(loom, ns, workspace)?;
            let metadata = HostedKafkaTopicMetadata::new(
                workspace,
                topic,
                partition_count,
                hosted_now_ms(),
                metadata_version,
            )?;
            write_kafka_topic_metadata(loom, ns, &metadata)?;
            Ok(Some(metadata))
        }))
    }

    pub fn kafka_topic_delete(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        topic: &str,
    ) -> HostedOutcome<bool> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = resolve_facet_ns(loom, FacetKind::Queue, workspace)?;
            loom.authorize(ns, FacetKind::Queue, AclRight::Write)?;
            match read_kafka_topic_metadata(loom, ns, topic) {
                Ok(_) => {}
                Err(err) if err.code == Code::NotFound => return Ok(false),
                Err(err) => return Err(err),
            }
            match loom.remove_file_reserved(ns, &facet_path(FacetKind::Queue, topic)) {
                Ok(()) => {}
                Err(err) if err.code == Code::NotFound => {}
                Err(err) => return Err(err),
            }
            loom.remove_file_reserved(ns, &kafka_topic_metadata_path(topic))?;
            Ok(true)
        }))
    }

    pub fn kafka_topic_metadata(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        topic: &str,
    ) -> HostedOutcome<HostedKafkaTopicMetadata> {
        self.read_facet(auth, FacetKind::Queue, workspace, |loom, ns| {
            let mut metadata = read_kafka_topic_metadata(loom, ns, topic)?;
            refresh_kafka_partition_offsets(loom, ns, &mut metadata)?;
            Ok(metadata)
        })
    }

    pub fn kafka_topics(
        &self,
        auth: &HostedAuth,
        workspace: &str,
    ) -> HostedOutcome<Vec<HostedKafkaTopicMetadata>> {
        self.read_facet(auth, FacetKind::Queue, workspace, |loom, ns| {
            let root = kafka_topic_metadata_dir();
            let entries = match loom.list_directory(ns, &root) {
                Ok(entries) => entries,
                Err(err) if err.code == Code::NotFound => return Ok(Vec::new()),
                Err(err) => return Err(err),
            };
            let mut topics = Vec::new();
            for entry in entries {
                if entry.kind != FileKind::File {
                    continue;
                }
                let mut metadata = HostedKafkaTopicMetadata::decode(
                    &loom.read_file(ns, &format!("{root}/{}", entry.name))?,
                )?;
                refresh_kafka_partition_offsets(loom, ns, &mut metadata)?;
                topics.push(metadata);
            }
            topics.sort_by(|a, b| a.topic.cmp(&b.topic));
            Ok(topics)
        })
    }

    pub fn kafka_produce_records(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        topic: &str,
        partition: i32,
        expected_base_offset: u64,
        record_batches: &[Vec<u8>],
        producer_append: Option<&HostedKafkaProducerAppend>,
    ) -> HostedOutcome<HostedKafkaProduceResult> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = resolve_facet_ns(loom, FacetKind::Queue, workspace)?;
            append_kafka_records(
                loom,
                ns,
                workspace,
                topic,
                partition,
                expected_base_offset,
                record_batches,
                producer_append,
            )
        }))
    }

    pub fn kafka_produce_transactional_records(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        transactional_id: &str,
        topic: &str,
        partition: i32,
        expected_base_offset: u64,
        record_batches: &[Vec<u8>],
        producer_append: &HostedKafkaProducerAppend,
    ) -> HostedOutcome<HostedKafkaProduceResult> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = resolve_facet_ns(loom, FacetKind::Queue, workspace)?;
            validate_kafka_transactional_id(transactional_id)?;
            let mut transaction = active_kafka_transaction(
                loom,
                ns,
                transactional_id,
                producer_append.producer_id,
                producer_append.producer_epoch,
            )?;
            if !transaction
                .topics
                .iter()
                .any(|entry| entry.topic == topic && entry.partitions.contains(&partition))
            {
                return Err(LoomError::new(
                    Code::Conflict,
                    "Kafka transaction partition is not registered",
                ));
            }
            let result = append_kafka_records(
                loom,
                ns,
                workspace,
                topic,
                partition,
                expected_base_offset,
                record_batches,
                Some(producer_append),
            )?;
            merge_kafka_pending_produced_range(
                &mut transaction.pending_produced_ranges,
                &HostedKafkaPendingProducedRange {
                    topic: topic.to_string(),
                    partition,
                    first_offset: result.base_offset,
                    record_count: producer_append.record_count,
                },
            );
            write_kafka_transaction_state(loom, ns, &transaction)?;
            Ok(result)
        }))
    }

    pub fn kafka_next_offset(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        topic: &str,
        partition: i32,
    ) -> HostedOutcome<u64> {
        self.read_facet(auth, FacetKind::Queue, workspace, |loom, ns| {
            let mut metadata = read_kafka_topic_metadata(loom, ns, topic)?;
            ensure_kafka_partition(&metadata, partition)?;
            refresh_kafka_partition_offsets(loom, ns, &mut metadata)?;
            metadata
                .partitions
                .iter()
                .find(|entry| entry.partition == partition)
                .map(|entry| entry.high_watermark)
                .ok_or_else(|| LoomError::not_found("Kafka partition not found"))
        })
    }

    pub fn kafka_fetch_records(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        topic: &str,
        partition: i32,
        fetch_offset: u64,
        max_bytes: usize,
        read_committed: bool,
    ) -> HostedOutcome<HostedKafkaFetchResult> {
        self.read_facet(auth, FacetKind::Queue, workspace, |loom, ns| {
            let mut metadata = read_kafka_topic_metadata(loom, ns, topic)?;
            ensure_kafka_partition(&metadata, partition)?;
            refresh_kafka_partition_offsets(loom, ns, &mut metadata)?;
            let high_watermark = metadata
                .partitions
                .iter()
                .find(|entry| entry.partition == partition)
                .map(|entry| entry.high_watermark)
                .unwrap_or(0);
            let lo = usize::try_from(fetch_offset)
                .map_err(|_| LoomError::invalid("Kafka fetch offset is too large"))?;
            let hi = usize::try_from(high_watermark)
                .map_err(|_| LoomError::invalid("Kafka high watermark is too large"))?;
            let payloads = if lo >= hi {
                Vec::new()
            } else {
                log::range(loom, ns, topic, lo, hi)?
            };
            let visibility = if read_committed {
                kafka_transaction_visibility(loom, ns, topic, partition)?
            } else {
                KafkaTransactionVisibility::default()
            };
            let mut records = Vec::new();
            for (index, payload) in payloads.into_iter().enumerate() {
                let offset = fetch_offset
                    .checked_add(
                        u64::try_from(index)
                            .map_err(|_| LoomError::invalid("Kafka fetch index is too large"))?,
                    )
                    .ok_or_else(|| LoomError::invalid("Kafka fetch offset overflows"))?;
                if read_committed && visibility.is_hidden(offset) {
                    continue;
                }
                if max_bytes > 0 && records.len().saturating_add(payload.len()) > max_bytes {
                    break;
                }
                records.extend_from_slice(&payload);
            }
            Ok(HostedKafkaFetchResult {
                high_watermark,
                last_stable_offset: visibility.last_stable_offset.unwrap_or(high_watermark),
                aborted_transactions: visibility.aborted_transactions,
                records,
            })
        })
    }

    pub fn kafka_init_producer_id(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        transactional_id: Option<&str>,
        requested_producer_id: i64,
        requested_producer_epoch: i16,
    ) -> HostedOutcome<HostedKafkaProducerState> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = resolve_facet_ns(loom, FacetKind::Queue, workspace)?;
            let state = match transactional_id {
                Some(transactional_id) => {
                    validate_kafka_transactional_id(transactional_id)?;
                    let current =
                        read_kafka_producer_state_by_transactional_id(loom, ns, transactional_id)?;
                    let next = match current {
                        Some(mut current) => {
                            if requested_producer_id >= 0
                                && (current.producer_id != requested_producer_id
                                    || current.producer_epoch != requested_producer_epoch)
                            {
                                return Err(LoomError::new(
                                    Code::FencingStale,
                                    "Kafka producer epoch is stale",
                                ));
                            }
                            current.producer_epoch =
                                current.producer_epoch.checked_add(1).ok_or_else(|| {
                                    LoomError::invalid("Kafka producer epoch overflows")
                                })?;
                            current
                        }
                        None => HostedKafkaProducerState {
                            producer_id: allocate_kafka_producer_id(loom, ns)?,
                            producer_epoch: 0,
                            transactional_id: Some(transactional_id.to_string()),
                        },
                    };
                    write_kafka_producer_state(loom, ns, &next)?;
                    next
                }
                None => {
                    let state = HostedKafkaProducerState {
                        producer_id: allocate_kafka_producer_id(loom, ns)?,
                        producer_epoch: 0,
                        transactional_id: None,
                    };
                    write_kafka_producer_state(loom, ns, &state)?;
                    state
                }
            };
            Ok(state)
        }))
    }

    pub fn kafka_add_partitions_to_transaction(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        transactional_id: &str,
        producer_id: i64,
        producer_epoch: i16,
        topics: &[HostedKafkaTransactionTopic],
    ) -> HostedOutcome<()> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = resolve_facet_ns(loom, FacetKind::Queue, workspace)?;
            validate_kafka_transactional_id(transactional_id)?;
            validate_kafka_producer_epoch(loom, ns, transactional_id, producer_id, producer_epoch)?;
            for topic in topics {
                let metadata = read_kafka_topic_metadata(loom, ns, &topic.topic)?;
                for partition in &topic.partitions {
                    ensure_kafka_partition(&metadata, *partition)?;
                }
            }
            let mut transaction =
                active_kafka_transaction(loom, ns, transactional_id, producer_id, producer_epoch)?;
            for topic in topics {
                merge_kafka_transaction_topic(&mut transaction.topics, topic);
            }
            write_kafka_transaction_state(loom, ns, &transaction)
        }))
    }

    pub fn kafka_add_offsets_to_transaction(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        transactional_id: &str,
        producer_id: i64,
        producer_epoch: i16,
        group_id: &str,
    ) -> HostedOutcome<()> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = resolve_facet_ns(loom, FacetKind::Queue, workspace)?;
            validate_kafka_transactional_id(transactional_id)?;
            validate_kafka_group_id(group_id)?;
            validate_kafka_producer_epoch(loom, ns, transactional_id, producer_id, producer_epoch)?;
            let mut transaction =
                active_kafka_transaction(loom, ns, transactional_id, producer_id, producer_epoch)?;
            if !transaction.groups.iter().any(|entry| entry == group_id) {
                transaction.groups.push(group_id.to_string());
                transaction.groups.sort();
            }
            write_kafka_transaction_state(loom, ns, &transaction)
        }))
    }

    pub fn kafka_end_transaction(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        transactional_id: &str,
        producer_id: i64,
        producer_epoch: i16,
        committed: bool,
    ) -> HostedOutcome<()> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = resolve_facet_ns(loom, FacetKind::Queue, workspace)?;
            validate_kafka_transactional_id(transactional_id)?;
            validate_kafka_producer_epoch(loom, ns, transactional_id, producer_id, producer_epoch)?;
            let Some(mut transaction) = read_kafka_transaction_state(loom, ns, transactional_id)?
            else {
                return Err(LoomError::new(
                    Code::Conflict,
                    "Kafka transaction is not active",
                ));
            };
            if transaction.status != HostedKafkaTransactionStatus::Active
                || transaction.producer_id != producer_id
                || transaction.producer_epoch != producer_epoch
            {
                return Err(LoomError::new(
                    Code::Conflict,
                    "Kafka transaction is not active",
                ));
            }
            if committed {
                for pending in &transaction.pending_offset_commits {
                    apply_kafka_offset_commit(
                        loom,
                        ns,
                        &pending.topic,
                        pending.partition,
                        &pending.group_id,
                        pending.offset,
                    )?;
                }
            }
            transaction.status = if committed {
                HostedKafkaTransactionStatus::Committed
            } else {
                HostedKafkaTransactionStatus::Aborted
            };
            write_kafka_transaction_state(loom, ns, &transaction)
        }))
    }

    pub fn kafka_validate_transaction(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        transactional_id: &str,
        producer_id: i64,
        producer_epoch: i16,
    ) -> HostedOutcome<()> {
        self.read_facet(auth, FacetKind::Queue, workspace, |loom, ns| {
            validate_kafka_transactional_id(transactional_id)?;
            validate_kafka_producer_epoch(loom, ns, transactional_id, producer_id, producer_epoch)?;
            let Some(transaction) = read_kafka_transaction_state(loom, ns, transactional_id)?
            else {
                return Err(LoomError::new(
                    Code::Conflict,
                    "Kafka transaction is not active",
                ));
            };
            if transaction.status == HostedKafkaTransactionStatus::Active {
                Ok(())
            } else {
                Err(LoomError::new(
                    Code::Conflict,
                    "Kafka transaction is not active",
                ))
            }
        })
    }

    pub fn kafka_txn_offset_commit(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        transactional_id: &str,
        producer_id: i64,
        producer_epoch: i16,
        offsets: &[HostedKafkaPendingOffsetCommit],
    ) -> HostedOutcome<()> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = resolve_facet_ns(loom, FacetKind::Queue, workspace)?;
            validate_kafka_transactional_id(transactional_id)?;
            validate_kafka_producer_epoch(loom, ns, transactional_id, producer_id, producer_epoch)?;
            let Some(mut transaction) = read_kafka_transaction_state(loom, ns, transactional_id)?
            else {
                return Err(LoomError::new(
                    Code::Conflict,
                    "Kafka transaction is not active",
                ));
            };
            if transaction.status != HostedKafkaTransactionStatus::Active
                || transaction.producer_id != producer_id
                || transaction.producer_epoch != producer_epoch
            {
                return Err(LoomError::new(
                    Code::Conflict,
                    "Kafka transaction is not active",
                ));
            }
            for offset in offsets {
                validate_kafka_offset_commit(
                    loom,
                    ns,
                    &offset.topic,
                    offset.partition,
                    &offset.group_id,
                    offset.offset,
                )?;
                merge_kafka_pending_offset(&mut transaction.pending_offset_commits, offset);
            }
            write_kafka_transaction_state(loom, ns, &transaction)
        }))
    }

    pub fn kafka_offset_commit(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        topic: &str,
        partition: i32,
        group_id: &str,
        offset: u64,
    ) -> HostedOutcome<()> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = resolve_facet_ns(loom, FacetKind::Queue, workspace)?;
            apply_kafka_offset_commit(loom, ns, topic, partition, group_id, offset)
        }))
    }

    pub fn kafka_offset_position(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        topic: &str,
        partition: i32,
        group_id: &str,
    ) -> HostedOutcome<u64> {
        self.read_facet(auth, FacetKind::Queue, workspace, |loom, ns| {
            log::consumer_position(loom, ns, topic, &kafka_consumer_id(group_id, partition))
        })
    }

    pub fn queue_get(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        stream: &str,
        seq: usize,
    ) -> HostedOutcome<Option<Vec<u8>>> {
        self.read_facet(auth, FacetKind::Queue, workspace, |loom, ns| {
            log::get(loom, ns, stream, seq)
        })
    }

    pub fn queue_len(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        stream: &str,
    ) -> HostedOutcome<usize> {
        self.read_facet(auth, FacetKind::Queue, workspace, |loom, ns| {
            log::len(loom, ns, stream)
        })
    }

    pub fn queue_range(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        stream: &str,
        lo: usize,
        hi: usize,
    ) -> HostedOutcome<Vec<QueueEntry>> {
        self.read_facet(auth, FacetKind::Queue, workspace, |loom, ns| {
            let payloads = log::range(loom, ns, stream, lo, hi)?;
            Ok(payloads
                .into_iter()
                .enumerate()
                .map(|(offset, payload)| QueueEntry {
                    seq: lo + offset,
                    payload,
                })
                .collect())
        })
    }

    pub fn timeseries_put(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        series: &str,
        timestamp: i64,
        value: Vec<u8>,
    ) -> HostedOutcome<()> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = ensure_facet_ns(loom, FacetKind::TimeSeries, workspace)?;
            timeseries::ts_put(loom, ns, series, timestamp, value)
        }))
    }

    pub fn timeseries_put_structured(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        series: &str,
        point: StructuredTimeSeriesPoint,
    ) -> HostedOutcome<()> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = ensure_facet_ns(loom, FacetKind::TimeSeries, workspace)?;
            timeseries::ts_put_point(
                loom,
                ns,
                series,
                StructuredPoint::new(
                    point.measurement,
                    point.tags,
                    point.timestamp_ns,
                    point.fields,
                )?,
            )
        }))
    }

    pub fn timeseries_put_structured_batch(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        series: &str,
        points: Vec<StructuredTimeSeriesPoint>,
    ) -> HostedOutcome<usize> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = ensure_facet_ns(loom, FacetKind::TimeSeries, workspace)?;
            let mut written = 0usize;
            for point in points {
                timeseries::ts_put_point(
                    loom,
                    ns,
                    series,
                    StructuredPoint::new(
                        point.measurement,
                        point.tags,
                        point.timestamp_ns,
                        point.fields,
                    )?,
                )?;
                written += 1;
            }
            Ok(written)
        }))
    }

    pub fn timeseries_get(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        series: &str,
        timestamp: i64,
    ) -> HostedOutcome<Option<Vec<u8>>> {
        self.read_facet(auth, FacetKind::TimeSeries, workspace, |loom, ns| {
            timeseries::ts_get(loom, ns, series, timestamp)
        })
    }

    pub fn timeseries_latest(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        series: &str,
    ) -> HostedOutcome<Option<TimeSeriesPoint>> {
        self.read_facet(auth, FacetKind::TimeSeries, workspace, |loom, ns| {
            Ok(timeseries::ts_latest(loom, ns, series)?
                .map(|(timestamp, value)| TimeSeriesPoint { timestamp, value }))
        })
    }

    pub fn timeseries_range(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        series: &str,
        from: i64,
        to: i64,
    ) -> HostedOutcome<Vec<TimeSeriesPoint>> {
        self.read_facet(auth, FacetKind::TimeSeries, workspace, |loom, ns| {
            let points = timeseries::ts_range(loom, ns, series, from, to)?;
            Ok(points
                .iter()
                .map(|(timestamp, value)| TimeSeriesPoint {
                    timestamp,
                    value: value.to_vec(),
                })
                .collect())
        })
    }

    pub fn timeseries_range_structured(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        series: &str,
        from_ns: i64,
        to_ns: i64,
    ) -> HostedOutcome<Vec<StructuredTimeSeriesPoint>> {
        self.read_facet(auth, FacetKind::TimeSeries, workspace, |loom, ns| {
            timeseries::ts_range_points(loom, ns, series, from_ns, to_ns).map(|points| {
                points
                    .into_iter()
                    .map(|point| StructuredTimeSeriesPoint {
                        measurement: point.measurement,
                        tags: point.tags,
                        timestamp_ns: point.timestamp_ns,
                        fields: point.fields,
                    })
                    .collect()
            })
        })
    }

    pub fn timeseries_policy(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        series: &str,
    ) -> HostedOutcome<TimeSeriesPolicy> {
        self.read_facet(auth, FacetKind::TimeSeries, workspace, |loom, ns| {
            timeseries::ts_policy(loom, ns, series)
        })
    }

    pub fn timeseries_set_policy(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        series: &str,
        query_start_ns: Option<i64>,
        rollups: Vec<(String, i64, TimeSeriesAggregation)>,
    ) -> HostedOutcome<()> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = ensure_facet_ns(loom, FacetKind::TimeSeries, workspace)?;
            let rollups = rollups
                .into_iter()
                .map(|(name, resolution_ns, aggregation)| {
                    TimeSeriesRollup::new(name, resolution_ns, aggregation)
                })
                .collect::<Result<Vec<_>>>()?;
            timeseries::ts_set_policy(
                loom,
                ns,
                series,
                TimeSeriesPolicy {
                    query_start_ns,
                    rollups,
                },
            )
        }))
    }

    pub fn timeseries_materialize_rollup(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        series: &str,
        rollup: &str,
    ) -> HostedOutcome<()> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = ensure_facet_ns(loom, FacetKind::TimeSeries, workspace)?;
            timeseries::ts_materialize_rollup(loom, ns, series, rollup)
        }))
    }

    pub fn timeseries_range_rollup_structured(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        series: &str,
        rollup: &str,
        from_ns: i64,
        to_ns: i64,
    ) -> HostedOutcome<Vec<StructuredTimeSeriesPoint>> {
        self.read_facet(auth, FacetKind::TimeSeries, workspace, |loom, ns| {
            timeseries::ts_range_rollup_points(loom, ns, series, rollup, from_ns, to_ns).map(
                |points| {
                    points
                        .into_iter()
                        .map(|point| StructuredTimeSeriesPoint {
                            measurement: point.measurement,
                            tags: point.tags,
                            timestamp_ns: point.timestamp_ns,
                            fields: point.fields,
                        })
                        .collect()
                },
            )
        })
    }

    pub fn timeseries_prune_before(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        series: &str,
        cutoff_ns: i64,
    ) -> HostedOutcome<usize> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = ensure_facet_ns(loom, FacetKind::TimeSeries, workspace)?;
            timeseries::ts_prune_before(loom, ns, series, cutoff_ns)
        }))
    }

    fn read_facet<T>(
        &self,
        auth: &HostedAuth,
        facet: FacetKind,
        workspace: &str,
        f: impl FnOnce(&Loom<FileStore>, WorkspaceId) -> Result<T>,
    ) -> HostedOutcome<T> {
        hosted_outcome(self.kernel.read(auth, |loom| {
            let ns = resolve_facet_ns(loom, facet, workspace)?;
            f(loom, ns)
        }))
    }

    pub fn graph_upsert_node(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        graph_name: &str,
        id: &str,
        props: graph::Props,
    ) -> HostedOutcome<()> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = ensure_facet_ns(loom, FacetKind::Graph, workspace)?;
            graph::graph_upsert_node(loom, ns, graph_name, id, props)
        }))
    }

    pub fn graph_get_node(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        graph_name: &str,
        id: &str,
    ) -> HostedOutcome<Option<graph::Props>> {
        self.read_facet(auth, FacetKind::Graph, workspace, |loom, ns| {
            graph::graph_get_node(loom, ns, graph_name, id)
        })
    }

    pub fn graph_apply_mutations(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        graph_name: &str,
        plan: &graph::GraphMutationPlan,
    ) -> HostedOutcome<graph::GraphMutationResult> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = ensure_facet_ns(loom, FacetKind::Graph, workspace)?;
            graph::graph_apply_mutations(loom, ns, graph_name, plan)
        }))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn graph_upsert_edge(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        graph_name: &str,
        id: &str,
        src: &str,
        dst: &str,
        label: &str,
        props: graph::Props,
    ) -> HostedOutcome<()> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = ensure_facet_ns(loom, FacetKind::Graph, workspace)?;
            graph::graph_upsert_edge(loom, ns, graph_name, id, src, dst, label, props)
        }))
    }

    pub fn graph_neighbors(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        graph_name: &str,
        id: &str,
    ) -> HostedOutcome<Vec<String>> {
        self.read_facet(auth, FacetKind::Graph, workspace, |loom, ns| {
            graph::graph_neighbors(loom, ns, graph_name, id)
        })
    }

    pub fn graph_reachable(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        graph_name: &str,
        start: &str,
        max_depth: Option<usize>,
        via_label: Option<&str>,
    ) -> HostedOutcome<Vec<String>> {
        self.read_facet(auth, FacetKind::Graph, workspace, |loom, ns| {
            graph::graph_reachable(loom, ns, graph_name, start, max_depth, via_label)
        })
    }

    pub fn graph_query(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        graph_name: &str,
        query: &graph::GraphQuery,
    ) -> HostedOutcome<graph::GraphQueryResult> {
        self.read_facet(auth, FacetKind::Graph, workspace, |loom, ns| {
            graph::graph_query(loom, ns, graph_name, query)
        })
    }

    pub fn graph_explain_query(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        graph_name: &str,
        query: &graph::GraphQuery,
    ) -> HostedOutcome<graph::GraphQueryExplain> {
        self.read_facet(auth, FacetKind::Graph, workspace, |loom, ns| {
            graph::graph_explain_query(loom, ns, graph_name, query)
        })
    }

    pub fn ledger_append(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
        payload: Vec<u8>,
    ) -> HostedOutcome<u64> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = ensure_facet_ns(loom, FacetKind::Ledger, workspace)?;
            ledger::ledger_append(loom, ns, collection, payload)
        }))
    }

    pub fn ledger_get(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
        seq: u64,
    ) -> HostedOutcome<Option<Vec<u8>>> {
        self.read_facet(auth, FacetKind::Ledger, workspace, |loom, ns| {
            ledger::ledger_get(loom, ns, collection, seq)
        })
    }

    pub fn ledger_range(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
        start: u64,
        end: u64,
    ) -> HostedOutcome<ledger::LedgerRangeScan> {
        self.read_facet(auth, FacetKind::Ledger, workspace, |loom, ns| {
            ledger::ledger_range(loom, ns, collection, start, end)
        })
    }

    pub fn ledger_head(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
    ) -> HostedOutcome<Option<String>> {
        self.read_facet(auth, FacetKind::Ledger, workspace, |loom, ns| {
            ledger::ledger_head(loom, ns, collection)
                .map(|head| head.map(|digest| digest.to_string()))
        })
    }

    pub fn ledger_len(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
    ) -> HostedOutcome<u64> {
        self.read_facet(auth, FacetKind::Ledger, workspace, |loom, ns| {
            ledger::ledger_len(loom, ns, collection)
        })
    }

    pub fn ledger_verify(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
    ) -> HostedOutcome<()> {
        self.read_facet(auth, FacetKind::Ledger, workspace, |loom, ns| {
            ledger::ledger_verify(loom, ns, collection)
        })
    }

    pub fn ledger_list_collections(
        &self,
        auth: &HostedAuth,
        workspace: &str,
    ) -> HostedOutcome<Vec<String>> {
        self.read_facet(auth, FacetKind::Ledger, workspace, |loom, ns| {
            ledger::ledger_list_collections(loom, ns)
        })
    }

    pub fn ledger_checkpoint_payload(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
    ) -> HostedOutcome<Vec<u8>> {
        self.read_facet(auth, FacetKind::Ledger, workspace, |loom, ns| {
            ledger::ledger_checkpoint_payload_bytes(loom, ns, collection)
        })
    }

    pub fn ledger_verify_checkpoint_signatures(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
    ) -> HostedOutcome<usize> {
        self.read_facet(auth, FacetKind::Ledger, workspace, |loom, ns| {
            ledger::ledger_verify_checkpoint_signatures(loom, ns, collection)
        })
    }

    pub fn ledger_proof_tree(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
    ) -> HostedOutcome<Vec<u8>> {
        self.read_facet(auth, FacetKind::Ledger, workspace, |loom, ns| {
            ledger::ledger_proof_tree(loom, ns, collection).map(|proof| proof.encode())
        })
    }

    pub fn ledger_inclusion_proof(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
        seq: u64,
    ) -> HostedOutcome<Vec<u8>> {
        self.read_facet(auth, FacetKind::Ledger, workspace, |loom, ns| {
            ledger::ledger_inclusion_proof(loom, ns, collection, seq).map(|proof| proof.encode())
        })
    }

    pub fn ledger_consistency_proof(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
        first_tree_size: u64,
        second_tree_size: u64,
    ) -> HostedOutcome<Vec<u8>> {
        self.read_facet(auth, FacetKind::Ledger, workspace, |loom, ns| {
            ledger::ledger_consistency_proof(
                loom,
                ns,
                collection,
                first_tree_size,
                second_tree_size,
            )
            .map(|proof| proof.encode())
        })
    }

    pub fn vector_create(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
        dim: usize,
        metric: Metric,
    ) -> HostedOutcome<()> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = ensure_facet_ns(loom, FacetKind::Vector, workspace)?;
            vector::vector_create(loom, ns, collection, dim, metric)
        }))
    }

    pub fn vector_upsert(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
        id: &str,
        vector: Vec<f32>,
        metadata: std::collections::BTreeMap<String, Value>,
    ) -> HostedOutcome<()> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = resolve_facet_ns(loom, FacetKind::Vector, workspace)?;
            vector::vector_upsert(loom, ns, collection, id, vector, metadata)
        }))
    }

    pub fn vector_get(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
        id: &str,
    ) -> HostedOutcome<Option<HostedVectorEntry>> {
        self.read_facet(auth, FacetKind::Vector, workspace, |loom, ns| {
            Ok(vector::vector_get(loom, ns, collection, id)?
                .map(|(vector, metadata)| HostedVectorEntry { vector, metadata }))
        })
    }

    pub fn vector_delete(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
        id: &str,
    ) -> HostedOutcome<bool> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = resolve_facet_ns(loom, FacetKind::Vector, workspace)?;
            vector::vector_delete(loom, ns, collection, id)
        }))
    }

    pub fn vector_info(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
    ) -> HostedOutcome<HostedVectorInfo> {
        self.read_facet(auth, FacetKind::Vector, workspace, |loom, ns| {
            let set = vector::get_vector_set(loom, ns, collection)?;
            Ok(HostedVectorInfo {
                dim: set.dim(),
                metric: set.metric(),
                count: set.len(),
            })
        })
    }

    pub fn vector_search(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
        query: &[f32],
        k: usize,
    ) -> HostedOutcome<Vec<Hit>> {
        self.vector_search_filtered(
            auth,
            workspace,
            collection,
            query,
            k,
            &vector::MetaFilter::All,
        )
    }

    pub fn vector_search_filtered(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
        query: &[f32],
        k: usize,
        filter: &vector::MetaFilter,
    ) -> HostedOutcome<Vec<Hit>> {
        self.read_facet(auth, FacetKind::Vector, workspace, |loom, ns| {
            vector::vector_search(loom, ns, collection, query, k, filter)
        })
    }

    pub fn vector_scroll(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
        offset: Option<&str>,
        limit: usize,
        filter: &vector::MetaFilter,
    ) -> HostedOutcome<Vec<(String, HostedVectorEntry)>> {
        self.read_facet(auth, FacetKind::Vector, workspace, |loom, ns| {
            let ids = vector::vector_ids(loom, ns, collection, None)?;
            let mut out = Vec::new();
            for id in ids {
                if let Some(offset) = offset
                    && id.as_str() <= offset
                {
                    continue;
                }
                let Some((vector, metadata)) = vector::vector_get(loom, ns, collection, &id)?
                else {
                    continue;
                };
                if filter.eval(&metadata) {
                    out.push((id, HostedVectorEntry { vector, metadata }));
                    if out.len() == limit {
                        break;
                    }
                }
            }
            Ok(out)
        })
    }

    pub fn vector_count(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
        filter: &vector::MetaFilter,
    ) -> HostedOutcome<usize> {
        self.read_facet(auth, FacetKind::Vector, workspace, |loom, ns| {
            let mut count = 0;
            for id in vector::vector_ids(loom, ns, collection, None)? {
                let Some((_, metadata)) = vector::vector_get(loom, ns, collection, &id)? else {
                    continue;
                };
                if filter.eval(&metadata) {
                    count += 1;
                }
            }
            Ok(count)
        })
    }

    pub fn vector_pinecone_workspace_stats(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        index: &str,
    ) -> HostedOutcome<Vec<HostedPineconeWorkspaceStats>> {
        self.read_facet(auth, FacetKind::Vector, workspace, |loom, ns| {
            let index_segment = path_segment(index);
            let root = facet_path(
                FacetKind::Vector,
                &format!("compat/pinecone/{index_segment}/workspaces"),
            );
            let paths = match loom.walk(ns, &root) {
                Ok(paths) => paths,
                Err(err) if err.code == Code::NotFound => return Ok(Vec::new()),
                Err(err) => return Err(err),
            };
            let mut workspaces = std::collections::BTreeSet::new();
            let prefix = format!("{root}/");
            for path in paths {
                let Some(rest) = path.strip_prefix(&prefix) else {
                    continue;
                };
                let Some(segment) = rest.split('/').next() else {
                    continue;
                };
                if !segment.is_empty() {
                    workspaces.insert(segment.to_string());
                }
            }
            let mut out = Vec::new();
            for workspace_segment in workspaces {
                let Some(profile_workspace) = decode_path_segment(&workspace_segment) else {
                    continue;
                };
                let vector_set =
                    format!("compat/pinecone/{index_segment}/workspaces/{workspace_segment}");
                let set = vector::get_vector_set(loom, ns, &vector_set)?;
                out.push(HostedPineconeWorkspaceStats {
                    workspace: profile_workspace,
                    dim: set.dim(),
                    metric: set.metric(),
                    count: set.len(),
                });
            }
            Ok(out)
        })
    }

    pub fn search_create(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
        mapping: Mapping,
    ) -> HostedOutcome<()> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = ensure_facet_ns(loom, FacetKind::Search, workspace)?;
            search::search_create(loom, ns, collection, mapping)
        }))
    }

    pub fn search_index(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
        id: Vec<u8>,
        doc: search::Document,
    ) -> HostedOutcome<()> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = resolve_facet_ns(loom, FacetKind::Search, workspace)?;
            search::search_index(loom, ns, collection, id, doc)
        }))
    }

    pub fn search_get(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
        id: &[u8],
    ) -> HostedOutcome<Option<search::Document>> {
        self.read_facet(auth, FacetKind::Search, workspace, |loom, ns| {
            search::search_get(loom, ns, collection, id)
        })
    }

    pub fn search_delete(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
        id: &[u8],
    ) -> HostedOutcome<bool> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = resolve_facet_ns(loom, FacetKind::Search, workspace)?;
            search::search_delete(loom, ns, collection, id)
        }))
    }

    pub fn search_drop(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
    ) -> HostedOutcome<bool> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = resolve_facet_ns(loom, FacetKind::Search, workspace)?;
            search::search_drop(loom, ns, collection)
        }))
    }

    pub fn search_ids(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
        prefix: Option<&[u8]>,
    ) -> HostedOutcome<Vec<Vec<u8>>> {
        self.read_facet(auth, FacetKind::Search, workspace, |loom, ns| {
            search::search_ids(loom, ns, collection, prefix)
        })
    }

    pub fn search_remap(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
        mapping: Mapping,
    ) -> HostedOutcome<()> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = resolve_facet_ns(loom, FacetKind::Search, workspace)?;
            search::search_remap(loom, ns, collection, mapping)
        }))
    }

    pub fn search_query(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
        request: QueryRequest,
    ) -> HostedOutcome<QueryResponse> {
        self.read_facet(auth, FacetKind::Search, workspace, |loom, ns| {
            search::search_query(loom, ns, collection, &request)
        })
    }

    pub fn search_query_documents(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
        request: QueryRequest,
    ) -> HostedOutcome<HostedSearchQueryDocuments> {
        self.read_facet(auth, FacetKind::Search, workspace, |loom, ns| {
            let search_collection = search::get_search(loom, ns, collection)?;
            let result = search_collection.query(&request)?;
            let documents = result
                .hits
                .iter()
                .filter_map(|hit| {
                    search_collection
                        .get(&hit.id)
                        .cloned()
                        .map(|doc| (hit.id.clone(), doc))
                })
                .collect();
            Ok(HostedSearchQueryDocuments { result, documents })
        })
    }

    pub fn search_collections(
        &self,
        auth: &HostedAuth,
        workspace: &str,
    ) -> HostedOutcome<Vec<String>> {
        self.read_facet(auth, FacetKind::Search, workspace, |loom, ns| {
            search::search_collections(loom, ns)
        })
    }

    pub fn search_alias_set(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        alias: &str,
        targets: Vec<(String, bool)>,
    ) -> HostedOutcome<()> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = resolve_facet_ns(loom, FacetKind::Search, workspace)?;
            search::search_alias_set(
                loom,
                ns,
                alias,
                search::SearchAlias {
                    targets: targets
                        .into_iter()
                        .map(|(collection, is_write_index)| search::SearchAliasTarget {
                            collection,
                            is_write_index,
                        })
                        .collect(),
                },
            )
        }))
    }

    pub fn search_alias_update(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        actions: Vec<HostedSearchAliasAction>,
    ) -> HostedOutcome<()> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = resolve_facet_ns(loom, FacetKind::Search, workspace)?;
            let mut aliases = BTreeMap::<String, search::SearchAlias>::new();
            for action in actions {
                match action {
                    HostedSearchAliasAction::Add {
                        index,
                        alias,
                        is_write_index,
                    } => {
                        if !aliases.contains_key(&alias) {
                            let record = search::search_alias_get(loom, ns, &alias)?.unwrap_or(
                                search::SearchAlias {
                                    targets: Vec::new(),
                                },
                            );
                            aliases.insert(alias.clone(), record);
                        }
                        let record = aliases
                            .get_mut(&alias)
                            .expect("alias record inserted before update");
                        record.targets.retain(|target| target.collection != index);
                        if is_write_index {
                            for target in &mut record.targets {
                                target.is_write_index = false;
                            }
                        }
                        record.targets.push(search::SearchAliasTarget {
                            collection: index,
                            is_write_index,
                        });
                    }
                    HostedSearchAliasAction::Remove { alias } => {
                        aliases.insert(
                            alias,
                            search::SearchAlias {
                                targets: Vec::new(),
                            },
                        );
                    }
                }
            }
            for (alias, record) in aliases {
                if record.targets.is_empty() {
                    search::search_alias_delete(loom, ns, &alias)?;
                } else {
                    search::search_alias_set(loom, ns, &alias, record)?;
                }
            }
            Ok(())
        }))
    }

    pub fn search_alias_get(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        alias: &str,
    ) -> HostedOutcome<Option<search::SearchAlias>> {
        self.read_facet(auth, FacetKind::Search, workspace, |loom, ns| {
            search::search_alias_get(loom, ns, alias)
        })
    }

    pub fn search_alias_delete(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        alias: &str,
    ) -> HostedOutcome<bool> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = resolve_facet_ns(loom, FacetKind::Search, workspace)?;
            search::search_alias_delete(loom, ns, alias)
        }))
    }

    pub fn search_aliases(&self, auth: &HostedAuth, workspace: &str) -> HostedOutcome<Vec<String>> {
        self.read_facet(auth, FacetKind::Search, workspace, |loom, ns| {
            search::search_aliases(loom, ns)
        })
    }

    pub fn dataframe_create(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        frame: &str,
        plan: &dataframe::DataframePlan,
    ) -> HostedOutcome<()> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = ensure_facet_ns(loom, FacetKind::Dataframe, workspace)?;
            dataframe::dataframe_create(loom, ns, frame, plan)
        }))
    }

    pub fn dataframe_collect(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        frame: &str,
    ) -> HostedOutcome<dataframe::DataframeBatch> {
        self.read_facet(auth, FacetKind::Dataframe, workspace, |loom, ns| {
            dataframe::dataframe_collect(loom, ns, frame)
        })
    }

    pub fn dataframe_preview(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        frame: &str,
        rows: u64,
    ) -> HostedOutcome<dataframe::DataframeBatch> {
        self.read_facet(auth, FacetKind::Dataframe, workspace, |loom, ns| {
            dataframe::dataframe_preview(loom, ns, frame, rows)
        })
    }

    pub fn dataframe_materialize(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        frame: &str,
    ) -> HostedOutcome<Option<Digest>> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let ns = resolve_facet_ns(loom, FacetKind::Dataframe, workspace)?;
            dataframe::dataframe_materialize(loom, ns, frame)
        }))
    }

    pub fn dataframe_plan_digest(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        frame: &str,
    ) -> HostedOutcome<Digest> {
        self.read_facet(auth, FacetKind::Dataframe, workspace, |loom, ns| {
            dataframe::dataframe_plan_digest(loom, ns, frame)
        })
    }

    pub fn dataframe_source_digests(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        frame: &str,
    ) -> HostedOutcome<Vec<Digest>> {
        self.read_facet(auth, FacetKind::Dataframe, workspace, |loom, ns| {
            dataframe::dataframe_source_digests(loom, ns, frame)
        })
    }
}

fn document_field_path_string(path: &document::DocumentFieldPath) -> String {
    let mut out = String::new();
    for segment in path.segments() {
        match segment {
            document::DocumentFieldPathSegment::Field(field) => {
                if !out.is_empty() {
                    out.push('.');
                }
                out.push_str(field);
            }
            document::DocumentFieldPathSegment::Index(index) => {
                out.push('[');
                out.push_str(&index.to_string());
                out.push(']');
            }
        }
    }
    out
}

fn kv_entries<'a>(entries: impl Iterator<Item = (&'a Value, &'a [u8])>) -> Vec<KvEntry> {
    entries
        .map(|(key, value)| KvEntry {
            key_cbor: kv::key_to_cbor(key),
            value: value.to_vec(),
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct EtcdKeyMeta {
    create_revision: i64,
    mod_revision: i64,
    version: i64,
    lease: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct EtcdLease {
    id: i64,
    ttl: i64,
    expires_at_ms: u64,
    keys: Vec<Vec<u8>>,
}

struct EtcdState {
    revision: i64,
    compacted_revision: i64,
    values: kv::KvMap,
    meta: kv::KvMap,
    leases: kv::KvMap,
    events: kv::KvMap,
}

impl EtcdState {
    fn load(loom: &mut Loom<FileStore>, ns: WorkspaceId, collection: &str) -> Result<Self> {
        let values = kv::kv_list(loom, ns, collection)?;
        let meta_collection = etcd_meta_collection(collection);
        let leases_collection = etcd_lease_collection(collection);
        let events_collection = etcd_event_collection(collection);
        let meta = kv::kv_list(loom, ns, &meta_collection)?;
        let leases = kv::kv_list(loom, ns, &leases_collection)?;
        let events = kv::kv_list(loom, ns, &events_collection)?;
        let revision = etcd_revision(&meta)?;
        let compacted_revision = etcd_compacted_revision(&meta)?;
        let mut state = Self {
            revision,
            compacted_revision,
            values,
            meta,
            leases,
            events,
        };
        state.expire_leases(hosted_now_ms());
        Ok(state)
    }

    fn save(self, loom: &mut Loom<FileStore>, ns: WorkspaceId, collection: &str) -> Result<()> {
        kv::replace_kv_map(loom, ns, collection, &self.values)?;
        let meta_collection = etcd_meta_collection(collection);
        let leases_collection = etcd_lease_collection(collection);
        let events_collection = etcd_event_collection(collection);
        kv::replace_kv_map(loom, ns, &meta_collection, &self.meta)?;
        kv::replace_kv_map(loom, ns, &leases_collection, &self.leases)?;
        kv::replace_kv_map(loom, ns, &events_collection, &self.events)
    }

    fn range(
        &self,
        key: Vec<u8>,
        range_end: Vec<u8>,
        limit: i64,
        revision: i64,
    ) -> Result<HostedEtcdRangeResult> {
        if revision < 0 {
            return Err(LoomError::invalid(
                "etcd range revision must not be negative",
            ));
        }
        if revision > 0 && revision <= self.compacted_revision {
            return Err(LoomError::invalid("etcd range revision has been compacted"));
        }
        if revision > self.revision {
            return Err(LoomError::invalid("etcd range revision is not available"));
        }
        if revision > 0 && revision < self.revision {
            return Err(LoomError::unsupported(
                "historical etcd range revisions are not supported",
            ));
        }
        let mut kvs = Vec::new();
        let max = if limit <= 0 {
            usize::MAX
        } else {
            usize::try_from(limit).unwrap_or(usize::MAX)
        };
        let mut count = 0i64;
        for (stored_key, value) in self.values.iter() {
            let Value::Bytes(bytes) = stored_key else {
                continue;
            };
            if !etcd_key_in_range(bytes, &key, &range_end) {
                continue;
            }
            count = count.saturating_add(1);
            if kvs.len() < max {
                kvs.push(self.kv_response(bytes.clone(), value.to_vec()));
            }
        }
        Ok(HostedEtcdRangeResult {
            revision: self.revision,
            count,
            kvs,
        })
    }

    fn compact(&mut self, revision: i64, physical: bool) -> Result<HostedEtcdCompactResult> {
        if revision < 0 {
            return Err(LoomError::invalid(
                "etcd compact revision must not be negative",
            ));
        }
        if revision > self.revision {
            return Err(LoomError::invalid("etcd compact revision is not available"));
        }
        self.compacted_revision = self.compacted_revision.max(revision);
        if physical {
            self.prune_events_through(revision);
        }
        self.store_compacted_revision();
        Ok(HostedEtcdCompactResult {
            revision: self.revision,
            compacted_revision: self.compacted_revision,
        })
    }

    fn watch_events(
        &self,
        key: Vec<u8>,
        range_end: Vec<u8>,
        start_revision: i64,
    ) -> Result<HostedEtcdWatchResult> {
        if start_revision < 0 {
            return Err(LoomError::invalid(
                "etcd watch start revision must not be negative",
            ));
        }
        let start = if start_revision == 0 {
            self.revision.saturating_add(1)
        } else {
            start_revision
        };
        if start <= self.compacted_revision {
            return Err(LoomError::invalid("etcd watch revision has been compacted"));
        }
        let mut events = Vec::new();
        for (event_key, bytes) in self.events.iter() {
            let Some((revision, _)) = decode_etcd_event_key(event_key) else {
                continue;
            };
            if revision < start {
                continue;
            }
            let Some(event) = decode_etcd_event(bytes) else {
                return Err(LoomError::corrupt("invalid etcd watch event record"));
            };
            if etcd_key_in_range(&event.kv.key, &key, &range_end) {
                events.push(event);
            }
        }
        Ok(HostedEtcdWatchResult {
            revision: self.revision,
            compacted_revision: self.compacted_revision,
            events,
        })
    }

    fn put(
        &mut self,
        key: Vec<u8>,
        value: Vec<u8>,
        lease: i64,
        prev_kv: bool,
    ) -> Result<HostedEtcdPutResult> {
        if lease < 0 {
            return Err(LoomError::invalid("etcd lease id must not be negative"));
        }
        if lease > 0 && self.lease(lease).is_none() {
            return Err(LoomError::invalid("etcd lease does not exist"));
        }
        let key_value = Value::Bytes(key.clone());
        let previous_value = self.values.get(&key_value).map(<[u8]>::to_vec);
        let previous_meta = self.key_meta(&key);
        let previous = previous_value
            .as_ref()
            .map(|value| self.kv_response(key.clone(), value.clone()));
        let revision = self.next_revision();
        let meta = match previous_meta {
            Some(mut meta) => {
                self.unlink_lease_key(meta.lease, &key);
                meta.mod_revision = revision;
                meta.version = meta.version.saturating_add(1);
                meta.lease = lease;
                meta
            }
            None => EtcdKeyMeta {
                create_revision: revision,
                mod_revision: revision,
                version: 1,
                lease,
            },
        };
        self.values.put(key_value.clone(), value.clone());
        self.meta.put(key_value, encode_etcd_key_meta(&meta));
        self.link_lease_key(lease, key.clone());
        self.append_event(
            HostedEtcdEventKind::Put,
            HostedEtcdKv {
                key,
                value,
                create_revision: meta.create_revision,
                mod_revision: meta.mod_revision,
                version: meta.version,
                lease: meta.lease,
            },
            previous.clone(),
        );
        self.store_revision();
        Ok(HostedEtcdPutResult {
            revision,
            prev_kv: if prev_kv { previous } else { None },
        })
    }

    fn delete_range(
        &mut self,
        key: Vec<u8>,
        range_end: Vec<u8>,
        prev_kv: bool,
    ) -> HostedEtcdDeleteRangeResult {
        let keys: Vec<Vec<u8>> = self
            .values
            .iter()
            .filter_map(|(stored_key, _)| match stored_key {
                Value::Bytes(bytes) if etcd_key_in_range(bytes, &key, &range_end) => {
                    Some(bytes.clone())
                }
                _ => None,
            })
            .collect();
        let deleted = keys.len() as i64;
        let mut prev_kvs = Vec::new();
        if !keys.is_empty() {
            self.next_revision();
        }
        for key in keys {
            let key_value = Value::Bytes(key.clone());
            if let Some(value) = self.values.get(&key_value).map(<[u8]>::to_vec) {
                let previous = self.kv_response(key.clone(), value);
                if prev_kv {
                    prev_kvs.push(previous.clone());
                }
                if let Some(meta) = self.key_meta(&key) {
                    self.unlink_lease_key(meta.lease, &key);
                }
                self.append_event(
                    HostedEtcdEventKind::Delete,
                    previous.clone(),
                    Some(previous),
                );
                self.values.delete(&key_value);
                self.meta.delete(&key_value);
            }
        }
        self.store_revision();
        HostedEtcdDeleteRangeResult {
            revision: self.revision,
            deleted,
            prev_kvs,
        }
    }

    fn txn(
        &mut self,
        compare: Vec<HostedEtcdCompare>,
        success: Vec<HostedEtcdRequestOp>,
        failure: Vec<HostedEtcdRequestOp>,
    ) -> Result<HostedEtcdTxnResult> {
        let succeeded = compare.iter().all(|cmp| self.compare(cmp));
        let ops = if succeeded { success } else { failure };
        let mut responses = Vec::with_capacity(ops.len());
        for op in ops {
            match op {
                HostedEtcdRequestOp::Range {
                    key,
                    range_end,
                    limit,
                    revision,
                } => responses.push(HostedEtcdResponseOp::Range(
                    self.range(key, range_end, limit, revision)?,
                )),
                HostedEtcdRequestOp::Put {
                    key,
                    value,
                    lease,
                    prev_kv,
                } => responses.push(HostedEtcdResponseOp::Put(
                    self.put(key, value, lease, prev_kv)?,
                )),
                HostedEtcdRequestOp::DeleteRange {
                    key,
                    range_end,
                    prev_kv,
                } => responses.push(HostedEtcdResponseOp::DeleteRange(
                    self.delete_range(key, range_end, prev_kv),
                )),
            }
        }
        Ok(HostedEtcdTxnResult {
            revision: self.revision,
            succeeded,
            responses,
        })
    }

    fn lease_grant(&mut self, requested_id: i64, ttl: i64) -> Result<HostedEtcdLeaseGrantResult> {
        if ttl <= 0 {
            return Err(LoomError::invalid(
                "etcd lease ttl must be greater than zero",
            ));
        }
        if requested_id < 0 {
            return Err(LoomError::invalid("etcd lease id must not be negative"));
        }
        let revision = self.next_revision();
        let mut id = if requested_id == 0 {
            revision
        } else {
            requested_id
        };
        while self.lease(id).is_some() {
            id = id.saturating_add(1);
        }
        let lease = EtcdLease {
            id,
            ttl,
            expires_at_ms: hosted_now_ms().saturating_add((ttl as u64).saturating_mul(1000)),
            keys: Vec::new(),
        };
        self.store_lease(&lease);
        self.store_revision();
        Ok(HostedEtcdLeaseGrantResult { id, ttl, revision })
    }

    fn lease_keep_alive(&mut self, id: i64) -> Result<HostedEtcdLeaseKeepAliveResult> {
        let mut lease = self
            .lease(id)
            .ok_or_else(|| LoomError::not_found("etcd lease not found"))?;
        lease.expires_at_ms =
            hosted_now_ms().saturating_add((lease.ttl as u64).saturating_mul(1000));
        self.store_lease(&lease);
        Ok(HostedEtcdLeaseKeepAliveResult {
            id,
            ttl: lease.ttl,
            revision: self.revision,
        })
    }

    fn lease_revoke(&mut self, id: i64) -> HostedEtcdDeleteRangeResult {
        let Some(lease) = self.lease(id) else {
            return HostedEtcdDeleteRangeResult {
                revision: self.revision,
                deleted: 0,
                prev_kvs: Vec::new(),
            };
        };
        if !lease.keys.is_empty() {
            self.next_revision();
        }
        let mut prev_kvs = Vec::new();
        for key in lease.keys {
            let key_value = Value::Bytes(key.clone());
            if let Some(value) = self.values.get(&key_value).map(<[u8]>::to_vec) {
                let previous = self.kv_response(key, value);
                self.append_event(
                    HostedEtcdEventKind::Delete,
                    previous.clone(),
                    Some(previous.clone()),
                );
                prev_kvs.push(previous);
                self.values.delete(&key_value);
                self.meta.delete(&key_value);
            }
        }
        self.leases.delete(&etcd_lease_key(id));
        self.store_revision();
        HostedEtcdDeleteRangeResult {
            revision: self.revision,
            deleted: prev_kvs.len() as i64,
            prev_kvs,
        }
    }

    fn compare(&self, compare: &HostedEtcdCompare) -> bool {
        let key_value = Value::Bytes(compare.key.clone());
        let value = self.values.get(&key_value).map(<[u8]>::to_vec);
        let meta = self.key_meta(&compare.key);
        let ordering = match &compare.target {
            HostedEtcdCompareTarget::Value(expected) => value
                .unwrap_or_default()
                .as_slice()
                .cmp(expected.as_slice()),
            HostedEtcdCompareTarget::Version(expected) => {
                meta.as_ref().map_or(0, |meta| meta.version).cmp(expected)
            }
            HostedEtcdCompareTarget::CreateRevision(expected) => meta
                .as_ref()
                .map_or(0, |meta| meta.create_revision)
                .cmp(expected),
            HostedEtcdCompareTarget::ModRevision(expected) => meta
                .as_ref()
                .map_or(0, |meta| meta.mod_revision)
                .cmp(expected),
            HostedEtcdCompareTarget::Lease(expected) => {
                meta.as_ref().map_or(0, |meta| meta.lease).cmp(expected)
            }
        };
        match compare.result {
            HostedEtcdCompareResult::Equal => ordering == std::cmp::Ordering::Equal,
            HostedEtcdCompareResult::Greater => ordering == std::cmp::Ordering::Greater,
            HostedEtcdCompareResult::Less => ordering == std::cmp::Ordering::Less,
            HostedEtcdCompareResult::NotEqual => ordering != std::cmp::Ordering::Equal,
        }
    }

    fn key_meta(&self, key: &[u8]) -> Option<EtcdKeyMeta> {
        self.meta
            .get(&Value::Bytes(key.to_vec()))
            .and_then(decode_etcd_key_meta)
    }

    fn lease(&self, id: i64) -> Option<EtcdLease> {
        self.leases
            .get(&etcd_lease_key(id))
            .and_then(decode_etcd_lease)
    }

    fn link_lease_key(&mut self, id: i64, key: Vec<u8>) {
        if id == 0 {
            return;
        }
        if let Some(mut lease) = self.lease(id)
            && !lease.keys.iter().any(|existing| existing == &key)
        {
            lease.keys.push(key);
            lease.keys.sort();
            self.store_lease(&lease);
        }
    }

    fn unlink_lease_key(&mut self, id: i64, key: &[u8]) {
        if id == 0 {
            return;
        }
        if let Some(mut lease) = self.lease(id) {
            lease.keys.retain(|existing| existing.as_slice() != key);
            self.store_lease(&lease);
        }
    }

    fn store_lease(&mut self, lease: &EtcdLease) {
        self.leases
            .put(etcd_lease_key(lease.id), encode_etcd_lease(lease));
    }

    fn append_event(
        &mut self,
        kind: HostedEtcdEventKind,
        kv: HostedEtcdKv,
        prev_kv: Option<HostedEtcdKv>,
    ) {
        let event_index = next_etcd_event_index(&self.events);
        let event = HostedEtcdWatchEvent {
            revision: self.revision,
            kind,
            kv,
            prev_kv,
        };
        self.events.put(
            etcd_event_key(self.revision, event_index),
            encode_etcd_event(&event),
        );
    }

    fn prune_events_through(&mut self, revision: i64) {
        let keys: Vec<Value> = self
            .events
            .iter()
            .filter_map(|(key, _)| {
                decode_etcd_event_key(key)
                    .filter(|(event_revision, _)| *event_revision <= revision)
                    .map(|_| key.clone())
            })
            .collect();
        for key in keys {
            self.events.delete(&key);
        }
    }

    fn expire_leases(&mut self, now_ms: u64) {
        let expired: Vec<i64> = self
            .leases
            .iter()
            .filter_map(|(_, bytes)| decode_etcd_lease(bytes))
            .filter(|lease| lease.expires_at_ms <= now_ms)
            .map(|lease| lease.id)
            .collect();
        for id in expired {
            self.lease_revoke(id);
        }
    }

    fn kv_response(&self, key: Vec<u8>, value: Vec<u8>) -> HostedEtcdKv {
        let meta = self.key_meta(&key).unwrap_or(EtcdKeyMeta {
            create_revision: 0,
            mod_revision: 0,
            version: 0,
            lease: 0,
        });
        HostedEtcdKv {
            key,
            value,
            create_revision: meta.create_revision,
            mod_revision: meta.mod_revision,
            version: meta.version,
            lease: meta.lease,
        }
    }

    fn next_revision(&mut self) -> i64 {
        self.revision = self.revision.saturating_add(1).max(1);
        self.revision
    }

    fn store_revision(&mut self) {
        self.meta
            .put(etcd_revision_key(), self.revision.to_be_bytes().to_vec());
    }

    fn store_compacted_revision(&mut self) {
        self.meta.put(
            etcd_compacted_revision_key(),
            self.compacted_revision.to_be_bytes().to_vec(),
        );
    }
}

fn etcd_meta_collection(collection: &str) -> String {
    format!(".etcd.meta.{}", hex::encode(collection.as_bytes()))
}

fn etcd_lease_collection(collection: &str) -> String {
    format!(".etcd.lease.{}", hex::encode(collection.as_bytes()))
}

fn etcd_event_collection(collection: &str) -> String {
    format!(".etcd.event.{}", hex::encode(collection.as_bytes()))
}

fn etcd_revision_key() -> Value {
    Value::Bytes(b"__loom_etcd_revision".to_vec())
}

fn etcd_compacted_revision_key() -> Value {
    Value::Bytes(b"__loom_etcd_compacted_revision".to_vec())
}

fn etcd_lease_key(id: i64) -> Value {
    Value::Bytes(id.to_be_bytes().to_vec())
}

fn etcd_event_key(revision: i64, index: u64) -> Value {
    let mut key = Vec::with_capacity(16);
    key.extend_from_slice(&revision.to_be_bytes());
    key.extend_from_slice(&index.to_be_bytes());
    Value::Bytes(key)
}

fn decode_etcd_event_key(key: &Value) -> Option<(i64, u64)> {
    let Value::Bytes(bytes) = key else {
        return None;
    };
    if bytes.len() != 16 {
        return None;
    }
    let revision = read_i64(&bytes[0..8])?;
    let mut raw_index = [0u8; 8];
    raw_index.copy_from_slice(&bytes[8..16]);
    Some((revision, u64::from_be_bytes(raw_index)))
}

fn next_etcd_event_index(events: &kv::KvMap) -> u64 {
    events
        .iter()
        .filter_map(|(key, _)| decode_etcd_event_key(key).map(|(_, index)| index))
        .max()
        .unwrap_or(0)
        .saturating_add(1)
}

fn etcd_revision(meta: &kv::KvMap) -> Result<i64> {
    match meta.get(&etcd_revision_key()) {
        Some(bytes) if bytes.len() == 8 => {
            let mut raw = [0u8; 8];
            raw.copy_from_slice(bytes);
            Ok(i64::from_be_bytes(raw).max(0))
        }
        Some(_) => Err(LoomError::corrupt("invalid etcd revision record")),
        None => Ok(0),
    }
}

fn etcd_compacted_revision(meta: &kv::KvMap) -> Result<i64> {
    match meta.get(&etcd_compacted_revision_key()) {
        Some(bytes) if bytes.len() == 8 => {
            let mut raw = [0u8; 8];
            raw.copy_from_slice(bytes);
            Ok(i64::from_be_bytes(raw).max(0))
        }
        Some(_) => Err(LoomError::corrupt("invalid etcd compacted revision record")),
        None => Ok(0),
    }
}

fn etcd_key_in_range(key: &[u8], start: &[u8], end: &[u8]) -> bool {
    if end.is_empty() {
        return key == start;
    }
    if end == [0] {
        return key >= start;
    }
    key >= start && key < end
}

const ETCD_KEY_META_MAGIC: &[u8] = b"LOOMETCDKMETA1";
const ETCD_LEASE_MAGIC: &[u8] = b"LOOMETCDLEASE1";
const ETCD_EVENT_MAGIC: &[u8] = b"LOOMETCDEVENT1";

fn encode_etcd_key_meta(meta: &EtcdKeyMeta) -> Vec<u8> {
    let mut out = Vec::with_capacity(ETCD_KEY_META_MAGIC.len() + 32);
    out.extend_from_slice(ETCD_KEY_META_MAGIC);
    out.extend_from_slice(&meta.create_revision.to_be_bytes());
    out.extend_from_slice(&meta.mod_revision.to_be_bytes());
    out.extend_from_slice(&meta.version.to_be_bytes());
    out.extend_from_slice(&meta.lease.to_be_bytes());
    out
}

fn decode_etcd_key_meta(bytes: &[u8]) -> Option<EtcdKeyMeta> {
    let body = bytes.strip_prefix(ETCD_KEY_META_MAGIC)?;
    if body.len() != 32 {
        return None;
    }
    Some(EtcdKeyMeta {
        create_revision: read_i64(&body[0..8])?,
        mod_revision: read_i64(&body[8..16])?,
        version: read_i64(&body[16..24])?,
        lease: read_i64(&body[24..32])?,
    })
}

fn encode_etcd_lease(lease: &EtcdLease) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(ETCD_LEASE_MAGIC);
    out.extend_from_slice(&lease.id.to_be_bytes());
    out.extend_from_slice(&lease.ttl.to_be_bytes());
    out.extend_from_slice(&lease.expires_at_ms.to_be_bytes());
    out.extend_from_slice(&(lease.keys.len() as u32).to_be_bytes());
    for key in &lease.keys {
        out.extend_from_slice(&(key.len() as u32).to_be_bytes());
        out.extend_from_slice(key);
    }
    out
}

fn decode_etcd_lease(bytes: &[u8]) -> Option<EtcdLease> {
    let mut body = bytes.strip_prefix(ETCD_LEASE_MAGIC)?;
    if body.len() < 28 {
        return None;
    }
    let id = take_i64(&mut body)?;
    let ttl = take_i64(&mut body)?;
    let expires_at_ms = take_u64(&mut body)?;
    let count = take_u32(&mut body)? as usize;
    let mut keys = Vec::with_capacity(count);
    for _ in 0..count {
        let len = take_u32(&mut body)? as usize;
        if body.len() < len {
            return None;
        }
        keys.push(body[..len].to_vec());
        body = &body[len..];
    }
    if !body.is_empty() {
        return None;
    }
    Some(EtcdLease {
        id,
        ttl,
        expires_at_ms,
        keys,
    })
}

fn encode_etcd_event(event: &HostedEtcdWatchEvent) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(ETCD_EVENT_MAGIC);
    out.push(match event.kind {
        HostedEtcdEventKind::Put => 0,
        HostedEtcdEventKind::Delete => 1,
    });
    out.extend_from_slice(&event.revision.to_be_bytes());
    encode_etcd_event_kv(&event.kv, &mut out);
    match &event.prev_kv {
        Some(prev) => {
            out.push(1);
            encode_etcd_event_kv(prev, &mut out);
        }
        None => out.push(0),
    }
    out
}

fn decode_etcd_event(bytes: &[u8]) -> Option<HostedEtcdWatchEvent> {
    let mut body = bytes.strip_prefix(ETCD_EVENT_MAGIC)?;
    let kind = match *body.first()? {
        0 => HostedEtcdEventKind::Put,
        1 => HostedEtcdEventKind::Delete,
        _ => return None,
    };
    body = &body[1..];
    let revision = take_i64(&mut body)?;
    let kv = decode_etcd_event_kv(&mut body)?;
    let prev_present = *body.first()?;
    body = &body[1..];
    let prev_kv = match prev_present {
        0 => None,
        1 => Some(decode_etcd_event_kv(&mut body)?),
        _ => return None,
    };
    if !body.is_empty() {
        return None;
    }
    Some(HostedEtcdWatchEvent {
        revision,
        kind,
        kv,
        prev_kv,
    })
}

fn encode_etcd_event_kv(kv: &HostedEtcdKv, out: &mut Vec<u8>) {
    out.extend_from_slice(&(kv.key.len() as u32).to_be_bytes());
    out.extend_from_slice(&kv.key);
    out.extend_from_slice(&(kv.value.len() as u32).to_be_bytes());
    out.extend_from_slice(&kv.value);
    out.extend_from_slice(&kv.create_revision.to_be_bytes());
    out.extend_from_slice(&kv.mod_revision.to_be_bytes());
    out.extend_from_slice(&kv.version.to_be_bytes());
    out.extend_from_slice(&kv.lease.to_be_bytes());
}

fn decode_etcd_event_kv(bytes: &mut &[u8]) -> Option<HostedEtcdKv> {
    let key_len = take_u32(bytes)? as usize;
    if bytes.len() < key_len {
        return None;
    }
    let key = bytes[..key_len].to_vec();
    *bytes = &bytes[key_len..];
    let value_len = take_u32(bytes)? as usize;
    if bytes.len() < value_len {
        return None;
    }
    let value = bytes[..value_len].to_vec();
    *bytes = &bytes[value_len..];
    Some(HostedEtcdKv {
        key,
        value,
        create_revision: take_i64(bytes)?,
        mod_revision: take_i64(bytes)?,
        version: take_i64(bytes)?,
        lease: take_i64(bytes)?,
    })
}

fn take_i64(bytes: &mut &[u8]) -> Option<i64> {
    if bytes.len() < 8 {
        return None;
    }
    let value = read_i64(&bytes[..8])?;
    *bytes = &bytes[8..];
    Some(value)
}

fn take_u64(bytes: &mut &[u8]) -> Option<u64> {
    if bytes.len() < 8 {
        return None;
    }
    let mut raw = [0u8; 8];
    raw.copy_from_slice(&bytes[..8]);
    *bytes = &bytes[8..];
    Some(u64::from_be_bytes(raw))
}

fn take_u32(bytes: &mut &[u8]) -> Option<u32> {
    if bytes.len() < 4 {
        return None;
    }
    let mut raw = [0u8; 4];
    raw.copy_from_slice(&bytes[..4]);
    *bytes = &bytes[4..];
    Some(u32::from_be_bytes(raw))
}

fn read_i64(bytes: &[u8]) -> Option<i64> {
    if bytes.len() != 8 {
        return None;
    }
    let mut raw = [0u8; 8];
    raw.copy_from_slice(bytes);
    Some(i64::from_be_bytes(raw))
}

fn resolve_facet_ns(loom: &Loom<FileStore>, facet: FacetKind, name: &str) -> Result<WorkspaceId> {
    loom.registry().open(&WsSelector::Typed {
        ty: facet,
        name: name.to_string(),
    })
}

fn ensure_facet_ns(
    loom: &mut Loom<FileStore>,
    facet: FacetKind,
    name: &str,
) -> Result<WorkspaceId> {
    match resolve_facet_ns(loom, facet, name) {
        Ok(ns) => Ok(ns),
        Err(err) if err.code == Code::NotFound => {
            loom.authorize_global_admin()?;
            loom.registry_mut().ensure_for_write(
                &WsSelector::Typed {
                    ty: facet,
                    name: name.to_string(),
                },
                fresh_workspace_id(),
            )
        }
        Err(err) => Err(err),
    }
}

const KAFKA_TOPIC_METADATA_MAGIC: &[u8] = b"LOOMKAFKATOPIC1";
const KAFKA_PRODUCER_MAGIC: &[u8] = b"LOOMKAFKAPRODUCER1";
const KAFKA_PRODUCER_SEQUENCE_MAGIC: &[u8] = b"LOOMKAFKAPRODSEQ1";
const KAFKA_TRANSACTION_MAGIC: &[u8] = b"LOOMKAFKATXN1";

impl HostedKafkaTopicMetadata {
    fn new(
        workspace: &str,
        topic: &str,
        partition_count: i32,
        created_at_ms: u64,
        metadata_version: u64,
    ) -> Result<Self> {
        let mut partitions = Vec::new();
        for partition in 0..partition_count {
            partitions.push(HostedKafkaPartitionMetadata {
                partition,
                next_offset: 0,
                leader_id: 0,
                leader_epoch: 1,
                high_watermark: 0,
            });
        }
        Ok(Self {
            topic: topic.to_string(),
            topic_id: kafka_topic_id(workspace, topic),
            metadata_version,
            created_at_ms,
            partitions,
        })
    }

    fn encode(&self) -> Result<Vec<u8>> {
        let topic = self.topic.as_bytes();
        let topic_len = u16::try_from(topic.len())
            .map_err(|_| LoomError::invalid("Kafka topic name is too long"))?;
        let partition_count = u32::try_from(self.partitions.len())
            .map_err(|_| LoomError::invalid("Kafka topic partition count is too large"))?;
        let mut out = Vec::with_capacity(
            KAFKA_TOPIC_METADATA_MAGIC.len()
                + 2
                + topic.len()
                + 16
                + 8
                + 8
                + 4
                + self.partitions.len() * 32,
        );
        out.extend_from_slice(KAFKA_TOPIC_METADATA_MAGIC);
        out.extend_from_slice(&topic_len.to_be_bytes());
        out.extend_from_slice(topic);
        out.extend_from_slice(&self.topic_id);
        out.extend_from_slice(&self.metadata_version.to_be_bytes());
        out.extend_from_slice(&self.created_at_ms.to_be_bytes());
        out.extend_from_slice(&partition_count.to_be_bytes());
        for partition in &self.partitions {
            out.extend_from_slice(&partition.partition.to_be_bytes());
            out.extend_from_slice(&partition.next_offset.to_be_bytes());
            out.extend_from_slice(&partition.leader_id.to_be_bytes());
            out.extend_from_slice(&partition.leader_epoch.to_be_bytes());
            out.extend_from_slice(&partition.high_watermark.to_be_bytes());
        }
        Ok(out)
    }

    fn decode(bytes: &[u8]) -> Result<Self> {
        let mut cursor = KafkaMetadataCursor::new(bytes);
        cursor.take_magic()?;
        let topic_len = usize::from(cursor.u16()?);
        let topic = std::str::from_utf8(cursor.take(topic_len)?)
            .map_err(|_| LoomError::invalid("Kafka topic metadata has invalid UTF-8 topic"))?
            .to_string();
        let topic_id = cursor.array_16()?;
        let metadata_version = cursor.u64()?;
        let created_at_ms = cursor.u64()?;
        let partition_count = usize::try_from(cursor.u32()?)
            .map_err(|_| LoomError::invalid("Kafka topic metadata partition count is too large"))?;
        let mut partitions = Vec::with_capacity(partition_count);
        for _ in 0..partition_count {
            partitions.push(HostedKafkaPartitionMetadata {
                partition: cursor.i32()?,
                next_offset: cursor.u64()?,
                leader_id: cursor.i32()?,
                leader_epoch: cursor.u64()?,
                high_watermark: cursor.u64()?,
            });
        }
        cursor.finish()?;
        Ok(Self {
            topic,
            topic_id,
            metadata_version,
            created_at_ms,
            partitions,
        })
    }
}

impl HostedKafkaProducerState {
    fn encode(&self) -> Result<Vec<u8>> {
        let mut out = Vec::with_capacity(KAFKA_PRODUCER_MAGIC.len() + 8 + 2 + 1 + 2 + 249);
        out.extend_from_slice(KAFKA_PRODUCER_MAGIC);
        out.extend_from_slice(&self.producer_id.to_be_bytes());
        out.extend_from_slice(&self.producer_epoch.to_be_bytes());
        match &self.transactional_id {
            Some(transactional_id) => {
                let len = u16::try_from(transactional_id.len())
                    .map_err(|_| LoomError::invalid("Kafka transactional id is too long"))?;
                out.push(1);
                out.extend_from_slice(&len.to_be_bytes());
                out.extend_from_slice(transactional_id.as_bytes());
            }
            None => {
                out.push(0);
                out.extend_from_slice(&0_u16.to_be_bytes());
            }
        }
        Ok(out)
    }

    fn decode(bytes: &[u8]) -> Result<Self> {
        let mut cursor = KafkaMetadataCursor::new(bytes);
        cursor.take_exact_magic(KAFKA_PRODUCER_MAGIC)?;
        let producer_id = cursor.i64()?;
        let producer_epoch = cursor.i16()?;
        let has_transactional_id = cursor.u8()?;
        let transactional_id_len = usize::from(cursor.u16()?);
        let transactional_id = match has_transactional_id {
            0 if transactional_id_len == 0 => None,
            1 => Some(
                std::str::from_utf8(cursor.take(transactional_id_len)?)
                    .map_err(|_| LoomError::invalid("Kafka producer has invalid UTF-8 id"))?
                    .to_string(),
            ),
            _ => return Err(LoomError::invalid("Kafka producer has invalid id marker")),
        };
        cursor.finish()?;
        Ok(Self {
            producer_id,
            producer_epoch,
            transactional_id,
        })
    }
}

impl HostedKafkaProducerSequenceState {
    fn encode(&self) -> Vec<u8> {
        let mut out =
            Vec::with_capacity(KAFKA_PRODUCER_SEQUENCE_MAGIC.len() + 8 + 2 + 4 + 4 + 4 + 8 + 8);
        out.extend_from_slice(KAFKA_PRODUCER_SEQUENCE_MAGIC);
        out.extend_from_slice(&self.producer_id.to_be_bytes());
        out.extend_from_slice(&self.producer_epoch.to_be_bytes());
        out.extend_from_slice(&self.next_sequence.to_be_bytes());
        out.extend_from_slice(&self.last_base_sequence.to_be_bytes());
        out.extend_from_slice(&self.last_record_count.to_be_bytes());
        out.extend_from_slice(&self.last_base_offset.to_be_bytes());
        out.extend_from_slice(&self.last_high_watermark.to_be_bytes());
        out
    }

    fn decode(bytes: &[u8]) -> Result<Self> {
        let mut cursor = KafkaMetadataCursor::new(bytes);
        cursor.take_exact_magic(KAFKA_PRODUCER_SEQUENCE_MAGIC)?;
        let producer_id = cursor.i64()?;
        let producer_epoch = cursor.i16()?;
        let next_sequence = cursor.i32()?;
        let last_base_sequence = cursor.i32()?;
        let last_record_count = cursor.u32()?;
        let last_base_offset = cursor.u64()?;
        let last_high_watermark = cursor.u64()?;
        cursor.finish()?;
        Ok(Self {
            producer_id,
            producer_epoch,
            next_sequence,
            last_base_sequence,
            last_record_count,
            last_base_offset,
            last_high_watermark,
        })
    }
}

impl HostedKafkaTransactionStatus {
    fn as_u8(&self) -> u8 {
        match self {
            Self::Active => 1,
            Self::Committed => 2,
            Self::Aborted => 3,
        }
    }

    fn from_u8(value: u8) -> Result<Self> {
        match value {
            1 => Ok(Self::Active),
            2 => Ok(Self::Committed),
            3 => Ok(Self::Aborted),
            _ => Err(LoomError::invalid("Kafka transaction has invalid status")),
        }
    }
}

impl HostedKafkaTransactionState {
    fn encode(&self) -> Result<Vec<u8>> {
        let transactional_id_len = u16::try_from(self.transactional_id.len())
            .map_err(|_| LoomError::invalid("Kafka transactional id is too long"))?;
        let topic_count = u32::try_from(self.topics.len())
            .map_err(|_| LoomError::invalid("Kafka transaction topic count is too large"))?;
        let group_count = u32::try_from(self.groups.len())
            .map_err(|_| LoomError::invalid("Kafka transaction group count is too large"))?;
        let pending_count = u32::try_from(self.pending_offset_commits.len()).map_err(|_| {
            LoomError::invalid("Kafka transaction pending offset count is too large")
        })?;
        let produced_count = u32::try_from(self.pending_produced_ranges.len()).map_err(|_| {
            LoomError::invalid("Kafka transaction produced range count is too large")
        })?;
        let mut out = Vec::new();
        out.extend_from_slice(KAFKA_TRANSACTION_MAGIC);
        out.extend_from_slice(&transactional_id_len.to_be_bytes());
        out.extend_from_slice(self.transactional_id.as_bytes());
        out.extend_from_slice(&self.producer_id.to_be_bytes());
        out.extend_from_slice(&self.producer_epoch.to_be_bytes());
        out.extend_from_slice(&self.transaction_epoch.to_be_bytes());
        out.push(self.status.as_u8());
        out.extend_from_slice(&topic_count.to_be_bytes());
        for topic in &self.topics {
            let topic_len = u16::try_from(topic.topic.len())
                .map_err(|_| LoomError::invalid("Kafka topic name is too long"))?;
            let partition_count = u32::try_from(topic.partitions.len()).map_err(|_| {
                LoomError::invalid("Kafka transaction partition count is too large")
            })?;
            out.extend_from_slice(&topic_len.to_be_bytes());
            out.extend_from_slice(topic.topic.as_bytes());
            out.extend_from_slice(&partition_count.to_be_bytes());
            for partition in &topic.partitions {
                out.extend_from_slice(&partition.to_be_bytes());
            }
        }
        out.extend_from_slice(&group_count.to_be_bytes());
        for group in &self.groups {
            let group_len = u16::try_from(group.len())
                .map_err(|_| LoomError::invalid("Kafka group id is too long"))?;
            out.extend_from_slice(&group_len.to_be_bytes());
            out.extend_from_slice(group.as_bytes());
        }
        out.extend_from_slice(&pending_count.to_be_bytes());
        for pending in &self.pending_offset_commits {
            let group_len = u16::try_from(pending.group_id.len())
                .map_err(|_| LoomError::invalid("Kafka group id is too long"))?;
            let topic_len = u16::try_from(pending.topic.len())
                .map_err(|_| LoomError::invalid("Kafka topic name is too long"))?;
            out.extend_from_slice(&group_len.to_be_bytes());
            out.extend_from_slice(pending.group_id.as_bytes());
            out.extend_from_slice(&topic_len.to_be_bytes());
            out.extend_from_slice(pending.topic.as_bytes());
            out.extend_from_slice(&pending.partition.to_be_bytes());
            out.extend_from_slice(&pending.offset.to_be_bytes());
        }
        out.extend_from_slice(&produced_count.to_be_bytes());
        for range in &self.pending_produced_ranges {
            let topic_len = u16::try_from(range.topic.len())
                .map_err(|_| LoomError::invalid("Kafka topic name is too long"))?;
            out.extend_from_slice(&topic_len.to_be_bytes());
            out.extend_from_slice(range.topic.as_bytes());
            out.extend_from_slice(&range.partition.to_be_bytes());
            out.extend_from_slice(&range.first_offset.to_be_bytes());
            out.extend_from_slice(&range.record_count.to_be_bytes());
        }
        Ok(out)
    }

    fn decode(bytes: &[u8]) -> Result<Self> {
        let mut cursor = KafkaMetadataCursor::new(bytes);
        cursor.take_exact_magic(KAFKA_TRANSACTION_MAGIC)?;
        let transactional_id_len = usize::from(cursor.u16()?);
        let transactional_id = std::str::from_utf8(cursor.take(transactional_id_len)?)
            .map_err(|_| LoomError::invalid("Kafka transaction has invalid UTF-8 id"))?
            .to_string();
        let producer_id = cursor.i64()?;
        let producer_epoch = cursor.i16()?;
        let transaction_epoch = cursor.u64()?;
        let status = HostedKafkaTransactionStatus::from_u8(cursor.u8()?)?;
        let topic_count = usize::try_from(cursor.u32()?)
            .map_err(|_| LoomError::invalid("Kafka transaction topic count is too large"))?;
        let mut topics = Vec::with_capacity(topic_count);
        for _ in 0..topic_count {
            let topic_len = usize::from(cursor.u16()?);
            let topic = std::str::from_utf8(cursor.take(topic_len)?)
                .map_err(|_| LoomError::invalid("Kafka transaction has invalid UTF-8 topic"))?
                .to_string();
            let partition_count = usize::try_from(cursor.u32()?).map_err(|_| {
                LoomError::invalid("Kafka transaction partition count is too large")
            })?;
            let mut partitions = Vec::with_capacity(partition_count);
            for _ in 0..partition_count {
                partitions.push(cursor.i32()?);
            }
            topics.push(HostedKafkaTransactionTopic { topic, partitions });
        }
        let group_count = usize::try_from(cursor.u32()?)
            .map_err(|_| LoomError::invalid("Kafka transaction group count is too large"))?;
        let mut groups = Vec::with_capacity(group_count);
        for _ in 0..group_count {
            let group_len = usize::from(cursor.u16()?);
            groups.push(
                std::str::from_utf8(cursor.take(group_len)?)
                    .map_err(|_| LoomError::invalid("Kafka transaction has invalid UTF-8 group"))?
                    .to_string(),
            );
        }
        let pending_count = usize::try_from(cursor.u32()?).map_err(|_| {
            LoomError::invalid("Kafka transaction pending offset count is too large")
        })?;
        let mut pending_offset_commits = Vec::with_capacity(pending_count);
        for _ in 0..pending_count {
            let group_len = usize::from(cursor.u16()?);
            let group_id = std::str::from_utf8(cursor.take(group_len)?)
                .map_err(|_| LoomError::invalid("Kafka transaction has invalid UTF-8 group"))?
                .to_string();
            let topic_len = usize::from(cursor.u16()?);
            let topic = std::str::from_utf8(cursor.take(topic_len)?)
                .map_err(|_| LoomError::invalid("Kafka transaction has invalid UTF-8 topic"))?
                .to_string();
            pending_offset_commits.push(HostedKafkaPendingOffsetCommit {
                group_id,
                topic,
                partition: cursor.i32()?,
                offset: cursor.u64()?,
            });
        }
        let mut pending_produced_ranges = Vec::new();
        if !cursor.is_finished() {
            let produced_count = usize::try_from(cursor.u32()?).map_err(|_| {
                LoomError::invalid("Kafka transaction produced range count is too large")
            })?;
            pending_produced_ranges = Vec::with_capacity(produced_count);
            for _ in 0..produced_count {
                let topic_len = usize::from(cursor.u16()?);
                let topic = std::str::from_utf8(cursor.take(topic_len)?)
                    .map_err(|_| LoomError::invalid("Kafka transaction has invalid UTF-8 topic"))?
                    .to_string();
                pending_produced_ranges.push(HostedKafkaPendingProducedRange {
                    topic,
                    partition: cursor.i32()?,
                    first_offset: cursor.u64()?,
                    record_count: cursor.u32()?,
                });
            }
        }
        cursor.finish()?;
        Ok(Self {
            transactional_id,
            producer_id,
            producer_epoch,
            transaction_epoch,
            status,
            topics,
            groups,
            pending_offset_commits,
            pending_produced_ranges,
        })
    }
}

struct KafkaMetadataCursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> KafkaMetadataCursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    fn take_magic(&mut self) -> Result<()> {
        self.take_exact_magic(KAFKA_TOPIC_METADATA_MAGIC)
    }

    fn take_exact_magic(&mut self, magic: &[u8]) -> Result<()> {
        if self.take(magic.len())? != magic {
            return Err(LoomError::invalid("Kafka topic metadata has invalid magic"));
        }
        Ok(())
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8]> {
        let end = self
            .pos
            .checked_add(len)
            .ok_or_else(|| LoomError::invalid("Kafka topic metadata cursor overflow"))?;
        let Some(bytes) = self.bytes.get(self.pos..end) else {
            return Err(LoomError::invalid("Kafka topic metadata is truncated"));
        };
        self.pos = end;
        Ok(bytes)
    }

    fn u16(&mut self) -> Result<u16> {
        let bytes = self.take(2)?;
        Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32> {
        let bytes = self.take(4)?;
        Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn i16(&mut self) -> Result<i16> {
        let bytes = self.take(2)?;
        Ok(i16::from_be_bytes([bytes[0], bytes[1]]))
    }

    fn i32(&mut self) -> Result<i32> {
        let bytes = self.take(4)?;
        Ok(i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn i64(&mut self) -> Result<i64> {
        let bytes = self.take(8)?;
        Ok(i64::from_be_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    fn u64(&mut self) -> Result<u64> {
        let bytes = self.take(8)?;
        Ok(u64::from_be_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    fn array_16(&mut self) -> Result<[u8; 16]> {
        let mut out = [0u8; 16];
        out.copy_from_slice(self.take(16)?);
        Ok(out)
    }

    fn finish(&self) -> Result<()> {
        if self.pos != self.bytes.len() {
            return Err(LoomError::invalid(
                "Kafka topic metadata has trailing bytes",
            ));
        }
        Ok(())
    }

    fn is_finished(&self) -> bool {
        self.pos == self.bytes.len()
    }
}

fn read_kafka_topic_metadata(
    loom: &Loom<FileStore>,
    ns: WorkspaceId,
    topic: &str,
) -> Result<HostedKafkaTopicMetadata> {
    HostedKafkaTopicMetadata::decode(&loom.read_file(ns, &kafka_topic_metadata_path(topic))?)
}

fn write_kafka_topic_metadata(
    loom: &mut Loom<FileStore>,
    ns: WorkspaceId,
    metadata: &HostedKafkaTopicMetadata,
) -> Result<()> {
    loom.create_directory_reserved(ns, &kafka_topic_metadata_dir(), true)?;
    loom.write_file_reserved(
        ns,
        &kafka_topic_metadata_path(&metadata.topic),
        &metadata.encode()?,
        0o100644,
    )
}

fn allocate_kafka_metadata_version(
    loom: &mut Loom<FileStore>,
    ns: WorkspaceId,
    workspace: &str,
) -> Result<u64> {
    let scope = CoordinationScope::new([
        "kafka".to_string(),
        workspace.to_string(),
        "metadata".to_string(),
    ])?;
    allocate_coordination_metadata_version(loom, ns, &scope)
}

fn allocate_coordination_metadata_version(
    loom: &mut Loom<FileStore>,
    ns: WorkspaceId,
    scope: &CoordinationScope,
) -> Result<u64> {
    let path = coordination_metadata_version_path(scope);
    let next = match loom.read_file(ns, &path) {
        Ok(bytes) => decode_coordination_sequence_high_water(&bytes)?,
        Err(err) if err.code == Code::NotFound => 1,
        Err(err) => return Err(err),
    };
    let following = next
        .checked_add(1)
        .ok_or_else(|| LoomError::invalid("coordination metadata version overflows"))?;
    loom.create_directory_reserved(ns, &coordination_metadata_version_dir(scope), true)?;
    loom.write_file_reserved(ns, &path, &following.to_be_bytes(), 0o100644)?;
    Ok(next)
}

fn decode_coordination_sequence_high_water(bytes: &[u8]) -> Result<u64> {
    if bytes.len() != 8 {
        return Err(LoomError::invalid(
            "coordination metadata version high-water is corrupt",
        ));
    }
    Ok(u64::from_be_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ]))
}

fn allocate_kafka_producer_id(loom: &mut Loom<FileStore>, ns: WorkspaceId) -> Result<i64> {
    let next = match loom.read_file(ns, &kafka_producer_next_id_path()) {
        Ok(bytes) => {
            if bytes.len() != 8 {
                return Err(LoomError::invalid("Kafka producer id counter is corrupt"));
            }
            i64::from_be_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ])
        }
        Err(err) if err.code == Code::NotFound => 1,
        Err(err) => return Err(err),
    };
    if next < 1 {
        return Err(LoomError::invalid("Kafka producer id counter is invalid"));
    }
    let following = next
        .checked_add(1)
        .ok_or_else(|| LoomError::invalid("Kafka producer id counter overflows"))?;
    loom.create_directory_reserved(ns, &kafka_producer_dir(), true)?;
    loom.write_file_reserved(
        ns,
        &kafka_producer_next_id_path(),
        &following.to_be_bytes(),
        0o100644,
    )?;
    Ok(next)
}

fn read_kafka_producer_state_by_transactional_id(
    loom: &Loom<FileStore>,
    ns: WorkspaceId,
    transactional_id: &str,
) -> Result<Option<HostedKafkaProducerState>> {
    match loom.read_file(ns, &kafka_transactional_producer_path(transactional_id)) {
        Ok(bytes) => HostedKafkaProducerState::decode(&bytes).map(Some),
        Err(err) if err.code == Code::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

fn read_kafka_producer_state_by_id(
    loom: &Loom<FileStore>,
    ns: WorkspaceId,
    producer_id: i64,
) -> Result<Option<HostedKafkaProducerState>> {
    match loom.read_file(ns, &kafka_producer_id_path(producer_id)) {
        Ok(bytes) => HostedKafkaProducerState::decode(&bytes).map(Some),
        Err(err) if err.code == Code::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

fn write_kafka_producer_state(
    loom: &mut Loom<FileStore>,
    ns: WorkspaceId,
    state: &HostedKafkaProducerState,
) -> Result<()> {
    loom.create_directory_reserved(ns, &kafka_producer_id_dir(), true)?;
    loom.write_file_reserved(
        ns,
        &kafka_producer_id_path(state.producer_id),
        &state.encode()?,
        0o100644,
    )?;
    if let Some(transactional_id) = &state.transactional_id {
        loom.create_directory_reserved(ns, &kafka_transactional_producer_dir(), true)?;
        loom.write_file_reserved(
            ns,
            &kafka_transactional_producer_path(transactional_id),
            &state.encode()?,
            0o100644,
        )?;
    }
    Ok(())
}

fn validate_kafka_producer_append(
    loom: &Loom<FileStore>,
    ns: WorkspaceId,
    topic: &str,
    partition: i32,
    append: &HostedKafkaProducerAppend,
) -> Result<Option<HostedKafkaProduceResult>> {
    if append.record_count == 0 {
        return Err(LoomError::invalid("Kafka producer append is empty"));
    }
    let Some(producer) = read_kafka_producer_state_by_id(loom, ns, append.producer_id)? else {
        return Err(LoomError::new(
            Code::FencingStale,
            "Kafka producer id is unknown",
        ));
    };
    if producer.producer_epoch != append.producer_epoch {
        return Err(LoomError::new(
            Code::FencingStale,
            "Kafka producer epoch is stale",
        ));
    }
    let state = read_kafka_producer_sequence_state(loom, ns, topic, partition, append.producer_id)?;
    match state {
        Some(state) if state.producer_epoch == append.producer_epoch => {
            if append.first_sequence == state.next_sequence {
                Ok(None)
            } else if append.first_sequence == state.last_base_sequence
                && append.record_count == state.last_record_count
            {
                Ok(Some(HostedKafkaProduceResult {
                    base_offset: state.last_base_offset,
                    high_watermark: state.last_high_watermark,
                }))
            } else {
                Err(LoomError::new(
                    Code::Conflict,
                    "Kafka producer sequence is out of order",
                ))
            }
        }
        Some(_) | None => {
            if append.first_sequence == 0 {
                Ok(None)
            } else {
                Err(LoomError::new(
                    Code::Conflict,
                    "Kafka producer sequence is out of order",
                ))
            }
        }
    }
}

fn read_kafka_producer_sequence_state(
    loom: &Loom<FileStore>,
    ns: WorkspaceId,
    topic: &str,
    partition: i32,
    producer_id: i64,
) -> Result<Option<HostedKafkaProducerSequenceState>> {
    match loom.read_file(
        ns,
        &kafka_producer_sequence_path(topic, partition, producer_id),
    ) {
        Ok(bytes) => HostedKafkaProducerSequenceState::decode(&bytes).map(Some),
        Err(err) if err.code == Code::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

fn write_kafka_producer_sequence_state(
    loom: &mut Loom<FileStore>,
    ns: WorkspaceId,
    topic: &str,
    partition: i32,
    state: &HostedKafkaProducerSequenceState,
) -> Result<()> {
    loom.create_directory_reserved(
        ns,
        &kafka_producer_sequence_dir(topic, partition, state.producer_id),
        true,
    )?;
    loom.write_file_reserved(
        ns,
        &kafka_producer_sequence_path(topic, partition, state.producer_id),
        &state.encode(),
        0o100644,
    )
}

fn validate_kafka_producer_epoch(
    loom: &Loom<FileStore>,
    ns: WorkspaceId,
    transactional_id: &str,
    producer_id: i64,
    producer_epoch: i16,
) -> Result<()> {
    let Some(state) = read_kafka_producer_state_by_transactional_id(loom, ns, transactional_id)?
    else {
        return Err(LoomError::new(
            Code::FencingStale,
            "Kafka transactional id is unknown",
        ));
    };
    if state.producer_id == producer_id && state.producer_epoch == producer_epoch {
        Ok(())
    } else {
        Err(LoomError::new(
            Code::FencingStale,
            "Kafka producer epoch is stale",
        ))
    }
}

fn append_kafka_records(
    loom: &mut Loom<FileStore>,
    ns: WorkspaceId,
    workspace: &str,
    topic: &str,
    partition: i32,
    expected_base_offset: u64,
    record_batches: &[Vec<u8>],
    producer_append: Option<&HostedKafkaProducerAppend>,
) -> Result<HostedKafkaProduceResult> {
    if record_batches.is_empty() {
        return Err(LoomError::invalid("Kafka produce records are empty"));
    }
    let mut metadata = read_kafka_topic_metadata(loom, ns, topic)?;
    ensure_kafka_partition(&metadata, partition)?;
    if let Some(producer_append) = producer_append
        && let Some(result) =
            validate_kafka_producer_append(loom, ns, topic, partition, producer_append)?
    {
        return Ok(result);
    }
    let current_offset = log::len(loom, ns, topic)? as u64;
    if current_offset != expected_base_offset {
        return Err(LoomError::invalid(
            "Kafka produce offset changed before append",
        ));
    }
    let base_offset = log::append(loom, ns, topic, &record_batches[0])? as u64;
    for record_batch in &record_batches[1..] {
        log::append(loom, ns, topic, record_batch)?;
    }
    let high_watermark = base_offset
        .checked_add(
            u64::try_from(record_batches.len())
                .map_err(|_| LoomError::invalid("Kafka record count overflows"))?,
        )
        .ok_or_else(|| LoomError::invalid("Kafka offset overflows"))?;
    for entry in &mut metadata.partitions {
        if entry.partition == partition {
            entry.next_offset = high_watermark;
            entry.high_watermark = high_watermark;
        }
    }
    metadata.metadata_version = allocate_kafka_metadata_version(loom, ns, workspace)?;
    write_kafka_topic_metadata(loom, ns, &metadata)?;
    if let Some(producer_append) = producer_append {
        write_kafka_producer_sequence_state(
            loom,
            ns,
            topic,
            partition,
            &HostedKafkaProducerSequenceState {
                producer_id: producer_append.producer_id,
                producer_epoch: producer_append.producer_epoch,
                next_sequence: producer_append
                    .first_sequence
                    .checked_add(i32::try_from(producer_append.record_count).map_err(|_| {
                        LoomError::invalid("Kafka producer record count is too large")
                    })?)
                    .ok_or_else(|| LoomError::invalid("Kafka producer sequence overflows"))?,
                last_base_sequence: producer_append.first_sequence,
                last_record_count: producer_append.record_count,
                last_base_offset: base_offset,
                last_high_watermark: high_watermark,
            },
        )?;
    }
    Ok(HostedKafkaProduceResult {
        base_offset,
        high_watermark,
    })
}

fn active_kafka_transaction(
    loom: &Loom<FileStore>,
    ns: WorkspaceId,
    transactional_id: &str,
    producer_id: i64,
    producer_epoch: i16,
) -> Result<HostedKafkaTransactionState> {
    match read_kafka_transaction_state(loom, ns, transactional_id)? {
        Some(transaction)
            if transaction.status == HostedKafkaTransactionStatus::Active
                && transaction.producer_id == producer_id
                && transaction.producer_epoch == producer_epoch =>
        {
            Ok(transaction)
        }
        Some(transaction)
            if transaction.status == HostedKafkaTransactionStatus::Active
                && transaction.producer_id == producer_id =>
        {
            Err(LoomError::new(
                Code::FencingStale,
                "Kafka producer epoch is stale",
            ))
        }
        Some(transaction) => {
            let transaction_epoch = transaction
                .transaction_epoch
                .checked_add(1)
                .ok_or_else(|| LoomError::invalid("Kafka transaction epoch overflows"))?;
            Ok(HostedKafkaTransactionState {
                transactional_id: transactional_id.to_string(),
                producer_id,
                producer_epoch,
                transaction_epoch,
                status: HostedKafkaTransactionStatus::Active,
                topics: Vec::new(),
                groups: Vec::new(),
                pending_offset_commits: Vec::new(),
                pending_produced_ranges: Vec::new(),
            })
        }
        None => Ok(HostedKafkaTransactionState {
            transactional_id: transactional_id.to_string(),
            producer_id,
            producer_epoch,
            transaction_epoch: 1,
            status: HostedKafkaTransactionStatus::Active,
            topics: Vec::new(),
            groups: Vec::new(),
            pending_offset_commits: Vec::new(),
            pending_produced_ranges: Vec::new(),
        }),
    }
}

fn read_kafka_transaction_state(
    loom: &Loom<FileStore>,
    ns: WorkspaceId,
    transactional_id: &str,
) -> Result<Option<HostedKafkaTransactionState>> {
    match loom.read_file(ns, &kafka_transaction_path(transactional_id)) {
        Ok(bytes) => HostedKafkaTransactionState::decode(&bytes).map(Some),
        Err(err) if err.code == Code::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

fn write_kafka_transaction_state(
    loom: &mut Loom<FileStore>,
    ns: WorkspaceId,
    state: &HostedKafkaTransactionState,
) -> Result<()> {
    loom.create_directory_reserved(ns, &kafka_transaction_dir(), true)?;
    loom.write_file_reserved(
        ns,
        &kafka_transaction_path(&state.transactional_id),
        &state.encode()?,
        0o100644,
    )
}

fn merge_kafka_transaction_topic(
    topics: &mut Vec<HostedKafkaTransactionTopic>,
    incoming: &HostedKafkaTransactionTopic,
) {
    match topics
        .iter_mut()
        .find(|entry| entry.topic == incoming.topic)
    {
        Some(existing) => {
            for partition in &incoming.partitions {
                if !existing.partitions.contains(partition) {
                    existing.partitions.push(*partition);
                }
            }
            existing.partitions.sort();
        }
        None => topics.push(HostedKafkaTransactionTopic {
            topic: incoming.topic.clone(),
            partitions: incoming.partitions.clone(),
        }),
    }
    topics.sort_by(|a, b| a.topic.cmp(&b.topic));
}

fn merge_kafka_pending_offset(
    offsets: &mut Vec<HostedKafkaPendingOffsetCommit>,
    incoming: &HostedKafkaPendingOffsetCommit,
) {
    match offsets.iter_mut().find(|entry| {
        entry.group_id == incoming.group_id
            && entry.topic == incoming.topic
            && entry.partition == incoming.partition
    }) {
        Some(existing) => existing.offset = incoming.offset,
        None => offsets.push(incoming.clone()),
    }
    offsets.sort_by(|a, b| {
        a.group_id
            .cmp(&b.group_id)
            .then_with(|| a.topic.cmp(&b.topic))
            .then_with(|| a.partition.cmp(&b.partition))
    });
}

fn merge_kafka_pending_produced_range(
    ranges: &mut Vec<HostedKafkaPendingProducedRange>,
    incoming: &HostedKafkaPendingProducedRange,
) {
    ranges.push(incoming.clone());
    ranges.sort_by(|a, b| {
        a.topic
            .cmp(&b.topic)
            .then_with(|| a.partition.cmp(&b.partition))
            .then_with(|| a.first_offset.cmp(&b.first_offset))
    });
}

#[derive(Default)]
struct KafkaTransactionVisibility {
    hidden_ranges: Vec<HostedKafkaPendingProducedRange>,
    last_stable_offset: Option<u64>,
    aborted_transactions: Vec<HostedKafkaAbortedTransaction>,
}

impl KafkaTransactionVisibility {
    fn is_hidden(&self, offset: u64) -> bool {
        self.hidden_ranges.iter().any(|range| {
            let end = range
                .first_offset
                .saturating_add(u64::from(range.record_count));
            offset >= range.first_offset && offset < end
        })
    }
}

fn kafka_transaction_visibility(
    loom: &Loom<FileStore>,
    ns: WorkspaceId,
    topic: &str,
    partition: i32,
) -> Result<KafkaTransactionVisibility> {
    let mut visibility = KafkaTransactionVisibility::default();
    let root = kafka_transaction_dir();
    let entries = match loom.list_directory(ns, &root) {
        Ok(entries) => entries,
        Err(err) if err.code == Code::NotFound => return Ok(visibility),
        Err(err) => return Err(err),
    };
    for entry in entries {
        if entry.kind != FileKind::File {
            continue;
        }
        let transaction = HostedKafkaTransactionState::decode(
            &loom.read_file(ns, &format!("{root}/{}", entry.name))?,
        )?;
        for range in transaction
            .pending_produced_ranges
            .iter()
            .filter(|range| range.topic == topic && range.partition == partition)
        {
            match transaction.status {
                HostedKafkaTransactionStatus::Committed => {}
                HostedKafkaTransactionStatus::Active => {
                    visibility.last_stable_offset = Some(
                        visibility
                            .last_stable_offset
                            .map_or(range.first_offset, |offset| offset.min(range.first_offset)),
                    );
                    visibility.hidden_ranges.push(range.clone());
                }
                HostedKafkaTransactionStatus::Aborted => {
                    visibility.hidden_ranges.push(range.clone());
                    visibility
                        .aborted_transactions
                        .push(HostedKafkaAbortedTransaction {
                            producer_id: transaction.producer_id,
                            first_offset: range.first_offset,
                        });
                }
            }
        }
    }
    visibility.hidden_ranges.sort_by(|a, b| {
        a.first_offset
            .cmp(&b.first_offset)
            .then_with(|| a.topic.cmp(&b.topic))
            .then_with(|| a.partition.cmp(&b.partition))
    });
    visibility
        .aborted_transactions
        .sort_by_key(|entry| entry.first_offset);
    Ok(visibility)
}

fn validate_kafka_offset_commit(
    loom: &Loom<FileStore>,
    ns: WorkspaceId,
    topic: &str,
    partition: i32,
    group_id: &str,
    offset: u64,
) -> Result<()> {
    validate_kafka_group_id(group_id)?;
    let mut metadata = read_kafka_topic_metadata(loom, ns, topic)?;
    ensure_kafka_partition(&metadata, partition)?;
    refresh_kafka_partition_offsets(loom, ns, &mut metadata)?;
    let high_watermark = metadata
        .partitions
        .iter()
        .find(|entry| entry.partition == partition)
        .map(|entry| entry.high_watermark)
        .unwrap_or(0);
    if offset > high_watermark {
        return Err(LoomError::invalid(
            "Kafka committed offset is beyond the high watermark",
        ));
    }
    Ok(())
}

fn apply_kafka_offset_commit(
    loom: &mut Loom<FileStore>,
    ns: WorkspaceId,
    topic: &str,
    partition: i32,
    group_id: &str,
    offset: u64,
) -> Result<()> {
    validate_kafka_offset_commit(loom, ns, topic, partition, group_id, offset)?;
    log::consumer_advance(
        loom,
        ns,
        topic,
        &kafka_consumer_id(group_id, partition),
        offset,
    )
}

fn refresh_kafka_partition_offsets(
    loom: &Loom<FileStore>,
    ns: WorkspaceId,
    metadata: &mut HostedKafkaTopicMetadata,
) -> Result<()> {
    let next_offset = log::len(loom, ns, &metadata.topic)? as u64;
    for partition in &mut metadata.partitions {
        if partition.partition == 0 {
            partition.next_offset = next_offset;
            partition.high_watermark = next_offset;
        }
    }
    Ok(())
}

fn ensure_kafka_partition(metadata: &HostedKafkaTopicMetadata, partition: i32) -> Result<()> {
    if metadata
        .partitions
        .iter()
        .any(|entry| entry.partition == partition)
    {
        Ok(())
    } else {
        Err(LoomError::not_found("Kafka partition"))
    }
}

fn kafka_consumer_id(group_id: &str, partition: i32) -> String {
    format!("kafka:{}:{partition}", path_segment(group_id))
}

fn validate_kafka_transactional_id(value: &str) -> Result<()> {
    if value.is_empty() {
        return Err(LoomError::invalid("Kafka transactional id is empty"));
    }
    if value.len() > 249 {
        return Err(LoomError::invalid("Kafka transactional id is too long"));
    }
    Ok(())
}

fn validate_kafka_group_id(value: &str) -> Result<()> {
    if value.is_empty() {
        return Err(LoomError::invalid("Kafka group id is empty"));
    }
    if value.len() > 249 {
        return Err(LoomError::invalid("Kafka group id is too long"));
    }
    Ok(())
}

fn kafka_topic_metadata_dir() -> String {
    facet_path(FacetKind::Queue, ".kafka/topics")
}

fn kafka_topic_metadata_path(topic: &str) -> String {
    facet_path(
        FacetKind::Queue,
        &format!(".kafka/topics/{}", path_segment(topic)),
    )
}

fn coordination_metadata_version_root() -> String {
    facet_path(FacetKind::Queue, ".coordination/metadata-versions")
}

fn coordination_metadata_version_dir(scope: &CoordinationScope) -> String {
    let mut path = coordination_metadata_version_root();
    let parts = scope.parts();
    for part in &parts[..parts.len().saturating_sub(1)] {
        path.push('/');
        path.push_str(&path_segment(part));
    }
    path
}

fn coordination_metadata_version_path(scope: &CoordinationScope) -> String {
    let mut path = coordination_metadata_version_root();
    for part in scope.parts() {
        path.push('/');
        path.push_str(&path_segment(part));
    }
    path
}

fn kafka_producer_dir() -> String {
    facet_path(FacetKind::Queue, ".kafka/producers")
}

fn kafka_producer_next_id_path() -> String {
    facet_path(FacetKind::Queue, ".kafka/producers/next-id")
}

fn kafka_producer_id_dir() -> String {
    facet_path(FacetKind::Queue, ".kafka/producers/id")
}

fn kafka_producer_id_path(producer_id: i64) -> String {
    facet_path(
        FacetKind::Queue,
        &format!(".kafka/producers/id/{producer_id}"),
    )
}

fn kafka_transactional_producer_dir() -> String {
    facet_path(FacetKind::Queue, ".kafka/producers/transactional")
}

fn kafka_transactional_producer_path(transactional_id: &str) -> String {
    facet_path(
        FacetKind::Queue,
        &format!(
            ".kafka/producers/transactional/{}",
            path_segment(transactional_id)
        ),
    )
}

fn kafka_producer_sequence_dir(topic: &str, partition: i32, _producer_id: i64) -> String {
    facet_path(
        FacetKind::Queue,
        &format!(
            ".kafka/producers/sequences/{}/{partition}",
            path_segment(topic)
        ),
    )
}

fn kafka_producer_sequence_path(topic: &str, partition: i32, producer_id: i64) -> String {
    facet_path(
        FacetKind::Queue,
        &format!(
            ".kafka/producers/sequences/{}/{partition}/{producer_id}",
            path_segment(topic)
        ),
    )
}

fn kafka_transaction_dir() -> String {
    facet_path(FacetKind::Queue, ".kafka/transactions")
}

fn kafka_transaction_path(transactional_id: &str) -> String {
    facet_path(
        FacetKind::Queue,
        &format!(".kafka/transactions/{}", path_segment(transactional_id)),
    )
}

fn kafka_topic_id(workspace: &str, topic: &str) -> [u8; 16] {
    let mut hash = Sha256::new();
    hash.update(b"loom.kafka.topic.v1");
    hash.update([0]);
    hash.update(workspace.as_bytes());
    hash.update([0]);
    hash.update(topic.as_bytes());
    let digest = hash.finalize();
    let mut out = [0u8; 16];
    out.copy_from_slice(&digest[..16]);
    out[6] = (out[6] & 0x0f) | 0x50;
    out[8] = (out[8] & 0x3f) | 0x80;
    out
}

fn hosted_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn fresh_workspace_id() -> WorkspaceId {
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let mut bytes = [0u8; 16];
    bytes[0..8].copy_from_slice(&nanos.to_be_bytes());
    bytes[8..12].copy_from_slice(&pid.to_be_bytes());
    bytes[12..16].copy_from_slice(&seq.to_be_bytes());
    WorkspaceId::v4_from_bytes(bytes)
}

fn path_segment(value: &str) -> String {
    let mut out = String::with_capacity(1 + value.len() * 2);
    out.push('h');
    for byte in value.as_bytes() {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn decode_path_segment(value: &str) -> Option<String> {
    let hex = value.strip_prefix('h')?;
    if hex.len() % 2 != 0 {
        return None;
    }
    let mut bytes = Vec::with_capacity(hex.len() / 2);
    let mut chars = hex.as_bytes().chunks_exact(2);
    for pair in &mut chars {
        let hi = hex_digit(pair[0])?;
        let lo = hex_digit(pair[1])?;
        bytes.push((hi << 4) | lo);
    }
    String::from_utf8(bytes).ok()
}

fn hex_digit(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

#[cfg(all(test, feature = "integration-tests"))]
mod tests {
    use std::fs;

    use loom_core::{
        FieldMapping, FieldValue, Mapping, Metric, Query, QueryRequest, Value, error::Code, graph,
        kv,
    };

    use super::{
        HostedEtcdCompare, HostedEtcdCompareResult, HostedEtcdCompareTarget, HostedEtcdEventKind,
        HostedEtcdRequestOp, HostedEtcdResponseOp,
    };
    use crate::test_support::{init, nid, temp_path};
    use crate::{HostedAuth, HostedKernel};

    #[test]
    fn hosted_data_adapter_covers_core_tier_one_facets() {
        let path = temp_path("data-adapter");
        init(&path, None);
        let auth = HostedAuth::passphrase(nid(1), "root-pass", "data-1");
        let kernel = HostedKernel::new(&path);
        let data = kernel.data();
        let key = kv::key_to_cbor(&Value::Text("a".to_string()));

        data.kv_put(&auth, "main", "cache", &key, b"one".to_vec())
            .unwrap();
        assert_eq!(
            data.kv_get(&auth, "main", "cache", &key).unwrap(),
            Some(b"one".to_vec())
        );
        assert_eq!(
            data.kv_list(&auth, "main", "cache")
                .unwrap()
                .first()
                .map(|entry| entry.value.clone()),
            Some(b"one".to_vec())
        );

        data.document_put(&auth, "main", "docs", "doc-1", br#"{"a":1}"#.to_vec())
            .unwrap();
        assert_eq!(
            data.document_get(&auth, "main", "docs", "doc-1").unwrap(),
            Some(br#"{"a":1}"#.to_vec())
        );
        assert_eq!(data.document_list(&auth, "main", "docs").unwrap().len(), 1);
        data.document_create_index(&auth, "main", "docs", "by_a", "a", false)
            .unwrap();
        assert_eq!(
            data.document_find(&auth, "main", "docs", "by_a", &Value::Int(1))
                .unwrap(),
            vec!["doc-1".to_string()]
        );
        assert_eq!(
            data.document_index_statuses(&auth, "main", "docs")
                .unwrap()
                .first()
                .map(|status| status.ready),
            Some(true)
        );
        let query = data
            .document_query(
                &auth,
                "main",
                "docs",
                &loom_core::DocumentQuery {
                    predicate: Some(loom_core::DocumentPredicate::Compare {
                        path: loom_core::DocumentFieldPath::dotted("a").unwrap(),
                        op: loom_core::CmpOp::Ge,
                        value: Value::Int(1),
                    }),
                    projections: vec![
                        loom_core::DocumentProjection::new(
                            "a",
                            loom_core::DocumentFieldPath::dotted("a").unwrap(),
                        )
                        .unwrap(),
                    ],
                    cursor: None,
                    limit: 10,
                    include_document: false,
                },
            )
            .unwrap();
        assert_eq!(query.items[0].id, "doc-1");
        assert_eq!(
            query.items[0].projections.get("a"),
            Some(&Some(Value::Int(1)))
        );

        assert_eq!(
            data.queue_append(&auth, "main", "events", b"event-1")
                .unwrap(),
            0
        );
        assert_eq!(
            data.queue_get(&auth, "main", "events", 0).unwrap(),
            Some(b"event-1".to_vec())
        );
        assert_eq!(data.queue_len(&auth, "main", "events").unwrap(), 1);

        data.timeseries_put(&auth, "main", "metrics", 100, b"p100".to_vec())
            .unwrap();
        assert_eq!(
            data.timeseries_get(&auth, "main", "metrics", 100).unwrap(),
            Some(b"p100".to_vec())
        );
        assert_eq!(
            data.timeseries_latest(&auth, "main", "metrics")
                .unwrap()
                .map(|point| point.value),
            Some(b"p100".to_vec())
        );

        data.graph_upsert_node(&auth, "main", "g", "a", Default::default())
            .unwrap();
        data.graph_upsert_node(&auth, "main", "g", "b", Default::default())
            .unwrap();
        data.graph_upsert_edge(
            &auth,
            "main",
            "g",
            "e1",
            "a",
            "b",
            "knows",
            Default::default(),
        )
        .unwrap();
        assert_eq!(
            data.graph_neighbors(&auth, "main", "g", "a").unwrap(),
            vec!["b".to_string()]
        );
        let mutations = graph::GraphMutationPlan::new(vec![
            graph::GraphMutation::CreateNode {
                id: "c".to_string(),
                labels: Default::default(),
                props: Default::default(),
            },
            graph::GraphMutation::CreateEdge {
                id: "e2".to_string(),
                src: "b".to_string(),
                dst: "c".to_string(),
                label: "knows".to_string(),
                props: Default::default(),
            },
        ]);
        assert_eq!(
            data.graph_apply_mutations(&auth, "main", "g", &mutations)
                .unwrap()
                .applied,
            2
        );
        let merge = graph::GraphMutationPlan::new(vec![
            graph::GraphMutation::MergeNode {
                id: "d".to_string(),
                labels: Default::default(),
                props: Default::default(),
            },
            graph::GraphMutation::MergeEdge {
                id: "e3".to_string(),
                src: "c".to_string(),
                dst: "d".to_string(),
                label: "knows".to_string(),
                props: Default::default(),
            },
        ]);
        assert_eq!(
            data.graph_apply_mutations(&auth, "main", "g", &merge)
                .unwrap()
                .applied,
            2
        );
        assert_eq!(
            data.graph_apply_mutations(&auth, "main", "g", &merge)
                .unwrap()
                .applied,
            2
        );
        assert_eq!(
            data.graph_neighbors(&auth, "main", "g", "b").unwrap(),
            vec!["a".to_string(), "c".to_string()]
        );

        assert_eq!(
            data.ledger_append(&auth, "main", "audit", b"entry".to_vec())
                .unwrap(),
            0
        );
        assert_eq!(
            data.ledger_get(&auth, "main", "audit", 0).unwrap(),
            Some(b"entry".to_vec())
        );
        data.ledger_verify(&auth, "main", "audit").unwrap();

        data.vector_create(&auth, "main", "embeddings", 2, Metric::Dot)
            .unwrap();
        data.vector_upsert(
            &auth,
            "main",
            "embeddings",
            "v1",
            vec![1.0, 0.0],
            Default::default(),
        )
        .unwrap();
        assert_eq!(
            data.vector_search(&auth, "main", "embeddings", &[1.0, 0.0], 1)
                .unwrap()
                .first()
                .map(|hit| hit.id.clone()),
            Some("v1".to_string())
        );

        let mut mapping = Mapping::new();
        mapping.insert("title".to_string(), FieldMapping::text());
        data.search_create(&auth, "main", "docs-search", mapping)
            .unwrap();
        data.search_index(
            &auth,
            "main",
            "docs-search",
            b"doc1".to_vec(),
            [(
                "title".to_string(),
                FieldValue::Text("quick brown fox".to_string()),
            )]
            .into_iter()
            .collect(),
        )
        .unwrap();
        let result = data
            .search_query(
                &auth,
                "main",
                "docs-search",
                QueryRequest::new(
                    Query::Match {
                        field: "title".to_string(),
                        text: "quick".to_string(),
                    },
                    1,
                    0,
                ),
            )
            .unwrap();
        assert_eq!(result.hits.len(), 1);

        fs::remove_file(path).unwrap();
    }

    #[test]
    fn hosted_etcd_adapter_tracks_revisions_leases_txns_and_preserves_raw_kv() {
        let path = temp_path("data-etcd-adapter");
        init(&path, None);
        let auth = HostedAuth::passphrase(nid(1), "root-pass", "data-etcd");
        let kernel = HostedKernel::new(&path);
        let data = kernel.data();
        let key = b"alpha".to_vec();

        let lease = data
            .etcd_lease_grant(&auth, "main", "config", 0, 60)
            .unwrap();
        assert_eq!(lease.revision, 1);
        assert!(lease.id > 0);
        assert_eq!(
            data.etcd_lease_keep_alive(&auth, "main", "config", lease.id)
                .unwrap()
                .ttl,
            60
        );

        let put = data
            .etcd_put(
                &auth,
                "main",
                "config",
                key.clone(),
                b"one".to_vec(),
                lease.id,
                false,
            )
            .unwrap();
        assert_eq!(put.revision, 2);
        let raw_key = kv::key_to_cbor(&Value::Bytes(key.clone()));
        assert_eq!(
            data.kv_get(&auth, "main", "config", &raw_key).unwrap(),
            Some(b"one".to_vec())
        );

        let range = data
            .etcd_range(&auth, "main", "config", key.clone(), Vec::new(), 0, 0)
            .unwrap();
        assert_eq!(range.count, 1);
        assert_eq!(range.kvs[0].create_revision, 2);
        assert_eq!(range.kvs[0].mod_revision, 2);
        assert_eq!(range.kvs[0].version, 1);
        assert_eq!(range.kvs[0].lease, lease.id);

        let txn = data
            .etcd_txn(
                &auth,
                "main",
                "config",
                vec![HostedEtcdCompare {
                    key: key.clone(),
                    result: HostedEtcdCompareResult::Equal,
                    target: HostedEtcdCompareTarget::Version(1),
                }],
                vec![HostedEtcdRequestOp::Put {
                    key: key.clone(),
                    value: b"two".to_vec(),
                    lease: lease.id,
                    prev_kv: true,
                }],
                vec![HostedEtcdRequestOp::DeleteRange {
                    key: key.clone(),
                    range_end: Vec::new(),
                    prev_kv: true,
                }],
            )
            .unwrap();
        assert!(txn.succeeded);
        assert_eq!(txn.revision, 3);
        assert!(matches!(txn.responses[0], HostedEtcdResponseOp::Put(_)));

        let range = data
            .etcd_range(&auth, "main", "config", key.clone(), Vec::new(), 0, 0)
            .unwrap();
        assert_eq!(range.kvs[0].value, b"two".to_vec());
        assert_eq!(range.kvs[0].version, 2);
        assert_eq!(range.kvs[0].mod_revision, 3);

        let watched = data
            .etcd_watch_events(&auth, "main", "config", key.clone(), Vec::new(), 2)
            .unwrap();
        assert_eq!(watched.revision, 3);
        assert_eq!(watched.events.len(), 2);
        assert_eq!(watched.events[0].kind, HostedEtcdEventKind::Put);
        assert_eq!(watched.events[0].revision, 2);
        assert_eq!(watched.events[0].kv.value, b"one".to_vec());
        assert_eq!(watched.events[1].kind, HostedEtcdEventKind::Put);
        assert_eq!(watched.events[1].revision, 3);
        assert_eq!(watched.events[1].kv.value, b"two".to_vec());
        assert_eq!(
            watched.events[1].prev_kv.as_ref().unwrap().value,
            b"one".to_vec()
        );

        let compact = data
            .etcd_compact(&auth, "main", "config", 2, false)
            .unwrap();
        assert_eq!(compact.revision, 3);
        assert_eq!(compact.compacted_revision, 2);
        assert_eq!(
            data.etcd_range(&auth, "main", "config", key.clone(), Vec::new(), 0, 2)
                .unwrap_err()
                .code,
            Code::InvalidArgument
        );
        assert_eq!(
            data.etcd_range(&auth, "main", "config", key.clone(), Vec::new(), 0, 0)
                .unwrap()
                .count,
            1
        );

        let revoked = data
            .etcd_lease_revoke(&auth, "main", "config", lease.id)
            .unwrap();
        assert_eq!(revoked.deleted, 1);
        let watched = data
            .etcd_watch_events(&auth, "main", "config", key.clone(), Vec::new(), 3)
            .unwrap();
        assert_eq!(watched.events.len(), 2);
        assert_eq!(watched.events[0].kind, HostedEtcdEventKind::Put);
        assert_eq!(watched.events[1].kind, HostedEtcdEventKind::Delete);
        assert_eq!(watched.events[1].revision, 4);
        assert_eq!(watched.events[1].kv.value, b"two".to_vec());
        assert_eq!(
            data.etcd_range(&auth, "main", "config", key, Vec::new(), 0, 0)
                .unwrap()
                .count,
            0
        );
        fs::remove_file(path).unwrap();
    }
}
