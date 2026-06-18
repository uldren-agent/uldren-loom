use loom_drive::{
    HostedDriveConflictResolution, HostedDriveCreateUpload, HostedDriveGrantShare,
    HostedDrivePinRetention,
};
use serde::Serialize;
use wasm_bindgen::prelude::*;

use super::{LoomStore, le, resolve_workspace_arg, save_loom};

fn to_json<T: Serialize>(value: loom_core::Result<T>) -> Result<String, JsError> {
    let value = value.map_err(le)?;
    serde_json::to_string(&value).map_err(|error| JsError::new(&error.to_string()))
}

fn parse_resolution(value: &str) -> Result<HostedDriveConflictResolution, JsError> {
    match value {
        "keep_current" => Ok(HostedDriveConflictResolution::KeepCurrent),
        "keep_conflict" => Ok(HostedDriveConflictResolution::KeepConflict),
        "keep_both" => Ok(HostedDriveConflictResolution::KeepBoth),
        _ => Err(JsError::new("invalid drive conflict resolution")),
    }
}

#[wasm_bindgen]
impl LoomStore {
    pub fn drive_list_json(
        &self,
        workspace: String,
        drive_workspace_id: String,
        folder_id: String,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        to_json(loom_drive::list_folder(
            &self.loom,
            ns,
            &drive_workspace_id,
            &folder_id,
        ))
    }

    pub fn drive_stat_json(
        &self,
        workspace: String,
        drive_workspace_id: String,
        folder_id: String,
        name: String,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        to_json(loom_drive::stat_node(
            &self.loom,
            ns,
            &drive_workspace_id,
            &folder_id,
            &name,
        ))
    }

