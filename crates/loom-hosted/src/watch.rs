use loom_core::vcs::ChangeKind;
use loom_core::workspace::FacetKind;
use loom_core::{Code, Digest, Loom, LoomError, WorkspaceId, log};
use loom_store::FileStore;
use loom_watch::{
    DomainChange, UnsupportedDomainDetail, WatchCursor, WatchSelector, watch_batch_to_cbor,
};
use serde::{Deserialize, Serialize};

pub const MAX_WATCH_POLL: u32 = 1024;

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct HostedWatchSubscribeInput {
    pub branch: Option<String>,
    pub from: Option<String>,
    pub facet: Option<String>,
    pub path_prefix: Option<String>,
    #[serde(default)]
    pub change_kinds: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HostedWatchSubscription {
    pub cursor: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HostedWatchBatch {
    pub events: Vec<HostedDataChange>,
    pub next: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HostedWatchStreamFrame {
    pub source_cursor: String,
    pub events: Vec<HostedDataChange>,
    pub next: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct HostedWatchMaterializeInput {
    pub cursor: String,
    pub max: u32,
    pub stream: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HostedWatchMaterialization {
    pub stream: String,
    pub seq: u64,
    pub source_cursor: String,
    pub events: u32,
    pub payload_schema: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HostedDataChange {
    pub workspace: String,
    #[serde(rename = "ref")]
    pub ref_name: String,
    pub commit: String,
    pub parent: Option<String>,
    pub seq: u64,
    pub changes: Vec<HostedDomainChange>,
    pub unsupported_domains: Vec<HostedUnsupportedDomain>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HostedDomainChange {
    pub domain: String,
    pub schema_version: u32,
    pub kind: String,
    #[serde(skip)]
    pub key: Vec<u8>,
    pub key_hex: String,
    pub before: Option<String>,
    pub after: Option<String>,
    #[serde(skip)]
    pub detail: Option<Vec<u8>>,
    pub detail_hex: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HostedUnsupportedDomain {
    pub domain: String,
    pub capability: String,
}

pub fn watch_subscribe(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    input: &HostedWatchSubscribeInput,
) -> loom_core::Result<HostedWatchSubscription> {
    let branch = input.branch.as_deref().unwrap_or("main");
    let from = input.from.as_deref().map(Digest::parse).transpose()?;
    let mut selector = WatchSelector::new(workspace, branch)?;
    if let Some(facet) = input.facet.as_deref() {
        selector = selector.with_facet(FacetKind::parse(facet)?);
    }
    if let Some(path_prefix) = input.path_prefix.as_deref() {
        selector = selector.with_path_prefix(path_prefix);
    }
    for kind in &input.change_kinds {
        selector = selector.with_change_kind(parse_change_kind(kind)?);
    }
    Ok(HostedWatchSubscription {
        cursor: loom.watch_subscribe(&selector, from)?.encode(),
    })
}

pub fn watch_poll(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    cursor: &str,
    max: u32,
) -> loom_core::Result<HostedWatchBatch> {
    validate_max(max)?;
    let cursor = decode_cursor(workspace, cursor)?;
    let batch = loom.watch_poll(&cursor, max as usize)?;
    Ok(HostedWatchBatch {
        events: batch
            .events
            .into_iter()
            .map(|event| HostedDataChange {
                workspace: event.workspace.to_string(),
                ref_name: event.branch,
                commit: event.commit.to_string(),
                parent: event.parent.map(|digest| digest.to_string()),
                seq: event.seq,
                changes: event.changes.into_iter().map(domain_change).collect(),
                unsupported_domains: event
                    .unsupported_domains
                    .into_iter()
                    .map(unsupported_domain)
                    .collect(),
            })
            .collect(),
        next: batch.next.encode(),
    })
}

pub fn watch_materialize(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    input: &HostedWatchMaterializeInput,
) -> loom_core::Result<HostedWatchMaterialization> {
    validate_max(input.max)?;
    if input.stream.is_empty() {
        return Err(LoomError::invalid(
            "watch materialization stream is required",
        ));
    }
    let cursor = decode_cursor(workspace, &input.cursor)?;
    let batch = loom.watch_poll(&cursor, input.max as usize)?;
    let source_cursor = batch.next.encode();
    let events = u32::try_from(batch.events.len()).unwrap_or(u32::MAX);
    let payload = watch_batch_to_cbor(&batch)?;
    let seq = log::append(loom, workspace, &input.stream, &payload)?;
    Ok(HostedWatchMaterialization {
        stream: input.stream.clone(),
        seq: seq as u64,
        source_cursor,
        events,
        payload_schema: "loom.watch.batch.v1".to_string(),
    })
}

pub fn watch_stream_frame(batch: HostedWatchBatch) -> HostedWatchStreamFrame {
    HostedWatchStreamFrame {
        source_cursor: batch.next.clone(),
        events: batch.events,
        next: batch.next,
    }
}

fn parse_change_kind(kind: &str) -> loom_core::Result<ChangeKind> {
    match kind {
        "added" => Ok(ChangeKind::Added),
        "modified" => Ok(ChangeKind::Modified),
        "deleted" => Ok(ChangeKind::Deleted),
        _ => Err(LoomError::invalid(format!(
            "watch change kind must be added, modified, or deleted, got {kind:?}"
        ))),
    }
}

fn validate_max(max: u32) -> loom_core::Result<()> {
    if max == 0 || max > MAX_WATCH_POLL {
        return Err(LoomError::new(
            Code::InvalidArgument,
            format!("watch poll max must be between 1 and {MAX_WATCH_POLL}"),
        ));
    }
    Ok(())
}

fn decode_cursor(workspace: WorkspaceId, cursor: &str) -> loom_core::Result<WatchCursor> {
    let cursor = WatchCursor::decode(cursor)?;
    if cursor.workspace != workspace {
        return Err(LoomError::new(
            Code::CursorInvalid,
            "watch cursor workspace mismatch",
        ));
    }
    Ok(cursor)
}

fn domain_change(change: DomainChange) -> HostedDomainChange {
    let detail_hex = change.detail.as_deref().map(hex_bytes);
    HostedDomainChange {
        domain: change.domain,
        schema_version: change.schema_version,
        kind: change.kind,
        key: change.key.clone(),
        key_hex: hex_bytes(&change.key),
        before: change.before.map(|digest| digest.to_string()),
        after: change.after.map(|digest| digest.to_string()),
        detail: change.detail,
        detail_hex,
    }
}

fn unsupported_domain(domain: UnsupportedDomainDetail) -> HostedUnsupportedDomain {
    HostedUnsupportedDomain {
        domain: domain.domain,
        capability: domain.capability,
    }
}

fn hex_bytes(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(hex_digit(byte >> 4));
        out.push(hex_digit(byte & 0x0f));
    }
    out
}

fn hex_digit(value: u8) -> char {
    match value {
        0..=9 => (b'0' + value) as char,
        10..=15 => (b'a' + value - 10) as char,
        _ => '0',
    }
}
