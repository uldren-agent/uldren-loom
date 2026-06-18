use loom_core::Loom;
use loom_core::error::Result;
use loom_core::workspace::WorkspaceId;
use loom_store::FileStore;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct HostedReferenceReconciliationStatus {
    pub pending: u64,
    pub resolved: u64,
    pub failed: u64,
    pub active_targets: u64,
    pub next_attempt_ms: Option<u64>,
    pub unsupported_targets: u64,
}

pub fn reference_reconciliation_status(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
) -> Result<HostedReferenceReconciliationStatus> {
    let summary = loom_reference::status(loom, workspace)?;
    let targets = loom_reference::targets(loom, workspace)?;
    let next_attempt_ms = targets.iter().map(|target| target.next_attempt_ms).min();
    let unsupported_targets = targets
        .iter()
        .filter(|target| target.source_profile != "tickets")
        .count() as u64;
    Ok(HostedReferenceReconciliationStatus {
        pending: summary.pending,
        resolved: summary.resolved,
        failed: summary.failed,
        active_targets: targets.len() as u64,
        next_attempt_ms,
        unsupported_targets,
    })
}
