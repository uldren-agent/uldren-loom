//! Reusable watch contracts for observation clients.

use loom_codec::Value;
use loom_types::{ChangeKind, Code, Digest, FacetKind, LoomError, Result, WorkspaceId};

const CURSOR_VERSION: &str = "loom-watch-v1";
const CURSOR_V2_PREFIX: &str = "loom-watch-v2.";
pub const FILES_DOMAIN: &str = "files";
pub const FILES_DOMAIN_CHANGE_SCHEMA_VERSION: u32 = 1;
const NO_COMMIT: &str = "-";
const B64_URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
const WATCH_DOMAIN_SUPPORTS: &[WatchDomainSupport] = &[
    WatchDomainSupport::stable(FacetKind::Files, "watch.domain.files"),
    WatchDomainSupport::unsupported(FacetKind::Sql, "watch.domain.sql"),
    WatchDomainSupport::unsupported(FacetKind::Kv, "watch.domain.kv"),
    WatchDomainSupport::unsupported(FacetKind::Document, "watch.domain.document"),
    WatchDomainSupport::unsupported(FacetKind::Vector, "watch.domain.vector"),
    WatchDomainSupport::unsupported(FacetKind::Graph, "watch.domain.graph"),
    WatchDomainSupport::unsupported(FacetKind::Columnar, "watch.domain.columnar"),
    WatchDomainSupport::unsupported(FacetKind::Queue, "watch.domain.queue"),
    WatchDomainSupport::unsupported(FacetKind::TimeSeries, "watch.domain.time-series"),
    WatchDomainSupport::unsupported(FacetKind::Cas, "watch.domain.cas"),
    WatchDomainSupport::unsupported(FacetKind::Ledger, "watch.domain.ledger"),
    WatchDomainSupport::unsupported(FacetKind::Program, "watch.domain.program"),
    WatchDomainSupport::unsupported(FacetKind::Calendar, "watch.domain.calendar"),
    WatchDomainSupport::unsupported(FacetKind::Contacts, "watch.domain.contacts"),
    WatchDomainSupport::unsupported(FacetKind::Mail, "watch.domain.mail"),
    WatchDomainSupport::unsupported(FacetKind::Search, "watch.domain.search"),
    WatchDomainSupport::unsupported(FacetKind::Dataframe, "watch.domain.dataframe"),
    WatchDomainSupport::unsupported(FacetKind::Metrics, "watch.domain.metrics"),
    WatchDomainSupport::unsupported(FacetKind::Logs, "watch.domain.logs"),
    WatchDomainSupport::unsupported(FacetKind::Traces, "watch.domain.traces"),
];
const EXPECTED_WATCH_DOMAIN_SUPPORT_COUNT: usize = FacetKind::ALL.len() - 1;
const EXPECTED_WATCH_DOMAIN_SUPPORT_MASK: u128 = expected_watch_domain_support_mask();
const WATCH_DOMAIN_SUPPORT_MASK: u128 = watch_domain_support_mask(WATCH_DOMAIN_SUPPORTS);
const _: [(); EXPECTED_WATCH_DOMAIN_SUPPORT_COUNT] = [(); WATCH_DOMAIN_SUPPORTS.len()];
const _: [(); 1] = [(); (EXPECTED_WATCH_DOMAIN_SUPPORT_MASK == WATCH_DOMAIN_SUPPORT_MASK) as usize];

const fn expected_watch_domain_support_mask() -> u128 {
    let mut index = 0;
    let mut mask = 0u128;
    while index < FacetKind::ALL.len() {
        let facet = FacetKind::ALL[index];
        let tag = facet.stable_tag();
        if tag != FacetKind::Vcs.stable_tag() {
            mask |= 1u128 << tag;
        }
        index += 1;
    }
    mask
}

