use std::time::Duration;

use loom_core::{Digest, WorkspaceId};

use crate::chat::{
    HostedChatChannel, HostedChatCursor, HostedChatEmojiRegistry, HostedChatPresence,
    HostedChatWrite,
};
use crate::drive::{
    HostedDriveConflict, HostedDriveConflictResolution, HostedDriveCreateUpload, HostedDriveFolder,
    HostedDriveGrantShare, HostedDriveOsFileProjection, HostedDriveOsMaterializedFile,
    HostedDriveOsWorkerPlan, HostedDriveOsWrite, HostedDrivePinRetention,
    HostedDriveRetentionApply, HostedDriveRetentionPin, HostedDriveShareExpiryApply,
    HostedDriveShareGrant, HostedDriveStat, HostedDriveUploadSession, HostedDriveVersion,
    HostedDriveWrite,
};
use crate::lanes::{HostedLaneCreate, HostedLaneTicketUpdate, HostedLaneUpdate};
use crate::meetings::{
    HostedMeetingDetail, HostedMeetingsAnnotationReview, HostedMeetingsEntityMergeWrite,
    HostedMeetingsExtractionReview, HostedMeetingsList, HostedMeetingsMaterializedOutputs,
    HostedMeetingsProjection, HostedMeetingsProjectionApply, HostedMeetingsSearch,
    HostedMeetingsVocabularyReview,
};
use crate::tickets::{
    HostedTicketCommentAdd, HostedTicketCommentDelete, HostedTicketCommentUpdate,
    HostedTicketCreate, HostedTicketDelete, HostedTicketProjectWrite, HostedTicketRelationRemove,
    HostedTicketRelationWrite, HostedTicketUpdate,
};
use crate::watch::{
    HostedWatchBatch, HostedWatchMaterialization, HostedWatchMaterializeInput,
    HostedWatchStreamFrame, HostedWatchSubscribeInput, HostedWatchSubscription, watch_stream_frame,
};
use crate::{HostedAuth, HostedError, HostedKernel};
use crate::{HostedReferenceReconciliationStatus, HostedSubstrateChangesBatch};
use loom_lanes::{Lane, PublicLane};
use loom_lifecycle::{
    LifecycleDefinitionSummary, LifecycleInstanceSummary, LifecycleOperationLogSummary,
    LifecycleSnapshotPlanSummary, LifecycleSnapshotRecordSummary, LifecycleStageSurfaceSummary,
    LifecycleTransitionRequest, LifecycleTransitionResult, StandardLifecycleRequest,
};
use loom_pages::{
    PageCreateRequest, PageHistoryEntry, PagePublishSummary, PageSummary, PageUpdateSummary,
    SpaceSummary, StructureBindRequest, StructureCreateRequest, StructureDecomposeRequest,
    StructureDecomposeSummary, StructureEdgeSummary, StructureLinkRequest, StructureMoveRequest,
    StructureMoveSummary, StructureNodeRequest, StructureNodeSummary, StructureRenderSummary,
};
use loom_tickets::{
    TicketComment, TicketHistoryRecord, TicketProjectSummary, TicketRelationSummary, TicketSummary,
};
use loom_types::{MutationChange, MutationEnvelope, MutationReceipt};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonRpcResponse<T> {
    pub result: T,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    pub data: JsonRpcErrorData,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonRpcErrorData {
    pub loom_code: &'static str,
    pub loom_code_number: i32,
}

pub type JsonRpcResult<T> = std::result::Result<JsonRpcResponse<T>, JsonRpcError>;

fn public_lane_response(response: JsonRpcResponse<Lane>) -> JsonRpcResponse<PublicLane> {
    JsonRpcResponse {
        result: loom_lanes::public_lane(&response.result),
    }
}

fn jsonrpc_ticket_response(
    response: JsonRpcResponse<TicketSummary>,
    operation: &str,
    root_before: Option<String>,
    changes: Vec<MutationChange>,
) -> JsonRpcResponse<MutationEnvelope<TicketSummary>> {
    let receipt = MutationReceipt::new(operation, "ticket", response.result.primary_key.clone())
        .operation_id(response.result.operation_id.clone())
        .roots(root_before, Some(response.result.profile_root.clone()))
        .changes(changes);
    JsonRpcResponse {
        result: MutationEnvelope::new(response.result, receipt),
    }
}

fn jsonrpc_relation_response(
    response: JsonRpcResponse<TicketRelationSummary>,
    operation: &str,
    root_before: Option<String>,
    changes: Vec<MutationChange>,
) -> JsonRpcResponse<MutationEnvelope<TicketRelationSummary>> {
    let receipt = MutationReceipt::new(
        operation,
        "ticket_relation",
        response.result.relation_id.clone(),
    )
    .operation_id(Some(response.result.operation_id.clone()))
    .roots(root_before, Some(response.result.profile_root.clone()))
    .changes(changes);
    JsonRpcResponse {
        result: MutationEnvelope::new(response.result, receipt),
    }
}

fn jsonrpc_ticket_field_value_changes(fields: &serde_json::Value) -> Vec<MutationChange> {
    fields.as_object().map_or_else(Vec::new, |fields| {
        fields
            .iter()
            .map(|(field, value)| MutationChange::field_set(field.clone(), value.to_string()))
            .collect()
    })
}

fn jsonrpc_ticket_update_changes(input: &HostedTicketUpdate<'_>) -> Vec<MutationChange> {
    let mut changes = input
        .set_fields
        .map(jsonrpc_ticket_field_value_changes)
        .unwrap_or_default();
    changes.extend(
        input
            .delete_fields
            .iter()
            .map(|field| MutationChange::field_deleted(field.clone(), None::<String>)),
    );
    if let Some(target_status) = input.target_status {
        changes.push(MutationChange::field_changed(
            "status",
            input.observed_source_status.map(str::to_string),
            Some(target_status.to_string()),
        ));
    }
    if let Some(assignee) = input.assignee {
        changes.push(MutationChange::field_changed(
            "assignee",
            None::<String>,
            Some(assignee.to_string()),
        ));
    }
    if input.action.is_some() && input.target_status.is_none() {
        changes.push(MutationChange::field_set("lifecycle_action", "applied"));
    }
    if let Some(comment) = input.comment.as_ref() {
        changes.push(MutationChange::field_set(
            "comment",
            comment.comment_type.unwrap_or("general"),
        ));
    }
    changes.extend(input.comments.iter().map(|comment| {
        MutationChange::field_set("comment", comment.comment_type.unwrap_or("general"))
    }));
    changes.extend(input.relation_sets.iter().map(|relation| {
        MutationChange::relation_set(
            relation
                .relation_id
                .map(str::to_string)
                .unwrap_or_else(|| "default".to_string()),
            relation.kind.as_str().to_string(),
            relation.target_id.to_string(),
        )
    }));
    changes.extend(input.relation_removes.iter().map(|relation| {
        MutationChange::field_deleted(format!("relation:{}", relation.relation_id), None::<String>)
    }));
    changes
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonRpcWatchStreamInput<'a> {
    pub workspace: WorkspaceId,
    pub cursor: &'a str,
    pub max: u32,
    pub interval_ms: u64,
    pub debounce_ms: u64,
    pub limit: Option<u64>,
}

pub struct JsonRpcAdapter<'a> {
    kernel: &'a HostedKernel,
}

impl HostedKernel {
    pub fn jsonrpc(&self) -> JsonRpcAdapter<'_> {
        JsonRpcAdapter { kernel: self }
    }
}

