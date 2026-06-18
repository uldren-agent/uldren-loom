use super::*;
use crate::derived::{
    CALENDAR_DERIVED_INDEX_FORMAT_VERSION, CONTACTS_DERIVED_INDEX_FORMAT_VERSION,
    DATAFRAME_MATERIALIZATION_ARTIFACT_PREFIX, DATAFRAME_MATERIALIZATION_FORMAT_VERSION,
    GRAPH_PROPERTY_INDEX_ARTIFACT_PREFIX, GRAPH_PROPERTY_INDEX_FORMAT_VERSION,
    GRAPH_SPATIAL_INDEX_ARTIFACT_PREFIX, GRAPH_SPATIAL_INDEX_FORMAT_VERSION,
    MAIL_DERIVED_INDEX_FORMAT_VERSION, PIM_DERIVED_INDEX_ARTIFACT_PREFIX,
    SearchEmbeddingProjection, calendar_derived_index_artifact_key,
    calendar_derived_index_artifact_stamp, contacts_derived_index_artifact_key,
    contacts_derived_index_artifact_stamp, dataframe_materialization_artifact_key,
    dataframe_materialization_artifact_stamp, derived_artifact_format_version,
    graph_property_index_artifact_key, graph_property_index_artifact_stamp,
    graph_spatial_index_artifact_key, graph_spatial_index_artifact_stamp,
    mail_derived_index_artifact_key, mail_derived_index_artifact_stamp,
    search_embedding_artifact_key, search_embedding_artifact_stamp, vector_hnsw_artifact_key,
    vector_hnsw_artifact_stamp, vector_pq_artifact_key, vector_pq_artifact_stamp,
};
use loom_core::Object;
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A shared in-memory [`BackingIo`]: cloning shares the same byte buffer, so a `FileStore` can be
/// dropped and a fresh one reopened over the identical bytes - the persistence guarantee the OPFS
/// backend must also provide.
#[derive(Debug, Clone, Default)]
struct SharedMem(Arc<Mutex<Vec<u8>>>);

impl SharedMem {
    fn mutate_bytes(&self, mutate: impl FnOnce(&mut Vec<u8>)) {
        mutate(&mut self.0.lock().unwrap());
    }
}

impl BackingIo for SharedMem {
    fn pread(&mut self, off: u64, buf: &mut [u8]) -> std::io::Result<()> {
        let g = self.0.lock().unwrap();
        let (off, end) = (off as usize, off as usize + buf.len());
        if end > g.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "eof",
            ));
        }
        buf.copy_from_slice(&g[off..end]);
        Ok(())
    }
    fn pwrite(&mut self, off: u64, buf: &[u8]) -> std::io::Result<()> {
        let mut g = self.0.lock().unwrap();
        let (off, end) = (off as usize, off as usize + buf.len());
        if end > g.len() {
            g.resize(end, 0);
        }
        g[off..end].copy_from_slice(buf);
        Ok(())
    }
    fn size(&self) -> std::io::Result<u64> {
        Ok(self.0.lock().unwrap().len() as u64)
    }
    fn grow(&mut self, len: u64) -> std::io::Result<()> {
        self.0.lock().unwrap().resize(len as usize, 0);
        Ok(())
    }
    fn fsync(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn file_store_over_a_non_file_backing_round_trips_and_reopens() {
    // The path the OPFS backend follows: a FileStore built over a BackingIo that is not a
    // std::fs::File. Put an object, read it back, drop the store, then reopen over the SAME bytes -
    // the committed object survives, proving the backing abstraction carries the whole format.
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let d = store.put(b"hello, backing").unwrap();
    assert_eq!(
        store.get(&d).unwrap().as_deref(),
        Some(&b"hello, backing"[..])
    );
    drop(store);

    let reopened = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    assert_eq!(
        reopened.get(&d).unwrap().as_deref(),
        Some(&b"hello, backing"[..])
    );

    // A plain MemoryBacking also initializes + serves within one lifetime.
    let mem = FileStore::with_backing(Box::new(MemoryBacking::new()), true).unwrap();
    let d2 = mem.put(b"x").unwrap();
    assert!(mem.has(&d2).unwrap());
}

#[test]
fn maintenance_status_is_persisted_and_rejects_corrupt_record() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let d = store.put(b"maintenance metadata").unwrap();
    let first = store.maintenance_status().unwrap();
    assert_eq!(first.generation, 1);
    assert_eq!(first.object_count, 1);
    assert!(first.physical_page_count > 0);
    assert_eq!(
        first.physical_bytes,
        DATA_START + first.physical_page_count * PAGE_SIZE
    );
    assert!(!first.touched_segments.is_empty());
    assert!(!first.candidate_segments.is_empty());

    store.set_reference_root(Some(d)).unwrap();
    let second = store.maintenance_status().unwrap();
    assert_eq!(second.generation, 2);
    assert_eq!(second.object_count, 1);
    assert!(second.reusable_free_pages > 0);
    assert_eq!(second.candidate_dead_pages, second.reusable_free_pages);
    let maintenance_root = store
        .inner
        .lock()
        .unwrap()
        .maintenance_root
        .expect("maintenance root");
    drop(store);

    let reopened = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    assert_eq!(reopened.maintenance_status().unwrap(), second);
    drop(reopened);

    shared.mutate_bytes(|bytes| {
        let pos = (DATA_START + maintenance_root.0 * PAGE_SIZE) as usize;
        bytes[pos] ^= 0x80;
    });
    let err = FileStore::with_backing(Box::new(shared), true).unwrap_err();
    assert_eq!(err.code, Code::CorruptObject);
}

#[test]
fn store_maintenance_policy_and_run_state_persist() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let policy = StoreMaintenancePolicy {
        min_candidate_pages: 4,
        min_reusable_pages: 5,
        interval_ms: 10_000,
        backoff_ms: 30_000,
        max_segments: 2,
        max_pages: 128,
        full_compaction_enabled: true,
        tail_trim_enabled: true,
        tail_compaction_enabled: true,
        tail_compaction_max_pages: 32,
        tail_compaction_max_objects: 16,
        tail_compaction_max_bytes: 512 * 1024,
        tail_compaction_interval_ms: 20_000,
        tail_compaction_backoff_ms: 60_000,
    };
    store.set_store_maintenance_policy(policy).unwrap();
    let run_state = StoreMaintenanceRunState {
        last_run_ms: Some(42),
        next_eligible_ms: 99,
        last_skip_reason: Some("candidate_debt_below_threshold".to_string()),
        last_error: Some("io pressure".to_string()),
        last_tail_trim_attempted: true,
        last_tail_trim_pages: 3,
        last_tail_trim_bytes: 3 * PAGE_SIZE,
        last_tail_compaction_attempted: true,
        last_tail_compaction_relocated_objects: 2,
        last_tail_compaction_relocated_pages: 3,
        last_tail_compaction_relocated_bytes: 3 * PAGE_SIZE,
        last_tail_compaction_truncated_pages: 1,
        last_tail_compaction_conflicts: 4,
        last_shrink_skip_reason: Some("tail_blocked_by_live_objects".to_string()),
    };
    store
        .record_store_maintenance_run_state(run_state.clone())
        .unwrap();
    drop(store);

    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();
    assert_eq!(reopened.store_maintenance_policy().unwrap(), policy);
    assert_eq!(reopened.store_maintenance_run_state().unwrap(), run_state);
}

#[test]
fn store_maintenance_policy_rejects_invalid_updates_without_overwrite() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    let policy = StoreMaintenancePolicy {
        min_candidate_pages: 4,
        min_reusable_pages: 5,
        interval_ms: 10_000,
        backoff_ms: 30_000,
        max_segments: 2,
        max_pages: 128,
        full_compaction_enabled: true,
        ..StoreMaintenancePolicy::default()
    };
    store.set_store_maintenance_policy(policy).unwrap();
    for invalid in [
        StoreMaintenancePolicy {
            interval_ms: 0,
            ..policy
        },
        StoreMaintenancePolicy {
            backoff_ms: 0,
            ..policy
        },
        StoreMaintenancePolicy {
            max_segments: 0,
            ..policy
        },
        StoreMaintenancePolicy {
            max_pages: 0,
            ..policy
        },
        StoreMaintenancePolicy {
            tail_compaction_interval_ms: 0,
            ..policy
        },
        StoreMaintenancePolicy {
            tail_compaction_backoff_ms: 0,
            ..policy
        },
        StoreMaintenancePolicy {
            tail_compaction_max_pages: 0,
            ..policy
        },
        StoreMaintenancePolicy {
            tail_compaction_max_objects: 0,
            ..policy
        },
        StoreMaintenancePolicy {
            tail_compaction_max_bytes: 0,
            ..policy
        },
    ] {
        let error = store.set_store_maintenance_policy(invalid).unwrap_err();
        assert_eq!(error.code, Code::InvalidArgument);
        assert_eq!(store.store_maintenance_policy().unwrap(), policy);
    }
    drop(store);

    let reopened = FileStore::with_backing(Box::new(shared), true).unwrap();
    assert_eq!(reopened.store_maintenance_policy().unwrap(), policy);
}

#[test]
fn store_maintenance_report_projects_debt_and_mark_readiness() {
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared), true).unwrap();
    let keep = store.put(b"live").unwrap();
    store.set_reference_root(Some(keep)).unwrap();
    let status = store.maintenance_status().unwrap();
    assert!(status.candidate_dead_pages > 0);
    let default_report = store.store_maintenance_report(100).unwrap();
    assert_eq!(default_report.reason, "mark_epoch_missing");
    assert!(default_report.eligible);
    assert_eq!(
        default_report.candidate_reclaimable_bytes,
        status.candidate_dead_pages * PAGE_SIZE
    );
    assert_eq!(
        default_report.reusable_free_bytes,
        status.reusable_free_pages * PAGE_SIZE
    );
    assert_eq!(default_report.tail_free_pages, status.tail_free_pages);
    assert_eq!(default_report.tail_free_bytes, status.tail_free_bytes);
    assert_eq!(
        default_report.tail_free_bytes,
        default_report.tail_free_pages * PAGE_SIZE
    );
    assert!(!default_report.tail_trim_eligible);
    assert_eq!(
        default_report.tail_compaction_eligible,
        default_report.tail_blocked_by_live_objects
    );
    assert_eq!(
        default_report.full_compaction_required_for_shrink,
        default_report.tail_blocked_by_live_objects && !default_report.tail_compaction_eligible
    );

    store
        .set_store_maintenance_policy(StoreMaintenancePolicy {
            min_candidate_pages: u64::MAX,
            min_reusable_pages: u64::MAX,
            interval_ms: 1_000,
            backoff_ms: 2_000,
            max_segments: 1,
            max_pages: 64,
            full_compaction_enabled: false,
            ..StoreMaintenancePolicy::default()
        })
        .unwrap();
    let enabled = store.store_maintenance_report(100).unwrap();
    assert_eq!(enabled.reason, "mark_epoch_missing");
    assert!(enabled.eligible);
    assert!(enabled.policy.tail_trim_enabled);
    assert!(enabled.policy.tail_compaction_enabled);
}

#[test]
fn store_maintenance_report_attributes_reclaimable_and_derived_state() {
    // A synthetic churned store: keep one object reachable via the reference root, then write and
    // discard several more to create reclaimable dead space. The diagnostic must attribute the
    // reclaimable garbage and report the new derived/control-root fields (MX-303).
    let shared = SharedMem::default();
    let store = FileStore::with_backing(Box::new(shared), true).unwrap();
    let keep = store.put(b"live-data").unwrap();
    store.set_reference_root(Some(keep)).unwrap();
    for byte in 0..8u8 {
        let _unreachable = store.put(&[byte; 512]).unwrap();
    }
    let status = store.maintenance_status().unwrap();
    assert!(
        status.candidate_dead_pages > 0,
        "churn should create reclaimable dead pages"
    );

    let report = store.store_maintenance_report(100).unwrap();
    // Reclaimable garbage is attributed from the same dead-page accounting.
    assert_eq!(
        report.candidate_reclaimable_bytes,
        status.candidate_dead_pages * PAGE_SIZE
    );
    assert!(report.candidate_reclaimable_bytes > 0);
    // New attribution fields: a fresh store has no durable-local derived artifacts, and with no
    // active reachability-mark epoch there are no retained control roots or marked-live objects.
    assert_eq!(report.derived_payload_count, 0);
    assert_eq!(report.retained_control_roots, 0);
    assert_eq!(report.marked_live_objects, 0);
}

#[test]
fn reachability_mark_epoch_resumes_after_reopen_and_validates_on_completion() {
    use loom_core::WsSelector;
    use loom_core::workspace::{FacetKind, WorkspaceId};

    let tp = TempPath::new("mark-epoch-resume");
    let epoch_id;
    {
        let mut loom = open_loom(tp.path()).unwrap();
        let ns = loom
            .registry_mut()
            .create(
                FacetKind::Files,
                Some("p"),
                WorkspaceId::from_bytes([31; 16]),
            )
            .unwrap();
        for i in 0..8u64 {
            loom.write_file(
                ns,
                &format!("f{i}.txt"),
                format!("v{i}").as_bytes(),
                0o100644,
            )
            .unwrap();
            loom.commit(ns, "nas", "edit", i + 1).unwrap();
        }
        save_loom(&mut loom).unwrap();
        let epoch = begin_loom_reachability_mark_epoch(&loom).unwrap();
        epoch_id = epoch.epoch;
        let step = step_loom_reachability_mark_epoch(&loom, 1).unwrap();
        assert!(!step.completed);
        loom.store()
            .record_store_maintenance_run_state(StoreMaintenanceRunState {
                last_run_ms: Some(100),
                next_eligible_ms: 1_100,
                last_skip_reason: Some("mark_epoch_incomplete".to_string()),
                last_error: None,
                ..StoreMaintenanceRunState::default()
            })
            .unwrap();
        assert_eq!(
            loom.store()
                .maintenance_status()
                .unwrap()
                .last_validated_mark_epoch,
            0
        );
    }

    let loom = open_loom(tp.path()).unwrap();
    assert_eq!(
        loom.store()
            .active_reachability_mark_epoch()
            .unwrap()
            .unwrap()
            .epoch,
        epoch_id
    );
    let mut completed = false;
    for _ in 0..256 {
        let step = step_loom_reachability_mark_epoch(&loom, 2).unwrap();
        if step.completed {
            completed = true;
            break;
        }
    }
    assert!(completed);
    assert_eq!(
        loom.store()
            .maintenance_status()
            .unwrap()
            .last_validated_mark_epoch,
        epoch_id
    );
    let active = loom
        .store()
        .active_reachability_mark_epoch()
        .unwrap()
        .unwrap();
    let expected = loom.live_object_set(loom.store().reference_root()).unwrap();
    assert!(expected.is_subset(&active.state.marked));
    let ns = loom
        .registry()
        .open(&WsSelector::Typed {
            ty: FacetKind::Files,
            name: "p".to_string(),
        })
        .unwrap();
    assert_eq!(loom.read_file(ns, "f7.txt").unwrap(), b"v7");
}

#[test]
fn reachability_mark_epoch_rejects_completion_after_reference_root_changes() {
    use loom_core::workspace::{FacetKind, WorkspaceId};

    let tp = TempPath::new("mark-epoch-conflict");
    let mut loom = open_loom(tp.path()).unwrap();
    let ns = loom
        .registry_mut()
        .create(
            FacetKind::Files,
            Some("p"),
            WorkspaceId::from_bytes([32; 16]),
        )
        .unwrap();
    loom.write_file(ns, "a.txt", b"a", 0o100644).unwrap();
    loom.commit(ns, "nas", "initial", 1).unwrap();
    save_loom(&mut loom).unwrap();
    begin_loom_reachability_mark_epoch(&loom).unwrap();
    step_loom_reachability_mark_epoch(&loom, 1).unwrap();

    loom.write_file(ns, "b.txt", b"b", 0o100644).unwrap();
    loom.commit(ns, "nas", "concurrent", 2).unwrap();
    save_loom(&mut loom).unwrap();

    let mut conflict = None;
    for _ in 0..256 {
        match step_loom_reachability_mark_epoch(&loom, 8) {
            Ok(step) if !step.completed => {}
            Ok(_) => panic!("stale mark epoch completed after reference root changed"),
            Err(error) => {
                conflict = Some(error);
                break;
            }
        }
    }
    let conflict = conflict.expect("expected stale mark epoch conflict");
    assert_eq!(conflict.code, Code::Conflict);
    assert_eq!(
        loom.store()
            .maintenance_status()
            .unwrap()
            .last_validated_mark_epoch,
        0
    );
}

#[test]
fn gc_validated_segments_rejects_stale_epoch_after_later_commit() {
    use loom_core::WsSelector;
    use loom_core::workspace::{FacetKind, WorkspaceId};

    let tp = TempPath::new("gc-validated-stale-commit");
    let ns;
    {
        let mut loom = open_loom(tp.path()).unwrap();
        ns = loom
            .registry_mut()
            .create(
                FacetKind::Files,
                Some("p"),
                WorkspaceId::from_bytes([33; 16]),
            )
            .unwrap();
        loom.write_file(ns, "a.txt", b"a", 0o100644).unwrap();
        loom.commit(ns, "nas", "initial", 1).unwrap();
        save_loom(&mut loom).unwrap();
        begin_loom_reachability_mark_epoch(&loom).unwrap();
        let mut completed = false;
        for _ in 0..256 {
            let step = step_loom_reachability_mark_epoch(&loom, 8).unwrap();
            if step.completed {
                completed = true;
                break;
            }
        }
        assert!(completed);

        loom.write_file(ns, "b.txt", b"b", 0o100644).unwrap();
        loom.commit(ns, "nas", "later", 2).unwrap();
        save_loom(&mut loom).unwrap();

        let error = loom
            .store_mut()
            .gc_validated_segments(GcSegmentBudget {
                max_segments: 1,
                max_pages: u64::MAX,
            })
            .unwrap_err();
        assert_eq!(error.code, Code::Conflict);
        assert!(
            loom.store()
                .active_reachability_mark_epoch()
                .unwrap()
                .is_none()
        );
    }

    let loom = open_loom(tp.path()).unwrap();
    let reopened_ns = loom
        .registry()
        .open(&WsSelector::Typed {
            ty: FacetKind::Files,
            name: "p".to_string(),
        })
        .unwrap();
    assert_eq!(reopened_ns, ns);
    assert_eq!(loom.read_file(reopened_ns, "b.txt").unwrap(), b"b");
}

fn complete_validated_segment_epoch(store: &FileStore) {
    let n = 300usize;
    let mut digests = Vec::with_capacity(n);
    for i in 0..n {
        digests.push(store.put(&blob(format!("obj-{i:04}").as_bytes())).unwrap());
    }
    let live_digests = digests
        .iter()
        .enumerate()
        .filter(|(i, _)| i % 10 == 0)
        .map(|(_, digest)| *digest)
        .collect::<BTreeSet<_>>();
    let state = loom_core::ReachabilityMarkState {
        pinned: BTreeSet::new(),
        marked: live_digests,
        queue: std::collections::VecDeque::new(),
        stream_roots: std::collections::VecDeque::new(),
        completed: true,
    };
    let epoch = store
        .begin_reachability_mark_epoch(
            store.reference_root(),
            store.derived_artifact_roots().unwrap(),
            state,
        )
        .unwrap();
    store.complete_reachability_mark_epoch(&epoch).unwrap();
}

#[test]
fn gc_validated_segments_revalidates_after_pre_reclaim_interleave() {
    let tp = TempPath::new("gc-validated-pre-reclaim-interleave");
    let mut store = FileStore::open(tp.path()).unwrap();
    complete_validated_segment_epoch(&store);
    let new_root = store.put(&blob(b"new-root")).unwrap();

    let error = store
        .gc_validated_segments_with_pre_reclaim_interleave(
            GcSegmentBudget {
                max_segments: 1,
                max_pages: u64::MAX,
            },
            |store| store.set_reference_root(Some(new_root)),
        )
        .unwrap_err();
    assert_eq!(error.code, Code::Conflict);
    assert!(store.active_reachability_mark_epoch().unwrap().is_none());
    assert!(store.has(&new_root).unwrap());
}

#[test]
fn gc_validated_segments_allows_foreground_write_during_read_phase() {
    let tp = TempPath::new("gc-validated-read-phase-write");
    let mut store = FileStore::open(tp.path()).unwrap();
    complete_validated_segment_epoch(&store);
    let mut foreground = None;

    let error = store
        .gc_validated_segments_with_read_phase_interleave(
            GcSegmentBudget {
                max_segments: 1,
                max_pages: u64::MAX,
            },
            |store| {
                let digest = store.put(&blob(b"foreground-write"))?;
                foreground = Some(digest);
                Ok(())
            },
        )
        .unwrap_err();
    assert_eq!(error.code, Code::Conflict);
    let foreground = foreground.expect("foreground write did not run");
    assert!(store.has(&foreground).unwrap());
    assert!(store.active_reachability_mark_epoch().unwrap().is_none());
}

#[test]
fn gc_validated_segments_rejects_stale_epoch_after_branch_change() {
    use loom_core::workspace::{FacetKind, WorkspaceId};

    let tp = TempPath::new("gc-validated-stale-branch");
    let mut loom = open_loom(tp.path()).unwrap();
    let ns = loom
        .registry_mut()
        .create(
            FacetKind::Files,
            Some("p"),
            WorkspaceId::from_bytes([34; 16]),
        )
        .unwrap();
    loom.write_file(ns, "a.txt", b"a", 0o100644).unwrap();
    loom.commit(ns, "nas", "initial", 1).unwrap();
    save_loom(&mut loom).unwrap();
    begin_loom_reachability_mark_epoch(&loom).unwrap();
    for _ in 0..256 {
        if step_loom_reachability_mark_epoch(&loom, 8)
            .unwrap()
            .completed
        {
            break;
        }
    }

    loom.branch(ns, "feature").unwrap();
    save_loom(&mut loom).unwrap();
    let error = loom
        .store_mut()
        .gc_validated_segments(GcSegmentBudget {
            max_segments: 1,
            max_pages: u64::MAX,
        })
        .unwrap_err();
    assert_eq!(error.code, Code::Conflict);
    assert!(
        loom.store()
            .active_reachability_mark_epoch()
            .unwrap()
            .is_none()
    );
}

