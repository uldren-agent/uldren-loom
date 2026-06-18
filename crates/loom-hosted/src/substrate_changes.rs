use loom_core::workspace::{FacetKind, WorkspaceId};
use loom_core::{AclRight, Code, Loom, LoomError};
use loom_store::FileStore;
use loom_substrate::changes::{OperationChangeBatch, OperationChangeCursor, OperationChangeRecord};
use loom_substrate::chat::{ChannelOperationLog, ChatOperationRecord};
use loom_substrate::pages::{PageOperationLog, page_profile_operation_log_key};
use loom_tickets::{TicketOperationLog, TicketProfileReader};

use crate::chat::chat_queue_stream_name;
use crate::watch::{HostedDataChange, MAX_WATCH_POLL};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct HostedSubstrateChangesBatch {
    pub events: Vec<HostedSubstrateChangeEvent>,
    pub next: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HostedSubstrateChangeEvent {
    Data {
        workspace: String,
        #[serde(rename = "ref")]
        ref_name: String,
        commit: String,
        parent: Option<String>,
        seq: u64,
        changes: Vec<crate::watch::HostedDomainChange>,
        unsupported_domains: Vec<crate::watch::HostedUnsupportedDomain>,
        lmdiff: Option<Vec<u8>>,
    },
    Operation {
        workspace_id: String,
        app_id: String,
        scope_id: String,
        operation_id: String,
        operation_kind: String,
        sequence: u64,
        actor_principal: String,
        timestamp_ms: u64,
        root_after: String,
        payload_digest: String,
        policy_labels: Vec<String>,
    },
}

pub fn substrate_changes(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    cursor: &str,
    max: u32,
) -> loom_core::Result<HostedSubstrateChangesBatch> {
    validate_max(max)?;
    if cursor.starts_with("oplog:") {
        let cursor = OperationChangeCursor::decode(cursor)?;
        let batch = profile_operation_changes(loom, workspace, &cursor, max as usize)?;
        return Ok(HostedSubstrateChangesBatch {
            events: batch.events.into_iter().map(operation_event).collect(),
            next: batch.next.encode(),
        });
    }
    let batch = crate::watch::watch_poll(loom, workspace, cursor, max)?;
    let mut events = Vec::with_capacity(batch.events.len());
    for event in batch.events {
        let lmdiff = event
            .parent
            .as_deref()
            .map(|parent| {
                let parent = loom_core::Digest::parse(parent)?;
                let commit = loom_core::Digest::parse(&event.commit)?;
                loom.diff_commits(workspace, parent, commit)
            })
            .transpose()?;
        events.push(HostedSubstrateChangeEvent::Data {
            workspace: event.workspace,
            ref_name: event.ref_name,
            commit: event.commit,
            parent: event.parent,
            seq: event.seq,
            changes: event.changes,
            unsupported_domains: event.unsupported_domains,
            lmdiff,
        });
    }
    Ok(HostedSubstrateChangesBatch {
        events,
        next: batch.next,
    })
}

fn profile_operation_changes(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    cursor: &OperationChangeCursor,
    max: usize,
) -> loom_core::Result<OperationChangeBatch> {
    loom.authorize(workspace, FacetKind::Vcs, AclRight::Read)?;
    if let Some(workspace_id) = cursor.scope_id.strip_prefix("tickets:") {
        let Some(profile) = TicketProfileReader::open(loom, workspace, workspace_id)? else {
            return Ok(OperationChangeBatch {
                events: Vec::new(),
                next: cursor.clone(),
            });
        };
        return TicketOperationLog::new(workspace_id, profile.operations()?)?.changes(cursor, max);
    }
    if let Some(workspace_id) = cursor.scope_id.strip_prefix("pages:") {
        return page_log(loom.store(), workspace_id)?.changes(cursor, max);
    }
    if let Some(scope) = cursor.scope_id.strip_prefix("chat:") {
        let (workspace_id, channel_id) = scope
            .split_once(':')
            .ok_or_else(|| LoomError::invalid("invalid chat operation cursor"))?;
        return chat_log(loom, workspace, workspace_id, channel_id)?.changes(cursor, max);
    }
    Err(LoomError::invalid("unsupported profile operation cursor"))
}

fn page_log(store: &FileStore, workspace_id: &str) -> loom_core::Result<PageOperationLog> {
    match store.control_get(&page_profile_operation_log_key(workspace_id)?)? {
        Some(bytes) => PageOperationLog::decode(&bytes),
        None => PageOperationLog::new(workspace_id, Vec::new()),
    }
}

fn chat_log(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    channel_id: &str,
) -> loom_core::Result<ChannelOperationLog> {
    let stream = chat_queue_stream_name(workspace_id, channel_id)?;
    let len = match loom.stream_len(workspace, &stream) {
        Ok(len) => len,
        Err(err) if err.code == Code::NotFound => {
            return ChannelOperationLog::new(workspace_id, channel_id, Vec::new());
        }
        Err(err) => return Err(err),
    };
    let records = loom
        .stream_range(workspace, &stream, 0, len)?
        .into_iter()
        .map(|entry| ChatOperationRecord::decode(&entry))
        .collect::<loom_core::Result<Vec<_>>>()?;
    ChannelOperationLog::new(workspace_id, channel_id, records)
}

pub(crate) fn operation_event(record: OperationChangeRecord) -> HostedSubstrateChangeEvent {
    HostedSubstrateChangeEvent::Operation {
        workspace_id: record.workspace_id,
        app_id: record.app_id,
        scope_id: record.scope_id,
        operation_id: record.operation_id,
        operation_kind: record.operation_kind,
        sequence: record.sequence,
        actor_principal: record.actor_principal,
        timestamp_ms: record.timestamp_ms,
        root_after: record.root_after.to_string(),
        payload_digest: record.payload_digest.to_string(),
        policy_labels: record.policy_labels,
    }
}

fn validate_max(max: u32) -> loom_core::Result<()> {
    if max == 0 || max > MAX_WATCH_POLL {
        return Err(LoomError::new(
            Code::InvalidArgument,
            format!("substrate changes max must be between 1 and {MAX_WATCH_POLL}"),
        ));
    }
    Ok(())
}

impl From<HostedDataChange> for HostedSubstrateChangeEvent {
    fn from(event: HostedDataChange) -> Self {
        Self::Data {
            workspace: event.workspace,
            ref_name: event.ref_name,
            commit: event.commit,
            parent: event.parent,
            seq: event.seq,
            changes: event.changes,
            unsupported_domains: event.unsupported_domains,
            lmdiff: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use loom_core::{Algo, Digest, Loom};
    use loom_store::{FileStore, open_loom_read_unlocked, save_loom};
    use loom_substrate::chat::{
        APP_ID as CHAT_APP_ID, ChatOperationPayload, ChatOperationRecord,
        chat_operation_cursor_scope,
    };
    use loom_substrate::{ActorKind, OperationEnvelope, OperationEnvelopeInput};

    use super::*;
    use crate::test_support::{nid, temp_path, watch_history};

    #[test]
    fn hosted_substrate_changes_project_data_events_with_lmdiff() {
        let path = temp_path("substrate-changes-data");
        let (ns, c0, _) = watch_history(&path);
        let loom = open_loom_read_unlocked(&path, None).unwrap();
        let cursor = loom
            .watch_subscribe(
                &loom_watch::WatchSelector::new(ns, "main").unwrap(),
                Some(c0),
            )
            .unwrap();

        let batch = substrate_changes(&loom, ns, &cursor.encode(), 10).unwrap();

        assert_eq!(batch.events.len(), 1);
        let HostedSubstrateChangeEvent::Data {
            workspace,
            changes,
            lmdiff,
            ..
        } = &batch.events[0]
        else {
            panic!("expected data event");
        };
        assert_eq!(workspace, &ns.to_string());
        assert_eq!(changes[0].domain, "files");
        assert!(lmdiff.as_ref().is_some_and(|bytes| !bytes.is_empty()));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn hosted_substrate_changes_project_chat_operation_events() {
        let path = temp_path("substrate-changes-chat");
        let actor = nid(35);
        let ns = loom_coordination::with_local_store_write_lock(&path, || {
            let store = FileStore::create_with_profile(&path, Algo::Blake3).unwrap();
            let mut loom = Loom::new(store);
            let ns = loom
                .registry_mut()
                .create(FacetKind::Files, Some("main"), nid(36))
                .unwrap();
            let payload = ChatOperationPayload::MessageCreated {
                message_id: "m1".to_string(),
                thread_id: None,
                body: b"hello".to_vec(),
            };
            let payload_bytes = payload.encode().unwrap();
            let envelope = OperationEnvelope::new(
                Algo::Blake3,
                OperationEnvelopeInput {
                    workspace_id: "studio",
                    app_id: CHAT_APP_ID,
                    scope_id: "general",
                    operation_id: "chat:studio:general:1",
                    operation_kind: payload.operation_kind(),
                    sequence: 1,
                    actor_principal: actor,
                    actor_kind: ActorKind::User,
                    timestamp_ms: 100,
                    idempotency_key: "chat:studio:general:1",
                    base_root: Digest::hash(Algo::Blake3, b"base"),
                    base_entity_version: None,
                    target_entity_id: Some(payload.target_entity_id()),
                    payload: &payload_bytes,
                    policy_labels: &["internal"],
                    signature: None,
                    agent: None,
                },
            )
            .unwrap();
            let record = ChatOperationRecord::new(
                1,
                "chat:studio:general:1",
                payload.operation_kind(),
                Some(payload.target_entity_id().to_string()),
                Digest::hash(Algo::Blake3, b"after"),
                envelope.encode().unwrap(),
            )
            .unwrap();
            let log = ChannelOperationLog::new("studio", "general", vec![record]).unwrap();
            let stream = chat_queue_stream_name("studio", "general").unwrap();
            for record in &log.records {
                loom.stream_append(ns, &stream, &record.encode().unwrap())
                    .unwrap();
            }
            save_loom(&mut loom).unwrap();
            drop(loom);
            Ok(ns)
        })
        .unwrap();
        let loom = open_loom_read_unlocked(&path, None).unwrap();
        let cursor =
            OperationChangeCursor::new(chat_operation_cursor_scope("studio", "general"), 1)
                .unwrap()
                .encode();

        let batch = substrate_changes(&loom, ns, &cursor, 10).unwrap();

        assert_eq!(batch.next, "oplog:2:chat:studio:general");
        let HostedSubstrateChangeEvent::Operation {
            workspace_id,
            app_id,
            scope_id,
            operation_kind,
            policy_labels,
            ..
        } = &batch.events[0]
        else {
            panic!("expected operation event");
        };
        assert_eq!(workspace_id, "studio");
        assert_eq!(app_id, "chat");
        assert_eq!(scope_id, "general");
        assert_eq!(operation_kind, "message.created");
        assert_eq!(policy_labels, &vec!["internal".to_string()]);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn hosted_substrate_changes_rejects_out_of_bounds_max() {
        let path = temp_path("substrate-changes-max");
        let (ns, c0, _) = watch_history(&path);
        let loom = open_loom_read_unlocked(&path, None).unwrap();
        let cursor = loom
            .watch_subscribe(
                &loom_watch::WatchSelector::new(ns, "main").unwrap(),
                Some(c0),
            )
            .unwrap();

        let err = substrate_changes(&loom, ns, &cursor.encode(), 0).unwrap_err();

        assert_eq!(err.code, Code::InvalidArgument);
        std::fs::remove_file(path).unwrap();
    }
}
