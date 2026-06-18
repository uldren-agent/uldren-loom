use super::*;
use loom_core::MemoryStore;

#[test]
fn memory_store_passes_cas_behavior() {
    let mut store = MemoryStore::new();
    run_cas_behavior(&mut store).expect("MemoryStore must satisfy the cas behavioral suite");
}

#[test]
fn memory_store_passes_inference_behavior() {
    run_inference_behavior().expect("MemoryStore must satisfy the inference behavioral suite");
}

#[test]
fn memory_store_passes_embedding_behavior() {
    run_embedding_behavior().expect("MemoryStore must satisfy the embedding behavioral suite");
}

#[test]
fn memory_store_passes_sql_error_behavior() {
    run_sql_error_behavior().expect("MemoryStore must satisfy the SQL error behavior suite");
}

#[test]
fn memory_store_passes_sql_history_behavior() {
    run_sql_history_behavior().expect("MemoryStore must satisfy the SQL history behavior suite");
}

#[test]
fn memory_store_passes_cas_facade_behavior() {
    let mut loom = Loom::new(MemoryStore::new());
    let mut dst = Loom::new(MemoryStore::new());
    run_cas_facade_behavior(&mut loom, &mut dst)
        .expect("MemoryStore must satisfy the workspace-scoped cas facade suite");
}

#[test]
fn memory_store_passes_kv_facade_behavior() {
    let mut loom = Loom::new(MemoryStore::new());
    let mut dst = Loom::new(MemoryStore::new());
    run_kv_facade_behavior(&mut loom, &mut dst)
        .expect("MemoryStore must satisfy the workspace-scoped kv facade suite");
}

#[test]
fn memory_store_passes_ephemeral_kv_behavior() {
    let mut loom = Loom::new(MemoryStore::new());
    run_ephemeral_kv_behavior(&mut loom).expect("MemoryStore must satisfy the ephemeral kv suite");
}

#[test]
fn memory_store_passes_lock_behavior() {
    run_lock_behavior().expect("MemoryStore must satisfy the embedded lock suite");
}

#[test]
fn memory_store_passes_identity_behavior() {
    run_identity_behavior().expect("MemoryStore must satisfy the identity suite");
}

#[test]
fn memory_store_passes_acl_behavior() {
    let mut loom = Loom::new(MemoryStore::new());
    run_acl_behavior(&mut loom).expect("MemoryStore must satisfy the acl suite");
}

#[test]
fn memory_store_passes_capability_behavior() {
    let loom = Loom::new(MemoryStore::new());
    run_capability_behavior(&loom).expect("MemoryStore must satisfy the capability suite");
}

#[test]
fn memory_store_passes_document_facade_behavior() {
    let mut loom = Loom::new(MemoryStore::new());
    let mut dst = Loom::new(MemoryStore::new());
    run_document_facade_behavior(&mut loom, &mut dst)
        .expect("MemoryStore must satisfy the workspace-scoped document facade suite");
}

#[test]
fn file_store_passes_derived_artifact_recovery_behavior() {
    run_derived_artifact_recovery_behavior()
        .expect("FileStore must satisfy the derived-artifact recovery suite");
}

#[test]
fn memory_store_passes_conditional_mutation_behavior() {
    let mut loom = Loom::new(MemoryStore::new());
    run_conditional_mutation_behavior(&mut loom)
        .expect("MemoryStore must satisfy the conditional mutation suite");
}

#[test]
fn memory_store_passes_dataframe_facade_behavior() {
    let mut loom = Loom::new(MemoryStore::new());
    let mut dst = Loom::new(MemoryStore::new());
    run_dataframe_facade_behavior(&mut loom, &mut dst)
        .expect("MemoryStore must satisfy the workspace-scoped dataframe facade suite");
}

#[test]
fn memory_store_passes_timeseries_facade_behavior() {
    let mut loom = Loom::new(MemoryStore::new());
    let mut dst = Loom::new(MemoryStore::new());
    run_timeseries_facade_behavior(&mut loom, &mut dst)
        .expect("MemoryStore must satisfy the workspace-scoped time-series facade suite");
}

#[test]
fn memory_store_passes_metrics_behavior() {
    let mut loom = Loom::new(MemoryStore::new());
    run_metrics_behavior(&mut loom).expect("MemoryStore must satisfy the metrics suite");
}

#[test]
fn memory_store_passes_ledger_facade_behavior() {
    let mut loom = Loom::new(MemoryStore::new());
    let mut dst = Loom::new(MemoryStore::new());
    run_ledger_facade_behavior(&mut loom, &mut dst)
        .expect("MemoryStore must satisfy the workspace-scoped ledger facade suite");
}

#[test]
fn memory_store_passes_merge_conflict_behavior() {
    let mut loom = Loom::new(MemoryStore::new());
    run_merge_conflict_behavior(&mut loom)
        .expect("MemoryStore must satisfy the merge-conflict suite");
}

#[test]
fn memory_store_passes_staging_behavior() {
    let mut loom = Loom::new(MemoryStore::new());
    run_staging_behavior(&mut loom).expect("MemoryStore must satisfy the staging suite");
}