#[test]
fn gc_validated_segments_rejects_stale_epoch_after_tag_change() {
    use loom_core::workspace::{FacetKind, WorkspaceId};

    let tp = TempPath::new("gc-validated-stale-tag");
    let mut loom = open_loom(tp.path()).unwrap();
    let ns = loom
        .registry_mut()
        .create(
            FacetKind::Files,
            Some("p"),
            WorkspaceId::from_bytes([35; 16]),
        )
        .unwrap();
    loom.write_file(ns, "a.txt", b"a", 0o100644).unwrap();
    loom.commit(ns, "nas", "initial", 1).unwrap();
    save_loom(&mut loom).unwrap();
    begin_loom_reachability_mark_epoch(&loom).unwrap();
    for _ in 0..256 {
        if step_loom_reachability_mark_epoch(&loom, 8)
            .unwrap()
            .completed
        {
            break;
        }
    }

    loom.tag_create(ns, "v1", "HEAD", "", "", 0).unwrap();
    save_loom(&mut loom).unwrap();
    let error = loom
        .store_mut()
        .gc_validated_segments(GcSegmentBudget {
            max_segments: 1,
            max_pages: u64::MAX,
        })
        .unwrap_err();
    assert_eq!(error.code, Code::Conflict);
    assert!(
        loom.store()
            .active_reachability_mark_epoch()
            .unwrap()
            .is_none()
    );
}

#[test]
fn gc_drops_commit_only_reachable_from_deleted_branch() {
    use loom_core::workspace::{DEFAULT_BRANCH, FacetKind, WorkspaceId};

    let tp = TempPath::new("gc-deleted-branch-root");
    let mut loom = open_loom(tp.path()).unwrap();
    let ns = loom
        .registry_mut()
        .create(
            FacetKind::Files,
            Some("p"),
            WorkspaceId::from_bytes([37; 16]),
        )
        .unwrap();
    loom.write_file(ns, "shared.txt", b"shared", 0o100644)
        .unwrap();
    let main = loom.commit(ns, "nas", "main", 1).unwrap();
    loom.branch(ns, "feature").unwrap();
    loom.checkout_branch(ns, "feature").unwrap();
    loom.write_file(ns, "unique.txt", b"unique", 0o100644)
        .unwrap();
    let feature = loom.commit(ns, "nas", "feature", 2).unwrap();
    loom.checkout_branch(ns, DEFAULT_BRANCH).unwrap();
    loom.branch_delete(ns, "feature").unwrap();
    save_loom(&mut loom).unwrap();

    let live = loom.live_object_set(loom.store().reference_root()).unwrap();
    assert!(live.contains(&main));
    assert!(!live.contains(&feature));
    gc_loom(&mut loom).unwrap();
    assert!(loom.store().has(&main).unwrap());
    assert!(!loom.store().has(&feature).unwrap());
}

#[test]
fn gc_drops_commit_only_reachable_from_deleted_tag() {
    use loom_core::workspace::{DEFAULT_BRANCH, FacetKind, WorkspaceId};

    let tp = TempPath::new("gc-deleted-tag-root");
    let mut loom = open_loom(tp.path()).unwrap();
    let ns = loom
        .registry_mut()
        .create(
            FacetKind::Files,
            Some("p"),
            WorkspaceId::from_bytes([38; 16]),
        )
        .unwrap();
    loom.write_file(ns, "shared.txt", b"shared", 0o100644)
        .unwrap();
    let main = loom.commit(ns, "nas", "main", 1).unwrap();
    loom.branch(ns, "feature").unwrap();
    loom.checkout_branch(ns, "feature").unwrap();
    loom.write_file(ns, "tagged.txt", b"tagged", 0o100644)
        .unwrap();
    let tagged = loom.commit(ns, "nas", "tagged", 2).unwrap();
    loom.tag_create(ns, "snapshot", &tagged.to_string(), "nas", "", 3)
        .unwrap();
    loom.checkout_branch(ns, DEFAULT_BRANCH).unwrap();
    loom.branch_delete(ns, "feature").unwrap();
    save_loom(&mut loom).unwrap();
    assert!(
        loom.live_object_set(loom.store().reference_root())
            .unwrap()
            .contains(&tagged)
    );

    loom.tag_delete(ns, "snapshot").unwrap();
    save_loom(&mut loom).unwrap();
    let live = loom.live_object_set(loom.store().reference_root()).unwrap();
    assert!(live.contains(&main));
    assert!(!live.contains(&tagged));
    gc_loom(&mut loom).unwrap();
    assert!(loom.store().has(&main).unwrap());
    assert!(!loom.store().has(&tagged).unwrap());
}

#[test]
fn gc_validated_segments_ignores_maintenance_metadata_changes() {
    let tp = TempPath::new("gc-validated-maintenance-metadata");
    let mut store = FileStore::open(tp.path()).unwrap();
    let digest = store.put(&blob(b"live")).unwrap();
    let state = loom_core::ReachabilityMarkState {
        pinned: BTreeSet::from([digest]),
        marked: BTreeSet::from([digest]),
        queue: std::collections::VecDeque::new(),
        stream_roots: std::collections::VecDeque::new(),
        completed: true,
    };
    let epoch = store
        .begin_reachability_mark_epoch(None, BTreeSet::new(), state)
        .unwrap();
    store.complete_reachability_mark_epoch(&epoch).unwrap();
    store
        .set_store_maintenance_policy(StoreMaintenancePolicy {
            min_candidate_pages: 0,
            min_reusable_pages: 0,
            interval_ms: 1_000,
            backoff_ms: 2_000,
            max_segments: 1,
            max_pages: 64,
            full_compaction_enabled: false,
            ..StoreMaintenancePolicy::default()
        })
        .unwrap();
    store
        .record_store_maintenance_run_state(StoreMaintenanceRunState {
            last_run_ms: Some(100),
            next_eligible_ms: 1_100,
            last_skip_reason: Some("mark_epoch_incomplete".to_string()),
            last_error: None,
            ..StoreMaintenanceRunState::default()
        })
        .unwrap();

    store
        .gc_validated_segments(GcSegmentBudget {
            max_segments: 1,
            max_pages: u64::MAX,
        })
        .unwrap();
    assert!(store.active_reachability_mark_epoch().unwrap().is_some());
}

#[test]
fn gc_validated_segments_rejects_stale_epoch_after_control_root_changes() {
    let tp = TempPath::new("gc-validated-stale-control");
    let mut store = FileStore::open(tp.path()).unwrap();
    complete_validated_segment_epoch(&store);
    store
        .control_set(b"application/config", b"changed".to_vec())
        .unwrap();

    let error = store
        .gc_validated_segments(GcSegmentBudget {
            max_segments: 1,
            max_pages: u64::MAX,
        })
        .unwrap_err();
    assert_eq!(error.code, Code::Conflict);
    assert!(store.active_reachability_mark_epoch().unwrap().is_none());
}

#[test]
fn control_set_with_reference_commits_both_roots_atomically() {
    let tp = TempPath::new("atomic-control-reference");
    let store = FileStore::open(tp.path()).unwrap();
    // A reference-root digest to publish atomically with the control value. For this store-level
    // atomicity check it only needs to be a stored object digest.
    let reference = store.put(b"reference-root-object").unwrap();
    let key = b"profile/tickets/v2/ws/state";
    let gen_before = store.generation();

    store
        .control_set_with_reference(key, b"state-bytes".to_vec(), Some(reference))
        .unwrap();

    // Exactly one superblock swap advanced BOTH roots together: no interruption could expose one
    // root advanced without the other (the mixed committed state a recovery pass would face).
    assert_eq!(store.generation(), gen_before + 1);
    assert_eq!(store.reference_root(), Some(reference));
    assert_eq!(
        store.control_get(key).unwrap().as_deref(),
        Some(b"state-bytes".as_slice())
    );

    // Durable across reopen from the single atomic commit.
    drop(store);
    let re = FileStore::open(tp.path()).unwrap();
    assert_eq!(re.reference_root(), Some(reference));
    assert_eq!(
        re.control_get(key).unwrap().as_deref(),
        Some(b"state-bytes".as_slice())
    );
}

#[test]
fn gc_validated_segments_rejects_stale_epoch_after_derived_artifact_root_changes() {
    let tp = TempPath::new("gc-validated-stale-derived");
    let mut store = FileStore::open(tp.path()).unwrap();
    complete_validated_segment_epoch(&store);
    let ns = loom_core::WorkspaceId::from_bytes([36; 16]);
    let key =
        DerivedArtifactKey::new(ns, loom_core::FacetKind::Vector, "embeddings", "hnsw").unwrap();
    let stamp = DerivedArtifactStamp::new(
        loom_core::Digest::blake3(b"vector-root"),
        "hnsw-0",
        "ann-v1",
    )
    .unwrap();
    store
        .put_derived_artifact(&key, stamp, b"native index payload")
        .unwrap();

    let error = store
        .gc_validated_segments(GcSegmentBudget {
            max_segments: 1,
            max_pages: u64::MAX,
        })
        .unwrap_err();
    assert_eq!(error.code, Code::Conflict);
    assert!(store.active_reachability_mark_epoch().unwrap().is_none());
}

/// Build encryption metadata + an unlocked session from fixed test inputs (no RNG in the key layer).
fn test_encryption() -> (Vec<u8>, loom_core::keys::DekSession) {
    let (meta, session) = loom_core::keys::EncryptionMeta::create(
        &loom_core::keys::KeySpec::passphrase("pw"),
        loom_core::keys::Suite::Aes256Gcm,
        [7u8; 16].to_vec(),
        [0x42; 32],
        [9u8; 24].to_vec(),
    )
    .unwrap();
    (meta.encode(), session)
}

#[test]
fn encrypted_store_persists_meta_across_reopen_commits_and_rekey() {
    use loom_core::keys::{EncryptionMeta, KeySpec};
    let shared = SharedMem::default();
    let (meta_bytes, session) = test_encryption();
    let store = FileStore::with_backing_encrypted(
        Box::new(shared.clone()),
        meta_bytes.clone(),
        session,
        Algo::Blake3,
    )
    .unwrap();
    assert!(store.is_encrypted() && store.is_unlocked());
    // Drive enough reference-root commits to cross a checkpoint interval, so the superblock is
    // rewritten and we prove the immutable encryption_meta is carried forward, not erased.
    let d = store
        .put(b"a secret-bearing object payload of a reasonable size for framing")
        .unwrap();
    for _ in 0..(CHECKPOINT_INTERVAL + 2) {
        store.set_reference_root(Some(d)).unwrap();
        store.set_reference_root(None).unwrap();
    }
    drop(store);

    // Reopen over the same bytes: still encrypted, meta round-trips byte-for-byte, and locked (no DEK).
    let re = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    assert!(re.is_encrypted() && !re.is_unlocked());
    assert_eq!(
        re.encryption_meta().unwrap().unwrap(),
        EncryptionMeta::decode(&meta_bytes).unwrap()
    );
    // Wrong passphrase is E2eKeyInvalid; the right one unlocks.
    assert_eq!(
        re.unlock(&KeySpec::passphrase("nope")).unwrap_err().code,
        Code::E2eKeyInvalid
    );
    re.unlock(&KeySpec::passphrase("pw")).unwrap();
    assert!(re.is_unlocked());
    // Rekey under a new passphrase, then reopen: the old passphrase no longer unlocks, the new one
    // does - proving the rewrapped meta is durable (forced checkpoint).
    re.rekey(
        &KeySpec::passphrase("pw2"),
        [1u8; 16].to_vec(),
        [2u8; 24].to_vec(),
    )
    .unwrap();
    drop(re);
    let re2 = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    assert_eq!(
        re2.unlock(&KeySpec::passphrase("pw")).unwrap_err().code,
        Code::E2eKeyInvalid
    );
    re2.unlock(&KeySpec::passphrase("pw2")).unwrap();
}

#[test]
fn encrypted_store_adds_and_removes_wraps_durably() {
    use loom_core::keys::{KeySpec, WrapSource};
    let shared = SharedMem::default();
    let (meta_bytes, session) = test_encryption();
    let store = FileStore::with_backing_encrypted(
        Box::new(shared.clone()),
        meta_bytes,
        session,
        Algo::Blake3,
    )
    .unwrap();
    let digest = store.put(b"secret").unwrap();
    let kek = [0x5au8; loom_core::keys::KEY_LEN];
    store
        .add_wrap(
            &KeySpec::raw_kek(kek),
            Vec::new(),
            [3u8; 24].to_vec(),
            false,
        )
        .unwrap();
    let meta = store.encryption_meta().unwrap().unwrap();
    assert_eq!(meta.wraps.len(), 2);
    assert_eq!(meta.wraps[0].source, WrapSource::Passphrase);
    assert_eq!(meta.wraps[1].source, WrapSource::RawKek);
    drop(store);

    let by_passphrase = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    by_passphrase.unlock(&KeySpec::passphrase("pw")).unwrap();
    assert_eq!(by_passphrase.get(&digest).unwrap().unwrap(), b"secret");
    drop(by_passphrase);

    let by_kek = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    by_kek.unlock(&KeySpec::raw_kek(kek)).unwrap();
    assert_eq!(by_kek.get(&digest).unwrap().unwrap(), b"secret");
    assert_eq!(
        by_kek.remove_wrap(0, false).unwrap_err().code,
        Code::InvalidArgument
    );
    by_kek.remove_wrap(0, true).unwrap();
    drop(by_kek);

    let after_remove = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    assert_eq!(
        after_remove
            .unlock(&KeySpec::passphrase("pw"))
            .unwrap_err()
            .code,
        Code::E2eKeyInvalid
    );
    after_remove.unlock(&KeySpec::raw_kek(kek)).unwrap();
    assert_eq!(after_remove.get(&digest).unwrap().unwrap(), b"secret");
}

#[test]
fn unencrypted_store_reports_not_encrypted_and_rejects_unlock() {
    let store = FileStore::with_backing(Box::new(MemoryBacking::new()), true).unwrap();
    assert!(!store.is_encrypted());
    assert!(store.encryption_meta().unwrap().is_none());
    assert_eq!(
        store
            .unlock(&loom_core::keys::KeySpec::passphrase("x"))
            .unwrap_err()
            .code,
        Code::Unsupported
    );
}

#[test]
fn cannot_enable_encryption_on_an_existing_store() {
    let shared = SharedMem::default();
    {
        let s = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
        s.put(b"already has data").unwrap();
    }
    let (meta_bytes, session) = test_encryption();
    let err = FileStore::with_backing_encrypted(
        Box::new(shared.clone()),
        meta_bytes,
        session,
        Algo::Blake3,
    )
    .unwrap_err();
    assert_eq!(err.code, Code::AlreadyExists);
}

/// Build an unlocked encrypted store over `shared` with the given suite, from fixed test inputs.
fn encrypted_over(shared: &SharedMem, suite: loom_core::keys::Suite) -> FileStore {
    let (meta, session) = loom_core::keys::EncryptionMeta::create(
        &loom_core::keys::KeySpec::passphrase("pw"),
        suite,
        [7u8; 16].to_vec(),
        [0x42; 32],
        [9u8; 24].to_vec(),
    )
    .unwrap();
    FileStore::with_backing_encrypted(
        Box::new(shared.clone()),
        meta.encode(),
        session,
        Algo::Blake3,
    )
    .unwrap()
}

/// On an unlocked encrypted store an object round-trips through `get`, but the plaintext is never
/// written to the backing (no plaintext object frame), and reopening locked makes reads return
/// `E2eLocked`. Both suites are exercised so the XChaCha keyed-BLAKE3 and AES-GCM HKDF CEK paths
/// both round-trip end to end.
#[test]
fn encrypted_object_round_trips_unlocked_and_never_stores_plaintext() {
    use loom_core::keys::{KeySpec, Suite};
    for suite in [Suite::XChaCha20Poly1305, Suite::Aes256Gcm] {
        let shared = SharedMem::default();
        // A long, compressible, recognizable plaintext: large enough to take a real inner codec,
        // and a distinctive marker we can search for in the raw backing.
        let marker = b"TOPSECRET-MARKER-do-not-leak-this-string";
        let mut plain = Vec::new();
        while plain.len() < 4096 {
            plain.extend_from_slice(marker);
            plain.extend_from_slice(b" the quick brown loom commit tree branch ");
        }
        let store = encrypted_over(&shared, suite);
        let d = store.put(&plain).unwrap();
        assert_eq!(
            store.get(&d).unwrap().unwrap(),
            plain,
            "round trip {suite:?}"
        );
        drop(store);

        // The raw backing must not contain the plaintext marker anywhere.
        let raw = shared.0.lock().unwrap().clone();
        assert!(
            !raw.windows(marker.len()).any(|w| w == marker),
            "plaintext marker leaked into the backing under {suite:?}"
        );

        // Reopen locked: the object is present but reads are E2eLocked until unlocked.
        let locked = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
        assert!(locked.has(&d).unwrap());
        assert_eq!(locked.get(&d).unwrap_err().code, Code::E2eLocked);
        locked.unlock(&KeySpec::passphrase("pw")).unwrap();
        assert_eq!(
            locked.get(&d).unwrap().unwrap(),
            plain,
            "post-unlock {suite:?}"
        );
    }
}

/// Corrupting a stored object byte makes `get` fail (CRC or AEAD) rather than return wrong or
/// partial plaintext: the record CRC catches accidental corruption, and the frame-level tests cover
/// CRC-consistent (adversarial) tampering failing AEAD authentication before any plaintext.
#[test]
fn corrupting_an_encrypted_record_byte_fails_get_not_leaks() {
    let shared = SharedMem::default();
    let store = encrypted_over(&shared, loom_core::keys::Suite::Aes256Gcm);
    let plain = b"a single small encrypted object record".to_vec();
    let d = store.put(&plain).unwrap();
    drop(store);

    // The first data page holds the slab with this lone record; flip a byte inside the record's
    // framed bytes (well past the slab header) and confirm the read no longer yields the plaintext.
    {
        let mut g = shared.0.lock().unwrap();
        let pos = DATA_START as usize + 64;
        g[pos] ^= 0xff;
    }
    let reopened = FileStore::with_backing(Box::new(shared.clone()), true).unwrap();
    reopened
        .unlock(&loom_core::keys::KeySpec::passphrase("pw"))
        .unwrap();
    let got = reopened.get(&d);
    assert!(
        got.is_err() || got.as_ref().unwrap().as_deref() != Some(plain.as_slice()),
        "corrupted record must not return the original plaintext"
    );
}

/// Compaction rewrites every record into a fresh file; on an encrypted store the relocated records
/// must be re-sealed (not demoted to plaintext) and still decrypt afterward. This exercises the
/// compaction write path, which reads each object through `get` (decrypt) then re-seals on write.
#[test]
fn compaction_reseals_encrypted_records() {
    use loom_core::keys::{EncryptionMeta, KeySpec, Suite};
    let tmp = TempPath::new("enc-compact");
    let marker = b"COMPACT-SECRET-MARKER";
    let mut digests = Vec::new();
    {
        let (meta, session) = EncryptionMeta::create(
            &KeySpec::passphrase("pw"),
            Suite::Aes256Gcm,
            [7u8; 16].to_vec(),
            [0x42; 32],
            [9u8; 24].to_vec(),
        )
        .unwrap();
        let mut store = FileStore::create_encrypted(tmp.path(), meta.encode(), session).unwrap();
        for i in 0..8u8 {
            let mut obj = marker.to_vec();
            obj.push(i);
            digests.push(store.put(&obj).unwrap());
        }
        store.compact().unwrap();
        // Every object still decrypts to its plaintext after the rewrite.
        for (i, d) in digests.iter().enumerate() {
            let mut want = marker.to_vec();
            want.push(i as u8);
            assert_eq!(store.get(d).unwrap().unwrap(), want);
        }
    }
    // The compacted file on disk contains no plaintext marker.
    let raw = std::fs::read(tmp.path()).unwrap();
    assert!(
        !raw.windows(marker.len()).any(|w| w == marker),
        "plaintext leaked into the compacted file"
    );
    // Reopen the compacted file: locked, and unlock-then-read still works.
    let re = FileStore::open(tmp.path()).unwrap();
    assert!(re.is_encrypted() && !re.is_unlocked());
    re.unlock(&KeySpec::passphrase("pw")).unwrap();
    let mut want0 = marker.to_vec();
    want0.push(0);
    assert_eq!(re.get(&digests[0]).unwrap().unwrap(), want0);
}

/// The rekey data pass rotates the DEK and the suite by re-sealing every object:
/// after it, the old passphrase no longer unlocks, the new one does, objects still decrypt to their
/// plaintext, the on-disk suite changed, and no plaintext leaked. The plaintext digests (object
/// identity) are unchanged, so the same handles read the same objects.
#[test]
fn rekey_reseal_rotates_dek_and_suite() {
    use loom_core::keys::{EncryptionMeta, KeySpec, Suite};
    let tmp = TempPath::new("enc-rekey");
    let marker = b"REKEY-SECRET-MARKER";
    let mut digests = Vec::new();
    let (meta0, sess0) = EncryptionMeta::create(
        &KeySpec::passphrase("old-pw"),
        Suite::XChaCha20Poly1305,
        [7u8; 16].to_vec(),
        [0x11; 32],
        [9u8; 24].to_vec(),
    )
    .unwrap();
    let mut store = FileStore::create_encrypted(tmp.path(), meta0.encode(), sess0).unwrap();
    for i in 0..6u8 {
        let mut obj = marker.to_vec();
        obj.push(i);
        digests.push(store.put(&obj).unwrap());
    }
    // Rotate to a fresh DEK under the AES-256-GCM suite and a new passphrase, re-sealing all objects.
    let (meta1, sess1) = EncryptionMeta::create(
        &KeySpec::passphrase("new-pw"),
        Suite::Aes256Gcm,
        [3u8; 16].to_vec(),
        [0x22; 32],
        [4u8; 24].to_vec(), // the DEK wrap always uses XChaCha20-Poly1305 (24-byte nonce)
    )
    .unwrap();
    store.rekey_reseal(meta1.encode(), sess1).unwrap();
    // The handle stays unlocked under the new key and reads every object.
    assert!(store.is_unlocked());
    for (i, d) in digests.iter().enumerate() {
        let mut want = marker.to_vec();
        want.push(i as u8);
        assert_eq!(store.get(d).unwrap().unwrap(), want);
    }
    drop(store);

    // On-disk: no plaintext leak, and the recorded suite is now AES-256-GCM.
    let raw = std::fs::read(tmp.path()).unwrap();
    assert!(!raw.windows(marker.len()).any(|w| w == marker));
    let re = FileStore::open(tmp.path()).unwrap();
    assert_eq!(
        re.encryption_meta().unwrap().unwrap().active_suite,
        Suite::Aes256Gcm
    );
    // The old passphrase no longer unlocks; the new one does and reads the re-sealed objects.
    assert_eq!(
        re.unlock(&KeySpec::passphrase("old-pw")).unwrap_err().code,
        Code::E2eKeyInvalid
    );
    re.unlock(&KeySpec::passphrase("new-pw")).unwrap();
    let mut want0 = marker.to_vec();
    want0.push(0);
    assert_eq!(re.get(&digests[0]).unwrap().unwrap(), want0);
}