const fn watch_domain_support_mask(supports: &[WatchDomainSupport]) -> u128 {
    let mut index = 0;
    let mut mask = 0u128;
    while index < supports.len() {
        mask |= 1u128 << supports[index].facet.stable_tag();
        index += 1;
    }
    mask
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchDomainDetail {
    Stable,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WatchDomainSupport {
    pub facet: FacetKind,
    pub domain: &'static str,
    pub capability: &'static str,
    pub detail: WatchDomainDetail,
}

impl WatchDomainSupport {
    const fn stable(facet: FacetKind, capability: &'static str) -> Self {
        Self {
            facet,
            domain: facet.as_str(),
            capability,
            detail: WatchDomainDetail::Stable,
        }
    }

    const fn unsupported(facet: FacetKind, capability: &'static str) -> Self {
        Self {
            facet,
            domain: facet.as_str(),
            capability,
            detail: WatchDomainDetail::Unsupported,
        }
    }
}

pub fn watch_domain_supports() -> &'static [WatchDomainSupport] {
    WATCH_DOMAIN_SUPPORTS
}

pub fn watch_domain_support(facet: FacetKind) -> Option<&'static WatchDomainSupport> {
    WATCH_DOMAIN_SUPPORTS
        .iter()
        .find(|support| support.facet == facet)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchSelector {
    pub workspace: WorkspaceId,
    pub branch: String,
    pub facet: Option<FacetKind>,
    pub path_prefix: Option<String>,
    pub change_kinds: Vec<ChangeKind>,
}

impl WatchSelector {
    pub fn new(workspace: WorkspaceId, branch: impl Into<String>) -> Result<Self> {
        let branch = branch.into();
        validate_branch(&branch)?;
        Ok(Self {
            workspace,
            branch,
            facet: None,
            path_prefix: None,
            change_kinds: Vec::new(),
        })
    }

    pub fn with_facet(mut self, facet: FacetKind) -> Self {
        self.facet = Some(facet);
        self
    }

    pub fn with_path_prefix(mut self, path_prefix: impl Into<String>) -> Self {
        self.path_prefix = Some(path_prefix.into());
        self
    }

    pub fn with_change_kind(mut self, kind: ChangeKind) -> Self {
        self.change_kinds.push(kind);
        canonicalize_change_kinds(&mut self.change_kinds);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchCursor {
    pub workspace: WorkspaceId,
    pub branch: String,
    pub commit: Option<Digest>,
    pub intra_commit_index: u32,
    pub facet: Option<FacetKind>,
    pub path_prefix: Option<String>,
    pub change_kinds: Vec<ChangeKind>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchBatch {
    pub events: Vec<ChangeEvent>,
    pub next: WatchCursor,
}

pub fn watch_batch_to_cbor(batch: &WatchBatch) -> Result<Vec<u8>> {
    let value = Value::Map(vec![
        cbor_pair("schema", Value::Text("loom.watch.batch.v1".to_string())),
        (
            Value::Text("events".to_string()),
            Value::Array(batch.events.iter().map(change_event_cbor).collect()),
        ),
        cbor_pair("next", Value::Text(batch.next.encode())),
    ]);
    Ok(loom_codec::encode(&value).expect("watch batch CBOR is encodable"))
}

impl WatchCursor {
    pub fn new(
        workspace: WorkspaceId,
        branch: impl Into<String>,
        commit: Option<Digest>,
        intra_commit_index: u32,
    ) -> Result<Self> {
        let branch = branch.into();
        validate_branch(&branch)?;
        Ok(Self {
            workspace,
            branch,
            commit,
            intra_commit_index,
            facet: None,
            path_prefix: None,
            change_kinds: Vec::new(),
        })
    }

    pub fn from_selector(selector: &WatchSelector, commit: Option<Digest>) -> Self {
        Self {
            workspace: selector.workspace,
            branch: selector.branch.clone(),
            commit,
            intra_commit_index: 0,
            facet: selector.facet,
            path_prefix: selector.path_prefix.clone(),
            change_kinds: selector.change_kinds.clone(),
        }
    }

    pub fn with_selector_from(mut self, cursor: &WatchCursor) -> Self {
        self.facet = cursor.facet;
        self.path_prefix = cursor.path_prefix.clone();
        self.change_kinds = cursor.change_kinds.clone();
        self
    }

    pub fn encode(&self) -> String {
        if self.facet.is_some() || self.path_prefix.is_some() || !self.change_kinds.is_empty() {
            return self.encode_v2();
        }
        format!(
            "{CURSOR_VERSION}|{}|{}|{}|{}",
            self.workspace,
            hex::encode(self.branch.as_bytes()),
            self.commit
                .map(|digest| digest.to_string())
                .unwrap_or_else(|| NO_COMMIT.to_string()),
            self.intra_commit_index
        )
    }

    pub fn decode(cursor: &str) -> Result<Self> {
        if let Some(payload) = cursor.strip_prefix(CURSOR_V2_PREFIX) {
            return Self::decode_v2(payload);
        }
        let mut parts = cursor.split('|');
        let version = parts.next().ok_or_else(cursor_invalid)?;
        let workspace = parts.next().ok_or_else(cursor_invalid)?;
        let branch_hex = parts.next().ok_or_else(cursor_invalid)?;
        let commit = parts.next().ok_or_else(cursor_invalid)?;
        let intra_commit_index = parts.next().ok_or_else(cursor_invalid)?;
        if parts.next().is_some() || version != CURSOR_VERSION {
            return Err(cursor_invalid());
        }

        let workspace = WorkspaceId::parse(workspace).map_err(|_| cursor_invalid())?;
        let branch_bytes = hex::decode(branch_hex).map_err(|_| cursor_invalid())?;
        let branch = String::from_utf8(branch_bytes).map_err(|_| cursor_invalid())?;
        validate_branch(&branch).map_err(|_| cursor_invalid())?;
        let commit = if commit == NO_COMMIT {
            None
        } else {
            Some(Digest::parse(commit).map_err(|_| cursor_invalid())?)
        };
        let intra_commit_index = intra_commit_index
            .parse::<u32>()
            .map_err(|_| cursor_invalid())?;
        Self::new(workspace, branch, commit, intra_commit_index)
    }

    fn encode_v2(&self) -> String {
        let value = Value::Array(vec![
            Value::Uint(2),
            Value::Text(self.workspace.to_string()),
            Value::Text(self.branch.clone()),
            self.commit
                .map(|digest| Value::Text(digest.to_string()))
                .unwrap_or(Value::Null),
            Value::Uint(self.intra_commit_index as u64),
            self.facet
                .map(|facet| Value::Text(facet.as_str().to_string()))
                .unwrap_or(Value::Null),
            self.path_prefix
                .as_ref()
                .map(|prefix| Value::Text(prefix.clone()))
                .unwrap_or(Value::Null),
            Value::Array(
                self.change_kinds
                    .iter()
                    .map(|kind| Value::Text(change_kind_tag(*kind).to_string()))
                    .collect(),
            ),
        ]);
        format!(
            "{CURSOR_V2_PREFIX}{}",
            b64_url_no_pad(&loom_codec::encode(&value).expect("watch cursor CBOR is encodable"))
        )
    }

    fn decode_v2(payload: &str) -> Result<Self> {
        let bytes = b64_url_no_pad_decode(payload).ok_or_else(cursor_invalid)?;
        let Value::Array(mut fields) = loom_codec::decode(&bytes).map_err(|_| cursor_invalid())?
        else {
            return Err(cursor_invalid());
        };
        if fields.len() != 8 {
            return Err(cursor_invalid());
        }
        let change_kinds = take_array(fields.pop().expect("length checked"))?
            .into_iter()
            .map(take_change_kind)
            .collect::<Result<Vec<_>>>()?;
        let path_prefix = take_optional_text(fields.pop().expect("length checked"))?;
        let facet = take_optional_facet(fields.pop().expect("length checked"))?;
        let intra_commit_index = take_u32(fields.pop().expect("length checked"))?;
        let commit = take_optional_digest(fields.pop().expect("length checked"))?;
        let branch = take_text(fields.pop().expect("length checked"))?;
        let workspace = WorkspaceId::parse(&take_text(fields.pop().expect("length checked"))?)
            .map_err(|_| cursor_invalid())?;
        if !matches!(fields.pop(), Some(Value::Uint(2))) {
            return Err(cursor_invalid());
        }
        validate_branch(&branch).map_err(|_| cursor_invalid())?;
        let mut cursor = Self::new(workspace, branch, commit, intra_commit_index)?;
        cursor.facet = facet;
        cursor.path_prefix = path_prefix;
        cursor.change_kinds = change_kinds;
        canonicalize_change_kinds(&mut cursor.change_kinds);
        ensure_supported_cursor_selector(&cursor)?;
        Ok(cursor)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeEvent {
    pub workspace: WorkspaceId,
    pub branch: String,
    pub commit: Digest,
    pub parent: Option<Digest>,
    pub seq: u64,
    pub changes: Vec<DomainChange>,
    pub unsupported_domains: Vec<UnsupportedDomainDetail>,
    pub path_changes: Vec<WatchPathChange>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainChange {
    pub domain: String,
    pub schema_version: u32,
    pub kind: String,
    pub key: Vec<u8>,
    pub before: Option<Digest>,
    pub after: Option<Digest>,
    pub detail: Option<Vec<u8>>,
}

impl DomainChange {
    pub fn file(
        path: String,
        kind: ChangeKind,
        before: Option<Digest>,
        after: Option<Digest>,
    ) -> Self {
        Self {
            domain: FILES_DOMAIN.to_string(),
            schema_version: FILES_DOMAIN_CHANGE_SCHEMA_VERSION,
            kind: change_kind_tag(kind).to_string(),
            key: path.into_bytes(),
            before,
            after,
            detail: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsupportedDomainDetail {
    pub domain: String,
    pub capability: String,
}

impl UnsupportedDomainDetail {
    pub fn from_facet(facet: FacetKind) -> Option<Self> {
        let support = watch_domain_support(facet)?;
        (support.detail == WatchDomainDetail::Unsupported).then(|| Self {
            domain: support.domain.to_string(),
            capability: support.capability.to_string(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchPathChange {
    pub path: String,
    pub kind: ChangeKind,
}

impl WatchPathChange {
    pub fn file(path: String, kind: ChangeKind) -> Self {
        Self { path, kind }
    }
}

pub fn ensure_supported_selector(selector: &WatchSelector) -> Result<()> {
    if let Some(facet) = selector.facet {
        ensure_supported_facet(facet)?;
    }
    Ok(())
}

pub fn ensure_supported_cursor_selector(cursor: &WatchCursor) -> Result<()> {
    if let Some(facet) = cursor.facet {
        ensure_supported_facet(facet)?;
    }
    Ok(())
}

pub fn watch_domain_for_path(path: &str) -> Option<FacetKind> {
    let Some(rest) = path.strip_prefix(".loom/facets") else {
        return Some(FacetKind::Files);
    };
    let rest = rest.strip_prefix('/')?;
    let domain = rest.split('/').next()?;
    FacetKind::parse(domain).ok()
}

fn cbor_pair(key: &str, value: Value) -> (Value, Value) {
    (Value::Text(key.to_string()), value)
}

fn digest_cbor(digest: Digest) -> Value {
    Value::Text(digest.to_string())
}

fn optional_digest_cbor(digest: Option<Digest>) -> Value {
    digest.map_or(Value::Null, digest_cbor)
}

fn domain_change_cbor(change: &DomainChange) -> Value {
    Value::Map(vec![
        cbor_pair("domain", Value::Text(change.domain.clone())),
        cbor_pair(
            "schema_version",
            Value::Uint(u64::from(change.schema_version)),
        ),
        cbor_pair("kind", Value::Text(change.kind.clone())),
        cbor_pair("key", Value::Bytes(change.key.clone())),
        cbor_pair("before", optional_digest_cbor(change.before)),
        cbor_pair("after", optional_digest_cbor(change.after)),
        cbor_pair(
            "detail",
            change.detail.clone().map_or(Value::Null, Value::Bytes),
        ),
    ])
}

fn unsupported_domain_cbor(domain: &UnsupportedDomainDetail) -> Value {
    Value::Map(vec![
        cbor_pair("domain", Value::Text(domain.domain.clone())),
        cbor_pair("capability", Value::Text(domain.capability.clone())),
    ])
}

fn change_event_cbor(event: &ChangeEvent) -> Value {
    Value::Map(vec![
        cbor_pair(
            "workspace",
            Value::Bytes(event.workspace.as_bytes().to_vec()),
        ),
        cbor_pair("ref", Value::Text(event.branch.clone())),
        cbor_pair("commit", digest_cbor(event.commit)),
        cbor_pair("parent", optional_digest_cbor(event.parent)),
        cbor_pair("seq", Value::Uint(event.seq)),
        (
            Value::Text("changes".to_string()),
            Value::Array(event.changes.iter().map(domain_change_cbor).collect()),
        ),
        (
            Value::Text("unsupported_domains".to_string()),
            Value::Array(
                event
                    .unsupported_domains
                    .iter()
                    .map(unsupported_domain_cbor)
                    .collect(),
            ),
        ),
    ])
}

/// Decode a canonical watch batch (`loom.watch.batch.v1`) produced by [`watch_batch_to_cbor`] back into
/// a [`WatchBatch`]. This is the symmetric inverse of the encoder and is used by remote clients (the MCP
/// host) that receive the batch bytes over the wire and must rebuild the typed batch to project it into
/// their own summary shapes.
pub fn watch_batch_from_cbor(bytes: &[u8]) -> Result<WatchBatch> {
    let value = loom_codec::decode(bytes)
        .map_err(|err| LoomError::new(Code::CorruptObject, format!("cbor: {err}")))?;
    let entries = expect_map(&value, "watch batch")?;
    let events = match map_field(entries, "events") {
        Some(Value::Array(items)) => items
            .iter()
            .map(change_event_from_cbor)
            .collect::<Result<Vec<_>>>()?,
        _ => return Err(LoomError::invalid("watch batch events must be an array")),
    };
    let next = match map_field(entries, "next") {
        Some(Value::Text(cursor)) => WatchCursor::decode(cursor)?,
        _ => {
            return Err(LoomError::invalid(
                "watch batch next must be a cursor string",
            ));
        }
    };
    Ok(WatchBatch { events, next })
}

/// Decode a single change event map (the inverse of [`change_event_cbor`]). A payload that predates the
/// `parent` field (or carries it as null) decodes with `parent: None`; `path_changes` is not part of the
/// wire form and decodes as empty.
pub fn change_event_from_cbor(value: &Value) -> Result<ChangeEvent> {
    let entries = expect_map(value, "watch change event")?;
    let workspace = match map_field(entries, "workspace") {
        Some(Value::Bytes(bytes)) => {
            let bytes: [u8; 16] = <[u8; 16]>::try_from(bytes.as_slice())
                .map_err(|_| LoomError::invalid("watch event workspace must be 16 bytes"))?;
            WorkspaceId::from_bytes(bytes)
        }
        _ => return Err(LoomError::invalid("watch event workspace must be bytes")),
    };
    let branch = expect_text(map_field(entries, "ref"), "watch event ref")?;
    let commit = Digest::parse(&expect_text(
        map_field(entries, "commit"),
        "watch event commit",
    )?)?;
    let parent = optional_digest_from(map_field(entries, "parent"))?;
    let seq = match map_field(entries, "seq") {
        Some(Value::Uint(seq)) => *seq,
        _ => return Err(LoomError::invalid("watch event seq must be a uint")),
    };
    let changes = match map_field(entries, "changes") {
        Some(Value::Array(items)) => items
            .iter()
            .map(domain_change_from_cbor)
            .collect::<Result<Vec<_>>>()?,
        _ => return Err(LoomError::invalid("watch event changes must be an array")),
    };
    let unsupported_domains = match map_field(entries, "unsupported_domains") {
        Some(Value::Array(items)) => items
            .iter()
            .map(unsupported_domain_from_cbor)
            .collect::<Result<Vec<_>>>()?,
        _ => {
            return Err(LoomError::invalid(
                "watch event unsupported_domains must be an array",
            ));
        }
    };
    Ok(ChangeEvent {
        workspace,
        branch,
        commit,
        parent,
        seq,
        changes,
        unsupported_domains,
        path_changes: Vec::new(),
    })
}

fn domain_change_from_cbor(value: &Value) -> Result<DomainChange> {
    let entries = expect_map(value, "watch domain change")?;
    let domain = expect_text(map_field(entries, "domain"), "domain change domain")?;
    let schema_version = match map_field(entries, "schema_version") {
        Some(Value::Uint(version)) => u32::try_from(*version)
            .map_err(|_| LoomError::invalid("domain change schema_version out of range"))?,
        _ => {
            return Err(LoomError::invalid(
                "domain change schema_version must be a uint",
            ));
        }
    };
    let kind = expect_text(map_field(entries, "kind"), "domain change kind")?;
    let key = match map_field(entries, "key") {
        Some(Value::Bytes(key)) => key.clone(),
        _ => return Err(LoomError::invalid("domain change key must be bytes")),
    };
    let before = optional_digest_from(map_field(entries, "before"))?;
    let after = optional_digest_from(map_field(entries, "after"))?;
    let detail = match map_field(entries, "detail") {
        Some(Value::Bytes(detail)) => Some(detail.clone()),
        Some(Value::Null) | None => None,
        _ => {
            return Err(LoomError::invalid(
                "domain change detail must be bytes or null",
            ));
        }
    };
    Ok(DomainChange {
        domain,
        schema_version,
        kind,
        key,
        before,
        after,
        detail,
    })
}

fn unsupported_domain_from_cbor(value: &Value) -> Result<UnsupportedDomainDetail> {
    let entries = expect_map(value, "unsupported domain")?;
    Ok(UnsupportedDomainDetail {
        domain: expect_text(map_field(entries, "domain"), "unsupported domain domain")?,
        capability: expect_text(
            map_field(entries, "capability"),
            "unsupported domain capability",
        )?,
    })
}

fn expect_map<'a>(value: &'a Value, what: &str) -> Result<&'a [(Value, Value)]> {
    match value {
        Value::Map(entries) => Ok(entries),
        _ => Err(LoomError::invalid(format!("{what} must be a map"))),
    }
}

fn map_field<'a>(entries: &'a [(Value, Value)], key: &str) -> Option<&'a Value> {
    entries
        .iter()
        .find(|(k, _)| matches!(k, Value::Text(text) if text == key))
        .map(|(_, v)| v)
}

fn expect_text(value: Option<&Value>, what: &str) -> Result<String> {
    match value {
        Some(Value::Text(text)) => Ok(text.clone()),
        _ => Err(LoomError::invalid(format!("{what} must be text"))),
    }
}

fn optional_digest_from(value: Option<&Value>) -> Result<Option<Digest>> {
    match value {
        Some(Value::Text(text)) => Ok(Some(Digest::parse(text)?)),
        Some(Value::Null) | None => Ok(None),
        _ => Err(LoomError::invalid(
            "watch digest field must be text or null",
        )),
    }
}

fn validate_branch(branch: &str) -> Result<()> {
    if branch.is_empty() {
        return Err(LoomError::invalid("watch branch must not be empty"));
    }
    Ok(())
}

fn ensure_supported_facet(facet: FacetKind) -> Result<()> {
    match watch_domain_support(facet) {
        Some(support) if support.detail == WatchDomainDetail::Stable => Ok(()),
        Some(support) => Err(LoomError::new(
            Code::Unsupported,
            format!(
                "watch detail for domain '{}' requires unsupported capability '{}'",
                support.domain, support.capability
            ),
        )),
        None => Err(LoomError::new(
            Code::Unsupported,
            format!("watch domain '{}' is not an event domain", facet.as_str()),
        )),
    }
}

fn canonicalize_change_kinds(kinds: &mut Vec<ChangeKind>) {
    kinds.sort_by_key(|kind| kind.stable_tag());
    kinds.dedup();
}

fn change_kind_tag(kind: ChangeKind) -> &'static str {
    match kind {
        ChangeKind::Added => "added",
        ChangeKind::Modified => "modified",
        ChangeKind::Deleted => "deleted",
    }
}

fn change_kind_from_tag(tag: &str) -> Result<ChangeKind> {
    match tag {
        "added" => Ok(ChangeKind::Added),
        "modified" => Ok(ChangeKind::Modified),
        "deleted" => Ok(ChangeKind::Deleted),
        _ => Err(cursor_invalid()),
    }
}

fn take_array(value: Value) -> Result<Vec<Value>> {
    match value {
        Value::Array(items) => Ok(items),
        _ => Err(cursor_invalid()),
    }
}

fn take_text(value: Value) -> Result<String> {
    match value {
        Value::Text(text) => Ok(text),
        _ => Err(cursor_invalid()),
    }
}

fn take_optional_text(value: Value) -> Result<Option<String>> {
    match value {
        Value::Null => Ok(None),
        Value::Text(text) => Ok(Some(text)),
        _ => Err(cursor_invalid()),
    }
}

fn take_optional_facet(value: Value) -> Result<Option<FacetKind>> {
    take_optional_text(value)?
        .map(|facet| FacetKind::parse(&facet).map_err(|_| cursor_invalid()))
        .transpose()
}

fn take_optional_digest(value: Value) -> Result<Option<Digest>> {
    take_optional_text(value)?
        .map(|digest| Digest::parse(&digest).map_err(|_| cursor_invalid()))
        .transpose()
}

fn take_change_kind(value: Value) -> Result<ChangeKind> {
    change_kind_from_tag(&take_text(value)?)
}

fn take_u32(value: Value) -> Result<u32> {
    match value {
        Value::Uint(value) => u32::try_from(value).map_err(|_| cursor_invalid()),
        _ => Err(cursor_invalid()),
    }
}

fn b64_url_no_pad(bytes: &[u8]) -> String {
    let mut out = String::with_capacity((bytes.len() * 4).div_ceil(3));
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);
        out.push(B64_URL[(b0 >> 2) as usize] as char);
        out.push(B64_URL[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            out.push(B64_URL[(((b1 & 0x0f) << 2) | (b2 >> 6)) as usize] as char);
        }
        if chunk.len() > 2 {
            out.push(B64_URL[(b2 & 0x3f) as usize] as char);
        }
    }
    out
}

fn b64_url_no_pad_decode(input: &str) -> Option<Vec<u8>> {
    if input.len() % 4 == 1 {
        return None;
    }
    let mut bits = 0u32;
    let mut bit_len = 0u8;
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    for byte in input.bytes() {
        let value = b64_url_value(byte)? as u32;
        bits = (bits << 6) | value;
        bit_len += 6;
        while bit_len >= 8 {
            bit_len -= 8;
            out.push((bits >> bit_len) as u8);
            bits &= (1 << bit_len) - 1;
        }
    }
    if bit_len > 0 && bits != 0 {
        return None;
    }
    Some(out)
}

fn b64_url_value(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'-' => Some(62),
        b'_' => Some(63),
        _ => None,
    }
}

fn cursor_invalid() -> LoomError {
    LoomError::new(Code::CursorInvalid, "invalid watch cursor")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_round_trips_without_commit() {
        let workspace = WorkspaceId::v4_from_bytes([1u8; 16]);
        let cursor = WatchCursor::new(workspace, "main", None, 0).unwrap();
        let encoded = cursor.encode();

        assert_eq!(WatchCursor::decode(&encoded).unwrap(), cursor);
    }

    #[test]
    fn selector_filters_round_trip_through_v2_cursor() {
        let workspace = WorkspaceId::v4_from_bytes([2u8; 16]);
        let selector = WatchSelector::new(workspace, "main")
            .unwrap()
            .with_facet(FacetKind::Files)
            .with_path_prefix("src/")
            .with_change_kind(ChangeKind::Modified)
            .with_change_kind(ChangeKind::Added);
        let cursor = WatchCursor::from_selector(&selector, Some(Digest::blake3(b"commit")));
        let decoded = WatchCursor::decode(&cursor.encode()).unwrap();

        assert_eq!(decoded, cursor);
        assert_eq!(
            decoded.change_kinds,
            vec![ChangeKind::Added, ChangeKind::Modified]
        );
    }

    #[test]
    fn unsupported_domain_detail_is_capability_labeled() {
        assert_eq!(
            watch_domain_support(FacetKind::Files).unwrap().detail,
            WatchDomainDetail::Stable
        );
        assert_eq!(
            UnsupportedDomainDetail::from_facet(FacetKind::Kv).unwrap(),
            UnsupportedDomainDetail {
                domain: "kv".to_string(),
                capability: "watch.domain.kv".to_string(),
            }
        );
    }

    #[test]
    fn batch_encodes_as_canonical_cbor_payload() {
        let workspace = WorkspaceId::v4_from_bytes([3u8; 16]);
        let commit = Digest::blake3(b"commit");
        let cursor = WatchCursor::new(workspace, "main", Some(commit), 0).unwrap();
        let batch = WatchBatch {
            events: vec![ChangeEvent {
                workspace,
                branch: "main".to_string(),
                commit,
                parent: None,
                seq: 1,
                changes: vec![DomainChange::file(
                    "a.txt".to_string(),
                    ChangeKind::Added,
                    None,
                    Some(commit),
                )],
                unsupported_domains: Vec::new(),
                path_changes: vec![WatchPathChange::file(
                    "a.txt".to_string(),
                    ChangeKind::Added,
                )],
            }],
            next: cursor,
        };

        let bytes = watch_batch_to_cbor(&batch).unwrap();
        assert!(!bytes.is_empty());
    }

    #[test]
    fn batch_round_trips_and_preserves_parent() {
        let workspace = WorkspaceId::v4_from_bytes([7u8; 16]);
        let parent = Digest::blake3(b"parent-commit");
        let commit = Digest::blake3(b"child-commit");
        let cursor = WatchCursor::new(workspace, "main", Some(commit), 0).unwrap();
        // `path_changes` is not part of the wire form, so build the event with it empty to allow a full
        // struct-equality assertion after the round-trip.
        let batch = WatchBatch {
            events: vec![ChangeEvent {
                workspace,
                branch: "main".to_string(),
                commit,
                parent: Some(parent),
                seq: 2,
                changes: vec![DomainChange::file(
                    "a.txt".to_string(),
                    ChangeKind::Modified,
                    Some(parent),
                    Some(commit),
                )],
                unsupported_domains: vec![UnsupportedDomainDetail {
                    domain: "kv".to_string(),
                    capability: "watch.domain.kv".to_string(),
                }],
                path_changes: Vec::new(),
            }],
            next: cursor,
        };

        let bytes = watch_batch_to_cbor(&batch).unwrap();
        let decoded = watch_batch_from_cbor(&bytes).unwrap();
        assert_eq!(decoded, batch);
        assert_eq!(decoded.events[0].parent, Some(parent));
    }

    #[test]
    fn batch_decode_tolerates_missing_parent() {
        // A payload that predates the `parent` field: the event map has no `parent` key at all.
        let workspace = WorkspaceId::v4_from_bytes([8u8; 16]);
        let commit = Digest::blake3(b"root-commit");
        let cursor = WatchCursor::new(workspace, "main", Some(commit), 0).unwrap();
        let legacy_event = Value::Map(vec![
            (
                Value::Text("workspace".to_string()),
                Value::Bytes(workspace.as_bytes().to_vec()),
            ),
            (
                Value::Text("ref".to_string()),
                Value::Text("main".to_string()),
            ),
            (
                Value::Text("commit".to_string()),
                Value::Text(commit.to_string()),
            ),
            (Value::Text("seq".to_string()), Value::Uint(0)),
            (Value::Text("changes".to_string()), Value::Array(Vec::new())),
            (
                Value::Text("unsupported_domains".to_string()),
                Value::Array(Vec::new()),
            ),
        ]);
        let legacy_batch = Value::Map(vec![
            (
                Value::Text("schema".to_string()),
                Value::Text("loom.watch.batch.v1".to_string()),
            ),
            (
                Value::Text("events".to_string()),
                Value::Array(vec![legacy_event]),
            ),
            (
                Value::Text("next".to_string()),
                Value::Text(cursor.encode()),
            ),
        ]);
        let bytes = loom_codec::encode(&legacy_batch).unwrap();
        let decoded = watch_batch_from_cbor(&bytes).unwrap();
        assert_eq!(decoded.events.len(), 1);
        assert_eq!(decoded.events[0].parent, None);
        assert_eq!(decoded.events[0].commit, commit);
    }
}
