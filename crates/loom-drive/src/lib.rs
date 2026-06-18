use loom_core::error::{Code, LoomError, Result};
use loom_core::workspace::WorkspaceId;
use loom_core::{
    AclDomain, AclEffect, AclGrant, AclResource, AclResourceScope, AclRight, AclScope,
    AclScopeKind, AclSubject, Digest, Loom, cas_get, cas_put,
};
use loom_store::FileStore;
use loom_substrate::drive::{
    APP_ID, DriveChunkManifest, DriveChunkRef, DriveConflictIndex, DriveConflictRecord,
    DriveConflictResolution, DriveContentRef, DriveDehydratedFileMarker, DriveFileVersion,
    DriveFileVersionIndex, DriveFolderChildren, DriveFolderEntry, DriveFolderIndex, DriveNodeKind,
    DriveOperationLog, DriveOperationRecord, DriveProfileSnapshot, DriveRetentionIndex,
    DriveRetentionPin, DriveRetentionPinInput, DriveRetentionPinKind, DriveShareGrant,
    DriveShareGrantInput, DriveShareIndex, DriveShareRole, DriveShareTargetKind, DriveUploadChunk,
    DriveUploadSession, DriveUploadSessionInput, DriveUploadTargetKind, conflict_copy_name,
    drive_conflict_index_key, drive_operation_log_key, drive_profile_key,
    drive_retention_index_key, drive_share_index_key, drive_upload_session_key,
    is_drive_dehydrated_file_marker,
};
use loom_substrate::versioning::{
    BodyRef, ProfileRevisionUpdate, ProfileTransaction, ProfileTransactionState,
    REVISION_INDEX_DIR, RevisionIndex, revision_index_path,
};
use loom_substrate::{ActorKind, OperationEnvelope, OperationEnvelopeInput};
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HostedDriveEntry {
    pub name: String,
    pub fold_key: String,
    pub node_id: String,
    pub kind: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HostedDriveFolder {
    pub workspace_id: String,
    pub folder_id: String,
    pub profile_root: String,
    pub entries: Vec<HostedDriveEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HostedDriveStat {
    pub workspace_id: String,
    pub node_id: String,
    pub name: String,
    pub kind: String,
    pub profile_root: String,
    pub latest_version: Option<HostedDriveVersion>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HostedDriveVersion {
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
pub struct HostedDriveUploadSession {
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
pub struct HostedDriveWrite {
    pub workspace_id: String,
    pub operation_id: String,
    pub operation_kind: String,
    pub sequence: u64,
    pub profile_root: String,
    pub target_entity_id: Option<String>,
    pub conflict_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HostedDriveConflict {
    pub conflict_id: String,
    pub folder_id: String,
    pub visible_node_id: String,
    pub conflict_node_id: String,
    pub conflict_name: String,
    pub base_root: String,
    pub resolution: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HostedDriveShareGrant {
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
pub struct HostedDriveRetentionPin {
    pub pin_id: String,
    pub kind: String,
    pub root: String,
    pub target_entity_id: Option<String>,
    pub added_by: String,
    pub added_at_ms: u64,
    pub expires_at_ms: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HostedDriveRetentionApply {
    pub workspace_id: String,
    pub now_ms: u64,
    pub expired_pin_ids: Vec<String>,
    pub remaining_pins: usize,
    pub operation: Option<HostedDriveWrite>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HostedDriveShareExpiryApply {
    pub workspace_id: String,
    pub now_ms: u64,
    pub expired_grant_ids: Vec<String>,
    pub remaining_grants: usize,
    pub operation: Option<HostedDriveWrite>,
}

pub struct HostedDriveCreateUpload<'a> {
    pub workspace_id: &'a str,
    pub upload_id: &'a str,
    pub parent_folder_id: &'a str,
    pub name: &'a str,
    pub file_id: &'a str,
    pub expected_root: &'a str,
    pub created_at_ms: u64,
    pub replace_file: bool,
}

pub struct HostedDriveGrantShare<'a> {
    pub workspace_id: &'a str,
    pub grant_id: &'a str,
    pub target_kind: &'a str,
    pub target_id: &'a str,
    pub principal: &'a str,
    pub role: &'a str,
    pub granted_at_ms: u64,
    pub expires_at_ms: Option<u64>,
}

pub struct HostedDrivePinRetention<'a> {
    pub workspace_id: &'a str,
    pub pin_id: &'a str,
    pub kind: &'a str,
    pub root: &'a str,
    pub target_entity_id: Option<&'a str>,
    pub added_at_ms: u64,
    pub expires_at_ms: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HostedDriveOsFileProjection {
    pub workspace_id: String,
    pub file_id: String,
    pub profile_root: String,
    pub state: String,
    pub size: u64,
    pub content_digest: String,
    pub uri: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HostedDriveOsMaterializedFile {
    pub file_id: String,
    pub hydrated: bool,
    pub pinned: bool,
    pub safely_replicated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HostedDriveOsWorkerPlan {
    pub workspace_id: String,
    pub profile_root: String,
    pub actions: Vec<HostedDriveOsWorkerAction>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HostedDriveOsWorkerAction {
    pub file_id: String,
    pub action: String,
    pub size: u64,
    pub content_digest: String,
    pub uri: String,
}

pub struct HostedDriveOsWrite<'a> {
    pub workspace_id: &'a str,
    pub upload_id: &'a str,
    pub parent_folder_id: &'a str,
    pub name: &'a str,
    pub file_id: &'a str,
    pub expected_root: &'a str,
    pub created_at_ms: u64,
    pub replace_file: bool,
    pub bytes: &'a [u8],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostedDriveConflictResolution {
    KeepCurrent,
    KeepConflict,
    KeepBoth,
}

pub fn list_folder(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    folder_id: &str,
) -> Result<HostedDriveFolder> {
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
    Ok(HostedDriveFolder {
        workspace_id: snapshot.workspace_id,
        folder_id: folder.folder_id.clone(),
        profile_root: profile_root.to_string(),
        entries: folder
            .entries
            .iter()
            .map(|entry| HostedDriveEntry {
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
) -> Result<HostedDriveStat> {
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
    Ok(HostedDriveStat {
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

pub fn dehydrate_file_for_os(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    file_id: &str,
) -> Result<HostedDriveOsFileProjection> {
    authorize_drive_target(
        loom,
        workspace,
        workspace_id,
        "file",
        file_id,
        AclRight::Read,
    )?;
    let snapshot = load_snapshot(loom, workspace_id)?;
    let profile_root = profile_root(loom, &snapshot)?;
    let version = snapshot
        .versions
        .latest(file_id)
        .ok_or_else(|| LoomError::not_found("drive file version not found"))?;
    let (content_digest, size) = content_digest_and_size(&version.content);
    let uri = drive_file_uri(workspace, workspace_id, file_id);
    let marker = DriveDehydratedFileMarker::new(file_id, size, content_digest, &uri)?;
    Ok(HostedDriveOsFileProjection {
        workspace_id: workspace_id.to_string(),
        file_id: file_id.to_string(),
        profile_root: profile_root.to_string(),
        state: "dehydrated".to_string(),
        size,
        content_digest: content_digest.to_string(),
        uri,
        bytes: marker.encode()?,
    })
}

pub fn hydrate_file_for_os(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    marker_bytes: &[u8],
) -> Result<HostedDriveOsFileProjection> {
    let marker = DriveDehydratedFileMarker::decode(marker_bytes)?;
    let expected_uri = drive_file_uri(workspace, workspace_id, &marker.file_id);
    if marker.uri != expected_uri {
        return Err(LoomError::invalid(
            "drive dehydrated marker does not match requested drive file",
        ));
    }
    let bytes = read_file(loom, workspace, workspace_id, &marker.file_id)?;
    verify_size(marker.size, bytes.len())?;
    let actual = Digest::hash(loom.store().digest_algo(), &bytes);
    if actual != marker.content_digest {
        return Err(LoomError::integrity_failure(
            "drive dehydrated marker content digest mismatch",
        ));
    }
    let snapshot = load_snapshot(loom, workspace_id)?;
    Ok(HostedDriveOsFileProjection {
        workspace_id: workspace_id.to_string(),
        file_id: marker.file_id,
        profile_root: profile_root(loom, &snapshot)?.to_string(),
        state: "hydrated".to_string(),
        size: marker.size,
        content_digest: actual.to_string(),
        uri: marker.uri,
        bytes,
    })
}

pub fn plan_os_projection_worker(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    materialized: &[HostedDriveOsMaterializedFile],
) -> Result<HostedDriveOsWorkerPlan> {
    authorize_drive_target(
        loom,
        workspace,
        workspace_id,
        "drive",
        workspace_id,
        AclRight::Read,
    )?;
    let snapshot = load_snapshot(loom, workspace_id)?;
    let profile_root = profile_root(loom, &snapshot)?;
    let mut actions = Vec::new();
    for version in latest_versions(&snapshot.versions) {
        let state = materialized
            .iter()
            .find(|file| file.file_id == version.file_id);
        let (content_digest, size) = content_digest_and_size(&version.content);
        let action = match state {
            Some(file) if file.pinned && !file.hydrated => "hydrate",
            Some(file) if !file.pinned && file.hydrated && file.safely_replicated => "evict",
            Some(_) => "keep",
            None => "skip",
        };
        actions.push(HostedDriveOsWorkerAction {
            file_id: version.file_id.clone(),
            action: action.to_string(),
            size,
            content_digest: content_digest.to_string(),
            uri: drive_file_uri(workspace, workspace_id, &version.file_id),
        });
    }
    Ok(HostedDriveOsWorkerPlan {
        workspace_id: workspace_id.to_string(),
        profile_root: profile_root.to_string(),
        actions,
    })
}

pub fn write_file_from_os(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    request: HostedDriveOsWrite<'_>,
) -> Result<HostedDriveWrite> {
    if is_drive_dehydrated_file_marker(request.bytes) {
        return Err(LoomError::invalid(
            "drive dehydrated marker bytes must not be uploaded as file content",
        ));
    }
    create_upload(
        loom,
        workspace,
        HostedDriveCreateUpload {
            workspace_id: request.workspace_id,
            upload_id: request.upload_id,
            parent_folder_id: request.parent_folder_id,
            name: request.name,
            file_id: request.file_id,
            expected_root: request.expected_root,
            created_at_ms: request.created_at_ms,
            replace_file: request.replace_file,
        },
    )?;
    upload_chunk(
        loom,
        workspace,
        request.workspace_id,
        request.upload_id,
        request.bytes,
    )?;
    commit_upload(loom, workspace, request.workspace_id, request.upload_id)
}

pub fn list_versions(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    file_id: &str,
) -> Result<Vec<HostedDriveVersion>> {
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
) -> Result<Vec<HostedDriveConflict>> {
    authorize_drive_collection(loom, workspace, workspace_id, AclRight::Read)?;
    load_conflicts(loom, workspace_id)?
        .conflicts
        .iter()
        .map(conflict_summary)
        .collect()
}

pub fn list_shares(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
) -> Result<Vec<HostedDriveShareGrant>> {
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
    request: HostedDriveGrantShare<'_>,
) -> Result<HostedDriveWrite> {
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
) -> Result<HostedDriveWrite> {
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
) -> Result<HostedDriveShareExpiryApply> {
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
    Ok(HostedDriveShareExpiryApply {
        workspace_id: workspace_id.to_string(),
        now_ms,
        expired_grant_ids,
        remaining_grants: retained.len(),
        operation,
    })
}

pub fn list_retention(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
) -> Result<Vec<HostedDriveRetentionPin>> {
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
    request: HostedDrivePinRetention<'_>,
) -> Result<HostedDriveWrite> {
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
) -> Result<HostedDriveWrite> {
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
) -> Result<HostedDriveRetentionApply> {
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
    Ok(HostedDriveRetentionApply {
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
) -> Result<HostedDriveWrite> {
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
    request: HostedDriveCreateUpload<'_>,
) -> Result<HostedDriveUploadSession> {
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

pub fn upload_chunk(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    upload_id: &str,
    bytes: &[u8],
) -> Result<HostedDriveUploadSession> {
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
) -> Result<HostedDriveWrite> {
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
        loom_chat::now_ms(),
        content,
    )?;
    snapshot.versions.versions.push(file_version.clone());
    snapshot.versions =
        DriveFileVersionIndex::new(snapshot.workspace_id.clone(), snapshot.versions.versions)?;
    if session.target_kind == DriveUploadTargetKind::NewFile {
        let parent = ensure_folder_mut(&mut snapshot, &session.parent_folder_id)?;
        if let Some(existing) = parent.entry_by_name(&session.name)?.cloned() {
            let conflict_name =
                conflict_copy_name(&session.name, "principal", loom_chat::now_ms(), 1)?;
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
    Ok(summary)
}

pub fn resolve_conflict(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    conflict_id: &str,
    resolution: HostedDriveConflictResolution,
) -> Result<HostedDriveWrite> {
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
            HostedDriveConflictResolution::KeepCurrent
            | HostedDriveConflictResolution::KeepBoth => {
                conflicts.conflicts[idx].resolution = match resolution {
                    HostedDriveConflictResolution::KeepCurrent => {
                        DriveConflictResolution::KeepCurrent
                    }
                    HostedDriveConflictResolution::KeepBoth => DriveConflictResolution::KeepBoth,
                    HostedDriveConflictResolution::KeepConflict => unreachable!(),
                };
            }
            HostedDriveConflictResolution::KeepConflict => {
                remove_entry_by_node(folder, &record.visible_node_id)?;
                conflicts.conflicts[idx].resolution = DriveConflictResolution::KeepConflict;
                prune_completed_folder_delete(&mut snapshot, &conflicts, &record)?;
            }
        }
    } else {
        match resolution {
            HostedDriveConflictResolution::KeepCurrent => {
                remove_entry_by_node(folder, &record.conflict_node_id)?;
                conflicts.conflicts[idx].resolution = DriveConflictResolution::KeepCurrent;
            }
            HostedDriveConflictResolution::KeepConflict => {
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
            HostedDriveConflictResolution::KeepBoth => {
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
) -> Result<HostedDriveWrite> {
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
) -> Result<HostedDriveWrite> {
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
) -> Result<HostedDriveWrite> {
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
        return Ok(HostedDriveWrite {
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
        .ok_or_else(|| LoomError::not_found("drive conflict entry not found"))?;
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
    record: DriveOperationRecord,
) -> Result<()> {
    let mut log = load_operation_log(loom, workspace_id)?;
    log.append(record)?;
    loom.store()
        .control_set(&drive_operation_log_key(workspace_id)?, log.encode()?)
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
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    snapshot: &DriveProfileSnapshot,
    operation_kind: &str,
    target_entity_id: Option<&str>,
    base_root: Digest,
    payload: &[u8],
) -> Result<HostedDriveWrite> {
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
            timestamp_ms: loom_chat::now_ms(),
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
    append_operation(loom, &snapshot.workspace_id, record)?;
    Ok(HostedDriveWrite {
        workspace_id: snapshot.workspace_id.clone(),
        operation_id,
        operation_kind: operation_kind.to_string(),
        sequence,
        profile_root: root_after.to_string(),
        target_entity_id: target_entity_id.map(str::to_string),
        conflict_id: None,
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

fn content_digest_and_size(content: &DriveContentRef) -> (Digest, u64) {
    match content {
        DriveContentRef::Blob { digest, size } => (*digest, *size),
        DriveContentRef::Manifest {
            content_digest,
            size,
            ..
        } => (*content_digest, *size),
    }
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
    let (digest, size) = content_digest_and_size(&file_version.content);
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

fn latest_versions(index: &DriveFileVersionIndex) -> Vec<&DriveFileVersion> {
    let mut file_ids = index
        .versions
        .iter()
        .map(|version| version.file_id.as_str())
        .collect::<Vec<_>>();
    file_ids.sort_unstable();
    file_ids.dedup();
    file_ids
        .into_iter()
        .filter_map(|file_id| index.latest(file_id))
        .collect()
}

fn drive_file_uri(workspace: WorkspaceId, _workspace_id: &str, file_id: &str) -> String {
    format!("loom://{workspace}/drive/files/{file_id}")
}

fn verify_size(expected: u64, actual: usize) -> Result<()> {
    let actual = u64::try_from(actual)
        .map_err(|_| LoomError::new(Code::InvalidArgument, "drive content is too large"))?;
    if expected != actual {
        return Err(LoomError::integrity_failure("drive content size mismatch"));
    }
    Ok(())
}

fn upload_summary(session: &DriveUploadSession) -> Result<HostedDriveUploadSession> {
    Ok(HostedDriveUploadSession {
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

fn conflict_summary(record: &DriveConflictRecord) -> Result<HostedDriveConflict> {
    Ok(HostedDriveConflict {
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

fn share_summary(grant: &DriveShareGrant) -> HostedDriveShareGrant {
    HostedDriveShareGrant {
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

fn retention_summary(pin: &DriveRetentionPin) -> HostedDriveRetentionPin {
    HostedDriveRetentionPin {
        pin_id: pin.pin_id.clone(),
        kind: retention_kind(pin.kind).to_string(),
        root: pin.root.to_string(),
        target_entity_id: pin.target_entity_id.clone(),
        added_by: pin.added_by.to_string(),
        added_at_ms: pin.added_at_ms,
        expires_at_ms: pin.expires_at_ms,
    }
}

fn version_summary(version: &DriveFileVersion) -> HostedDriveVersion {
    match &version.content {
        DriveContentRef::Blob { digest, size } => HostedDriveVersion {
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
        } => HostedDriveVersion {
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

#[cfg(test)]
mod tests {
    use super::*;
    use loom_core::{Algo, Loom};
    use loom_store::FileStore;
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn workspace() -> WorkspaceId {
        WorkspaceId::v4_from_bytes([7; 16])
    }

    fn loom_path(test_name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "uldren-loom-drive-{test_name}-{}-{nanos}.loom",
            std::process::id()
        ))
    }

    fn open_test_loom(test_name: &str) -> (PathBuf, Loom<FileStore>) {
        let path = loom_path(test_name);
        let store = FileStore::create_with_profile(&path, Algo::Blake3).unwrap();
        (path, Loom::new(store))
    }

    fn current_root(loom: &Loom<FileStore>, workspace_id: &str) -> String {
        list_folder(loom, workspace(), workspace_id, "root")
            .unwrap()
            .profile_root
    }

    fn upload_file(
        loom: &mut Loom<FileStore>,
        workspace_id: &str,
        upload_id: &str,
        file_id: &str,
        name: &str,
        bytes: &[u8],
    ) {
        let expected_root = current_root(loom, workspace_id);
        create_upload(
            loom,
            workspace(),
            HostedDriveCreateUpload {
                workspace_id,
                upload_id,
                parent_folder_id: "root",
                name,
                file_id,
                expected_root: &expected_root,
                created_at_ms: 1,
                replace_file: false,
            },
        )
        .unwrap();
        upload_chunk(loom, workspace(), workspace_id, upload_id, bytes).unwrap();
        commit_upload(loom, workspace(), workspace_id, upload_id).unwrap();
    }

    #[test]
    fn os_projection_marker_round_trips_and_rejects_marker_uploads() {
        let (path, mut loom) = open_test_loom("os-projection");
        let workspace_id = "drive";
        upload_file(
            &mut loom,
            workspace_id,
            "upload-one",
            "file-one",
            "one.txt",
            b"drive content",
        );

        let dehydrated =
            dehydrate_file_for_os(&loom, workspace(), workspace_id, "file-one").unwrap();
        assert_eq!(dehydrated.state, "dehydrated");
        assert_eq!(dehydrated.file_id, "file-one");
        assert!(is_drive_dehydrated_file_marker(&dehydrated.bytes));

        let hydrated =
            hydrate_file_for_os(&loom, workspace(), workspace_id, &dehydrated.bytes).unwrap();
        assert_eq!(hydrated.state, "hydrated");
        assert_eq!(hydrated.bytes, b"drive content");
        assert_eq!(hydrated.content_digest, dehydrated.content_digest);

        let expected_root = current_root(&loom, workspace_id);
        let err = write_file_from_os(
            &mut loom,
            workspace(),
            HostedDriveOsWrite {
                workspace_id,
                upload_id: "upload-marker",
                parent_folder_id: "root",
                name: "marker.txt",
                file_id: "file-marker",
                expected_root: &expected_root,
                created_at_ms: 2,
                replace_file: false,
                bytes: &dehydrated.bytes,
            },
        )
        .unwrap_err();
        assert_eq!(err.code, Code::InvalidArgument);

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn os_projection_worker_plans_hydrate_evict_keep_and_skip() {
        let (path, mut loom) = open_test_loom("os-worker-plan");
        let workspace_id = "drive";
        upload_file(
            &mut loom,
            workspace_id,
            "upload-hydrate",
            "file-hydrate",
            "hydrate.txt",
            b"hydrate",
        );
        upload_file(
            &mut loom,
            workspace_id,
            "upload-evict",
            "file-evict",
            "evict.txt",
            b"evict",
        );
        upload_file(
            &mut loom,
            workspace_id,
            "upload-keep",
            "file-keep",
            "keep.txt",
            b"keep",
        );
        upload_file(
            &mut loom,
            workspace_id,
            "upload-skip",
            "file-skip",
            "skip.txt",
            b"skip",
        );

        let plan = plan_os_projection_worker(
            &loom,
            workspace(),
            workspace_id,
            &[
                HostedDriveOsMaterializedFile {
                    file_id: "file-hydrate".to_string(),
                    hydrated: false,
                    pinned: true,
                    safely_replicated: false,
                },
                HostedDriveOsMaterializedFile {
                    file_id: "file-evict".to_string(),
                    hydrated: true,
                    pinned: false,
                    safely_replicated: true,
                },
                HostedDriveOsMaterializedFile {
                    file_id: "file-keep".to_string(),
                    hydrated: true,
                    pinned: true,
                    safely_replicated: true,
                },
            ],
        )
        .unwrap();

        let actions = plan
            .actions
            .iter()
            .map(|action| (action.file_id.as_str(), action.action.as_str()))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(actions.get("file-hydrate"), Some(&"hydrate"));
        assert_eq!(actions.get("file-evict"), Some(&"evict"));
        assert_eq!(actions.get("file-keep"), Some(&"keep"));
        assert_eq!(actions.get("file-skip"), Some(&"skip"));

        let _ = std::fs::remove_file(path);
    }
}