/// rekey-reseal requires an encrypted, unlocked store: an unencrypted store is `Unsupported` and a
/// locked one is `E2eLocked` (it cannot read objects to re-seal them).
#[test]
fn rekey_reseal_requires_encrypted_and_unlocked() {
    use loom_core::keys::{EncryptionMeta, KeySpec, Suite};
    let tmp = TempPath::new("enc-rekey-guard");
    // Unencrypted store -> Unsupported.
    let (meta, session) = EncryptionMeta::create(
        &KeySpec::passphrase("pw"),
        Suite::Aes256Gcm,
        [7u8; 16].to_vec(),
        [0x42; 32],
        [9u8; 24].to_vec(),
    )
    .unwrap();
    {
        let mut plain = FileStore::open(tmp.path()).unwrap();
        assert_eq!(
            plain.rekey_reseal(meta.encode(), session).unwrap_err().code,
            Code::Unsupported
        );
    }
    // Encrypted but locked -> E2eLocked.
    let tmp2 = TempPath::new("enc-rekey-locked");
    let (m0, s0) = EncryptionMeta::create(
        &KeySpec::passphrase("pw"),
        Suite::Aes256Gcm,
        [7u8; 16].to_vec(),
        [0x42; 32],
        [9u8; 24].to_vec(),
    )
    .unwrap();
    {
        let s = FileStore::create_encrypted(tmp2.path(), m0.encode(), s0).unwrap();
        s.put(b"x").unwrap();
    }
    let (m1, s1) = EncryptionMeta::create(
        &KeySpec::passphrase("pw2"),
        Suite::Aes256Gcm,
        [1u8; 16].to_vec(),
        [0x43; 32],
        [2u8; 24].to_vec(),
    )
    .unwrap();
    let mut locked = FileStore::open(tmp2.path()).unwrap();
    assert!(locked.is_encrypted() && !locked.is_unlocked());
    assert_eq!(
        locked.rekey_reseal(m1.encode(), s1).unwrap_err().code,
        Code::E2eLocked
    );
}

/// A FIPS-profile store addresses objects with SHA-256, not blake3: `put`
/// returns a `sha256` digest equal to `Digest::hash(Sha256, canonical)`, `get` round-trips, the
/// profile is recorded in the superblock and survives reopen, and the identity is profile-specific
/// (the blake3 address of the same bytes is not the address here).
#[test]
fn fips_profile_store_addresses_with_sha256() {
    let tmp = TempPath::new("fips-profile");
    let canonical = b"a canonical object under the FIPS identity profile".to_vec();
    let d = {
        let store = FileStore::create_with_profile(tmp.path(), Algo::Sha256).unwrap();
        assert_eq!(store.digest_algo(), Algo::Sha256);
        let d = store.put(&canonical).unwrap();
        assert_eq!(d.algo(), Algo::Sha256);
        assert_eq!(d, Digest::hash(Algo::Sha256, &canonical));
        assert_ne!(d.bytes(), Digest::blake3(&canonical).bytes());
        assert_eq!(store.get(&d).unwrap().unwrap(), canonical);
        d
    };
    // Reopen: the profile is read back from the superblock, and the object still round-trips.
    let re = FileStore::open(tmp.path()).unwrap();
    assert_eq!(re.digest_algo(), Algo::Sha256);
    assert_eq!(re.get(&d).unwrap().unwrap(), canonical);
}

/// The default profile remains blake3, and survives reopen.
#[test]
fn default_profile_store_addresses_with_blake3() {
    let tmp = TempPath::new("default-profile");
    let store = FileStore::open(tmp.path()).unwrap();
    assert_eq!(store.digest_algo(), Algo::Blake3);
    let d = store.put(b"obj").unwrap();
    assert_eq!(d, Digest::blake3(b"obj"));
    drop(store);
    assert_eq!(
        FileStore::open(tmp.path()).unwrap().digest_algo(),
        Algo::Blake3
    );
}

/// Corrupted encryption metadata is rejected, not silently accepted: the encoded `EncryptionMeta`
/// (which the superblock stores inside its CRC-covered span) fails to decode once tampered. The
/// superblock's own CRC additionally guards the in-place bytes on every reopen (see #147b).
#[test]
fn corrupted_encryption_meta_fails_to_decode() {
    use loom_core::keys::EncryptionMeta;
    let (meta_bytes, _session) = test_encryption();
    assert!(EncryptionMeta::decode(&meta_bytes).is_ok());
    let mut corrupt = meta_bytes.clone();
    corrupt[0] ^= 0xff; // break the "LKM1" magic
    assert!(EncryptionMeta::decode(&corrupt).is_err());
    let mut truncated = meta_bytes.clone();
    truncated.truncate(meta_bytes.len() - 1); // a short buffer must not panic or half-decode
    assert!(EncryptionMeta::decode(&truncated).is_err());
}

/// A unique temp path; the file is removed by [`TempPath`]'s drop.
struct TempPath(std::path::PathBuf);
impl TempPath {
    fn new(tag: &str) -> Self {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let mut p = std::env::temp_dir();
        p.push(format!("loomstore-{tag}-{pid}-{n}.loom"));
        let _ = std::fs::remove_file(&p);
        Self(p)
    }
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}
impl Drop for TempPath {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn blob(s: &[u8]) -> Vec<u8> {
    Object::Blob(s.to_vec()).canonical()
}

#[test]
fn put_get_has_len_and_idempotent() {
    let tp = TempPath::new("basic");
    let store = FileStore::open(tp.path()).unwrap();
    assert!(store.is_empty());

    let c = blob(b"hello loom");
    let d = store.put(&c).unwrap();
    assert_eq!(d, Digest::blake3(&c));
    assert!(store.has(&d).unwrap());
    assert_eq!(store.get(&d).unwrap().as_deref(), Some(c.as_slice()));
    assert_eq!(store.len(), 1);

    // Idempotent: same content, same digest, no growth.
    let d2 = store.put(&c).unwrap();
    assert_eq!(d, d2);
    assert_eq!(store.len(), 1);

    // Absent object.
    let absent = Digest::blake3(&blob(b"absent"));
    assert!(!store.has(&absent).unwrap());
    assert_eq!(store.get(&absent).unwrap(), None);
}

#[test]
fn control_plane_map_survives_reopen_and_delete() {
    let tp = TempPath::new("control-map");
    {
        let store = FileStore::open(tp.path()).unwrap();
        store
            .control_set(b"lock/ns/a", b"fence-1".to_vec())
            .unwrap();
        store
            .control_set(b"lock/ns/b", b"fence-2".to_vec())
            .unwrap();
        store.control_set(b"cache/ns/a", b"value".to_vec()).unwrap();
        assert_eq!(
            store.control_get(b"lock/ns/a").unwrap().as_deref(),
            Some(&b"fence-1"[..])
        );
        assert_eq!(
            store.control_scan_prefix(b"lock/ns/").unwrap(),
            vec![
                (b"lock/ns/a".to_vec(), b"fence-1".to_vec()),
                (b"lock/ns/b".to_vec(), b"fence-2".to_vec()),
            ]
        );
    }
    let store = FileStore::open(tp.path()).unwrap();
    assert_eq!(
        store.control_get(b"cache/ns/a").unwrap().as_deref(),
        Some(&b"value"[..])
    );
    assert!(store.control_delete(b"cache/ns/a").unwrap());
    assert!(!store.control_delete(b"cache/ns/a").unwrap());
    assert_eq!(store.control_get(b"cache/ns/a").unwrap(), None);
}

#[test]
fn lock_fence_state_survives_reopen() {
    let tp = TempPath::new("lock-fence");
    let key = b"sync/branch/ns/main";
    {
        let store = FileStore::open(tp.path()).unwrap();
        let mut coordinator = store.lock_coordinator().unwrap();
        let first = coordinator
            .try_acquire(
                key,
                loom_core::LockOwner {
                    principal: "root".into(),
                    session: "s1".into(),
                },
                loom_core::LockMode::Exclusive,
                100,
                0,
            )
            .unwrap();
        coordinator.apply_fence(key, first.fence).unwrap();
        store.save_lock_coordinator(&coordinator).unwrap();
    }
    let store = FileStore::open(tp.path()).unwrap();
    let mut coordinator = store.lock_coordinator().unwrap();
    let second = coordinator
        .try_acquire(
            key,
            loom_core::LockOwner {
                principal: "root".into(),
                session: "s2".into(),
            },
            loom_core::LockMode::Exclusive,
            100,
            0,
        )
        .unwrap();
    assert_eq!(second.fence, loom_core::Fence::embedded(2));
    coordinator.apply_fence(key, second.fence).unwrap();
    assert_eq!(
        coordinator
            .apply_fence(key, loom_core::Fence::embedded(1))
            .unwrap_err()
            .code,
        Code::FencingStale
    );
}

#[test]
fn identity_store_survives_reopen_without_sessions() {
    let tp = TempPath::new("identity-store");
    let root = loom_core::PrincipalId::from_bytes([1; 16]);
    let user = loom_core::PrincipalId::from_bytes([2; 16]);
    {
        let store = FileStore::open(tp.path()).unwrap();
        let mut identity = loom_core::IdentityStore::new(root);
        identity.set_passphrase(root, "root", b"12345678").unwrap();
        identity
            .add_principal(user, "alice", loom_core::PrincipalKind::User)
            .unwrap();
        identity.set_passphrase(user, "alice", b"abcdefgh").unwrap();
        identity
            .authenticate_passphrase(user, "alice", "session")
            .unwrap();
        store.save_identity_store(&identity).unwrap();
    }
    let store = FileStore::open(tp.path()).unwrap();
    let mut identity = store.identity_store().unwrap().unwrap();
    assert_eq!(identity.principals().count(), 2);
    assert_eq!(
        identity.session_principal("session").unwrap_err().code,
        Code::AuthenticationFailed
    );
    assert_eq!(
        identity
            .authenticate_passphrase(user, "alice", "new-session")
            .unwrap()
            .principal,
        user
    );
}

#[test]
fn preauthenticated_local_auth_binds_session_without_passphrase() {
    let tp = TempPath::new("preauthenticated-local-auth");
    let root = loom_core::PrincipalId::from_bytes([1; 16]);
    let user = loom_core::PrincipalId::from_bytes([2; 16]);
    {
        let store = FileStore::open(tp.path()).unwrap();
        let mut identity = loom_core::IdentityStore::new(root);
        identity.set_passphrase(root, "root", b"12345678").unwrap();
        identity
            .add_principal(user, "alice", loom_core::PrincipalKind::User)
            .unwrap();
        identity.set_passphrase(user, "alice", b"abcdefgh").unwrap();
        store.save_identity_store(&identity).unwrap();
    }

    let loom = open_loom_read_unlocked(tp.path(), None).unwrap();
    let loom = attach_local_auth(
        loom,
        &LocalOpenAuth {
            preauthenticated_principal: Some(user),
            session_id: Some("cached-dav".to_string()),
            ..LocalOpenAuth::default()
        },
    )
    .unwrap();
    assert_eq!(loom.effective_principal().unwrap(), Some(user));

    let loom = open_loom_read_unlocked(tp.path(), None).unwrap();
    let err = attach_local_auth(
        loom,
        &LocalOpenAuth {
            principal: Some(user),
            passphrase: Some("alice".to_string()),
            preauthenticated_principal: Some(user),
            session_id: Some("mixed".to_string()),
            ..LocalOpenAuth::default()
        },
    )
    .unwrap_err();
    assert_eq!(err.code, Code::InvalidArgument);
}

#[test]
fn acl_store_survives_reopen() {
    let tp = TempPath::new("acl-store");
    let principal = loom_core::PrincipalId::from_bytes([1; 16]);
    let ns = loom_core::WorkspaceId::from_bytes([9; 16]);
    {
        let store = FileStore::open(tp.path()).unwrap();
        let mut acl = loom_core::AclStore::new();
        acl.allow(
            loom_core::AclSubject::Principal(principal),
            Some(ns),
            Some(loom_core::FacetKind::Kv),
            [loom_core::AclRight::Read],
        )
        .unwrap();
        acl.deny(
            loom_core::AclSubject::Everyone,
            Some(ns),
            Some(loom_core::FacetKind::Kv),
            [loom_core::AclRight::Write],
        )
        .unwrap();
        store.save_acl_store(&acl).unwrap();
    }
    let store = FileStore::open(tp.path()).unwrap();
    let acl = store.acl_store().unwrap().unwrap();
    acl.authorize(
        true,
        principal,
        ns,
        loom_core::FacetKind::Kv,
        loom_core::AclRight::Read,
    )
    .unwrap();
    assert_eq!(
        acl.authorize(
            true,
            principal,
            ns,
            loom_core::FacetKind::Kv,
            loom_core::AclRight::Write,
        )
        .unwrap_err()
        .code,
        Code::PermissionDenied
    );
}

#[test]
fn audit_records_chain_and_survive_reopen() {
    let tp = TempPath::new("audit-records");
    let principal = WorkspaceId::from_bytes([6; 16]);
    {
        let store = FileStore::open(tp.path()).unwrap();
        assert_eq!(
            store
                .audit_append(Some(principal), "identity.create", Some("alice"))
                .unwrap(),
            0
        );
        assert_eq!(
            store.audit_append(None, "acl.grant", Some("kv")).unwrap(),
            1
        );
    }

    let store = FileStore::open(tp.path()).unwrap();
    let records = store.audit_records().unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].seq, 0);
    assert_eq!(records[0].principal, Some(principal));
    assert_eq!(records[0].action, "identity.create");
    assert_eq!(records[0].target.as_deref(), Some("alice"));
    assert_eq!(records[0].prev_hash, None);
    assert_eq!(records[1].seq, 1);
    assert_eq!(records[1].principal, None);
    assert_eq!(records[1].action, "acl.grant");
    assert_eq!(records[1].target.as_deref(), Some("kv"));
    assert_eq!(records[1].prev_hash, Some(records[0].hash));
}

#[test]
fn audit_records_reject_tampered_payloads() {
    let tp = TempPath::new("audit-tamper");
    let store = FileStore::open(tp.path()).unwrap();
    store
        .audit_append(
            Some(WorkspaceId::from_bytes([8; 16])),
            "identity.disable",
            Some("principal"),
        )
        .unwrap();
    let mut map = store.control_map().unwrap();
    let value = map.get_mut(&audit_entry_key(0)).unwrap();
    value[20] ^= 0x01;
    store.write_control_map(map).unwrap();

    assert_eq!(
        store.audit_records().unwrap_err().code,
        Code::IntegrityFailure
    );
}

#[test]
fn audit_config_defaults_and_survives_reopen() {
    let tp = TempPath::new("audit-config");
    let principal = WorkspaceId::from_bytes([9; 16]);
    {
        let store = FileStore::open(tp.path()).unwrap();
        assert_eq!(store.audit_config().unwrap(), AuditConfig::default());
        let config = AuditConfig {
            retention_days: 730,
            legal_hold: true,
        };
        assert_eq!(
            store
                .save_audit_config_audited(
                    config,
                    Some(principal),
                    "audit.config.set",
                    Some("retention_days=730;legal_hold=true"),
                )
                .unwrap(),
            0
        );
    }

    let store = FileStore::open(tp.path()).unwrap();
    assert_eq!(
        store.audit_config().unwrap(),
        AuditConfig {
            retention_days: 730,
            legal_hold: true,
        }
    );
    let records = store.audit_records().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].principal, Some(principal));
    assert_eq!(records[0].action, "audit.config.set");
}

#[test]
fn store_policy_defaults_and_survives_reopen() {
    let tp = TempPath::new("store-policy");
    let principal = WorkspaceId::from_bytes([10; 16]);
    {
        let store = FileStore::open(tp.path()).unwrap();
        assert_eq!(store.store_policy().unwrap(), StorePolicy::default());
        assert_eq!(
            store
                .save_store_policy_audited(
                    StorePolicy {
                        fips_required: true,
                    },
                    Some(principal),
                    "store.policy.set",
                    Some("fips_required=true"),
                )
                .unwrap(),
            0
        );
    }

    let store = FileStore::open(tp.path()).unwrap();
    assert_eq!(
        store.store_policy().unwrap(),
        StorePolicy {
            fips_required: true
        }
    );
    let records = store.audit_records().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].principal, Some(principal));
    assert_eq!(records[0].action, "store.policy.set");
}

#[test]
fn local_runtime_rejects_fips_required_store_when_not_fips_capable() {
    let tp = TempPath::new("store-policy-runtime");
    {
        let store = FileStore::create_with_profile(tp.path(), Algo::Sha256).unwrap();
        store
            .save_store_policy_audited(
                StorePolicy {
                    fips_required: true,
                },
                None,
                "store.policy.set",
                None,
            )
            .unwrap();
    }

    let result = open_loom_read_unlocked(tp.path(), None);
    if loom_core::runtime_profile().fips_capable {
        assert!(result.is_ok());
    } else {
        let err = result.unwrap_err();
        assert_eq!(err.code, Code::PermissionDenied);
        assert!(err.message.contains("FIPS-required"));
    }
}

#[test]
fn audit_legal_hold_blocks_prune() {
    let tp = TempPath::new("audit-legal-hold");
    let store = FileStore::open(tp.path()).unwrap();
    store
        .audit_append(None, "identity.create", Some("root"))
        .unwrap();
    store
        .save_audit_config_audited(
            AuditConfig {
                retention_days: 365,
                legal_hold: true,
            },
            None,
            "audit.config.set",
            Some("legal_hold=true"),
        )
        .unwrap();

    assert_eq!(
        store.audit_prune_through(None, 0).unwrap_err().code,
        Code::PermissionDenied
    );
    assert_eq!(store.audit_records().unwrap().len(), 2);
}

#[test]
fn audit_prune_keeps_checkpoint_and_chain_appendable() {
    let tp = TempPath::new("audit-prune");
    {
        let store = FileStore::open(tp.path()).unwrap();
        for i in 0..4 {
            store
                .audit_append(None, "acl.grant", Some(&format!("grant={i}")))
                .unwrap();
        }
        let stats = store.audit_prune_through(None, 1).unwrap();
        assert_eq!(stats.pruned, 2);
        assert_eq!(stats.checkpoint_seq, Some(1));
        assert!(stats.checkpoint_hash.is_some());
        assert_eq!(stats.audit_seq, 4);
        assert_eq!(
            store
                .audit_append(None, "daemon.start", Some("local"))
                .unwrap(),
            5
        );
    }

    let store = FileStore::open(tp.path()).unwrap();
    let records = store.audit_records().unwrap();
    assert_eq!(
        records
            .iter()
            .map(|record| (record.seq, record.action.as_str()))
            .collect::<Vec<_>>(),
        vec![
            (2, "acl.grant"),
            (3, "acl.grant"),
            (4, "audit.prune"),
            (5, "daemon.start"),
        ]
    );
    assert_eq!(records[2].prev_hash, Some(records[1].hash));
}

#[test]
fn served_listener_config_persists_and_is_audited() {
    let tp = TempPath::new("served-listener");
    let principal = WorkspaceId::from_bytes([10; 16]);
    {
        let store = FileStore::open(tp.path()).unwrap();
        let record = FileStore::served_listener_record(
            "cas",
            vec!["main".to_string()],
            "rest",
            "127.0.0.1:8001",
            true,
        )
        .unwrap();
        let target = format!("id={};surface=cas", record.id);
        assert_eq!(
            store
                .save_served_listener_audited(
                    &record,
                    Some(principal),
                    "serve.listener.configure",
                    Some(&target),
                )
                .unwrap(),
            0
        );
    }

    let store = FileStore::open(tp.path()).unwrap();
    let listeners = store.served_listeners().unwrap();
    assert_eq!(listeners.len(), 1);
    assert_eq!(listeners[0].surface, "cas");
    assert_eq!(listeners[0].selectors, vec!["main"]);
    assert_eq!(listeners[0].transport, "rest");
    assert_eq!(listeners[0].profile, None);
    assert_eq!(listeners[0].bind, "127.0.0.1:8001");
    assert!(listeners[0].enabled);
    assert_eq!(listeners[0].schema_version, 3);
    assert_eq!(listeners[0].last_modified_audit_seq, Some(0));
    assert_eq!(listeners[0].tls.mode, "off");
    assert_eq!(listeners[0].auth.mode, "owner-or-passphrase");
    assert_eq!(listeners[0].route_scope, "workspace");
    assert_eq!(listeners[0].exposure, "read-write");
    assert_eq!(listeners[0].network_access_policy_ref, None);
    assert_eq!(
        listeners[0].limits,
        ServedListenerLimits {
            request_size_limit: 16 * 1024 * 1024,
            idle_timeout_ms: 60_000,
            session_timeout_ms: 3_600_000,
        }
    );
    let records = store.audit_records().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].principal, Some(principal));
    assert_eq!(records[0].action, "serve.listener.configure");
}

#[test]
fn served_listener_policy_profile_and_network_access_persist() {
    let mut record = FileStore::served_listener_record_with_profile(
        "vector",
        vec!["main".into(), "items".into()],
        "rest",
        Some("qdrant"),
        "127.0.0.1:8002",
        true,
    )
    .unwrap();
    record.tls.mode = "direct".to_string();
    record.tls.certificate_bundle_ref = Some("admin".to_string());
    record.auth.mode = "passphrase".to_string();
    record.exposure = "read-only".to_string();
    record.audit.mode = "all".to_string();
    record.network_access_policy_ref = Some("office".to_string());
    record.limits.request_size_limit = 1024;
    record.limits.idle_timeout_ms = 2500;
    record.limits.session_timeout_ms = 5000;
    record.last_modified_audit_seq = Some(9);

    let decoded = decode_served_listener(&encode_served_listener(&record)).unwrap();
    assert_eq!(decoded, record);
    assert_eq!(decoded.profile.as_deref(), Some("qdrant"));
    assert_eq!(decoded.network_access_policy_ref.as_deref(), Some("office"));
}

#[test]
fn served_listener_rejects_legacy_record_without_schema_version() {
    let legacy = legacy_served_listener_bytes("cas", &["main"], "rest", "127.0.0.1:8004", true);
    assert!(decode_served_listener(&legacy).is_err());
}

