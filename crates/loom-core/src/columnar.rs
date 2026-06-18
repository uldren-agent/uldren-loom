//! The columnar facet - a versioned, append-oriented typed dataset stored as ordered segments. Pure
//! Rust, `wasm32`-clean, deterministic, and versioned through the engine.
//!
//! Segment policy: the writer honors a **target segment size** (rows per segment), rolling to a new
//! segment at the target so it never surprises with background rewrites, and the caller runs
//! **explicit [`ColumnarSet::compact`]** to merge the small-segment tail. This module does not
//! reconcile same-segment edits across branches.

use crate::AclRight;
use crate::error::{Code, LoomError, Result};
use crate::provider::ObjectStore;
use crate::tabular::{CmpOp, ColumnType, Value};
use crate::vcs::Loom;
use crate::workspace::{FacetKind, WorkspaceId, facet_path, facet_root};
pub use loom_columnar::{
    ColumnarAggregate, ColumnarAggregateOp, ColumnarColumnStatistics, ColumnarCompressionPolicy,
    ColumnarExecutor, ColumnarInspect, ColumnarManifest, ColumnarSegmentEncoding,
    ColumnarSegmentManifest, ColumnarSegmentMaterial, ColumnarSegmentStatistics, ColumnarSet,
    ColumnarStatisticsPolicy,
};
use std::collections::BTreeMap;

fn dataset_path(name: &str) -> String {
    facet_path(FacetKind::Columnar, name)
}

fn segment_name(digest: crate::Digest) -> String {
    digest.to_hex()
}

/// Stage `dataset` under `name` in `ns` as a structured columnar root; `commit` snapshots it.
pub fn put_columnar<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    name: &str,
    dataset: &ColumnarSet,
) -> Result<()> {
    loom.authorize_collection(ns, FacetKind::Columnar, name, AclRight::Write)?;
    let algo = loom.store().digest_algo();
    let materials = dataset.segment_materials_with_algo(algo);
    let manifest = ColumnarManifest {
        version: dataset.manifest_with_algo(algo).version,
        columns: dataset.columns().to_vec(),
        target_segment_rows: dataset.target_segment_rows(),
        statistics_policy: ColumnarStatisticsPolicy::Basic,
        compression_policy: ColumnarCompressionPolicy::None,
        segments: materials
            .iter()
            .map(|material| material.manifest.clone())
            .collect(),
    };
    let segments = materials
        .into_iter()
        .map(|material| (segment_name(material.manifest.digest), material.bytes))
        .collect::<BTreeMap<_, _>>();
    loom.create_directory_reserved(ns, &facet_root(FacetKind::Columnar), true)?;
    loom.stage_columnar_reserved(ns, &dataset_path(name), &manifest.encode(), segments)
}

/// Load the dataset named `name` from `ns`'s current working tree, or `NOT_FOUND`.
pub fn get_columnar<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
) -> Result<ColumnarSet> {
    loom.authorize_collection(ns, FacetKind::Columnar, name, AclRight::Read)?;
    let algo = loom.store().digest_algo();
    let (manifest_addr, segments) = loom.columnar_parts_reserved(ns, &dataset_path(name))?;
    let manifest = ColumnarManifest::decode(&loom.load_content(manifest_addr)?, algo)?;
    let mut materials = Vec::with_capacity(manifest.segments.len());
    for segment in &manifest.segments {
        let name = segment_name(segment.digest);
        let addr = segments
            .get(&name)
            .ok_or_else(|| LoomError::corrupt("columnar segment payload missing"))?;
        materials.push(ColumnarSegmentMaterial {
            manifest: segment.clone(),
            bytes: loom.load_content(*addr)?,
        });
    }
    ColumnarSet::from_manifest_segments(manifest, materials, algo)
}

/// Digest of the committed columnar source that native projections must be stamped against.
pub fn columnar_source_digest<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
) -> Result<crate::Digest> {
    loom.authorize_collection(ns, FacetKind::Columnar, name, AclRight::Read)?;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"loom-columnar-structured-root-v1");
    bytes.extend_from_slice(
        loom.columnar_root_reserved(ns, &dataset_path(name))?
            .bytes(),
    );
    Ok(crate::Digest::hash(loom.store().digest_algo(), &bytes))
}