impl JsonRpcAdapter<'_> {
    pub fn cas_put(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        bytes: &[u8],
    ) -> JsonRpcResult<String> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            loom_core::cas_put(loom, workspace, bytes).map(|digest| digest.to_string())
        }))
    }

    pub fn cas_get(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        digest: &Digest,
    ) -> JsonRpcResult<Option<Vec<u8>>> {
        jsonrpc_result(
            self.kernel
                .read(auth, |loom| loom_core::cas_get(loom, workspace, digest)),
        )
    }

    pub fn cas_has(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        digest: &Digest,
    ) -> JsonRpcResult<bool> {
        jsonrpc_result(
            self.kernel
                .read(auth, |loom| loom_core::cas_has(loom, workspace, digest)),
        )
    }

    pub fn cas_list(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
    ) -> JsonRpcResult<Vec<String>> {
        jsonrpc_result(self.kernel.read(auth, |loom| {
            loom_core::cas_list(loom, workspace).map(|digests| {
                digests
                    .into_iter()
                    .map(|digest| digest.to_string())
                    .collect()
            })
        }))
    }

    pub fn fs_import(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        src_path: &str,
        commit: bool,
        dry_run: bool,
    ) -> JsonRpcResult<Vec<u8>> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            crate::archive::fs_import(loom, workspace, src_path, commit, dry_run)
        }))
    }

    pub fn fs_export(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        dst_path: &str,
        revision: Option<&str>,
        dry_run: bool,
    ) -> JsonRpcResult<Vec<u8>> {
        jsonrpc_result(self.kernel.read(auth, |loom| {
            crate::archive::fs_export(loom, workspace, dst_path, revision, dry_run)
        }))
    }

    pub fn archive_import(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        src_path: &str,
        kind: &str,
        dry_run: bool,
    ) -> JsonRpcResult<Vec<u8>> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            crate::archive::archive_import(loom, workspace, src_path, kind, dry_run)
        }))
    }

    pub fn archive_export(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        dst_path: &str,
        kind: &str,
        revision: Option<&str>,
        dry_run: bool,
    ) -> JsonRpcResult<Vec<u8>> {
        jsonrpc_result(self.kernel.read(auth, |loom| {
            crate::archive::archive_export(loom, workspace, dst_path, kind, revision, dry_run)
        }))
    }

    pub fn car_import(
        &self,
        auth: &HostedAuth,
        src_path: &str,
        dry_run: bool,
    ) -> JsonRpcResult<Vec<u8>> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            crate::archive::car_import(loom, src_path, dry_run)
        }))
    }

    pub fn car_export(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        dst_path: &str,
        dry_run: bool,
    ) -> JsonRpcResult<Vec<u8>> {
        jsonrpc_result(self.kernel.read(auth, |loom| {
            crate::archive::car_export(loom, workspace, dst_path, dry_run)
        }))
    }

    pub fn tickets_project_create(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        input: HostedTicketProjectWrite<'_>,
    ) -> JsonRpcResult<TicketProjectSummary> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            crate::tickets::project_create(loom, workspace, input)
        }))
    }

    pub fn tickets_project_rekey(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        input: HostedTicketProjectWrite<'_>,
    ) -> JsonRpcResult<TicketProjectSummary> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            crate::tickets::project_rekey(loom, workspace, input)
        }))
    }

    pub fn tickets_project_settings_get(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        project_id: &str,
    ) -> JsonRpcResult<Option<TicketProjectSummary>> {
        jsonrpc_result(self.kernel.read(auth, |loom| {
            crate::tickets::project_settings_get(loom, workspace, workspace_id, project_id)
        }))
    }

    pub fn tickets_project_settings_set(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        input: crate::tickets::HostedTicketProjectSettings<'_>,
    ) -> JsonRpcResult<TicketProjectSummary> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            crate::tickets::project_settings_set(loom, workspace, input)
        }))
    }

    pub fn tickets_create(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        input: HostedTicketCreate<'_>,
    ) -> JsonRpcResult<MutationEnvelope<TicketSummary>> {
        let root_before = input.expected_root.map(str::to_string);
        jsonrpc_result(
            self.kernel
                .write(auth, |loom| crate::tickets::create(loom, workspace, input)),
        )
        .map(|response| {
            jsonrpc_ticket_response(
                response,
                "ticket.created",
                root_before,
                vec![MutationChange::ResourceCreated],
            )
        })
    }

    pub fn tickets_update_fields(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        ticket_id: &str,
        fields: &serde_json::Value,
        expected_root: Option<&str>,
    ) -> JsonRpcResult<MutationEnvelope<TicketSummary>> {
        let root_before = expected_root.map(str::to_string);
        let changes = jsonrpc_ticket_field_value_changes(fields);
        jsonrpc_result(self.kernel.write(auth, |loom| {
            crate::tickets::update_fields(
                loom,
                workspace,
                workspace_id,
                ticket_id,
                fields,
                expected_root,
            )
        }))
        .map(|response| jsonrpc_ticket_response(response, "ticket.updated", root_before, changes))
    }

    pub fn tickets_update(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        input: HostedTicketUpdate<'_>,
    ) -> JsonRpcResult<MutationEnvelope<TicketSummary>> {
        let root_before = input.expected_root.map(str::to_string);
        let changes = jsonrpc_ticket_update_changes(&input);
        jsonrpc_result(
            self.kernel
                .write(auth, |loom| crate::tickets::update(loom, workspace, input)),
        )
        .map(|response| jsonrpc_ticket_response(response, "ticket.updated", root_before, changes))
    }

    pub fn tickets_delete(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        input: HostedTicketDelete<'_>,
    ) -> JsonRpcResult<MutationEnvelope<TicketSummary>> {
        let root_before = input.expected_root.map(str::to_string);
        jsonrpc_result(
            self.kernel
                .write(auth, |loom| crate::tickets::delete(loom, workspace, input)),
        )
        .map(|response| {
            jsonrpc_ticket_response(
                response,
                "ticket.deleted",
                root_before,
                vec![MutationChange::ResourceDeleted],
            )
        })
    }

    pub fn tickets_comments(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        ticket_id: &str,
    ) -> JsonRpcResult<Vec<TicketComment>> {
        jsonrpc_result(self.kernel.read(auth, |loom| {
            crate::tickets::comments(loom, workspace, workspace_id, ticket_id)
        }))
    }

    pub fn tickets_comment_add(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        input: HostedTicketCommentAdd<'_>,
    ) -> JsonRpcResult<MutationEnvelope<TicketSummary>> {
        let root_before = input.expected_root.map(str::to_string);
        let mut changes = vec![MutationChange::field_set(
            "comment_type",
            input
                .comment_type
                .unwrap_or(loom_tickets::TICKET_DEFAULT_COMMENT_TYPE),
        )];
        if let Some(comment_id) = input.comment_id {
            changes.push(MutationChange::field_set("comment_id", comment_id));
        }
        jsonrpc_result(self.kernel.write(auth, |loom| {
            crate::tickets::comment_add(loom, workspace, input)
        }))
        .map(|response| {
            jsonrpc_ticket_response(response, "ticket.comment_added", root_before, changes)
        })
    }

    pub fn tickets_comment_update(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        input: HostedTicketCommentUpdate<'_>,
    ) -> JsonRpcResult<MutationEnvelope<TicketSummary>> {
        let root_before = input.expected_root.map(str::to_string);
        let mut changes = vec![MutationChange::field_set("comment_id", input.comment_id)];
        if let Some(comment_type) = input.comment_type {
            changes.push(MutationChange::field_set("comment_type", comment_type));
        }
        if input.body.is_some() {
            changes.push(MutationChange::field_set("body", "updated"));
        }
        jsonrpc_result(self.kernel.write(auth, |loom| {
            crate::tickets::comment_update(loom, workspace, input)
        }))
        .map(|response| {
            jsonrpc_ticket_response(response, "ticket.comment_updated", root_before, changes)
        })
    }

    pub fn tickets_comment_delete(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        input: HostedTicketCommentDelete<'_>,
    ) -> JsonRpcResult<MutationEnvelope<TicketSummary>> {
        let root_before = input.expected_root.map(str::to_string);
        let changes = vec![MutationChange::field_deleted(
            "comment",
            Some(input.comment_id.to_string()),
        )];
        jsonrpc_result(self.kernel.write(auth, |loom| {
            crate::tickets::comment_delete(loom, workspace, input)
        }))
        .map(|response| {
            jsonrpc_ticket_response(response, "ticket.comment_deleted", root_before, changes)
        })
    }

    pub fn tickets_relation_set(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        input: HostedTicketRelationWrite<'_>,
    ) -> JsonRpcResult<MutationEnvelope<TicketRelationSummary>> {
        let root_before = input.expected_root.map(str::to_string);
        jsonrpc_result(self.kernel.write(auth, |loom| {
            crate::tickets::relation_set(loom, workspace, input)
        }))
        .map(|response| {
            let change = MutationChange::relation_set(
                response.result.relation_id.clone(),
                response.result.kind.clone(),
                response.result.target_id.clone(),
            );
            jsonrpc_relation_response(response, "ticket.relation_set", root_before, vec![change])
        })
    }

    pub fn tickets_relation_remove(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        input: HostedTicketRelationRemove<'_>,
    ) -> JsonRpcResult<MutationEnvelope<TicketRelationSummary>> {
        let root_before = input.expected_root.map(str::to_string);
        jsonrpc_result(self.kernel.write(auth, |loom| {
            crate::tickets::relation_remove(loom, workspace, input)
        }))
        .map(|response| {
            let change = MutationChange::relation_removed(
                response.result.relation_id.clone(),
                response.result.kind.clone(),
                response.result.target_id.clone(),
            );
            jsonrpc_relation_response(
                response,
                "ticket.relation_removed",
                root_before,
                vec![change],
            )
        })
    }

    pub fn tickets_get(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        ticket_id: &str,
        projection: Option<&str>,
    ) -> JsonRpcResult<Option<TicketSummary>> {
        jsonrpc_result(self.kernel.read(auth, |loom| {
            crate::tickets::get(loom, workspace, workspace_id, ticket_id, projection)
        }))
    }

    pub fn tickets_history(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        ticket_id: Option<&str>,
    ) -> JsonRpcResult<Vec<TicketHistoryRecord>> {
        jsonrpc_result(self.kernel.read(auth, |loom| {
            crate::tickets::history(loom, workspace, workspace_id, ticket_id)
        }))
    }

    pub fn lanes_create(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        input: HostedLaneCreate<'_>,
    ) -> JsonRpcResult<PublicLane> {
        jsonrpc_result(
            self.kernel
                .write(auth, |loom| crate::lanes::create(loom, workspace, input)),
        )
        .map(public_lane_response)
    }

    pub fn lanes_get(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        lane_id: &str,
    ) -> JsonRpcResult<Option<PublicLane>> {
        jsonrpc_result(
            self.kernel
                .read(auth, |loom| crate::lanes::get(loom, workspace, lane_id)),
        )
        .map(|response| JsonRpcResponse {
            result: response.result.as_ref().map(loom_lanes::public_lane),
        })
    }

    pub fn lanes_list(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
    ) -> JsonRpcResult<Vec<PublicLane>> {
        jsonrpc_result(
            self.kernel
                .read(auth, |loom| crate::lanes::list(loom, workspace)),
        )
        .map(|response| JsonRpcResponse {
            result: response
                .result
                .iter()
                .map(loom_lanes::public_lane)
                .collect(),
        })
    }

    pub fn lanes_update(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        input: HostedLaneUpdate<'_>,
    ) -> JsonRpcResult<PublicLane> {
        jsonrpc_result(
            self.kernel
                .write(auth, |loom| crate::lanes::update(loom, workspace, input)),
        )
        .map(public_lane_response)
    }

    pub fn lanes_ticket_add(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        input: HostedLaneTicketUpdate<'_>,
    ) -> JsonRpcResult<PublicLane> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            crate::lanes::add_ticket(loom, workspace, input)
        }))
        .map(public_lane_response)
    }

    pub fn lanes_ticket_remove(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        input: HostedLaneTicketUpdate<'_>,
    ) -> JsonRpcResult<PublicLane> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            crate::lanes::remove_ticket(loom, workspace, input)
        }))
        .map(public_lane_response)
    }

    pub fn lanes_ticket_transfer(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        source_lane_id: &str,
        target_lane_id: &str,
        ticket_id: &str,
        updated_by: &str,
    ) -> JsonRpcResult<PublicLane> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            crate::lanes::transfer_ticket(
                loom,
                workspace,
                source_lane_id,
                target_lane_id,
                ticket_id,
                updated_by,
            )
        }))
        .map(public_lane_response)
    }

    pub fn lanes_delete(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        lane_id: &str,
        updated_by: &str,
    ) -> JsonRpcResult<PublicLane> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            crate::lanes::delete(loom, workspace, lane_id, updated_by)
        }))
        .map(public_lane_response)
    }

    pub fn spaces_create(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        space_id: &str,
        title: &str,
        expected_root: Option<&str>,
    ) -> JsonRpcResult<SpaceSummary> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            loom_pages::create_space(
                loom,
                workspace,
                workspace_id,
                space_id,
                title,
                expected_root,
            )
        }))
    }

    pub fn spaces_list(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
    ) -> JsonRpcResult<Vec<SpaceSummary>> {
        jsonrpc_result(self.kernel.read(auth, |loom| {
            loom_pages::list_spaces(loom, workspace, workspace_id)
        }))
    }

    pub fn spaces_get(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        space_id: &str,
    ) -> JsonRpcResult<Option<SpaceSummary>> {
        jsonrpc_result(self.kernel.read(auth, |loom| {
            loom_pages::get_space(loom, workspace, workspace_id, space_id)
        }))
    }

    pub fn pages_create(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        request: PageCreateRequest<'_>,
    ) -> JsonRpcResult<PageSummary> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            loom_pages::create_page(loom, workspace, request)
        }))
    }

    pub fn pages_update(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        page_id: &str,
        body: Vec<u8>,
        expected_root: Option<&str>,
    ) -> JsonRpcResult<PageUpdateSummary> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            loom_pages::update_page(
                loom,
                workspace,
                workspace_id,
                page_id,
                body,
                now_ms(),
                expected_root,
            )
        }))
    }

    pub fn pages_publish(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        page_id: &str,
        expected_root: Option<&str>,
    ) -> JsonRpcResult<PagePublishSummary> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            loom_pages::publish_page(
                loom,
                workspace,
                workspace_id,
                page_id,
                now_ms(),
                expected_root,
            )
        }))
    }

    pub fn pages_get(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        page_id: &str,
    ) -> JsonRpcResult<Option<PageSummary>> {
        jsonrpc_result(self.kernel.read(auth, |loom| {
            loom_pages::get_page(loom, workspace, workspace_id, page_id)
        }))
    }

    pub fn pages_history(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        page_id: &str,
    ) -> JsonRpcResult<Vec<PageHistoryEntry>> {
        jsonrpc_result(self.kernel.read(auth, |loom| {
            loom_pages::page_history(loom, workspace, workspace_id, page_id)
        }))
    }

    pub fn structures_create(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        request: StructureCreateRequest<'_>,
    ) -> JsonRpcResult<StructureRenderSummary> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            loom_pages::create_structure(loom, workspace, request)
        }))
    }

    pub fn structures_add_node(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        request: StructureNodeRequest<'_>,
    ) -> JsonRpcResult<StructureNodeSummary> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            loom_pages::add_structure_node(loom, workspace, request)
        }))
    }

    pub fn structures_update_node(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        request: StructureNodeRequest<'_>,
    ) -> JsonRpcResult<StructureNodeSummary> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            loom_pages::update_structure_node(loom, workspace, request)
        }))
    }

    pub fn structures_bind(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        request: StructureBindRequest<'_>,
    ) -> JsonRpcResult<StructureNodeSummary> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            loom_pages::bind_structure_node(loom, workspace, request)
        }))
    }

    pub fn structures_move_node(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        request: StructureMoveRequest<'_>,
    ) -> JsonRpcResult<StructureMoveSummary> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            loom_pages::move_structure_node(loom, workspace, request)
        }))
    }

    pub fn structures_link_node(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        request: StructureLinkRequest<'_>,
    ) -> JsonRpcResult<StructureEdgeSummary> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            loom_pages::link_structure_node(loom, workspace, request)
        }))
    }

    pub fn structures_decompose_to_tickets(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        request: StructureDecomposeRequest<'_>,
    ) -> JsonRpcResult<StructureDecomposeSummary> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            let snapshot = loom.export_state();
            match loom_pages::decompose_to_tickets(loom, workspace, request) {
                Ok(summary) => Ok(summary),
                Err(error) => {
                    if let Err(rollback_error) = loom.import_state(&snapshot) {
                        return Err(loom_core::LoomError::new(
                            loom_core::Code::Internal,
                            format!(
                                "structure decomposition rollback failed after operation error {error}: {rollback_error}"
                            ),
                        ));
                    }
                    Err(error)
                }
            }
        }))
    }

    pub fn structures_get(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        structure_id: &str,
    ) -> JsonRpcResult<Option<StructureRenderSummary>> {
        jsonrpc_result(self.kernel.read(auth, |loom| {
            loom_pages::get_structure(loom, workspace, workspace_id, structure_id)
        }))
    }

    pub fn lifecycles_define(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        definition_cbor: &[u8],
    ) -> JsonRpcResult<LifecycleDefinitionSummary> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            loom_lifecycle::define_lifecycle(loom, workspace, workspace_id, definition_cbor)
        }))
    }

    pub fn lifecycles_define_standard(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        input: StandardLifecycleRequest<'_>,
    ) -> JsonRpcResult<LifecycleDefinitionSummary> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            loom_lifecycle::define_standard_lifecycle(loom, workspace, input)
        }))
    }

    pub fn lifecycles_instantiate(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        instance_id: &str,
        definition_id: &str,
        subject_refs: Vec<String>,
    ) -> JsonRpcResult<LifecycleInstanceSummary> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            loom_lifecycle::instantiate(
                loom,
                workspace,
                workspace_id,
                instance_id,
                definition_id,
                subject_refs,
            )
        }))
    }

    pub fn lifecycles_transition(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        input: LifecycleTransitionRequest<'_>,
    ) -> JsonRpcResult<LifecycleTransitionResult> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            loom_lifecycle::transition(loom, workspace, input)
        }))
    }

    pub fn lifecycles_definition(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        definition_id: &str,
    ) -> JsonRpcResult<Option<LifecycleDefinitionSummary>> {
        jsonrpc_result(self.kernel.read(auth, |loom| {
            loom_lifecycle::get_definition(loom, workspace, workspace_id, definition_id)
        }))
    }

    pub fn lifecycles_definitions(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
    ) -> JsonRpcResult<Vec<LifecycleDefinitionSummary>> {
        jsonrpc_result(self.kernel.read(auth, |loom| {
            loom_lifecycle::list_definitions(loom, workspace, workspace_id)
        }))
    }

    pub fn lifecycles_instance(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        instance_id: &str,
    ) -> JsonRpcResult<Option<LifecycleInstanceSummary>> {
        jsonrpc_result(self.kernel.read(auth, |loom| {
            loom_lifecycle::get_instance(loom, workspace, workspace_id, instance_id)
        }))
    }

    pub fn lifecycles_instances(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
    ) -> JsonRpcResult<Vec<LifecycleInstanceSummary>> {
        jsonrpc_result(self.kernel.read(auth, |loom| {
            loom_lifecycle::list_instances(loom, workspace, workspace_id)
        }))
    }

    pub fn lifecycles_snapshot_plan(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        instance_id: &str,
        to_stage_id: &str,
    ) -> JsonRpcResult<LifecycleSnapshotPlanSummary> {
        jsonrpc_result(self.kernel.read(auth, |loom| {
            loom_lifecycle::snapshot_plan(loom, workspace, workspace_id, instance_id, to_stage_id)
        }))
    }

    pub fn lifecycles_current_surface(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        instance_id: &str,
    ) -> JsonRpcResult<LifecycleStageSurfaceSummary> {
        jsonrpc_result(self.kernel.read(auth, |loom| {
            loom_lifecycle::current_surface(loom, workspace, workspace_id, instance_id)
        }))
    }

    pub fn lifecycles_snapshot(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        snapshot_id: &str,
    ) -> JsonRpcResult<Option<LifecycleSnapshotRecordSummary>> {
        jsonrpc_result(self.kernel.read(auth, |loom| {
            loom_lifecycle::get_snapshot(loom, workspace, workspace_id, snapshot_id)
        }))
    }

    pub fn lifecycles_snapshots(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
    ) -> JsonRpcResult<Vec<LifecycleSnapshotRecordSummary>> {
        jsonrpc_result(self.kernel.read(auth, |loom| {
            loom_lifecycle::list_snapshots(loom, workspace, workspace_id)
        }))
    }

    pub fn lifecycles_operation_log(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
    ) -> JsonRpcResult<LifecycleOperationLogSummary> {
        jsonrpc_result(self.kernel.read(auth, |loom| {
            loom_lifecycle::operation_log(loom, workspace, workspace_id)
        }))
    }

    pub fn drive_list(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        folder_id: &str,
    ) -> JsonRpcResult<HostedDriveFolder> {
        jsonrpc_result(self.kernel.read(auth, |loom| {
            crate::drive::list_folder(loom, workspace, workspace_id, folder_id)
        }))
    }

    pub fn drive_stat(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        folder_id: &str,
        name: &str,
    ) -> JsonRpcResult<HostedDriveStat> {
        jsonrpc_result(self.kernel.read(auth, |loom| {
            crate::drive::stat_node(loom, workspace, workspace_id, folder_id, name)
        }))
    }

    pub fn drive_read(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        file_id: &str,
    ) -> JsonRpcResult<Vec<u8>> {
        jsonrpc_result(self.kernel.read(auth, |loom| {
            crate::drive::read_file(loom, workspace, workspace_id, file_id)
        }))
    }

    pub fn drive_list_versions(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        file_id: &str,
    ) -> JsonRpcResult<Vec<HostedDriveVersion>> {
        jsonrpc_result(self.kernel.read(auth, |loom| {
            crate::drive::list_versions(loom, workspace, workspace_id, file_id)
        }))
    }

    pub fn drive_dehydrate_for_os(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        file_id: &str,
    ) -> JsonRpcResult<HostedDriveOsFileProjection> {
        jsonrpc_result(self.kernel.read(auth, |loom| {
            crate::drive::dehydrate_file_for_os(loom, workspace, workspace_id, file_id)
        }))
    }

    pub fn drive_hydrate_for_os(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        marker_bytes: &[u8],
    ) -> JsonRpcResult<HostedDriveOsFileProjection> {
        jsonrpc_result(self.kernel.read(auth, |loom| {
            crate::drive::hydrate_file_for_os(loom, workspace, workspace_id, marker_bytes)
        }))
    }

    pub fn drive_plan_os_worker(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        materialized: &[HostedDriveOsMaterializedFile],
    ) -> JsonRpcResult<HostedDriveOsWorkerPlan> {
        jsonrpc_result(self.kernel.read(auth, |loom| {
            crate::drive::plan_os_projection_worker(loom, workspace, workspace_id, materialized)
        }))
    }

    pub fn drive_write_from_os(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        request: HostedDriveOsWrite<'_>,
    ) -> JsonRpcResult<HostedDriveWrite> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            crate::drive::write_file_from_os(loom, workspace, request)
        }))
    }

    pub fn drive_list_conflicts(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
    ) -> JsonRpcResult<Vec<HostedDriveConflict>> {
        jsonrpc_result(self.kernel.read(auth, |loom| {
            crate::drive::list_conflicts(loom, workspace, workspace_id)
        }))
    }

    pub fn drive_list_shares(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
    ) -> JsonRpcResult<Vec<HostedDriveShareGrant>> {
        jsonrpc_result(self.kernel.read(auth, |loom| {
            crate::drive::list_shares(loom, workspace, workspace_id)
        }))
    }

    pub fn drive_grant_share(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        request: HostedDriveGrantShare<'_>,
    ) -> JsonRpcResult<HostedDriveWrite> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            crate::drive::grant_share(loom, workspace, request)
        }))
    }

    pub fn drive_revoke_share(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        grant_id: &str,
    ) -> JsonRpcResult<HostedDriveWrite> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            crate::drive::revoke_share(loom, workspace, workspace_id, grant_id)
        }))
    }

    pub fn drive_apply_share_expiry(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        now_ms: u64,
    ) -> JsonRpcResult<HostedDriveShareExpiryApply> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            crate::drive::apply_share_expiry(loom, workspace, workspace_id, now_ms)
        }))
    }

    pub fn drive_list_retention(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
    ) -> JsonRpcResult<Vec<HostedDriveRetentionPin>> {
        jsonrpc_result(self.kernel.read(auth, |loom| {
            crate::drive::list_retention(loom, workspace, workspace_id)
        }))
    }

    pub fn drive_pin_retention(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        request: HostedDrivePinRetention<'_>,
    ) -> JsonRpcResult<HostedDriveWrite> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            crate::drive::pin_retention(loom, workspace, request)
        }))
    }

    pub fn drive_unpin_retention(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        pin_id: &str,
    ) -> JsonRpcResult<HostedDriveWrite> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            crate::drive::unpin_retention(loom, workspace, workspace_id, pin_id)
        }))
    }

    pub fn drive_apply_retention(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        now_ms: u64,
    ) -> JsonRpcResult<HostedDriveRetentionApply> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            crate::drive::apply_retention(loom, workspace, workspace_id, now_ms)
        }))
    }

    pub fn drive_create_folder(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        parent_folder_id: &str,
        folder_id: &str,
        name: &str,
        expected_root: &str,
    ) -> JsonRpcResult<HostedDriveWrite> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            crate::drive::create_folder(
                loom,
                workspace,
                workspace_id,
                parent_folder_id,
                folder_id,
                name,
                expected_root,
            )
        }))
    }

    pub fn drive_create_upload(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        request: HostedDriveCreateUpload<'_>,
    ) -> JsonRpcResult<HostedDriveUploadSession> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            crate::drive::create_upload(loom, workspace, request)
        }))
    }

    pub fn drive_upload_chunk(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        upload_id: &str,
        bytes: &[u8],
    ) -> JsonRpcResult<HostedDriveUploadSession> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            crate::drive::upload_chunk(loom, workspace, workspace_id, upload_id, bytes)
        }))
    }

    pub fn drive_commit_upload(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        upload_id: &str,
    ) -> JsonRpcResult<HostedDriveWrite> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            crate::drive::commit_upload(loom, workspace, workspace_id, upload_id)
        }))
    }

    pub fn drive_resolve_conflict(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        conflict_id: &str,
        resolution: HostedDriveConflictResolution,
    ) -> JsonRpcResult<HostedDriveWrite> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            crate::drive::resolve_conflict(loom, workspace, workspace_id, conflict_id, resolution)
        }))
    }

    pub fn drive_rename(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        folder_id: &str,
        node_id: &str,
        new_name: &str,
        expected_root: &str,
    ) -> JsonRpcResult<HostedDriveWrite> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            crate::drive::rename_node(
                loom,
                workspace,
                workspace_id,
                folder_id,
                node_id,
                new_name,
                expected_root,
            )
        }))
    }

    pub fn drive_move(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        source_folder_id: &str,
        target_folder_id: &str,
        node_id: &str,
        expected_root: &str,
    ) -> JsonRpcResult<HostedDriveWrite> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            crate::drive::move_node(
                loom,
                workspace,
                workspace_id,
                source_folder_id,
                target_folder_id,
                node_id,
                expected_root,
            )
        }))
    }

    pub fn drive_delete(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        folder_id: &str,
        node_id: &str,
        expected_root: &str,
    ) -> JsonRpcResult<HostedDriveWrite> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            crate::drive::delete_node(
                loom,
                workspace,
                workspace_id,
                folder_id,
                node_id,
                expected_root,
            )
        }))
    }

    pub fn cas_delete(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        digest: &Digest,
    ) -> JsonRpcResult<bool> {
        jsonrpc_result(
            self.kernel
                .write(auth, |loom| loom_core::cas_delete(loom, workspace, digest)),
        )
    }

    pub fn fs_read_file(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        path: &str,
    ) -> JsonRpcResult<Vec<u8>> {
        jsonrpc_result(
            self.kernel
                .read(auth, |loom| loom.read_file(workspace, path)),
        )
    }

    pub fn fs_write_file(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        path: &str,
        bytes: &[u8],
    ) -> JsonRpcResult<()> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            loom.write_file(workspace, path, bytes, 0o100644)
        }))
    }

    pub fn exec_cbor(&self, auth: &HostedAuth, request: &[u8]) -> JsonRpcResult<Vec<u8>> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            loom_compute::execute_cbor(loom, request)
                .map_err(|err| loom_core::LoomError::new(err.code(), err.to_string()))
        }))
    }

    pub fn watch_subscribe(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        input: &HostedWatchSubscribeInput,
    ) -> JsonRpcResult<HostedWatchSubscription> {
        jsonrpc_result(self.kernel.read(auth, |loom| {
            crate::watch::watch_subscribe(loom, workspace, input)
        }))
    }

    pub fn watch_poll(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        cursor: &str,
        max: u32,
    ) -> JsonRpcResult<HostedWatchBatch> {
        jsonrpc_result(self.kernel.read(auth, |loom| {
            crate::watch::watch_poll(loom, workspace, cursor, max)
        }))
    }

    pub fn substrate_changes(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        cursor: &str,
        max: u32,
    ) -> JsonRpcResult<HostedSubstrateChangesBatch> {
        jsonrpc_result(self.kernel.read(auth, |loom| {
            crate::substrate_changes::substrate_changes(loom, workspace, cursor, max)
        }))
    }

    pub fn reference_reconciliation_status(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
    ) -> JsonRpcResult<HostedReferenceReconciliationStatus> {
        jsonrpc_result(self.kernel.read(auth, |loom| {
            crate::reference::reference_reconciliation_status(loom, workspace)
        }))
    }

    pub fn chat_post_message(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        channel_id: &str,
        message_id: &str,
        thread_id: Option<&str>,
        body: Vec<u8>,
    ) -> JsonRpcResult<HostedChatWrite> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            crate::chat::post_message(
                loom,
                workspace,
                workspace_id,
                channel_id,
                message_id,
                thread_id,
                body,
            )
        }))
    }

    pub fn chat_edit_message(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        channel_id: &str,
        message_id: &str,
        body: Vec<u8>,
    ) -> JsonRpcResult<HostedChatWrite> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            crate::chat::edit_message(loom, workspace, workspace_id, channel_id, message_id, body)
        }))
    }

    pub fn chat_redact_message(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        channel_id: &str,
        message_id: &str,
        reason: Option<&str>,
    ) -> JsonRpcResult<HostedChatWrite> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            crate::chat::redact_message(
                loom,
                workspace,
                workspace_id,
                channel_id,
                message_id,
                reason,
            )
        }))
    }

    pub fn chat_create_thread(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        channel_id: &str,
        thread_id: &str,
        parent_message_id: &str,
    ) -> JsonRpcResult<HostedChatWrite> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            crate::chat::create_thread(
                loom,
                workspace,
                workspace_id,
                channel_id,
                thread_id,
                parent_message_id,
            )
        }))
    }

    pub fn chat_create_task(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        channel_id: &str,
        task_id: &str,
        message_id: Option<&str>,
        title: &str,
    ) -> JsonRpcResult<HostedChatWrite> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            crate::chat::create_task(
                loom,
                workspace,
                workspace_id,
                channel_id,
                task_id,
                message_id,
                title,
            )
        }))
    }

    pub fn chat_claim_task(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        channel_id: &str,
        task_id: &str,
        claim_id: &str,
        lease_token: Option<&str>,
    ) -> JsonRpcResult<HostedChatWrite> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            crate::chat::claim_task(
                loom,
                workspace,
                workspace_id,
                channel_id,
                task_id,
                claim_id,
                lease_token,
            )
        }))
    }

    pub fn chat_complete_task(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        channel_id: &str,
        task_id: &str,
        claim_id: &str,
        result_message_id: Option<&str>,
    ) -> JsonRpcResult<HostedChatWrite> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            crate::chat::complete_task(
                loom,
                workspace,
                workspace_id,
                channel_id,
                task_id,
                claim_id,
                result_message_id,
            )
        }))
    }

    pub fn chat_invoke_agent(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        channel_id: &str,
        invocation_id: &str,
        agent_principal: WorkspaceId,
        source_message_ids: Vec<String>,
        prompt: Vec<u8>,
    ) -> JsonRpcResult<HostedChatWrite> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            crate::chat::invoke_agent(
                loom,
                workspace,
                workspace_id,
                channel_id,
                invocation_id,
                agent_principal,
                source_message_ids,
                prompt,
            )
        }))
    }

    pub fn chat_agent_reply(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        channel_id: &str,
        invocation_id: &str,
        message_id: &str,
    ) -> JsonRpcResult<HostedChatWrite> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            crate::chat::agent_reply(
                loom,
                workspace,
                workspace_id,
                channel_id,
                invocation_id,
                message_id,
            )
        }))
    }

    pub fn chat_request_handoff(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        channel_id: &str,
        handoff_id: &str,
        from_agent_principal: WorkspaceId,
        to_principal: Option<WorkspaceId>,
        reason: Option<&str>,
    ) -> JsonRpcResult<HostedChatWrite> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            crate::chat::request_handoff(
                loom,
                workspace,
                workspace_id,
                channel_id,
                handoff_id,
                from_agent_principal,
                to_principal,
                reason,
            )
        }))
    }

    pub fn chat_add_reaction(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        channel_id: &str,
        message_id: &str,
        kind: &str,
    ) -> JsonRpcResult<HostedChatWrite> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            crate::chat::add_reaction(loom, workspace, workspace_id, channel_id, message_id, kind)
        }))
    }

    pub fn chat_remove_reaction(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        channel_id: &str,
        message_id: &str,
        kind: &str,
    ) -> JsonRpcResult<HostedChatWrite> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            crate::chat::remove_reaction(
                loom,
                workspace,
                workspace_id,
                channel_id,
                message_id,
                kind,
            )
        }))
    }

    pub fn chat_emoji_list(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
    ) -> JsonRpcResult<HostedChatEmojiRegistry> {
        jsonrpc_result(self.kernel.read(auth, |loom| {
            crate::chat::emoji_registry(loom, workspace, workspace_id)
        }))
    }

    pub fn chat_emoji_register(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        kind: &str,
    ) -> JsonRpcResult<HostedChatEmojiRegistry> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            crate::chat::register_emoji(loom, workspace, workspace_id, kind)
        }))
    }

    pub fn chat_emoji_unregister(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        kind: &str,
    ) -> JsonRpcResult<HostedChatEmojiRegistry> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            crate::chat::unregister_emoji(loom, workspace, workspace_id, kind)
        }))
    }

    pub fn chat_messages(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        channel_id: &str,
    ) -> JsonRpcResult<HostedChatChannel> {
        jsonrpc_result(self.kernel.read(auth, |loom| {
            crate::chat::channel_projection(loom, workspace, workspace_id, channel_id)
        }))
    }

    pub fn chat_cursor(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        channel_id: &str,
    ) -> JsonRpcResult<HostedChatCursor> {
        jsonrpc_result(self.kernel.read(auth, |loom| {
            crate::chat::read_cursor(loom, workspace, workspace_id, channel_id)
        }))
    }

    pub fn chat_update_cursor(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        channel_id: &str,
        next_sequence: u64,
    ) -> JsonRpcResult<HostedChatCursor> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            crate::chat::update_cursor(loom, workspace, workspace_id, channel_id, next_sequence)
        }))
    }

    pub fn chat_set_presence(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        channel_id: &str,
        status: &str,
        ttl_ms: u64,
    ) -> JsonRpcResult<HostedChatPresence> {
        jsonrpc_result(self.kernel.read(auth, |loom| {
            loom.authorize(
                workspace,
                loom_core::FacetKind::Vcs,
                loom_core::AclRight::Read,
            )?;
            let channel_id =
                crate::chat::resolve_channel_id(loom, workspace, workspace_id, channel_id)?;
            let principal = loom.effective_principal()?.unwrap_or(workspace).to_string();
            self.kernel.chat_presence().set(
                workspace,
                workspace_id,
                &channel_id,
                &principal,
                status,
                ttl_ms,
                crate::chat::now_ms(),
            )
        }))
    }

    pub fn chat_presence(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        channel_id: &str,
    ) -> JsonRpcResult<Vec<HostedChatPresence>> {
        jsonrpc_result(self.kernel.read(auth, |loom| {
            loom.authorize(
                workspace,
                loom_core::FacetKind::Vcs,
                loom_core::AclRight::Read,
            )?;
            let channel_id =
                crate::chat::resolve_channel_id(loom, workspace, workspace_id, channel_id)?;
            Ok(self.kernel.chat_presence().list(
                workspace,
                workspace_id,
                &channel_id,
                crate::chat::now_ms(),
            ))
        }))
    }

    pub fn chat_fetch_events(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        channel_id: &str,
        from_sequence: u64,
        max: usize,
    ) -> JsonRpcResult<HostedSubstrateChangesBatch> {
        jsonrpc_result(self.kernel.read(auth, |loom| {
            let batch = crate::chat::operation_changes(
                loom,
                workspace,
                workspace_id,
                channel_id,
                from_sequence,
                max,
            )?;
            Ok(HostedSubstrateChangesBatch {
                events: batch
                    .events
                    .into_iter()
                    .map(crate::substrate_changes::operation_event)
                    .collect(),
                next: batch.next.encode(),
            })
        }))
    }

    pub fn meetings_projection_outputs(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
    ) -> JsonRpcResult<HostedMeetingsProjection> {
        jsonrpc_result(self.kernel.read(auth, |loom| {
            crate::meetings::projection_outputs(loom, workspace, workspace_id)
        }))
    }

    pub fn meetings_list(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        limit: usize,
        offset: usize,
    ) -> JsonRpcResult<HostedMeetingsList> {
        jsonrpc_result(self.kernel.read(auth, |loom| {
            crate::meetings::list(loom, workspace, workspace_id, limit, offset)
        }))
    }

    pub fn meetings_get(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        meeting_id: &str,
    ) -> JsonRpcResult<HostedMeetingDetail> {
        jsonrpc_result(self.kernel.read(auth, |loom| {
            crate::meetings::get(loom, workspace, workspace_id, meeting_id)
        }))
    }

    pub fn meetings_search(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        query: &str,
        field: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> JsonRpcResult<HostedMeetingsSearch> {
        jsonrpc_result(self.kernel.read(auth, |loom| {
            crate::meetings::search_meetings(
                loom,
                workspace,
                workspace_id,
                query,
                field,
                limit,
                offset,
            )
        }))
    }

    pub fn meetings_apply_projection_outputs(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
    ) -> JsonRpcResult<HostedMeetingsProjectionApply> {
        #[cfg(feature = "inference")]
        {
            let runtime = self.kernel.meetings_embedding_runtime().cloned();
            jsonrpc_result(self.kernel.write(auth, |loom| {
                crate::meetings::apply_projection_outputs_with_runtime(
                    loom,
                    workspace,
                    workspace_id,
                    runtime.as_ref(),
                )
            }))
        }
        #[cfg(not(feature = "inference"))]
        {
            jsonrpc_result(self.kernel.write(auth, |loom| {
                crate::meetings::apply_projection_outputs(loom, workspace, workspace_id)
            }))
        }
    }

    pub fn meetings_materialized_outputs(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
    ) -> JsonRpcResult<HostedMeetingsMaterializedOutputs> {
        jsonrpc_result(self.kernel.read(auth, |loom| {
            crate::meetings::materialized_outputs(loom, workspace, workspace_id)
        }))
    }

    pub fn meetings_accept_annotation(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        annotation_id: &str,
    ) -> JsonRpcResult<HostedMeetingsAnnotationReview> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            crate::meetings::accept_annotation(loom, workspace, workspace_id, annotation_id)
        }))
    }

    pub fn meetings_reject_annotation(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        annotation_id: &str,
    ) -> JsonRpcResult<HostedMeetingsAnnotationReview> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            crate::meetings::reject_annotation(loom, workspace, workspace_id, annotation_id)
        }))
    }

    pub fn meetings_propose_vocabulary(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        input: loom_substrate::meetings::VocabularyTermInput<'_>,
        aliases: Vec<String>,
    ) -> JsonRpcResult<HostedMeetingsVocabularyReview> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            crate::meetings::propose_vocabulary_term(loom, workspace, workspace_id, input, aliases)
        }))
    }

    pub fn meetings_accept_vocabulary(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        term_id: &str,
    ) -> JsonRpcResult<HostedMeetingsVocabularyReview> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            crate::meetings::accept_vocabulary_term(loom, workspace, workspace_id, term_id)
        }))
    }

    pub fn meetings_reject_vocabulary(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        term_id: &str,
    ) -> JsonRpcResult<HostedMeetingsVocabularyReview> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            crate::meetings::reject_vocabulary_term(loom, workspace, workspace_id, term_id)
        }))
    }

    pub fn meetings_add_entity_merge(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
        merge_id: &str,
        canonical_entity_id: &str,
        merged_entity_ids: Vec<String>,
        evidence_annotation_ids: Vec<String>,
    ) -> JsonRpcResult<HostedMeetingsEntityMergeWrite> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            crate::meetings::add_entity_merge(
                loom,
                workspace,
                workspace_id,
                merge_id,
                canonical_entity_id,
                merged_entity_ids,
                evidence_annotation_ids,
            )
        }))
    }

    pub fn meetings_extraction_review(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        workspace_id: &str,
    ) -> JsonRpcResult<HostedMeetingsExtractionReview> {
        jsonrpc_result(self.kernel.read(auth, |loom| {
            crate::meetings::extraction_review(loom, workspace, workspace_id)
        }))
    }

    pub fn watch_stream(
        &self,
        auth: &HostedAuth,
        input: JsonRpcWatchStreamInput<'_>,
    ) -> JsonRpcResult<Vec<HostedWatchStreamFrame>> {
        let JsonRpcWatchStreamInput {
            workspace,
            cursor,
            max,
            interval_ms,
            debounce_ms,
            limit,
        } = input;
        validate_stream_max(max)
            .map_err(|error| JsonRpcError::from(HostedError::from_error(error)))?;
        let interval = validate_interval_ms(interval_ms)
            .map_err(|error| JsonRpcError::from(HostedError::from_error(error)))?;
        let debounce = validate_interval_ms(debounce_ms)
            .map_err(|error| JsonRpcError::from(HostedError::from_error(error)))?;
        if let Some(limit) = limit
            && limit == 0
        {
            return Err(JsonRpcError::from(HostedError::from_error(
                loom_core::LoomError::invalid("watch stream limit must be greater than zero"),
            )));
        }

        let mut remaining: Option<usize> = limit.and_then(|value| usize::try_from(value).ok());
        let mut cursor = cursor.to_string();
        let mut frames = Vec::new();

        loop {
            if remaining == Some(0) {
                break;
            }

            if !debounce.is_zero() {
                std::thread::sleep(debounce);
            }

            let out = jsonrpc_result(self.kernel.read(auth, |loom| {
                crate::watch::watch_poll(loom, workspace, &cursor, max)
            }))?;

            if out.result.events.is_empty() {
                if interval.is_zero() {
                    break;
                }
                std::thread::sleep(interval);
                continue;
            }

            let frame = watch_stream_frame(out.result);
            cursor = frame.next.clone();
            frames.push(frame);
            if let Some(remaining) = remaining.as_mut() {
                *remaining = remaining.saturating_sub(1);
            }
        }

        Ok(JsonRpcResponse { result: frames })
    }

    pub fn watch_materialize(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        input: &HostedWatchMaterializeInput,
    ) -> JsonRpcResult<HostedWatchMaterialization> {
        jsonrpc_result(self.kernel.write(auth, |loom| {
            crate::watch::watch_materialize(loom, workspace, input)
        }))
    }
}

