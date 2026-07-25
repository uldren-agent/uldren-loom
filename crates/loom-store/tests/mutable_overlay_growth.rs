use loom_store::{FileStore, OverlayKey};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct TempPath(PathBuf);

impl TempPath {
    fn new(tag: &str) -> Self {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let mut path = std::env::temp_dir();
        path.push(format!("loomstore-{tag}-{pid}-{n}.loom"));
        let _ = std::fs::remove_file(&path);
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempPath {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn hot_key() -> OverlayKey {
    OverlayKey::from_segments([
        b"workspace",
        &[9; 16],
        b"tickets",
        b"matrix",
        b"ticket",
        b"MX-392",
    ])
    .unwrap()
}

fn write_hot_values(store: &FileStore, key: &OverlayKey, start: u64, count: u64) {
    for update in start..start + count {
        store
            .put_mutable_overlay_value(key.clone(), format!("current-{update}").into_bytes())
            .unwrap();
    }
}

fn bundle_key(domain: &str, id: u64) -> OverlayKey {
    OverlayKey::from_segments([
        b"workspace",
        &[9; 16],
        domain.as_bytes(),
        b"random-bundle",
        b"current",
        format!("{id:016x}").as_bytes(),
    ])
    .unwrap()
}

fn page_class_bytes(store: &FileStore, class_name: &str) -> u64 {
    store
        .page_class_attribution(4)
        .unwrap()
        .classes
        .into_iter()
        .find(|class| class.class == class_name)
        .map(|class| class.bytes)
        .unwrap_or(0)
}

#[test]
fn hot_overlay_writes_keep_one_logical_record_and_bound_physical_growth() {
    let path = TempPath::new("mutable-overlay-growth");
    let store = FileStore::open(path.path()).unwrap();
    let key = hot_key();

    write_hot_values(&store, &key, 0, 1);
    let first = store.maintenance_status().unwrap();
    write_hot_values(&store, &key, 1, 256);
    let warm = store.maintenance_status().unwrap();
    write_hot_values(&store, &key, 257, 256);
    let measured = store.maintenance_status().unwrap();
    let report = store.store_maintenance_report(0).unwrap();
    let actual_file_bytes = std::fs::metadata(path.path()).unwrap().len();
    let measured_growth = measured.physical_bytes.saturating_sub(warm.physical_bytes);

    assert_eq!(report.overlay_health.current_record_count, 1);
    assert_eq!(report.overlay_health.hot_write_count, 513);
    assert_eq!(report.overlay_health.current_generation, 513);
    assert_eq!(measured.physical_bytes, actual_file_bytes);
    assert!(
        warm.physical_bytes > first.physical_bytes,
        "put_mutable_overlay_value must exercise the persistent overlay writer"
    );
    assert!(
        measured_growth <= 128 * 1024,
        "commit_mutable_overlay_records grew {} bytes over 256 hot writes after warmup",
        measured_growth
    );
}

#[test]
fn random_new_item_bundles_report_current_records_without_stale_amplification() {
    let path = TempPath::new("random-new-item-growth");
    let store = FileStore::open(path.path()).unwrap();
    let domains = ["tickets", "lanes", "pages", "documents"];
    // Twelve bundles exercise all four random-new-item domains with enough records to populate
    // attribution classes while keeping the default test small.
    let bundle_count = 12u64;

    for bundle in 0..bundle_count {
        let entries = domains
            .iter()
            .map(|domain| {
                (
                    bundle_key(domain, bundle),
                    format!("{{\"domain\":\"{domain}\",\"bundle\":{bundle}}}").into_bytes(),
                )
            })
            .collect::<Vec<_>>();
        store.put_mutable_overlay_values(entries).unwrap();
    }

    let status = store.maintenance_status().unwrap();
    let report = store.store_maintenance_report(0).unwrap();
    let actual_file_bytes = std::fs::metadata(path.path()).unwrap().len();
    let current_records = bundle_count * domains.len() as u64;
    let payload_bytes = report
        .growth_domains
        .iter()
        .map(|domain| domain.payload_bytes)
        .sum::<u64>();

    assert_eq!(report.overlay_health.current_record_count, current_records);
    assert_eq!(report.overlay_health.hot_write_count, current_records);
    assert_eq!(report.overlay_obsolete_record_count, 0);
    assert_eq!(status.physical_bytes, actual_file_bytes);
    assert_eq!(
        report
            .growth_domains
            .iter()
            .filter(|domain| domains.contains(&domain.domain.as_str()))
            .map(|domain| domain.current_records)
            .sum::<u64>(),
        current_records
    );
    assert!(payload_bytes > 0);
    assert!(page_class_bytes(&store, "mutable_overlay_record_slab_page") > 0);
    assert!(page_class_bytes(&store, "mutable_overlay_tree_page") > 0);
    assert!(
        status.physical_bytes <= payload_bytes + 768 * 1024,
        "random new-item bundles used {} physical bytes for {} payload bytes",
        status.physical_bytes,
        payload_bytes
    );
}

#[test]
fn hot_overlay_writes_reclaim_superseded_current_record_pages() {
    let path = TempPath::new("mutable-overlay-reclaim");
    let store = FileStore::open(path.path()).unwrap();
    let key = hot_key();

    write_hot_values(&store, &key, 0, 1);
    let first = store.maintenance_status().unwrap();
    write_hot_values(&store, &key, 1, 96);
    let warm = store.maintenance_status().unwrap();
    write_hot_values(&store, &key, 97, 96);
    let measured = store.maintenance_status().unwrap();
    let report = store.store_maintenance_report(0).unwrap();
    let measured_growth = measured.physical_bytes.saturating_sub(warm.physical_bytes);

    assert_eq!(report.overlay_health.current_record_count, 1);
    assert_eq!(report.overlay_health.hot_write_count, 193);
    assert_eq!(report.overlay_health.current_generation, 193);
    assert!(
        warm.physical_bytes > first.physical_bytes,
        "put_mutable_overlay_value must exercise the persistent overlay writer"
    );
    assert!(
        measured_growth <= 64 * 1024,
        "commit_mutable_overlay_records grew {} bytes over 96 hot writes after warmup",
        measured_growth
    );
}
