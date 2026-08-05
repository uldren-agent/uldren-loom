use super::*;
use crate::pagemap::{FreeMapExtentUpdate, FreePageRun, PageAllocator};
use loom_core::{Object, ObjectStore, OverlayKey, ReachabilityMarkState};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TEST_PATH_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct TestStorePath(PathBuf);

impl TestStorePath {
    fn new(name: &str) -> Self {
        let sequence = TEST_PATH_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "uldren-loom-storage-invariant-{name}-{}-{sequence}.loom",
            std::process::id()
        ));
        let _ = fs::remove_file(&path);
        Self(path)
    }

    fn as_path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestStorePath {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

#[derive(Debug)]
struct CommittedStorageSnapshot {
    generation: u64,
    page_count: u64,
    roots: Vec<(String, PageId)>,
    authority_free: Vec<FreePageRun>,
    persisted_free: Vec<FreePageRun>,
    metadata_bootstrap_pages: Vec<u64>,
}

fn committed_storage_snapshot(store: &FileStore) -> CommittedStorageSnapshot {
    let (generation, page_count, roots, authority_free, freemap, metadata_bootstrap_pages) = {
        let inner = store.inner.lock().expect("store inner lock");
        let mut roots = Vec::new();
        for (name, root) in [
            ("object_index", inner.index_root),
            ("legacy_overlay", inner.overlay_root),
            ("current_records", inner.current_record_root),
            ("root_catalog", inner.root_catalog_root),
            ("region_table", inner.region_table_root),
            ("maintenance", inner.maintenance_root),
            ("free_map", inner.freemap.map(|(root, _)| root)),
        ] {
            if let Some(root) = root {
                roots.push((name.to_string(), root));
            }
        }
        for entry in &inner.root_catalog_entries {
            let family_name = ROOT_FAMILY_REGISTRY
                .iter()
                .find(|descriptor| descriptor.family_id == entry.family_id)
                .map_or("unknown", |descriptor| descriptor.name);
            roots.push((
                format!("root_catalog:{family_name}:{}", entry.family_id),
                entry.root,
            ));
        }
        let metadata_bootstrap_pages = inner
            .metadata_bootstrap_reserve
            .extents
            .iter()
            .flat_map(|extent| extent.start..extent.start.saturating_add(extent.len))
            .collect::<Vec<_>>();
        (
            inner.generation,
            inner.page_count,
            roots,
            inner.free.clone(),
            inner.freemap,
            metadata_bootstrap_pages,
        )
    };
    let persisted_free = match freemap {
        Some((root, _)) => {
            let mut file = store.file.lock().expect("store backing lock");
            pagemap::read_map_with_root_span(&mut **file, DATA_START, root, page_count)
                .expect("decode committed free-map tree")
                .0
        }
        None => Vec::new(),
    };
    CommittedStorageSnapshot {
        generation,
        page_count,
        roots,
        authority_free,
        persisted_free,
        metadata_bootstrap_pages,
    }
}

fn free_run_containing(runs: &[FreePageRun], page: u64) -> Option<FreePageRun> {
    runs.iter()
        .copied()
        .find(|run| page >= run.start && page < run.start.saturating_add(run.len))
}

fn assert_committed_storage_invariants(store: &FileStore, operation: usize, label: &str) {
    let snapshot = committed_storage_snapshot(store);
    assert_eq!(
        snapshot.persisted_free, snapshot.authority_free,
        "operation={operation} label={label} generation={} offending_root=free_map free-map mismatch; persisted={:?} authority={:?}",
        snapshot.generation, snapshot.persisted_free, snapshot.authority_free
    );
    for (family, root) in &snapshot.roots {
        assert!(
            root.0 < snapshot.page_count,
            "operation={operation} label={label} generation={} offending_root={family} page={} page_count={} decoded_free={:?}",
            snapshot.generation,
            root.0,
            snapshot.page_count,
            snapshot.persisted_free
        );
        assert!(
            free_run_containing(&snapshot.persisted_free, root.0).is_none(),
            "operation={operation} label={label} generation={} offending_root={family} page={} decoded_free={:?}",
            snapshot.generation,
            root.0,
            snapshot.persisted_free
        );
    }
    for page in snapshot.metadata_bootstrap_pages {
        assert!(
            free_run_containing(&snapshot.persisted_free, page).is_none(),
            "operation={operation} label={label} generation={} metadata_bootstrap_page={page} decoded_free={:?}",
            snapshot.generation,
            snapshot.persisted_free
        );
    }
}

fn assert_visible_payloads(
    store: &FileStore,
    operation: usize,
    live_digest: Option<Digest>,
    current: Option<(&OverlayKey, &[u8])>,
) {
    if let Some(digest) = live_digest {
        assert!(
            store.has(&digest).expect("read live digest presence"),
            "operation={operation} generation={} live digest missing",
            store.generation()
        );
        assert_eq!(
            store.get(&digest).expect("read live digest payload"),
            Some(Object::Blob(b"persistent-live".to_vec()).canonical()),
            "operation={operation} generation={} live payload mismatch",
            store.generation()
        );
    }
    if let Some((key, expected)) = current {
        let actual = store
            .mutable_overlay_snapshot()
            .expect("open mutable snapshot")
            .read_composite(key, |_| Ok(None))
            .expect("read mutable payload");
        assert_eq!(
            actual.as_deref(),
            Some(expected),
            "operation={operation} generation={} current payload mismatch",
            store.generation()
        );
    }
}

#[test]
fn storage_invariant_committed_roots_and_free_map_match_after_reopen() {
    let path = TestStorePath::new("roots-free-map");
    let store = FileStore::open(path.as_path()).expect("create store");
    assert_committed_storage_invariants(&store, 0, "create");

    let digest = store
        .put(&Object::Blob(b"persistent-live".to_vec()).canonical())
        .expect("commit object");
    assert_committed_storage_invariants(&store, 1, "object put");

    let key = OverlayKey::from_segments([
        b"storage-invariant",
        b"workspace",
        b"current",
        b"record",
        b"root",
        b"one",
    ])
    .expect("overlay key");
    store
        .put_mutable_overlay_value(key.clone(), b"current-v1".to_vec())
        .expect("commit current record");
    assert_committed_storage_invariants(&store, 2, "current record");

    store
        .commit_family_root_records_for_test(
            RETAINED_HISTORY_FAMILY_ID,
            &[([0x31; 32], b"history-v1".to_vec())],
        )
        .expect("commit family root");
    assert_committed_storage_invariants(&store, 3, "family root");
    assert_visible_payloads(&store, 3, Some(digest), Some((&key, b"current-v1")));

    drop(store);
    let reopened = FileStore::open(path.as_path()).expect("reopen store");
    assert_committed_storage_invariants(&reopened, 4, "reopen");
    assert_visible_payloads(&reopened, 4, Some(digest), Some((&key, b"current-v1")));
}

#[test]
fn storage_invariant_bounded_state_machine_survives_mark_gc_and_reopen() {
    let path = TestStorePath::new("state-machine");
    let mut store = FileStore::open(path.as_path()).expect("create store");
    let key = OverlayKey::from_segments([
        b"storage-invariant",
        b"workspace",
        b"state",
        b"machine",
        b"current",
        b"one",
    ])
    .expect("overlay key");
    let mut operation = 0usize;
    let digest = store
        .put(&Object::Blob(b"persistent-live".to_vec()).canonical())
        .expect("put live object");
    operation += 1;
    let live_digest = Some(digest);
    assert_committed_storage_invariants(&store, operation, "allocate object record");
    assert_visible_payloads(&store, operation, live_digest, None);

    let mut current = Some(b"current-v1".to_vec());
    store
        .put_mutable_overlay_value(key.clone(), current.clone().unwrap_or_default())
        .expect("put current record");
    operation += 1;
    assert_committed_storage_invariants(&store, operation, "current record update");
    assert_visible_payloads(
        &store,
        operation,
        live_digest,
        current.as_deref().map(|value| (&key, value)),
    );

    store
        .commit_family_root_records_for_test(
            RETAINED_HISTORY_FAMILY_ID,
            &[([0x41; 32], b"family-v1".to_vec())],
        )
        .expect("put family record");
    operation += 1;
    assert_committed_storage_invariants(&store, operation, "family root update");
    assert_visible_payloads(
        &store,
        operation,
        live_digest,
        current.as_deref().map(|value| (&key, value)),
    );

    let initial_free = FreePageRun {
        start: 5,
        len: 2,
        freed_gen: 1,
    };
    let mut allocator =
        PageAllocator::new_with_reusable_runs(16, 20, vec![initial_free], vec![initial_free]);
    let reused = allocator.alloc(1);
    assert_eq!(reused, PageId(5));
    operation += 1;
    assert_committed_storage_invariants(&store, operation, "allocator committed reuse");
    assert_visible_payloads(
        &store,
        operation,
        live_digest,
        current.as_deref().map(|value| (&key, value)),
    );
    allocator.free(reused, 1).expect("transaction-local free");
    operation += 1;
    assert_committed_storage_invariants(&store, operation, "allocator transaction free");
    assert_visible_payloads(
        &store,
        operation,
        live_digest,
        current.as_deref().map(|value| (&key, value)),
    );
    assert_eq!(allocator.alloc(1), reused);
    operation += 1;
    assert_committed_storage_invariants(&store, operation, "allocator transaction reuse");
    assert_visible_payloads(
        &store,
        operation,
        live_digest,
        current.as_deref().map(|value| (&key, value)),
    );

    let state = ReachabilityMarkState {
        pinned: BTreeSet::new(),
        marked: BTreeSet::from([digest]),
        queue: VecDeque::new(),
        stream_roots: VecDeque::new(),
        content_roots: VecDeque::new(),
        prolly_cursors: VecDeque::new(),
        completed: true,
    };
    let mut epoch = store
        .begin_reachability_mark_epoch(
            store.reference_root(),
            store.derived_artifact_roots().expect("derived roots"),
            state,
        )
        .expect("begin mark epoch");
    operation += 1;
    assert_committed_storage_invariants(&store, operation, "begin mark epoch");
    assert_visible_payloads(
        &store,
        operation,
        live_digest,
        current.as_deref().map(|value| (&key, value)),
    );

    current = Some(b"current-after-snapshot".to_vec());
    store
        .put_mutable_overlay_value(key.clone(), current.clone().unwrap_or_default())
        .expect("post-snapshot mutation");
    operation += 1;
    assert_committed_storage_invariants(&store, operation, "post-snapshot mutation");
    assert_visible_payloads(
        &store,
        operation,
        live_digest,
        current.as_deref().map(|value| (&key, value)),
    );

    while !epoch.metadata_completed {
        assert!(
            operation < 35,
            "bounded mark traversal exceeded operation budget"
        );
        let visited = store
            .step_reachability_metadata_mark_epoch(&mut epoch, 8, None)
            .expect("advance bounded mark slice");
        assert!(visited > 0, "bounded mark traversal stalled");
        operation += 1;
        assert_committed_storage_invariants(&store, operation, "bounded mark slice");
        assert_visible_payloads(
            &store,
            operation,
            live_digest,
            current.as_deref().map(|value| (&key, value)),
        );
    }
    store
        .complete_reachability_mark_epoch(&epoch)
        .expect("complete mark epoch");
    operation += 1;
    assert_committed_storage_invariants(&store, operation, "complete mark epoch");
    assert_visible_payloads(
        &store,
        operation,
        live_digest,
        current.as_deref().map(|value| (&key, value)),
    );

    store
        .gc_validated_segments(GcSegmentBudget::unlimited())
        .expect("validated reclaim");
    operation += 1;
    assert_committed_storage_invariants(&store, operation, "validated reclaim");
    assert_visible_payloads(
        &store,
        operation,
        live_digest,
        current.as_deref().map(|value| (&key, value)),
    );

    drop(store);
    let reopened = FileStore::open(path.as_path()).expect("reopen after reclaim");
    operation += 1;
    assert_committed_storage_invariants(&reopened, operation, "reopen after reclaim");
    assert_visible_payloads(
        &reopened,
        operation,
        live_digest,
        current.as_deref().map(|value| (&key, value)),
    );

    current = Some(b"current-after-reopen".to_vec());
    reopened
        .put_mutable_overlay_value(key.clone(), current.clone().unwrap_or_default())
        .expect("mutate after reopen");
    operation += 1;
    assert!(operation <= 40, "state machine exceeded forty operations");
    assert_committed_storage_invariants(&reopened, operation, "mutation after reopen");
    assert_visible_payloads(
        &reopened,
        operation,
        live_digest,
        current.as_deref().map(|value| (&key, value)),
    );
}

#[derive(Clone, Copy)]
enum AllocatorOperation {
    Allocate { pages: u64, expected: u64 },
    Free { start: u64, pages: u64 },
}

struct AllocatorSequence {
    name: &'static str,
    page_count: u64,
    initial: Vec<FreePageRun>,
    operations: Vec<AllocatorOperation>,
}

fn free_pages(runs: &[FreePageRun]) -> BTreeSet<u64> {
    runs.iter()
        .flat_map(|run| run.start..run.start.saturating_add(run.len))
        .collect()
}

fn apply_extent_updates(
    initial: &[FreePageRun],
    updates: &[FreeMapExtentUpdate],
) -> Vec<FreePageRun> {
    let mut physical = initial
        .iter()
        .copied()
        .map(|run| (run.start, run))
        .collect::<BTreeMap<_, _>>();
    for update in updates {
        match update {
            FreeMapExtentUpdate::Delete(run) => {
                let end = run.start.saturating_add(run.len);
                physical.retain(|start, _| *start < run.start || *start >= end);
            }
            FreeMapExtentUpdate::Upsert(run) => {
                physical.insert(run.start, *run);
            }
        }
    }
    physical.into_values().collect()
}

fn page_generations(
    runs: &[FreePageRun],
    sequence: &str,
    operation: usize,
    layer: &str,
) -> BTreeMap<u64, u64> {
    let mut pages = BTreeMap::new();
    for run in runs {
        for page in run.start..run.start.saturating_add(run.len) {
            assert!(
                pages.insert(page, run.freed_gen).is_none(),
                "sequence={sequence} operation={operation} generation=20 offending_root=free_map obsolete_physical_extent_key={page} overlapping {layer} extents={runs:?}"
            );
        }
    }
    pages
}

fn runs_overlap(left: FreePageRun, right: FreePageRun) -> bool {
    left.start < right.start.saturating_add(right.len)
        && right.start < left.start.saturating_add(left.len)
}

fn assert_physical_extent_inventory(
    sequence: &AllocatorSequence,
    operation: usize,
    allocator: &PageAllocator,
) {
    let mut probe = allocator.clone();
    let updates = probe.take_free_map_extent_updates();
    let deletes = updates
        .iter()
        .filter_map(|update| match update {
            FreeMapExtentUpdate::Delete(run) => Some(run.start),
            FreeMapExtentUpdate::Upsert(_) => None,
        })
        .collect::<BTreeSet<_>>();
    for replacement in updates.iter().filter_map(|update| match update {
        FreeMapExtentUpdate::Delete(_) => None,
        FreeMapExtentUpdate::Upsert(run) => Some(*run),
    }) {
        for original in sequence
            .initial
            .iter()
            .copied()
            .filter(|original| original.start != replacement.start)
            .filter(|original| runs_overlap(*original, replacement))
        {
            assert!(
                deletes.contains(&original.start),
                "sequence={} operation={operation} generation=20 offending_root=free_map missing_delete={} replacement={replacement:?} updates={updates:?}",
                sequence.name,
                original.start
            );
        }
    }
    let physical = apply_extent_updates(&sequence.initial, &updates);
    let physical_pages = page_generations(&physical, sequence.name, operation, "physical");
    let authority = allocator.snapshot_free();
    let authority_pages = page_generations(&authority, sequence.name, operation, "authority");
    assert_eq!(
        physical_pages, authority_pages,
        "sequence={} operation={operation} generation=20 offending_root=free_map physical extent-key state differs from normalized authority; physical={physical:?} authority={authority:?} updates={updates:?}",
        sequence.name
    );
}

#[test]
fn storage_invariant_fixed_sequences_preserve_physical_free_map_identity() {
    let sequences = vec![
        AllocatorSequence {
            name: "adjacent committed extents",
            page_count: 32,
            initial: vec![
                FreePageRun {
                    start: 8,
                    len: 1,
                    freed_gen: 1,
                },
                FreePageRun {
                    start: 9,
                    len: 1,
                    freed_gen: 2,
                },
            ],
            operations: vec![
                AllocatorOperation::Allocate {
                    pages: 1,
                    expected: 8,
                },
                AllocatorOperation::Allocate {
                    pages: 1,
                    expected: 9,
                },
            ],
        },
        AllocatorSequence {
            name: "committed split reuse",
            page_count: 32,
            initial: vec![FreePageRun {
                start: 10,
                len: 4,
                freed_gen: 1,
            }],
            operations: vec![
                AllocatorOperation::Allocate {
                    pages: 2,
                    expected: 10,
                },
                AllocatorOperation::Allocate {
                    pages: 1,
                    expected: 12,
                },
            ],
        },
        AllocatorSequence {
            name: "transaction-local free and reuse",
            page_count: 20,
            initial: Vec::new(),
            operations: vec![
                AllocatorOperation::Allocate {
                    pages: 1,
                    expected: 20,
                },
                AllocatorOperation::Free {
                    start: 20,
                    pages: 1,
                },
                AllocatorOperation::Allocate {
                    pages: 1,
                    expected: 20,
                },
            ],
        },
        AllocatorSequence {
            name: "coalesced committed and transaction-local reuse",
            page_count: 32,
            initial: vec![FreePageRun {
                start: 10,
                len: 1,
                freed_gen: 1,
            }],
            operations: vec![
                AllocatorOperation::Allocate {
                    pages: 1,
                    expected: 10,
                },
                AllocatorOperation::Free {
                    start: 10,
                    pages: 1,
                },
                AllocatorOperation::Free { start: 9, pages: 1 },
                AllocatorOperation::Allocate {
                    pages: 1,
                    expected: 10,
                },
            ],
        },
    ];

    for sequence in sequences {
        let mut allocator = PageAllocator::new_with_reusable_runs(
            sequence.page_count,
            20,
            sequence.initial.clone(),
            sequence.initial.clone(),
        );
        let mut expected_free = free_pages(&sequence.initial);
        for (operation, action) in sequence.operations.iter().copied().enumerate() {
            match action {
                AllocatorOperation::Allocate { pages, expected } => {
                    let allocated = allocator.alloc(pages);
                    assert_eq!(
                        allocated,
                        PageId(expected),
                        "sequence={} operation={operation}",
                        sequence.name
                    );
                    for page in expected..expected.saturating_add(pages) {
                        expected_free.remove(&page);
                    }
                }
                AllocatorOperation::Free { start, pages } => {
                    allocator
                        .free(PageId(start), pages)
                        .expect("record allocator free");
                    expected_free.extend(start..start.saturating_add(pages));
                }
            }
            assert_eq!(
                free_pages(&allocator.snapshot_free()),
                expected_free,
                "sequence={} operation={operation} generation=20 offending_root=free_map logical free-set mismatch; decoded_free={:?}",
                sequence.name,
                allocator.snapshot_free()
            );
            assert_physical_extent_inventory(&sequence, operation, &allocator);
        }
    }
}

#[test]
#[ignore = "diagnostic: attributes the 512-page free-map publication admission boundary"]
fn diagnostic_storage_invariant_free_map_capacity_attribution() {
    const BASE_EXTENTS: u64 = 1_024;
    const DIRTY_RANGES_OVER_CAPACITY: u64 =
        pagemap::FOREGROUND_TRANSACTION_METADATA_LIMIT_PAGES + 1;

    let path = TestStorePath::new("free-map-capacity-attribution");
    let store = FileStore::open(path.as_path()).expect("create attribution store");
    let (free_start, base_page_count) = {
        let inner = store.inner.lock().expect("store inner lock");
        let free_start = inner.page_count;
        let base_page_count = free_start
            .saturating_add(BASE_EXTENTS.saturating_mul(2))
            .saturating_add(1);
        (free_start, base_page_count)
    };
    let base_extents = (0..BASE_EXTENTS)
        .map(|index| FreePageRun {
            start: free_start.saturating_add(index.saturating_mul(2)),
            len: 1,
            freed_gen: 1,
        })
        .collect::<Vec<_>>();
    let base_updates = base_extents
        .iter()
        .copied()
        .map(FreeMapExtentUpdate::Upsert)
        .collect::<Vec<_>>();

    let (root, source_page_count, depth) = {
        let mut file = store.file.lock().expect("store backing lock");
        file.grow(DATA_START + base_page_count.saturating_mul(PAGE_SIZE))
            .expect("grow attribution backing");
        let mut allocator = PageAllocator::new(base_page_count, 2, Vec::new());
        let root = pagemap::write_tree_map(
            &mut **file,
            DATA_START,
            &mut allocator,
            None,
            &[],
            base_updates,
        )
        .expect("write persisted physical extent keys")
        .expect("free-map root");
        let source_page_count = allocator.page_count();
        let depth = pagebtree::free_page_extent_tree_depth(
            &mut **file,
            DATA_START,
            root,
            source_page_count,
        )
        .expect("free-map depth");
        (root, source_page_count, depth)
    };

    let prepare = |dirty_ranges: u64| {
        let mut allocator = PageAllocator::new(source_page_count, 3, base_extents.clone());
        for index in 0..dirty_ranges {
            allocator
                .free(
                    PageId(
                        free_start
                            .saturating_add(index.saturating_mul(2))
                            .saturating_add(1),
                    ),
                    1,
                )
                .expect("record disjoint freed page");
        }
        let updates = allocator.take_free_map_extent_updates();
        let deletes = updates
            .iter()
            .filter(|update| matches!(update, FreeMapExtentUpdate::Delete(_)))
            .count() as u64;
        let upserts = updates
            .iter()
            .filter(|update| matches!(update, FreeMapExtentUpdate::Upsert(_)))
            .count() as u64;
        let unique_starts = updates
            .iter()
            .map(|update| match update {
                FreeMapExtentUpdate::Delete(run) | FreeMapExtentUpdate::Upsert(run) => run.start,
            })
            .collect::<BTreeSet<_>>()
            .len() as u64;
        let prepared = {
            let mut file = store.file.lock().expect("store backing lock");
            pagemap::prepare_tree_map_publication(
                &mut **file,
                DATA_START,
                Some(root),
                &base_extents,
                updates.clone(),
                updates,
                source_page_count,
            )
            .expect("prepare free-map publication")
        };
        let demand = prepared.demand();
        (deletes, upserts, unique_starts, demand)
    };

    let bounded = prepare(1);
    let over_capacity = prepare(DIRTY_RANGES_OVER_CAPACITY);
    let reserve_available_pages = pagemap::METADATA_BOOTSTRAP_CAPACITY_PAGES;
    eprintln!(
        "free_map_capacity_attribution base_physical_keys={BASE_EXTENTS} free_map_depth={depth} reserve_available_pages={reserve_available_pages} bounded={{dirty_logical_ranges:{}, deletes:{}, upserts:{}, existing_btree_nodes_touched:{}, split_pages:{}, total_demanded_pages:{}}} large={{dirty_logical_ranges:{}, unique_dirty_starts:{}, deletes:{}, upserts:{}, existing_btree_nodes_touched:{}, split_pages:{}, total_demanded_pages:{}}}",
        bounded.0.saturating_add(bounded.1),
        bounded.3.extent_deletes,
        bounded.3.extent_upserts,
        bounded.3.affected_existing_btree_pages,
        bounded.3.split_decisions,
        bounded.3.allocation_pages(),
        over_capacity.0.saturating_add(over_capacity.1),
        over_capacity.2,
        over_capacity.3.extent_deletes,
        over_capacity.3.extent_upserts,
        over_capacity.3.affected_existing_btree_pages,
        over_capacity.3.split_decisions,
        over_capacity.3.allocation_pages(),
    );

    assert_eq!(bounded.0, 0);
    assert_eq!(bounded.1, 1);
    assert_eq!(bounded.2, 1);
    assert!(bounded.3.allocation_pages() < reserve_available_pages);

    assert_eq!(over_capacity.0, 0);
    assert_eq!(over_capacity.1, DIRTY_RANGES_OVER_CAPACITY);
    assert_eq!(over_capacity.2, DIRTY_RANGES_OVER_CAPACITY);
    assert!(over_capacity.3.allocation_pages() > bounded.3.allocation_pages());
}