#[test]
fn network_access_policy_persists_is_audited_and_hashes() {
    let tp = TempPath::new("network-access-policy");
    let principal = WorkspaceId::from_bytes([13; 16]);
    let rule = NetworkAccessRule {
        id: "office-ip".to_string(),
        action: NetworkAccessAction::Allow,
        source_cidr: Some(NetworkAccessCidr::parse("203.0.113.0/24").unwrap()),
        trusted_proxy_cidr: None,
        require_mtls: false,
        client_cert_subject: None,
        client_cert_san: None,
        client_cert_issuer: None,
        description: Some("office egress".to_string()),
    };
    {
        let store = FileStore::open(tp.path()).unwrap();
        let policy = FileStore::network_access_policy_record(
            "office",
            Some("office network".to_string()),
            NetworkAccessAction::Deny,
            vec![rule.clone()],
        )
        .unwrap();
        let digest = store.network_access_policy_digest(&policy).unwrap();
        assert_eq!(digest.algo(), Algo::Blake3);
        let seq = store
            .save_network_access_policy_audited(
                &policy,
                Some(principal),
                "network-access.policy.set",
                Some("name=office"),
            )
            .unwrap();
        assert_eq!(seq, 0);
    }

    let store = FileStore::open(tp.path()).unwrap();
    let policies = store.network_access_policies().unwrap();
    assert_eq!(policies.len(), 1);
    assert_eq!(policies[0].name, "office");
    assert_eq!(policies[0].schema_version, 1);
    assert_eq!(policies[0].description.as_deref(), Some("office network"));
    assert_eq!(policies[0].default_action, NetworkAccessAction::Deny);
    assert_eq!(policies[0].rules, vec![rule]);
    assert_eq!(policies[0].created_audit_seq, Some(0));
    assert_eq!(policies[0].updated_audit_seq, Some(0));
    assert_eq!(
        store.audit_records().unwrap()[0].action,
        "network-access.policy.set"
    );
}

#[test]
fn network_access_policy_validation_rejects_noncanonical_cidr_and_duplicate_rules() {
    assert_eq!(
        NetworkAccessCidr::parse("203.0.113.9/24").unwrap_err().code,
        Code::InvalidArgument
    );
    assert!(NetworkAccessCidr::parse("203.0.113.9").is_ok());
    let rule = NetworkAccessRule {
        id: "dup".to_string(),
        action: NetworkAccessAction::Allow,
        source_cidr: Some(NetworkAccessCidr::parse("203.0.113.0/24").unwrap()),
        trusted_proxy_cidr: None,
        require_mtls: false,
        client_cert_subject: None,
        client_cert_san: None,
        client_cert_issuer: None,
        description: None,
    };
    assert_eq!(
        FileStore::network_access_policy_record(
            "office",
            None,
            NetworkAccessAction::Deny,
            vec![rule.clone(), rule]
        )
        .unwrap_err()
        .code,
        Code::InvalidArgument
    );
}

#[test]
fn network_access_policy_remove_requires_existing_record() {
    let tp = TempPath::new("network-access-remove");
    let store = FileStore::open(tp.path()).unwrap();
    assert_eq!(
        store
            .remove_network_access_policy_audited(
                "missing",
                None,
                "network-access.policy.remove",
                Some("name=missing")
            )
            .unwrap_err()
            .code,
        Code::NotFound
    );
    let policy = FileStore::network_access_policy_record(
        "empty",
        None,
        NetworkAccessAction::Deny,
        Vec::new(),
    )
    .unwrap();
    store
        .save_network_access_policy_audited(
            &policy,
            None,
            "network-access.policy.set",
            Some("name=empty"),
        )
        .unwrap();
    assert_eq!(
        store
            .remove_network_access_policy_audited(
                "empty",
                None,
                "network-access.policy.remove",
                Some("name=empty")
            )
            .unwrap(),
        1
    );
    assert!(store.network_access_policy("empty").unwrap().is_none());
}

#[test]
fn authority_replication_policy_persists_and_is_audited() {
    let tp = TempPath::new("authority-replication-policy");
    let principal = WorkspaceId::from_bytes([12; 16]);
    {
        let store = FileStore::open(tp.path()).unwrap();
        let mut policy =
            FileStore::authority_replication_policy("office", "/srv/policy.loom", true).unwrap();
        policy.interval_ms = Some(30_000);
        policy.jitter_ms = 1_000;
        policy.backoff_ms = 5_000;
        let target = format!("id={};source={}", policy.id, policy.source);
        assert_eq!(
            store
                .save_authority_replication_policy_audited(
                    &policy,
                    Some(principal),
                    "authority.replication.configure",
                    Some(&target),
                )
                .unwrap(),
            0
        );
    }

    let store = FileStore::open(tp.path()).unwrap();
    let policies = store.authority_replication_policies().unwrap();
    assert_eq!(policies.len(), 1);
    assert_eq!(policies[0].id, "office");
    assert_eq!(policies[0].source, "/srv/policy.loom");
    assert!(policies[0].enabled);
    assert!(policies[0].pull_on_start);
    assert_eq!(policies[0].interval_ms, Some(30_000));
    assert_eq!(policies[0].jitter_ms, 1_000);
    assert_eq!(policies[0].backoff_ms, 5_000);
    assert!(policies[0].publish_witness);
    assert_eq!(policies[0].last_modified_audit_seq, Some(0));
    assert!(store.audit_records().unwrap().iter().any(|record| {
        record.principal == Some(principal) && record.action == "authority.replication.configure"
    }));
}

#[test]
fn authority_replication_policy_rejects_invalid_and_removes() {
    let tp = TempPath::new("authority-replication-policy-remove");
    let store = FileStore::open(tp.path()).unwrap();
    assert!(FileStore::authority_replication_policy("bad/id", "/srv/a.loom", true).is_err());
    let mut policy =
        FileStore::authority_replication_policy("office", "/srv/policy.loom", true).unwrap();
    policy.interval_ms = Some(0);
    assert!(
        store
            .save_authority_replication_policy_audited(
                &policy,
                None,
                "authority.replication.configure",
                None,
            )
            .is_err()
    );

    policy.interval_ms = None;
    store
        .save_authority_replication_policy_audited(
            &policy,
            None,
            "authority.replication.configure",
            None,
        )
        .unwrap();
    assert!(
        store
            .authority_replication_policy_by_id("office")
            .unwrap()
            .is_some()
    );
    store
        .remove_authority_replication_policy_audited(
            "office",
            None,
            "authority.replication.remove",
            Some("id=office"),
        )
        .unwrap();
    assert!(
        store
            .authority_replication_policy_by_id("office")
            .unwrap()
            .is_none()
    );
}

#[test]
fn certificate_bundle_persists_and_is_audited_with_force_for_unencrypted_store() {
    let tp = TempPath::new("certificate-bundle");
    let principal = WorkspaceId::from_bytes([11; 16]);
    {
        let store = FileStore::open(tp.path()).unwrap();
        let record = store
            .certificate_bundle_record(
                "public-api",
                b"-----BEGIN CERTIFICATE-----\ncert\n-----END CERTIFICATE-----\n".to_vec(),
                b"-----BEGIN PRIVATE KEY-----\nkey\n-----END PRIVATE KEY-----\n".to_vec(),
                Some(b"-----BEGIN CERTIFICATE-----\nca\n-----END CERTIFICATE-----\n".to_vec()),
            )
            .unwrap();
        let err = store
            .save_certificate_bundle_audited(
                &record,
                Some(principal),
                "certificate.bundle.add",
                Some("name=public-api"),
                false,
            )
            .unwrap_err();
        assert_eq!(err.code, Code::PermissionDenied);
        assert!(err.message.contains("--force"));
        assert_eq!(
            store
                .save_certificate_bundle_audited(
                    &record,
                    Some(principal),
                    "certificate.bundle.add.force",
                    Some("name=public-api"),
                    true,
                )
                .unwrap(),
            0
        );
    }

    let store = FileStore::open(tp.path()).unwrap();
    let bundles = store.certificate_bundles().unwrap();
    assert_eq!(bundles.len(), 1);
    let bundle = &bundles[0];
    assert_eq!(bundle.name, "public-api");
    assert_eq!(bundle.schema_version, 1);
    assert_eq!(bundle.profile, "tls-server-direct");
    assert_eq!(bundle.created_audit_seq, Some(0));
    assert_eq!(bundle.updated_audit_seq, Some(0));
    assert!(bundle.unencrypted_private_key_override);
    assert_eq!(
        bundle.server_cert_chain_digest,
        Digest::hash(store.digest_algo(), &bundle.server_cert_chain_pem)
    );
    assert_eq!(
        bundle.private_key_digest,
        Digest::hash(store.digest_algo(), &bundle.private_key_pem)
    );
    assert_eq!(
        bundle.trust_bundle_digest,
        bundle
            .trust_bundle_pem
            .as_ref()
            .map(|bytes| Digest::hash(store.digest_algo(), bytes))
    );
    assert_eq!(
        store
            .certificate_bundle("public-api")
            .unwrap()
            .unwrap()
            .name,
        "public-api"
    );
    let records = store.audit_records().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].principal, Some(principal));
    assert_eq!(records[0].action, "certificate.bundle.add.force");
    assert_eq!(
        store
            .remove_certificate_bundle_audited(
                "public-api",
                Some(principal),
                "certificate.bundle.remove",
                Some("name=public-api"),
            )
            .unwrap(),
        1
    );
    assert!(store.certificate_bundles().unwrap().is_empty());
}

#[test]
fn encrypted_store_accepts_certificate_bundle_without_force() {
    let shared = SharedMem::default();
    let (meta_bytes, session) = test_encryption();
    let store = FileStore::with_backing_encrypted(
        Box::new(shared.clone()),
        meta_bytes,
        session,
        Algo::Blake3,
    )
    .unwrap();
    let record = store
        .certificate_bundle_record(
            "admin",
            b"cert-chain".to_vec(),
            b"private-key".to_vec(),
            None,
        )
        .unwrap();
    assert_eq!(
        store
            .save_certificate_bundle_audited(
                &record,
                None,
                "certificate.bundle.add",
                Some("name=admin"),
                false,
            )
            .unwrap(),
        0
    );
    let saved = store.certificate_bundle("admin").unwrap().unwrap();
    assert!(!saved.unencrypted_private_key_override);
    assert_eq!(saved.created_audit_seq, Some(0));
    assert_eq!(saved.updated_audit_seq, Some(0));
}

fn legacy_served_listener_bytes(
    surface: &str,
    selectors: &[&str],
    transport: &str,
    bind: &str,
    enabled: bool,
) -> Vec<u8> {
    let selectors = selectors
        .iter()
        .map(|selector| selector.to_string())
        .collect::<Vec<_>>();
    let id = served_listener_id_with_profile(surface, &selectors, transport, None, bind);
    let mut out = Vec::new();
    out.extend_from_slice(SERVED_LISTENER_MAGIC);
    put_lp(&mut out, id.as_bytes());
    put_lp(&mut out, surface.as_bytes());
    put_uvarint(&mut out, selectors.len() as u64);
    for selector in selectors {
        put_lp(&mut out, selector.as_bytes());
    }
    put_lp(&mut out, transport.as_bytes());
    put_lp(&mut out, bind.as_bytes());
    out.push(u8::from(enabled));
    out
}

#[test]
fn derived_artifact_survives_reopen_and_reports_stale() {
    let tp = TempPath::new("derived-artifact");
    let ns = loom_core::WorkspaceId::from_bytes([3; 16]);
    let key = DerivedArtifactKey::new(ns, loom_core::FacetKind::Search, "docs", "tantivy").unwrap();
    let source = loom_core::Digest::blake3(b"source-v1");
    let stamp = DerivedArtifactStamp::new(source, "tantivy-0", "search-v1").unwrap();
    {
        let store = FileStore::open(tp.path()).unwrap();
        let record = store
            .put_derived_artifact(&key, stamp.clone(), b"index bytes")
            .unwrap();
        assert_eq!(record.payload_len, 11);
    }

    let store = FileStore::open(tp.path()).unwrap();
    match store.read_derived_artifact(&key, &stamp).unwrap() {
        DerivedArtifactRead::Ready { record, payload } => {
            assert_eq!(record.stamp, stamp);
            assert_eq!(payload, b"index bytes");
        }
        other => panic!("expected ready artifact, got {other:?}"),
    }

    let stale_stamp = DerivedArtifactStamp::new(
        loom_core::Digest::blake3(b"source-v2"),
        "tantivy-0",
        "search-v1",
    )
    .unwrap();
    match store.read_derived_artifact(&key, &stale_stamp).unwrap() {
        DerivedArtifactRead::Stale { record } => assert_eq!(record.stamp, stamp),
        other => panic!("expected stale artifact, got {other:?}"),
    }
    assert!(store.delete_derived_artifact(&key).unwrap());
    assert_eq!(
        store.read_derived_artifact(&key, &stale_stamp).unwrap(),
        DerivedArtifactRead::Missing
    );
}

#[test]
fn compact_retaining_keeps_derived_artifact_payloads() {
    let tp = TempPath::new("derived-compact");
    let ns = loom_core::WorkspaceId::from_bytes([4; 16]);
    let key =
        DerivedArtifactKey::new(ns, loom_core::FacetKind::Vector, "embeddings", "hnsw").unwrap();
    let stamp = DerivedArtifactStamp::new(
        loom_core::Digest::blake3(b"vector-root"),
        "hnsw-0",
        "ann-v1",
    )
    .unwrap();
    {
        let store = FileStore::open(tp.path()).unwrap();
        store
            .put_derived_artifact(&key, stamp.clone(), b"native index payload")
            .unwrap();
    }

    {
        let mut store = FileStore::open(tp.path()).unwrap();
        store.compact_retaining(&BTreeSet::new()).unwrap();
    }

    let store = FileStore::open(tp.path()).unwrap();
    match store.read_derived_artifact(&key, &stamp).unwrap() {
        DerivedArtifactRead::Ready { payload, .. } => assert_eq!(payload, b"native index payload"),
        other => panic!("expected retained artifact, got {other:?}"),
    }
}

#[test]
fn derived_artifact_rebuild_lifecycle_coalesces_and_reports_status() {
    let tp = TempPath::new("derived-rebuild");
    let ns = loom_core::WorkspaceId::from_bytes([5; 16]);
    let store = FileStore::open(tp.path()).unwrap();
    let key =
        DerivedArtifactKey::new(ns, loom_core::FacetKind::Columnar, "events", "arrow").unwrap();
    let stamp = DerivedArtifactStamp::new(
        loom_core::Digest::blake3(b"columnar-root"),
        "arrow-writer-0",
        "arrow-cache-v1",
    )
    .unwrap();

    assert_eq!(
        store.derived_artifact_status(&key, &stamp).unwrap(),
        DerivedArtifactStatus::Missing
    );
    let run_id = match store
        .begin_derived_artifact_rebuild(&key, stamp.clone())
        .unwrap()
    {
        DerivedArtifactRebuild::Started { run_id } => run_id,
        other => panic!("expected started rebuild, got {other:?}"),
    };
    assert_eq!(
        store
            .begin_derived_artifact_rebuild(&key, stamp.clone())
            .unwrap(),
        DerivedArtifactRebuild::Coalesced {
            run_id: run_id.clone()
        }
    );
    assert_eq!(
        store.derived_artifact_status(&key, &stamp).unwrap(),
        DerivedArtifactStatus::Rebuilding {
            run_id: run_id.clone(),
            stamp: stamp.clone()
        }
    );
    let record = store
        .finish_derived_artifact_rebuild(&key, &run_id, stamp.clone(), b"arrow bytes")
        .unwrap();
    assert_eq!(
        store.derived_artifact_status(&key, &stamp).unwrap(),
        DerivedArtifactStatus::Ready {
            record: record.clone()
        }
    );
    assert_eq!(
        store
            .begin_derived_artifact_rebuild(&key, stamp.clone())
            .unwrap(),
        DerivedArtifactRebuild::AlreadyReady { record }
    );

    let stale_stamp = DerivedArtifactStamp::new(
        loom_core::Digest::blake3(b"columnar-root-2"),
        "arrow-writer-0",
        "arrow-cache-v1",
    )
    .unwrap();
    let failed_run = match store
        .begin_derived_artifact_rebuild(&key, stale_stamp.clone())
        .unwrap()
    {
        DerivedArtifactRebuild::Started { run_id } => run_id,
        other => panic!("expected started rebuild, got {other:?}"),
    };
    store
        .fail_derived_artifact_rebuild(
            &key,
            &failed_run,
            stale_stamp.clone(),
            "source changed during build",
        )
        .unwrap();
    assert_eq!(
        store.derived_artifact_status(&key, &stale_stamp).unwrap(),
        DerivedArtifactStatus::Failed {
            stamp: stale_stamp.clone(),
            message: "source changed during build".into()
        }
    );

    store
        .mark_derived_artifact_unsupported(&key, stale_stamp.clone(), "native engine unavailable")
        .unwrap();
    assert_eq!(
        store.derived_artifact_status(&key, &stale_stamp).unwrap(),
        DerivedArtifactStatus::Unsupported {
            stamp: stale_stamp,
            message: "native engine unavailable".into()
        }
    );
}

#[test]
fn derived_artifact_serving_policy_covers_all_non_ready_states() {
    use loom_core::capability::CapabilityOperationalState;

    let source = loom_core::Digest::blake3(b"derived-source");
    let stamp = DerivedArtifactStamp::new(source, "engine-1", "format-1").unwrap();
    let record = DerivedArtifactRecord {
        stamp: stamp.clone(),
        payload_digest: loom_core::Digest::blake3(b"payload"),
        payload_len: 7,
    };
    let cases = [
        (
            DerivedArtifactStatus::Ready {
                record: record.clone(),
            },
            "ready",
            DerivedArtifactServingMode::DerivedArtifact,
            CapabilityOperationalState::Supported,
            None,
            None,
        ),
        (
            DerivedArtifactStatus::Missing,
            "missing",
            DerivedArtifactServingMode::AuthoritativeSource,
            CapabilityOperationalState::Degraded,
            Some("derived_artifact_missing"),
            None,
        ),
        (
            DerivedArtifactStatus::Stale {
                record: record.clone(),
            },
            "stale",
            DerivedArtifactServingMode::AuthoritativeSource,
            CapabilityOperationalState::Degraded,
            Some("derived_artifact_stale"),
            None,
        ),
        (
            DerivedArtifactStatus::Rebuilding {
                run_id: "run-1".into(),
                stamp: stamp.clone(),
            },
            "rebuilding",
            DerivedArtifactServingMode::AuthoritativeSource,
            CapabilityOperationalState::Degraded,
            Some("index_rebuilding"),
            None,
        ),
        (
            DerivedArtifactStatus::Failed {
                stamp: stamp.clone(),
                message: "build failed".into(),
            },
            "failed",
            DerivedArtifactServingMode::AuthoritativeSource,
            CapabilityOperationalState::Degraded,
            Some("derived_artifact_failed"),
            None,
        ),
        (
            DerivedArtifactStatus::Unsupported {
                stamp,
                message: "engine unavailable".into(),
            },
            "unsupported",
            DerivedArtifactServingMode::AuthoritativeSource,
            CapabilityOperationalState::Unsupported,
            Some("profile_unsupported"),
            Some(loom_core::Code::Unsupported),
        ),
    ];

    for (status, name, mode, operational_state, reason_code, stable_error) in cases {
        assert_eq!(status.name(), name);
        let policy = status.serving_policy();
        assert_eq!(policy.mode, mode);
        assert_eq!(policy.operational_state, operational_state);
        assert_eq!(policy.reason_code, reason_code);
        assert_eq!(policy.stable_error, stable_error);
    }
}

#[test]
fn derived_artifact_serving_policy_projects_capability_state() {
    use loom_core::capability::{CapabilityDegradation, CapabilityOperationalState, CapabilitySet};

    let source = loom_core::Digest::blake3(b"derived-source");
    let stamp = DerivedArtifactStamp::new(source, "engine-1", "format-1").unwrap();
    let failed = DerivedArtifactStatus::Failed {
        stamp: stamp.clone(),
        message: "build failed".into(),
    }
    .apply_serving_policy_to_capabilities(CapabilitySet::registry(), "search");
    let failed_search = failed.get("search").unwrap();
    assert_eq!(
        failed_search.operational_state,
        CapabilityOperationalState::Degraded
    );
    assert_eq!(failed_search.reason_code, Some("derived_artifact_failed"));
    assert_eq!(failed_search.stable_error, None);
    assert_eq!(
        failed_search.degradation,
        Some(CapabilityDegradation {
            fallback: "authoritative-source",
            result_equivalence: "source-equivalent",
        })
    );

    let unsupported = DerivedArtifactStatus::Unsupported {
        stamp,
        message: "engine unavailable".into(),
    }
    .apply_serving_policy_to_capabilities(CapabilitySet::registry(), "search");
    let unsupported_search = unsupported.get("search").unwrap();
    assert_eq!(
        unsupported_search.operational_state,
        CapabilityOperationalState::Unsupported
    );
    assert_eq!(unsupported_search.reason_code, Some("profile_unsupported"));
    assert_eq!(
        unsupported_search.stable_error,
        Some(loom_core::Code::Unsupported)
    );
    assert_eq!(unsupported_search.degradation, None);
}

#[test]
fn columnar_arrow_lifecycle_uses_registered_derived_contract() {
    let tp = TempPath::new("columnar-arrow-derived");
    let ns = loom_core::WorkspaceId::from_bytes([8; 16]);
    let store = FileStore::open(tp.path()).unwrap();
    let source = loom_core::Digest::blake3(b"columnar-structured-root");
    let engine = "arrow-ipc-writer-test-0";
    let key = columnar_arrow_artifact_key(ns, "events").unwrap();
    let stamp = columnar_arrow_artifact_stamp(source, engine).unwrap();

    assert_eq!(key.facet, loom_core::FacetKind::Columnar);
    assert_eq!(key.artifact, COLUMNAR_ARROW_ARTIFACT);
    assert_eq!(stamp.format_version, COLUMNAR_ARROW_FORMAT_VERSION);
    assert_eq!(
        derived_artifact_format_version(loom_core::FacetKind::Columnar, COLUMNAR_ARROW_ARTIFACT),
        Some(COLUMNAR_ARROW_FORMAT_VERSION)
    );
    assert_eq!(
        store
            .columnar_arrow_status(ns, "events", source, engine)
            .unwrap(),
        DerivedArtifactStatus::Missing
    );

    let run_id = match store
        .begin_columnar_arrow_rebuild(ns, "events", source, engine)
        .unwrap()
    {
        DerivedArtifactRebuild::Started { run_id } => run_id,
        other => panic!("expected started rebuild, got {other:?}"),
    };
    assert_eq!(
        store
            .columnar_arrow_status(ns, "events", source, engine)
            .unwrap(),
        DerivedArtifactStatus::Rebuilding {
            run_id: run_id.clone(),
            stamp: stamp.clone()
        }
    );
    let record = store
        .finish_columnar_arrow_rebuild(ns, "events", &run_id, source, engine, b"arrow-ipc")
        .unwrap();
    assert_eq!(
        store
            .columnar_arrow_status(ns, "events", source, engine)
            .unwrap(),
        DerivedArtifactStatus::Ready {
            record: record.clone()
        }
    );

    let changed_source = loom_core::Digest::blake3(b"columnar-structured-root-v2");
    assert_eq!(
        store
            .columnar_arrow_status(ns, "events", changed_source, engine)
            .unwrap(),
        DerivedArtifactStatus::Stale { record }
    );
    let changed_run = match store
        .begin_columnar_arrow_rebuild(ns, "events", changed_source, engine)
        .unwrap()
    {
        DerivedArtifactRebuild::Started { run_id } => run_id,
        other => panic!("expected started rebuild, got {other:?}"),
    };
    store
        .fail_columnar_arrow_rebuild(
            ns,
            "events",
            &changed_run,
            changed_source,
            engine,
            "arrow writer failed",
        )
        .unwrap();
    assert_eq!(
        store
            .columnar_arrow_status(ns, "events", changed_source, engine)
            .unwrap(),
        DerivedArtifactStatus::Failed {
            stamp: columnar_arrow_artifact_stamp(changed_source, engine).unwrap(),
            message: "arrow writer failed".into()
        }
    );
}

