use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use loom_core::error::Code;
use loom_core::{Algo, FacetKind, Loom, WorkspaceId};
use loom_pages::PageCreateRequest;
use loom_store::{
    BtreeBatchTransactionPageStats, FileStore, ForegroundAllocatorPageStats, GcSegmentBudget,
    ObjectIndexBatchPageStats, REUSE_SAFE_GENERATION_WINDOW, StoreBtreeRootDepth,
    begin_loom_reachability_mark_epoch, gc_loom_validated_segments,
    install_rejected_free_map_publication_test_observer, open_loom, save_loom,
    step_loom_reachability_mark_epoch, take_btree_batch_transaction_page_stats,
    take_foreground_allocator_page_stats, take_object_index_batch_page_stats,
};
use loom_tickets::{TicketCreateRequest, TicketUpdateRequest};
use serde_json::json;
use uldren_loom_mcp::writes::{LaneCreateRequest, LaneUpdateRequest};
use uldren_loom_mcp::{LoomMcp, StoreAccess};

const STORE_PAGE_BYTES: i128 = 4_096;
const WINDOW_OPERATIONS: u64 = REUSE_SAFE_GENERATION_WINDOW;
const MEASURED_WINDOWS: usize = 3;
const WARM_OPERATIONS: u64 = REUSE_SAFE_GENERATION_WINDOW + 1;
const WINDOW_LATENCY_CEILING_PER_OPERATION: Option<Duration> = None;

static NEXT_PATH: AtomicU64 = AtomicU64::new(0);

struct TempStore(PathBuf);

impl TempStore {
    fn new(label: &str) -> Self {
        let sequence = NEXT_PATH.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "loom-storage-amplification-{label}-{}-{sequence}.loom",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

#[derive(Clone, Copy, Debug)]
struct RecoveryCycle {
    mark_slices: u64,
    mark_visited: u64,
    pages_freed: u64,
}

fn complete_recovery_cycle(path: &Path, restart_foreground_epoch: bool) -> RecoveryCycle {
    let mut loom = open_loom(path).expect("open store for reachability and validated GC");
    if loom
        .store()
        .active_reachability_mark_epoch()
        .expect("inspect active reachability epoch")
        .is_none()
    {
        begin_loom_reachability_mark_epoch(&loom).expect("begin reachability epoch");
    }
    let mut mark_slices = 0u64;
    let mut mark_visited = 0u64;
    loop {
        let step =
            step_loom_reachability_mark_epoch(&loom, 4_096).expect("advance reachability epoch");
        mark_slices = mark_slices.saturating_add(1);
        mark_visited = mark_visited.saturating_add(step.visited as u64);
        if step.completed {
            break;
        }
        assert!(mark_slices <= 1_024, "reachability epoch did not converge");
    }
    let gc = gc_loom_validated_segments(&mut loom, GcSegmentBudget::unlimited())
        .expect("reclaim validated stale pages");
    if restart_foreground_epoch {
        begin_loom_reachability_mark_epoch(&loom).expect("begin foreground reachability epoch");
    }
    RecoveryCycle {
        mark_slices,
        mark_visited,
        pages_freed: gc.pages_freed,
    }
}

impl Drop for TempStore {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[derive(Clone, Copy, Debug)]
struct StorageSample {
    generation: u64,
    physical_bytes: u64,
    physical_pages: u64,
    compacted_bytes: u64,
    active_tree_pages: u64,
    stale_tree_pages: u64,
    stale_tree_bytes: u64,
    stale_recovery_bytes: u64,
    retained_payload_bytes: u64,
    live_tree_pages: u64,
    live_tree_bytes: u64,
    free_map_metadata_pages: u64,
    reusable_free_pages: u64,
    reusable_free_bytes: u64,
    reclaimable_bytes: u64,
    stale_acceptance_bytes: u64,
    pinned_reader_blockers: Option<u64>,
}

#[derive(Clone, Copy, Debug, Default)]
struct MetricDelta {
    retained_payload_bytes: i128,
    compacted_bytes: i128,
    active_physical_bytes: i128,
    physical_pages: i128,
    retained_physical_bytes_after: u64,
    compacted_bytes_after: u64,
    active_tree_pages: i128,
    live_tree_pages: i128,
    live_tree_bytes: i128,
    stale_tree_pages: i128,
    stale_tree_bytes: i128,
    stale_recovery_bytes: i128,
    generations: u64,
    mutation_generations: u64,
    free_map_metadata_pages_after: u64,
    window_latency: Duration,
    reusable_free_pages_after: u64,
    reusable_free_bytes_after: u64,
    reclaimable_bytes_after: u64,
    stale_acceptance_bytes_after: u64,
    pinned_reader_blockers_after: Option<u64>,
}

impl MetricDelta {
    fn between(before: StorageSample, after: StorageSample) -> Self {
        Self {
            retained_payload_bytes: i128::from(after.retained_payload_bytes)
                - i128::from(before.retained_payload_bytes),
            compacted_bytes: i128::from(after.compacted_bytes) - i128::from(before.compacted_bytes),
            active_physical_bytes: i128::from(after.physical_bytes)
                - i128::from(before.physical_bytes),
            physical_pages: i128::from(after.physical_pages) - i128::from(before.physical_pages),
            retained_physical_bytes_after: after.physical_bytes,
            compacted_bytes_after: after.compacted_bytes,
            active_tree_pages: i128::from(after.active_tree_pages)
                - i128::from(before.active_tree_pages),
            live_tree_pages: i128::from(after.live_tree_pages) - i128::from(before.live_tree_pages),
            live_tree_bytes: i128::from(after.live_tree_bytes) - i128::from(before.live_tree_bytes),
            stale_tree_pages: i128::from(after.stale_tree_pages)
                - i128::from(before.stale_tree_pages),
            stale_tree_bytes: i128::from(after.stale_tree_bytes)
                - i128::from(before.stale_tree_bytes),
            stale_recovery_bytes: i128::from(after.stale_recovery_bytes)
                - i128::from(before.stale_recovery_bytes),
            generations: after.generation.saturating_sub(before.generation),
            mutation_generations: 0,
            free_map_metadata_pages_after: after.free_map_metadata_pages,
            window_latency: Duration::ZERO,
            reusable_free_pages_after: after.reusable_free_pages,
            reusable_free_bytes_after: after.reusable_free_bytes,
            reclaimable_bytes_after: after.reclaimable_bytes,
            stale_acceptance_bytes_after: after.stale_acceptance_bytes,
            pinned_reader_blockers_after: after.pinned_reader_blockers,
        }
    }

    fn with_mutation_observation(mut self, generations: u64, latency: Duration) -> Self {
        self.mutation_generations = generations;
        self.window_latency = latency;
        self
    }

    fn per_operation(self, operations: u64) -> [i128; 6] {
        let operations = i128::from(operations);
        [
            self.retained_payload_bytes / operations,
            self.compacted_bytes / operations,
            self.active_physical_bytes / operations,
            self.live_tree_bytes / operations,
            self.stale_tree_bytes / operations,
            self.stale_recovery_bytes / operations,
        ]
    }
}

#[derive(Clone, Copy, Debug)]
enum WorkloadKind {
    TicketCreate,
    TicketOverwrite,
    PageCreate,
    PageOverwrite,
    LaneOverwrite,
    DocumentOverwrite,
}

impl WorkloadKind {
    fn label(self) -> &'static str {
        match self {
            Self::TicketCreate => "ticket.create",
            Self::TicketOverwrite => "ticket.overwrite",
            Self::PageCreate => "page.create",
            Self::PageOverwrite => "page.overwrite",
            Self::LaneOverwrite => "lane.overwrite",
            Self::DocumentOverwrite => "document.overwrite",
        }
    }

    fn generations_per_mutation(self) -> u64 {
        1
    }

    fn publications_per_mutation(self) -> u64 {
        1
    }

    fn online_size_ceiling(self, compacted_bytes: u64) -> u64 {
        match self {
            Self::TicketCreate | Self::PageCreate => compacted_bytes
                .saturating_mul(4)
                .max(compacted_bytes.saturating_add(1024 * 1024)),
            Self::TicketOverwrite
            | Self::PageOverwrite
            | Self::LaneOverwrite
            | Self::DocumentOverwrite => compacted_bytes
                .saturating_mul(3)
                .max(compacted_bytes.saturating_add(512 * 1024)),
        }
    }

    fn reclaimable_share_divisor(self) -> u64 {
        match self {
            Self::TicketCreate | Self::PageCreate => 4,
            Self::TicketOverwrite
            | Self::PageOverwrite
            | Self::LaneOverwrite
            | Self::DocumentOverwrite => 3,
        }
    }

    fn object_index_key_limit(self) -> u64 {
        match self {
            Self::TicketCreate | Self::TicketOverwrite => 16,
            Self::PageCreate | Self::PageOverwrite => 0,
            Self::LaneOverwrite => 0,
            Self::DocumentOverwrite => 16,
        }
    }

    fn family_key_limits(self) -> &'static [(&'static str, u64)] {
        match self {
            Self::TicketCreate | Self::TicketOverwrite => &[
                ("current_records", 8),
                ("retained_history", 4),
                ("owner_tokens", 8),
                ("secondary_indexes", 8),
                ("workflow_idempotency", 1),
                ("audit_retention", 2),
            ],
            Self::PageCreate | Self::PageOverwrite => &[
                ("current_records", 6),
                ("retained_history", 4),
                ("owner_tokens", 6),
                ("secondary_indexes", 4),
                ("workflow_idempotency", 1),
                ("audit_retention", 2),
            ],
            Self::LaneOverwrite => &[
                ("current_records", 1),
                ("owner_tokens", 1),
                ("secondary_indexes", 1),
                ("audit_retention", 1),
            ],
            Self::DocumentOverwrite => &[
                ("current_records", 6),
                ("retained_history", 4),
                ("owner_tokens", 6),
                ("secondary_indexes", 4),
                ("workflow_idempotency", 1),
                ("audit_retention", 2),
            ],
        }
    }

