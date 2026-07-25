use loom_core::error::{Code, LoomError, Result};
use loom_core::workspace::WorkspaceId;
use loom_core::{
    AclDomain, AclResource, AclResourceScope, AclRight, AclScopeKind, Digest, Fence, Loom, cas_get,
};
use loom_store::FileStore;
use loom_substrate::drive::{
    DriveConflictIndex, DriveConflictRecord, DriveConflictResolution, DriveContentRef,
    DriveFileVersion, DriveFileVersionIndex, DriveFolderChildren, DriveFolderIndex, DriveNodeKind,
    DriveProfileSnapshot, DriveRetentionIndex, DriveRetentionPin, DriveRetentionPinKind,
    DriveShareGrant, DriveShareIndex, DriveShareRole, DriveShareTargetKind,
    drive_conflict_index_key, drive_profile_key, drive_retention_index_key, drive_share_index_key,
};
use serde::Serialize;

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
    loom_drive::record_lease_operation(
        loom,
        workspace,
        workspace_id,
        operation_kind,
        target_kind,
        target_id,
    )
    .map(drive_write_summary)
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
    loom_drive::grant_share(
        loom,
        workspace,
        loom_drive::HostedDriveGrantShare {
            workspace_id: request.workspace_id,
            grant_id: request.grant_id,
            target_kind: request.target_kind,
            target_id: request.target_id,
            principal: request.principal,
            role: request.role,
            granted_at_ms: request.granted_at_ms,
            expires_at_ms: request.expires_at_ms,
        },
    )
    .map(drive_write_summary)
}

pub fn revoke_share(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    grant_id: &str,
) -> Result<DriveWriteSummary> {
    loom_drive::revoke_share(loom, workspace, workspace_id, grant_id).map(drive_write_summary)
}