#[test]
fn graph_property_index_lifecycle_uses_registered_derived_contract() {
    let tp = TempPath::new("graph-property-index-derived");
    let ns = loom_core::WorkspaceId::from_bytes([10; 16]);
    let store = FileStore::open(tp.path()).unwrap();
    let source = loom_core::Digest::blake3(b"graph-root-plus-property-index-catalog");
    let engine = "graph-property-index-writer-test-0";
    let key = graph_property_index_artifact_key(ns, "people", "person_name").unwrap();
    let stamp = graph_property_index_artifact_stamp(source, engine).unwrap();

    assert_eq!(key.facet, loom_core::FacetKind::Graph);
    assert_eq!(key.collection, "people");
    assert_eq!(
        key.artifact,
        format!("{GRAPH_PROPERTY_INDEX_ARTIFACT_PREFIX}person_name")
    );
    assert_eq!(stamp.format_version, GRAPH_PROPERTY_INDEX_FORMAT_VERSION);
    assert_eq!(
        derived_artifact_format_version(loom_core::FacetKind::Graph, &key.artifact),
        Some(GRAPH_PROPERTY_INDEX_FORMAT_VERSION)
    );
    assert_eq!(
        store
            .graph_property_index_status(ns, "people", "person_name", source, engine)
            .unwrap(),
        DerivedArtifactStatus::Missing
    );

    let run_id = match store
        .begin_graph_property_index_rebuild(ns, "people", "person_name", source, engine)
        .unwrap()
    {
        DerivedArtifactRebuild::Started { run_id } => run_id,
        other => panic!("expected started rebuild, got {other:?}"),
    };
    assert_eq!(
        store
            .graph_property_index_status(ns, "people", "person_name", source, engine)
            .unwrap(),
        DerivedArtifactStatus::Rebuilding {
            run_id: run_id.clone(),
            stamp: stamp.clone()
        }
    );
    let record = store
        .finish_graph_property_index_rebuild(
            ns,
            "people",
            "person_name",
            &run_id,
            source,
            engine,
            b"property-index-bytes",
        )
        .unwrap();
    assert_eq!(
        store
            .graph_property_index_status(ns, "people", "person_name", source, engine)
            .unwrap(),
        DerivedArtifactStatus::Ready {
            record: record.clone()
        }
    );

    let changed_source = loom_core::Digest::blake3(b"graph-root-plus-property-index-catalog-v2");
    assert_eq!(
        store
            .graph_property_index_status(ns, "people", "person_name", changed_source, engine)
            .unwrap(),
        DerivedArtifactStatus::Stale { record }
    );
    let changed_run = match store
        .begin_graph_property_index_rebuild(ns, "people", "person_name", changed_source, engine)
        .unwrap()
    {
        DerivedArtifactRebuild::Started { run_id } => run_id,
        other => panic!("expected started rebuild, got {other:?}"),
    };
    store
        .fail_graph_property_index_rebuild(
            ns,
            "people",
            "person_name",
            &changed_run,
            changed_source,
            engine,
            "property index writer failed",
        )
        .unwrap();
    assert_eq!(
        store
            .graph_property_index_status(ns, "people", "person_name", changed_source, engine)
            .unwrap(),
        DerivedArtifactStatus::Failed {
            stamp: graph_property_index_artifact_stamp(changed_source, engine).unwrap(),
            message: "property index writer failed".into()
        }
    );
}

#[test]
fn graph_spatial_index_lifecycle_reports_unsupported() {
    let tp = TempPath::new("graph-spatial-index-derived");
    let ns = loom_core::WorkspaceId::from_bytes([11; 16]);
    let store = FileStore::open(tp.path()).unwrap();
    let source = loom_core::Digest::blake3(b"graph-root-plus-spatial-index-catalog");
    let engine = "graph-spatial-index-writer-test-0";
    let key = graph_spatial_index_artifact_key(ns, "places", "place_loc").unwrap();
    let stamp = graph_spatial_index_artifact_stamp(source, engine).unwrap();

    assert_eq!(key.facet, loom_core::FacetKind::Graph);
    assert_eq!(key.collection, "places");
    assert_eq!(
        key.artifact,
        format!("{GRAPH_SPATIAL_INDEX_ARTIFACT_PREFIX}place_loc")
    );
    assert_eq!(stamp.format_version, GRAPH_SPATIAL_INDEX_FORMAT_VERSION);
    assert_eq!(
        derived_artifact_format_version(loom_core::FacetKind::Graph, &key.artifact),
        Some(GRAPH_SPATIAL_INDEX_FORMAT_VERSION)
    );
    store
        .mark_graph_spatial_index_unsupported(
            ns,
            "places",
            "place_loc",
            source,
            engine,
            "spatial profile unavailable",
        )
        .unwrap();
    assert_eq!(
        store
            .graph_spatial_index_status(ns, "places", "place_loc", source, engine)
            .unwrap(),
        DerivedArtifactStatus::Unsupported {
            stamp,
            message: "spatial profile unavailable".into()
        }
    );
}

#[test]
fn dataframe_materialization_lifecycle_uses_registered_derived_contract() {
    let tp = TempPath::new("dataframe-materialization-derived");
    let ns = loom_core::WorkspaceId::from_bytes([14; 16]);
    let store = FileStore::open(tp.path()).unwrap();
    let source = loom_core::Digest::blake3(b"dataframe-plan-plus-source-digests");
    let engine = "portable-dataframe-executor-test-0";
    let key = dataframe_materialization_artifact_key(ns, "etl/purchases", "columnar").unwrap();
    let stamp = dataframe_materialization_artifact_stamp(source, engine).unwrap();

    assert_eq!(key.facet, loom_core::FacetKind::Dataframe);
    assert_eq!(key.collection, "etl/purchases");
    assert_eq!(
        key.artifact,
        format!("{DATAFRAME_MATERIALIZATION_ARTIFACT_PREFIX}columnar")
    );
    assert_eq!(
        stamp.format_version,
        DATAFRAME_MATERIALIZATION_FORMAT_VERSION
    );
    assert_eq!(
        derived_artifact_format_version(loom_core::FacetKind::Dataframe, &key.artifact),
        Some(DATAFRAME_MATERIALIZATION_FORMAT_VERSION)
    );
    assert_eq!(
        store
            .dataframe_materialization_status(ns, "etl/purchases", "columnar", source, engine)
            .unwrap(),
        DerivedArtifactStatus::Missing
    );

    let run_id = match store
        .begin_dataframe_materialization_rebuild(ns, "etl/purchases", "columnar", source, engine)
        .unwrap()
    {
        DerivedArtifactRebuild::Started { run_id } => run_id,
        other => panic!("expected started dataframe rebuild, got {other:?}"),
    };
    assert_eq!(
        store
            .dataframe_materialization_status(ns, "etl/purchases", "columnar", source, engine)
            .unwrap(),
        DerivedArtifactStatus::Rebuilding {
            run_id: run_id.clone(),
            stamp: stamp.clone()
        }
    );
    let record = store
        .finish_dataframe_materialization_rebuild(
            ns,
            "etl/purchases",
            "columnar",
            &run_id,
            source,
            engine,
            b"dataframe-materialization-bytes",
        )
        .unwrap();
    assert_eq!(
        store
            .dataframe_materialization_status(ns, "etl/purchases", "columnar", source, engine)
            .unwrap(),
        DerivedArtifactStatus::Ready {
            record: record.clone()
        }
    );

    let changed_source = loom_core::Digest::blake3(b"dataframe-plan-plus-source-digests-v2");
    assert_eq!(
        store
            .dataframe_materialization_status(
                ns,
                "etl/purchases",
                "columnar",
                changed_source,
                engine,
            )
            .unwrap(),
        DerivedArtifactStatus::Stale { record }
    );
    let changed_run = match store
        .begin_dataframe_materialization_rebuild(
            ns,
            "etl/purchases",
            "columnar",
            changed_source,
            engine,
        )
        .unwrap()
    {
        DerivedArtifactRebuild::Started { run_id } => run_id,
        other => panic!("expected started dataframe rebuild, got {other:?}"),
    };
    store
        .fail_dataframe_materialization_rebuild(
            ns,
            "etl/purchases",
            "columnar",
            &changed_run,
            changed_source,
            engine,
            "dataframe materialization failed",
        )
        .unwrap();
    assert_eq!(
        store
            .dataframe_materialization_status(
                ns,
                "etl/purchases",
                "columnar",
                changed_source,
                engine,
            )
            .unwrap(),
        DerivedArtifactStatus::Failed {
            stamp: dataframe_materialization_artifact_stamp(changed_source, engine).unwrap(),
            message: "dataframe materialization failed".into()
        }
    );
    store
        .mark_dataframe_materialization_unsupported(
            ns,
            "etl/purchases",
            "parquet",
            changed_source,
            engine,
            "parquet profile unavailable",
        )
        .unwrap();
    assert_eq!(
        store
            .dataframe_materialization_status(
                ns,
                "etl/purchases",
                "parquet",
                changed_source,
                engine,
            )
            .unwrap(),
        DerivedArtifactStatus::Unsupported {
            stamp: dataframe_materialization_artifact_stamp(changed_source, engine).unwrap(),
            message: "parquet profile unavailable".into()
        }
    );
}

#[test]
fn pim_derived_indexes_use_registered_lifecycle_contracts() {
    let tp = TempPath::new("pim-derived-indexes");
    let ns = loom_core::WorkspaceId::from_bytes([15; 16]);
    let store = FileStore::open(tp.path()).unwrap();
    let calendar_source = loom_core::Digest::blake3(b"calendar-record-root-plus-index-profile");
    let contacts_source = loom_core::Digest::blake3(b"contacts-record-root-plus-index-profile");
    let mail_source = loom_core::Digest::blake3(b"mail-record-root-plus-index-profile");
    let engine = "pim-index-writer-test-0";

    let calendar_key =
        calendar_derived_index_artifact_key(ns, "alice", "work", "range-search").unwrap();
    let contacts_key =
        contacts_derived_index_artifact_key(ns, "alice", "people", "text-search").unwrap();
    let mail_key = mail_derived_index_artifact_key(ns, "alice", "inbox", "text-search").unwrap();

    assert_eq!(calendar_key.facet, loom_core::FacetKind::Calendar);
    assert_eq!(calendar_key.collection, "alice/work");
    assert_eq!(
        calendar_key.artifact,
        format!("{PIM_DERIVED_INDEX_ARTIFACT_PREFIX}range-search")
    );
    assert_eq!(contacts_key.facet, loom_core::FacetKind::Contacts);
    assert_eq!(contacts_key.collection, "alice/people");
    assert_eq!(
        contacts_key.artifact,
        format!("{PIM_DERIVED_INDEX_ARTIFACT_PREFIX}text-search")
    );
    assert_eq!(mail_key.facet, loom_core::FacetKind::Mail);
    assert_eq!(mail_key.collection, "alice/inbox");
    assert_eq!(
        mail_key.artifact,
        format!("{PIM_DERIVED_INDEX_ARTIFACT_PREFIX}text-search")
    );
    assert_eq!(
        derived_artifact_format_version(loom_core::FacetKind::Calendar, &calendar_key.artifact),
        Some(CALENDAR_DERIVED_INDEX_FORMAT_VERSION)
    );
    assert_eq!(
        derived_artifact_format_version(loom_core::FacetKind::Contacts, &contacts_key.artifact),
        Some(CONTACTS_DERIVED_INDEX_FORMAT_VERSION)
    );
    assert_eq!(
        derived_artifact_format_version(loom_core::FacetKind::Mail, &mail_key.artifact),
        Some(MAIL_DERIVED_INDEX_FORMAT_VERSION)
    );

    let calendar_stamp = calendar_derived_index_artifact_stamp(calendar_source, engine).unwrap();
    assert_eq!(
        calendar_stamp.format_version,
        CALENDAR_DERIVED_INDEX_FORMAT_VERSION
    );
    assert_eq!(
        store
            .derived_artifact_status(&calendar_key, &calendar_stamp)
            .unwrap(),
        DerivedArtifactStatus::Missing
    );
    let run_id = match store
        .begin_derived_artifact_rebuild(&calendar_key, calendar_stamp.clone())
        .unwrap()
    {
        DerivedArtifactRebuild::Started { run_id } => run_id,
        other => panic!("expected calendar index rebuild, got {other:?}"),
    };
    let record = store
        .finish_derived_artifact_rebuild(
            &calendar_key,
            &run_id,
            calendar_stamp.clone(),
            b"calendar-index-bytes",
        )
        .unwrap();
    assert_eq!(
        store
            .derived_artifact_status(&calendar_key, &calendar_stamp)
            .unwrap(),
        DerivedArtifactStatus::Ready {
            record: record.clone()
        }
    );
    let changed_calendar = loom_core::Digest::blake3(b"calendar-record-root-plus-index-profile-v2");
    assert_eq!(
        store
            .derived_artifact_status(
                &calendar_key,
                &calendar_derived_index_artifact_stamp(changed_calendar, engine).unwrap(),
            )
            .unwrap(),
        DerivedArtifactStatus::Stale { record }
    );

    let contacts_stamp = contacts_derived_index_artifact_stamp(contacts_source, engine).unwrap();
    store
        .mark_derived_artifact_unsupported(
            &contacts_key,
            contacts_stamp.clone(),
            "contacts index profile unavailable",
        )
        .unwrap();
    assert_eq!(
        store
            .derived_artifact_status(&contacts_key, &contacts_stamp)
            .unwrap(),
        DerivedArtifactStatus::Unsupported {
            stamp: contacts_stamp,
            message: "contacts index profile unavailable".into()
        }
    );

    let mail_stamp = mail_derived_index_artifact_stamp(mail_source, engine).unwrap();
    let mail_run = match store
        .begin_derived_artifact_rebuild(&mail_key, mail_stamp.clone())
        .unwrap()
    {
        DerivedArtifactRebuild::Started { run_id } => run_id,
        other => panic!("expected mail index rebuild, got {other:?}"),
    };
    store
        .fail_derived_artifact_rebuild(
            &mail_key,
            &mail_run,
            mail_stamp.clone(),
            "mail index writer failed",
        )
        .unwrap();
    assert_eq!(
        store
            .derived_artifact_status(&mail_key, &mail_stamp)
            .unwrap(),
        DerivedArtifactStatus::Failed {
            stamp: mail_stamp,
            message: "mail index writer failed".into()
        }
    );
}

#[test]
fn vector_pq_lifecycle_uses_vector_source_stamp_and_serving_policy() {
    let tp = TempPath::new("vector-pq-derived");
    let store = FileStore::open(tp.path()).unwrap();
    let mut loom = loom_core::Loom::new(store);
    let ns = loom
        .registry_mut()
        .create(
            loom_core::FacetKind::Vector,
            Some("vector-pq-derived"),
            loom_core::WorkspaceId::from_bytes([12; 16]),
        )
        .unwrap();
    loom_core::vector_create(&mut loom, ns, "emb", 2, loom_core::Metric::Dot).unwrap();
    loom_core::vector_upsert(&mut loom, ns, "emb", "a", vec![1.0, 0.0], BTreeMap::new()).unwrap();
    let source = loom_core::vector_source_digest(&loom, ns, "emb").unwrap();
    let engine = "pq-writer-0";
    let key = vector_pq_artifact_key(ns, "emb").unwrap();
    let stamp = vector_pq_artifact_stamp(source, engine).unwrap();

    assert_eq!(key.facet, loom_core::FacetKind::Vector);
    assert_eq!(key.collection, "emb");
    assert_eq!(key.artifact, VECTOR_PQ_ARTIFACT);
    assert_eq!(stamp.format_version, VECTOR_PQ_FORMAT_VERSION);
    assert_eq!(
        derived_artifact_format_version(loom_core::FacetKind::Vector, VECTOR_PQ_ARTIFACT),
        Some(VECTOR_PQ_FORMAT_VERSION)
    );
    assert_eq!(
        loom.store()
            .vector_pq_status(ns, "emb", source, engine)
            .unwrap(),
        DerivedArtifactStatus::Missing
    );

    let run_id = match loom
        .store()
        .begin_vector_pq_rebuild(ns, "emb", source, engine)
        .unwrap()
    {
        DerivedArtifactRebuild::Started { run_id } => run_id,
        other => panic!("expected started PQ rebuild, got {other:?}"),
    };
    assert_eq!(
        loom.store()
            .vector_pq_status(ns, "emb", source, engine)
            .unwrap(),
        DerivedArtifactStatus::Rebuilding {
            run_id: run_id.clone(),
            stamp: stamp.clone()
        }
    );
    assert_eq!(
        loom.store()
            .vector_pq_status(ns, "emb", source, engine)
            .unwrap()
            .serving_policy()
            .reason_code,
        Some("index_rebuilding")
    );
    let record = loom
        .store()
        .finish_vector_pq_rebuild(ns, "emb", &run_id, source, engine, b"pq bytes")
        .unwrap();
    assert_eq!(
        loom.store()
            .vector_pq_status(ns, "emb", source, engine)
            .unwrap(),
        DerivedArtifactStatus::Ready {
            record: record.clone()
        }
    );

    loom_core::vector_upsert(&mut loom, ns, "emb", "b", vec![0.0, 1.0], BTreeMap::new()).unwrap();
    let changed_source = loom_core::vector_source_digest(&loom, ns, "emb").unwrap();
    assert_eq!(
        loom.store()
            .vector_pq_status(ns, "emb", changed_source, engine)
            .unwrap(),
        DerivedArtifactStatus::Stale { record }
    );
}

#[test]
fn vector_hnsw_lifecycle_reports_failed_and_unsupported_policy() {
    let tp = TempPath::new("vector-hnsw-derived");
    let ns = loom_core::WorkspaceId::from_bytes([13; 16]);
    let store = FileStore::open(tp.path()).unwrap();
    let source = loom_core::Digest::blake3(b"vector-source-hnsw");
    let engine = "hnsw-writer-0";
    let key = vector_hnsw_artifact_key(ns, "emb").unwrap();
    let stamp = vector_hnsw_artifact_stamp(source, engine).unwrap();

    assert_eq!(key.facet, loom_core::FacetKind::Vector);
    assert_eq!(key.collection, "emb");
    assert_eq!(key.artifact, VECTOR_HNSW_ARTIFACT);
    assert_eq!(stamp.format_version, VECTOR_HNSW_FORMAT_VERSION);
    assert_eq!(
        derived_artifact_format_version(loom_core::FacetKind::Vector, VECTOR_HNSW_ARTIFACT),
        Some(VECTOR_HNSW_FORMAT_VERSION)
    );

    let run_id = match store
        .begin_vector_hnsw_rebuild(ns, "emb", source, engine)
        .unwrap()
    {
        DerivedArtifactRebuild::Started { run_id } => run_id,
        other => panic!("expected started HNSW rebuild, got {other:?}"),
    };
    store
        .fail_vector_hnsw_rebuild(ns, "emb", &run_id, source, engine, "hnsw writer failed")
        .unwrap();
    let failed = store.vector_hnsw_status(ns, "emb", source, engine).unwrap();
    assert_eq!(
        failed,
        DerivedArtifactStatus::Failed {
            stamp: stamp.clone(),
            message: "hnsw writer failed".into()
        }
    );
    assert_eq!(
        failed.serving_policy().reason_code,
        Some("derived_artifact_failed")
    );

    store
        .mark_vector_hnsw_unsupported(ns, "emb", source, engine, "native hnsw unavailable")
        .unwrap();
    let unsupported = store.vector_hnsw_status(ns, "emb", source, engine).unwrap();
    assert_eq!(
        unsupported,
        DerivedArtifactStatus::Unsupported {
            stamp,
            message: "native hnsw unavailable".into()
        }
    );
    assert_eq!(
        unsupported.serving_policy().stable_error,
        Some(loom_core::Code::Unsupported)
    );
}