    fn has_complete_structural_ceiling(self) -> bool {
        matches!(
            self,
            Self::TicketCreate
                | Self::TicketOverwrite
                | Self::PageCreate
                | Self::PageOverwrite
                | Self::LaneOverwrite
        )
    }
}

#[derive(Clone, Debug)]
struct StructuralBudget {
    kind: WorkloadKind,
    depths: BTreeMap<String, u64>,
    semantic_cow_pages_per_operation: u64,
    free_map_pages_per_operation: u64,
    root_catalog_pages_per_operation: u64,
    region_table_pages_per_operation: u64,
    maintenance_pages_per_operation: u64,
    recovery_bytes: i128,
}

fn path_and_split_pages(depth: u64, keys: u64, batches: u64) -> u64 {
    let depth = depth.max(1);
    keys.saturating_mul(depth)
        .saturating_add(batches.saturating_mul(depth))
}

fn round_up_nonnegative_to_page(bytes: i128) -> i128 {
    let bytes = bytes.max(0);
    ((bytes + STORE_PAGE_BYTES - 1) / STORE_PAGE_BYTES) * STORE_PAGE_BYTES
}

fn structural_budget(path: &Path, kind: WorkloadKind) -> StructuralBudget {
    let store = FileStore::open(path).expect("open store for structural root depths");
    let depths = store
        .btree_root_depths_for_test()
        .expect("inspect B-tree root depths")
        .into_iter()
        .map(|StoreBtreeRootDepth { root, depth }| (root, depth))
        .collect::<BTreeMap<_, _>>();
    let depth = |root: &str| depths.get(root).copied().unwrap_or(1);

    let publications = kind.publications_per_mutation();
    let mut semantic_cow_pages = path_and_split_pages(
        depth("object_index"),
        kind.object_index_key_limit(),
        publications,
    );
    for (root, key_limit) in kind.family_key_limits() {
        semantic_cow_pages =
            semantic_cow_pages.saturating_add(path_and_split_pages(depth(root), *key_limit, 1));
    }

    // Every superseded semantic tree page can add at most one free extent. An extent replacement
    // owns one value page plus a delete and an upsert root-to-leaf path. Split propagation is one
    // additional path for the complete logical operation.
    let free_map_depth = depth("free_map");
    let free_map_pages = semantic_cow_pages
        .saturating_mul(free_map_depth.saturating_mul(2).saturating_add(1))
        .saturating_add(free_map_depth);
    let root_catalog_pages = 1;
    let region_table_pages = publications;
    let maintenance_pages = publications;
    let recovery_pages_per_operation = semantic_cow_pages
        .saturating_add(free_map_pages)
        .saturating_add(root_catalog_pages)
        .saturating_add(region_table_pages)
        .saturating_add(maintenance_pages);
    let recovery_bytes = i128::from(recovery_pages_per_operation)
        * i128::from(REUSE_SAFE_GENERATION_WINDOW)
        * STORE_PAGE_BYTES;

    StructuralBudget {
        kind,
        depths,
        semantic_cow_pages_per_operation: semantic_cow_pages,
        free_map_pages_per_operation: free_map_pages,
        root_catalog_pages_per_operation: root_catalog_pages,
        region_table_pages_per_operation: region_table_pages,
        maintenance_pages_per_operation: maintenance_pages,
        recovery_bytes,
    }
}