fn jsonrpc_result<T>(result: loom_core::Result<T>) -> JsonRpcResult<T> {
    result.map_or_else(
        |err| Err(JsonRpcError::from(HostedError::from_error(err))),
        |result| Ok(JsonRpcResponse { result }),
    )
}

impl From<HostedError> for JsonRpcError {
    fn from(error: HostedError) -> Self {
        Self {
            code: jsonrpc_error_code(error.code_number),
            message: error.message,
            data: JsonRpcErrorData {
                loom_code: error.code_name,
                loom_code_number: error.code_number,
            },
        }
    }
}

fn jsonrpc_error_code(loom_code_number: i32) -> i32 {
    -32000 - loom_code_number
}

fn validate_interval_ms(value_ms: u64) -> loom_core::Result<Duration> {
    if !(10..=60_000).contains(&value_ms) && value_ms != 0 {
        return Err(loom_core::LoomError::invalid(
            "watch stream interval/debounce must be 0 or between 10 and 60000",
        ));
    }
    Ok(Duration::from_millis(value_ms))
}

fn validate_stream_max(max: u32) -> loom_core::Result<()> {
    if max == 0 || max > 1024 {
        return Err(loom_core::LoomError::invalid(
            "watch stream max must be between 1 and 1024",
        ));
    }
    Ok(())
}