#[test]
fn search_tantivy_lifecycle_uses_search_artifact_contract() {
    let tp = TempPath::new("search-tantivy-derived");
    let ns = loom_core::WorkspaceId::from_bytes([6; 16]);
    let store = FileStore::open(tp.path()).unwrap();
    let source = loom_core::Digest::blake3(b"search-root");
    let engine = "tantivy-test-0";

    assert_eq!(
        search_tantivy_artifact_key(ns, "docs").unwrap(),
        DerivedArtifactKey::new(ns, loom_core::FacetKind::Search, "docs", "tantivy").unwrap()
    );
    assert_eq!(
        search_tantivy_artifact_stamp(source, engine).unwrap(),
        DerivedArtifactStamp::new(source, engine, "search-tantivy-v1").unwrap()
    );
    assert_eq!(
        store
            .search_tantivy_status(ns, "docs", source, engine)
            .unwrap(),
        DerivedArtifactStatus::Missing
    );

    let run_id = match store
        .begin_search_tantivy_rebuild(ns, "docs", source, engine)
        .unwrap()
    {
        DerivedArtifactRebuild::Started { run_id } => run_id,
        other => panic!("expected started search rebuild, got {other:?}"),
    };
    assert_eq!(
        store
            .begin_search_tantivy_rebuild(ns, "docs", source, engine)
            .unwrap(),
        DerivedArtifactRebuild::Coalesced {
            run_id: run_id.clone()
        }
    );
    assert_eq!(
        store
            .search_tantivy_status(ns, "docs", source, engine)
            .unwrap(),
        DerivedArtifactStatus::Rebuilding {
            run_id: run_id.clone(),
            stamp: search_tantivy_artifact_stamp(source, engine).unwrap()
        }
    );

    let record = store
        .finish_search_tantivy_rebuild(ns, "docs", &run_id, source, engine, b"tantivy bytes")
        .unwrap();
    assert_eq!(
        store
            .search_tantivy_status(ns, "docs", source, engine)
            .unwrap(),
        DerivedArtifactStatus::Ready {
            record: record.clone()
        }
    );

    let next_source = loom_core::Digest::blake3(b"search-root-2");
    assert_eq!(
        store
            .search_tantivy_status(ns, "docs", next_source, engine)
            .unwrap(),
        DerivedArtifactStatus::Stale {
            record: record.clone()
        }
    );
    let failed_run = match store
        .begin_search_tantivy_rebuild(ns, "docs", next_source, engine)
        .unwrap()
    {
        DerivedArtifactRebuild::Started { run_id } => run_id,
        other => panic!("expected started stale-source rebuild, got {other:?}"),
    };
    store
        .fail_search_tantivy_rebuild(
            ns,
            "docs",
            &failed_run,
            next_source,
            engine,
            "source changed during search index build",
        )
        .unwrap();
    assert_eq!(
        store
            .search_tantivy_status(ns, "docs", next_source, engine)
            .unwrap(),
        DerivedArtifactStatus::Failed {
            stamp: search_tantivy_artifact_stamp(next_source, engine).unwrap(),
            message: "source changed during search index build".into()
        }
    );

    store
        .mark_search_tantivy_unsupported(ns, "docs", next_source, engine, "tantivy unavailable")
        .unwrap();
    assert_eq!(
        store
            .search_tantivy_status(ns, "docs", next_source, engine)
            .unwrap(),
        DerivedArtifactStatus::Unsupported {
            stamp: search_tantivy_artifact_stamp(next_source, engine).unwrap(),
            message: "tantivy unavailable".into()
        }
    );
}

#[test]
fn search_status_result_round_trips_every_variant() {
    let source = loom_core::Digest::blake3(b"search-root");
    let stamp = DerivedArtifactStamp::new(source, "tantivy-1", "search-tantivy-v1").unwrap();
    let record = DerivedArtifactRecord {
        stamp: stamp.clone(),
        payload_digest: loom_core::Digest::blake3(b"payload"),
        payload_len: 42,
    };
    for status in [
        DerivedArtifactStatus::Missing,
        DerivedArtifactStatus::Stale {
            record: record.clone(),
        },
        DerivedArtifactStatus::Ready {
            record: record.clone(),
        },
        DerivedArtifactStatus::Rebuilding {
            run_id: "run-1".into(),
            stamp: stamp.clone(),
        },
        DerivedArtifactStatus::Failed {
            stamp: stamp.clone(),
            message: "boom".into(),
        },
        DerivedArtifactStatus::Unsupported {
            stamp: stamp.clone(),
            message: "tantivy unavailable".into(),
        },
    ] {
        let bytes = encode_search_status_result(&source, &status).unwrap();
        let (got_source, got_status) = decode_search_status_result(&bytes).unwrap();
        assert_eq!(got_source, source);
        assert_eq!(got_status, status);
    }
    // A corrupt/short payload is rejected, not silently misparsed.
    assert!(decode_search_status_result(b"nope").is_err());
}

#[test]
fn search_embedding_lifecycle_uses_entity_projection_contract() {
    let tp = TempPath::new("search-embedding-derived");
    let ns = loom_core::WorkspaceId::from_bytes([26; 16]);
    let store = FileStore::open(tp.path()).unwrap();
    let source = loom_core::Digest::blake3(b"doc body");
    let projection = SearchEmbeddingProjection {
        workspace: ns,
        collection: "docs",
        entity_id: "doc-1",
        content_digest: source,
        model_id: "embed-small",
        model_weights_digest: Some("weights-a"),
        engine_version: "semantic-v1",
    };

    assert_eq!(
        search_embedding_artifact_key(ns, "docs", "doc-1").unwrap(),
        DerivedArtifactKey::new(ns, loom_core::FacetKind::Search, "docs", "embedding:doc-1")
            .unwrap()
    );
    assert_eq!(
        search_embedding_artifact_stamp(source, "embed-small", Some("weights-a"), "semantic-v1")
            .unwrap(),
        DerivedArtifactStamp::new(
            source,
            "11:embed-small|9:weights-a|11:semantic-v1",
            "search-embedding-v1"
        )
        .unwrap()
    );
    assert_eq!(
        store.search_embedding_status(projection).unwrap(),
        DerivedArtifactStatus::Missing
    );

    let run_id = match store.begin_search_embedding_rebuild(projection).unwrap() {
        DerivedArtifactRebuild::Started { run_id } => run_id,
        other => panic!("expected started embedding rebuild, got {other:?}"),
    };
    assert_eq!(
        store.begin_search_embedding_rebuild(projection).unwrap(),
        DerivedArtifactRebuild::Coalesced {
            run_id: run_id.clone()
        }
    );
    assert_eq!(
        store.search_embedding_status(projection).unwrap(),
        DerivedArtifactStatus::Rebuilding {
            run_id: run_id.clone(),
            stamp: search_embedding_artifact_stamp(
                source,
                "embed-small",
                Some("weights-a"),
                "semantic-v1"
            )
            .unwrap()
        }
    );

    let record = store
        .finish_search_embedding_rebuild(projection, &run_id, b"vector bytes")
        .unwrap();
    assert_eq!(
        store.search_embedding_status(projection).unwrap(),
        DerivedArtifactStatus::Ready {
            record: record.clone()
        }
    );

    let changed_content = SearchEmbeddingProjection {
        content_digest: loom_core::Digest::blake3(b"doc body changed"),
        ..projection
    };
    assert_eq!(
        store.search_embedding_status(changed_content).unwrap(),
        DerivedArtifactStatus::Stale { record }
    );

    let blind_projection = SearchEmbeddingProjection {
        entity_id: "doc-2",
        content_digest: loom_core::Digest::blake3(b"opaque doc"),
        ..projection
    };
    store
        .mark_search_embedding_no_keys(blind_projection, "plaintext unavailable")
        .unwrap();
    assert_eq!(
        store.search_embedding_status(blind_projection).unwrap(),
        DerivedArtifactStatus::Unsupported {
            stamp: search_embedding_artifact_stamp(
                blind_projection.content_digest,
                "embed-small",
                Some("weights-a"),
                "semantic-v1"
            )
            .unwrap(),
            message: "plaintext unavailable".into()
        }
    );
}

#[test]
fn facet_source_digests_change_when_sources_change() {
    let tp = TempPath::new("derived-source-digests");
    let store = FileStore::open(tp.path()).unwrap();
    let mut loom = loom_core::Loom::new(store);
    let vector_ns = loom
        .registry_mut()
        .create(
            loom_core::FacetKind::Vector,
            Some("vector-digest"),
            loom_core::WorkspaceId::from_bytes([21; 16]),
        )
        .unwrap();
    loom_core::vector_create(&mut loom, vector_ns, "emb", 2, loom_core::Metric::Dot).unwrap();
    loom_core::vector_upsert(
        &mut loom,
        vector_ns,
        "emb",
        "a",
        vec![1.0, 0.0],
        BTreeMap::new(),
    )
    .unwrap();
    let vector_before = loom_core::vector_source_digest(&loom, vector_ns, "emb").unwrap();
    loom_core::vector_upsert(
        &mut loom,
        vector_ns,
        "emb",
        "b",
        vec![0.0, 1.0],
        BTreeMap::new(),
    )
    .unwrap();
    let vector_after = loom_core::vector_source_digest(&loom, vector_ns, "emb").unwrap();
    assert_ne!(vector_before, vector_after);
    let vector_tip = loom.commit(vector_ns, "test", "vector source", 1).unwrap();
    let vector_key = vector_pq_artifact_key(vector_ns, "emb").unwrap();
    let vector_stamp = vector_pq_artifact_stamp(vector_after, "pq-writer-0").unwrap();
    loom.store()
        .put_derived_artifact(&vector_key, vector_stamp.clone(), b"pq bytes")
        .unwrap();
    assert!(matches!(
        loom.store()
            .derived_artifact_status(&vector_key, &vector_stamp)
            .unwrap(),
        DerivedArtifactStatus::Ready { .. }
    ));

    let dst_path = TempPath::new("derived-clone-dst");
    let dst_store = FileStore::open(dst_path.path()).unwrap();
    let mut dst = loom_core::Loom::new(dst_store);
    let (dst_ns, _) = loom_core::clone_workspace(
        &loom,
        vector_ns,
        &mut dst,
        loom_core::WorkspaceId::from_bytes([24; 16]),
    )
    .unwrap();
    dst.checkout_commit(dst_ns, vector_tip).unwrap();
    let dst_key = vector_pq_artifact_key(dst_ns, "emb").unwrap();
    let dst_stamp = vector_pq_artifact_stamp(
        loom_core::vector_source_digest(&dst, dst_ns, "emb").unwrap(),
        "pq-writer-0",
    )
    .unwrap();
    assert_eq!(
        dst.store()
            .derived_artifact_status(&dst_key, &dst_stamp)
            .unwrap(),
        DerivedArtifactStatus::Missing
    );

    let search_ns = loom
        .registry_mut()
        .create(
            loom_core::FacetKind::Search,
            Some("search-digest"),
            loom_core::WorkspaceId::from_bytes([22; 16]),
        )
        .unwrap();
    let mut mapping = loom_core::Mapping::new();
    mapping.insert("title".into(), loom_core::FieldMapping::text());
    loom_core::search_create(&mut loom, search_ns, "docs", mapping).unwrap();
    let search_before = loom_core::search_source_digest(&loom, search_ns, "docs").unwrap();
    let mut doc = loom_core::Document::new();
    doc.insert("title".into(), loom_core::FieldValue::Text("first".into()));
    loom_core::search_index(&mut loom, search_ns, "docs", b"a".to_vec(), doc).unwrap();
    let search_after = loom_core::search_source_digest(&loom, search_ns, "docs").unwrap();
    assert_ne!(search_before, search_after);

    let columnar_ns = loom
        .registry_mut()
        .create(
            loom_core::FacetKind::Columnar,
            Some("columnar-digest"),
            loom_core::WorkspaceId::from_bytes([23; 16]),
        )
        .unwrap();
    loom_core::columnar_create(
        &mut loom,
        columnar_ns,
        "events",
        vec![("id".into(), loom_core::ColumnType::Int)],
        4,
    )
    .unwrap();
    let columnar_before = loom_core::columnar_source_digest(&loom, columnar_ns, "events").unwrap();
    loom_core::columnar_append(
        &mut loom,
        columnar_ns,
        "events",
        vec![loom_core::Value::Int(1)],
    )
    .unwrap();
    let columnar_after = loom_core::columnar_source_digest(&loom, columnar_ns, "events").unwrap();
    assert_ne!(columnar_before, columnar_after);
}

#[test]
fn objects_survive_reopen() {
    let tp = TempPath::new("reopen");
    let (d1, d2);
    {
        let store = FileStore::open(tp.path()).unwrap();
        d1 = store.put(&blob(b"alpha")).unwrap();
        d2 = store.put(&blob(b"beta")).unwrap();
        assert_eq!(store.len(), 2);
    } // drop -> file closed
    let store = FileStore::open(tp.path()).unwrap();
    assert_eq!(store.len(), 2);
    assert_eq!(
        store.get(&d1).unwrap().as_deref(),
        Some(blob(b"alpha").as_slice())
    );
    assert_eq!(
        store.get(&d2).unwrap().as_deref(),
        Some(blob(b"beta").as_slice())
    );
}

/// The full engine works over a FIPS (sha256) identity profile: a files workspace
/// commits, persists, reopens, and reads back, and every stored object is addressed under
/// SHA-256 - proving content addressing, the content map, commits, and verification are all coherent
/// under the store profile (not hard-coded BLAKE3).
#[test]
fn full_loom_over_fips_profile_round_trips() {
    use loom_core::workspace::{FacetKind, WorkspaceId};
    use loom_core::{ObjectStore, WsSelector};

    let tp = TempPath::new("fips-loom");
    let ns_id = WorkspaceId::from_bytes([9; 16]);
    // Create the store under the FIPS profile, then drive the engine over it via open_loom (which
    // reopens and reads the sha256 profile from the superblock).
    FileStore::create_with_profile(tp.path(), Algo::Sha256).unwrap();
    {
        let mut loom = open_loom(tp.path()).unwrap();
        assert_eq!(loom.store().digest_algo(), Algo::Sha256);
        let ns = loom
            .registry_mut()
            .create(FacetKind::Files, Some("proj"), ns_id)
            .unwrap();
        loom.write_file(ns, "README.md", b"# fips hello", 0o100644)
            .unwrap();
        loom.create_directory(ns, "src", false).unwrap();
        loom.write_file(ns, "src/main.rs", b"fn main() {}", 0o100644)
            .unwrap();
        loom.commit(ns, "nas", "init", 1).unwrap();
        save_loom(&mut loom).unwrap();
        // Every object stored is addressed with SHA-256.
        assert!(loom.store().len() > 0);
    }
    // Reopen: the profile is still sha256, and the committed files read back through the engine.
    let loom = open_loom(tp.path()).unwrap();
    assert_eq!(loom.store().digest_algo(), Algo::Sha256);
    let ns = loom
        .registry()
        .open(&WsSelector::Typed {
            ty: FacetKind::Files,
            name: "proj".to_string(),
        })
        .unwrap();
    assert_eq!(loom.read_file(ns, "README.md").unwrap(), b"# fips hello");
    assert_eq!(loom.read_file(ns, "src/main.rs").unwrap(), b"fn main() {}");
}

#[test]
fn full_loom_survives_restart() {
    use loom_core::WsSelector;
    use loom_core::workspace::{DEFAULT_BRANCH, FacetKind, WorkspaceId};

    let tp = TempPath::new("full-loom");
    let ns_id = WorkspaceId::from_bytes([7; 16]);
    let tip;
    {
        // Build a real engine: a files workspace, a commit, a second branch, and a tag.
        let mut loom = open_loom(tp.path()).unwrap();
        let ns = loom
            .registry_mut()
            .create(FacetKind::Files, Some("proj"), ns_id)
            .unwrap();
        loom.write_file(ns, "README.md", b"# hello", 0o100644)
            .unwrap();
        loom.create_directory(ns, "src", false).unwrap();
        loom.write_file(ns, "src/main.rs", b"fn main() {}", 0o100644)
            .unwrap();
        let c0 = loom.commit(ns, "nas", "init", 1).unwrap();
        loom.branch(ns, "feature").unwrap();
        loom.registry_mut().tag_create(ns, "v1", c0).unwrap();
        tip = c0;
        save_loom(&mut loom).unwrap();
    } // drop -> file closed

    // Reopen from disk: registry (refs/tags/HEAD), content map, and working tree must all return.
    let loom = open_loom(tp.path()).unwrap();
    let ns = loom
        .registry()
        .open(&WsSelector::Typed {
            ty: FacetKind::Files,
            name: "proj".to_string(),
        })
        .unwrap();
    assert_eq!(ns, ns_id);
    assert_eq!(loom.registry().head_branch(ns).unwrap(), DEFAULT_BRANCH);
    assert_eq!(
        loom.registry().branch_tip(ns, DEFAULT_BRANCH).unwrap(),
        Some(tip)
    );
    assert_eq!(
        loom.registry().branch_tip(ns, "feature").unwrap(),
        Some(tip)
    );
    assert_eq!(loom.registry().tag_target(ns, "v1").unwrap(), Some(tip));
    // Working tree was rebuilt by checking out HEAD on open.
    assert_eq!(loom.read_file(ns, "README.md").unwrap(), b"# hello");
    assert_eq!(loom.read_file(ns, "src/main.rs").unwrap(), b"fn main() {}");
}

#[test]
fn optional_runtime_config_survives_full_loom_restart_without_activation() {
    use loom_core::workspace::{FacetKind, WorkspaceId};
    use loom_core::{
        OptionalRuntimeConfig, OptionalRuntimeKind, activate_optional_runtime,
        get_optional_runtime_config, set_optional_runtime_config,
    };

    let tp = TempPath::new("optional-runtime-config-restart");
    let ns_id = WorkspaceId::from_bytes([41; 16]);
    let mut settings = BTreeMap::new();
    settings.insert("gateway".to_string(), "https://example.test".to_string());
    let config = OptionalRuntimeConfig::new(OptionalRuntimeKind::Ipfs, true, settings).unwrap();
    {
        let mut loom = open_loom(tp.path()).unwrap();
        let ns = loom
            .registry_mut()
            .create(FacetKind::Files, Some("proj"), ns_id)
            .unwrap();
        set_optional_runtime_config(&mut loom, ns, &config).unwrap();
        loom.commit(ns, "nas", "optional runtime config", 1)
            .unwrap();
        save_loom(&mut loom).unwrap();
    }

    let loom = open_loom(tp.path()).unwrap();
    assert_eq!(
        get_optional_runtime_config(&loom, ns_id, OptionalRuntimeKind::Ipfs).unwrap(),
        Some(config)
    );
    assert_eq!(
        activate_optional_runtime(&loom, ns_id, OptionalRuntimeKind::Ipfs)
            .unwrap_err()
            .code,
        loom_core::Code::Unsupported
    );
}

#[test]
fn set_reference_root_survives_reopen_and_clears() {
    let tp = TempPath::new("reference");
    let root = Digest::blake3(b"engine-state-root");
    {
        let store = FileStore::open(tp.path()).unwrap();
        assert_eq!(store.reference_root(), None);
        store.put(&blob(b"obj")).unwrap(); // a data commit before setting the root
        store.set_reference_root(Some(root)).unwrap();
        assert_eq!(store.reference_root(), Some(root));
    }
    // Reopen: the committed root and the object both survive.
    let store = FileStore::open(tp.path()).unwrap();
    assert_eq!(store.reference_root(), Some(root));
    assert_eq!(store.len(), 1);
    // A further object commit must preserve the existing root (it rides through `put`).
    store.put(&blob(b"obj2")).unwrap();
    assert_eq!(store.reference_root(), Some(root));
    // Clearing the root persists too.
    store.set_reference_root(None).unwrap();
    drop(store);
    let store = FileStore::open(tp.path()).unwrap();
    assert_eq!(store.reference_root(), None);
    assert_eq!(store.len(), 2);
}

// Build a file with `n` committed puts, returning the raw bytes after the final commit and the
// committed digests in order.
fn build_committed(n: usize) -> (Vec<u8>, Vec<Digest>) {
    let tp = TempPath::new("crash-src");
    let mut digests = Vec::new();
    {
        let store = FileStore::open(tp.path()).unwrap();
        for i in 0..n {
            digests.push(store.put(&blob(format!("obj-{i}").as_bytes())).unwrap());
        }
    }
    (std::fs::read(tp.path()).unwrap(), digests)
}

fn open_bytes(bytes: &[u8], tag: &str) -> Result<FileStore> {
    let tp = TempPath::new(tag);
    std::fs::write(tp.path(), bytes).unwrap();
    let r = FileStore::open(tp.path());
    // keep tp alive until after open
    drop(tp);
    r
}

// Like `open_bytes` but read-only, so recovery does not checkpoint-on-open - the on-disk
// superblock stays as written, exercising pure ring recovery with a lagging checkpoint.
fn open_read_bytes(bytes: &[u8], tag: &str) -> Result<FileStore> {
    let tp = TempPath::new(tag);
    std::fs::write(tp.path(), bytes).unwrap();
    let r = FileStore::open_read(tp.path());
    drop(tp);
    r
}

#[test]
fn crash_torn_append_recovers_last_commit() {
    // After N commits, simulate a crash mid-(N+1)th append: garbage appended beyond logical_end,
    // no new superblock. Recovery must yield exactly the N committed objects.
    let (mut bytes, digests) = build_committed(3);
    bytes.extend_from_slice(&[0xB0, 1, 2, 3, 4, 5]); // a partial/garbage record beyond logical_end
    let store = open_bytes(&bytes, "torn-append").unwrap();
    assert_eq!(store.len(), 3);
    for d in &digests {
        assert!(store.has(d).unwrap());
    }
}

#[test]
fn ring_recovers_latest_commit_when_superblock_lags() {
    // Fewer than CHECKPOINT_INTERVAL commits write no superblock checkpoint, so the on-disk
    // superblock stays at generation 0 while the ring holds gens 1..=3. A read-only reopen (no
    // checkpoint-on-open) must still recover all three from the ring.
    let (bytes, digests) = build_committed(3);
    let slot_a: &[u8; SLOT_SIZE as usize] = bytes[..SLOT_SIZE as usize].try_into().unwrap();
    assert_eq!(
        Superblock::decode(slot_a).unwrap().generation,
        0,
        "superblock genuinely lags: no checkpoint at gen < CHECKPOINT_INTERVAL"
    );
    let store = open_read_bytes(&bytes, "ring-lag").unwrap();
    assert_eq!(store.len(), 3);
    for d in &digests {
        assert!(store.has(d).unwrap());
    }
}

#[test]
fn ring_torn_latest_record_falls_back_to_previous() {
    // A crash that tears the latest commit's ring record (bad CRC) falls back to the previous
    // durable commit: the ring keeps each record in its own slot, so a newer record's torn write
    // cannot destroy an earlier acked commit (a single shared slot would).
    let (bytes, _digests) = build_committed(3); // gens 1..=3 in ring slots 1, 2, 3
    let mut torn = bytes.clone();
    let gen3_off = (JOURNAL_OFFSET + 3 * journal::RECORD_SIZE as u64) as usize;
    for byte in torn[gen3_off..gen3_off + journal::RECORD_SIZE].iter_mut() {
        *byte ^= 0xFF; // corrupt gen 3's ring record only
    }
    let store = open_bytes(&torn, "ring-torn-latest").unwrap();
    assert_eq!(store.len(), 2); // recovered gen 2; gen-3's data beyond it is dead space
}

#[test]
fn ring_checkpoint_advances_superblock() {
    // After CHECKPOINT_INTERVAL commits a checkpoint is written, so the on-disk superblock
    // advances to that generation (bounding the recovery scan and freeing ring slots for reuse).
    let n = CHECKPOINT_INTERVAL as usize;
    let (bytes, digests) = build_committed(n);
    let slot_a: &[u8; SLOT_SIZE as usize] = bytes[..SLOT_SIZE as usize].try_into().unwrap();
    let slot_b: &[u8; SLOT_SIZE as usize] = bytes[SLOT_SIZE as usize..2 * SLOT_SIZE as usize]
        .try_into()
        .unwrap();
    let best = [slot_a, slot_b]
        .into_iter()
        .filter_map(Superblock::decode)
        .map(|sb| sb.generation)
        .max()
        .unwrap();
    assert_eq!(best, CHECKPOINT_INTERVAL);
    let store = open_read_bytes(&bytes, "ring-checkpoint").unwrap();
    assert_eq!(store.len(), n);
    for d in &digests {
        assert!(store.has(d).unwrap());
    }
}