/// Create an empty columnar dataset `name` in `ns` over `columns`, rolling segments at
/// `target_segment_rows` (0 selects the default), staging it. `CONFLICT` if a dataset already exists
/// under `name` (the schema is fixed at creation); `INVALID_ARGUMENT` if `columns` is empty.
pub fn columnar_create<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    name: &str,
    columns: Vec<(String, ColumnType)>,
    target_segment_rows: usize,
) -> Result<()> {
    loom.authorize_collection(ns, FacetKind::Columnar, name, AclRight::Write)?;
    match loom.columnar_parts_reserved(ns, &dataset_path(name)) {
        Ok(_) => Err(LoomError::new(
            Code::Conflict,
            format!("columnar dataset {name:?} already exists"),
        )),
        Err(e) if e.code == Code::NotFound => {
            let dataset = ColumnarSet::new(columns, target_segment_rows)?;
            put_columnar(loom, ns, name, &dataset)
        }
        Err(e) => Err(e),
    }
}

/// Append `row` to dataset `name` (validating arity + column types), staging the result. `NOT_FOUND` if
/// the dataset was never created; `INVALID_ARGUMENT` on an arity or type mismatch.
pub fn columnar_append<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    name: &str,
    row: Vec<Value>,
) -> Result<()> {
    loom.authorize_collection(ns, FacetKind::Columnar, name, AclRight::Write)?;
    let mut dataset = get_columnar(loom, ns, name)?;
    dataset.append_row(row)?;
    put_columnar(loom, ns, name, &dataset)
}

/// All rows of dataset `name` in append order. `NOT_FOUND` if the dataset does not exist.
pub fn columnar_scan<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
) -> Result<Vec<Vec<Value>>> {
    loom.authorize_collection(ns, FacetKind::Columnar, name, AclRight::Read)?;
    Ok(get_columnar(loom, ns, name)?.scan().cloned().collect())
}

/// The `(name, type)` columns of dataset `name`. `NOT_FOUND` if the dataset does not exist.
pub fn columnar_columns<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
) -> Result<Vec<(String, ColumnType)>> {
    loom.authorize_collection(ns, FacetKind::Columnar, name, AclRight::Read)?;
    Ok(get_columnar(loom, ns, name)?.columns().to_vec())
}

/// The total row count of dataset `name`. `NOT_FOUND` if the dataset does not exist.
pub fn columnar_rows<S: ObjectStore>(loom: &Loom<S>, ns: WorkspaceId, name: &str) -> Result<usize> {
    loom.authorize_collection(ns, FacetKind::Columnar, name, AclRight::Read)?;
    Ok(get_columnar(loom, ns, name)?.rows())
}

/// Re-chunk dataset `name` at its target segment size, staging the result.
pub fn columnar_compact<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    name: &str,
) -> Result<()> {
    loom.authorize_collection(ns, FacetKind::Columnar, name, AclRight::Write)?;
    let mut dataset = get_columnar(loom, ns, name)?;
    dataset.compact();
    put_columnar(loom, ns, name, &dataset)
}

/// Summary metadata for dataset `name`.
pub fn columnar_inspect<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
) -> Result<ColumnarInspect> {
    loom.authorize_collection(ns, FacetKind::Columnar, name, AclRight::Read)?;
    let dataset = get_columnar(loom, ns, name)?;
    let source_digest = columnar_source_digest(loom, ns, name)?;
    Ok(ColumnarInspect {
        columns: dataset.columns().to_vec(),
        rows: dataset.rows(),
        segment_count: dataset.segment_count(),
        target_segment_rows: dataset.target_segment_rows(),
        source_digest,
    })
}

/// Project `columns` from dataset `name`'s rows matching `filter` (see [`ColumnarSet::select`]).
/// `NOT_FOUND` if the dataset does not exist; `INVALID_ARGUMENT` on an unknown column. The portable
/// StateAccess path; [`columnar_select_auto`] is the same with an optional native executor.
pub fn columnar_select<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
    columns: &[&str],
    filter: Option<(&str, CmpOp, &Value)>,
) -> Result<Vec<Vec<Value>>> {
    loom.authorize_collection(ns, FacetKind::Columnar, name, AclRight::Read)?;
    columnar_select_auto(loom, ns, name, columns, filter, None)
}

