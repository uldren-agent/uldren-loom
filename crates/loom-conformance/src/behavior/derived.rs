use loom_core::capability::{CapabilityOperationalState, CapabilitySet};
use loom_core::{Code, Digest, FacetKind, Result, WorkspaceId};
use loom_store::FileStore;
use loom_store::derived::{
    DerivedArtifactKey, DerivedArtifactRead, DerivedArtifactRebuild, DerivedArtifactServingMode,
    DerivedArtifactStamp, DerivedArtifactStatus,
};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn run_derived_artifact_recovery_behavior() -> Result<()> {
    let path = std::env::temp_dir().join(format!(
        "loom-derived-recovery-{}-{}.loom",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default()
    ));
    let store = FileStore::open(&path)?;
    let result = run_derived_artifact_recovery_behavior_in_store(&store);
    let _ = std::fs::remove_file(&path);
    result
}

pub fn run_derived_artifact_recovery_behavior_in_store(store: &FileStore) -> Result<()> {
    let workspace = WorkspaceId::from_bytes([39; 16]);
    let key = DerivedArtifactKey::new(workspace, FacetKind::Search, "docs", "tantivy")?;
    let ready_stamp = DerivedArtifactStamp::new(
        Digest::blake3(b"source-ready"),
        "tantivy-1",
        "search-index-v1",
    )?;
    let stale_stamp = DerivedArtifactStamp::new(
        Digest::blake3(b"source-stale"),
        "tantivy-1",
        "search-index-v1",
    )?;

    assert_status(
        &store.derived_artifact_status(&key, &ready_stamp)?,
        "missing",
        DerivedArtifactServingMode::AuthoritativeSource,
        CapabilityOperationalState::Degraded,
        Some("derived_artifact_missing"),
        None,
    );
    assert_eq!(
        store.read_derived_artifact(&key, &ready_stamp)?,
        DerivedArtifactRead::Missing
    );

    let record = store.put_derived_artifact(&key, ready_stamp.clone(), b"ready-bytes")?;
    assert_status(
        &store.derived_artifact_status(&key, &ready_stamp)?,
        "ready",
        DerivedArtifactServingMode::DerivedArtifact,
        CapabilityOperationalState::Supported,
        None,
        None,
    );
    match store.read_derived_artifact(&key, &ready_stamp)? {
        DerivedArtifactRead::Ready {
            record: got,
            payload,
        } => {
            assert_eq!(got, record);
            assert_eq!(payload, b"ready-bytes");
        }
        other => panic!("expected ready derived artifact, got {other:?}"),
    }

    assert_status(
        &store.derived_artifact_status(&key, &stale_stamp)?,
        "stale",
        DerivedArtifactServingMode::AuthoritativeSource,
        CapabilityOperationalState::Degraded,
        Some("derived_artifact_stale"),
        None,
    );
    match store.read_derived_artifact(&key, &stale_stamp)? {
        DerivedArtifactRead::Stale { record: got } => assert_eq!(got, record),
        other => panic!("expected stale derived artifact, got {other:?}"),
    }

    let rebuild = store.begin_derived_artifact_rebuild(&key, stale_stamp.clone())?;
    let DerivedArtifactRebuild::Started { run_id } = rebuild else {
        panic!("expected started rebuild, got {rebuild:?}");
    };
    assert_eq!(
        store.begin_derived_artifact_rebuild(&key, stale_stamp.clone())?,
        DerivedArtifactRebuild::Coalesced {
            run_id: run_id.clone()
        }
    );
    assert_status(
        &store.derived_artifact_status(&key, &stale_stamp)?,
        "rebuilding",
        DerivedArtifactServingMode::AuthoritativeSource,
        CapabilityOperationalState::Degraded,
        Some("index_rebuilding"),
        None,
    );

    store.fail_derived_artifact_rebuild(
        &key,
        &run_id,
        stale_stamp.clone(),
        "index writer failed",
    )?;
    assert_status(
        &store.derived_artifact_status(&key, &stale_stamp)?,
        "failed",
        DerivedArtifactServingMode::AuthoritativeSource,
        CapabilityOperationalState::Degraded,
        Some("derived_artifact_failed"),
        None,
    );

    store.mark_derived_artifact_unsupported(
        &key,
        stale_stamp.clone(),
        "native engine unavailable",
    )?;
    let unsupported = store.derived_artifact_status(&key, &stale_stamp)?;
    assert_status(
        &unsupported,
        "unsupported",
        DerivedArtifactServingMode::AuthoritativeSource,
        CapabilityOperationalState::Unsupported,
        Some("profile_unsupported"),
        Some(Code::Unsupported),
    );
    let capabilities =
        unsupported.apply_serving_policy_to_capabilities(CapabilitySet::registry(), "search");
    let search = capabilities
        .get("search")
        .expect("canonical registry contains search");
    assert_eq!(
        search.operational_state,
        CapabilityOperationalState::Unsupported
    );
    assert_eq!(search.reason_code, Some("profile_unsupported"));
    assert_eq!(search.stable_error, Some(Code::Unsupported));
    Ok(())
}

fn assert_status(
    status: &DerivedArtifactStatus,
    name: &str,
    mode: DerivedArtifactServingMode,
    operational_state: CapabilityOperationalState,
    reason_code: Option<&'static str>,
    stable_error: Option<Code>,
) {
    assert_eq!(status.name(), name);
    let policy = status.serving_policy();
    assert_eq!(policy.mode, mode);
    assert_eq!(policy.operational_state, operational_state);
    assert_eq!(policy.reason_code, reason_code);
    assert_eq!(policy.stable_error, stable_error);
    let capabilities =
        status.apply_serving_policy_to_capabilities(CapabilitySet::registry(), "search");
    let search = capabilities
        .get("search")
        .expect("canonical registry contains search");
    assert_eq!(search.operational_state, operational_state);
    assert_eq!(search.reason_code, reason_code);
    assert_eq!(search.stable_error, stable_error);
}