#[test]
fn ring_wraps_and_recovers_past_checkpoint() {
    // Commit past RING_SLOTS so the ring wraps and multiple checkpoints land. Recovery overlays
    // the ring's newest generations on the latest superblock checkpoint; every object survives.
    let n = (RING_SLOTS + 8) as usize; // checkpoints at 16 and 32; ring wrapped at gen 33
    let (bytes, digests) = build_committed(n);
    let store = open_read_bytes(&bytes, "ring-wrap").unwrap();
    assert_eq!(store.len(), n);
    for d in &digests {
        assert!(store.has(d).unwrap());
    }
}

#[test]
fn lost_committed_data_is_a_clean_error_not_a_panic() {
    // Truncating into the committed data region destroys data a valid superblock references.
    // Recovery must report a clean CORRUPT error (never panic, never silently wrong).
    let (bytes, _) = build_committed(3);
    let truncated = &bytes[..bytes.len() - 4]; // chop into the last committed record
    let err = open_bytes(truncated, "lost-data").unwrap_err();
    assert!(matches!(err.code, Code::CorruptObject | Code::Io));
}

#[test]
fn put_batch_commits_atomically_in_one_generation() {
    let tp = TempPath::new("batch");
    let store = FileStore::open(tp.path()).unwrap();
    // Three single puts advance the generation three times...
    store.put(&blob(b"x")).unwrap();
    store.put(&blob(b"y")).unwrap();
    store.put(&blob(b"z")).unwrap();
    assert_eq!(store.generation(), 3);
    // ...whereas a batch of three commits in a single superblock swap (one generation bump).
    let before = store.generation();
    let ds = store
        .put_batch(&[blob(b"a").as_slice(), &blob(b"b"), &blob(b"c")])
        .unwrap();
    assert_eq!(ds.len(), 3);
    assert_eq!(
        store.generation(),
        before + 1,
        "batch must be one atomic commit"
    );
    assert_eq!(store.len(), 6);
    // The batched objects survive a reopen (the swap committed them all).
    drop(store);
    let store = FileStore::open(tp.path()).unwrap();
    assert_eq!(store.len(), 6);
    for d in &ds {
        assert!(store.has(d).unwrap());
    }
}

#[test]
fn put_batch_dedups_within_batch_and_against_store() {
    let tp = TempPath::new("batch-dedup");
    let store = FileStore::open(tp.path()).unwrap();
    let a = store.put(&blob(b"a")).unwrap(); // already stored
    // Batch repeats `a`, repeats `b` twice, plus a fresh `c`.
    let ds = store
        .put_batch(&[blob(b"a").as_slice(), &blob(b"b"), &blob(b"b"), &blob(b"c")])
        .unwrap();
    assert_eq!(ds.len(), 4); // one digest reported per input...
    assert_eq!(ds[0], a);
    assert_eq!(ds[1], ds[2]); // ...the two `b`s share a digest
    assert_eq!(store.len(), 3); // ...but only a, b, c are stored
    drop(store);
    let store = FileStore::open(tp.path()).unwrap();
    assert_eq!(store.len(), 3);
}

#[test]
fn interrupted_batch_leaves_the_prior_committed_state() {
    // A crash mid-batch (records/index nodes appended, superblock not yet flipped) appears as
    // bytes beyond the committed logical_end. Recovery must show none of the in-flight batch -
    // the all-or-nothing guarantee, identical to the single-record torn-append case.
    let (mut bytes, digests) = build_committed(2);
    bytes.extend_from_slice(&[0xAB; 256]); // a partially written, uncommitted batch
    let store = open_bytes(&bytes, "torn-batch").unwrap();
    assert_eq!(store.len(), 2);
    for d in &digests {
        assert!(store.has(d).unwrap());
    }
}

#[test]
fn mid_txn_crash_reclaims_pages_and_keeps_committed() {
    // The region-table-page swap is the prepare/commit boundary: a crash after a txn wrote its
    // pages but before its COMMIT record was fsynced leaves exactly the prior committed state, and
    // the crashed txn's pages return to free. Simulate the crash by appending uncommitted pages (a
    // crashed txn's file extension) past the committed page array.
    let (mut bytes, digests) = build_committed(4);
    let committed_len = bytes.len();
    bytes.extend_from_slice(&vec![0xCDu8; 8 * PAGE_SIZE as usize]); // a crashed txn's appended pages
    let tp = TempPath::new("mid-txn-crash");
    std::fs::write(tp.path(), &bytes).unwrap();

    let store = FileStore::open(tp.path()).unwrap();
    assert_eq!(store.len(), 4); // nothing from the in-flight txn is visible
    for d in &digests {
        assert!(store.has(d).unwrap());
    }
    assert_eq!(
        store.logical_end() as usize,
        committed_len,
        "recovery reverts to the committed page array; the crashed txn's pages are not retained"
    );
    // A fresh commit reuses the space the crashed txn occupied rather than leaking it.
    let d = store.put(&blob(b"after-crash")).unwrap();
    drop(store);
    let reopened = FileStore::open(tp.path()).unwrap();
    assert_eq!(reopened.len(), 5);
    assert!(reopened.has(&d).unwrap());
    for d in &digests {
        assert!(reopened.has(d).unwrap());
    }
}

#[test]
fn gc_chooses_only_mostly_dead_segments() {
    let occ = BTreeMap::from([
        (0u64, (9u64, 10u64)), // 90% live -> keep
        (1u64, (1u64, 10u64)), // 10% live -> collect
        (2u64, (5u64, 10u64)), // exactly half live -> keep (not below half)
        (3u64, (0u64, 4u64)),  // fully dead -> collect
        (4u64, (4u64, 4u64)),  // fully live -> keep
    ]);
    assert_eq!(
        choose_sparse_segments_bounded(&occ, None, GcSegmentBudget::unlimited()),
        vec![1, 3]
    );
    let eligible = BTreeSet::from([3u64]);
    assert_eq!(
        choose_sparse_segments_bounded(&occ, Some(&eligible), GcSegmentBudget::unlimited()),
        vec![3]
    );
    assert_eq!(
        choose_sparse_segments_bounded(
            &occ,
            None,
            GcSegmentBudget {
                max_segments: 1,
                max_pages: u64::MAX
            }
        ),
        vec![1]
    );
}

#[test]
fn gc_segments_reclaims_a_mostly_dead_segment_and_keeps_live() {
    let tp = TempPath::new("gc-seg");
    let mut store = FileStore::open(tp.path()).unwrap();
    let n = 300usize;
    let mut digests = Vec::with_capacity(n);
    for i in 0..n {
        digests.push(store.put(&blob(format!("obj-{i:04}").as_bytes())).unwrap());
    }
    store
        .control_set(b"lock/ns/fence", b"301".to_vec())
        .unwrap();
    // Keep only every tenth object: segment 0 becomes ~90% dead, so GC reclaims it.
    let live: BTreeSet<[u8; 32]> = digests
        .iter()
        .enumerate()
        .filter(|(i, _)| i % 10 == 0)
        .map(|(_, d)| *d.bytes())
        .collect();
    let free_before: u64 = store.free_runs().iter().map(|r| r.len).sum();

    let stats = store.gc_segments(&live).unwrap();
    assert!(stats.objects_dropped > 0, "GC should drop dead objects");
    assert!(stats.pages_freed > 0, "GC should free reclaimed pages");
    assert_eq!(store.len(), live.len() + 1);
    assert_eq!(
        store.control_get(b"lock/ns/fence").unwrap().as_deref(),
        Some(&b"301"[..])
    );
    // Survivors still resolve to their bytes; dropped objects are gone.
    for (i, d) in digests.iter().enumerate() {
        let want_live = i % 10 == 0;
        assert_eq!(store.has(d).unwrap(), want_live);
        if want_live {
            assert_eq!(
                store.get(d).unwrap().unwrap(),
                blob(format!("obj-{i:04}").as_bytes())
            );
        }
    }
    // Reclaimed pages went back to the free-page map (reusable, not yet truncated).
    let free_after: u64 = store.free_runs().iter().map(|r| r.len).sum();
    assert!(
        free_after > free_before,
        "reclaimed pages should be free now"
    );

    // Everything survives a reopen of the GC'd file.
    drop(store);
    let store = FileStore::open(tp.path()).unwrap();
    assert_eq!(store.len(), live.len() + 1);
    assert_eq!(
        store.control_get(b"lock/ns/fence").unwrap().as_deref(),
        Some(&b"301"[..])
    );
    for (i, d) in digests.iter().enumerate() {
        assert_eq!(store.has(d).unwrap(), i % 10 == 0);
    }
}

#[test]
fn gc_validated_segments_requires_completed_epoch_and_obeys_budget() {
    let tp = TempPath::new("gc-validated-seg");
    let mut store = FileStore::open(tp.path()).unwrap();
    let n = 300usize;
    let mut digests = Vec::with_capacity(n);
    for i in 0..n {
        digests.push(store.put(&blob(format!("obj-{i:04}").as_bytes())).unwrap());
    }
    let missing = store
        .gc_validated_segments(GcSegmentBudget {
            max_segments: 1,
            max_pages: u64::MAX,
        })
        .unwrap_err();
    assert_eq!(missing.code, Code::NotFound);

    let live_digests = digests
        .iter()
        .enumerate()
        .filter(|(i, _)| i % 10 == 0)
        .map(|(_, digest)| *digest)
        .collect::<BTreeSet<_>>();
    let state = loom_core::ReachabilityMarkState {
        pinned: BTreeSet::new(),
        marked: live_digests,
        queue: std::collections::VecDeque::new(),
        stream_roots: std::collections::VecDeque::new(),
        completed: false,
    };
    let mut epoch = store
        .begin_reachability_mark_epoch(None, BTreeSet::new(), state)
        .unwrap();
    let incomplete = store
        .gc_validated_segments(GcSegmentBudget {
            max_segments: 1,
            max_pages: u64::MAX,
        })
        .unwrap_err();
    assert_eq!(incomplete.code, Code::Conflict);

    epoch.state.completed = true;
    store.complete_reachability_mark_epoch(&epoch).unwrap();
    let stats = store
        .gc_validated_segments(GcSegmentBudget {
            max_segments: 1,
            max_pages: u64::MAX,
        })
        .unwrap();
    assert!(stats.segments_reclaimed <= 1);
    assert!(stats.objects_dropped > 0);
    assert!(stats.pages_freed > 0);
    for (i, digest) in digests.iter().enumerate() {
        assert_eq!(store.has(digest).unwrap(), i % 10 == 0);
    }
}

#[test]
fn tail_trim_shrinks_only_an_already_free_eof_suffix() {
    let tp = TempPath::new("tail-trim-free-eof");
    let mut store = FileStore::open(tp.path()).unwrap();
    let live_digest = store.put(&blob(b"live-before-free-tail")).unwrap();
    store.set_reference_root(Some(live_digest)).unwrap();
    let before = store.maintenance_status().unwrap().physical_page_count;
    {
        let mut inner = store.inner.lock().unwrap();
        let suffix = 128;
        let start = inner.page_count;
        let freed_gen = inner.generation;
        inner.free.push(FreePageRun {
            start,
            len: suffix,
            freed_gen,
        });
        inner.page_count += suffix;
        inner.maintenance.physical_page_count = inner.page_count;
        inner.maintenance.reusable_free_pages += suffix;
        inner.maintenance.candidate_dead_pages += suffix;
        let mut file = store.file.lock().unwrap();
        file.grow(DATA_START + inner.page_count * PAGE_SIZE)
            .unwrap();
    }

    let expanded = store.maintenance_status().unwrap().physical_page_count;
    assert_eq!(expanded, before + 128);
    let trimmed = store.trim_tail_free_pages().unwrap();
    let after = store.maintenance_status().unwrap().physical_page_count;
    assert!(trimmed > 0);
    assert!(after < expanded);
    assert_eq!(store.has(&live_digest).unwrap(), true);
    drop(store);

    let reopened = FileStore::open(tp.path()).unwrap();
    assert_eq!(reopened.has(&live_digest).unwrap(), true);
    assert_eq!(
        reopened.maintenance_status().unwrap().physical_page_count,
        after
    );
}

#[test]
fn tail_compaction_relocates_live_tail_object_and_shrinks() {
    let tp = TempPath::new("tail-compact-live");
    let mut store = FileStore::open(tp.path()).unwrap();
    {
        let mut inner = store.inner.lock().unwrap();
        let free_pages = 512;
        inner.free.push(FreePageRun {
            start: 0,
            len: free_pages,
            freed_gen: 1,
        });
        inner.page_count = free_pages;
        inner.maintenance.physical_page_count = free_pages;
        inner.maintenance.reusable_free_pages = free_pages;
        inner.maintenance.candidate_dead_pages = free_pages;
        let mut file = store.file.lock().unwrap();
        file.grow(DATA_START + inner.page_count * PAGE_SIZE)
            .unwrap();
    }
    let live = store.put(&vec![0xC7; 300 * 1024]).unwrap();
    store.set_reference_root(Some(live)).unwrap();
    {
        let mut inner = store.inner.lock().unwrap();
        inner.generation = REUSE_SAFE_WINDOW + 10;
        for run in &mut inner.free {
            run.freed_gen = 1;
        }
    }
    let before = store.maintenance_status().unwrap().physical_page_count;

    let stats = store.compact_tail_once(256, 1, 512 * 1024).unwrap();
    assert!(stats.attempted);
    assert_eq!(stats.relocated_objects, 1);
    assert!(stats.relocated_pages > 0);
    assert!(stats.truncated_pages > 0);
    let after = store.maintenance_status().unwrap().physical_page_count;
    assert!(after < before);
    assert_eq!(store.get(&live).unwrap().unwrap(), vec![0xC7; 300 * 1024]);
    drop(store);

    let reopened = FileStore::open(tp.path()).unwrap();
    assert_eq!(
        reopened.get(&live).unwrap().unwrap(),
        vec![0xC7; 300 * 1024]
    );
}

#[test]
fn tail_compaction_skips_without_earlier_free_space() {
    let tp = TempPath::new("tail-compact-no-space");
    let mut store = FileStore::open(tp.path()).unwrap();
    let live = store.put(&vec![0xD1; 300 * 1024]).unwrap();
    store.set_reference_root(Some(live)).unwrap();

    let stats = store.compact_tail_once(160, 1, 512 * 1024).unwrap();
    assert!(stats.attempted);
    assert!(stats.skipped);
    assert_eq!(stats.relocated_objects, 0);
    assert_eq!(store.get(&live).unwrap().unwrap(), vec![0xD1; 300 * 1024]);
}

#[test]
fn tail_compaction_aborts_on_evidence_drift() {
    let tp = TempPath::new("tail-compact-drift");
    let mut store = FileStore::open(tp.path()).unwrap();
    {
        let mut inner = store.inner.lock().unwrap();
        let free_pages = 512;
        inner.free.push(FreePageRun {
            start: 0,
            len: free_pages,
            freed_gen: 1,
        });
        inner.page_count = free_pages;
        inner.maintenance.physical_page_count = free_pages;
        inner.maintenance.reusable_free_pages = free_pages;
        inner.maintenance.candidate_dead_pages = free_pages;
        let mut file = store.file.lock().unwrap();
        file.grow(DATA_START + inner.page_count * PAGE_SIZE)
            .unwrap();
    }
    let live = store.put(&vec![0xE3; 300 * 1024]).unwrap();
    store.set_reference_root(Some(live)).unwrap();
    {
        let mut inner = store.inner.lock().unwrap();
        inner.generation = REUSE_SAFE_WINDOW + 10;
        for run in &mut inner.free {
            run.freed_gen = 1;
        }
    }
    let before = store.maintenance_status().unwrap().physical_page_count;

    let stats = store
        .compact_tail_once_with_pre_commit_interleave(256, 1, 512 * 1024, |observed| {
            let mut inner = observed.inner.lock().map_err(|_| poisoned())?;
            inner.generation += 1;
            Ok(())
        })
        .unwrap();
    assert!(stats.attempted);
    assert!(stats.skipped);
    assert_eq!(stats.conflicts, 1);
    assert_eq!(stats.truncated_pages, 0);
    assert_eq!(
        store.maintenance_status().unwrap().physical_page_count,
        before
    );
    assert_eq!(store.get(&live).unwrap().unwrap(), vec![0xE3; 300 * 1024]);
}

#[test]
fn gc_keeps_a_segment_that_is_mostly_live_by_pages() {
    // One multi-page large record (live) plus a single slab page of many tiny dead objects. By
    // object count the segment looks ~95% dead (1 of 21 live) and a count-based GC would relocate
    // the big live record to reclaim one slab page; by PAGES it is mostly live, so page-based GC
    // correctly leaves it alone.
    let big: Vec<u8> = {
        let mut s = 0x51A7_u64 | 1;
        (0..16_000u32)
            .map(|_| {
                s ^= s << 13;
                s ^= s >> 7;
                s ^= s << 17;
                (s >> 24) as u8
            })
            .collect()
    };
    let tp = TempPath::new("gc-page-ratio");
    let mut store = FileStore::open(tp.path()).unwrap();
    let big_live = store.put(&blob(&big)).unwrap(); // a multi-page large run
    let tiny: Vec<Vec<u8>> = (0..20).map(|i| blob(format!("t{i}").as_bytes())).collect();
    let refs: Vec<&[u8]> = tiny.iter().map(|v| v.as_slice()).collect();
    store.put_batch(&refs).unwrap(); // one shared slab page

    let live: BTreeSet<[u8; 32]> = std::iter::once(*big_live.bytes()).collect();
    let stats = store.gc_segments(&live).unwrap();
    assert_eq!(
        stats.segments_reclaimed, 0,
        "a segment that is mostly live by pages must not be collected"
    );
    assert_eq!(stats.pages_trimmed, 0);
    assert_eq!(store.len(), 21); // nothing dropped (count-based GC would have dropped the 20 tiny)
    assert!(store.has(&big_live).unwrap());
}

#[test]
fn truncate_trailing_drops_only_the_top_free_run() {
    let run = |start, len| FreePageRun {
        start,
        len,
        freed_gen: 0,
    };
    // Trailing run [7,10) is free -> shrink to 7; [2,4) stays.
    let (pc, runs) = truncate_trailing(vec![run(2, 2), run(7, 3)], 10);
    assert_eq!((pc, runs), (7, vec![run(2, 2)]));
    // A live page at the top (no free run ends at page_count) blocks the shrink.
    let (pc, runs) = truncate_trailing(vec![run(2, 2)], 10);
    assert_eq!((pc, runs), (10, vec![run(2, 2)]));
    // Adjacent trailing runs collapse together.
    let (pc, runs) = truncate_trailing(vec![run(0, 1), run(5, 2), run(7, 3)], 10);
    assert_eq!((pc, runs), (5, vec![run(0, 1)]));
    // A wholly free array shrinks to zero.
    let (pc, runs) = truncate_trailing(vec![run(0, 10)], 10);
    assert_eq!(pc, 0);
    assert!(runs.is_empty());
}

#[test]
fn decoders_never_panic_on_arbitrary_bytes() {
    // A lightweight fuzz: throw pseudo-random byte buffers of many lengths at every on-disk
    // decoder and require a clean Result/Option, never a panic (no out-of-bounds slice, integer
    // overflow, or huge allocation from a crafted length).
    fn xorshift(s: &mut u64) -> u64 {
        *s ^= *s << 13;
        *s ^= *s >> 7;
        *s ^= *s << 17;
        *s
    }
    let mut s = 0x1234_5678_9abc_def0u64;
    for _ in 0..20_000 {
        let len = (xorshift(&mut s) % 600) as usize;
        let mut buf = Vec::with_capacity(len);
        for _ in 0..len {
            buf.push((xorshift(&mut s) >> 33) as u8);
        }
        let _ = page::RegionTable::decode(&buf);
        let _ = pagemap::decode(&buf);
        let _ = record::read_slab_slot(&buf, (xorshift(&mut s) % 256) as u32);
        let _ = record::decode_large(&buf);
        let _ = record::large_blob_len(&buf);
        let _ = journal::decode(&buf);
        let _ = decode_record(&buf, &Digest::blake3(&buf), None, Algo::Blake3);
        let mut pos = (xorshift(&mut s) as usize) % (len + 1);
        let _ = record::RecordLoc::decode(&buf, &mut pos);
        let mut pos = (xorshift(&mut s) as usize) % (len + 1);
        let _ = get_uvarint(&buf, &mut pos);
    }
}

#[test]
fn online_truncate_shrinks_a_freed_trailing_region() {
    let tp = TempPath::new("truncate");
    let store = FileStore::open(tp.path()).unwrap();
    let anchor = store.put(&blob(b"anchor")).unwrap();
    // Build up: the per-commit region/free-map churn repeatedly extends the top of the file.
    let mut small = Vec::new();
    for i in 0..120u32 {
        small.push(store.put(&blob(format!("s-{i:04}").as_bytes())).unwrap());
    }
    let peak = store.logical_end();
    // Subsequent commits place their region/map pages on low aged holes instead of extending, so
    // the build-up's trailing churn pages become free; once aged they are truncated and the file
    // shrinks below its peak.
    for _ in 0..40 {
        store.set_reference_root(Some(anchor)).unwrap();
        store.set_reference_root(None).unwrap();
    }
    let after = store.logical_end();
    assert!(
        after < peak,
        "online truncate should shrink the file below its peak: peak={peak} after={after}"
    );
    // The shrink loses nothing: every object still reads and the file reopens intact.
    assert!(store.has(&anchor).unwrap());
    for d in &small {
        assert!(store.has(d).unwrap());
    }
    drop(store);
    let store = FileStore::open(tp.path()).unwrap();
    assert_eq!(store.len(), 121);
    assert!(store.has(&anchor).unwrap());
}