    pub fn drive_read_file(
        &self,
        workspace: String,
        drive_workspace_id: String,
        file_id: String,
    ) -> Result<Vec<u8>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        loom_drive::read_file(&self.loom, ns, &drive_workspace_id, &file_id).map_err(le)
    }

    pub fn drive_list_versions_json(
        &self,
        workspace: String,
        drive_workspace_id: String,
        file_id: String,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        to_json(loom_drive::list_versions(
            &self.loom,
            ns,
            &drive_workspace_id,
            &file_id,
        ))
    }

    pub fn drive_list_conflicts_json(
        &self,
        workspace: String,
        drive_workspace_id: String,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        to_json(loom_drive::list_conflicts(
            &self.loom,
            ns,
            &drive_workspace_id,
        ))
    }

    pub fn drive_list_shares_json(
        &self,
        workspace: String,
        drive_workspace_id: String,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        to_json(loom_drive::list_shares(
            &self.loom,
            ns,
            &drive_workspace_id,
        ))
    }

    pub fn drive_list_retention_json(
        &self,
        workspace: String,
        drive_workspace_id: String,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        to_json(loom_drive::list_retention(
            &self.loom,
            ns,
            &drive_workspace_id,
        ))
    }

    pub fn drive_create_folder_json(
        &mut self,
        workspace: String,
        drive_workspace_id: String,
        parent_folder_id: String,
        folder_id: String,
        name: String,
        expected_root: String,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let out = to_json(loom_drive::create_folder(
            &mut self.loom,
            ns,
            &drive_workspace_id,
            &parent_folder_id,
            &folder_id,
            &name,
            &expected_root,
        ))?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(out)
    }

    pub fn drive_create_upload_json(
        &mut self,
        workspace: String,
        drive_workspace_id: String,
        upload_id: String,
        parent_folder_id: String,
        name: String,
        file_id: String,
        expected_root: String,
        created_at_ms: u64,
        replace_file: bool,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let out = to_json(loom_drive::create_upload(
            &mut self.loom,
            ns,
            HostedDriveCreateUpload {
                workspace_id: &drive_workspace_id,
                upload_id: &upload_id,
                parent_folder_id: &parent_folder_id,
                name: &name,
                file_id: &file_id,
                expected_root: &expected_root,
                created_at_ms,
                replace_file,
            },
        ))?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(out)
    }

    pub fn drive_upload_chunk_json(
        &mut self,
        workspace: String,
        drive_workspace_id: String,
        upload_id: String,
        chunk: Vec<u8>,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let out = to_json(loom_drive::upload_chunk(
            &mut self.loom,
            ns,
            &drive_workspace_id,
            &upload_id,
            &chunk,
        ))?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(out)
    }

    pub fn drive_commit_upload_json(
        &mut self,
        workspace: String,
        drive_workspace_id: String,
        upload_id: String,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let out = to_json(loom_drive::commit_upload(
            &mut self.loom,
            ns,
            &drive_workspace_id,
            &upload_id,
        ))?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(out)
    }

    pub fn drive_rename_json(
        &mut self,
        workspace: String,
        drive_workspace_id: String,
        folder_id: String,
        node_id: String,
        new_name: String,
        expected_root: String,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let out = to_json(loom_drive::rename_node(
            &mut self.loom,
            ns,
            &drive_workspace_id,
            &folder_id,
            &node_id,
            &new_name,
            &expected_root,
        ))?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(out)
    }

    pub fn drive_move_json(
        &mut self,
        workspace: String,
        drive_workspace_id: String,
        source_folder_id: String,
        target_folder_id: String,
        node_id: String,
        expected_root: String,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let out = to_json(loom_drive::move_node(
            &mut self.loom,
            ns,
            &drive_workspace_id,
            &source_folder_id,
            &target_folder_id,
            &node_id,
            &expected_root,
        ))?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(out)
    }

    pub fn drive_delete_json(
        &mut self,
        workspace: String,
        drive_workspace_id: String,
        folder_id: String,
        node_id: String,
        expected_root: String,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let out = to_json(loom_drive::delete_node(
            &mut self.loom,
            ns,
            &drive_workspace_id,
            &folder_id,
            &node_id,
            &expected_root,
        ))?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(out)
    }

    pub fn drive_resolve_conflict_json(
        &mut self,
        workspace: String,
        drive_workspace_id: String,
        conflict_id: String,
        resolution: String,
    ) -> Result<String, JsError> {
        let resolution = parse_resolution(&resolution)?;
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let out = to_json(loom_drive::resolve_conflict(
            &mut self.loom,
            ns,
            &drive_workspace_id,
            &conflict_id,
            resolution,
        ))?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(out)
    }

    pub fn drive_grant_share_json(
        &mut self,
        workspace: String,
        drive_workspace_id: String,
        grant_id: String,
        target_kind: String,
        target_id: String,
        principal: String,
        role: String,
        granted_at_ms: u64,
        expires_at_ms: Option<u64>,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let out = to_json(loom_drive::grant_share(
            &mut self.loom,
            ns,
            HostedDriveGrantShare {
                workspace_id: &drive_workspace_id,
                grant_id: &grant_id,
                target_kind: &target_kind,
                target_id: &target_id,
                principal: &principal,
                role: &role,
                granted_at_ms,
                expires_at_ms,
            },
        ))?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(out)
    }

    pub fn drive_revoke_share_json(
        &mut self,
        workspace: String,
        drive_workspace_id: String,
        grant_id: String,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let out = to_json(loom_drive::revoke_share(
            &mut self.loom,
            ns,
            &drive_workspace_id,
            &grant_id,
        ))?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(out)
    }

    pub fn drive_apply_share_expiry_json(
        &mut self,
        workspace: String,
        drive_workspace_id: String,
        now_ms: u64,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let out = to_json(loom_drive::apply_share_expiry(
            &mut self.loom,
            ns,
            &drive_workspace_id,
            now_ms,
        ))?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(out)
    }

    pub fn drive_pin_retention_json(
        &mut self,
        workspace: String,
        drive_workspace_id: String,
        pin_id: String,
        kind: String,
        root: String,
        target_entity_id: Option<String>,
        added_at_ms: u64,
        expires_at_ms: Option<u64>,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let out = to_json(loom_drive::pin_retention(
            &mut self.loom,
            ns,
            HostedDrivePinRetention {
                workspace_id: &drive_workspace_id,
                pin_id: &pin_id,
                kind: &kind,
                root: &root,
                target_entity_id: target_entity_id.as_deref(),
                added_at_ms,
                expires_at_ms,
            },
        ))?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(out)
    }

    pub fn drive_unpin_retention_json(
        &mut self,
        workspace: String,
        drive_workspace_id: String,
        pin_id: String,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let out = to_json(loom_drive::unpin_retention(
            &mut self.loom,
            ns,
            &drive_workspace_id,
            &pin_id,
        ))?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(out)
    }

    pub fn drive_apply_retention_json(
        &mut self,
        workspace: String,
        drive_workspace_id: String,
        now_ms: u64,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let out = to_json(loom_drive::apply_retention(
            &mut self.loom,
            ns,
            &drive_workspace_id,
            now_ms,
        ))?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(out)
    }
}
