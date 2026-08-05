use crate::digest::Digest;
use crate::error::{Code, LoomError, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::{Arc, RwLock};

const KEY_SCHEMA: &[u8] = b"loom.mutable-overlay.key.v1";
const TOKEN_SCHEMA: &[u8] = b"loom.mutable-overlay.owner-token.v1";
const OVERLAY_KEY_SEGMENT_COUNT: u8 = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum OverlayDurabilityPolicy {
    /// Acknowledged only after the commit record or equivalent WAL commit is fsynced.
    ///
    /// Recovered after process restart and after power loss when the backing storage honors fsync.
    /// Use this for audit, ledger, explicit VCS commit, sync checkpoint, and critical metadata
    /// boundaries.
    Strict,
    /// Appended to a WAL or mutable commit queue before acknowledgement, with fsync grouped or
    /// periodic.
    ///
    /// Recovered after process restart. A power loss may drop the latest acknowledged window, but
    /// recovery must not expose a torn or corrupt partial transaction. This is the default target for
    /// operational facets such as tickets, lanes, pages, documents, PIM, KV, and queue offsets.
    Normal,
    /// May rely on OS flushing for rebuildable or loss-tolerant state.
    ///
    /// Recovery is best effort. Recent acknowledged writes may be lost, but canonical state must not
    /// be corrupted. Use this for derived indexes, projections, and caches with backing data.
    Relaxed,
    /// No durable recovery guarantee.
    ///
    /// Entries may be lost across reopen, restart, power loss, export, or promotion. Use this only
    /// for session state, temporary caches, runtime observations, and volatile health data.
    Ephemeral,
}

impl OverlayDurabilityPolicy {
    pub const ALL: [Self; 4] = [Self::Strict, Self::Normal, Self::Relaxed, Self::Ephemeral];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Strict => "strict",
            Self::Normal => "normal",
            Self::Relaxed => "relaxed",
            Self::Ephemeral => "ephemeral",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "strict" => Ok(Self::Strict),
            "normal" => Ok(Self::Normal),
            "relaxed" => Ok(Self::Relaxed),
            "ephemeral" => Ok(Self::Ephemeral),
            _ => Err(LoomError::invalid("unknown overlay durability policy")),
        }
    }

    pub fn is_durable(self) -> bool {
        !matches!(self, Self::Ephemeral)
    }

    pub fn survives_process_restart(self) -> bool {
        matches!(self, Self::Strict | Self::Normal)
    }

    pub fn survives_power_loss_when_storage_honors_fsync(self) -> bool {
        matches!(self, Self::Strict)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct OverlayGeneration(u64);

impl OverlayGeneration {
    pub fn new(value: u64) -> Self {
        Self(value)
    }

    pub fn as_u64(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct OverlayKey(Vec<u8>);

impl OverlayKey {
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self> {
        validate_overlay_key_framing(&bytes)?;
        Ok(Self(bytes))
    }

    pub fn from_segments(segments: [&[u8]; OVERLAY_KEY_SEGMENT_COUNT as usize]) -> Result<Self> {
        let mut out = Vec::new();
        out.extend_from_slice(KEY_SCHEMA);
        out.push(segments.len() as u8);
        for segment in segments {
            if segment.len() > u32::MAX as usize {
                return Err(LoomError::invalid("overlay key segment too long"));
            }
            out.extend_from_slice(&(segment.len() as u32).to_be_bytes());
            out.extend_from_slice(segment);
        }
        Ok(Self(out))
    }

    pub fn prefix_from_segments<const N: usize>(
        total_segments: u8,
        segments: [&[u8]; N],
    ) -> Result<OverlayKeyPrefix> {
        if usize::from(total_segments) < N {
            return Err(LoomError::invalid(
                "overlay key prefix exceeds segment count",
            ));
        }
        let mut out = Vec::new();
        out.extend_from_slice(KEY_SCHEMA);
        out.push(total_segments);
        for segment in segments {
            if segment.len() > u32::MAX as usize {
                return Err(LoomError::invalid("overlay key segment too long"));
            }
            out.extend_from_slice(&(segment.len() as u32).to_be_bytes());
            out.extend_from_slice(segment);
        }
        OverlayKeyPrefix::from_encoded_bytes(out)
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn segments(&self) -> Result<Vec<&[u8]>> {
        if !self.0.starts_with(KEY_SCHEMA) {
            return Err(LoomError::corrupt("unknown overlay key schema"));
        }
        let mut pos = KEY_SCHEMA.len();
        let Some(count) = self.0.get(pos).copied() else {
            return Err(LoomError::corrupt("overlay key segment count missing"));
        };
        if count != OVERLAY_KEY_SEGMENT_COUNT {
            return Err(LoomError::corrupt("overlay key segment count mismatch"));
        }
        pos += 1;
        let mut segments = Vec::with_capacity(count as usize);
        for _ in 0..count {
            if pos + 4 > self.0.len() {
                return Err(LoomError::corrupt("overlay key segment length truncated"));
            }
            let len = u32::from_be_bytes(
                self.0[pos..pos + 4]
                    .try_into()
                    .map_err(|_| LoomError::corrupt("overlay key segment length invalid"))?,
            ) as usize;
            pos += 4;
            if pos + len > self.0.len() {
                return Err(LoomError::corrupt("overlay key segment truncated"));
            }
            segments.push(&self.0[pos..pos + len]);
            pos += len;
        }
        if pos != self.0.len() {
            return Err(LoomError::corrupt("overlay key has trailing bytes"));
        }
        Ok(segments)
    }

    pub fn from_encoded_bytes(bytes: Vec<u8>) -> Result<Self> {
        validate_overlay_key_framing(&bytes)?;
        Ok(Self(bytes))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayKeyPrefix(Vec<u8>);

impl OverlayKeyPrefix {
    pub fn from_encoded_bytes(bytes: Vec<u8>) -> Result<Self> {
        validate_overlay_key_prefix_framing(&bytes)?;
        Ok(Self(bytes))
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    fn start_key(&self) -> Result<OverlayKey> {
        Ok(OverlayKey(self.0.clone()))
    }

    fn end_key(&self) -> Result<Option<OverlayKey>> {
        Ok(byte_prefix_successor(&self.0).map(OverlayKey))
    }
}

fn validate_overlay_key_framing(bytes: &[u8]) -> Result<()> {
    let parsed = validate_overlay_key_common_framing(bytes, "overlay key")?;
    if parsed != usize::from(OVERLAY_KEY_SEGMENT_COUNT) {
        return Err(LoomError::corrupt("overlay key segment count mismatch"));
    }
    Ok(())
}

fn validate_overlay_key_prefix_framing(bytes: &[u8]) -> Result<()> {
    let parsed = validate_overlay_key_common_framing(bytes, "overlay key prefix")?;
    if parsed == 0 {
        return Err(LoomError::corrupt("overlay key prefix has no segments"));
    }
    Ok(())
}

fn validate_overlay_key_common_framing(bytes: &[u8], label: &str) -> Result<usize> {
    if !bytes.starts_with(KEY_SCHEMA) {
        return Err(LoomError::corrupt(format!("unknown {label} schema")));
    }
    let mut pos = KEY_SCHEMA.len();
    let Some(count) = bytes.get(pos).copied() else {
        return Err(LoomError::corrupt(format!("{label} segment count missing")));
    };
    if count != OVERLAY_KEY_SEGMENT_COUNT {
        return Err(LoomError::corrupt(format!(
            "{label} segment count mismatch"
        )));
    }
    pos += 1;
    let mut parsed = 0usize;
    while pos < bytes.len() {
        if parsed >= usize::from(count) {
            return Err(LoomError::corrupt(format!("{label} has too many segments")));
        }
        if pos + 4 > bytes.len() {
            return Err(LoomError::corrupt(format!(
                "{label} segment length truncated"
            )));
        }
        let len = u32::from_be_bytes(
            bytes[pos..pos + 4]
                .try_into()
                .map_err(|_| LoomError::corrupt(format!("{label} segment length invalid")))?,
        ) as usize;
        pos += 4;
        if pos + len > bytes.len() {
            return Err(LoomError::corrupt(format!("{label} segment truncated")));
        }
        pos += len;
        parsed += 1;
    }
    Ok(parsed)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayOwnerToken([u8; 32]);

impl OverlayOwnerToken {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayEntryKind {
    Value,
    Tombstone,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OverlayEntry {
    generation: OverlayGeneration,
    key: OverlayKey,
    owner_token: OverlayOwnerToken,
    kind: OverlayEntryKind,
    payload: Vec<u8>,
}

#[derive(Debug, Clone, Default)]
pub struct MutableOverlay {
    generation: u64,
    entries: Arc<RwLock<Vec<OverlayEntry>>>,
    owner_tokens: BTreeMap<OverlayKey, OverlayOwnerToken>,
    current_entry_index: Arc<RwLock<BTreeMap<OverlayKey, Vec<usize>>>>,
    parent: Option<OverlaySnapshot>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MutableOverlayHealth {
    pub current_generation: u64,
    pub current_record_count: u64,
    pub tombstone_count: u64,
    pub live_checkpoint_references: u64,
    pub reclaimable_overlay_pages: u64,
    pub blocked_reclamation_reasons: Vec<String>,
    pub hot_write_count: u64,
    pub active_writer_contention_indicators: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutableOverlayEntrySnapshot {
    pub generation: OverlayGeneration,
    pub key: OverlayKey,
    pub owner_token: OverlayOwnerToken,
    pub kind: OverlayEntryKind,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct OverlayOwnerScope {
    scope_kind: Vec<u8>,
    scope_id: Vec<u8>,
    domain: Vec<u8>,
}

impl OverlayOwnerScope {
    pub fn new(
        scope_kind: impl Into<Vec<u8>>,
        scope_id: impl Into<Vec<u8>>,
        domain: impl Into<Vec<u8>>,
    ) -> Result<Self> {
        let scope = Self {
            scope_kind: scope_kind.into(),
            scope_id: scope_id.into(),
            domain: domain.into(),
        };
        if scope.scope_kind.is_empty() || scope.scope_id.is_empty() || scope.domain.is_empty() {
            return Err(LoomError::invalid("overlay owner scope segment is empty"));
        }
        Ok(scope)
    }

    pub fn from_key(key: &OverlayKey) -> Result<Self> {
        let segments = key.segments()?;
        if segments.len() != 6 {
            return Err(LoomError::corrupt("overlay key owner scope shape mismatch"));
        }
        Self::new(segments[0], segments[1], segments[2])
    }

    pub fn scope_kind(&self) -> &[u8] {
        &self.scope_kind
    }

    pub fn scope_id(&self) -> &[u8] {
        &self.scope_id
    }

    pub fn domain(&self) -> &[u8] {
        &self.domain
    }

    fn matches_key(&self, key: &OverlayKey) -> Result<bool> {
        let segments = key.segments()?;
        if segments.len() != 6 {
            return Err(LoomError::corrupt("overlay key owner scope shape mismatch"));
        }
        Ok(segments[0] == self.scope_kind
            && segments[1] == self.scope_id
            && segments[2] == self.domain)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayPromotionEntry {
    pub key: OverlayKey,
    pub owner_scope: OverlayOwnerScope,
    pub owner_token: OverlayOwnerToken,
    pub kind: OverlayEntryKind,
    pub payload: Vec<u8>,
    pub generation: OverlayGeneration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayPromotionSelection {
    pub generation: OverlayGeneration,
    pub owner_scopes: Vec<OverlayOwnerScope>,
    pub entries: Vec<OverlayPromotionEntry>,
}

#[derive(Debug, Clone)]
pub struct OverlayCheckpoint {
    generation: OverlayGeneration,
    keys: Vec<OverlayKey>,
    snapshot: OverlaySnapshot,
}

impl OverlayCheckpoint {
    pub fn generation(&self) -> OverlayGeneration {
        self.generation
    }

    pub fn keys(&self) -> &[OverlayKey] {
        &self.keys
    }

    pub fn read_composite(
        &self,
        key: &OverlayKey,
        base_read: impl FnOnce(&OverlayKey) -> Result<Option<Vec<u8>>>,
    ) -> Result<Option<Vec<u8>>> {
        self.snapshot.read_composite(key, base_read)
    }

    pub fn select_owner_scopes(
        &self,
        owner_scopes: &[OverlayOwnerScope],
    ) -> Result<OverlayPromotionSelection> {
        self.snapshot.select_owner_scopes(owner_scopes)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverlayReadSnapshotIdentity {
    pub overlay_generation: OverlayGeneration,
    pub immutable_base_root: Option<Digest>,
}

pub trait OverlaySnapshotPin: fmt::Debug + Send + Sync {
    fn release(&self) -> Result<bool>;
}

#[derive(Debug)]
pub struct OverlayReadSnapshot {
    identity: OverlayReadSnapshotIdentity,
    snapshot: OverlaySnapshot,
    pin: Option<Box<dyn OverlaySnapshotPin>>,
}

impl OverlayReadSnapshot {
    pub fn new(
        snapshot: OverlaySnapshot,
        immutable_base_root: Option<Digest>,
        pin: Option<Box<dyn OverlaySnapshotPin>>,
    ) -> Self {
        let identity = OverlayReadSnapshotIdentity {
            overlay_generation: snapshot.generation(),
            immutable_base_root,
        };
        Self {
            identity,
            snapshot,
            pin,
        }
    }

    pub fn identity(&self) -> OverlayReadSnapshotIdentity {
        self.identity
    }

    pub fn overlay_generation(&self) -> OverlayGeneration {
        self.identity.overlay_generation
    }

    pub fn immutable_base_root(&self) -> Option<Digest> {
        self.identity.immutable_base_root
    }

    pub fn fork_overlay(&self) -> MutableOverlay {
        MutableOverlay::fork_from_snapshot(self.snapshot.clone())
    }

    pub fn owner_token(&self, key: &OverlayKey) -> Result<Option<OverlayOwnerToken>> {
        self.snapshot.owner_token(key)
    }

    pub fn read_composite(
        &self,
        key: &OverlayKey,
        base_read: impl FnOnce(Option<Digest>, &OverlayKey) -> Result<Option<Vec<u8>>>,
    ) -> Result<Option<Vec<u8>>> {
        let base_root = self.identity.immutable_base_root;
        self.snapshot
            .read_composite(key, |key| base_read(base_root, key))
    }

    pub fn release(&self) -> Result<bool> {
        match self.pin.as_ref() {
            Some(pin) => pin.release(),
            None => Ok(false),
        }
    }
}

impl Drop for OverlayReadSnapshot {
    fn drop(&mut self) {
        if let Some(pin) = self.pin.as_ref() {
            let _ = pin.release();
        }
    }
}

impl MutableOverlay {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn fork_from_snapshot(snapshot: OverlaySnapshot) -> Self {
        Self {
            generation: snapshot.generation().as_u64(),
            entries: Arc::new(RwLock::new(Vec::new())),
            owner_tokens: BTreeMap::new(),
            current_entry_index: Arc::new(RwLock::new(BTreeMap::new())),
            parent: Some(snapshot),
        }
    }

    pub fn generation(&self) -> OverlayGeneration {
        OverlayGeneration(self.generation)
    }

    pub fn snapshot(&self) -> OverlaySnapshot {
        OverlaySnapshot {
            generation: self.generation(),
            entries: Arc::clone(&self.entries),
            current_entry_index: Arc::clone(&self.current_entry_index),
            parent: self.parent.clone().map(Arc::new),
        }
    }

    pub fn checkpoint(&self) -> OverlayCheckpoint {
        OverlayCheckpoint {
            generation: self.generation(),
            keys: self.owner_tokens.keys().cloned().collect(),
            snapshot: self.snapshot(),
        }
    }

    pub fn checkpoint_with_inherited_keys(&self) -> Result<OverlayCheckpoint> {
        let snapshot = self.snapshot();
        Ok(OverlayCheckpoint {
            generation: self.generation(),
            keys: snapshot.current_keys()?,
            snapshot,
        })
    }

    pub fn validate_checkpoint(&self, checkpoint: &OverlayCheckpoint) -> Result<()> {
        if self.generation() == checkpoint.generation {
            Ok(())
        } else {
            Err(LoomError::new(
                Code::Conflict,
                "overlay checkpoint generation is stale",
            ))
        }
    }

    pub fn health(&self) -> Result<MutableOverlayHealth> {
        let entries = self.entries.read().map_err(|_| entry_storage_poisoned())?;
        let tombstone_count = entries
            .iter()
            .filter(|entry| entry.kind == OverlayEntryKind::Tombstone)
            .count() as u64;
        Ok(MutableOverlayHealth {
            current_generation: self.generation,
            current_record_count: self.owner_tokens.len() as u64,
            tombstone_count,
            live_checkpoint_references: 0,
            reclaimable_overlay_pages: 0,
            blocked_reclamation_reasons: Vec::new(),
            hot_write_count: entries.len() as u64,
            active_writer_contention_indicators: 0,
        })
    }

    pub fn export_entries(&self) -> Result<Vec<MutableOverlayEntrySnapshot>> {
        self.export_entries_with_progress(|_, _| {})
    }

    pub fn export_entries_with_progress(
        &self,
        mut progress: impl FnMut(u64, u64),
    ) -> Result<Vec<MutableOverlayEntrySnapshot>> {
        let entries = self.snapshot().entries()?;
        let total = entries.len() as u64;
        let mut snapshots = Vec::with_capacity(entries.len());
        for (index, entry) in entries.into_iter().enumerate() {
            progress(index as u64, total);
            snapshots.push(MutableOverlayEntrySnapshot {
                generation: entry.generation,
                key: entry.key,
                owner_token: entry.owner_token,
                kind: entry.kind,
                payload: entry.payload,
            });
        }
        progress(total, total);
        Ok(snapshots)
    }

    pub fn export_entries_with_key_prefix(
        &self,
        prefix: &OverlayKeyPrefix,
    ) -> Result<Vec<MutableOverlayEntrySnapshot>> {
        let mut entries = BTreeMap::new();
        if let Some(parent) = &self.parent {
            for entry in parent.current_entries_with_key_prefix(prefix)? {
                entries.insert(entry.key.clone(), entry);
            }
        }
        let start = prefix.start_key()?;
        match prefix.end_key()? {
            Some(end) => {
                let local_entries = self.entries.read().map_err(|_| entry_storage_poisoned())?;
                let current_entry_index = self
                    .current_entry_index
                    .read()
                    .map_err(|_| entry_storage_poisoned())?;
                for (key, history) in current_entry_index.range(start..end) {
                    if let Some(entry) =
                        latest_visible_entry(&local_entries, history, self.generation())
                    {
                        entries.insert(key.clone(), entry);
                    }
                }
            }
            None => {
                let local_entries = self.entries.read().map_err(|_| entry_storage_poisoned())?;
                let current_entry_index = self
                    .current_entry_index
                    .read()
                    .map_err(|_| entry_storage_poisoned())?;
                for (key, history) in current_entry_index.range(start..) {
                    if let Some(entry) =
                        latest_visible_entry(&local_entries, history, self.generation())
                    {
                        entries.insert(key.clone(), entry);
                    }
                }
            }
        }
        Ok(entries.into_values().collect())
    }

    pub fn import_entries(entries: &[MutableOverlayEntrySnapshot]) -> Result<Self> {
        Self::import_entries_with_progress(entries, |_, _| {})
    }

    pub fn import_entries_with_progress(
        entries: &[MutableOverlayEntrySnapshot],
        mut progress: impl FnMut(u64, u64),
    ) -> Result<Self> {
        let mut overlay = Self::new();
        let total = entries.len() as u64;
        let mut seen = BTreeSet::new();
        for (index, entry) in entries.iter().enumerate() {
            progress(index as u64, total);
            let generation = if entry.generation.0 == 0 {
                overlay
                    .generation
                    .checked_add(1)
                    .ok_or_else(|| LoomError::invalid("overlay generation overflow"))?
            } else {
                entry.generation.0
            };
            if generation < overlay.generation {
                return Err(LoomError::invalid(
                    "overlay import entries must not decrease generations",
                ));
            }
            if !seen.insert((generation, entry.key.clone())) {
                return Err(LoomError::invalid(
                    "overlay import entries must not repeat a key in one generation",
                ));
            }
            overlay.generation = overlay.generation.max(generation);
            let imported = OverlayEntry {
                generation: OverlayGeneration(overlay.generation),
                key: entry.key.clone(),
                owner_token: entry.owner_token.clone(),
                kind: entry.kind,
                payload: entry.payload.clone(),
            };
            overlay
                .owner_tokens
                .insert(imported.key.clone(), imported.owner_token.clone());
            let index_key = imported.key.clone();
            let mut overlay_entries = overlay
                .entries
                .write()
                .map_err(|_| entry_storage_poisoned())?;
            let entry_index = overlay_entries.len();
            overlay_entries.push(imported);
            overlay
                .current_entry_index
                .write()
                .map_err(|_| entry_storage_poisoned())?
                .entry(index_key)
                .or_default()
                .push(entry_index);
        }
        progress(total, total);
        Ok(overlay)
    }

    pub fn set_generation_floor(&mut self, generation: u64) {
        self.generation = self.generation.max(generation);
    }

    pub fn put_value(
        &mut self,
        key: OverlayKey,
        expected_owner_token: Option<&OverlayOwnerToken>,
        payload: impl Into<Vec<u8>>,
    ) -> Result<OverlayOwnerToken> {
        self.write(
            key,
            expected_owner_token,
            OverlayEntryKind::Value,
            payload.into(),
        )
    }

    pub fn put_tombstone(
        &mut self,
        key: OverlayKey,
        expected_owner_token: Option<&OverlayOwnerToken>,
    ) -> Result<OverlayOwnerToken> {
        self.write(
            key,
            expected_owner_token,
            OverlayEntryKind::Tombstone,
            Vec::new(),
        )
    }

    pub fn put_entries_in_next_generation(
        &mut self,
        writes: Vec<(
            OverlayKey,
            Option<OverlayOwnerToken>,
            OverlayEntryKind,
            Vec<u8>,
        )>,
    ) -> Result<Vec<OverlayOwnerToken>> {
        if writes.is_empty() {
            return Ok(Vec::new());
        }
        let mut seen = BTreeSet::new();
        for (key, expected_owner_token, _, _) in &writes {
            if !seen.insert(key.clone()) {
                return Err(LoomError::invalid(
                    "overlay transaction cannot write the same key twice",
                ));
            }
            let current = self.current_owner_token(key)?;
            if current.as_ref() != expected_owner_token.as_ref() {
                return Err(LoomError::new(
                    Code::Conflict,
                    "overlay owner token does not match current record",
                ));
            }
        }
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or_else(|| LoomError::invalid("overlay generation overflow"))?;
        let generation = self.generation();
        let mut owner_tokens = Vec::with_capacity(writes.len());
        let mut entries = self.entries.write().map_err(|_| entry_storage_poisoned())?;
        let mut current_entry_index = self
            .current_entry_index
            .write()
            .map_err(|_| entry_storage_poisoned())?;
        for (key, expected_owner_token, kind, payload) in writes {
            let owner_token = owner_token(&key, expected_owner_token.as_ref(), kind, &payload);
            let entry_index = entries.len();
            entries.push(OverlayEntry {
                generation,
                key: key.clone(),
                owner_token: owner_token.clone(),
                kind,
                payload,
            });
            current_entry_index
                .entry(key.clone())
                .or_default()
                .push(entry_index);
            self.owner_tokens.insert(key, owner_token.clone());
            owner_tokens.push(owner_token);
        }
        Ok(owner_tokens)
    }

    fn write(
        &mut self,
        key: OverlayKey,
        expected_owner_token: Option<&OverlayOwnerToken>,
        kind: OverlayEntryKind,
        payload: Vec<u8>,
    ) -> Result<OverlayOwnerToken> {
        let current = self.current_owner_token(&key)?;
        if current.as_ref() != expected_owner_token {
            return Err(LoomError::new(
                Code::Conflict,
                "overlay owner token does not match current record",
            ));
        }
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or_else(|| LoomError::invalid("overlay generation overflow"))?;
        let generation = self.generation();
        let owner_token = owner_token(&key, current.as_ref(), kind, &payload);
        let entry = OverlayEntry {
            generation,
            key: key.clone(),
            owner_token: owner_token.clone(),
            kind,
            payload,
        };
        let mut entries = self.entries.write().map_err(|_| entry_storage_poisoned())?;
        let entry_index = entries.len();
        entries.push(entry);
        self.current_entry_index
            .write()
            .map_err(|_| entry_storage_poisoned())?
            .entry(key.clone())
            .or_default()
            .push(entry_index);
        self.owner_tokens.insert(key, owner_token.clone());
        Ok(owner_token)
    }

    pub fn current_entry(&self, key: &OverlayKey) -> Option<MutableOverlayEntrySnapshot> {
        self.entries
            .read()
            .ok()
            .zip(self.current_entry_index.read().ok())
            .and_then(|(entries, current_entry_index)| {
                current_entry_index
                    .get(key)
                    .and_then(|history| latest_visible_entry(&entries, history, self.generation()))
            })
            .or_else(|| {
                self.parent
                    .as_ref()
                    .and_then(|parent| parent.current_entry(key).ok().flatten())
            })
    }

    pub fn synchronize_current_entry(&mut self, entry: MutableOverlayEntrySnapshot) -> Result<()> {
        if self
            .current_entry(&entry.key)
            .is_some_and(|current| current.owner_token == entry.owner_token)
        {
            self.generation = self.generation.max(entry.generation.as_u64());
            return Ok(());
        }
        self.generation = self.generation.max(entry.generation.as_u64());
        let imported = OverlayEntry {
            generation: entry.generation,
            key: entry.key.clone(),
            owner_token: entry.owner_token.clone(),
            kind: entry.kind,
            payload: entry.payload.clone(),
        };
        let mut entries = self.entries.write().map_err(|_| entry_storage_poisoned())?;
        let entry_index = entries.len();
        entries.push(imported);
        self.owner_tokens
            .insert(entry.key.clone(), entry.owner_token.clone());
        self.current_entry_index
            .write()
            .map_err(|_| entry_storage_poisoned())?
            .entry(entry.key.clone())
            .or_default()
            .push(entry_index);
        Ok(())
    }

    fn current_owner_token(&self, key: &OverlayKey) -> Result<Option<OverlayOwnerToken>> {
        if let Some(token) = self.owner_tokens.get(key) {
            return Ok(Some(token.clone()));
        }
        self.parent
            .as_ref()
            .map(|parent| parent.owner_token(key))
            .transpose()
            .map(Option::flatten)
    }
}

fn byte_prefix_successor(prefix: &[u8]) -> Option<Vec<u8>> {
    let mut end = prefix.to_vec();
    for byte in end.iter_mut().rev() {
        if *byte != u8::MAX {
            *byte += 1;
            return Some(end);
        }
    }
    None
}

fn latest_visible_entry(
    entries: &[OverlayEntry],
    history: &[usize],
    generation: OverlayGeneration,
) -> Option<MutableOverlayEntrySnapshot> {
    history
        .iter()
        .rev()
        .filter_map(|entry_index| entries.get(*entry_index))
        .find(|entry| entry.generation <= generation)
        .map(entry_snapshot)
}

fn entry_snapshot(entry: &OverlayEntry) -> MutableOverlayEntrySnapshot {
    MutableOverlayEntrySnapshot {
        generation: entry.generation,
        key: entry.key.clone(),
        owner_token: entry.owner_token.clone(),
        kind: entry.kind,
        payload: entry.payload.clone(),
    }
}

#[derive(Debug, Clone)]
pub struct OverlaySnapshot {
    generation: OverlayGeneration,
    entries: Arc<RwLock<Vec<OverlayEntry>>>,
    current_entry_index: Arc<RwLock<BTreeMap<OverlayKey, Vec<usize>>>>,
    parent: Option<Arc<OverlaySnapshot>>,
}

impl OverlaySnapshot {
    pub fn generation(&self) -> OverlayGeneration {
        self.generation
    }

    pub fn read_composite(
        &self,
        key: &OverlayKey,
        base_read: impl FnOnce(&OverlayKey) -> Result<Option<Vec<u8>>>,
    ) -> Result<Option<Vec<u8>>> {
        let entry = self.current_entry(key)?;
        match entry {
            Some(MutableOverlayEntrySnapshot {
                kind: OverlayEntryKind::Value,
                payload,
                ..
            }) => Ok(Some(payload)),
            Some(MutableOverlayEntrySnapshot {
                kind: OverlayEntryKind::Tombstone,
                ..
            }) => Ok(None),
            None => base_read(key),
        }
    }

    pub fn owner_token(&self, key: &OverlayKey) -> Result<Option<OverlayOwnerToken>> {
        Ok(self.current_entry(key)?.map(|entry| entry.owner_token))
    }

    fn current_entry(&self, key: &OverlayKey) -> Result<Option<MutableOverlayEntrySnapshot>> {
        let local_entries = self.entries.read().map_err(|_| entry_storage_poisoned())?;
        let current_entry_index = self
            .current_entry_index
            .read()
            .map_err(|_| entry_storage_poisoned())?;
        let current = current_entry_index
            .get(key)
            .and_then(|history| latest_visible_entry(&local_entries, history, self.generation));
        match current {
            Some(entry) => Ok(Some(entry)),
            None => self
                .parent
                .as_ref()
                .map(|parent| parent.current_entry(key))
                .transpose()
                .map(Option::flatten),
        }
    }

    fn current_entries_with_key_prefix(
        &self,
        prefix: &OverlayKeyPrefix,
    ) -> Result<Vec<MutableOverlayEntrySnapshot>> {
        let mut entries = BTreeMap::new();
        if let Some(parent) = &self.parent {
            for entry in parent.current_entries_with_key_prefix(prefix)? {
                entries.insert(entry.key.clone(), entry);
            }
        }
        let start = prefix.start_key()?;
        match prefix.end_key()? {
            Some(end) => {
                let local_entries = self.entries.read().map_err(|_| entry_storage_poisoned())?;
                let current_entry_index = self
                    .current_entry_index
                    .read()
                    .map_err(|_| entry_storage_poisoned())?;
                for (key, history) in current_entry_index.range(start..end) {
                    if let Some(entry) =
                        latest_visible_entry(&local_entries, history, self.generation)
                    {
                        entries.insert(key.clone(), entry);
                    }
                }
            }
            None => {
                let local_entries = self.entries.read().map_err(|_| entry_storage_poisoned())?;
                let current_entry_index = self
                    .current_entry_index
                    .read()
                    .map_err(|_| entry_storage_poisoned())?;
                for (key, history) in current_entry_index.range(start..) {
                    if let Some(entry) =
                        latest_visible_entry(&local_entries, history, self.generation)
                    {
                        entries.insert(key.clone(), entry);
                    }
                }
            }
        }
        Ok(entries.into_values().collect())
    }

    fn entries(&self) -> Result<Vec<OverlayEntry>> {
        let mut entries = match self.parent.as_ref() {
            Some(parent) => parent.entries()?,
            None => Vec::new(),
        };
        entries.extend(
            self.entries
                .read()
                .map_err(|_| entry_storage_poisoned())?
                .iter()
                .filter(|entry| entry.generation <= self.generation)
                .cloned(),
        );
        Ok(entries)
    }

    fn current_keys(&self) -> Result<Vec<OverlayKey>> {
        let mut keys = BTreeSet::new();
        for entry in self.entries()? {
            if entry.generation <= self.generation {
                keys.insert(entry.key);
            }
        }
        Ok(keys.into_iter().collect())
    }

    pub fn select_owner_scopes(
        &self,
        owner_scopes: &[OverlayOwnerScope],
    ) -> Result<OverlayPromotionSelection> {
        let mut latest = BTreeMap::<OverlayKey, OverlayPromotionEntry>::new();
        for entry in self.entries()? {
            if entry.generation > self.generation {
                continue;
            }
            let Some(owner_scope) = matching_owner_scope(owner_scopes, &entry.key)? else {
                continue;
            };
            latest.insert(
                entry.key.clone(),
                OverlayPromotionEntry {
                    key: entry.key.clone(),
                    owner_scope,
                    owner_token: entry.owner_token.clone(),
                    kind: entry.kind,
                    payload: entry.payload.clone(),
                    generation: entry.generation,
                },
            );
        }
        Ok(OverlayPromotionSelection {
            generation: self.generation,
            owner_scopes: owner_scopes.to_vec(),
            entries: latest.into_values().collect(),
        })
    }
}

fn matching_owner_scope(
    owner_scopes: &[OverlayOwnerScope],
    key: &OverlayKey,
) -> Result<Option<OverlayOwnerScope>> {
    for scope in owner_scopes {
        if scope.matches_key(key)? {
            return Ok(Some(scope.clone()));
        }
    }
    Ok(None)
}

fn entry_storage_poisoned() -> LoomError {
    LoomError::new(Code::Internal, "overlay entry storage lock poisoned")
}

fn owner_token(
    key: &OverlayKey,
    prior: Option<&OverlayOwnerToken>,
    kind: OverlayEntryKind,
    payload: &[u8],
) -> OverlayOwnerToken {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(TOKEN_SCHEMA);
    bytes.extend_from_slice(key.as_bytes());
    match prior {
        Some(token) => {
            bytes.push(1);
            bytes.extend_from_slice(token.as_bytes());
        }
        None => bytes.push(0),
    }
    bytes.push(match kind {
        OverlayEntryKind::Value => 1,
        OverlayEntryKind::Tombstone => 2,
    });
    bytes.extend_from_slice(&(payload.len() as u64).to_be_bytes());
    bytes.extend_from_slice(payload);
    OverlayOwnerToken::from_bytes(*Digest::blake3(&bytes).bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scoped_key(scope_id: &[u8], domain: &[u8], record_id: &[u8]) -> OverlayKey {
        OverlayKey::from_segments([
            b"workspace",
            scope_id,
            domain,
            b"notes",
            b"document-head",
            record_id,
        ])
        .unwrap()
    }

    fn key(record_id: &[u8]) -> OverlayKey {
        scoped_key(&[7; 16], b"document", record_id)
    }

    fn key_prefix(scope_id: &[u8], domain: &[u8]) -> OverlayKeyPrefix {
        OverlayKey::prefix_from_segments(
            6,
            [b"workspace", scope_id, domain, b"notes", b"document-head"],
        )
        .unwrap()
    }

    fn base(
        values: BTreeMap<OverlayKey, Vec<u8>>,
    ) -> impl Fn(&OverlayKey) -> Result<Option<Vec<u8>>> {
        move |key| Ok(values.get(key).cloned())
    }

    #[test]
    fn composite_read_prefers_overlay_value_over_base() {
        let item = key(b"doc-1");
        let mut base_values = BTreeMap::new();
        base_values.insert(item.clone(), b"base".to_vec());
        let mut overlay = MutableOverlay::new();
        overlay
            .put_value(item.clone(), None, b"overlay".to_vec())
            .unwrap();

        let read = overlay
            .snapshot()
            .read_composite(&item, base(base_values))
            .unwrap();

        assert_eq!(read.as_deref(), Some(&b"overlay"[..]));
    }

    #[test]
    fn composite_read_tombstone_masks_base_value() {
        let item = key(b"doc-2");
        let mut base_values = BTreeMap::new();
        base_values.insert(item.clone(), b"base".to_vec());
        let mut overlay = MutableOverlay::new();
        overlay.put_tombstone(item.clone(), None).unwrap();

        let read = overlay
            .snapshot()
            .read_composite(&item, base(base_values))
            .unwrap();

        assert_eq!(read, None);
    }

    #[test]
    fn prefix_selection_uses_forked_parent_current_range_only() {
        let wanted = scoped_key(&[7; 16], b"tickets", b"lane-a");
        let prefix = key_prefix(&[7; 16], b"tickets");
        let mut parent = MutableOverlay::new();
        for index in 0..200 {
            let unrelated = scoped_key(&[7; 16], b"document", format!("doc-{index}").as_bytes());
            parent
                .put_value(unrelated, None, format!("unrelated-{index}").into_bytes())
                .unwrap();
        }
        parent
            .put_value(wanted.clone(), None, b"lane".to_vec())
            .unwrap();
        let mut fork = MutableOverlay::fork_from_snapshot(parent.snapshot());
        for index in 0..200 {
            let unrelated = scoped_key(&[8; 16], b"tickets", format!("ticket-{index}").as_bytes());
            fork.put_value(unrelated, None, format!("local-{index}").into_bytes())
                .unwrap();
        }

        let entries = fork.export_entries_with_key_prefix(&prefix).unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].key, wanted);
        assert_eq!(entries[0].payload, b"lane");
    }

    #[test]
    fn snapshot_creation_shares_current_entry_index_without_cloning_records() {
        let mut overlay = MutableOverlay::new();
        for index in 0..200 {
            overlay
                .put_value(
                    key(format!("doc-{index}").as_bytes()),
                    None,
                    format!("payload-{index}").into_bytes(),
                )
                .unwrap();
        }

        assert_eq!(Arc::strong_count(&overlay.current_entry_index), 1);
        let snapshot = overlay.snapshot();

        assert_eq!(Arc::strong_count(&overlay.current_entry_index), 2);
        assert!(Arc::ptr_eq(
            &overlay.current_entry_index,
            &snapshot.current_entry_index
        ));
        assert_eq!(
            snapshot
                .current_entry(&key(b"doc-199"))
                .unwrap()
                .unwrap()
                .payload,
            b"payload-199"
        );
    }

    #[test]
    fn current_entry_index_retains_log_positions_not_payload_copies() {
        let item = key(b"doc-owned-once");
        let mut overlay = MutableOverlay::new();
        let mut expected = None;
        for index in 0..32 {
            let token = overlay
                .put_value(
                    item.clone(),
                    expected.as_ref(),
                    format!("payload-{index}").into_bytes(),
                )
                .unwrap();
            expected = Some(token);
        }

        let entries = overlay.entries.read().unwrap();
        let current_entry_index = overlay.current_entry_index.read().unwrap();
        let indexes = current_entry_index.get(&item).unwrap();

        assert_eq!(entries.len(), 32);
        assert_eq!(indexes.len(), 32);
        for (expected_index, entry_index) in indexes.iter().copied().enumerate() {
            assert_eq!(entry_index, expected_index);
            assert_eq!(
                entries[entry_index].payload,
                format!("payload-{expected_index}").into_bytes()
            );
        }
        assert_eq!(
            std::mem::size_of_val(indexes.as_slice()),
            32 * std::mem::size_of::<usize>()
        );
    }

    #[test]
    fn malformed_overlay_key_prefixes_fail_closed() {
        assert!(OverlayKeyPrefix::from_encoded_bytes(b"not-a-key".to_vec()).is_err());
        let mut missing_count = KEY_SCHEMA.to_vec();
        assert!(OverlayKeyPrefix::from_encoded_bytes(missing_count.clone()).is_err());
        missing_count.push(1);
        missing_count.extend_from_slice(&5u32.to_be_bytes());
        missing_count.extend_from_slice(b"abc");
        assert!(OverlayKeyPrefix::from_encoded_bytes(missing_count).is_err());

        let mut too_many_segments = KEY_SCHEMA.to_vec();
        too_many_segments.push(0);
        too_many_segments.extend_from_slice(&0u32.to_be_bytes());
        assert!(OverlayKeyPrefix::from_encoded_bytes(too_many_segments).is_err());

        let mut trailing_segment = KEY_SCHEMA.to_vec();
        trailing_segment.push(1);
        trailing_segment.extend_from_slice(&0u32.to_be_bytes());
        trailing_segment.extend_from_slice(&0u32.to_be_bytes());
        assert!(OverlayKeyPrefix::from_encoded_bytes(trailing_segment).is_err());

        let mut wrong_declared_count = KEY_SCHEMA.to_vec();
        wrong_declared_count.push(7);
        wrong_declared_count.extend_from_slice(&0u32.to_be_bytes());
        assert!(OverlayKeyPrefix::from_encoded_bytes(wrong_declared_count).is_err());

        let mut no_bounded_segment = KEY_SCHEMA.to_vec();
        no_bounded_segment.push(6);
        assert!(OverlayKeyPrefix::from_encoded_bytes(no_bounded_segment).is_err());

        let valid_partial = OverlayKey::prefix_from_segments(
            6,
            [
                b"workspace",
                &[7; 16],
                b"document",
                b"notes",
                b"document-head",
            ],
        )
        .unwrap();
        assert_eq!(
            OverlayKeyPrefix::from_encoded_bytes(valid_partial.as_bytes().to_vec())
                .unwrap()
                .as_bytes(),
            valid_partial.as_bytes()
        );
    }

    #[test]
    fn snapshot_fork_reads_parent_and_isolates_writes() {
        let unchanged = key(b"unchanged");
        let changed = key(b"changed");
        let added = key(b"added");
        let mut base = MutableOverlay::new();
        base.put_value(unchanged.clone(), None, b"base-only".to_vec())
            .unwrap();
        base.put_value(changed.clone(), None, b"before".to_vec())
            .unwrap();
        let base_generation = base.generation();
        let base_token = base.snapshot().owner_token(&changed).unwrap().unwrap();

        let mut fork = MutableOverlay::fork_from_snapshot(base.snapshot());
        fork.put_value(changed.clone(), Some(&base_token), b"after".to_vec())
            .unwrap();
        fork.put_value(added.clone(), None, b"new".to_vec())
            .unwrap();

        assert_eq!(
            fork.snapshot()
                .read_composite(&unchanged, |_| Ok(None))
                .unwrap()
                .as_deref(),
            Some(&b"base-only"[..])
        );
        assert_eq!(
            fork.snapshot()
                .read_composite(&changed, |_| Ok(None))
                .unwrap()
                .as_deref(),
            Some(&b"after"[..])
        );
        assert_eq!(
            base.snapshot()
                .read_composite(&changed, |_| Ok(None))
                .unwrap()
                .as_deref(),
            Some(&b"before"[..])
        );
        assert_eq!(
            base.snapshot()
                .read_composite(&added, |_| Ok(None))
                .unwrap(),
            None
        );
        assert_eq!(base.generation(), base_generation);
        assert_eq!(fork.export_entries().unwrap().len(), 4);
    }

    #[test]
    fn composite_read_falls_back_to_base_when_overlay_has_no_entry() {
        let item = key(b"doc-3");
        let mut base_values = BTreeMap::new();
        base_values.insert(item.clone(), b"base".to_vec());
        let overlay = MutableOverlay::new();

        let read = overlay
            .snapshot()
            .read_composite(&item, base(base_values))
            .unwrap();

        assert_eq!(read.as_deref(), Some(&b"base"[..]));
    }

    #[test]
    fn snapshot_generation_isolates_later_overlay_writes() {
        let item = key(b"doc-4");
        let mut overlay = MutableOverlay::new();
        let first = overlay
            .put_value(item.clone(), None, b"first".to_vec())
            .unwrap();
        let snapshot = overlay.snapshot();
        overlay
            .put_value(item.clone(), Some(&first), b"second".to_vec())
            .unwrap();

        let read = snapshot
            .read_composite(&item, base(BTreeMap::new()))
            .unwrap();
        let latest = overlay
            .snapshot()
            .read_composite(&item, base(BTreeMap::new()))
            .unwrap();

        assert_eq!(read.as_deref(), Some(&b"first"[..]));
        assert_eq!(latest.as_deref(), Some(&b"second"[..]));
    }

    #[test]
    fn checkpoint_generation_isolates_later_overlay_writes() {
        let item = key(b"doc-checkpoint");
        let mut overlay = MutableOverlay::new();
        let first = overlay
            .put_value(item.clone(), None, b"first".to_vec())
            .unwrap();
        let checkpoint = overlay.checkpoint();
        overlay
            .put_value(item.clone(), Some(&first), b"second".to_vec())
            .unwrap();

        let checkpoint_read = checkpoint
            .read_composite(&item, base(BTreeMap::new()))
            .unwrap();
        let latest = overlay
            .snapshot()
            .read_composite(&item, base(BTreeMap::new()))
            .unwrap();

        assert_eq!(checkpoint.generation().as_u64(), 1);
        assert_eq!(checkpoint.keys(), &[item]);
        assert_eq!(checkpoint_read.as_deref(), Some(&b"first"[..]));
        assert_eq!(latest.as_deref(), Some(&b"second"[..]));
    }

    #[test]
    fn checkpoint_validation_rejects_stale_generation() {
        let item = key(b"doc-stale-checkpoint");
        let mut overlay = MutableOverlay::new();
        let first = overlay
            .put_value(item.clone(), None, b"first".to_vec())
            .unwrap();
        let checkpoint = overlay.checkpoint();
        overlay
            .put_value(item, Some(&first), b"second".to_vec())
            .unwrap();

        let error = overlay.validate_checkpoint(&checkpoint).unwrap_err();

        assert_eq!(error.code, Code::Conflict);
    }

    #[test]
    fn checkpoint_selection_uses_pinned_generation_not_later_hot_state() {
        let item = key(b"doc-promote");
        let mut overlay = MutableOverlay::new();
        let first = overlay
            .put_value(item.clone(), None, b"first".to_vec())
            .unwrap();
        let checkpoint = overlay.checkpoint();
        overlay
            .put_value(item.clone(), Some(&first), b"second".to_vec())
            .unwrap();
        let scope = OverlayOwnerScope::new(b"workspace", &[7; 16], b"document").unwrap();

        let selected = checkpoint.select_owner_scopes(&[scope.clone()]).unwrap();

        assert_eq!(selected.generation.as_u64(), 1);
        assert_eq!(selected.owner_scopes, vec![scope]);
        assert_eq!(selected.entries.len(), 1);
        assert_eq!(selected.entries[0].key, item);
        assert_eq!(selected.entries[0].payload, b"first");
        assert_eq!(selected.entries[0].generation.as_u64(), 1);
    }

    #[test]
    fn checkpoint_selection_includes_exact_owner_scopes_only() {
        let document = key(b"doc-selected");
        let ticket = scoped_key(&[7; 16], b"tickets", b"ticket-unrelated");
        let other_workspace = scoped_key(&[8; 16], b"document", b"doc-unrelated");
        let mut overlay = MutableOverlay::new();
        overlay
            .put_value(document.clone(), None, b"document".to_vec())
            .unwrap();
        overlay.put_value(ticket, None, b"ticket".to_vec()).unwrap();
        overlay
            .put_value(other_workspace, None, b"other".to_vec())
            .unwrap();
        let checkpoint = overlay.checkpoint();
        let scope = OverlayOwnerScope::new(b"workspace", &[7; 16], b"document").unwrap();

        let selected = checkpoint.select_owner_scopes(&[scope.clone()]).unwrap();

        assert_eq!(selected.owner_scopes, vec![scope.clone()]);
        assert_eq!(selected.entries.len(), 1);
        assert_eq!(selected.entries[0].owner_scope, scope);
        assert_eq!(selected.entries[0].key, document);
        assert_eq!(selected.entries[0].payload, b"document");
    }

    #[test]
    fn checkpoint_selection_preserves_current_tombstone_boundaries() {
        let item = key(b"doc-deleted");
        let mut overlay = MutableOverlay::new();
        let token = overlay
            .put_value(item.clone(), None, b"body".to_vec())
            .unwrap();
        overlay.put_tombstone(item.clone(), Some(&token)).unwrap();
        let checkpoint = overlay.checkpoint();
        let scope = OverlayOwnerScope::new(b"workspace", &[7; 16], b"document").unwrap();

        let selected = checkpoint.select_owner_scopes(&[scope]).unwrap();

        assert_eq!(selected.entries.len(), 1);
        assert_eq!(selected.entries[0].key, item);
        assert_eq!(selected.entries[0].kind, OverlayEntryKind::Tombstone);
        assert!(selected.entries[0].payload.is_empty());
        assert_eq!(selected.entries[0].generation.as_u64(), 2);
    }

    #[test]
    fn compare_token_validation_rejects_stale_owner_token() {
        let item = key(b"doc-5");
        let other = key(b"doc-other");
        let mut overlay = MutableOverlay::new();
        let current = overlay
            .put_value(item.clone(), None, b"first".to_vec())
            .unwrap();
        let stale = overlay.put_value(other, None, b"other".to_vec()).unwrap();
        let error = overlay
            .put_value(item.clone(), Some(&stale), b"bad".to_vec())
            .unwrap_err();

        assert_eq!(error.code, Code::Conflict);
        assert_eq!(
            overlay
                .snapshot()
                .owner_token(&item)
                .unwrap()
                .map(|token| token.as_bytes().to_owned()),
            Some(*current.as_bytes())
        );
    }

    #[test]
    fn overlay_health_reports_current_records_and_hot_writes() {
        let first = key(b"doc-6");
        let second = key(b"doc-7");
        let mut overlay = MutableOverlay::new();
        let token = overlay
            .put_value(first.clone(), None, b"first".to_vec())
            .unwrap();
        overlay
            .put_value(first, Some(&token), b"second".to_vec())
            .unwrap();
        overlay.put_tombstone(second, None).unwrap();

        let health = overlay.health().unwrap();

        assert_eq!(health.current_generation, 3);
        assert_eq!(health.current_record_count, 2);
        assert_eq!(health.tombstone_count, 1);
        assert_eq!(health.live_checkpoint_references, 0);
        assert_eq!(health.reclaimable_overlay_pages, 0);
        assert!(health.blocked_reclamation_reasons.is_empty());
        assert_eq!(health.hot_write_count, 3);
        assert_eq!(health.active_writer_contention_indicators, 0);
    }

    #[test]
    fn durability_policy_names_and_recovery_contracts_are_stable() {
        assert_eq!(
            OverlayDurabilityPolicy::ALL.map(OverlayDurabilityPolicy::as_str),
            ["strict", "normal", "relaxed", "ephemeral"]
        );

        assert!(OverlayDurabilityPolicy::Strict.is_durable());
        assert!(OverlayDurabilityPolicy::Normal.is_durable());
        assert!(OverlayDurabilityPolicy::Relaxed.is_durable());
        assert!(!OverlayDurabilityPolicy::Ephemeral.is_durable());

        assert!(OverlayDurabilityPolicy::Strict.survives_process_restart());
        assert!(OverlayDurabilityPolicy::Normal.survives_process_restart());
        assert!(!OverlayDurabilityPolicy::Relaxed.survives_process_restart());
        assert!(!OverlayDurabilityPolicy::Ephemeral.survives_process_restart());

        assert!(OverlayDurabilityPolicy::Strict.survives_power_loss_when_storage_honors_fsync());
        assert!(!OverlayDurabilityPolicy::Normal.survives_power_loss_when_storage_honors_fsync());
    }

    #[test]
    fn durability_policy_parse_rejects_unknown_names() {
        assert_eq!(
            OverlayDurabilityPolicy::parse("strict").unwrap(),
            OverlayDurabilityPolicy::Strict
        );
        assert_eq!(
            OverlayDurabilityPolicy::parse("normal").unwrap(),
            OverlayDurabilityPolicy::Normal
        );
        assert_eq!(
            OverlayDurabilityPolicy::parse("relaxed").unwrap(),
            OverlayDurabilityPolicy::Relaxed
        );
        assert_eq!(
            OverlayDurabilityPolicy::parse("ephemeral").unwrap(),
            OverlayDurabilityPolicy::Ephemeral
        );

        let error = OverlayDurabilityPolicy::parse("best-effort").unwrap_err();
        assert_eq!(error.code, Code::InvalidArgument);
    }
}