fn compacted_bytes(path: &Path, label: &str) -> u64 {
    let copy = path.with_extension(format!("{label}.compacted.loom"));
    std::fs::copy(path, &copy).expect("copy store for compaction sample");
    let mut store = FileStore::open(&copy).expect("open compaction sample");
    store.compact().expect("compact sample copy");
    let bytes = store
        .maintenance_status()
        .expect("compacted maintenance status")
        .physical_bytes;
    drop(store);
    std::fs::remove_file(copy).expect("remove compaction sample");
    bytes
}

fn sample(path: &Path, label: &str) -> StorageSample {
    let store = FileStore::open(path).expect("open store for attribution");
    let maintenance = store.maintenance_status().expect("maintenance status");
    let classes = store
        .page_class_attribution(0)
        .expect("page attribution")
        .classes;
    let class_bytes = |predicate: &dyn Fn(&str) -> bool| -> u64 {
        classes
            .iter()
            .filter(|entry| predicate(&entry.class))
            .map(|entry| entry.bytes)
            .sum()
    };
    let stale_tree_bytes = class_bytes(&|class| class == "stale_tree_page");
    let stale_recovery_bytes =
        class_bytes(&|class| class.starts_with("stale_") || class.starts_with("unreferenced_"));
    let reusable_free_bytes = maintenance
        .reusable_free_pages
        .saturating_sub(maintenance.tail_free_pages)
        .saturating_mul(STORE_PAGE_BYTES as u64);
    let reclaimable_bytes = maintenance
        .candidate_dead_pages
        .saturating_sub(maintenance.reusable_free_pages)
        .saturating_mul(STORE_PAGE_BYTES as u64);
    let stale_acceptance_bytes = class_bytes(&|class| {
        matches!(
            class,
            "stale_record_slab_page"
                | "stale_record_large_page"
                | "stale_record_chunked_page"
                | "stale_tree_page"
                | "stale_region_table_page"
                | "stale_maintenance_page"
                | "stale_free_map_page"
                | "unreferenced_unclassified_page"
                | "unreferenced_zero_page"
        )
    })
    .saturating_add(reusable_free_bytes);
    let class_pages = |class: &str| {
        classes
            .iter()
            .filter(|entry| entry.class == class)
            .map(|entry| entry.pages)
            .sum()
    };
    let active_tree_pages = classes
        .iter()
        .filter(|entry| {
            entry.class.ends_with("_tree_page")
                && !entry.class.starts_with("stale_")
                && !entry.class.starts_with("unreferenced_")
        })
        .map(|entry| entry.pages)
        .sum();
    let roots = store
        .root_storage_attribution(0)
        .expect("root storage attribution")
        .roots;
    let epoch = store
        .active_reachability_mark_epoch()
        .expect("active reachability epoch");
    eprintln!(
        "sample={label} epoch={:?} page_classes={:?}",
        epoch.as_ref().map(|epoch| (
            epoch.base_generation,
            epoch.page_high_water_mark,
            epoch.captured_free_consumed_through,
            epoch.captured_free_root,
        )),
        classes
            .iter()
            .map(|entry| (entry.class.as_str(), entry.pages, entry.bytes))
            .collect::<Vec<_>>()
    );
    StorageSample {
        generation: maintenance.generation,
        physical_bytes: maintenance.physical_bytes,
        physical_pages: maintenance.physical_page_count,
        compacted_bytes: compacted_bytes(path, label),
        active_tree_pages,
        stale_tree_pages: class_pages("stale_tree_page"),
        stale_tree_bytes,
        stale_recovery_bytes,
        retained_payload_bytes: roots.iter().map(|root| root.payload_bytes).sum(),
        live_tree_pages: roots.iter().map(|root| root.tree_pages).sum(),
        live_tree_bytes: roots.iter().map(|root| root.tree_bytes).sum(),
        free_map_metadata_pages: class_pages("free_map_page"),
        reusable_free_pages: maintenance.reusable_free_pages,
        reusable_free_bytes,
        reclaimable_bytes,
        stale_acceptance_bytes,
        pinned_reader_blockers: maintenance.group_commit.pinned_reader_blockers,
    }
}

fn store_generation(path: &Path) -> u64 {
    FileStore::open(path)
        .expect("open store for generation observation")
        .maintenance_status()
        .expect("generation observation maintenance status")
        .generation
}

fn reset_batch_observations() {
    take_object_index_batch_page_stats();
    take_btree_batch_transaction_page_stats();
    take_foreground_allocator_page_stats();
}

fn take_batch_observations() -> (
    Vec<ObjectIndexBatchPageStats>,
    Vec<BtreeBatchTransactionPageStats>,
    Vec<ForegroundAllocatorPageStats>,
) {
    (
        take_object_index_batch_page_stats(),
        take_btree_batch_transaction_page_stats(),
        take_foreground_allocator_page_stats(),
    )
}

