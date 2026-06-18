use loom_core::error::{Code, Result};
use loom_core::workspace::WorkspaceId;
use loom_core::{Digest, Loom};
use loom_store::FileStore;
use loom_substrate::refs::{
    AliasBinding, AliasIndex, EntityRef, MarkdownReferenceKind, ReferenceSource,
    UnresolvedReference,
};

pub(crate) const REF_INDEX_DIR: &str = loom_reference::INDEX_DIR;
pub(crate) const REF_INDEX_PATH: &str = loom_reference::INDEX_PATH;
pub(crate) const ALIAS_INDEX_PATH: &str = ".loom/substrate/refs/aliases.lai";
pub use loom_reference::ReconciliationSummary as ReferenceReconciliationSummary;

pub(crate) fn reconcile_ticket_references(
    loom: &mut Loom<FileStore>,
    ns: WorkspaceId,
    workspace_id: &str,
    now_ms: u64,
    max: usize,
) -> Result<ReferenceReconciliationSummary> {
    let principal = loom.effective_principal()?.unwrap_or(ns).to_string();
    let mut summary = loom_reference::status(loom, ns)?;
    let mut index = loom_reference::load_or_rebuild_index(loom, ns)?;
    let records = loom_tickets::reconcile_reference_candidates(
        loom,
        ns,
        workspace_id,
        now_ms,
        max,
        &principal,
    )?;
    loom_reference::apply_resolved_edges(&mut index, &records)?;
    loom_reference::save_index(loom, ns, &index)?;
    summary.processed = records.len() as u64;
    let current = loom_reference::status(loom, ns)?;
    summary.pending = current.pending;
    summary.resolved = current.resolved;
    summary.failed = current.failed;
    Ok(summary)
}

pub(crate) fn reconcile_chat_references(
    loom: &mut Loom<FileStore>,
    ns: WorkspaceId,
    workspace_id: &str,
    now_ms: u64,
    max: usize,
) -> Result<ReferenceReconciliationSummary> {
    let principal = loom.effective_principal()?.unwrap_or(ns).to_string();
    let mut index = loom_reference::load_or_rebuild_index(loom, ns)?;
    let mut processed = 0u64;
    for target in loom_reference::targets(loom, ns)? {
        if target.source_profile != "chat"
            || !target.source_scope.starts_with(&format!("{workspace_id}:"))
        {
            continue;
        }
        let remaining = max.saturating_sub(processed as usize);
        if remaining == 0 {
            break;
        }
        let records = loom_reference::reconcile(
            loom,
            ns,
            &target,
            now_ms,
            remaining,
            &principal,
            |loom, candidate| resolve_chat_candidate(loom, ns, &target.source_scope, candidate),
        )?;
        processed = processed.saturating_add(records.len() as u64);
        loom_reference::apply_resolved_edges(&mut index, &records)?;
    }
    loom_reference::save_index(loom, ns, &index)?;
    let mut summary = loom_reference::status(loom, ns)?;
    summary.processed = processed;
    Ok(summary)
}

fn resolve_chat_candidate(
    loom: &Loom<FileStore>,
    ns: WorkspaceId,
    source_scope: &str,
    candidate: &UnresolvedReference,
) -> Result<Option<EntityRef>> {
    let (workspace_id, _) = source_scope
        .rsplit_once(':')
        .ok_or_else(|| loom_core::LoomError::corrupt("chat reference scope is invalid"))?;
    if let Some(handle) = candidate.alias_text.strip_prefix('@') {
        return loom
            .identity_store()
            .map(|identity| identity.resolve_handle(handle))
            .transpose()?
            .flatten()
            .map(|principal| EntityRef::parse(&format!("principal:{principal}")))
            .transpose();
    }
    if let Some(handle) = candidate.alias_text.strip_prefix('#') {
        return crate::chat::resolve_channel_id(loom, ns, workspace_id, handle)
            .ok()
            .map(|channel| EntityRef::parse(&format!("channel:{channel}")))
            .transpose();
    }
    if let Some(key) = candidate.alias_text.strip_prefix("!ticket:") {
        return resolve_ticket_candidate(loom, ns, workspace_id, key);
    }
    if let Some(target) = candidate.alias_text.strip_prefix('!') {
        return EntityRef::parse(target).map(Some);
    }
    Ok(None)
}