#[test]
fn memory_store_passes_file_ops_behavior() {
    let mut loom = Loom::new(MemoryStore::new());
    run_file_ops_behavior(&mut loom).expect("MemoryStore must satisfy the file-ops suite");
}

#[test]
fn memory_store_passes_file_handle_behavior() {
    let mut loom = Loom::new(MemoryStore::new());
    run_file_handle_behavior(&mut loom).expect("MemoryStore must satisfy the file-handle suite");
}

#[test]
fn memory_store_passes_symlink_behavior() {
    let mut loom = Loom::new(MemoryStore::new());
    run_symlink_behavior(&mut loom).expect("MemoryStore must satisfy the symlink suite");
}

#[test]
fn memory_store_passes_tags_behavior() {
    let mut loom = Loom::new(MemoryStore::new());
    run_tags_behavior(&mut loom).expect("MemoryStore must satisfy the tags suite");
}

#[test]
fn memory_store_passes_restore_behavior() {
    let mut loom = Loom::new(MemoryStore::new());
    run_restore_behavior(&mut loom).expect("MemoryStore must satisfy the restore suite");
}

#[test]
fn memory_store_passes_replay_behavior() {
    let mut loom = Loom::new(MemoryStore::new());
    run_replay_behavior(&mut loom).expect("MemoryStore must satisfy the replay suite");
}

#[test]
fn memory_store_passes_squash_behavior() {
    let mut loom = Loom::new(MemoryStore::new());
    run_squash_behavior(&mut loom).expect("MemoryStore must satisfy the squash suite");
}

#[test]
fn memory_store_passes_protected_ref_behavior() {
    let mut loom = Loom::new(MemoryStore::new());
    run_protected_ref_behavior(&mut loom)
        .expect("MemoryStore must satisfy the protected-ref suite");
}

#[test]
fn memory_store_passes_diff_commits_behavior() {
    let mut loom = Loom::new(MemoryStore::new());
    run_diff_commits_behavior(&mut loom).expect("MemoryStore must satisfy the vcs-diff suite");
}

#[test]
fn memory_store_passes_watch_behavior() {
    let mut loom = Loom::new(MemoryStore::new());
    run_watch_behavior(&mut loom).expect("MemoryStore must satisfy the watch suite");
}

#[test]
fn memory_store_passes_workspace_behavior() {
    let mut loom = Loom::new(MemoryStore::new());
    let mut imported = Loom::new(MemoryStore::new());
    run_workspace_behavior(&mut loom, &mut imported)
        .expect("MemoryStore must satisfy the workspace behavioral suite");
}

#[test]
fn memory_store_passes_sync_behavior() {
    let mut src = Loom::new(MemoryStore::new());
    let mut dst = Loom::new(MemoryStore::new());
    run_sync_behavior(&mut src, &mut dst)
        .expect("MemoryStore must satisfy the sync behavioral suite");
}

#[test]
fn memory_store_passes_queue_behavior() {
    let mut loom = Loom::new(MemoryStore::new());
    let mut dst = Loom::new(MemoryStore::new());
    run_queue_behavior(&mut loom, &mut dst)
        .expect("MemoryStore must satisfy the queue behavioral suite");
}

#[test]
fn memory_store_passes_consumer_behavior() {
    let mut loom = Loom::new(MemoryStore::new());
    let mut dst = Loom::new(MemoryStore::new());
    run_consumer_behavior(&mut loom, &mut dst)
        .expect("MemoryStore must satisfy the queue consumer-offset suite");
}

#[test]
fn memory_store_passes_delivery_behavior() {
    let mut loom = Loom::new(MemoryStore::new());
    run_delivery_behavior(&mut loom).expect("MemoryStore must satisfy the delivery suite");
}

#[test]
fn memory_store_passes_pim_trigger_behavior() {
    let mut loom = Loom::new(MemoryStore::new());
    run_pim_trigger_behavior(&mut loom).expect("MemoryStore must satisfy the PIM trigger suite");
}

#[test]
fn memory_store_passes_exec_behavior() {
    let mut loom = Loom::new(MemoryStore::new());
    run_exec_behavior(&mut loom).expect("MemoryStore must satisfy the exec behavioral suite");
}

#[test]
fn memory_store_passes_sql_state_access_behavior() {
    let mut loom = Loom::new(MemoryStore::new());
    run_sql_state_access_behavior(&mut loom)
        .expect("MemoryStore must satisfy the SQL StateAccess suite");
}

#[test]
fn file_store_passes_ticket_comment_behavior() {
    run_ticket_comment_behavior().expect("FileStore must satisfy the ticket comment suite");
}

#[test]
fn every_suite_has_scenarios() {
    // Sanity: the index is wired, every suite has scenarios, and capability names are unique. Dynamic on
    // purpose - adding a suite must not require editing a hardcoded count - while still failing on a
    // genuine mistake: an empty index, an empty suite, or a duplicate capability entry.
    assert!(
        !BEHAVIOR_SUITES.is_empty(),
        "behavioral suite index is empty"
    );
    let mut seen = std::collections::BTreeSet::new();
    for (cap, suite) in BEHAVIOR_SUITES {
        assert!(!suite.is_empty(), "behavioral suite for `{cap}` is empty");
        assert!(seen.insert(*cap), "duplicate behavioral suite for `{cap}`");
    }
}