fn report_window(
    kind: WorkloadKind,
    window: usize,
    delta: MetricDelta,
    budget: &StructuralBudget,
    object_stats: &[ObjectIndexBatchPageStats],
    btree_stats: &[BtreeBatchTransactionPageStats],
    allocator_stats: &[ForegroundAllocatorPageStats],
    recovery: RecoveryCycle,
) {
    let per_operation = delta.per_operation(WINDOW_OPERATIONS);
    let observed_semantic_pages: u64 = btree_stats
        .iter()
        .map(|stats| stats.existing_pages_replaced + stats.new_split_pages_written)
        .sum();
    let predicted_semantic_pages = budget
        .semantic_cow_pages_per_operation
        .saturating_mul(WINDOW_OPERATIONS);
    if kind.has_complete_structural_ceiling() {
        assert!(
            observed_semantic_pages <= predicted_semantic_pages,
            "{} window {window} observed {observed_semantic_pages} semantic COW pages above precomputed {predicted_semantic_pages}",
            kind.label()
        );
    }
    let observed_object_pages: u64 = object_stats
        .iter()
        .map(|stats| stats.existing_pages_replaced + stats.new_split_pages_written)
        .sum();
    let publication_reused_pages: u64 = allocator_stats
        .iter()
        .map(|stats| stats.publication_reused_pages)
        .sum();
    let publication_reserved_pages: u64 = allocator_stats
        .iter()
        .map(|stats| stats.publication_reserved_pages)
        .sum();
    let publication_unused_pages: u64 = allocator_stats
        .iter()
        .map(|stats| stats.publication_unused_pages)
        .sum();
    let ordinary_reused_pages: u64 = allocator_stats
        .iter()
        .map(|stats| stats.ordinary_reused_pages)
        .sum();
    let transaction_reused_pages: u64 = allocator_stats
        .iter()
        .map(|stats| stats.transaction_reused_pages)
        .sum();
    let extended_pages: u64 = allocator_stats
        .iter()
        .map(|stats| stats.extended_pages)
        .sum();
    let free_map_updates: u64 = allocator_stats
        .iter()
        .map(|stats| stats.free_map_updates)
        .sum();
    let publication_reserve_exhaustions: u64 = allocator_stats
        .iter()
        .map(|stats| stats.publication_reserve_exhaustions)
        .sum();
    let reusable_eligible_pages_left: u64 = allocator_stats
        .iter()
        .map(|stats| stats.reusable_eligible_pages_left)
        .sum();
    let metadata_bootstrap_reused_pages: u64 = allocator_stats
        .iter()
        .map(|stats| stats.metadata_bootstrap_reused_pages)
        .sum();
    let metadata_bootstrap_extended_pages: u64 = allocator_stats
        .iter()
        .map(|stats| stats.metadata_bootstrap_extended_pages)
        .sum();
    let metadata_bootstrap_unused_pages: u64 = allocator_stats
        .iter()
        .map(|stats| stats.metadata_bootstrap_unused_pages)
        .sum();
    eprintln!(
        "{} window={window} operations={WINDOW_OPERATIONS} generations={} mutation_generations={} latency_us={} latency_us_per_op={} retained={} compacted={} active={} retained_physical_bytes={} compacted_bytes_after={} reusable_free_bytes_after={} reclaimable_bytes_after={} stale_acceptance_bytes_after={} physical_pages={} active_tree_pages={} live_tree_pages={} stale_tree_pages={} free_map_metadata_pages={} live_tree={} stale_tree={} stale_recovery={} retained_per_op={} compacted_per_op={} active_per_op={} live_tree_per_op={} stale_tree_per_op={} stale_recovery_per_op={} reusable_free_pages={} pinned_reader_blockers={:?} mark_slices={} mark_visited={} gc_pages_freed={} observed_object_pages={observed_object_pages} observed_semantic_pages={observed_semantic_pages} predicted_semantic_pages={predicted_semantic_pages} publication_reserved_pages={publication_reserved_pages} publication_reused_pages={publication_reused_pages} publication_unused_pages={publication_unused_pages} publication_reserve_exhaustions={publication_reserve_exhaustions} ordinary_reused_pages={ordinary_reused_pages} transaction_reused_pages={transaction_reused_pages} reusable_eligible_pages_left={reusable_eligible_pages_left} extended_pages={extended_pages} metadata_bootstrap_reused_pages={metadata_bootstrap_reused_pages} metadata_bootstrap_extended_pages={metadata_bootstrap_extended_pages} metadata_bootstrap_unused_pages={metadata_bootstrap_unused_pages} free_map_updates={free_map_updates} semantic_cow_pages_per_op={} free_map_pages_per_op={} root_catalog_pages_per_op={} region_table_pages_per_op={} maintenance_pages_per_op={} structural_recovery_bytes={} structural_ceiling_resolved={} depths={:?}",
        kind.label(),
        delta.generations,
        delta.mutation_generations,
        delta.window_latency.as_micros(),
        delta.window_latency.as_micros() / u128::from(WINDOW_OPERATIONS),
        delta.retained_payload_bytes,
        delta.compacted_bytes,
        delta.active_physical_bytes,
        delta.retained_physical_bytes_after,
        delta.compacted_bytes_after,
        delta.reusable_free_bytes_after,
        delta.reclaimable_bytes_after,
        delta.stale_acceptance_bytes_after,
        delta.physical_pages,
        delta.active_tree_pages,
        delta.live_tree_pages,
        delta.stale_tree_pages,
        delta.free_map_metadata_pages_after,
        delta.live_tree_bytes,
        delta.stale_tree_bytes,
        delta.stale_recovery_bytes,
        per_operation[0],
        per_operation[1],
        per_operation[2],
        per_operation[3],
        per_operation[4],
        per_operation[5],
        delta.reusable_free_pages_after,
        delta.pinned_reader_blockers_after,
        recovery.mark_slices,
        recovery.mark_visited,
        recovery.pages_freed,
        budget.semantic_cow_pages_per_operation,
        budget.free_map_pages_per_operation,
        budget.root_catalog_pages_per_operation,
        budget.region_table_pages_per_operation,
        budget.maintenance_pages_per_operation,
        budget.recovery_bytes,
        kind.has_complete_structural_ceiling(),
        budget.depths,
    );
}

fn assert_window_contract(
    kind: WorkloadKind,
    deltas: &[MetricDelta],
    budgets: &[StructuralBudget],
) {
    assert_eq!(deltas.len(), MEASURED_WINDOWS);
    assert_eq!(budgets.len(), MEASURED_WINDOWS);
    for (delta, budget) in deltas.iter().zip(budgets) {
        assert_eq!(budget.kind.label(), kind.label());
        assert_eq!(
            delta.mutation_generations,
            WINDOW_OPERATIONS.saturating_mul(kind.generations_per_mutation()),
            "{} must publish exactly one generation per logical mutation",
            kind.label()
        );
        if let Some(latency_ceiling) = WINDOW_LATENCY_CEILING_PER_OPERATION {
            assert!(
                delta.window_latency / WINDOW_OPERATIONS as u32 <= latency_ceiling,
                "{} per-operation latency {:?} exceeds the source-derived ceiling {latency_ceiling:?}",
                kind.label(),
                delta.window_latency / WINDOW_OPERATIONS as u32
            );
        }
        if !kind.has_complete_structural_ceiling() {
            continue;
        }
        let online_ceiling = kind.online_size_ceiling(delta.compacted_bytes_after);
        assert!(
            delta.retained_physical_bytes_after <= online_ceiling,
            "{} online size {} exceeds the endpoint compacted-size ceiling {} (compacted={})",
            kind.label(),
            delta.retained_physical_bytes_after,
            online_ceiling,
            delta.compacted_bytes_after
        );
        let reclaimable_ceiling =
            delta.retained_physical_bytes_after / kind.reclaimable_share_divisor();
        assert!(
            delta
                .reusable_free_bytes_after
                .saturating_add(delta.reclaimable_bytes_after)
                <= reclaimable_ceiling,
            "{} reusable plus reclaimable bytes {} exceed the endpoint share ceiling {}",
            kind.label(),
            delta
                .reusable_free_bytes_after
                .saturating_add(delta.reclaimable_bytes_after),
            reclaimable_ceiling
        );
        let stale_ceiling = delta.retained_physical_bytes_after / 4;
        assert!(
            delta.stale_acceptance_bytes_after <= stale_ceiling,
            "{} stale acceptance bytes {} exceed the endpoint share ceiling {}",
            kind.label(),
            delta.stale_acceptance_bytes_after,
            stale_ceiling
        );
        let compacted_ceiling = round_up_nonnegative_to_page(delta.retained_payload_bytes)
            + i128::from(WINDOW_OPERATIONS) * STORE_PAGE_BYTES
            + delta.live_tree_bytes.max(0)
            + STORE_PAGE_BYTES;
        assert!(
            delta.compacted_bytes <= compacted_ceiling,
            "{} compacted growth {} exceeds retained payload, page rounding, live-tree growth, and its one root-catalog page ceiling {compacted_ceiling}",
            kind.label(),
            delta.compacted_bytes
        );
        assert!(
            delta.active_physical_bytes
                <= delta
                    .compacted_bytes
                    .max(0)
                    .saturating_add(budget.recovery_bytes),
            "{} active growth {} exceeds compacted growth {} plus the precomputed structural reserve {}",
            kind.label(),
            delta.active_physical_bytes,
            delta.compacted_bytes,
            budget.recovery_bytes
        );
    }
}