fn resolve_ticket_candidate(
    loom: &Loom<FileStore>,
    ns: WorkspaceId,
    workspace_id: &str,
    key: &str,
) -> Result<Option<EntityRef>> {
    let Some(profile) = loom_tickets::TicketProfileReader::open(loom, ns, workspace_id)? else {
        return Ok(None);
    };
    profile
        .resolve_ticket_key(key)?
        .map(|resolution| EntityRef::parse(&format!("ticket:{}", resolution.ticket_id)))
        .transpose()
}

pub(crate) fn reference_reconciliation_status(
    loom: &Loom<FileStore>,
    ns: WorkspaceId,
) -> Result<ReferenceReconciliationSummary> {
    loom_reference::status(loom, ns)
}

pub(crate) fn update_chat_message_refs(
    loom: &mut Loom<FileStore>,
    ns: WorkspaceId,
    workspace_id: &str,
    channel_id: &str,
    message_id: &str,
    operation_id: &str,
    source_root: Digest,
    body: &[u8],
    now_ms: u64,
) -> Result<()> {
    let source_scope = format!("{workspace_id}:{channel_id}");
    let source = ReferenceSource::new("chat", source_scope, message_id, "body")?;
    let index = loom_reference::load_or_rebuild_index(loom, ns)?;
    let index = loom_reference::update_markdown_references(
        loom,
        index,
        loom_reference::MarkdownReferenceUpdate {
            workspace: ns,
            source,
            operation_id,
            source_root,
            body,
            now_ms,
            relation: "refers_to",
        },
        |loom, candidate| resolve_chat_markdown_candidate(loom, ns, workspace_id, candidate),
    )?;
    loom_reference::save_index(loom, ns, &index)
}

fn resolve_chat_markdown_candidate(
    loom: &Loom<FileStore>,
    ns: WorkspaceId,
    workspace_id: &str,
    candidate: &loom_substrate::refs::MarkdownReferenceCandidate,
) -> Result<Option<EntityRef>> {
    match candidate.kind {
        MarkdownReferenceKind::Typed => {
            let Some(target_text) = candidate.text.strip_prefix('!') else {
                return Ok(None);
            };
            if let Some(key) = target_text.strip_prefix("ticket:") {
                resolve_ticket_candidate(loom, ns, workspace_id, key)
            } else {
                Ok(EntityRef::parse(target_text).ok())
            }
        }
        MarkdownReferenceKind::PrincipalHandle => loom
            .identity_store()
            .map(|identity| identity.resolve_handle(&candidate.text[1..]))
            .transpose()?
            .flatten()
            .map(|principal| EntityRef::parse(&format!("principal:{principal}")))
            .transpose(),
        MarkdownReferenceKind::ChannelHandle => {
            crate::chat::resolve_channel_id(loom, ns, workspace_id, &candidate.text[1..])
                .ok()
                .map(|channel| EntityRef::parse(&format!("channel:{channel}")))
                .transpose()
        }
    }
}

pub(crate) fn bind_alias(
    loom: &mut Loom<FileStore>,
    ns: WorkspaceId,
    scope_id: &str,
    alias: &str,
    target: &str,
) -> Result<AliasBinding> {
    let mut index = load_alias_index_if_present(loom, ns)?.unwrap_or_default();
    let target = EntityRef::parse(target)?;
    let sequence = index.next_sequence(scope_id);
    let binding = AliasBinding::new(alias, target, scope_id, sequence)?;
    index.bind(binding.clone());
    save_alias_index(loom, ns, &index)?;
    Ok(binding)
}

pub(crate) fn release_alias(
    loom: &mut Loom<FileStore>,
    ns: WorkspaceId,
    scope_id: &str,
    alias: &str,
) -> Result<bool> {
    let Some(mut index) = load_alias_index_if_present(loom, ns)? else {
        return Ok(false);
    };
    let released = index.release(scope_id, alias).is_some();
    if released {
        save_alias_index(loom, ns, &index)?;
    }
    Ok(released)
}

pub(crate) fn load_alias_index_if_present(
    loom: &Loom<FileStore>,
    ns: WorkspaceId,
) -> Result<Option<AliasIndex>> {
    match loom.read_file_reserved(ns, ALIAS_INDEX_PATH) {
        Ok(bytes) => AliasIndex::decode(&bytes).map(Some),
        Err(e) if e.code == Code::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

fn save_alias_index(loom: &mut Loom<FileStore>, ns: WorkspaceId, index: &AliasIndex) -> Result<()> {
    loom.create_directory_reserved(ns, REF_INDEX_DIR, true)?;
    loom.write_file_reserved(ns, ALIAS_INDEX_PATH, &index.encode()?, 0o100644)
}
