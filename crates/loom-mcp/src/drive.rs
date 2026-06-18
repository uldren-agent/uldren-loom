use loom_core::error::{Code, LoomError, Result};
use loom_core::workspace::WorkspaceId;
use loom_core::{
    AclDomain, AclEffect, AclGrant, AclResource, AclResourceScope, AclRight, AclScope,
    AclScopeKind, AclSubject, Digest, Fence, Loom, cas_get, cas_put,
};
use loom_store::FileStore;
use loom_substrate::drive::{
    APP_ID, DriveChunkManifest, DriveChunkRef, DriveConflictIndex, DriveConflictRecord,
    DriveConflictResolution, DriveContentRef, DriveFileVersion, DriveFileVersionIndex,
    DriveFolderChildren, DriveFolderEntry, DriveFolderIndex, DriveNodeKind, DriveOperationLog,
    DriveOperationRecord, DriveProfileSnapshot, DriveRetentionIndex, DriveRetentionPin,
    DriveRetentionPinInput, DriveRetentionPinKind, DriveShareGrant, DriveShareGrantInput,
    DriveShareIndex, DriveShareRole, DriveShareTargetKind, DriveUploadChunk, DriveUploadSession,
    DriveUploadSessionInput, DriveUploadTargetKind, conflict_copy_name, drive_conflict_index_key,
    drive_operation_log_key, drive_profile_key, drive_retention_index_key, drive_share_index_key,
    drive_upload_session_key, is_drive_dehydrated_file_marker,
};
use loom_substrate::versioning::{
    BodyRef, ProfileRevisionUpdate, ProfileTransaction, ProfileTransactionState, RevisionIndex,
};
use loom_substrate::{ActorKind, OperationEnvelope, OperationEnvelopeInput};
use serde::Serialize;