fn fresh_store(path: &Path) {
    loom_coordination::with_local_store_write_lock(path, || {
        let store = FileStore::create_with_profile(path, Algo::Blake3)?;
        let mut loom = Loom::new(store);
        let workspace = loom.registry_mut().create(
            FacetKind::Files,
            Some("repo"),
            WorkspaceId::v4_from_bytes([3; 16]),
        )?;
        loom.registry_mut().add_facet(workspace, FacetKind::Vcs)?;
        loom.registry_mut().add_facet(workspace, FacetKind::Cas)?;
        loom.registry_mut()
            .add_facet(workspace, FacetKind::Document)?;
        loom.registry_mut().add_facet(workspace, FacetKind::Queue)?;
        save_loom(&mut loom)
    })
    .expect("initialize store");
}

fn measure_ticket_creates() {
    let temp = TempStore::new("ticket-create");
    fresh_store(temp.path());
    let mcp = LoomMcp::new(StoreAccess::per_request(temp.path(), None));
    let project = mcp
        .write_tickets_project_create("repo", "studio", "eng", "ENG", "Engineering", None)
        .expect("create ticket project");
    let first_before = sample(temp.path(), "ticket-create-first-before");
    let first = mcp
        .write_tickets_create(
            "repo",
            TicketCreateRequest {
                workspace_id: "studio",
                project_id: "eng",
                ticket_type: "task",
                external_source: None,
                external_id: None,
                fields: &json!({"title": "First ticket"}),
                policy_labels: &[],
                expected_root: Some(&project.profile_root),
            },
        )
        .expect("create first ticket");
    let first_delta = MetricDelta::between(
        first_before,
        sample(temp.path(), "ticket-create-first-after"),
    );
    eprintln!("ticket.create first_use={first_delta:?}");

    let mut root = first.profile_root;
    for operation in 0..WARM_OPERATIONS {
        let title = format!("Ticket create warmup {operation}");
        root = mcp
            .write_tickets_create(
                "repo",
                TicketCreateRequest {
                    workspace_id: "studio",
                    project_id: "eng",
                    ticket_type: "task",
                    external_source: None,
                    external_id: None,
                    fields: &json!({"title": title}),
                    policy_labels: &[],
                    expected_root: Some(&root),
                },
            )
            .expect("warm ticket creates")
            .profile_root;
    }
    let warm_recovery = complete_recovery_cycle(temp.path(), true);
    eprintln!("ticket.create warm_recovery={warm_recovery:?}");
    let stale_root = root.clone();
    let mut before = sample(temp.path(), "ticket-create-window-start");
    let mut deltas = Vec::new();
    let mut budgets = Vec::new();
    let mut last_ticket_id = String::new();
    let mut expected_title = String::new();
    for window in 1..=MEASURED_WINDOWS {
        let budget = structural_budget(temp.path(), WorkloadKind::TicketCreate);
        reset_batch_observations();
        let mutation_generation_before = store_generation(temp.path());
        let mutation_started = Instant::now();
        for operation in 0..WINDOW_OPERATIONS {
            expected_title = format!("Ticket create window {window} operation {operation}");
            let ticket = mcp
                .write_tickets_create(
                    "repo",
                    TicketCreateRequest {
                        workspace_id: "studio",
                        project_id: "eng",
                        ticket_type: "task",
                        external_source: None,
                        external_id: None,
                        fields: &json!({"title": expected_title}),
                        policy_labels: &[],
                        expected_root: Some(&root),
                    },
                )
                .expect("measure ticket creates");
            root = ticket.profile_root;
            last_ticket_id = ticket.ticket_id;
        }
        let mutation_latency = mutation_started.elapsed();
        let mutation_generations =
            store_generation(temp.path()).saturating_sub(mutation_generation_before);
        let (object_stats, btree_stats, allocator_stats) = take_batch_observations();
        let recovery = complete_recovery_cycle(temp.path(), window < MEASURED_WINDOWS);
        let after = sample(temp.path(), &format!("ticket-create-window-{window}"));
        let delta = MetricDelta::between(before, after)
            .with_mutation_observation(mutation_generations, mutation_latency);
        report_window(
            WorkloadKind::TicketCreate,
            window,
            delta,
            &budget,
            &object_stats,
            &btree_stats,
            &allocator_stats,
            recovery,
        );
        deltas.push(delta);
        budgets.push(budget);
        before = after;
    }
    assert_window_contract(WorkloadKind::TicketCreate, &deltas, &budgets);

    let failure_generation = before.generation;
    let error = mcp
        .write_tickets_create(
            "repo",
            TicketCreateRequest {
                workspace_id: "studio",
                project_id: "eng",
                ticket_type: "task",
                external_source: None,
                external_id: None,
                fields: &json!({"title": "Stale ticket"}),
                policy_labels: &[],
                expected_root: Some(&stale_root),
            },
        )
        .unwrap_err();
    assert_eq!(error.code, Code::Conflict);
    assert_eq!(
        FileStore::open(temp.path())
            .unwrap()
            .maintenance_status()
            .unwrap()
            .generation,
        failure_generation
    );
    drop(mcp);
    let reopened = LoomMcp::new(StoreAccess::per_request(temp.path(), None));
    let ticket = reopened
        .read_tickets_get("repo", "studio", &last_ticket_id, None)
        .unwrap()
        .expect("reopened created ticket");
    assert_eq!(ticket.fields["title"], expected_title);
    let history = reopened
        .read_tickets_history("repo", "studio", Some(&last_ticket_id))
        .unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].operation_kind, "ticket.created");
}

