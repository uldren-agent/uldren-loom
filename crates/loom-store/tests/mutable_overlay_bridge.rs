use loom_core::error::Code;
use loom_store::{MutableOverlay, OverlayKey};
use std::collections::BTreeMap;

fn key(record_id: &[u8]) -> OverlayKey {
    OverlayKey::from_segments([
        b"workspace",
        &[7; 16],
        b"tickets",
        b"matrix",
        b"ticket",
        record_id,
    ])
    .unwrap()
}

fn base(
    values: BTreeMap<OverlayKey, Vec<u8>>,
) -> impl Fn(&OverlayKey) -> loom_core::Result<Option<Vec<u8>>> {
    move |key| Ok(values.get(key).cloned())
}

#[test]
fn composite_read_prefers_overlay_value_over_base() {
    let item = key(b"MX-1");
    let mut base_values = BTreeMap::new();
    base_values.insert(item.clone(), b"base".to_vec());
    let mut overlay = MutableOverlay::new();
    overlay
        .put_value(item.clone(), None, b"overlay".to_vec())
        .unwrap();

    let read = overlay
        .snapshot()
        .read_composite(&item, base(base_values))
        .unwrap();

    assert_eq!(read.as_deref(), Some(&b"overlay"[..]));
}

#[test]
fn composite_read_tombstone_masks_base_value() {
    let item = key(b"MX-2");
    let mut base_values = BTreeMap::new();
    base_values.insert(item.clone(), b"base".to_vec());
    let mut overlay = MutableOverlay::new();
    overlay.put_tombstone(item.clone(), None).unwrap();

    let read = overlay
        .snapshot()
        .read_composite(&item, base(base_values))
        .unwrap();

    assert_eq!(read, None);
}

#[test]
fn composite_read_falls_back_to_base_when_overlay_has_no_entry() {
    let item = key(b"MX-3");
    let mut base_values = BTreeMap::new();
    base_values.insert(item.clone(), b"base".to_vec());
    let overlay = MutableOverlay::new();

    let read = overlay
        .snapshot()
        .read_composite(&item, base(base_values))
        .unwrap();

    assert_eq!(read.as_deref(), Some(&b"base"[..]));
}

#[test]
fn snapshot_generation_isolates_later_overlay_writes() {
    let item = key(b"MX-4");
    let mut overlay = MutableOverlay::new();
    let first = overlay
        .put_value(item.clone(), None, b"first".to_vec())
        .unwrap();
    let snapshot = overlay.snapshot();
    overlay
        .put_value(item.clone(), Some(&first), b"second".to_vec())
        .unwrap();

    let read = snapshot
        .read_composite(&item, base(BTreeMap::new()))
        .unwrap();
    let latest = overlay
        .snapshot()
        .read_composite(&item, base(BTreeMap::new()))
        .unwrap();

    assert_eq!(read.as_deref(), Some(&b"first"[..]));
    assert_eq!(latest.as_deref(), Some(&b"second"[..]));
}

#[test]
fn compare_token_validation_rejects_stale_owner_token() {
    let item = key(b"MX-5");
    let other = key(b"MX-other");
    let mut overlay = MutableOverlay::new();
    let current = overlay
        .put_value(item.clone(), None, b"first".to_vec())
        .unwrap();
    let stale = overlay.put_value(other, None, b"other".to_vec()).unwrap();
    let error = overlay
        .put_value(item.clone(), Some(&stale), b"bad".to_vec())
        .unwrap_err();

    assert_eq!(error.code, Code::Conflict);
    assert_eq!(
        overlay
            .snapshot()
            .owner_token(&item)
            .unwrap()
            .map(|token| token.as_bytes().to_owned()),
        Some(*current.as_bytes())
    );
}

#[test]
fn overlay_health_reports_current_records_and_hot_writes() {
    let first = key(b"MX-6");
    let second = key(b"MX-7");
    let mut overlay = MutableOverlay::new();
    let first_token = overlay
        .put_value(first.clone(), None, b"first".to_vec())
        .unwrap();
    overlay
        .put_value(first, Some(&first_token), b"second".to_vec())
        .unwrap();
    overlay.put_tombstone(second, None).unwrap();

    let health = overlay.health().unwrap();

    assert_eq!(health.current_generation, 3);
    assert_eq!(health.current_record_count, 2);
    assert_eq!(health.tombstone_count, 1);
    assert_eq!(health.live_checkpoint_references, 0);
    assert_eq!(health.reclaimable_overlay_pages, 0);
    assert!(health.blocked_reclamation_reasons.is_empty());
    assert_eq!(health.hot_write_count, 3);
    assert_eq!(health.active_writer_contention_indicators, 0);
}