use crate::substrate_revisions::{REVISION_INDEX_DIR, revision_index_path};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DriveEntrySummary {
    pub name: String,
    pub fold_key: String,
    pub node_id: String,
    pub kind: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DriveFolderSummary {
    pub workspace_id: String,
    pub folder_id: String,
    pub profile_root: String,
    pub entries: Vec<DriveEntrySummary>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DriveStatSummary {
    pub workspace_id: String,
    pub node_id: String,
    pub name: String,
    pub kind: String,
    pub profile_root: String,
    pub latest_version: Option<DriveVersionSummary>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DriveVersionSummary {
    pub file_id: String,
    pub version: u64,
    pub operation_id: String,
    pub author_principal: String,
    pub timestamp_ms: u64,
    pub content_digest: String,
    pub manifest_digest: Option<String>,
    pub size: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DriveUploadSessionSummary {
    pub workspace_id: String,
    pub upload_id: String,
    pub target_kind: String,
    pub parent_folder_id: String,
    pub name: String,
    pub file_id: String,
    pub expected_root: String,
    pub chunk_count: usize,
    pub total_size: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DriveConflictSummary {
    pub conflict_id: String,
    pub folder_id: String,
    pub visible_node_id: String,
    pub conflict_node_id: String,
    pub conflict_name: String,
    pub base_root: String,
    pub resolution: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DriveShareGrantSummary {
    pub grant_id: String,
    pub target_kind: String,
    pub target_id: String,
    pub principal: String,
    pub role: String,
    pub granted_by: String,
    pub granted_at_ms: u64,
    pub expires_at_ms: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DriveRetentionPinSummary {
    pub pin_id: String,
    pub kind: String,
    pub root: String,
    pub target_entity_id: Option<String>,
    pub added_by: String,
    pub added_at_ms: u64,
    pub expires_at_ms: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DriveRetentionApplySummary {
    pub workspace_id: String,
    pub now_ms: u64,
    pub expired_pin_ids: Vec<String>,
    pub remaining_pins: usize,
    pub operation: Option<DriveWriteSummary>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DriveShareExpiryApplySummary {
    pub workspace_id: String,
    pub now_ms: u64,
    pub expired_grant_ids: Vec<String>,
    pub remaining_grants: usize,
    pub operation: Option<DriveWriteSummary>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DriveWriteSummary {
    pub workspace_id: String,
    pub operation_id: String,
    pub operation_kind: String,
    pub sequence: u64,
    pub profile_root: String,
    pub target_entity_id: Option<String>,
    pub conflict_id: Option<String>,
}

pub struct DriveGrantShareRequest<'a> {
    pub workspace_id: &'a str,
    pub grant_id: &'a str,
    pub target_kind: &'a str,
    pub target_id: &'a str,
    pub principal: &'a str,
    pub role: &'a str,
    pub granted_at_ms: u64,
    pub expires_at_ms: Option<u64>,
}

pub struct DrivePinRetentionRequest<'a> {
    pub workspace_id: &'a str,
    pub pin_id: &'a str,
    pub kind: &'a str,
    pub root: &'a str,
    pub target_entity_id: Option<&'a str>,
    pub added_at_ms: u64,
    pub expires_at_ms: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FenceSummary {
    pub authority: u32,
    pub epoch: u32,
    pub sequence: u64,
}

impl From<Fence> for FenceSummary {
    fn from(fence: Fence) -> Self {
        Self {
            authority: fence.authority(),
            epoch: fence.epoch(),
            sequence: fence.sequence(),
        }
    }
}

impl From<FenceSummary> for Fence {
    fn from(fence: FenceSummary) -> Self {
        Self::new(fence.authority, fence.epoch, fence.sequence)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DriveLeaseTokenSummary {
    pub key: String,
    pub principal: String,
    pub session: String,
    pub mode: String,
    pub fence: FenceSummary,
    pub lease_deadline_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DriveLeaseBreakSummary {
    pub key: String,
    pub broken_holders: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriveConflictResolutionRequest {
    Current,
    Conflict,
    Both,
}

pub fn drive_lease_key(
    workspace: WorkspaceId,
    workspace_id: &str,
    target_kind: &str,
    target_id: &str,
) -> Result<String> {
    validate_lock_segment("workspace_id", workspace_id)?;
    match target_kind {
        "file" | "folder" => {}
        _ => {
            return Err(LoomError::invalid(
                "drive lease target_kind must be file or folder",
            ));
        }
    }
    validate_lock_segment("target_id", target_id)?;
    Ok(format!(
        "drive/{workspace}/{workspace_id}/{target_kind}/{target_id}"
    ))
}

pub fn record_lease_operation(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    operation_kind: &str,
    target_kind: &str,
    target_id: &str,
) -> Result<DriveWriteSummary> {
    authorize_drive_target(
        loom,
        workspace,
        workspace_id,
        target_kind,
        target_id,
        AclRight::Write,
    )?;
    let snapshot = load_snapshot_or_empty(loom, workspace_id)?;
    let root = profile_root(loom, &snapshot)?;
    let target_entity_id = format!("{target_kind}:{target_id}");
    record_operation(
        loom,
        workspace,
        &snapshot,
        operation_kind,
        Some(&target_entity_id),
        root,
        operation_kind.as_bytes(),
    )
}

pub fn list_folder(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    folder_id: &str,
) -> Result<DriveFolderSummary> {
    authorize_drive_target(
        loom,
        workspace,
        workspace_id,
        "folder",
        folder_id,
        AclRight::Read,
    )?;
    let snapshot = if folder_id == "root" {
        load_snapshot_or_empty(loom, workspace_id)?
    } else {
        load_snapshot(loom, workspace_id)?
    };
    let profile_root = profile_root(loom, &snapshot)?;
    let folder = snapshot
        .folders
        .children(folder_id)
        .ok_or_else(|| LoomError::not_found("drive folder not found"))?;
    Ok(DriveFolderSummary {
        workspace_id: snapshot.workspace_id,
        folder_id: folder.folder_id.clone(),
        profile_root: profile_root.to_string(),
        entries: folder
            .entries
            .iter()
            .map(|entry| DriveEntrySummary {
                name: entry.name.clone(),
                fold_key: entry.fold_key.clone(),
                node_id: entry.node_id.clone(),
                kind: node_kind(entry.kind).to_string(),
            })
            .collect(),
    })
}

pub fn stat_node(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    folder_id: &str,
    name: &str,
) -> Result<DriveStatSummary> {
    let snapshot = load_snapshot(loom, workspace_id)?;
    let profile_root = profile_root(loom, &snapshot)?;
    let folder = snapshot
        .folders
        .children(folder_id)
        .ok_or_else(|| LoomError::not_found("drive folder not found"))?;
    let entry = folder
        .entry_by_name(name)?
        .ok_or_else(|| LoomError::not_found("drive entry not found"))?;
    authorize_drive_target(
        loom,
        workspace,
        workspace_id,
        node_kind(entry.kind),
        &entry.node_id,
        AclRight::Read,
    )?;
    Ok(DriveStatSummary {
        workspace_id: snapshot.workspace_id.clone(),
        node_id: entry.node_id.clone(),
        name: entry.name.clone(),
        kind: node_kind(entry.kind).to_string(),
        profile_root: profile_root.to_string(),
        latest_version: snapshot
            .versions
            .latest(&entry.node_id)
            .map(version_summary),
    })
}

pub fn read_file(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    file_id: &str,
) -> Result<Vec<u8>> {
    authorize_drive_target(
        loom,
        workspace,
        workspace_id,
        "file",
        file_id,
        AclRight::Read,
    )?;
    let snapshot = load_snapshot(loom, workspace_id)?;
    let version = snapshot
        .versions
        .latest(file_id)
        .ok_or_else(|| LoomError::not_found("drive file version not found"))?;
    read_content(loom, workspace, &version.content)
}

pub fn list_versions(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    file_id: &str,
) -> Result<Vec<DriveVersionSummary>> {
    authorize_drive_target(
        loom,
        workspace,
        workspace_id,
        "file",
        file_id,
        AclRight::Read,
    )?;
    let snapshot = load_snapshot(loom, workspace_id)?;
    let versions = snapshot
        .versions
        .versions
        .into_iter()
        .filter(|version| version.file_id == file_id)
        .map(|version| version_summary(&version))
        .collect::<Vec<_>>();
    if versions.is_empty() {
        return Err(LoomError::not_found("drive file version not found"));
    }
    Ok(versions)
}

pub fn list_conflicts(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
) -> Result<Vec<DriveConflictSummary>> {
    authorize_drive_collection(loom, workspace, workspace_id, AclRight::Read)?;
    load_conflicts(loom, workspace_id)?
        .conflicts
        .iter()
        .map(conflict_summary)
        .collect()
}

pub fn list_share_grants(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
) -> Result<Vec<DriveShareGrantSummary>> {
    authorize_drive_collection(loom, workspace, workspace_id, AclRight::Admin)?;
    Ok(load_shares(loom, workspace_id)?
        .grants
        .iter()
        .map(share_summary)
        .collect())
}

pub fn grant_share(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    request: DriveGrantShareRequest<'_>,
) -> Result<DriveWriteSummary> {
    authorize_drive_collection(loom, workspace, request.workspace_id, AclRight::Admin)?;
    let actor = loom.effective_principal()?.unwrap_or(workspace);
    let grant = DriveShareGrant::new(DriveShareGrantInput {
        grant_id: request.grant_id.to_string(),
        target_kind: parse_share_target_kind(request.target_kind)?,
        target_id: request.target_id.to_string(),
        principal: WorkspaceId::parse(request.principal)?,
        role: parse_share_role(request.role)?,
        granted_by: actor,
        granted_at_ms: request.granted_at_ms,
        expires_at_ms: request.expires_at_ms,
    })?;
    let mut shares = load_shares(loom, request.workspace_id)?;
    if shares
        .grants
        .iter()
        .any(|candidate| candidate.grant_id == request.grant_id)
    {
        return Err(LoomError::new(
            Code::AlreadyExists,
            "drive share grant already exists",
        ));
    }
    let acl_grant = drive_share_acl_grant(request.workspace_id, &grant);
    shares.grants.push(grant);
    shares = DriveShareIndex::new(request.workspace_id, shares.grants)?;
    save_shares(loom, &shares)?;
    let acl_snapshot = {
        let acl = loom.acl_store_mut();
        acl.grant(acl_grant)?;
        acl.clone()
    };
    loom.store().save_acl_store_audited(
        &acl_snapshot,
        Some(actor),
        "drive.share_acl.grant",
        Some(&format!(
            "drive:{};share:{}",
            request.workspace_id, request.grant_id
        )),
    )?;
    let snapshot = load_snapshot_or_empty(loom, request.workspace_id)?;
    record_operation(
        loom,
        workspace,
        &snapshot,
        "share.granted",
        Some(&format!("share:{}", request.grant_id)),
        profile_root(loom, &snapshot)?,
        request.grant_id.as_bytes(),
    )
}

pub fn revoke_share(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    grant_id: &str,
) -> Result<DriveWriteSummary> {
    authorize_drive_collection(loom, workspace, workspace_id, AclRight::Admin)?;
    let mut shares = load_shares(loom, workspace_id)?;
    let idx = shares
        .grants
        .iter()
        .position(|candidate| candidate.grant_id == grant_id)
        .ok_or_else(|| LoomError::not_found("drive share grant not found"))?;
    let grant = shares.grants.remove(idx);
    shares = DriveShareIndex::new(workspace_id, shares.grants)?;
    save_shares(loom, &shares)?;
    let acl_grant = drive_share_acl_grant(workspace_id, &grant);
    let acl_snapshot = {
        let acl = loom.acl_store_mut();
        let removed = acl.revoke_one(&acl_grant);
        removed.then(|| acl.clone())
    };
    if let Some(snapshot) = acl_snapshot {
        loom.store().save_acl_store_audited(
            &snapshot,
            loom.effective_principal()?,
            "drive.share_acl.revoke",
            Some(&format!("drive:{workspace_id};share:{grant_id}")),
        )?;
    }
    let snapshot = load_snapshot_or_empty(loom, workspace_id)?;
    record_operation(
        loom,
        workspace,
        &snapshot,
        "share.revoked",
        Some(&format!("share:{grant_id}")),
        profile_root(loom, &snapshot)?,
        grant_id.as_bytes(),
    )
}

pub fn apply_share_expiry(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    now_ms: u64,
) -> Result<DriveShareExpiryApplySummary> {
    authorize_drive_collection(loom, workspace, workspace_id, AclRight::Admin)?;
    let shares = load_shares(loom, workspace_id)?;
    let mut expired = Vec::new();
    let mut retained = Vec::new();
    for grant in shares.grants {
        if grant
            .expires_at_ms
            .is_some_and(|expires_at_ms| expires_at_ms <= now_ms)
        {
            expired.push(grant);
        } else {
            retained.push(grant);
        }
    }
    let operation = if expired.is_empty() {
        None
    } else {
        let expired_grant_ids = expired
            .iter()
            .map(|grant| grant.grant_id.clone())
            .collect::<Vec<_>>();
        let shares = DriveShareIndex::new(workspace_id, retained.clone())?;
        save_shares(loom, &shares)?;
        let acl_snapshot = {
            let acl = loom.acl_store_mut();
            let mut removed = false;
            for grant in &expired {
                removed |= acl.revoke_one(&drive_share_acl_grant(workspace_id, grant));
            }
            removed.then(|| acl.clone())
        };
        if let Some(snapshot) = acl_snapshot {
            loom.store().save_acl_store_audited(
                &snapshot,
                loom.effective_principal()?,
                "drive.share_acl.expire",
                Some(&format!("drive:{workspace_id};shares:expired")),
            )?;
        }
        let snapshot = load_snapshot_or_empty(loom, workspace_id)?;
        Some(record_operation(
            loom,
            workspace,
            &snapshot,
            "share.expired",
            Some("share:expired"),
            profile_root(loom, &snapshot)?,
            expired_grant_ids.join("\n").as_bytes(),
        )?)
    };
    let expired_grant_ids = expired.into_iter().map(|grant| grant.grant_id).collect();
    Ok(DriveShareExpiryApplySummary {
        workspace_id: workspace_id.to_string(),
        now_ms,
        expired_grant_ids,
        remaining_grants: retained.len(),
        operation,
    })
}

pub fn list_retention_pins(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
) -> Result<Vec<DriveRetentionPinSummary>> {
    authorize_drive_collection(loom, workspace, workspace_id, AclRight::Admin)?;
    Ok(load_retention(loom, workspace_id)?
        .pins
        .iter()
        .map(retention_summary)
        .collect())
}

pub fn pin_retention(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    request: DrivePinRetentionRequest<'_>,
) -> Result<DriveWriteSummary> {
    authorize_drive_collection(loom, workspace, request.workspace_id, AclRight::Admin)?;
    let actor = loom.effective_principal()?.unwrap_or(workspace);
    let pin = DriveRetentionPin::new(DriveRetentionPinInput {
        pin_id: request.pin_id.to_string(),
        kind: parse_retention_kind(request.kind)?,
        root: Digest::parse(request.root)?,
        target_entity_id: request.target_entity_id.map(str::to_string),
        added_by: actor,
        added_at_ms: request.added_at_ms,
        expires_at_ms: request.expires_at_ms,
    })?;
    let mut retention = load_retention(loom, request.workspace_id)?;
    if retention
        .pins
        .iter()
        .any(|candidate| candidate.pin_id == request.pin_id)
    {
        return Err(LoomError::new(
            Code::AlreadyExists,
            "drive retention pin already exists",
        ));
    }
    retention.pins.push(pin);
    retention = DriveRetentionIndex::new(request.workspace_id, retention.pins)?;
    save_retention(loom, &retention)?;
    let snapshot = load_snapshot_or_empty(loom, request.workspace_id)?;
    record_operation(
        loom,
        workspace,
        &snapshot,
        "retention.pinned",
        Some(&format!("retention:{}", request.pin_id)),
        profile_root(loom, &snapshot)?,
        request.pin_id.as_bytes(),
    )
}

pub fn unpin_retention(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    pin_id: &str,
) -> Result<DriveWriteSummary> {
    authorize_drive_collection(loom, workspace, workspace_id, AclRight::Admin)?;
    let mut retention = load_retention(loom, workspace_id)?;
    let idx = retention
        .pins
        .iter()
        .position(|candidate| candidate.pin_id == pin_id)
        .ok_or_else(|| LoomError::not_found("drive retention pin not found"))?;
    retention.pins.remove(idx);
    retention = DriveRetentionIndex::new(workspace_id, retention.pins)?;
    save_retention(loom, &retention)?;
    let snapshot = load_snapshot_or_empty(loom, workspace_id)?;
    record_operation(
        loom,
        workspace,
        &snapshot,
        "retention.unpinned",
        Some(&format!("retention:{pin_id}")),
        profile_root(loom, &snapshot)?,
        pin_id.as_bytes(),
    )
}

pub fn apply_retention(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    now_ms: u64,
) -> Result<DriveRetentionApplySummary> {
    authorize_drive_collection(loom, workspace, workspace_id, AclRight::Admin)?;
    let retention = load_retention(loom, workspace_id)?;
    let mut expired_pin_ids = Vec::new();
    let mut retained = Vec::new();
    for pin in retention.pins {
        if pin
            .expires_at_ms
            .is_some_and(|expires_at_ms| expires_at_ms <= now_ms)
        {
            expired_pin_ids.push(pin.pin_id);
        } else {
            retained.push(pin);
        }
    }
    let operation = if expired_pin_ids.is_empty() {
        None
    } else {
        let retention = DriveRetentionIndex::new(workspace_id, retained.clone())?;
        save_retention(loom, &retention)?;
        let snapshot = load_snapshot_or_empty(loom, workspace_id)?;
        Some(record_operation(
            loom,
            workspace,
            &snapshot,
            "retention.applied",
            Some("retention:expired"),
            profile_root(loom, &snapshot)?,
            expired_pin_ids.join("\n").as_bytes(),
        )?)
    };
    Ok(DriveRetentionApplySummary {
        workspace_id: workspace_id.to_string(),
        now_ms,
        expired_pin_ids,
        remaining_pins: retained.len(),
        operation,
    })
}

pub fn create_folder(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    parent_folder_id: &str,
    folder_id: &str,
    name: &str,
    expected_root: &str,
) -> Result<DriveWriteSummary> {
    authorize_drive_target(
        loom,
        workspace,
        workspace_id,
        "folder",
        parent_folder_id,
        AclRight::Write,
    )?;
    let mut snapshot = load_snapshot_or_empty(loom, workspace_id)?;
    let base_root = enforce_expected_root(loom, &snapshot, expected_root)?;
    ensure_folder_mut(&mut snapshot, parent_folder_id)?
        .entries
        .push(DriveFolderEntry::new(
            name,
            folder_id,
            DriveNodeKind::Folder,
        )?);
    snapshot
        .folders
        .folders
        .push(DriveFolderChildren::new(folder_id, Vec::new())?);
    snapshot.folders =
        DriveFolderIndex::new(snapshot.workspace_id.clone(), snapshot.folders.folders)?;
    save_snapshot(loom, &snapshot)?;
    record_operation(
        loom,
        workspace,
        &snapshot,
        "folder.created",
        Some(folder_id),
        base_root,
        b"folder.created",
    )
}

pub fn create_upload(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    request: DriveCreateUploadRequest<'_>,
) -> Result<DriveUploadSessionSummary> {
    let snapshot = load_snapshot_or_empty(loom, request.workspace_id)?;
    enforce_expected_root(loom, &snapshot, request.expected_root)?;
    let target_kind = if request.replace_file {
        authorize_drive_target(
            loom,
            workspace,
            request.workspace_id,
            "file",
            request.file_id,
            AclRight::Write,
        )?;
        if snapshot.versions.latest(request.file_id).is_none() {
            return Err(LoomError::not_found("drive replacement file not found"));
        }
        DriveUploadTargetKind::ReplaceFile
    } else {
        authorize_drive_target(
            loom,
            workspace,
            request.workspace_id,
            "folder",
            request.parent_folder_id,
            AclRight::Write,
        )?;
        ensure_folder(&snapshot, request.parent_folder_id)?;
        DriveUploadTargetKind::NewFile
    };
    let session = DriveUploadSession::new(DriveUploadSessionInput {
        workspace_id: request.workspace_id.to_string(),
        upload_id: request.upload_id.to_string(),
        target_kind,
        parent_folder_id: request.parent_folder_id.to_string(),
        name: request.name.to_string(),
        file_id: request.file_id.to_string(),
        expected_root: Digest::parse(request.expected_root)?,
        author_principal: loom.effective_principal()?.unwrap_or(workspace),
        created_at_ms: request.created_at_ms,
        chunks: Vec::new(),
    })?;
    save_upload_session(loom, &session)?;
    upload_summary(&session)
}

pub struct DriveCreateUploadRequest<'a> {
    pub workspace_id: &'a str,
    pub upload_id: &'a str,
    pub parent_folder_id: &'a str,
    pub name: &'a str,
    pub file_id: &'a str,
    pub expected_root: &'a str,
    pub created_at_ms: u64,
    pub replace_file: bool,
}

pub fn upload_chunk(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    upload_id: &str,
    bytes: &[u8],
) -> Result<DriveUploadSessionSummary> {
    let mut session = load_upload_session(loom, workspace_id, upload_id)?;
    let write_target = match session.target_kind {
        DriveUploadTargetKind::NewFile => ("folder", session.parent_folder_id.as_str()),
        DriveUploadTargetKind::ReplaceFile => ("file", session.file_id.as_str()),
    };
    authorize_drive_target(
        loom,
        workspace,
        workspace_id,
        write_target.0,
        write_target.1,
        AclRight::Write,
    )?;
    let digest = cas_put(loom, workspace, bytes)?;
    let size = u64::try_from(bytes.len())
        .map_err(|_| LoomError::new(Code::InvalidArgument, "drive chunk is too large"))?;
    let sequence = u64::try_from(session.chunks.len())
        .map_err(|_| LoomError::invalid("drive upload chunk count overflow"))?;
    session.append_chunk(DriveUploadChunk::new(sequence, digest, size)?)?;
    save_upload_session(loom, &session)?;
    upload_summary(&session)
}

pub fn commit_upload(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    upload_id: &str,
) -> Result<DriveWriteSummary> {
    let session = load_upload_session(loom, workspace_id, upload_id)?;
    let write_target = match session.target_kind {
        DriveUploadTargetKind::NewFile => ("folder", session.parent_folder_id.as_str()),
        DriveUploadTargetKind::ReplaceFile => ("file", session.file_id.as_str()),
    };
    authorize_drive_target(
        loom,
        workspace,
        workspace_id,
        write_target.0,
        write_target.1,
        AclRight::Write,
    )?;
    let mut snapshot = load_snapshot_or_empty(loom, workspace_id)?;
    let actual_root = profile_root(loom, &snapshot)?;
    let stale_base = actual_root != session.expected_root;
    if stale_base && session.target_kind != DriveUploadTargetKind::NewFile {
        return Err(LoomError::new(
            Code::Conflict,
            "drive replacement upload base is stale",
        ));
    }
    let base_root = session.expected_root;
    let content = upload_content_ref(loom, workspace, &session)?;
    let mut conflict_id = None;
    let next_version = snapshot
        .versions
        .latest(&session.file_id)
        .map_or(1, |version| version.version + 1);
    let file_version = DriveFileVersion::new(
        session.file_id.clone(),
        next_version,
        format!("upload:{upload_id}"),
        session.author_principal,
        crate::now_ms(),
        content,
    )?;
    snapshot.versions.versions.push(file_version.clone());
    snapshot.versions =
        DriveFileVersionIndex::new(snapshot.workspace_id.clone(), snapshot.versions.versions)?;
    if session.target_kind == DriveUploadTargetKind::NewFile {
        let parent = ensure_folder_mut(&mut snapshot, &session.parent_folder_id)?;
        if let Some(existing) = parent.entry_by_name(&session.name)?.cloned() {
            let conflict_name = conflict_copy_name(&session.name, "principal", crate::now_ms(), 1)?;
            parent.entries.push(DriveFolderEntry::new(
                &conflict_name,
                &session.file_id,
                DriveNodeKind::File,
            )?);
            let id = format!("{upload_id}:conflict");
            let mut conflicts = load_conflicts(loom, workspace_id)?;
            conflicts.append(DriveConflictRecord::new(
                &id,
                &session.parent_folder_id,
                existing.node_id,
                &session.file_id,
                conflict_name,
                session.expected_root,
                DriveConflictResolution::Open,
            )?)?;
            save_conflicts(loom, &conflicts)?;
            conflict_id = Some(id);
        } else {
            parent.entries.push(DriveFolderEntry::new(
                &session.name,
                &session.file_id,
                DriveNodeKind::File,
            )?);
        }
        snapshot.folders =
            DriveFolderIndex::new(snapshot.workspace_id.clone(), snapshot.folders.folders)?;
    }
    save_snapshot(loom, &snapshot)?;
    let operation_kind = match session.target_kind {
        DriveUploadTargetKind::NewFile => "file.upload_committed",
        DriveUploadTargetKind::ReplaceFile => "file.content_replaced",
    };
    let mut summary = record_operation(
        loom,
        workspace,
        &snapshot,
        operation_kind,
        Some(&session.file_id),
        base_root,
        b"upload.committed",
    )?;
    update_file_revision_index(
        loom,
        workspace,
        workspace_id,
        &file_version,
        summary.profile_root.as_str(),
    )?;
    summary.conflict_id = conflict_id;
    delete_upload_session(loom, workspace_id, upload_id)?;
    Ok(summary)
}

pub fn resolve_conflict(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    conflict_id: &str,
    resolution: DriveConflictResolutionRequest,
) -> Result<DriveWriteSummary> {
    authorize_drive_collection(loom, workspace, workspace_id, AclRight::Write)?;
    let mut snapshot = load_snapshot(loom, workspace_id)?;
    let base_root = profile_root(loom, &snapshot)?;
    let mut conflicts = load_conflicts(loom, workspace_id)?;
    let idx = conflicts
        .conflicts
        .iter()
        .position(|record| record.conflict_id == conflict_id)
        .ok_or_else(|| LoomError::not_found("drive conflict not found"))?;
    if conflicts.conflicts[idx].resolution != DriveConflictResolution::Open {
        return Err(LoomError::new(Code::Conflict, "drive conflict is resolved"));
    }
    let record = conflicts.conflicts[idx].clone();
    let folder = ensure_folder_mut(&mut snapshot, &record.folder_id)?;
    if held_delete_target(&record).is_some() {
        match resolution {
            DriveConflictResolutionRequest::Current | DriveConflictResolutionRequest::Both => {
                conflicts.conflicts[idx].resolution = match resolution {
                    DriveConflictResolutionRequest::Current => DriveConflictResolution::KeepCurrent,
                    DriveConflictResolutionRequest::Both => DriveConflictResolution::KeepBoth,
                    DriveConflictResolutionRequest::Conflict => unreachable!(),
                };
            }
            DriveConflictResolutionRequest::Conflict => {
                remove_entry_by_node(folder, &record.visible_node_id)?;
                conflicts.conflicts[idx].resolution = DriveConflictResolution::KeepConflict;
                prune_completed_folder_delete(&mut snapshot, &conflicts, &record)?;
            }
        }
    } else {
        match resolution {
            DriveConflictResolutionRequest::Current => {
                remove_entry_by_node(folder, &record.conflict_node_id)?;
                conflicts.conflicts[idx].resolution = DriveConflictResolution::KeepCurrent;
            }
            DriveConflictResolutionRequest::Conflict => {
                remove_entry_by_node(folder, &record.conflict_node_id)?;
                let visible = folder
                    .entries
                    .iter_mut()
                    .find(|entry| entry.node_id == record.visible_node_id)
                    .ok_or_else(|| {
                        LoomError::not_found("drive visible conflict entry not found")
                    })?;
                visible.node_id.clone_from(&record.conflict_node_id);
                conflicts.conflicts[idx].resolution = DriveConflictResolution::KeepConflict;
            }
            DriveConflictResolutionRequest::Both => {
                conflicts.conflicts[idx].resolution = DriveConflictResolution::KeepBoth;
            }
        }
    }
    snapshot.folders =
        DriveFolderIndex::new(snapshot.workspace_id.clone(), snapshot.folders.folders)?;
    save_snapshot(loom, &snapshot)?;
    save_conflicts(loom, &conflicts)?;
    record_operation(
        loom,
        workspace,
        &snapshot,
        "conflict.resolved",
        Some(conflict_id),
        base_root,
        b"conflict.resolved",
    )
}

pub fn rename_node(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    folder_id: &str,
    node_id: &str,
    new_name: &str,
    expected_root: &str,
) -> Result<DriveWriteSummary> {
    authorize_drive_target(
        loom,
        workspace,
        workspace_id,
        "folder",
        folder_id,
        AclRight::Write,
    )?;
    let mut snapshot = load_snapshot(loom, workspace_id)?;
    let base_root = enforce_expected_root(loom, &snapshot, expected_root)?;
    let folder = ensure_folder_mut(&mut snapshot, folder_id)?;
    let entry = folder
        .entries
        .iter_mut()
        .find(|entry| entry.node_id == node_id)
        .ok_or_else(|| LoomError::not_found("drive entry not found"))?;
    *entry = DriveFolderEntry::new(new_name, node_id, entry.kind)?;
    snapshot.folders =
        DriveFolderIndex::new(snapshot.workspace_id.clone(), snapshot.folders.folders)?;
    save_snapshot(loom, &snapshot)?;
    record_operation(
        loom,
        workspace,
        &snapshot,
        "file.renamed",
        Some(node_id),
        base_root,
        b"file.renamed",
    )
}

pub fn move_node(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    source_folder_id: &str,
    target_folder_id: &str,
    node_id: &str,
    expected_root: &str,
) -> Result<DriveWriteSummary> {
    authorize_drive_target(
        loom,
        workspace,
        workspace_id,
        "folder",
        source_folder_id,
        AclRight::Write,
    )?;
    authorize_drive_target(
        loom,
        workspace,
        workspace_id,
        "folder",
        target_folder_id,
        AclRight::Write,
    )?;
    let mut snapshot = load_snapshot(loom, workspace_id)?;
    let base_root = enforce_expected_root(loom, &snapshot, expected_root)?;
    let source_idx = folder_index(&snapshot, source_folder_id)?;
    let entry_idx = snapshot.folders.folders[source_idx]
        .entries
        .iter()
        .position(|entry| entry.node_id == node_id)
        .ok_or_else(|| LoomError::not_found("drive entry not found"))?;
    let entry = snapshot.folders.folders[source_idx]
        .entries
        .remove(entry_idx);
    ensure_folder_mut(&mut snapshot, target_folder_id)?
        .entries
        .push(entry);
    snapshot.folders =
        DriveFolderIndex::new(snapshot.workspace_id.clone(), snapshot.folders.folders)?;
    save_snapshot(loom, &snapshot)?;
    record_operation(
        loom,
        workspace,
        &snapshot,
        "file.moved",
        Some(node_id),
        base_root,
        b"file.moved",
    )
}

pub fn delete_node(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    folder_id: &str,
    node_id: &str,
    expected_root: &str,
) -> Result<DriveWriteSummary> {
    authorize_drive_target(
        loom,
        workspace,
        workspace_id,
        "folder",
        folder_id,
        AclRight::Write,
    )?;
    let mut snapshot = load_snapshot(loom, workspace_id)?;
    let expected = Digest::parse(expected_root)?;
    let actual = profile_root(loom, &snapshot)?;
    let folder = ensure_folder_mut(&mut snapshot, folder_id)?;
    let idx = folder
        .entries
        .iter()
        .position(|entry| entry.node_id == node_id)
        .ok_or_else(|| LoomError::not_found("drive entry not found"))?;
    let entry = folder.entries[idx].clone();
    if actual != expected {
        let conflict_id = append_held_delete_conflicts(
            loom,
            &snapshot,
            workspace_id,
            folder_id,
            &entry,
            expected,
        )?;
        let summary = record_operation(
            loom,
            workspace,
            &snapshot,
            held_delete_operation_kind(entry.kind),
            Some(node_id),
            expected,
            b"delete.held",
        )?;
        return Ok(DriveWriteSummary {
            conflict_id,
            ..summary
        });
    }
    folder.entries.remove(idx);
    snapshot.folders =
        DriveFolderIndex::new(snapshot.workspace_id.clone(), snapshot.folders.folders)?;
    save_snapshot(loom, &snapshot)?;
    record_operation(
        loom,
        workspace,
        &snapshot,
        "file.deleted",
        Some(node_id),
        expected,
        b"file.deleted",
    )
}

fn load_snapshot(loom: &Loom<FileStore>, workspace_id: &str) -> Result<DriveProfileSnapshot> {
    match loom
        .store()
        .control_get(&drive_profile_key(workspace_id)?)?
    {
        Some(bytes) => DriveProfileSnapshot::decode(&bytes),
        None => Err(LoomError::not_found("drive snapshot not found")),
    }
}

fn load_snapshot_or_empty(
    loom: &Loom<FileStore>,
    workspace_id: &str,
) -> Result<DriveProfileSnapshot> {
    match loom
        .store()
        .control_get(&drive_profile_key(workspace_id)?)?
    {
        Some(bytes) => DriveProfileSnapshot::decode(&bytes),
        None => empty_snapshot(workspace_id),
    }
}

fn empty_snapshot(workspace_id: &str) -> Result<DriveProfileSnapshot> {
    DriveProfileSnapshot::new(
        workspace_id,
        DriveFolderIndex::new(
            workspace_id,
            vec![DriveFolderChildren::new("root", Vec::new())?],
        )?,
        DriveFileVersionIndex::new(workspace_id, Vec::new())?,
    )
}

fn save_snapshot(loom: &Loom<FileStore>, snapshot: &DriveProfileSnapshot) -> Result<()> {
    loom.store().control_set(
        &drive_profile_key(&snapshot.workspace_id)?,
        snapshot.encode()?,
    )
}

fn profile_root(loom: &Loom<FileStore>, snapshot: &DriveProfileSnapshot) -> Result<Digest> {
    Ok(Digest::hash(
        loom.store().digest_algo(),
        &snapshot.encode()?,
    ))
}

fn enforce_expected_root(
    loom: &Loom<FileStore>,
    snapshot: &DriveProfileSnapshot,
    expected_root: &str,
) -> Result<Digest> {
    enforce_expected_digest(loom, snapshot, Digest::parse(expected_root)?)
}

fn enforce_expected_digest(
    loom: &Loom<FileStore>,
    snapshot: &DriveProfileSnapshot,
    expected: Digest,
) -> Result<Digest> {
    let actual = profile_root(loom, snapshot)?;
    if actual != expected {
        return Err(LoomError::new(
            Code::Conflict,
            "drive profile root does not match expected_root",
        ));
    }
    Ok(expected)
}

fn ensure_folder<'a>(
    snapshot: &'a DriveProfileSnapshot,
    folder_id: &str,
) -> Result<&'a DriveFolderChildren> {
    snapshot
        .folders
        .children(folder_id)
        .ok_or_else(|| LoomError::not_found("drive folder not found"))
}

fn ensure_folder_mut<'a>(
    snapshot: &'a mut DriveProfileSnapshot,
    folder_id: &str,
) -> Result<&'a mut DriveFolderChildren> {
    snapshot
        .folders
        .folders
        .iter_mut()
        .find(|children| children.folder_id == folder_id)
        .ok_or_else(|| LoomError::not_found("drive folder not found"))
}

fn folder_index(snapshot: &DriveProfileSnapshot, folder_id: &str) -> Result<usize> {
    snapshot
        .folders
        .folders
        .iter()
        .position(|children| children.folder_id == folder_id)
        .ok_or_else(|| LoomError::not_found("drive folder not found"))
}

fn remove_entry_by_node(
    folder: &mut DriveFolderChildren,
    node_id: &str,
) -> Result<DriveFolderEntry> {
    let idx = folder
        .entries
        .iter()
        .position(|entry| entry.node_id == node_id)
        .ok_or_else(|| LoomError::not_found("drive entry not found"))?;
    Ok(folder.entries.remove(idx))
}

fn prune_completed_folder_delete(
    snapshot: &mut DriveProfileSnapshot,
    conflicts: &DriveConflictIndex,
    record: &DriveConflictRecord,
) -> Result<()> {
    let Some(folder_id) = held_delete_deleted_folder_id(record, snapshot) else {
        return Ok(());
    };
    let prefix = format!("delete:{folder_id}:");
    if !conflicts
        .conflicts
        .iter()
        .filter(|candidate| candidate.conflict_id.starts_with(&prefix))
        .all(|candidate| candidate.resolution == DriveConflictResolution::KeepConflict)
    {
        return Ok(());
    }
    if let Some((folder_idx, entry_idx)) = find_entry_position(snapshot, folder_id) {
        snapshot.folders.folders[folder_idx]
            .entries
            .remove(entry_idx);
    }
    Ok(())
}

fn held_delete_deleted_folder_id<'a>(
    record: &'a DriveConflictRecord,
    snapshot: &DriveProfileSnapshot,
) -> Option<&'a str> {
    let rest = record.conflict_id.strip_prefix("delete:")?;
    let (deleted_id, survivor_id) = rest.split_once(':')?;
    if deleted_id == survivor_id {
        return None;
    }
    let (_, _, entry) = find_entry(snapshot, deleted_id)?;
    (entry.kind == DriveNodeKind::Folder).then_some(deleted_id)
}

fn find_entry_position(snapshot: &DriveProfileSnapshot, node_id: &str) -> Option<(usize, usize)> {
    snapshot
        .folders
        .folders
        .iter()
        .enumerate()
        .find_map(|(folder_idx, folder)| {
            folder
                .entries
                .iter()
                .position(|entry| entry.node_id == node_id)
                .map(|entry_idx| (folder_idx, entry_idx))
        })
}

fn find_entry<'a>(
    snapshot: &'a DriveProfileSnapshot,
    node_id: &str,
) -> Option<(usize, usize, &'a DriveFolderEntry)> {
    snapshot
        .folders
        .folders
        .iter()
        .enumerate()
        .find_map(|(folder_idx, folder)| {
            folder
                .entries
                .iter()
                .enumerate()
                .find(|(_, entry)| entry.node_id == node_id)
                .map(|(entry_idx, entry)| (folder_idx, entry_idx, entry))
        })
}

fn held_delete_conflict_node_id(node_id: &str) -> String {
    format!("held-delete:{node_id}")
}

fn held_delete_target(record: &DriveConflictRecord) -> Option<&str> {
    record.conflict_node_id.strip_prefix("held-delete:")
}

fn held_delete_operation_kind(kind: DriveNodeKind) -> &'static str {
    match kind {
        DriveNodeKind::File => "file.delete_held",
        DriveNodeKind::Folder => "folder.delete_held",
        DriveNodeKind::Shortcut => "shortcut.delete_held",
    }
}

fn append_held_delete_conflicts(
    loom: &Loom<FileStore>,
    snapshot: &DriveProfileSnapshot,
    workspace_id: &str,
    folder_id: &str,
    entry: &DriveFolderEntry,
    base_root: Digest,
) -> Result<Option<String>> {
    let mut conflicts = load_conflicts(loom, workspace_id)?;
    let targets = if entry.kind == DriveNodeKind::Folder {
        let mut descendants = descendant_entries(snapshot, &entry.node_id)?;
        if descendants.is_empty() {
            descendants.push((folder_id.to_string(), entry.clone()));
        }
        descendants
    } else {
        vec![(folder_id.to_string(), entry.clone())]
    };
    let mut first_id = None;
    for (target_folder_id, target_entry) in targets {
        let conflict_id = format!("delete:{}:{}", entry.node_id, target_entry.node_id);
        if conflicts
            .conflicts
            .iter()
            .any(|record| record.conflict_id == conflict_id)
        {
            first_id.get_or_insert(conflict_id);
            continue;
        }
        conflicts.append(DriveConflictRecord::new(
            &conflict_id,
            &target_folder_id,
            &target_entry.node_id,
            held_delete_conflict_node_id(&target_entry.node_id),
            target_entry.name,
            base_root,
            DriveConflictResolution::Open,
        )?)?;
        first_id.get_or_insert(conflict_id);
    }
    save_conflicts(loom, &conflicts)?;
    Ok(first_id)
}

fn descendant_entries(
    snapshot: &DriveProfileSnapshot,
    folder_id: &str,
) -> Result<Vec<(String, DriveFolderEntry)>> {
    let Some(folder) = snapshot.folders.children(folder_id) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for entry in &folder.entries {
        out.push((folder.folder_id.clone(), entry.clone()));
        if entry.kind == DriveNodeKind::Folder {
            out.extend(descendant_entries(snapshot, &entry.node_id)?);
        }
    }
    Ok(out)
}

fn authorize_drive_collection(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    right: AclRight,
) -> Result<()> {
    authorize_drive_scope(loom, workspace, workspace_id.as_bytes(), right)
}

fn authorize_drive_target(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    target_kind: &str,
    target_id: &str,
    right: AclRight,
) -> Result<()> {
    authorize_drive_scope(
        loom,
        workspace,
        drive_acl_scope_value(workspace_id, target_kind, target_id).as_bytes(),
        right,
    )
}

fn authorize_drive_scope(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    value: &[u8],
    right: AclRight,
) -> Result<()> {
    loom.authorize_resource(
        AclResource::scoped(
            workspace,
            AclDomain::Files,
            None,
            AclResourceScope::Prefix {
                kind: AclScopeKind::Collection,
                value,
            },
        ),
        right,
    )
}

fn drive_share_acl_grant(workspace_id: &str, grant: &DriveShareGrant) -> AclGrant {
    AclGrant {
        subject: AclSubject::Principal(grant.principal),
        workspace: None,
        domain: Some(AclDomain::Files),
        ref_glob: None,
        scopes: vec![AclScope::Prefix {
            kind: AclScopeKind::Collection,
            prefix: drive_acl_scope_value(
                workspace_id,
                share_target_kind(grant.target_kind),
                &grant.target_id,
            )
            .into_bytes(),
        }],
        rights: drive_share_acl_rights(grant.role),
        effect: AclEffect::Allow,
        predicate: None,
    }
}

fn drive_acl_scope_value(workspace_id: &str, target_kind: &str, target_id: &str) -> String {
    format!("{workspace_id}/{target_kind}/{target_id}")
}

fn drive_share_acl_rights(role: DriveShareRole) -> std::collections::BTreeSet<AclRight> {
    match role {
        DriveShareRole::Viewer | DriveShareRole::Commenter | DriveShareRole::AgentReader => {
            [AclRight::Read].into_iter().collect()
        }
        DriveShareRole::Editor | DriveShareRole::AgentEditor => {
            [AclRight::Read, AclRight::Write].into_iter().collect()
        }
        DriveShareRole::Owner => [AclRight::Read, AclRight::Write, AclRight::Admin]
            .into_iter()
            .collect(),
    }
}

fn load_upload_session(
    loom: &Loom<FileStore>,
    workspace_id: &str,
    upload_id: &str,
) -> Result<DriveUploadSession> {
    match loom
        .store()
        .control_get(&drive_upload_session_key(workspace_id, upload_id)?)?
    {
        Some(bytes) => DriveUploadSession::decode(&bytes),
        None => Err(LoomError::not_found("drive upload session not found")),
    }
}

fn save_upload_session(loom: &Loom<FileStore>, session: &DriveUploadSession) -> Result<()> {
    loom.store().control_set(
        &drive_upload_session_key(&session.workspace_id, &session.upload_id)?,
        session.encode()?,
    )
}

fn delete_upload_session(
    loom: &Loom<FileStore>,
    workspace_id: &str,
    upload_id: &str,
) -> Result<bool> {
    loom.store()
        .control_delete(&drive_upload_session_key(workspace_id, upload_id)?)
}

fn load_operation_log(loom: &Loom<FileStore>, workspace_id: &str) -> Result<DriveOperationLog> {
    match loom
        .store()
        .control_get(&drive_operation_log_key(workspace_id)?)?
    {
        Some(bytes) => DriveOperationLog::decode(&bytes),
        None => DriveOperationLog::new(workspace_id, Vec::new()),
    }
}

fn append_operation(
    loom: &Loom<FileStore>,
    workspace_id: &str,
    record: &DriveOperationRecord,
) -> Result<()> {
    let mut log = load_operation_log(loom, workspace_id)?;
    log.append(record.clone())?;
    loom.store()
        .control_set(&drive_operation_log_key(workspace_id)?, log.encode()?)
}

fn update_file_revision_index(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    file_version: &DriveFileVersion,
    root_after: &str,
) -> Result<()> {
    let index_path = revision_index_path(workspace_id)?;
    let index = match loom.read_file_reserved(workspace, &index_path) {
        Ok(bytes) => RevisionIndex::decode(&bytes)?,
        Err(err) if err.code == Code::NotFound => RevisionIndex::new(),
        Err(err) => return Err(err),
    };
    let (digest, size) = drive_content_body(&file_version.content);
    let root = Digest::parse(root_after)?;
    let mut state = ProfileTransactionState::new(root, index);
    let update = ProfileRevisionUpdate::new(
        format!("drive:file:{}", file_version.file_id),
        file_version.operation_id.clone(),
        BodyRef::new(
            digest,
            size,
            "application/vnd.uldren.loom.drive.file-content",
        )?,
        file_version.timestamp_ms,
        format!("drive:{}:{}", file_version.file_id, file_version.version),
        Some(file_version.version.saturating_sub(1)),
    )?;
    state.apply(ProfileTransaction::new(
        workspace_id,
        None,
        root,
        vec![update],
    )?)?;
    let index = state.into_revision_index();
    loom.create_directory_reserved(workspace, REVISION_INDEX_DIR, true)?;
    loom.write_file_reserved(workspace, &index_path, &index.encode()?, 0o100644)
}

fn drive_content_body(content: &DriveContentRef) -> (Digest, u64) {
    match content {
        DriveContentRef::Blob { digest, size } => (*digest, *size),
        DriveContentRef::Manifest {
            content_digest,
            size,
            ..
        } => (*content_digest, *size),
    }
}

fn load_conflicts(loom: &Loom<FileStore>, workspace_id: &str) -> Result<DriveConflictIndex> {
    match loom
        .store()
        .control_get(&drive_conflict_index_key(workspace_id)?)?
    {
        Some(bytes) => DriveConflictIndex::decode(&bytes),
        None => DriveConflictIndex::new(workspace_id, Vec::new()),
    }
}

fn save_conflicts(loom: &Loom<FileStore>, conflicts: &DriveConflictIndex) -> Result<()> {
    loom.store().control_set(
        &drive_conflict_index_key(&conflicts.workspace_id)?,
        conflicts.encode()?,
    )
}

fn load_shares(loom: &Loom<FileStore>, workspace_id: &str) -> Result<DriveShareIndex> {
    match loom
        .store()
        .control_get(&drive_share_index_key(workspace_id)?)?
    {
        Some(bytes) => DriveShareIndex::decode(&bytes),
        None => DriveShareIndex::new(workspace_id, Vec::new()),
    }
}

fn save_shares(loom: &Loom<FileStore>, shares: &DriveShareIndex) -> Result<()> {
    loom.store().control_set(
        &drive_share_index_key(&shares.workspace_id)?,
        shares.encode()?,
    )
}

fn load_retention(loom: &Loom<FileStore>, workspace_id: &str) -> Result<DriveRetentionIndex> {
    match loom
        .store()
        .control_get(&drive_retention_index_key(workspace_id)?)?
    {
        Some(bytes) => DriveRetentionIndex::decode(&bytes),
        None => DriveRetentionIndex::new(workspace_id, Vec::new()),
    }
}

fn save_retention(loom: &Loom<FileStore>, retention: &DriveRetentionIndex) -> Result<()> {
    loom.store().control_set(
        &drive_retention_index_key(&retention.workspace_id)?,
        retention.encode()?,
    )
}

fn record_operation(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    snapshot: &DriveProfileSnapshot,
    operation_kind: &str,
    target_entity_id: Option<&str>,
    base_root: Digest,
    payload: &[u8],
) -> Result<DriveWriteSummary> {
    let log = load_operation_log(loom, &snapshot.workspace_id)?;
    let sequence = log.next_sequence();
    let operation_id = format!("{}:{sequence}", snapshot.workspace_id);
    let root_after = profile_root(loom, snapshot)?;
    let actor_principal = loom.effective_principal()?.unwrap_or(workspace);
    let envelope = OperationEnvelope::new(
        loom.store().digest_algo(),
        OperationEnvelopeInput {
            workspace_id: &snapshot.workspace_id,
            app_id: APP_ID,
            scope_id: &snapshot.workspace_id,
            operation_id: &operation_id,
            operation_kind,
            sequence,
            actor_principal,
            actor_kind: ActorKind::User,
            timestamp_ms: crate::now_ms(),
            idempotency_key: &operation_id,
            base_root,
            base_entity_version: None,
            target_entity_id,
            payload,
            policy_labels: &[],
            signature: None,
            agent: None,
        },
    )?;
    let record = DriveOperationRecord::new(
        sequence,
        operation_id.clone(),
        operation_kind,
        target_entity_id.map(str::to_string),
        root_after,
        envelope.encode()?,
    )?;
    append_operation(loom, &snapshot.workspace_id, &record)?;
    update_metadata_revision_index(loom, workspace, snapshot.workspace_id.as_str(), &record)?;
    Ok(DriveWriteSummary {
        workspace_id: snapshot.workspace_id.clone(),
        operation_id,
        operation_kind: operation_kind.to_string(),
        sequence,
        profile_root: root_after.to_string(),
        target_entity_id: target_entity_id.map(str::to_string),
        conflict_id: None,
    })
}

fn update_metadata_revision_index(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    record: &DriveOperationRecord,
) -> Result<()> {
    let Some(target_entity_id) = record.target_entity_id.as_deref() else {
        return Ok(());
    };
    let index_path = revision_index_path(workspace_id)?;
    let index = match loom.read_file_reserved(workspace, &index_path) {
        Ok(bytes) => RevisionIndex::decode(&bytes)?,
        Err(err) if err.code == Code::NotFound => RevisionIndex::new(),
        Err(err) => return Err(err),
    };
    let envelope = OperationEnvelope::decode(&record.envelope)?;
    let entity_id = format!("drive:metadata:{target_entity_id}");
    let expected_latest_revision = index
        .latest(&entity_id)
        .map(|entry| entry.revision)
        .unwrap_or(0);
    let mut state = ProfileTransactionState::new(record.root_after, index);
    let update = ProfileRevisionUpdate::new(
        entity_id,
        record.operation_id.clone(),
        BodyRef::new(
            Digest::hash(loom.store().digest_algo(), &record.envelope),
            record.envelope.len() as u64,
            "application/vnd.uldren.loom.drive.operation+cbor",
        )?,
        envelope.timestamp_ms,
        format!("drive:metadata:{target_entity_id}:{}", record.sequence),
        Some(expected_latest_revision),
    )?;
    state.apply(ProfileTransaction::new(
        workspace_id,
        None,
        record.root_after,
        vec![update],
    )?)?;
    let index = state.into_revision_index();
    loom.create_directory_reserved(workspace, REVISION_INDEX_DIR, true)?;
    loom.write_file_reserved(workspace, &index_path, &index.encode()?, 0o100644)
}

fn share_summary(grant: &DriveShareGrant) -> DriveShareGrantSummary {
    DriveShareGrantSummary {
        grant_id: grant.grant_id.clone(),
        target_kind: share_target_kind(grant.target_kind).to_string(),
        target_id: grant.target_id.clone(),
        principal: grant.principal.to_string(),
        role: share_role(grant.role).to_string(),
        granted_by: grant.granted_by.to_string(),
        granted_at_ms: grant.granted_at_ms,
        expires_at_ms: grant.expires_at_ms,
    }
}

fn retention_summary(pin: &DriveRetentionPin) -> DriveRetentionPinSummary {
    DriveRetentionPinSummary {
        pin_id: pin.pin_id.clone(),
        kind: retention_kind(pin.kind).to_string(),
        root: pin.root.to_string(),
        target_entity_id: pin.target_entity_id.clone(),
        added_by: pin.added_by.to_string(),
        added_at_ms: pin.added_at_ms,
        expires_at_ms: pin.expires_at_ms,
    }
}

fn conflict_summary(record: &DriveConflictRecord) -> Result<DriveConflictSummary> {
    Ok(DriveConflictSummary {
        conflict_id: record.conflict_id.clone(),
        folder_id: record.folder_id.clone(),
        visible_node_id: record.visible_node_id.clone(),
        conflict_node_id: record.conflict_node_id.clone(),
        conflict_name: record.conflict_name.clone(),
        base_root: record.base_root.to_string(),
        resolution: match record.resolution {
            DriveConflictResolution::Open => "open",
            DriveConflictResolution::KeepCurrent => "keep_current",
            DriveConflictResolution::KeepConflict => "keep_conflict",
            DriveConflictResolution::KeepBoth => "keep_both",
        }
        .to_string(),
    })
}

fn upload_content_ref(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    session: &DriveUploadSession,
) -> Result<DriveContentRef> {
    if session.chunks.is_empty() {
        return Err(LoomError::invalid("drive upload has no chunks"));
    }
    let total = session.total_size()?;
    let mut bytes = Vec::new();
    for chunk in &session.chunks {
        let chunk_bytes = cas_get(loom, workspace, &chunk.digest)?
            .ok_or_else(|| LoomError::not_found("drive upload chunk not found"))?;
        verify_size(chunk.size, chunk_bytes.len())?;
        bytes.extend(chunk_bytes);
    }
    if is_drive_dehydrated_file_marker(&bytes) {
        return Err(LoomError::invalid(
            "drive dehydrated marker bytes must not be uploaded as file content",
        ));
    }
    let content_digest = Digest::hash(loom.store().digest_algo(), &bytes);
    if total <= loom_substrate::drive::CHUNK_MIN_SIZE {
        let digest = cas_put(loom, workspace, &bytes)?;
        return Ok(DriveContentRef::Blob {
            digest,
            size: total,
        });
    }
    let manifest = DriveChunkManifest::new(
        content_digest,
        total,
        session
            .chunks
            .iter()
            .map(|chunk| DriveChunkRef::new(chunk.digest, chunk.size))
            .collect::<Result<Vec<_>>>()?,
    )?;
    let manifest_digest = cas_put(loom, workspace, &manifest.encode()?)?;
    Ok(DriveContentRef::Manifest {
        manifest_digest,
        content_digest,
        size: total,
    })
}

fn upload_summary(session: &DriveUploadSession) -> Result<DriveUploadSessionSummary> {
    Ok(DriveUploadSessionSummary {
        workspace_id: session.workspace_id.clone(),
        upload_id: session.upload_id.clone(),
        target_kind: match session.target_kind {
            DriveUploadTargetKind::NewFile => "new_file",
            DriveUploadTargetKind::ReplaceFile => "replace_file",
        }
        .to_string(),
        parent_folder_id: session.parent_folder_id.clone(),
        name: session.name.clone(),
        file_id: session.file_id.clone(),
        expected_root: session.expected_root.to_string(),
        chunk_count: session.chunks.len(),
        total_size: session.total_size()?,
    })
}

fn read_content(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    content: &DriveContentRef,
) -> Result<Vec<u8>> {
    match content {
        DriveContentRef::Blob { digest, size } => {
            let bytes = read_cas_blob(loom, workspace, digest)?;
            verify_size(*size, bytes.len())?;
            Ok(bytes)
        }
        DriveContentRef::Manifest {
            manifest_digest,
            content_digest,
            size,
            ..
        } => {
            let manifest_bytes = read_cas_blob(loom, workspace, manifest_digest)?;
            let manifest = loom_substrate::drive::DriveChunkManifest::decode(&manifest_bytes)?;
            let mut out = Vec::new();
            for chunk in manifest.chunks {
                out.extend(read_cas_blob(loom, workspace, &chunk.digest)?);
            }
            verify_size(*size, out.len())?;
            let actual = Digest::hash(loom.store().digest_algo(), &out);
            if actual != *content_digest {
                return Err(LoomError::integrity_failure(
                    "drive manifest content digest mismatch",
                ));
            }
            Ok(out)
        }
    }
}

fn read_cas_blob(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    digest: &Digest,
) -> Result<Vec<u8>> {
    cas_get(loom, workspace, digest)?.ok_or_else(|| LoomError::not_found("drive content not found"))
}

fn verify_size(expected: u64, actual: usize) -> Result<()> {
    let actual = u64::try_from(actual)
        .map_err(|_| LoomError::new(Code::InvalidArgument, "drive content is too large"))?;
    if expected != actual {
        return Err(LoomError::integrity_failure("drive content size mismatch"));
    }
    Ok(())
}

fn version_summary(version: &DriveFileVersion) -> DriveVersionSummary {
    match &version.content {
        DriveContentRef::Blob { digest, size } => DriveVersionSummary {
            file_id: version.file_id.clone(),
            version: version.version,
            operation_id: version.operation_id.clone(),
            author_principal: version.author_principal.to_string(),
            timestamp_ms: version.timestamp_ms,
            content_digest: digest.to_string(),
            manifest_digest: None,
            size: *size,
        },
        DriveContentRef::Manifest {
            manifest_digest,
            content_digest,
            size,
        } => DriveVersionSummary {
            file_id: version.file_id.clone(),
            version: version.version,
            operation_id: version.operation_id.clone(),
            author_principal: version.author_principal.to_string(),
            timestamp_ms: version.timestamp_ms,
            content_digest: content_digest.to_string(),
            manifest_digest: Some(manifest_digest.to_string()),
            size: *size,
        },
    }
}

fn node_kind(kind: DriveNodeKind) -> &'static str {
    match kind {
        DriveNodeKind::File => "file",
        DriveNodeKind::Folder => "folder",
        DriveNodeKind::Shortcut => "shortcut",
    }
}

fn parse_share_target_kind(value: &str) -> Result<DriveShareTargetKind> {
    match value {
        "file" => Ok(DriveShareTargetKind::File),
        "folder" => Ok(DriveShareTargetKind::Folder),
        "comment" => Ok(DriveShareTargetKind::Comment),
        "link" => Ok(DriveShareTargetKind::Link),
        "artifact" => Ok(DriveShareTargetKind::Artifact),
        _ => Err(LoomError::invalid(
            "drive share target_kind must be file, folder, comment, link, or artifact",
        )),
    }
}

fn share_target_kind(kind: DriveShareTargetKind) -> &'static str {
    match kind {
        DriveShareTargetKind::File => "file",
        DriveShareTargetKind::Folder => "folder",
        DriveShareTargetKind::Comment => "comment",
        DriveShareTargetKind::Link => "link",
        DriveShareTargetKind::Artifact => "artifact",
    }
}

fn parse_share_role(value: &str) -> Result<DriveShareRole> {
    match value {
        "viewer" => Ok(DriveShareRole::Viewer),
        "commenter" => Ok(DriveShareRole::Commenter),
        "editor" => Ok(DriveShareRole::Editor),
        "owner" => Ok(DriveShareRole::Owner),
        "agent_reader" => Ok(DriveShareRole::AgentReader),
        "agent_editor" => Ok(DriveShareRole::AgentEditor),
        _ => Err(LoomError::invalid(
            "drive share role must be viewer, commenter, editor, owner, agent_reader, or agent_editor",
        )),
    }
}

fn share_role(role: DriveShareRole) -> &'static str {
    match role {
        DriveShareRole::Viewer => "viewer",
        DriveShareRole::Commenter => "commenter",
        DriveShareRole::Editor => "editor",
        DriveShareRole::Owner => "owner",
        DriveShareRole::AgentReader => "agent_reader",
        DriveShareRole::AgentEditor => "agent_editor",
    }
}

fn parse_retention_kind(value: &str) -> Result<DriveRetentionPinKind> {
    match value {
        "current_root" => Ok(DriveRetentionPinKind::CurrentRoot),
        "trash_subtree" => Ok(DriveRetentionPinKind::TrashSubtree),
        "legal_hold" => Ok(DriveRetentionPinKind::LegalHold),
        "revision_retention" => Ok(DriveRetentionPinKind::RevisionRetention),
        _ => Err(LoomError::invalid(
            "drive retention kind must be current_root, trash_subtree, legal_hold, or revision_retention",
        )),
    }
}

fn retention_kind(kind: DriveRetentionPinKind) -> &'static str {
    match kind {
        DriveRetentionPinKind::CurrentRoot => "current_root",
        DriveRetentionPinKind::TrashSubtree => "trash_subtree",
        DriveRetentionPinKind::LegalHold => "legal_hold",
        DriveRetentionPinKind::RevisionRetention => "revision_retention",
    }
}

fn validate_lock_segment(name: &str, value: &str) -> Result<()> {
    if value.is_empty() || value.contains('/') || value.contains('\t') {
        return Err(LoomError::invalid(format!(
            "drive lease {name} must be non-empty and must not contain '/' or tab"
        )));
    }
    if value.chars().any(char::is_control) {
        return Err(LoomError::invalid(format!(
            "drive lease {name} must not contain control characters"
        )));
    }
    Ok(())
}