fn measure_ticket_overwrites() {
    let temp = TempStore::new("ticket-overwrite");
    fresh_store(temp.path());
    let mcp = LoomMcp::new(StoreAccess::per_request(temp.path(), None));
    let project = mcp
        .write_tickets_project_create("repo", "studio", "eng", "ENG", "Engineering", None)
        .unwrap();
    let ticket = mcp
        .write_tickets_create(
            "repo",
            TicketCreateRequest {
                workspace_id: "studio",
                project_id: "eng",
                ticket_type: "task",
                external_source: None,
                external_id: None,
                fields: &json!({"title": "Fixed ticket"}),
                policy_labels: &[],
                expected_root: Some(&project.profile_root),
            },
        )
        .unwrap();
    let ticket_id = ticket.ticket_id;
    let mut root = ticket.profile_root;
    for operation in 0..WARM_OPERATIONS {
        let fields = json!({"title": format!("Ticket overwrite warmup {operation}")});
        root = mcp
            .write_tickets_update(
                "repo",
                TicketUpdateRequest {
                    workspace_id: "studio",
                    ticket_id: &ticket_id,
                    set_fields: Some(&fields),
                    delete_fields: &[],
                    action: None,
                    target_status: None,
                    observed_source_status: None,
                    observed_workflow_version: None,
                    assignee: None,
                    expected_root: Some(&root),
                    comment: None,
                    comments: &[],
                    relation_sets: &[],
                    relation_removes: &[],
                },
            )
            .unwrap()
            .profile_root;
    }
    let warm_recovery = complete_recovery_cycle(temp.path(), true);
    eprintln!("ticket.overwrite warm_recovery={warm_recovery:?}");
    let stale_root = root.clone();
    let mut before = sample(temp.path(), "ticket-overwrite-window-start");
    let mut deltas = Vec::new();
    let mut budgets = Vec::new();
    for window in 1..=MEASURED_WINDOWS {
        let budget = structural_budget(temp.path(), WorkloadKind::TicketOverwrite);
        reset_batch_observations();
        let mutation_generation_before = store_generation(temp.path());
        let mutation_started = Instant::now();
        for operation in 0..WINDOW_OPERATIONS {
            let fields =
                json!({"title": format!("Ticket overwrite window {window} operation {operation}")});
            root = mcp
                .write_tickets_update(
                    "repo",
                    TicketUpdateRequest {
                        workspace_id: "studio",
                        ticket_id: &ticket_id,
                        set_fields: Some(&fields),
                        delete_fields: &[],
                        action: None,
                        target_status: None,
                        observed_source_status: None,
                        observed_workflow_version: None,
                        assignee: None,
                        expected_root: Some(&root),
                        comment: None,
                        comments: &[],
                        relation_sets: &[],
                        relation_removes: &[],
                    },
                )
                .expect("measure ticket overwrites")
                .profile_root;
        }
        let mutation_latency = mutation_started.elapsed();
        let mutation_generations =
            store_generation(temp.path()).saturating_sub(mutation_generation_before);
        let (object_stats, btree_stats, allocator_stats) = take_batch_observations();
        let recovery = complete_recovery_cycle(temp.path(), window < MEASURED_WINDOWS);
        let after = sample(temp.path(), &format!("ticket-overwrite-window-{window}"));
        let delta = MetricDelta::between(before, after)
            .with_mutation_observation(mutation_generations, mutation_latency);
        report_window(
            WorkloadKind::TicketOverwrite,
            window,
            delta,
            &budget,
            &object_stats,
            &btree_stats,
            &allocator_stats,
            recovery,
        );
        deltas.push(delta);
        budgets.push(budget);
        before = after;
    }
    assert_window_contract(WorkloadKind::TicketOverwrite, &deltas, &budgets);

    let failed_generation = before.generation;
    let fields = json!({"title": "Stale overwrite"});
    let error = mcp
        .write_tickets_update(
            "repo",
            TicketUpdateRequest {
                workspace_id: "studio",
                ticket_id: &ticket_id,
                set_fields: Some(&fields),
                delete_fields: &[],
                action: None,
                target_status: None,
                observed_source_status: None,
                observed_workflow_version: None,
                assignee: None,
                expected_root: Some(&stale_root),
                comment: None,
                comments: &[],
                relation_sets: &[],
                relation_removes: &[],
            },
        )
        .unwrap_err();
    assert_eq!(error.code, Code::Conflict);
    assert_eq!(
        FileStore::open(temp.path())
            .unwrap()
            .maintenance_status()
            .unwrap()
            .generation,
        failed_generation
    );
    drop(mcp);
    let reopened = LoomMcp::new(StoreAccess::per_request(temp.path(), None));
    let ticket = reopened
        .read_tickets_get("repo", "studio", &ticket_id, None)
        .unwrap()
        .unwrap();
    assert_eq!(
        ticket.fields["title"],
        "Ticket overwrite window 3 operation 31"
    );
    assert_eq!(
        reopened
            .read_tickets_history("repo", "studio", Some(&ticket_id))
            .unwrap()
            .len(),
        1 + WARM_OPERATIONS as usize + MEASURED_WINDOWS * WINDOW_OPERATIONS as usize
    );
}