pub fn apply_share_expiry(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    now_ms: u64,
) -> Result<DriveShareExpiryApplySummary> {
    let result = loom_drive::apply_share_expiry(loom, workspace, workspace_id, now_ms)?;
    Ok(DriveShareExpiryApplySummary {
        workspace_id: result.workspace_id,
        now_ms: result.now_ms,
        expired_grant_ids: result.expired_grant_ids,
        remaining_grants: result.remaining_grants,
        operation: result.operation.map(drive_write_summary),
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
    loom_drive::pin_retention(
        loom,
        workspace,
        loom_drive::HostedDrivePinRetention {
            workspace_id: request.workspace_id,
            pin_id: request.pin_id,
            kind: request.kind,
            root: request.root,
            target_entity_id: request.target_entity_id,
            added_at_ms: request.added_at_ms,
            expires_at_ms: request.expires_at_ms,
        },
    )
    .map(drive_write_summary)
}

pub fn unpin_retention(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    pin_id: &str,
) -> Result<DriveWriteSummary> {
    loom_drive::unpin_retention(loom, workspace, workspace_id, pin_id).map(drive_write_summary)
}

pub fn apply_retention(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    now_ms: u64,
) -> Result<DriveRetentionApplySummary> {
    let result = loom_drive::apply_retention(loom, workspace, workspace_id, now_ms)?;
    Ok(DriveRetentionApplySummary {
        workspace_id: result.workspace_id,
        now_ms: result.now_ms,
        expired_pin_ids: result.expired_pin_ids,
        remaining_pins: result.remaining_pins,
        operation: result.operation.map(drive_write_summary),
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
    loom_drive::create_folder(
        loom,
        workspace,
        workspace_id,
        parent_folder_id,
        folder_id,
        name,
        expected_root,
    )
    .map(drive_write_summary)
}

pub fn create_upload(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    request: DriveCreateUploadRequest<'_>,
) -> Result<DriveUploadSessionSummary> {
    loom_drive::create_upload(
        loom,
        workspace,
        loom_drive::HostedDriveCreateUpload {
            workspace_id: request.workspace_id,
            upload_id: request.upload_id,
            parent_folder_id: request.parent_folder_id,
            name: request.name,
            file_id: request.file_id,
            expected_root: request.expected_root,
            created_at_ms: request.created_at_ms,
            replace_file: request.replace_file,
        },
    )
    .map(drive_upload_summary)
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
    loom_drive::upload_chunk(loom, workspace, workspace_id, upload_id, bytes)
        .map(drive_upload_summary)
}

pub fn commit_upload(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    upload_id: &str,
) -> Result<DriveWriteSummary> {
    let summary = loom_drive::commit_upload(loom, workspace, workspace_id, upload_id)?;
    Ok(DriveWriteSummary {
        workspace_id: summary.workspace_id,
        operation_id: summary.operation_id,
        operation_kind: summary.operation_kind,
        sequence: summary.sequence,
        profile_root: summary.profile_root,
        target_entity_id: summary.target_entity_id,
        conflict_id: summary.conflict_id,
    })
}

pub fn resolve_conflict(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    conflict_id: &str,
    resolution: DriveConflictResolutionRequest,
) -> Result<DriveWriteSummary> {
    let resolution = match resolution {
        DriveConflictResolutionRequest::Current => {
            loom_drive::HostedDriveConflictResolution::KeepCurrent
        }
        DriveConflictResolutionRequest::Conflict => {
            loom_drive::HostedDriveConflictResolution::KeepConflict
        }
        DriveConflictResolutionRequest::Both => loom_drive::HostedDriveConflictResolution::KeepBoth,
    };
    loom_drive::resolve_conflict(loom, workspace, workspace_id, conflict_id, resolution)
        .map(drive_write_summary)
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
    loom_drive::rename_node(
        loom,
        workspace,
        workspace_id,
        folder_id,
        node_id,
        new_name,
        expected_root,
    )
    .map(drive_write_summary)
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
    loom_drive::move_node(
        loom,
        workspace,
        workspace_id,
        source_folder_id,
        target_folder_id,
        node_id,
        expected_root,
    )
    .map(drive_write_summary)
}

pub fn delete_node(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    folder_id: &str,
    node_id: &str,
    expected_root: &str,
) -> Result<DriveWriteSummary> {
    loom_drive::delete_node(
        loom,
        workspace,
        workspace_id,
        folder_id,
        node_id,
        expected_root,
    )
    .map(drive_write_summary)
}

fn drive_write_summary(summary: loom_drive::HostedDriveWrite) -> DriveWriteSummary {
    DriveWriteSummary {
        workspace_id: summary.workspace_id,
        operation_id: summary.operation_id,
        operation_kind: summary.operation_kind,
        sequence: summary.sequence,
        profile_root: summary.profile_root,
        target_entity_id: summary.target_entity_id,
        conflict_id: summary.conflict_id,
    }
}

fn drive_upload_summary(
    summary: loom_drive::HostedDriveUploadSession,
) -> DriveUploadSessionSummary {
    DriveUploadSessionSummary {
        workspace_id: summary.workspace_id,
        upload_id: summary.upload_id,
        target_kind: summary.target_kind,
        parent_folder_id: summary.parent_folder_id,
        name: summary.name,
        file_id: summary.file_id,
        expected_root: summary.expected_root,
        chunk_count: summary.chunk_count,
        total_size: summary.total_size,
    }
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

fn profile_root(loom: &Loom<FileStore>, snapshot: &DriveProfileSnapshot) -> Result<Digest> {
    Ok(Digest::hash(
        loom.store().digest_algo(),
        &snapshot.encode()?,
    ))
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

fn drive_acl_scope_value(workspace_id: &str, target_kind: &str, target_id: &str) -> String {
    format!("{workspace_id}/{target_kind}/{target_id}")
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

fn load_shares(loom: &Loom<FileStore>, workspace_id: &str) -> Result<DriveShareIndex> {
    match loom
        .store()
        .control_get(&drive_share_index_key(workspace_id)?)?
    {
        Some(bytes) => DriveShareIndex::decode(&bytes),
        None => DriveShareIndex::new(workspace_id, Vec::new()),
    }
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

fn share_target_kind(kind: DriveShareTargetKind) -> &'static str {
    match kind {
        DriveShareTargetKind::File => "file",
        DriveShareTargetKind::Folder => "folder",
        DriveShareTargetKind::Comment => "comment",
        DriveShareTargetKind::Link => "link",
        DriveShareTargetKind::Artifact => "artifact",
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