#[cfg(all(test, feature = "integration-tests"))]
mod tests {
    use std::fs;

    use loom_core::{Code, Digest, WorkspaceId};
    use loom_interchange::{ExportReport, ImportReport};
    use loom_substrate::versioning::{RevisionIndex, revision_index_path};

    use crate::drive::{HostedDriveGrantShare, HostedDrivePinRetention};
    use crate::jsonrpc::JsonRpcWatchStreamInput;
    use crate::lanes::{HostedLaneCreate, HostedLaneTicketUpdate, HostedLaneUpdate};
    use crate::test_support::{
        chat_snapshot, drive_snapshot, init, meetings_snapshot, nid, temp_path, watch_history,
    };
    use crate::{
        HostedAuth, HostedDriveConflictResolution, HostedDriveCreateUpload, HostedKernel,
        HostedWatchMaterializeInput, HostedWatchSubscribeInput,
    };

    fn revision_index(
        kernel: &HostedKernel,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        scope_id: &str,
    ) -> RevisionIndex {
        let path = revision_index_path(scope_id).unwrap();
        kernel
            .read(auth, |loom| {
                loom.read_file_reserved(workspace, &path)
                    .and_then(|bytes| RevisionIndex::decode(&bytes))
            })
            .unwrap()
    }

    #[test]
    fn jsonrpc_reference_reconciliation_status_uses_hosted_auth() {
        let path = temp_path("jsonrpc-reference-status");
        let ns = init(&path, None);
        let kernel = HostedKernel::new(&path);
        let jsonrpc = kernel.jsonrpc();
        let auth = HostedAuth::passphrase(nid(1), "root-pass", "jsonrpc-reference-status-root");

        let status = jsonrpc.reference_reconciliation_status(&auth, ns).unwrap();
        assert_eq!(status.result.pending, 0);
        assert_eq!(status.result.active_targets, 0);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn jsonrpc_lanes_round_trip_and_reject_invalid_status() {
        let path = temp_path("jsonrpc-lanes");
        let ns = init(&path, None);
        let kernel = HostedKernel::new(&path);
        let jsonrpc = kernel.jsonrpc();
        let auth = HostedAuth::passphrase(nid(1), "root-pass", "jsonrpc-lanes-root");
        let tickets = vec![loom_lanes::LaneTicket {
            ticket_id: "MX-102".to_string(),
            order_key: "F".to_string(),
        }];

        let created = jsonrpc
            .lanes_create(
                &auth,
                ns,
                HostedLaneCreate {
                    lane_id: "tickets-schema",
                    lane_key: "tickets-schema",
                    title: "Tickets schema lane",
                    description: "Durable intention for jsonrpc round-trip.",
                    lane_kind: loom_lanes::LaneKind::Assignment.as_str(),
                    owner_principal: Some("agent:3"),
                    lane_status: "working",
                    lane_tickets: &tickets,
                    active_ticket_id: Some("MX-102"),
                    status_report: "working",
                    reviewer_feedback: "",
                    updated_by: "agent:3",
                },
            )
            .unwrap();
        assert_eq!(created.result.lane_id, "tickets-schema");
        assert_eq!(created.result.lane_tickets, vec!["MX-102".to_string()]);

        let updated = jsonrpc
            .lanes_update(
                &auth,
                ns,
                HostedLaneUpdate {
                    lane_id: "tickets-schema",
                    title: None,
                    description: None,
                    lane_status: None,
                    status_report: Some("ready"),
                    reviewer_feedback: Some("fix typo"),
                    updated_by: "reviewer",
                },
            )
            .unwrap();
        assert_eq!(updated.result.status_report, "ready");
        assert_eq!(updated.result.reviewer_feedback, "fix typo");
        assert_eq!(updated.result.updated_by, "reviewer");

        let updated = jsonrpc
            .lanes_ticket_add(
                &auth,
                ns,
                HostedLaneTicketUpdate {
                    lane_id: "tickets-schema",
                    ticket_id: "MX-103",
                    placement: loom_lanes::LaneTicketPlacement::First,
                    updated_by: "agent:3",
                },
            )
            .unwrap();
        assert_eq!(updated.result.lane_tickets[0], "MX-103");
        assert_eq!(updated.result.lane_tickets[1], "MX-102");

        let updated = jsonrpc
            .lanes_ticket_remove(
                &auth,
                ns,
                HostedLaneTicketUpdate {
                    lane_id: "tickets-schema",
                    ticket_id: "MX-103",
                    placement: loom_lanes::LaneTicketPlacement::Append,
                    updated_by: "agent:3",
                },
            )
            .unwrap();
        assert_eq!(updated.result.lane_tickets.len(), 1);

        let fetched = jsonrpc.lanes_get(&auth, ns, "tickets-schema").unwrap();
        assert_eq!(fetched.result.unwrap().lane_tickets[0], "MX-102");
        let listed = jsonrpc.lanes_list(&auth, ns).unwrap();
        assert_eq!(listed.result.len(), 1);

        fs::remove_file(path).unwrap();
    }

    #[test]
    fn jsonrpc_read_write_share_hosted_auth_and_pep() {
        let path = temp_path("jsonrpc");
        let ns = init(&path, None);
        let kernel = HostedKernel::new(&path);
        let jsonrpc = kernel.jsonrpc();
        let auth = HostedAuth::passphrase(nid(1), "root-pass", "jsonrpc-1");

        jsonrpc
            .fs_write_file(&auth, ns, "notes.txt", b"jsonrpc")
            .unwrap();
        let result = jsonrpc.fs_read_file(&auth, ns, "notes.txt").unwrap();
        assert_eq!(result.result, b"jsonrpc".to_vec());
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn jsonrpc_cas_round_trips_and_uses_acl() {
        let path = temp_path("jsonrpc-cas");
        let user = nid(7);
        let ns = init(&path, Some(user));
        let kernel = HostedKernel::new(&path);
        let jsonrpc = kernel.jsonrpc();
        let root = HostedAuth::passphrase(nid(1), "root-pass", "jsonrpc-cas-root");
        let user = HostedAuth::passphrase(user, "alice-pass", "jsonrpc-cas-user");

        let put = jsonrpc.cas_put(&root, ns, b"alpha").unwrap();
        let digest = Digest::parse(&put.result).unwrap();
        assert!(jsonrpc.cas_has(&root, ns, &digest).unwrap().result);
        assert_eq!(
            jsonrpc.cas_get(&root, ns, &digest).unwrap().result,
            Some(b"alpha".to_vec())
        );
        assert_eq!(
            jsonrpc.cas_list(&root, ns).unwrap().result,
            vec![put.result]
        );
        assert_eq!(
            jsonrpc
                .cas_get(&user, ns, &digest)
                .unwrap_err()
                .data
                .loom_code,
            "PERMISSION_DENIED"
        );
        assert!(jsonrpc.cas_delete(&root, ns, &digest).unwrap().result);
        assert_eq!(jsonrpc.cas_get(&root, ns, &digest).unwrap().result, None);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn jsonrpc_archive_and_car_return_canonical_reports() {
        let path = temp_path("jsonrpc-archive");
        let ns = init(&path, None);
        let kernel = HostedKernel::new(&path);
        let jsonrpc = kernel.jsonrpc();
        let auth = HostedAuth::passphrase(nid(1), "root-pass", "jsonrpc-archive-root");
        let archive_path = temp_path("jsonrpc-archive-out");
        let import_path = temp_path("jsonrpc-archive-import");
        let car_path = temp_path("jsonrpc-car-out");
        let fs_import_dir = temp_path("jsonrpc-fs-import").with_extension("dir");
        let fs_export_dir = temp_path("jsonrpc-fs-export").with_extension("dir");

        fs::create_dir_all(fs_import_dir.join("docs")).unwrap();
        fs::write(fs_import_dir.join("docs").join("fs.txt"), b"fs alpha").unwrap();
        let fs_import = jsonrpc
            .fs_import(&auth, ns, fs_import_dir.to_str().unwrap(), false, false)
            .unwrap();
        let fs_import_report = ImportReport::decode(&fs_import.result).unwrap();
        assert_eq!(fs_import_report.profile, "fs");
        assert_eq!(fs_import_report.operations_applied, 2);
        let fs_export = jsonrpc
            .fs_export(&auth, ns, fs_export_dir.to_str().unwrap(), None, false)
            .unwrap();
        let fs_export_report = ExportReport::decode(&fs_export.result).unwrap();
        assert_eq!(fs_export_report.profile, "fs");
        assert!(fs_export_report.files_written >= 1);
        assert_eq!(
            fs::read(fs_export_dir.join("docs").join("fs.txt")).unwrap(),
            b"fs alpha"
        );

        jsonrpc.fs_write_file(&auth, ns, "a.txt", b"alpha").unwrap();
        kernel
            .write(&auth, |loom| {
                loom.commit(ns, "hosted", "archive fixture", 1).map(|_| ())
            })
            .unwrap();
        let archive = jsonrpc
            .archive_export(
                &auth,
                ns,
                archive_path.to_str().unwrap(),
                "tar",
                None,
                false,
            )
            .unwrap();
        let archive_report = ExportReport::decode(&archive.result).unwrap();
        assert_eq!(archive_report.profile, "archive");
        assert_eq!(archive_report.files_written, 2);

        let imported_ns = init(&import_path, None);
        let import_kernel = HostedKernel::new(&import_path);
        let import_jsonrpc = import_kernel.jsonrpc();
        let imported = import_jsonrpc
            .archive_import(
                &auth,
                imported_ns,
                archive_path.to_str().unwrap(),
                "tar",
                false,
            )
            .unwrap();
        let import_report = ImportReport::decode(&imported.result).unwrap();
        assert_eq!(import_report.profile, "archive");
        assert!(import_report.bytes_in > 0);
        assert!(import_report.bytes_stored >= 5);
        assert_eq!(
            import_jsonrpc
                .fs_read_file(&auth, imported_ns, "a.txt")
                .unwrap()
                .result,
            b"alpha"
        );

        let car = jsonrpc
            .car_export(&auth, ns, car_path.to_str().unwrap(), false)
            .unwrap();
        let car_report = ExportReport::decode(&car.result).unwrap();
        assert_eq!(car_report.profile, "car");
        assert!(car_report.rows_written > 0);

        let imported_car = jsonrpc
            .car_import(&auth, car_path.to_str().unwrap(), true)
            .unwrap();
        let car_import_report = ImportReport::decode(&imported_car.result).unwrap();
        assert_eq!(car_import_report.profile, "car");
        assert!(car_import_report.dry_run);
        assert!(car_import_report.operations_planned > 0);

        let _ = fs::remove_file(path);
        let _ = fs::remove_file(import_path);
        let _ = fs::remove_file(archive_path);
        let _ = fs::remove_file(car_path);
        let _ = fs::remove_dir_all(fs_import_dir);
        let _ = fs::remove_dir_all(fs_export_dir);
    }

    #[test]
    fn jsonrpc_drive_projects_snapshot_and_verified_cas_content() {
        let path = temp_path("jsonrpc-drive");
        let (ns, digest) = drive_snapshot(&path);
        let kernel = HostedKernel::new(&path);
        let jsonrpc = kernel.jsonrpc();
        let auth = HostedAuth::passphrase(nid(1), "root-pass", "jsonrpc-drive-root");

        let list = jsonrpc.drive_list(&auth, ns, "main", "root").unwrap();
        assert_eq!(list.result.entries.len(), 2);
        assert_eq!(list.result.entries[0].name, "Specs");
        assert_eq!(list.result.entries[1].kind, "file");

        let stat = jsonrpc
            .drive_stat(&auth, ns, "main", "root", "plan.txt")
            .unwrap()
            .result;
        assert_eq!(stat.node_id, "file-1");
        assert_eq!(
            stat.latest_version.as_ref().map(|version| version.version),
            Some(1)
        );
        assert_eq!(
            jsonrpc
                .drive_read(&auth, ns, "main", "file-1")
                .unwrap()
                .result,
            b"hello drive"
        );
        assert_eq!(
            jsonrpc
                .drive_list_versions(&auth, ns, "main", "file-1")
                .unwrap()
                .result[0]
                .content_digest,
            digest.to_string()
        );
        assert_eq!(
            jsonrpc
                .drive_stat(&auth, ns, "main", "root", "missing")
                .unwrap_err()
                .data
                .loom_code,
            "NOT_FOUND"
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn jsonrpc_meetings_projects_outputs_review_and_evidence() {
        let path = temp_path("jsonrpc-meetings");
        let ns = meetings_snapshot(&path);
        let kernel = HostedKernel::new(&path);
        let jsonrpc = kernel.jsonrpc();
        let auth = HostedAuth::passphrase(nid(1), "root-pass", "jsonrpc-meetings-root");

        let projections = jsonrpc
            .meetings_projection_outputs(&auth, ns, "organization")
            .unwrap();
        assert_eq!(projections.result.workspace_id, "organization");
        assert!(
            projections
                .result
                .outputs
                .iter()
                .any(|output| output.projection == "document" && output.action == "upsert")
        );
        assert!(!projections.result.output_set_cbor_hex.is_empty());

        let review = jsonrpc
            .meetings_extraction_review(&auth, ns, "organization")
            .unwrap();
        assert_eq!(
            review.result.accepted_annotation_ids,
            vec!["ann-1".to_string()]
        );

        fs::remove_file(path).unwrap();
    }

    #[test]
    fn jsonrpc_drive_share_and_retention_management_round_trip() {
        let path = temp_path("jsonrpc-drive-share-retention");
        let (ns, _) = drive_snapshot(&path);
        let kernel = HostedKernel::new(&path);
        let jsonrpc = kernel.jsonrpc();
        let auth = HostedAuth::passphrase(nid(1), "root-pass", "jsonrpc-drive-admin");

        assert!(
            jsonrpc
                .drive_list_shares(&auth, ns, "main")
                .unwrap()
                .result
                .is_empty()
        );
        let grant = jsonrpc
            .drive_grant_share(
                &auth,
                ns,
                HostedDriveGrantShare {
                    workspace_id: "main",
                    grant_id: "grant-1",
                    target_kind: "folder",
                    target_id: "root",
                    principal: &nid(5).to_string(),
                    role: "editor",
                    granted_at_ms: 100,
                    expires_at_ms: None,
                },
            )
            .unwrap();
        assert_eq!(grant.result.operation_kind, "share.granted");
        let shares = jsonrpc.drive_list_shares(&auth, ns, "main").unwrap().result;
        assert_eq!(shares.len(), 1);
        assert_eq!(shares[0].role, "editor");
        assert_eq!(shares[0].expires_at_ms, None);
        assert_eq!(
            jsonrpc
                .drive_grant_share(
                    &auth,
                    ns,
                    HostedDriveGrantShare {
                        workspace_id: "main",
                        grant_id: "grant-1",
                        target_kind: "folder",
                        target_id: "root",
                        principal: &nid(5).to_string(),
                        role: "viewer",
                        granted_at_ms: 100,
                        expires_at_ms: None,
                    },
                )
                .unwrap_err()
                .data
                .loom_code,
            "ALREADY_EXISTS"
        );
        let expiring = jsonrpc
            .drive_grant_share(
                &auth,
                ns,
                HostedDriveGrantShare {
                    workspace_id: "main",
                    grant_id: "grant-2",
                    target_kind: "folder",
                    target_id: "root",
                    principal: &nid(5).to_string(),
                    role: "viewer",
                    granted_at_ms: 100,
                    expires_at_ms: Some(200),
                },
            )
            .unwrap();
        assert_eq!(expiring.result.operation_kind, "share.granted");
        let expired = jsonrpc
            .drive_apply_share_expiry(&auth, ns, "main", 500)
            .unwrap()
            .result;
        assert_eq!(expired.expired_grant_ids, ["grant-2"]);
        assert_eq!(expired.remaining_grants, 1);
        assert_eq!(
            expired
                .operation
                .as_ref()
                .map(|op| op.operation_kind.as_str()),
            Some("share.expired")
        );
        let shares = jsonrpc.drive_list_shares(&auth, ns, "main").unwrap().result;
        assert_eq!(shares.len(), 1);
        assert_eq!(shares[0].grant_id, "grant-1");
        let revoked = jsonrpc
            .drive_revoke_share(&auth, ns, "main", "grant-1")
            .unwrap();
        assert_eq!(revoked.result.operation_kind, "share.revoked");
        assert!(
            jsonrpc
                .drive_list_shares(&auth, ns, "main")
                .unwrap()
                .result
                .is_empty()
        );

        let root = jsonrpc
            .drive_list(&auth, ns, "main", "root")
            .unwrap()
            .result
            .profile_root;
        assert!(
            jsonrpc
                .drive_list_retention(&auth, ns, "main")
                .unwrap()
                .result
                .is_empty()
        );
        let pin = jsonrpc
            .drive_pin_retention(
                &auth,
                ns,
                HostedDrivePinRetention {
                    workspace_id: "main",
                    pin_id: "hold-1",
                    kind: "legal_hold",
                    root: &root,
                    target_entity_id: Some("folder:root"),
                    added_at_ms: 300,
                    expires_at_ms: None,
                },
            )
            .unwrap();
        assert_eq!(pin.result.operation_kind, "retention.pinned");
        let pins = jsonrpc
            .drive_list_retention(&auth, ns, "main")
            .unwrap()
            .result;
        assert_eq!(pins.len(), 1);
        assert_eq!(pins[0].kind, "legal_hold");
        assert_eq!(pins[0].target_entity_id.as_deref(), Some("folder:root"));
        jsonrpc
            .drive_pin_retention(
                &auth,
                ns,
                HostedDrivePinRetention {
                    workspace_id: "main",
                    pin_id: "trash-1",
                    kind: "trash_subtree",
                    root: &root,
                    target_entity_id: Some("folder:trash"),
                    added_at_ms: 300,
                    expires_at_ms: Some(400),
                },
            )
            .unwrap();
        let applied = jsonrpc
            .drive_apply_retention(&auth, ns, "main", 500)
            .unwrap()
            .result;
        assert_eq!(applied.expired_pin_ids, ["trash-1"]);
        assert_eq!(applied.remaining_pins, 1);
        assert_eq!(
            applied
                .operation
                .as_ref()
                .map(|op| op.operation_kind.as_str()),
            Some("retention.applied")
        );
        assert_eq!(
            jsonrpc
                .drive_pin_retention(
                    &auth,
                    ns,
                    HostedDrivePinRetention {
                        workspace_id: "main",
                        pin_id: "hold-2",
                        kind: "legal_hold",
                        root: &root,
                        target_entity_id: None,
                        added_at_ms: 300,
                        expires_at_ms: Some(400),
                    },
                )
                .unwrap_err()
                .data
                .loom_code,
            "INVALID_ARGUMENT"
        );
        let unpinned = jsonrpc
            .drive_unpin_retention(&auth, ns, "main", "hold-1")
            .unwrap();
        assert_eq!(unpinned.result.operation_kind, "retention.unpinned");
        assert!(
            jsonrpc
                .drive_list_retention(&auth, ns, "main")
                .unwrap()
                .result
                .is_empty()
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn jsonrpc_drive_writes_project_uploads_and_conflicts() {
        let path = temp_path("jsonrpc-drive-write");
        let (ns, _) = drive_snapshot(&path);
        let kernel = HostedKernel::new(&path);
        let jsonrpc = kernel.jsonrpc();
        let auth = HostedAuth::passphrase(nid(1), "root-pass", "jsonrpc-drive-write-root");

        let root = jsonrpc
            .drive_list(&auth, ns, "main", "root")
            .unwrap()
            .result;
        let folder = jsonrpc
            .drive_create_folder(
                &auth,
                ns,
                "main",
                "root",
                "folder-2",
                "Drafts",
                &root.profile_root,
            )
            .unwrap();
        assert_eq!(folder.result.operation_kind, "folder.created");
        assert!(
            jsonrpc
                .drive_create_folder(
                    &auth,
                    ns,
                    "main",
                    "root",
                    "folder-3",
                    "Stale",
                    &root.profile_root,
                )
                .is_err()
        );
        jsonrpc
            .drive_create_upload(
                &auth,
                ns,
                HostedDriveCreateUpload {
                    workspace_id: "main",
                    upload_id: "upload-1",
                    parent_folder_id: "folder-2",
                    name: "Notes.txt",
                    file_id: "file-2",
                    expected_root: &folder.result.profile_root,
                    created_at_ms: 100,
                    replace_file: false,
                },
            )
            .unwrap();
        jsonrpc
            .drive_upload_chunk(&auth, ns, "main", "upload-1", b"hosted drive")
            .unwrap();
        let committed = jsonrpc
            .drive_commit_upload(&auth, ns, "main", "upload-1")
            .unwrap();
        assert_eq!(committed.result.operation_kind, "file.upload_committed");
        let history = revision_index(&kernel, &auth, ns, "main");
        let file_history = history.history("drive:file:file-2");
        assert_eq!(file_history.len(), 1);
        assert_eq!(file_history[0].revision, 1);
        assert_eq!(
            file_history[0].body.media_type,
            "application/vnd.uldren.loom.drive.file-content"
        );
        assert_eq!(
            jsonrpc
                .drive_read(&auth, ns, "main", "file-2")
                .unwrap()
                .result,
            b"hosted drive"
        );

        let renamed = jsonrpc
            .drive_rename(
                &auth,
                ns,
                "main",
                "folder-2",
                "file-2",
                "Notes v2.txt",
                &committed.result.profile_root,
            )
            .unwrap();
        let moved = jsonrpc
            .drive_move(
                &auth,
                ns,
                "main",
                "folder-2",
                "root",
                "file-2",
                &renamed.result.profile_root,
            )
            .unwrap();
        let deleted = jsonrpc
            .drive_delete(
                &auth,
                ns,
                "main",
                "root",
                "file-2",
                &moved.result.profile_root,
            )
            .unwrap();
        assert_eq!(deleted.result.operation_kind, "file.deleted");

        let held_root = jsonrpc
            .drive_list(&auth, ns, "main", "root")
            .unwrap()
            .result;
        jsonrpc
            .drive_create_upload(
                &auth,
                ns,
                HostedDriveCreateUpload {
                    workspace_id: "main",
                    upload_id: "upload-held-1",
                    parent_folder_id: "root",
                    name: "Held.txt",
                    file_id: "file-held",
                    expected_root: &held_root.profile_root,
                    created_at_ms: 150,
                    replace_file: false,
                },
            )
            .unwrap();
        jsonrpc
            .drive_upload_chunk(&auth, ns, "main", "upload-held-1", b"held-v1")
            .unwrap();
        let held_committed = jsonrpc
            .drive_commit_upload(&auth, ns, "main", "upload-held-1")
            .unwrap();
        let delete_base = held_committed.result.profile_root.clone();
        jsonrpc
            .drive_create_upload(
                &auth,
                ns,
                HostedDriveCreateUpload {
                    workspace_id: "main",
                    upload_id: "upload-held-2",
                    parent_folder_id: "root",
                    name: "Held.txt",
                    file_id: "file-held",
                    expected_root: &delete_base,
                    created_at_ms: 160,
                    replace_file: true,
                },
            )
            .unwrap();
        jsonrpc
            .drive_upload_chunk(&auth, ns, "main", "upload-held-2", b"held-v2")
            .unwrap();
        jsonrpc
            .drive_commit_upload(&auth, ns, "main", "upload-held-2")
            .unwrap();
        let held = jsonrpc
            .drive_delete(&auth, ns, "main", "root", "file-held", &delete_base)
            .unwrap();
        assert_eq!(held.result.operation_kind, "file.delete_held");
        assert_eq!(
            held.result.conflict_id.as_deref(),
            Some("delete:file-held:file-held")
        );
        assert_eq!(
            jsonrpc
                .drive_read(&auth, ns, "main", "file-held")
                .unwrap()
                .result,
            b"held-v2"
        );
        jsonrpc
            .drive_resolve_conflict(
                &auth,
                ns,
                "main",
                "delete:file-held:file-held",
                HostedDriveConflictResolution::KeepConflict,
            )
            .unwrap();
        assert!(
            jsonrpc
                .drive_stat(&auth, ns, "main", "root", "Held.txt")
                .is_err()
        );

        let root_for_folder = jsonrpc
            .drive_list(&auth, ns, "main", "root")
            .unwrap()
            .result;
        let folder = jsonrpc
            .drive_create_folder(
                &auth,
                ns,
                "main",
                "root",
                "folder-held",
                "Held Folder",
                &root_for_folder.profile_root,
            )
            .unwrap()
            .result;
        jsonrpc
            .drive_create_upload(
                &auth,
                ns,
                HostedDriveCreateUpload {
                    workspace_id: "main",
                    upload_id: "upload-held-folder-1",
                    parent_folder_id: "folder-held",
                    name: "Child.txt",
                    file_id: "child-held",
                    expected_root: &folder.profile_root,
                    created_at_ms: 170,
                    replace_file: false,
                },
            )
            .unwrap();
        jsonrpc
            .drive_upload_chunk(&auth, ns, "main", "upload-held-folder-1", b"child-v1")
            .unwrap();
        let child_committed = jsonrpc
            .drive_commit_upload(&auth, ns, "main", "upload-held-folder-1")
            .unwrap()
            .result;
        let folder_delete_base = child_committed.profile_root.clone();
        jsonrpc
            .drive_create_upload(
                &auth,
                ns,
                HostedDriveCreateUpload {
                    workspace_id: "main",
                    upload_id: "upload-held-folder-2",
                    parent_folder_id: "folder-held",
                    name: "Child.txt",
                    file_id: "child-held",
                    expected_root: &folder_delete_base,
                    created_at_ms: 180,
                    replace_file: true,
                },
            )
            .unwrap();
        jsonrpc
            .drive_upload_chunk(&auth, ns, "main", "upload-held-folder-2", b"child-v2")
            .unwrap();
        jsonrpc
            .drive_commit_upload(&auth, ns, "main", "upload-held-folder-2")
            .unwrap();
        let held_folder = jsonrpc
            .drive_delete(
                &auth,
                ns,
                "main",
                "root",
                "folder-held",
                &folder_delete_base,
            )
            .unwrap()
            .result;
        assert_eq!(held_folder.operation_kind, "folder.delete_held");
        jsonrpc
            .drive_resolve_conflict(
                &auth,
                ns,
                "main",
                "delete:folder-held:child-held",
                HostedDriveConflictResolution::KeepConflict,
            )
            .unwrap();
        assert!(
            jsonrpc
                .drive_list(&auth, ns, "main", "root")
                .unwrap()
                .result
                .entries
                .iter()
                .all(|entry| entry.node_id != "folder-held")
        );

        let root_after_delete = jsonrpc
            .drive_list(&auth, ns, "main", "root")
            .unwrap()
            .result;
        jsonrpc
            .drive_create_upload(
                &auth,
                ns,
                HostedDriveCreateUpload {
                    workspace_id: "main",
                    upload_id: "upload-2",
                    parent_folder_id: "root",
                    name: "Budget.xlsx",
                    file_id: "file-3",
                    expected_root: &root_after_delete.profile_root,
                    created_at_ms: 200,
                    replace_file: false,
                },
            )
            .unwrap();
        jsonrpc
            .drive_upload_chunk(&auth, ns, "main", "upload-2", b"a")
            .unwrap();
        jsonrpc
            .drive_create_upload(
                &auth,
                ns,
                HostedDriveCreateUpload {
                    workspace_id: "main",
                    upload_id: "upload-3",
                    parent_folder_id: "root",
                    name: "budget.XLSX",
                    file_id: "file-4",
                    expected_root: &root_after_delete.profile_root,
                    created_at_ms: 300,
                    replace_file: false,
                },
            )
            .unwrap();
        jsonrpc
            .drive_upload_chunk(&auth, ns, "main", "upload-3", b"b")
            .unwrap();
        let _first = jsonrpc
            .drive_commit_upload(&auth, ns, "main", "upload-2")
            .unwrap();
        let conflict = jsonrpc
            .drive_commit_upload(&auth, ns, "main", "upload-3")
            .unwrap();
        assert_eq!(
            conflict.result.conflict_id.as_deref(),
            Some("upload-3:conflict")
        );
        let conflicts = jsonrpc.drive_list_conflicts(&auth, ns, "main").unwrap();
        assert_eq!(
            conflicts
                .result
                .iter()
                .find(|conflict| conflict.conflict_id == "upload-3:conflict")
                .unwrap()
                .resolution,
            "open"
        );
        assert!(
            jsonrpc
                .drive_list(&auth, ns, "main", "root")
                .unwrap()
                .result
                .entries
                .iter()
                .any(|entry| entry.name.contains("conflicted copy"))
        );
        let resolved = jsonrpc
            .drive_resolve_conflict(
                &auth,
                ns,
                "main",
                "upload-3:conflict",
                HostedDriveConflictResolution::KeepCurrent,
            )
            .unwrap();
        assert_eq!(resolved.result.operation_kind, "conflict.resolved");
        let resolved_conflicts = jsonrpc.drive_list_conflicts(&auth, ns, "main").unwrap();
        assert_eq!(
            resolved_conflicts
                .result
                .iter()
                .find(|conflict| conflict.conflict_id == "upload-3:conflict")
                .unwrap()
                .resolution,
            "keep_current"
        );
        assert!(
            !jsonrpc
                .drive_list(&auth, ns, "main", "root")
                .unwrap()
                .result
                .entries
                .iter()
                .any(|entry| entry.name.contains("conflicted copy"))
        );

        fs::remove_file(path).unwrap();
    }

    #[test]
    fn jsonrpc_exec_cbor_preserves_loom_error_data() {
        let path = temp_path("jsonrpc-exec");
        let _ns = init(&path, None);
        let kernel = HostedKernel::new(&path);
        let auth = HostedAuth::passphrase(nid(1), "root-pass", "jsonrpc-exec-root");

        let err = kernel.jsonrpc().exec_cbor(&auth, b"not-cbor").unwrap_err();
        assert_eq!(err.data.loom_code, "INVALID_ARGUMENT");
        assert_eq!(err.data.loom_code_number, Code::InvalidArgument.as_i32());
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn jsonrpc_watch_subscribe_and_poll_project_domain_changes() {
        let path = temp_path("jsonrpc-watch");
        let (ns, c0, c1) = watch_history(&path);
        let kernel = HostedKernel::new(&path);
        let jsonrpc = kernel.jsonrpc();
        let auth = HostedAuth::passphrase(nid(1), "root-pass", "jsonrpc-watch-root");

        let sub = jsonrpc
            .watch_subscribe(
                &auth,
                ns,
                &HostedWatchSubscribeInput {
                    branch: Some("main".to_string()),
                    from: Some(c0.to_string()),
                    facet: Some("files".to_string()),
                    path_prefix: Some("b.".to_string()),
                    change_kinds: vec!["added".to_string()],
                },
            )
            .unwrap();
        let batch = jsonrpc
            .watch_poll(&auth, ns, &sub.result.cursor, 10)
            .unwrap();

        assert_eq!(batch.result.events.len(), 1);
        assert_eq!(batch.result.events[0].commit, c1.to_string());
        assert_eq!(batch.result.events[0].changes.len(), 1);
        assert_eq!(batch.result.events[0].changes[0].domain, "files");
        assert_eq!(batch.result.events[0].changes[0].kind, "added");
        assert_eq!(batch.result.events[0].changes[0].key_hex, "622e747874");
        assert!(batch.result.events[0].unsupported_domains.is_empty());
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn jsonrpc_substrate_changes_projects_data_events() {
        let path = temp_path("jsonrpc-substrate-changes");
        let (ns, c0, _) = watch_history(&path);
        let kernel = HostedKernel::new(&path);
        let jsonrpc = kernel.jsonrpc();
        let auth = HostedAuth::passphrase(nid(1), "root-pass", "jsonrpc-substrate-changes-root");
        let sub = jsonrpc
            .watch_subscribe(
                &auth,
                ns,
                &HostedWatchSubscribeInput {
                    branch: Some("main".to_string()),
                    from: Some(c0.to_string()),
                    facet: Some("files".to_string()),
                    path_prefix: None,
                    change_kinds: Vec::new(),
                },
            )
            .unwrap();

        let batch = jsonrpc
            .substrate_changes(&auth, ns, &sub.result.cursor, 10)
            .unwrap();

        assert_eq!(batch.result.events.len(), 1);
        let crate::HostedSubstrateChangeEvent::Data {
            changes, lmdiff, ..
        } = &batch.result.events[0]
        else {
            panic!("expected data event");
        };
        assert_eq!(changes[0].domain, "files");
        assert!(lmdiff.as_ref().is_some_and(|bytes| !bytes.is_empty()));
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn jsonrpc_chat_projects_messages_threads_reactions_and_events() {
        let path = temp_path("jsonrpc-chat");
        let (ns, channel_id) = chat_snapshot(&path);
        let kernel = HostedKernel::new(&path);
        let jsonrpc = kernel.jsonrpc();
        let auth = HostedAuth::passphrase(nid(1), "root-pass", "jsonrpc-chat-root");

        let first = jsonrpc
            .chat_post_message(
                &auth,
                ns,
                "studio",
                "general",
                "m1",
                None,
                b"hello #general".to_vec(),
            )
            .unwrap();
        assert_eq!(first.result.sequence, 1);
        assert_eq!(first.result.channel_id, channel_id.to_string());
        let emoji = jsonrpc.chat_emoji_list(&auth, ns, "studio").unwrap().result;
        assert!(emoji.custom.contains(&"approved".to_string()));
        let emoji = jsonrpc
            .chat_emoji_register(&auth, ns, "studio", "reviewed")
            .unwrap()
            .result;
        assert!(emoji.custom.contains(&"reviewed".to_string()));
        jsonrpc
            .chat_add_reaction(&auth, ns, "studio", "general", "m1", "reviewed")
            .unwrap();
        let emoji = jsonrpc
            .chat_emoji_unregister(&auth, ns, "studio", "reviewed")
            .unwrap()
            .result;
        assert!(!emoji.custom.contains(&"reviewed".to_string()));
        let channel_ref =
            loom_substrate::refs::EntityRef::parse(&format!("channel:{channel_id}")).unwrap();
        let ref_index = kernel
            .read(&auth, |loom| loom_reference::load_index(loom, ns))
            .unwrap()
            .unwrap();
        let edges = ref_index.inbound(&channel_ref);
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].source.entity_id, "m1");
        assert_eq!(edges[0].evidence, "#general");
        jsonrpc
            .chat_create_thread(&auth, ns, "studio", "general", "t1", "m1")
            .unwrap();
        jsonrpc
            .chat_post_message(
                &auth,
                ns,
                "studio",
                "general",
                "m2",
                Some("t1"),
                b"reply".to_vec(),
            )
            .unwrap();
        jsonrpc
            .chat_add_reaction(&auth, ns, "studio", "general", "m1", "approved")
            .unwrap();
        jsonrpc
            .chat_edit_message(&auth, ns, "studio", "general", "m2", b"edited".to_vec())
            .unwrap();
        let history = revision_index(&kernel, &auth, ns, "studio");
        let message_history = history.history(&format!("chat:{channel_id}:message:m2"));
        assert_eq!(message_history.len(), 2);
        assert_eq!(message_history[0].revision, 1);
        assert_eq!(message_history[1].revision, 2);
        assert_eq!(
            message_history[1].body.media_type,
            "application/vnd.uldren.loom.chat.operation+cbor"
        );

        let channel = jsonrpc
            .chat_messages(&auth, ns, "studio", "general")
            .unwrap()
            .result;
        assert_eq!(channel.channel_id, channel_id.to_string());
        assert_eq!(channel.messages.len(), 2);
        assert_eq!(channel.messages[0].message_id, "m1");
        assert_eq!(channel.messages[0].body, b"hello #general");
        assert_eq!(channel.messages[0].reactions[0].kind, "approved");
        assert_eq!(channel.messages[1].body, b"edited");
        assert_eq!(channel.threads[0].thread_id, "t1");

        let cursor = jsonrpc
            .chat_cursor(&auth, ns, "studio", "general")
            .unwrap()
            .result;
        assert_eq!(cursor.channel_id, channel_id.to_string());
        assert_eq!(cursor.next_sequence, 0);
        assert_eq!(cursor.head_sequence, 6);
        assert_eq!(cursor.unread_count, 6);
        let advanced = jsonrpc
            .chat_update_cursor(&auth, ns, "studio", "general", 3)
            .unwrap()
            .result;
        assert_eq!(advanced.next_sequence, 3);
        assert_eq!(advanced.unread_count, 3);
        assert!(
            jsonrpc
                .chat_update_cursor(&auth, ns, "studio", "general", 7)
                .is_err()
        );
        let presence = jsonrpc
            .chat_set_presence(&auth, ns, "studio", "general", "typing", 30_000)
            .unwrap()
            .result;
        assert_eq!(presence.channel_id, channel_id.to_string());
        assert_eq!(presence.status, "typing");
        let live = jsonrpc
            .chat_presence(&auth, ns, "studio", "general")
            .unwrap()
            .result;
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].principal, nid(1).to_string());
        jsonrpc
            .chat_create_task(
                &auth,
                ns,
                "studio",
                "general",
                "task-1",
                Some("m1"),
                "triage",
            )
            .unwrap();
        jsonrpc
            .chat_claim_task(
                &auth,
                ns,
                "studio",
                "general",
                "task-1",
                "claim-1",
                Some("lease-1"),
            )
            .unwrap();
        assert!(
            jsonrpc
                .chat_claim_task(&auth, ns, "studio", "general", "task-1", "claim-2", None)
                .is_err()
        );
        jsonrpc
            .chat_complete_task(
                &auth,
                ns,
                "studio",
                "general",
                "task-1",
                "claim-1",
                Some("m2"),
            )
            .unwrap();
        let agent = nid(9);
        jsonrpc
            .chat_invoke_agent(
                &auth,
                ns,
                "studio",
                "general",
                "invoke-1",
                agent,
                vec!["m1".to_string()],
                b"summarize".to_vec(),
            )
            .unwrap();
        jsonrpc
            .chat_post_message(
                &auth,
                ns,
                "studio",
                "general",
                "m3",
                None,
                b"summary".to_vec(),
            )
            .unwrap();
        jsonrpc
            .chat_agent_reply(&auth, ns, "studio", "general", "invoke-1", "m3")
            .unwrap();
        jsonrpc
            .chat_request_handoff(
                &auth,
                ns,
                "studio",
                "general",
                "handoff-1",
                agent,
                Some(nid(1)),
                Some("needs human"),
            )
            .unwrap();
        let channel = jsonrpc
            .chat_messages(&auth, ns, "studio", "general")
            .unwrap()
            .result;
        assert_eq!(channel.tasks.len(), 1);
        assert_eq!(channel.tasks[0].task_id, "task-1");
        assert!(matches!(
            channel.tasks[0].state,
            crate::chat::HostedChatTaskState::Completed { .. }
        ));
        assert_eq!(channel.agent_invocations.len(), 1);
        assert_eq!(channel.agent_invocations[0].reply_message_ids, ["m3"]);
        assert_eq!(channel.handoffs.len(), 1);
        assert_eq!(channel.handoffs[0].reason.as_deref(), Some("needs human"));

        let events = jsonrpc
            .chat_fetch_events(&auth, ns, "studio", "general", 1, 10)
            .unwrap()
            .result;
        assert_eq!(events.events.len(), 10);
        assert_eq!(events.next, format!("oplog:11:chat:studio:{channel_id}"));
        let crate::HostedSubstrateChangeEvent::Operation { operation_kind, .. } = &events.events[0]
        else {
            panic!("expected chat operation event");
        };
        assert_eq!(operation_kind, "message.created");
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn jsonrpc_watch_poll_rejects_out_of_bounds_max() {
        let path = temp_path("jsonrpc-watch-max");
        let (ns, c0, _) = watch_history(&path);
        let kernel = HostedKernel::new(&path);
        let jsonrpc = kernel.jsonrpc();
        let auth = HostedAuth::passphrase(nid(1), "root-pass", "jsonrpc-watch-max-root");
        let sub = jsonrpc
            .watch_subscribe(
                &auth,
                ns,
                &HostedWatchSubscribeInput {
                    branch: None,
                    from: Some(c0.to_string()),
                    facet: None,
                    path_prefix: None,
                    change_kinds: Vec::new(),
                },
            )
            .unwrap();

        let err = jsonrpc
            .watch_poll(&auth, ns, &sub.result.cursor, 0)
            .unwrap_err();
        assert_eq!(err.data.loom_code, "INVALID_ARGUMENT");
        assert_eq!(err.data.loom_code_number, Code::InvalidArgument.as_i32());
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn jsonrpc_watch_materialize_appends_canonical_batch() {
        let path = temp_path("jsonrpc-watch-materialize");
        let (ns, c0, _) = watch_history(&path);
        let kernel = HostedKernel::new(&path);
        let jsonrpc = kernel.jsonrpc();
        let auth = HostedAuth::passphrase(nid(1), "root-pass", "jsonrpc-watch-materialize-root");
        let sub = jsonrpc
            .watch_subscribe(
                &auth,
                ns,
                &HostedWatchSubscribeInput {
                    branch: Some("main".to_string()),
                    from: Some(c0.to_string()),
                    facet: Some("files".to_string()),
                    path_prefix: Some("b.".to_string()),
                    change_kinds: vec!["added".to_string()],
                },
            )
            .unwrap();

        let out = jsonrpc
            .watch_materialize(
                &auth,
                ns,
                &HostedWatchMaterializeInput {
                    cursor: sub.result.cursor,
                    max: 10,
                    stream: "watch-feed".to_string(),
                },
            )
            .unwrap();

        assert_eq!(out.result.seq, 0);
        assert_eq!(out.result.events, 1);
        assert_eq!(out.result.payload_schema, "loom.watch.batch.v1");
        let payload = kernel
            .read(&auth, |loom| loom_core::log::get(loom, ns, "watch-feed", 0))
            .unwrap()
            .unwrap();
        assert!(!payload.is_empty());
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn jsonrpc_watch_stream_projects_cursor_advance_and_respects_limit() {
        let path = temp_path("jsonrpc-watch-stream");
        let (ns, c0, c1) = watch_history(&path);
        let kernel = HostedKernel::new(&path);
        let jsonrpc = kernel.jsonrpc();
        let auth = HostedAuth::passphrase(nid(1), "root-pass", "jsonrpc-watch-stream-root");

        let sub = jsonrpc
            .watch_subscribe(
                &auth,
                ns,
                &HostedWatchSubscribeInput {
                    branch: Some("main".to_string()),
                    from: Some(c0.to_string()),
                    facet: Some("files".to_string()),
                    path_prefix: Some("b.".to_string()),
                    change_kinds: vec!["added".to_string()],
                },
            )
            .unwrap();

        let frames = jsonrpc
            .watch_stream(
                &auth,
                JsonRpcWatchStreamInput {
                    workspace: ns,
                    cursor: &sub.result.cursor,
                    max: 10,
                    interval_ms: 0,
                    debounce_ms: 0,
                    limit: Some(1),
                },
            )
            .unwrap();
        assert_eq!(frames.result.len(), 1);
        assert_eq!(frames.result[0].events[0].commit, c1.to_string());
        assert_eq!(frames.result[0].source_cursor, frames.result[0].next);

        let empty = jsonrpc
            .watch_stream(
                &auth,
                JsonRpcWatchStreamInput {
                    workspace: ns,
                    cursor: &frames.result[0].next,
                    max: 10,
                    interval_ms: 0,
                    debounce_ms: 0,
                    limit: Some(1),
                },
            )
            .unwrap();
        assert_eq!(empty.result.len(), 0);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn jsonrpc_watch_stream_rejects_bad_max_and_empty_limit() {
        let path = temp_path("jsonrpc-watch-stream-invalid");
        let (ns, c0, _) = watch_history(&path);
        let kernel = HostedKernel::new(&path);
        let jsonrpc = kernel.jsonrpc();
        let auth = HostedAuth::passphrase(nid(1), "root-pass", "jsonrpc-watch-stream-invalid-root");

        let sub = jsonrpc
            .watch_subscribe(
                &auth,
                ns,
                &HostedWatchSubscribeInput {
                    branch: None,
                    from: Some(c0.to_string()),
                    facet: None,
                    path_prefix: None,
                    change_kinds: Vec::new(),
                },
            )
            .unwrap();

        let err = jsonrpc
            .watch_stream(
                &auth,
                JsonRpcWatchStreamInput {
                    workspace: ns,
                    cursor: &sub.result.cursor,
                    max: 0,
                    interval_ms: 0,
                    debounce_ms: 0,
                    limit: Some(1),
                },
            )
            .unwrap_err();
        assert_eq!(err.data.loom_code, "INVALID_ARGUMENT");
        assert_eq!(err.code, -32000 - Code::InvalidArgument.as_i32());

        let err = jsonrpc
            .watch_stream(
                &auth,
                JsonRpcWatchStreamInput {
                    workspace: ns,
                    cursor: &sub.result.cursor,
                    max: 10,
                    interval_ms: 0,
                    debounce_ms: 0,
                    limit: Some(0),
                },
            )
            .unwrap_err();
        assert_eq!(err.data.loom_code, "INVALID_ARGUMENT");
        assert_eq!(err.code, -32000 - Code::InvalidArgument.as_i32());
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn jsonrpc_error_preserves_loom_code_data() {
        let path = temp_path("jsonrpc-error");
        let ns = init(&path, None);
        let kernel = HostedKernel::new(&path);
        let err = kernel
            .jsonrpc()
            .fs_read_file(&HostedAuth::unauthenticated(), ns, "notes.txt")
            .unwrap_err();
        assert_eq!(err.data.loom_code, "AUTHENTICATION_FAILED");
        assert_eq!(
            err.data.loom_code_number,
            Code::AuthenticationFailed.as_i32()
        );
        assert_eq!(err.code, -32000 - Code::AuthenticationFailed.as_i32());
        fs::remove_file(path).unwrap();
    }
}