fn measure_page_workload(kind: WorkloadKind) {
    let temp = TempStore::new(kind.label());
    fresh_store(temp.path());
    let mcp = LoomMcp::new(StoreAccess::per_request(temp.path(), None));
    let space = mcp
        .write_spaces_create("repo", "studio", "eng", "Engineering", None)
        .unwrap();
    let first_before = sample(temp.path(), &format!("{}-first-before", kind.label()));
    let first = mcp
        .write_pages_create(
            "repo",
            PageCreateRequest {
                workspace_id: "studio",
                page_id: "fixed-page",
                space_id: "eng",
                parent_page_id: None,
                title: "Fixed page",
                expected_root: Some(&space.profile_root),
            },
        )
        .unwrap();
    let first_delta = MetricDelta::between(
        first_before,
        sample(temp.path(), &format!("{}-first-after", kind.label())),
    );
    eprintln!("{} first_use={first_delta:?}", kind.label());
    let mut root = first.profile_root;
    for operation in 0..WARM_OPERATIONS {
        root = match kind {
            WorkloadKind::PageCreate => {
                let page_id = format!("page-warm-{operation}");
                mcp.write_pages_create(
                    "repo",
                    PageCreateRequest {
                        workspace_id: "studio",
                        page_id: &page_id,
                        space_id: "eng",
                        parent_page_id: None,
                        title: "Page warmup",
                        expected_root: Some(&root),
                    },
                )
                .unwrap()
                .profile_root
            }
            WorkloadKind::PageOverwrite => {
                mcp.write_pages_update_text(
                    "repo",
                    "studio",
                    "fixed-page",
                    &format!("Page overwrite warmup {operation}"),
                    Some(&root),
                )
                .unwrap()
                .profile_root
            }
            _ => unreachable!(),
        };
    }
    let warm_recovery = complete_recovery_cycle(temp.path(), true);
    eprintln!("{} warm_recovery={warm_recovery:?}", kind.label());
    let stale_root = root.clone();
    let mut before = sample(temp.path(), &format!("{}-window-start", kind.label()));
    let mut deltas = Vec::new();
    let mut budgets = Vec::new();
    let mut last_page_id = "fixed-page".to_string();
    let mut expected_draft_text = None;
    for window in 1..=MEASURED_WINDOWS {
        let budget = structural_budget(temp.path(), kind);
        reset_batch_observations();
        let mutation_generation_before = store_generation(temp.path());
        let mutation_started = Instant::now();
        for operation in 0..WINDOW_OPERATIONS {
            root = match kind {
                WorkloadKind::PageCreate => {
                    let page_id = format!("page-window-{window}-{operation}");
                    let page = mcp
                        .write_pages_create(
                            "repo",
                            PageCreateRequest {
                                workspace_id: "studio",
                                page_id: &page_id,
                                space_id: "eng",
                                parent_page_id: None,
                                title: "Measured page",
                                expected_root: Some(&root),
                            },
                        )
                        .expect("measure page creates");
                    last_page_id = page.page_id;
                    page.profile_root
                }
                WorkloadKind::PageOverwrite => {
                    let text = format!("Page overwrite window {window} operation {operation}");
                    mcp.write_pages_update_text("repo", "studio", "fixed-page", &text, Some(&root))
                        .map(|summary| {
                            expected_draft_text = Some(text);
                            summary.profile_root
                        })
                        .expect("measure page overwrites")
                }
                _ => unreachable!(),
            };
        }
        let mutation_latency = mutation_started.elapsed();
        let mutation_generations =
            store_generation(temp.path()).saturating_sub(mutation_generation_before);
        let (object_stats, btree_stats, allocator_stats) = take_batch_observations();
        let recovery = complete_recovery_cycle(temp.path(), window < MEASURED_WINDOWS);
        let after = sample(temp.path(), &format!("{}-window-{window}", kind.label()));
        let delta = MetricDelta::between(before, after)
            .with_mutation_observation(mutation_generations, mutation_latency);
        report_window(
            kind,
            window,
            delta,
            &budget,
            &object_stats,
            &btree_stats,
            &allocator_stats,
            recovery,
        );
        deltas.push(delta);
        budgets.push(budget);
        before = after;
    }
    assert_window_contract(kind, &deltas, &budgets);

    let failed_generation = before.generation;
    let error = match kind {
        WorkloadKind::PageCreate => mcp
            .write_pages_create(
                "repo",
                PageCreateRequest {
                    workspace_id: "studio",
                    page_id: "stale-page",
                    space_id: "eng",
                    parent_page_id: None,
                    title: "Stale page",
                    expected_root: Some(&stale_root),
                },
            )
            .unwrap_err(),
        WorkloadKind::PageOverwrite => mcp
            .write_pages_update_text(
                "repo",
                "studio",
                "fixed-page",
                "Stale overwrite",
                Some(&stale_root),
            )
            .unwrap_err(),
        _ => unreachable!(),
    };
    assert_eq!(error.code, Code::Conflict);
    assert_eq!(
        FileStore::open(temp.path())
            .unwrap()
            .maintenance_status()
            .unwrap()
            .generation,
        failed_generation
    );
    drop(mcp);
    let reopened = LoomMcp::new(StoreAccess::per_request(temp.path(), None));
    let page = reopened
        .read_pages_get("repo", "studio", &last_page_id)
        .unwrap()
        .expect("reopened page");
    assert_eq!(page.page_id, last_page_id);
    match kind {
        WorkloadKind::PageCreate => assert_eq!(page.title, "Measured page"),
        WorkloadKind::PageOverwrite => assert_eq!(
            page.draft_body_text.as_deref(),
            Some(format!("{}\n", expected_draft_text.as_deref().unwrap()).as_str())
        ),
        _ => unreachable!(),
    }
    let page_id = match kind {
        WorkloadKind::PageCreate => &last_page_id,
        WorkloadKind::PageOverwrite => "fixed-page",
        _ => unreachable!(),
    };
    let history = reopened
        .read_substrate_history("repo", "studio", &format!("page:draft:{page_id}"))
        .unwrap();
    assert!(history.index_present);
    match kind {
        WorkloadKind::PageCreate => assert_eq!(history.revisions.len(), 1),
        WorkloadKind::PageOverwrite => assert_eq!(
            history.revisions.len(),
            WARM_OPERATIONS as usize + MEASURED_WINDOWS * WINDOW_OPERATIONS as usize
        ),
        _ => unreachable!(),
    }
}

fn measure_lane_overwrites() {
    let kind = WorkloadKind::LaneOverwrite;
    let temp = TempStore::new(kind.label());
    fresh_store(temp.path());
    let mcp = LoomMcp::new(StoreAccess::per_request(temp.path(), None));
    mcp.write_lanes_create(
        "repo",
        LaneCreateRequest {
            lane_id: "amplification-lane",
            lane_key: "amplification-lane",
            title: "Amplification lane",
            description: "Storage amplification diagnostic lane",
            lane_kind: loom_lanes::LaneKind::Assignment.as_str(),
            owner_principal: None,
            lane_status: "ready",
            lane_tickets: &[],
            active_ticket_id: None,
            status_report: "created",
            reviewer_feedback: "",
            updated_by: Some("amplification-diagnostic"),
        },
    )
    .expect("create lane overwrite fixture");
    for operation in 0..WARM_OPERATIONS {
        mcp.write_lanes_update(
            "repo",
            LaneUpdateRequest {
                lane_id: "amplification-lane",
                title: None,
                description: None,
                lane_status: None,
                status_report: Some(&format!("lane overwrite warmup {operation}")),
                reviewer_feedback: None,
                updated_by: Some("amplification-diagnostic"),
            },
        )
        .expect("warm lane overwrites");
    }
    let warm_recovery = complete_recovery_cycle(temp.path(), true);
    eprintln!("{} warm_recovery={warm_recovery:?}", kind.label());
    let mut before = sample(temp.path(), "lane-overwrite-window-start");
    let mut deltas = Vec::new();
    let mut budgets = Vec::new();
    let mut expected_status = String::new();
    for window in 1..=MEASURED_WINDOWS {
        let budget = structural_budget(temp.path(), kind);
        reset_batch_observations();
        let mutation_generation_before = store_generation(temp.path());
        let mutation_started = Instant::now();
        for operation in 0..WINDOW_OPERATIONS {
            expected_status = format!("lane overwrite window {window} operation {operation}");
            mcp.write_lanes_update(
                "repo",
                LaneUpdateRequest {
                    lane_id: "amplification-lane",
                    title: None,
                    description: None,
                    lane_status: None,
                    status_report: Some(&expected_status),
                    reviewer_feedback: None,
                    updated_by: Some("amplification-diagnostic"),
                },
            )
            .expect("measure lane overwrites");
        }
        let mutation_latency = mutation_started.elapsed();
        let mutation_generations =
            store_generation(temp.path()).saturating_sub(mutation_generation_before);
        let (object_stats, btree_stats, allocator_stats) = take_batch_observations();
        let recovery = complete_recovery_cycle(temp.path(), window < MEASURED_WINDOWS);
        let after = sample(temp.path(), &format!("lane-overwrite-window-{window}"));
        let delta = MetricDelta::between(before, after)
            .with_mutation_observation(mutation_generations, mutation_latency);
        report_window(
            kind,
            window,
            delta,
            &budget,
            &object_stats,
            &btree_stats,
            &allocator_stats,
            recovery,
        );
        deltas.push(delta);
        budgets.push(budget);
        before = after;
    }
    assert_window_contract(kind, &deltas, &budgets);

    drop(mcp);
    let reopened = LoomMcp::new(StoreAccess::per_request(temp.path(), None));
    let lane = reopened
        .read_lanes_get("repo", "amplification-lane")
        .expect("read reopened lane")
        .expect("reopened lane exists");
    assert_eq!(lane.status_report, expected_status);
}