/// Project `columns` from dataset `name` matching `filter`, running the portable [`ColumnarSet::select`]
/// when `exec` is `None` and delegating to the injected native executor otherwise. Both paths return the
/// same rows in the same order, so the switch is invisible except in speed. `NOT_FOUND` if the dataset
/// does not exist; `INVALID_ARGUMENT` on an unknown column.
pub fn columnar_select_auto<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
    columns: &[&str],
    filter: Option<(&str, CmpOp, &Value)>,
    exec: Option<&dyn ColumnarExecutor>,
) -> Result<Vec<Vec<Value>>> {
    loom.authorize_collection(ns, FacetKind::Columnar, name, AclRight::Read)?;
    let set = get_columnar(loom, ns, name)?;
    match exec {
        Some(e) => e.select(&set, columns, filter),
        None => set.select(columns, filter),
    }
}

/// Evaluate aggregates against dataset `name` using the portable path.
pub fn columnar_aggregate<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
    aggregates: &[ColumnarAggregate],
    filter: Option<(&str, CmpOp, &Value)>,
) -> Result<Vec<Value>> {
    loom.authorize_collection(ns, FacetKind::Columnar, name, AclRight::Read)?;
    columnar_aggregate_auto(loom, ns, name, aggregates, filter, None)
}

/// Evaluate aggregates through an injected native executor, or the portable path when none is present.
pub fn columnar_aggregate_auto<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
    aggregates: &[ColumnarAggregate],
    filter: Option<(&str, CmpOp, &Value)>,
    exec: Option<&dyn ColumnarExecutor>,
) -> Result<Vec<Value>> {
    loom.authorize_collection(ns, FacetKind::Columnar, name, AclRight::Read)?;
    let set = get_columnar(loom, ns, name)?;
    match exec {
        Some(e) => e.aggregate(&set, aggregates, filter),
        None => set.aggregate(aggregates, filter),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::digest::{Algo, DIGEST_LEN, Digest};
    use crate::provider::ObjectStore;
    use crate::provider::memory::MemoryStore;
    use crate::workspace::{FacetKind, WorkspaceId};
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    struct ProfileMemoryStore {
        algo: Algo,
        objects: Mutex<BTreeMap<[u8; DIGEST_LEN], Vec<u8>>>,
    }

    impl ProfileMemoryStore {
        fn new(algo: Algo) -> Self {
            Self {
                algo,
                objects: Mutex::new(BTreeMap::new()),
            }
        }

        fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<[u8; DIGEST_LEN], Vec<u8>>> {
            self.objects.lock().expect("profile memory store lock")
        }
    }

    impl ObjectStore for ProfileMemoryStore {
        fn put(&self, canonical: &[u8]) -> Result<Digest> {
            let digest = Digest::hash(self.algo, canonical);
            self.lock()
                .entry(*digest.bytes())
                .or_insert_with(|| canonical.to_vec());
            Ok(digest)
        }

        fn get(&self, digest: &Digest) -> Result<Option<Vec<u8>>> {
            Ok(self.lock().get(digest.bytes()).cloned())
        }

        fn has(&self, digest: &Digest) -> Result<bool> {
            Ok(self.lock().contains_key(digest.bytes()))
        }

        fn len(&self) -> usize {
            self.lock().len()
        }

        fn digest_algo(&self) -> Algo {
            self.algo
        }
    }

    fn cols() -> Vec<(String, ColumnType)> {
        vec![
            ("id".into(), ColumnType::Int),
            ("name".into(), ColumnType::Text),
        ]
    }

    #[test]
    fn facade_create_append_scan() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Columnar, None, WorkspaceId::from_bytes([9; 16]))
            .unwrap();
        // Operations require an explicit create (the schema is fixed).
        assert_eq!(
            columnar_append(
                &mut loom,
                ns,
                "events",
                vec![Value::Int(1), Value::Text("a".into())]
            )
            .unwrap_err()
            .code,
            Code::NotFound
        );
        columnar_create(&mut loom, ns, "events", cols(), 4).unwrap();
        // Re-create is a conflict.
        assert_eq!(
            columnar_create(&mut loom, ns, "events", cols(), 4)
                .unwrap_err()
                .code,
            Code::Conflict
        );
        columnar_append(
            &mut loom,
            ns,
            "events",
            vec![Value::Int(1), Value::Text("a".into())],
        )
        .unwrap();
        columnar_append(
            &mut loom,
            ns,
            "events",
            vec![Value::Int(2), Value::Text("b".into())],
        )
        .unwrap();
        // Type validation flows through the facade.
        assert!(
            columnar_append(
                &mut loom,
                ns,
                "events",
                vec![Value::Text("x".into()), Value::Text("y".into())]
            )
            .is_err()
        );
        assert_eq!(columnar_rows(&loom, ns, "events").unwrap(), 2);
        assert_eq!(columnar_columns(&loom, ns, "events").unwrap().len(), 2);
        let inspect = columnar_inspect(&loom, ns, "events").unwrap();
        assert_eq!(inspect.rows, 2);
        assert_eq!(inspect.segment_count, 1);
        assert_eq!(inspect.target_segment_rows, 4);
        assert_eq!(
            inspect.source_digest,
            columnar_source_digest(&loom, ns, "events").unwrap()
        );
        columnar_compact(&mut loom, ns, "events").unwrap();
        let rows = columnar_scan(&loom, ns, "events").unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0][0], Value::Int(1));
    }

    #[test]
    fn facade_versions_with_workspace_commits() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Columnar, None, WorkspaceId::from_bytes([2; 16]))
            .unwrap();
        columnar_create(&mut loom, ns, "events", cols(), 4).unwrap();
        columnar_append(
            &mut loom,
            ns,
            "events",
            vec![Value::Int(1), Value::Text("a".into())],
        )
        .unwrap();
        columnar_append(
            &mut loom,
            ns,
            "events",
            vec![Value::Int(2), Value::Text("b".into())],
        )
        .unwrap();
        let first = loom.commit(ns, "nas", "two rows", 1).unwrap();
        columnar_append(
            &mut loom,
            ns,
            "events",
            vec![Value::Int(3), Value::Text("c".into())],
        )
        .unwrap();
        loom.commit(ns, "nas", "three rows", 2).unwrap();
        assert_eq!(get_columnar(&loom, ns, "events").unwrap().rows(), 3);
        loom.checkout_commit(ns, first).unwrap();
        assert_eq!(get_columnar(&loom, ns, "events").unwrap().rows(), 2);
    }

    #[test]
    fn committed_columnar_manifest_uses_store_identity_profile() {
        let mut loom = Loom::new(ProfileMemoryStore::new(Algo::Sha256));
        let ns = loom
            .registry_mut()
            .create(FacetKind::Columnar, None, WorkspaceId::from_bytes([32; 16]))
            .unwrap();
        columnar_create(&mut loom, ns, "events", cols(), 4).unwrap();
        columnar_append(
            &mut loom,
            ns,
            "events",
            vec![Value::Int(1), Value::Text("a".into())],
        )
        .unwrap();

        assert_eq!(
            loom.read_file_reserved(ns, &dataset_path("events"))
                .unwrap_err()
                .code,
            Code::InvalidArgument
        );
        let decoded = get_columnar(&loom, ns, "events").unwrap();
        assert_eq!(decoded.rows(), 1);
        let manifest = decoded.manifest_with_algo(Algo::Sha256);
        assert_eq!(manifest.segments[0].digest.algo(), Algo::Sha256);
        loom.stage_columnar_reserved(
            ns,
            &dataset_path("events"),
            &manifest.encode(),
            BTreeMap::new(),
        )
        .unwrap();
        assert_eq!(
            get_columnar(&loom, ns, "events").unwrap_err().code,
            Code::CorruptObject
        );
    }

    #[test]
    fn authenticated_columnar_operations_honor_collection_scopes() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Columnar, None, WorkspaceId::from_bytes([31; 16]))
            .unwrap();
        let root = WorkspaceId::from_bytes([1; 16]);
        let mut identity = crate::IdentityStore::new(root);
        identity.set_passphrase(root, "root", b"12345678").unwrap();
        let session = identity
            .authenticate_passphrase(root, "root", "session")
            .unwrap();
        loom.set_identity_store(identity);
        loom.set_session(session.id);
        loom.acl_store_mut()
            .grant(crate::AclGrant {
                subject: crate::AclSubject::Principal(root),
                workspace: Some(ns),
                domain: Some(FacetKind::Columnar.into()),
                ref_glob: None,
                scopes: vec![crate::AclScope::Prefix {
                    kind: crate::AclScopeKind::Collection,
                    prefix: b"work".to_vec(),
                }],
                rights: [crate::AclRight::Write, crate::AclRight::Read]
                    .into_iter()
                    .collect(),
                effect: crate::AclEffect::Allow,
                predicate: None,
            })
            .unwrap();

        columnar_create(&mut loom, ns, "work", cols(), 4).unwrap();
        columnar_append(
            &mut loom,
            ns,
            "work",
            vec![Value::Int(1), Value::Text("a".into())],
        )
        .unwrap();
        assert_eq!(columnar_rows(&loom, ns, "work").unwrap(), 1);
        assert_eq!(
            columnar_create(&mut loom, ns, "private", cols(), 4)
                .unwrap_err()
                .code,
            Code::PermissionDenied
        );
    }

    /// A trivial executor standing in for the gated native engine: it delegates to the portable
    /// [`ColumnarSet::select`], so it must reconcile exactly.
    struct PassthroughExecutor;
    impl ColumnarExecutor for PassthroughExecutor {
        fn select(
            &self,
            set: &ColumnarSet,
            columns: &[&str],
            filter: Option<(&str, CmpOp, &Value)>,
        ) -> Result<Vec<Vec<Value>>> {
            set.select(columns, filter)
        }

        fn aggregate(
            &self,
            set: &ColumnarSet,
            aggregates: &[ColumnarAggregate],
            filter: Option<(&str, CmpOp, &Value)>,
        ) -> Result<Vec<Value>> {
            set.aggregate(aggregates, filter)
        }
    }

    #[test]
    fn select_auto_reconciles_with_an_injected_executor() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Columnar, None, WorkspaceId::from_bytes([12; 16]))
            .unwrap();
        columnar_create(&mut loom, ns, "t", cols(), 0).unwrap();
        for i in 0..5 {
            columnar_append(
                &mut loom,
                ns,
                "t",
                vec![Value::Int(i), Value::Text(format!("n{i}"))],
            )
            .unwrap();
        }
        let cols_sel = ["name"];
        let filter = Some(("id", CmpOp::Ge, &Value::Int(3)));
        // The portable path and the injected executor must return identical rows in identical order.
        let portable = columnar_select_auto(&loom, ns, "t", &cols_sel, filter, None).unwrap();
        let injected = columnar_select_auto(
            &loom,
            ns,
            "t",
            &cols_sel,
            filter,
            Some(&PassthroughExecutor),
        )
        .unwrap();
        assert_eq!(
            portable, injected,
            "an executor must reconcile to the portable select"
        );
        assert_eq!(portable.len(), 2);
        // The public facade is the no-executor path.
        assert_eq!(
            columnar_select(&loom, ns, "t", &cols_sel, filter).unwrap(),
            portable
        );
    }

    #[test]
    fn aggregate_auto_reconciles_with_an_injected_executor() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(
                FacetKind::Columnar,
                Some("aggregate"),
                WorkspaceId::from_bytes([13; 16]),
            )
            .unwrap();
        columnar_create(&mut loom, ns, "t", cols(), 0).unwrap();
        for i in 0..5 {
            columnar_append(
                &mut loom,
                ns,
                "t",
                vec![Value::Int(i), Value::Text(format!("n{i}"))],
            )
            .unwrap();
        }
        let aggregates = [
            ColumnarAggregate {
                op: ColumnarAggregateOp::Count,
                column: None,
            },
            ColumnarAggregate {
                op: ColumnarAggregateOp::Min,
                column: Some("id".into()),
            },
            ColumnarAggregate {
                op: ColumnarAggregateOp::Max,
                column: Some("id".into()),
            },
            ColumnarAggregate {
                op: ColumnarAggregateOp::Sum,
                column: Some("id".into()),
            },
        ];
        let filter = Some(("id", CmpOp::Ge, &Value::Int(2)));
        let portable = columnar_aggregate_auto(&loom, ns, "t", &aggregates, filter, None).unwrap();
        let injected = columnar_aggregate_auto(
            &loom,
            ns,
            "t",
            &aggregates,
            filter,
            Some(&PassthroughExecutor),
        )
        .unwrap();
        assert_eq!(portable, injected);
        assert_eq!(
            columnar_aggregate(&loom, ns, "t", &aggregates, filter).unwrap(),
            vec![Value::U64(3), Value::Int(2), Value::Int(4), Value::Int(9)]
        );
    }
}