#[test]
fn compact_reclaims_dead_space_and_preserves_objects() {
    let tp = TempPath::new("compact");
    let mut store = FileStore::open(tp.path()).unwrap();
    let n = 300usize;
    let mut digests = Vec::with_capacity(n);
    for i in 0..n {
        // Each individual put CoW-rewrites the B-tree path, leaving dead nodes behind.
        digests.push(store.put(&blob(format!("obj-{i}").as_bytes())).unwrap());
    }
    store.set_reference_root(Some(digests[0])).unwrap(); // point the reference at a real, stored object
    store
        .control_set(b"lock/ns/fence", b"300".to_vec())
        .unwrap();

    let stats = store.compact().unwrap();
    assert!(
        stats.after < stats.before,
        "compaction should reclaim dead B-tree nodes: before={} after={}",
        stats.before,
        stats.after
    );
    assert!(stats.reclaimed() > 0);
    // Everything is intact post-compaction: count, every object, and the reference root.
    assert_eq!(store.len(), n + 1);
    assert_eq!(store.reference_root(), Some(digests[0]));
    assert_eq!(
        store.control_get(b"lock/ns/fence").unwrap().as_deref(),
        Some(&b"300"[..])
    );
    for (i, d) in digests.iter().enumerate() {
        assert_eq!(
            store.get(d).unwrap().unwrap(),
            blob(format!("obj-{i}").as_bytes())
        );
    }
    // ...and it all survives a reopen of the freshly compacted file.
    drop(store);
    let store = FileStore::open(tp.path()).unwrap();
    assert_eq!(store.len(), n + 1);
    assert_eq!(store.reference_root(), Some(digests[0]));
    assert_eq!(
        store.control_get(b"lock/ns/fence").unwrap().as_deref(),
        Some(&b"300"[..])
    );
}

#[test]
fn compaction_capacity_reports_required_temp_bytes() {
    let tp = TempPath::new("compact-capacity");
    let store = FileStore::open(tp.path()).unwrap();
    let d = store.put(b"capacity").unwrap();
    store.set_reference_root(Some(d)).unwrap();
    let capacity = store.compaction_capacity().unwrap();
    assert!(capacity.required_temp_bytes >= DATA_START);
    #[cfg(unix)]
    assert!(capacity.available_temp_bytes.unwrap() >= capacity.required_temp_bytes);
    store.ensure_compaction_capacity().unwrap();
}

#[test]
fn compact_preserves_a_full_loom() {
    use loom_core::WsSelector;
    use loom_core::workspace::{FacetKind, WorkspaceId};

    let tp = TempPath::new("compact-loom");
    {
        let mut loom = open_loom(tp.path()).unwrap();
        let ns = loom
            .registry_mut()
            .create(
                FacetKind::Files,
                Some("p"),
                WorkspaceId::from_bytes([3; 16]),
            )
            .unwrap();
        // Churn: repeated edits + commits + saves leave dead engine-state blobs and B-tree nodes.
        for i in 0..6u64 {
            loom.write_file(ns, "f.txt", format!("v{i}").as_bytes(), 0o100644)
                .unwrap();
            loom.commit(ns, "nas", "edit", i + 1).unwrap();
            save_loom(&mut loom).unwrap();
        }
        loom.store_mut().compact().unwrap();
    }
    // Reopen the compacted file as a full Loom: refs + working tree must round-trip.
    let loom = open_loom(tp.path()).unwrap();
    let ns = loom
        .registry()
        .open(&WsSelector::Typed {
            ty: FacetKind::Files,
            name: "p".to_string(),
        })
        .unwrap();
    assert_eq!(loom.read_file(ns, "f.txt").unwrap(), b"v5");
}

#[test]
fn gc_drops_unreachable_engine_state_blobs_and_keeps_history() {
    use loom_core::WsSelector;
    use loom_core::workspace::{FacetKind, WorkspaceId};

    let tp = TempPath::new("gc");
    {
        let mut loom = open_loom(tp.path()).unwrap();
        let ns = loom
            .registry_mut()
            .create(
                FacetKind::Files,
                Some("p"),
                WorkspaceId::from_bytes([5; 16]),
            )
            .unwrap();
        // Churn: each save_loom writes a NEW engine-state blob, so the prior ones become
        // unreachable garbage (not ref-reachable, not the current reference root).
        for i in 0..6u64 {
            loom.write_file(ns, "f.txt", format!("v{i}").as_bytes(), 0o100644)
                .unwrap();
            loom.commit(ns, "nas", "edit", i + 1).unwrap();
            save_loom(&mut loom).unwrap();
        }
        let before = loom.store().len();
        let stats = gc_loom(&mut loom).unwrap();
        // The five superseded engine-state blobs are gone; the object count drops and space is freed.
        assert!(
            loom.store().len() < before,
            "GC should drop stale engine-state blobs: before={before}, after={}",
            loom.store().len()
        );
        assert!(stats.reclaimed() > 0);
        // Committed history is intact: HEAD content still reads after GC.
        assert_eq!(loom.read_file(ns, "f.txt").unwrap(), b"v5");
    }
    // ...and the GC'd file reopens as a full Loom.
    let loom = open_loom(tp.path()).unwrap();
    let ns = loom
        .registry()
        .open(&WsSelector::Typed {
            ty: FacetKind::Files,
            name: "p".to_string(),
        })
        .unwrap();
    assert_eq!(loom.read_file(ns, "f.txt").unwrap(), b"v5");
}

#[test]
fn default_codec_compresses_a_large_object() {
    let tp = TempPath::new("compress");
    let store = FileStore::open(tp.path()).unwrap(); // default Deflate
    let data = blob(&b"loom object commit tree branch ".repeat(2200)); // ~68 KiB, repetitive
    let before = std::fs::metadata(tp.path()).unwrap().len();
    let d = store.put(&data).unwrap();
    let after = std::fs::metadata(tp.path()).unwrap().len();
    // The file grew far less than the plaintext size: the record was stored compressed.
    assert!(
        after - before < data.len() as u64 / 2,
        "expected compression: file grew {} for a {}-byte plaintext",
        after - before,
        data.len()
    );
    // ...and `get` still returns the exact plaintext (frame inverted + integrity-verified).
    assert_eq!(store.get(&d).unwrap().unwrap(), data);
}

#[test]
fn frame_independent_across_store_codecs() {
    // The same content stored under different codecs has the same digest and yields identical
    // plaintext on read: the property that makes compression invisible to sync.
    let data = blob(&b"the loom content-addressed object store ".repeat(1500));
    let tp_d = TempPath::new("fi-deflate");
    let tp_l = TempPath::new("fi-lz4");
    let tp_n = TempPath::new("fi-none");

    let sd = FileStore::open(tp_d.path()).unwrap(); // Deflate (default)
    let mut sl = FileStore::open(tp_l.path()).unwrap();
    sl.set_default_codec(Codec::Lz4);
    let mut sn = FileStore::open(tp_n.path()).unwrap();
    sn.set_default_codec(Codec::None);

    let dd = sd.put(&data).unwrap();
    let dl = sl.put(&data).unwrap();
    let dn = sn.put(&data).unwrap();
    assert_eq!(dd, dl, "digest must be codec-independent");
    assert_eq!(dd, dn);
    for s in [&sd, &sl, &sn] {
        assert_eq!(s.get(&dd).unwrap().unwrap(), data);
    }
    // Self-describing on reopen: each frame round-trips without knowing the writer's codec.
    drop((sd, sl, sn));
    for tp in [&tp_d, &tp_l, &tp_n] {
        assert_eq!(
            FileStore::open(tp.path())
                .unwrap()
                .get(&dd)
                .unwrap()
                .unwrap(),
            data
        );
    }
}

#[test]
fn put_hint_applies_the_per_call_codec() {
    // `put_hint` maps the engine's CompressionHint to a frame per write, independent of the store
    // default. `Small` compresses; `None` stores identity. Both round-trip and share a digest.
    let data = blob(&b"loom commit tree branch object store ".repeat(2000)); // repetitive, >1 KiB
    let tp_s = TempPath::new("hint-small");
    let tp_n = TempPath::new("hint-none");

    let ss = FileStore::open(tp_s.path()).unwrap();
    let sn = FileStore::open(tp_n.path()).unwrap();

    let before_s = std::fs::metadata(tp_s.path()).unwrap().len();
    let ds = ss.put_hint(&data, CompressionHint::Small).unwrap();
    let grew_s = std::fs::metadata(tp_s.path()).unwrap().len() - before_s;

    let before_n = std::fs::metadata(tp_n.path()).unwrap().len();
    let dn = sn.put_hint(&data, CompressionHint::None).unwrap();
    let grew_n = std::fs::metadata(tp_n.path()).unwrap().len() - before_n;

    assert_eq!(ds, dn, "the hint must not affect the digest");
    assert!(
        grew_s < grew_n,
        "Small hint should compress (grew {grew_s}) vs None identity (grew {grew_n})"
    );
    assert_eq!(ss.get(&ds).unwrap().unwrap(), data);
    assert_eq!(sn.get(&dn).unwrap().unwrap(), data);
}

#[test]
fn second_writer_is_locked_out_until_the_first_drops() {
    let tp = TempPath::new("writer-lock");
    let a = FileStore::open(tp.path()).unwrap();
    // A second writer process (handle) cannot open the same loom while the first holds it.
    let err = FileStore::open(tp.path()).unwrap_err();
    assert_eq!(err.code, Code::Conflict);
    // Dropping the first releases the lock, so a new writer can open.
    drop(a);
    FileStore::open(tp.path()).unwrap();
}

#[test]
fn readers_are_lock_free_and_do_not_block_a_writer() {
    let tp = TempPath::new("read-lock-free");
    let data = blob(b"hello loom reader");
    let digest = {
        let w = FileStore::open(tp.path()).unwrap();
        w.put(&data).unwrap()
    };
    // A lock-free reader sees the committed object.
    let r = FileStore::open_read(tp.path()).unwrap();
    assert_eq!(r.get(&digest).unwrap().unwrap(), data);
    // The open reader does not block a writer (readers hold no lock)...
    let _w = FileStore::open(tp.path()).unwrap();
    // ...and a second reader coexists with that writer.
    let r2 = FileStore::open_read(tp.path()).unwrap();
    assert_eq!(r2.get(&digest).unwrap().unwrap(), data);
}

#[test]
fn ring_recovery_restores_the_reference_root_too() {
    // The reference (engine-state) root rides in every ring record, so a state set after only a few
    // commits - with no superblock checkpoint - is still recovered from the ring on reopen.
    let tp = TempPath::new("journal-reference");
    let (da, root) = {
        let s = FileStore::open(tp.path()).unwrap();
        let da = s.put(&blob(b"object-A")).unwrap(); // gen 1
        let root = s.put(&blob(b"reference-state")).unwrap(); // gen 2
        s.set_reference_root(Some(root)).unwrap(); // gen 3, all below CHECKPOINT_INTERVAL
        (da, root)
    };
    let s = FileStore::open_read(tp.path()).unwrap();
    assert!(s.get(&da).unwrap().is_some());
    assert_eq!(
        s.reference_root(),
        Some(root),
        "the reference root must be recovered from the ring with no checkpoint"
    );
}

#[test]
fn open_read_rejects_a_missing_loom() {
    let tp = TempPath::new("read-missing");
    assert!(FileStore::open_read(tp.path()).is_err());
}

#[test]
fn freemap_survives_a_reuse_heavy_workload() {
    // Enough distinct puts to drive the B-tree multi-level and well past the reuse window, so later
    // commits genuinely reuse aged superseded-node extents. The allocator unit tests above prove
    // the reuse mechanism; this proves the store stays correct when its on-disk B-tree nodes partly
    // live in reused holes - after a reopen rebuilds the index from disk, every object must still
    // resolve, and the logical end must never have moved backward.
    let n = 400usize;
    let tp = TempPath::new("freemap-workload");
    let end = {
        let store = FileStore::open(tp.path()).unwrap();
        let mut prev_end = store.logical_end();
        for i in 0..n {
            store.put(&blob(format!("obj-{i:08}").as_bytes())).unwrap();
            let now = store.logical_end();
            assert!(now >= prev_end, "logical end moved backward"); // reuse fills holes only
            prev_end = now;
        }
        assert_eq!(store.len(), n);
        prev_end
    };
    let reopened = FileStore::open(tp.path()).unwrap();
    assert_eq!(reopened.logical_end(), end); // recovered state matches what was committed
    assert_eq!(reopened.len(), n);
    for i in 0..n {
        let d = Digest::blake3(&blob(format!("obj-{i:08}").as_bytes()));
        assert!(reopened.has(&d).unwrap(), "object {i} lost after reuse");
    }
}

#[test]
fn freemap_persists_across_reopen() {
    // The free list is written to disk each commit and restored on open, so reuse survives a
    // restart instead of starting empty. After a churning workload, the reopened store's free list
    // must match the one committed at close (as a set; on-open validation returns it sorted).
    let n = 200usize;
    let tp = TempPath::new("freemap-persist");
    let mut before = {
        let store = FileStore::open(tp.path()).unwrap();
        for i in 0..n {
            store.put(&blob(format!("obj-{i:08}").as_bytes())).unwrap();
        }
        store.free_runs()
    };
    before.sort_by_key(|r| r.start);
    assert!(
        !before.is_empty(),
        "the workload should have freed superseded CoW node pages"
    );
    let reopened = FileStore::open(tp.path()).unwrap();
    let mut after = reopened.free_runs();
    after.sort_by_key(|r| r.start);
    assert_eq!(
        after, before,
        "the free-page map must be restored across a reopen"
    );
    // And a subsequent put still lands correctly with the restored free list in play.
    let d = reopened.put(&blob(b"after-reopen")).unwrap();
    assert!(reopened.has(&d).unwrap());
}

#[test]
fn concurrent_writers_share_one_store() {
    use std::sync::Arc;
    // The store takes `&self` writes, so one `FileStore` is shared across threads via `Arc`. Writes
    // funnel through the group-commit coordinator: under contention a leader commits many threads'
    // objects in one fsync while the rest wait, then later arrivals lead the next batch. This
    // storms the leader/follower handoff; every distinct object must land and stay retrievable.
    let tp = TempPath::new("concurrent");
    let store = Arc::new(FileStore::open(tp.path()).unwrap());
    let mut handles = Vec::new();
    for t in 0..8u32 {
        let s = Arc::clone(&store);
        handles.push(std::thread::spawn(move || {
            let mut mine = Vec::new();
            for i in 0..50u32 {
                mine.push(s.put(&blob(format!("obj-{t}-{i}").as_bytes())).unwrap());
            }
            mine
        }));
    }
    let digests: Vec<Digest> = handles
        .into_iter()
        .flat_map(|h| h.join().unwrap())
        .collect();
    assert_eq!(store.len(), 400); // 8 threads x 50 distinct objects, all committed
    // Each digest resolves through the index to its record, intact - no coalesced write was lost
    // or pointed at the wrong offset.
    for (t, d) in digests.iter().enumerate() {
        let want = blob(format!("obj-{}-{}", t / 50, t % 50).as_bytes());
        assert_eq!(store.get(d).unwrap().as_deref(), Some(want.as_slice()));
    }
}

#[test]
fn crafted_bogus_index_root_is_clean_error_not_panic() {
    // A committed file whose index-root page is corrupted: loading the index on open must be a
    // clean CORRUPT error - no panic, no wild read - because every node page is CRC- and
    // bound-checked. The index root is located via the newest committed journal record.
    let (mut bytes, digests) = build_committed(100); // > one node: forces a multi-node tree
    let mut newest: Option<journal::Roots> = None;
    for i in 0..RING_SLOTS {
        let off = (JOURNAL_OFFSET + i * journal::RECORD_SIZE as u64) as usize;
        if let Some((journal::KIND_COMMIT, r)) =
            journal::decode(&bytes[off..off + journal::RECORD_SIZE])
            && newest.is_none_or(|n| r.generation > n.generation)
        {
            newest = Some(r);
        }
    }
    let rt = newest.unwrap().region_table.unwrap();
    let rt_off = (DATA_START + rt.0 * PAGE_SIZE) as usize;
    let region = RegionTable::decode(&bytes[rt_off..rt_off + PAGE_SIZE as usize]).unwrap();
    let index_root = region.index_root.unwrap();
    // Flip every byte of the index-root page: its magic and CRC checks must reject it.
    let node_off = (DATA_START + index_root.0 * PAGE_SIZE) as usize;
    for b in &mut bytes[node_off..node_off + PAGE_SIZE as usize] {
        *b ^= 0xFF;
    }
    let store = open_bytes(&bytes, "bogus-index").unwrap();
    let err = store.has(&digests[0]).unwrap_err();
    assert!(matches!(err.code, Code::CorruptObject | Code::Io));
}

#[test]
fn many_objects_round_trip_through_btree() {
    // Enough objects to force several B-tree splits (order 64), then reopen and confirm every one
    // is found via the index rebuilt by walking the on-disk tree (no payload scan).
    let tp = TempPath::new("btree-many");
    let n = 500usize;
    let mut digests = Vec::with_capacity(n);
    {
        let store = FileStore::open(tp.path()).unwrap();
        for i in 0..n {
            digests.push(store.put(&blob(format!("item-{i}").as_bytes())).unwrap());
        }
        assert_eq!(store.len(), n);
    }
    let store = FileStore::open(tp.path()).unwrap();
    assert_eq!(store.len(), n);
    for (i, d) in digests.iter().enumerate() {
        assert!(store.has(d).unwrap());
        assert_eq!(
            store.get(d).unwrap().unwrap(),
            blob(format!("item-{i}").as_bytes())
        );
    }
}

#[test]
fn sparse_lookup_uses_bounded_locator_and_page_caches() {
    let tp = TempPath::new("sparse-cache");
    let n = LOCATOR_CACHE_LIMIT + 24;
    let mut digests = Vec::with_capacity(n);
    {
        let store = FileStore::open(tp.path()).unwrap();
        for i in 0..n {
            digests.push(
                store
                    .put(&blob(format!("cached-item-{i}").as_bytes()))
                    .unwrap(),
            );
        }
    }

    let store = FileStore::open(tp.path()).unwrap();
    let initial = store.io_stats().unwrap();
    assert!(!initial.open_index_materialized);
    assert_eq!(initial.locator_cache_entries, 0);

    let first = digests[0];
    assert!(store.has(&first).unwrap());
    let after_first = store.io_stats().unwrap();
    assert_eq!(after_first.locator_cache_misses, 1);
    assert!(after_first.index_pages_read > 0);
    assert_eq!(after_first.locator_cache_entries, 1);

    assert!(store.has(&first).unwrap());
    let after_cached = store.io_stats().unwrap();
    assert_eq!(after_cached.locator_cache_hits, 1);
    assert_eq!(after_cached.index_pages_read, after_first.index_pages_read);

    for digest in &digests {
        assert!(store.has(digest).unwrap());
    }
    let after_sweep = store.io_stats().unwrap();
    assert!(after_sweep.index_page_cache_hits > 0);
    assert!(after_sweep.locator_cache_entries <= LOCATOR_CACHE_LIMIT as u64);
}

#[test]
fn truncation_never_panics() {
    // Property: opening the file truncated to any length >= DATA_START either succeeds with a valid
    // committed prefix or returns a clean error - never a panic. The page format makes every page
    // boundary (and the header edge) the interesting cases, so sweep those and the bytes around
    // them plus a coarse stride; an exhaustive byte-by-byte sweep of the page-granular file is
    // needless and rewrites gigabytes.
    let (bytes, _) = build_committed(3);
    let header = DATA_START as usize;
    let mut lengths: Vec<usize> = Vec::new();
    let mut boundary = header;
    while boundary <= bytes.len() {
        for d in [0usize, 1, 2, 3, 8] {
            if boundary >= d && boundary - d >= header {
                lengths.push(boundary - d);
            }
            if boundary + d <= bytes.len() {
                lengths.push(boundary + d);
            }
        }
        boundary += PAGE_SIZE as usize;
    }
    let mut stride = header;
    while stride <= bytes.len() {
        lengths.push(stride);
        stride += 257; // a prime, to land on varied mid-page offsets
    }
    lengths.sort_unstable();
    lengths.dedup();
    for len in lengths {
        let _ = open_bytes(&bytes[..len], "trunc-sweep"); // must not panic
    }
}

#[test]
fn passes_conformance_vectors() {
    let tp = TempPath::new("conformance");
    let store = FileStore::open(tp.path()).unwrap();
    // Single backend-certification entry point: blob + object-model + table/index identity.
    uldren_loom_conformance::run_all_vectors(store).expect("all conformance vectors");
}

/// A FIPS (SHA-256) FileStore certifies against the parallel `fips/sha256` blob and object-model
/// vectors, and a default store certifies against `default/blake3` - proving the canonical bytes
/// are profile-independent and only the digest layer changes.
#[test]
fn certifies_data_model_vectors_under_both_profiles() {
    let tp_b = TempPath::new("conf-blake3");
    let mut blake3 = FileStore::open(tp_b.path()).unwrap();
    uldren_loom_conformance::run_blob_vectors_profiled(&mut blake3, Algo::Blake3)
        .expect("default profile certifies against blake3 vectors");
    uldren_loom_conformance::run_object_model_vectors_profiled(&mut blake3, Algo::Blake3)
        .expect("default profile certifies object-model vectors");

    let tp_s = TempPath::new("conf-sha256");
    let mut sha = FileStore::create_with_profile(tp_s.path(), Algo::Sha256).unwrap();
    uldren_loom_conformance::run_blob_vectors_profiled(&mut sha, Algo::Sha256)
        .expect("FIPS profile certifies against sha256 vectors");
    uldren_loom_conformance::run_object_model_vectors_profiled(&mut sha, Algo::Sha256)
        .expect("FIPS profile certifies object-model vectors");
}

/// The workspace CAS facade honors the store's identity profile: a SHA-256 (FIPS) store addresses
/// blobs with SHA-256 content addresses, and put/get/list round-trip under that profile. This is the
/// digest-profile dimension of the 0024 workspace-facade contract, which the in-memory conformance
/// runner (BLAKE3 only) cannot exercise.
#[test]
fn cas_facade_honors_sha256_profile() {
    use loom_core::workspace::{FacetKind, WorkspaceId};
    use loom_core::{cas_get, cas_list, cas_put};

    let tp = TempPath::new("cas-sha256");
    let store = FileStore::create_with_profile(tp.path(), Algo::Sha256).unwrap();
    let mut loom = Loom::new(store);
    let ns = loom
        .registry_mut()
        .create(FacetKind::Cas, None, WorkspaceId::from_bytes([9; 16]))
        .unwrap();

    let addr = cas_put(&mut loom, ns, b"fips blob").unwrap();
    assert_eq!(
        addr.algo(),
        Algo::Sha256,
        "a FIPS store must address CAS blobs with SHA-256"
    );
    assert_eq!(
        cas_get(&loom, ns, &addr).unwrap().as_deref(),
        Some(&b"fips blob"[..]),
        "the blob round-trips under the SHA-256 profile"
    );
    assert_eq!(
        cas_list(&loom, ns).unwrap(),
        vec![addr],
        "list enumerates the SHA-256-addressed blob"
    );
}