fn measure_document_overwrites() {
    let kind = WorkloadKind::DocumentOverwrite;
    let temp = TempStore::new(kind.label());
    fresh_store(temp.path());
    let mcp = LoomMcp::new(StoreAccess::per_request(temp.path(), None));
    let first = mcp
        .write_document_put_text(
            "repo",
            "amplification",
            "fixed-document",
            "document overwrite seed",
            None,
        )
        .expect("create document overwrite fixture");
    let mut entity_tag = first.entity_tag;
    for operation in 0..WARM_OPERATIONS {
        let result = mcp
            .write_document_put_text(
                "repo",
                "amplification",
                "fixed-document",
                &format!("document overwrite warmup {operation}"),
                Some(&entity_tag),
            )
            .expect("warm document overwrites");
        entity_tag = result.entity_tag;
    }
    let warm_recovery = complete_recovery_cycle(temp.path(), true);
    eprintln!("{} warm_recovery={warm_recovery:?}", kind.label());
    let stale_entity_tag = entity_tag.clone();
    let mut before = sample(temp.path(), "document-overwrite-window-start");
    let mut deltas = Vec::new();
    let mut budgets = Vec::new();
    let mut expected_text = String::new();
    for window in 1..=MEASURED_WINDOWS {
        let budget = structural_budget(temp.path(), kind);
        reset_batch_observations();
        let mutation_generation_before = store_generation(temp.path());
        let mutation_started = Instant::now();
        for operation in 0..WINDOW_OPERATIONS {
            expected_text = format!("document overwrite window {window} operation {operation}");
            let result = mcp
                .write_document_put_text(
                    "repo",
                    "amplification",
                    "fixed-document",
                    &expected_text,
                    Some(&entity_tag),
                )
                .expect("measure document overwrites");
            entity_tag = result.entity_tag;
        }
        let mutation_latency = mutation_started.elapsed();
        let mutation_generations =
            store_generation(temp.path()).saturating_sub(mutation_generation_before);
        let (object_stats, btree_stats, allocator_stats) = take_batch_observations();
        let recovery = complete_recovery_cycle(temp.path(), window < MEASURED_WINDOWS);
        let after = sample(temp.path(), &format!("document-overwrite-window-{window}"));
        let delta = MetricDelta::between(before, after)
            .with_mutation_observation(mutation_generations, mutation_latency);
        report_window(
            kind,
            window,
            delta,
            &budget,
            &object_stats,
            &btree_stats,
            &allocator_stats,
            recovery,
        );
        deltas.push(delta);
        budgets.push(budget);
        before = after;
    }
    assert_window_contract(kind, &deltas, &budgets);

    let failed_generation = store_generation(temp.path());
    let error = mcp
        .write_document_put_text(
            "repo",
            "amplification",
            "fixed-document",
            "stale document overwrite",
            Some(&stale_entity_tag),
        )
        .unwrap_err();
    assert_eq!(error.code, Code::Conflict);
    assert_eq!(store_generation(temp.path()), failed_generation);
    drop(mcp);
    let reopened = LoomMcp::new(StoreAccess::per_request(temp.path(), None));
    let document = reopened
        .read_document_get_text("repo", "amplification", "fixed-document")
        .expect("read reopened document")
        .expect("reopened document exists");
    assert_eq!(document.text, expected_text);
    assert_eq!(document.entity_tag, entity_tag);
}

#[test]
#[ignore = "diagnostic: cross-facet physical plateau gate; run via just test-storage-amplification"]
fn ticket_and_page_creates_and_overwrites_reach_structural_plateau() {
    let rejected_publications = Arc::new(Mutex::new(Vec::new()));
    let captured_rejections = Arc::clone(&rejected_publications);
    let _rejected_publication_guard = install_rejected_free_map_publication_test_observer(
        Arc::new(move |diagnostic| {
            eprintln!(
                "rejected_free_map_publication demanded_pages={} reserve_capacity_pages={} reserve_available_pages={} dirty_ranges={} extent_deletes={} extent_upserts={} btree_node_pages={} affected_existing_btree_pages={} split_decisions={} free_map_depth={}",
                diagnostic.demanded_pages,
                diagnostic.reserve_capacity_pages,
                diagnostic.reserve_available_pages,
                diagnostic.dirty_range_count,
                diagnostic.extent_deletes,
                diagnostic.extent_upserts,
                diagnostic.btree_node_pages,
                diagnostic.affected_existing_btree_pages,
                diagnostic.split_decisions,
                diagnostic.free_map_depth,
            );
            captured_rejections.lock().unwrap().push(diagnostic);
        }),
    );
    measure_ticket_creates();
    measure_ticket_overwrites();
    measure_page_workload(WorkloadKind::PageCreate);
    measure_page_workload(WorkloadKind::PageOverwrite);
    measure_lane_overwrites();
    measure_document_overwrites();
    let mut unresolved = Vec::new();
    if WINDOW_LATENCY_CEILING_PER_OPERATION.is_none() {
        unresolved.push("per-window latency has no source-derived ceiling");
    }
    if !WorkloadKind::DocumentOverwrite.has_complete_structural_ceiling() {
        unresolved.push("document overwrite has no complete source-derived physical ceiling");
    }
    assert!(
        unresolved.is_empty(),
        "storage amplification acceptance remains unresolved: {}",
        unresolved.join("; ")
    );
}
