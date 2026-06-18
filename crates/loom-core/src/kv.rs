//! The key-value facet - a versioned map keyed by a typed, order-preserving [`Value`] (keys share one
//! ordering with SQL; `Bytes` covers the opaque case), valued by opaque bytes. Pure-Rust,
//! `wasm32`-clean, deterministic.
//!
//! A canonical map root plus content-addressed value components versions, branches, and syncs
//! through the engine like any other Loom state. This module does not reconcile same-key edits across
//! branches.

use crate::cbor;
use crate::digest::{DIGEST_LEN, Digest};
use crate::error::{Code, LoomError, Result};
use crate::object::content_address_with;
use crate::provider::ObjectStore;
use crate::tabular::{Value, cell_from, cell_value};
use crate::vcs::Loom;
use crate::workspace::{FacetKind, WorkspaceId, facet_path, facet_root};
use std::collections::{BTreeMap, BTreeSet};

const PROLLY_MAP_ROOT_SCHEMA: &str = "loom.kv.prolly-map-root.v1";

/// Encode a single typed key as Loom Canonical CBOR (one tagged cell). This is the public KV key wire
/// form: the same cell codec the SQL result path uses, so a key crosses the ABI losslessly.
pub fn key_to_cbor(key: &Value) -> Vec<u8> {
    cbor::encode(&cell_value(key))
}

/// Decode a single typed key from its Loom Canonical CBOR cell form ([`key_to_cbor`]).
pub fn key_from_cbor(bytes: &[u8]) -> Result<Value> {
    cell_from(cbor::decode(bytes)?)
}

/// A versioned key-value map: typed keys in order, opaque byte values.
#[derive(Debug, Clone, Default)]
pub struct KvMap {
    anchors: BTreeMap<Value, u64>,
    entries: BTreeMap<Value, Vec<u8>>,
}

/// Owner-scoped condition for a single-key KV mutation.
#[derive(Clone, PartialEq, Eq)]
pub enum KvCondition {
    /// Apply regardless of the current entry state.
    Any,
    /// Apply only while the entry is absent.
    Absent,
    /// Apply only while the entry matches an owner-issued token.
    Exact(KvExactToken),
}

/// Opaque token for an exact KV entry comparison.
#[derive(Clone, PartialEq, Eq)]
pub struct KvExactToken(Vec<u8>);

impl KvMap {
    /// An empty map.
    pub fn new() -> Self {
        Self::default()
    }
    /// Number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    /// Whether the map is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    /// The owner-local mutation anchor for `key`, retained after deletion.
    pub fn anchor(&self, key: &Value) -> u64 {
        self.anchors.get(key).copied().unwrap_or(0)
    }
    /// Insert or replace the value at `key`.
    pub fn put(&mut self, key: Value, value: Vec<u8>) {
        self.anchors.entry(key.clone()).or_insert(1);
        self.entries.insert(key, value);
    }
    /// The value at `key`.
    pub fn get(&self, key: &Value) -> Option<&[u8]> {
        self.entries.get(key).map(Vec::as_slice)
    }
    /// Remove `key`; returns whether it was present.
    pub fn delete(&mut self, key: &Value) -> bool {
        self.entries.remove(key).is_some()
    }
    /// Entries in key order (the typed `Value` total order, so e.g. `Int(2)` precedes `Int(10)`).
    pub fn iter(&self) -> impl Iterator<Item = (&Value, &[u8])> {
        self.entries.iter().map(|(k, v)| (k, v.as_slice()))
    }
    /// Entries with `lo <= key < hi`, in key order (half-open range scan).
    pub fn range(&self, lo: &Value, hi: &Value) -> Vec<(&Value, &[u8])> {
        self.entries
            .range(lo.clone()..hi.clone())
            .map(|(k, v)| (k, v.as_slice()))
            .collect()
    }

    /// Canonical bytes: `[format, entries, anchors]`, where both maps are sorted `[key, value]`
    /// pairs. Anchors retain deleted-key generations. Legacy formats decode deterministically.
    pub fn encode(&self) -> Vec<u8> {
        let entries = self
            .entries
            .iter()
            .map(|(k, v)| {
                cbor::Value::Array(vec![
                    crate::tabular::cell_value(k),
                    cbor::Value::Bytes(v.clone()),
                ])
            })
            .collect();
        let anchors = self
            .anchors
            .iter()
            .map(|(key, anchor)| {
                cbor::Value::Array(vec![cell_value(key), cbor::Value::Uint(*anchor)])
            })
            .collect();
        cbor::encode(&cbor::Value::Array(vec![
            cbor::Value::Uint(2),
            cbor::Value::Array(entries),
            cbor::Value::Array(anchors),
        ]))
    }
    /// Parse a map from [`KvMap::encode`] output.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let encoded = cbor::decode_array(bytes)?;
        let (legacy_anchor, entries, anchors) = match encoded.as_slice() {
            [
                cbor::Value::Uint(1),
                cbor::Value::Uint(anchor),
                cbor::Value::Array(entries),
            ] => (Some(*anchor), entries.clone(), Vec::new()),
            [
                cbor::Value::Uint(2),
                cbor::Value::Array(entries),
                cbor::Value::Array(anchors),
            ] => (None, entries.clone(), anchors.clone()),
            _ => (Some(0), encoded, Vec::new()),
        };
        let mut map = KvMap {
            anchors: BTreeMap::new(),
            entries: BTreeMap::new(),
        };
        for item in entries {
            let mut f = cbor::Fields::new(cbor::as_array(item)?);
            let key = crate::tabular::cell_from(f.next_field()?)?;
            let val = f.bytes()?;
            f.end()?;
            if map.entries.insert(key, val).is_some() {
                return Err(LoomError::corrupt("duplicate KV entry key"));
            }
        }
        for item in anchors {
            let mut f = cbor::Fields::new(cbor::as_array(item)?);
            let key = crate::tabular::cell_from(f.next_field()?)?;
            let anchor = f.uint()?;
            f.end()?;
            if anchor == 0 {
                return Err(LoomError::corrupt("KV anchor must be greater than zero"));
            }
            if map.anchors.insert(key, anchor).is_some() {
                return Err(LoomError::corrupt("duplicate KV anchor key"));
            }
        }
        if legacy_anchor.is_none() && map.entries.keys().any(|key| !map.anchors.contains_key(key)) {
            return Err(LoomError::corrupt("live KV entry is missing an anchor"));
        }
        if let Some(anchor) = legacy_anchor {
            for key in map.entries.keys() {
                map.anchors.insert(key.clone(), anchor);
            }
        }
        Ok(map)
    }

    fn advance_anchor(&mut self, key: &Value) -> Result<()> {
        let anchor = self.anchors.entry(key.clone()).or_default();
        *anchor = anchor
            .checked_add(1)
            .ok_or_else(|| LoomError::new(Code::Conflict, "KV mutation anchor exhausted"))?;
        Ok(())
    }
}

/// Per-entry lifetime options for the ephemeral KV tier.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EphemeralPutOptions {
    pub ttl_ms: Option<u64>,
    pub idle_ttl_ms: Option<u64>,
}

/// The eviction policy applied to an ephemeral map when a capacity bound is hit. Eviction order is
/// implementation-defined within the named policy and is not a conformance-pinned object graph (there is
/// no versioned graph for this tier).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum EvictionPolicy {
    /// Never evict: capacity bounds are advisory and over-capacity puts still grow the map.
    #[default]
    None,
    /// Evict the least-recently-accessed entry.
    Lru,
    /// Evict the least-frequently-accessed entry.
    Lfu,
    /// Evict an arbitrary entry (deterministic but unspecified within the policy).
    Random,
    /// Evict the oldest-written entry (insertion order), independent of access.
    Fifo,
    /// Evict the entry that will expire soonest (entries without an expiry are evicted last).
    TtlPriority,
}

impl EvictionPolicy {
    /// The stable wire tag for engine-state serialization.
    pub fn to_u8(self) -> u8 {
        match self {
            EvictionPolicy::None => 0,
            EvictionPolicy::Lru => 1,
            EvictionPolicy::Lfu => 2,
            EvictionPolicy::Random => 3,
            EvictionPolicy::Fifo => 4,
            EvictionPolicy::TtlPriority => 5,
        }
    }
    /// The eviction policy for a wire tag.
    pub fn from_u8(b: u8) -> Result<Self> {
        Ok(match b {
            0 => EvictionPolicy::None,
            1 => EvictionPolicy::Lru,
            2 => EvictionPolicy::Lfu,
            3 => EvictionPolicy::Random,
            4 => EvictionPolicy::Fifo,
            5 => EvictionPolicy::TtlPriority,
            other => {
                return Err(LoomError::corrupt(format!(
                    "unknown eviction policy {other}"
                )));
            }
        })
    }
}

/// What happens to an entry when it is evicted under capacity pressure.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OnEvict {
    /// Drop the evicted entry (the default).
    #[default]
    Drop,
    /// Flush the evicted entry to the backing versioned map before dropping it (requires a backing map).
    WriteThrough,
}

impl OnEvict {
    /// The stable wire tag for engine-state serialization.
    pub fn to_u8(self) -> u8 {
        match self {
            OnEvict::Drop => 0,
            OnEvict::WriteThrough => 1,
        }
    }
    /// The on-evict action for a wire tag.
    pub fn from_u8(b: u8) -> Result<Self> {
        Ok(match b {
            0 => OnEvict::Drop,
            1 => OnEvict::WriteThrough,
            other => {
                return Err(LoomError::corrupt(format!(
                    "unknown on_evict action {other}"
                )));
            }
        })
    }
}

/// What an ephemeral cache does under sustained write-behind pressure once it crosses the soft
/// high-water mark (`flush_high_water_pct` of a capacity bound). The hard bound is still enforced by
/// eviction; this governs the *latency-vs-bound* trade-off for the asynchronous flush queue.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BackPressure {
    /// Drain the whole flush queue synchronously before the write returns: a hard memory bound at the
    /// cost of unbounded write latency. The default - never silently lose buffered writes.
    #[default]
    Block,
    /// Reject the write with [`Code::Locked`](crate::error::Code::Locked) while saturated, so the caller
    /// backs off and retries: a hard bound with no latency penalty for accepted writes, at the cost of
    /// rejected ones.
    Pressure,
    /// Flush one bounded batch (`flush_batch`) then let the write proceed: bounded latency and a soft
    /// bound that may briefly exceed the high-water mark under burst.
    Assisted,
}

impl BackPressure {
    /// The stable wire tag for config serialization.
    pub fn to_u8(self) -> u8 {
        match self {
            BackPressure::Block => 0,
            BackPressure::Pressure => 1,
            BackPressure::Assisted => 2,
        }
    }
    /// The back-pressure policy for a wire tag.
    pub fn from_u8(b: u8) -> Result<Self> {
        Ok(match b {
            0 => BackPressure::Block,
            1 => BackPressure::Pressure,
            2 => BackPressure::Assisted,
            other => {
                return Err(LoomError::corrupt(format!(
                    "unknown back_pressure policy {other}"
                )));
            }
        })
    }
}

/// The storage tier for a named KV map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KvTier {
    Versioned,
    Ephemeral,
}

impl KvTier {
    /// The stable wire tag for config serialization.
    pub fn to_u8(self) -> u8 {
        match self {
            KvTier::Versioned => 0,
            KvTier::Ephemeral => 1,
        }
    }
    /// The tier for a wire tag.
    pub fn from_u8(b: u8) -> Result<Self> {
        Ok(match b {
            0 => KvTier::Versioned,
            1 => KvTier::Ephemeral,
            other => return Err(LoomError::corrupt(format!("unknown kv tier {other}"))),
        })
    }
}

/// Durable configuration for a named KV map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KvMapConfig {
    pub tier: KvTier,
    pub default_put: EphemeralPutOptions,
    pub read_through: bool,
    pub write_through: bool,
    /// Capacity bound on entry count; `None` is unbounded. Eviction applies only when `eviction` is set.
    pub max_entries: Option<u64>,
    /// Capacity bound on stored value bytes; `None` is unbounded.
    pub max_bytes: Option<u64>,
    /// The policy applied when a capacity bound is hit.
    pub eviction: EvictionPolicy,
    /// What happens to an entry when it is evicted.
    pub on_evict: OnEvict,
    /// Asynchronous write-back: a put/delete updates the cache immediately and buffers the backing-map
    /// mutation in a coalescing dirty queue, flushed later by [`Loom::flush_pending`] or under
    /// back-pressure. Takes precedence over `write_through`. A crash (or a stateless host that cannot
    /// keep the queue alive) before a flush loses the un-flushed delta - acceptable for a cache.
    pub write_behind: bool,
    /// Write-around: a put/delete writes the backing versioned map synchronously and does **not** populate
    /// the cache (it invalidates any stale entry instead). For write-heavy keys that are not re-read soon.
    /// Takes precedence over both `write_behind` and `write_through`.
    pub write_around: bool,
    /// Back-pressure policy for the write-behind flush queue once it crosses `flush_high_water_pct`.
    pub back_pressure: BackPressure,
    /// Soft flush threshold as a percent (1..=100) of a capacity bound; once crossed, `back_pressure`
    /// governs the write-behind queue. `None` means only the hard capacity bound applies (treated as 100%).
    pub flush_high_water_pct: Option<u8>,
    /// Maximum dirty entries flushed per assisted/incremental batch (coalesced, in key order). `None`
    /// flushes the whole queue in one drain.
    pub flush_batch: Option<u64>,
}

impl KvMapConfig {
    /// The default source-of-truth map: versioned, synced, and committed.
    pub const VERSIONED: Self = Self {
        tier: KvTier::Versioned,
        default_put: EphemeralPutOptions {
            ttl_ms: None,
            idle_ttl_ms: None,
        },
        read_through: false,
        write_through: false,
        max_entries: None,
        max_bytes: None,
        eviction: EvictionPolicy::None,
        on_evict: OnEvict::Drop,
        write_behind: false,
        write_around: false,
        back_pressure: BackPressure::Block,
        flush_high_water_pct: None,
        flush_batch: None,
    };

    /// A volatile cache with no backing.
    pub const EPHEMERAL: Self = Self {
        tier: KvTier::Ephemeral,
        default_put: EphemeralPutOptions {
            ttl_ms: None,
            idle_ttl_ms: None,
        },
        read_through: false,
        write_through: false,
        max_entries: None,
        max_bytes: None,
        eviction: EvictionPolicy::None,
        on_evict: OnEvict::Drop,
        write_behind: false,
        write_around: false,
        back_pressure: BackPressure::Block,
        flush_high_water_pct: None,
        flush_batch: None,
    };

    /// The effective config for a stateless host that cannot keep a runtime write-behind queue alive
    /// between calls: write-behind downgrades to synchronous write-through, so a buffered delta is never
    /// lost across a per-request reopen. Other modes are unchanged.
    pub fn for_stateless(self) -> Self {
        if self.write_behind {
            Self {
                write_behind: false,
                write_through: true,
                ..self
            }
        } else {
            self
        }
    }

    /// Canonical CBOR for the durable config: a fixed-length array of its fields. Deterministic, so the
    /// config blob addresses stably and versions/syncs with the workspace like any other reserved file.
    pub fn encode(&self) -> Vec<u8> {
        let opt = |v: Option<u64>| v.map_or(cbor::Value::Null, cbor::Value::Uint);
        cbor::encode(&cbor::Value::Array(vec![
            cbor::Value::Uint(u64::from(self.tier.to_u8())),
            opt(self.default_put.ttl_ms),
            opt(self.default_put.idle_ttl_ms),
            cbor::Value::Bool(self.read_through),
            cbor::Value::Bool(self.write_through),
            opt(self.max_entries),
            opt(self.max_bytes),
            cbor::Value::Uint(u64::from(self.eviction.to_u8())),
            cbor::Value::Uint(u64::from(self.on_evict.to_u8())),
            cbor::Value::Bool(self.write_behind),
            cbor::Value::Bool(self.write_around),
            cbor::Value::Uint(u64::from(self.back_pressure.to_u8())),
            opt(self.flush_high_water_pct.map(u64::from)),
            opt(self.flush_batch),
        ]))
    }

    /// Parse a config from [`KvMapConfig::encode`] output.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let mut f = cbor::Fields::new(cbor::decode_array(bytes)?);
        let tier = KvTier::from_u8(u8::try_from(f.uint()?).unwrap_or(u8::MAX))?;
        let ttl_ms = opt_u64_field(&mut f)?;
        let idle_ttl_ms = opt_u64_field(&mut f)?;
        let read_through = bool_field(&mut f)?;
        let write_through = bool_field(&mut f)?;
        let max_entries = opt_u64_field(&mut f)?;
        let max_bytes = opt_u64_field(&mut f)?;
        let eviction = EvictionPolicy::from_u8(u8::try_from(f.uint()?).unwrap_or(u8::MAX))?;
        let on_evict = OnEvict::from_u8(u8::try_from(f.uint()?).unwrap_or(u8::MAX))?;
        let write_behind = bool_field(&mut f)?;
        let write_around = bool_field(&mut f)?;
        let back_pressure = BackPressure::from_u8(u8::try_from(f.uint()?).unwrap_or(u8::MAX))?;
        let flush_high_water_pct = opt_u64_field(&mut f)?.map(|v| u8::try_from(v).unwrap_or(100));
        let flush_batch = opt_u64_field(&mut f)?;
        f.end()?;
        Ok(Self {
            tier,
            default_put: EphemeralPutOptions {
                ttl_ms,
                idle_ttl_ms,
            },
            read_through,
            write_through,
            max_entries,
            max_bytes,
            eviction,
            on_evict,
            write_behind,
            write_around,
            back_pressure,
            flush_high_water_pct,
            flush_batch,
        })
    }
}

/// Read one `Null | Uint` config field as `Option<u64>`.
fn opt_u64_field(f: &mut cbor::Fields) -> Result<Option<u64>> {
    match f.next_field()? {
        cbor::Value::Null => Ok(None),
        cbor::Value::Uint(n) => Ok(Some(n)),
        _ => Err(LoomError::corrupt("kv config: expected uint or null")),
    }
}

/// Read one `Bool` config field.
fn bool_field(f: &mut cbor::Fields) -> Result<bool> {
    match f.next_field()? {
        cbor::Value::Bool(b) => Ok(b),
        _ => Err(LoomError::corrupt("kv config: expected bool")),
    }
}

/// The reserved working-tree path for a KV map's durable config. Stored under a `.config` subtree of the
/// KV facet root so it commits and syncs with the workspace; `.config` is a reserved collection name.
fn config_dir() -> String {
    format!("{}/.config", facet_root(FacetKind::Kv))
}

fn config_path(collection: &str) -> String {
    format!("{}/{collection}", config_dir())
}

/// Persist `config` for `collection` as a committed reserved file (versions and syncs with the workspace).
pub fn put_kv_config<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    config: &KvMapConfig,
) -> Result<()> {
    loom.create_directory_reserved(ns, &config_dir(), true)?;
    loom.write_file_reserved(ns, &config_path(collection), &config.encode(), 0o100644)
}

/// The durable config for `collection`, or [`KvMapConfig::VERSIONED`] when none is stored (or unreadable).
pub fn get_kv_config<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
) -> KvMapConfig {
    match loom.read_file_reserved(ns, &config_path(collection)) {
        Ok(bytes) => KvMapConfig::decode(&bytes).unwrap_or(KvMapConfig::VERSIONED),
        Err(_) => KvMapConfig::VERSIONED,
    }
}

/// Remove a stored config (reverting `collection` to the default versioned tier). A no-op when absent.
pub fn remove_kv_config<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    collection: &str,
) -> Result<()> {
    match loom.remove_file_reserved(ns, &config_path(collection)) {
        Ok(()) => Ok(()),
        Err(e) if e.code == Code::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Per-entry memory overhead added to the key+value byte size for `max_bytes` accounting. A fixed,
/// config-independent estimate of the bookkeeping cost (struct fields, map node) so the byte bound
/// approximates real footprint rather than just payload bytes.
const ENTRY_OVERHEAD_BYTES: u64 = 64;

/// A non-versioned, non-synced KV cache for one coordinator.
#[derive(Debug, Clone, Default)]
pub struct EphemeralKvMap {
    entries: BTreeMap<Value, EphemeralEntry>,
    max_entries: Option<u64>,
    max_bytes: Option<u64>,
    eviction: EvictionPolicy,
    /// Rotating cursor that makes [`EvictionPolicy::Random`] deterministic across a run.
    evict_cursor: u64,
    /// Running sum of every entry's accounted size, kept incrementally so `max_bytes` checks are O(1).
    bytes: u64,
    /// Monotonic insertion counter for [`EvictionPolicy::Fifo`] ordering.
    next_seq: u64,
    /// Coalescing write-behind buffer: the pending backing-map mutation per key (`Some(bytes)` = put,
    /// `None` = delete). Last write per key wins, so a key never flushes more than its latest state.
    dirty: BTreeMap<Value, Option<Vec<u8>>>,
}

#[derive(Debug, Clone)]
struct EphemeralEntry {
    value: Vec<u8>,
    expires_at_ms: Option<u64>,
    idle_ttl_ms: Option<u64>,
    last_access_ms: u64,
    /// Access count, used by [`EvictionPolicy::Lfu`].
    hits: u64,
    /// Insertion sequence, used by [`EvictionPolicy::Fifo`].
    seq: u64,
    /// Accounted size (key + value + overhead), tracked so removal adjusts [`EphemeralKvMap::bytes`].
    size: u64,
}

/// The accounted size of an entry: encoded key bytes + value bytes + a fixed per-entry overhead.
fn entry_size(key: &Value, value_len: usize) -> u64 {
    key_to_cbor(key).len() as u64 + value_len as u64 + ENTRY_OVERHEAD_BYTES
}

impl EphemeralKvMap {
    /// An empty ephemeral map.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the capacity bounds and eviction policy. `max_entries`/`max_bytes` are `None` for unbounded;
    /// eviction applies only when a bound is hit and `eviction` is not [`EvictionPolicy::None`].
    pub fn set_limits(
        &mut self,
        max_entries: Option<u64>,
        max_bytes: Option<u64>,
        eviction: EvictionPolicy,
    ) {
        self.max_entries = max_entries;
        self.max_bytes = max_bytes;
        self.eviction = eviction;
    }

    /// Number of unexpired entries after lazy expiry at `now_ms`.
    pub fn len(&mut self, now_ms: u64) -> usize {
        self.expire(now_ms);
        self.entries.len()
    }

    /// Insert or replace a cache entry, enforcing capacity bounds (evicted entries are dropped). See
    /// [`EphemeralKvMap::put_evicting`] to recover the evicted entries (for on-evict write-through).
    pub fn put(
        &mut self,
        key: Value,
        value: Vec<u8>,
        opts: EphemeralPutOptions,
        now_ms: u64,
    ) -> Result<()> {
        self.put_evicting(key, value, opts, now_ms)?;
        Ok(())
    }

    /// Insert or replace a cache entry, returning the entries evicted to honor the capacity bounds (in
    /// eviction order). Expired entries are reclaimed first, so eviction only sheds live entries.
    pub fn put_evicting(
        &mut self,
        key: Value,
        value: Vec<u8>,
        opts: EphemeralPutOptions,
        now_ms: u64,
    ) -> Result<Vec<(Value, Vec<u8>)>> {
        let expires_at_ms = deadline(now_ms, opts.ttl_ms, "ttl")?;
        validate_positive(opts.idle_ttl_ms, "idle_ttl")?;
        self.expire(now_ms);
        let size = entry_size(&key, value.len());
        let seq = self.next_seq;
        self.next_seq = self.next_seq.wrapping_add(1);
        // Replacing a key swaps its accounted size; subtract the old before adding the new.
        if let Some(old) = self.entries.insert(
            key,
            EphemeralEntry {
                value,
                expires_at_ms,
                idle_ttl_ms: opts.idle_ttl_ms,
                last_access_ms: now_ms,
                hits: 0,
                seq,
                size,
            },
        ) {
            self.bytes = self.bytes.saturating_sub(old.size);
        }
        self.bytes = self.bytes.saturating_add(size);
        Ok(self.enforce_capacity())
    }

    /// Read a cache entry. Expired entries behave as absent and are removed lazily.
    pub fn get(&mut self, key: &Value, now_ms: u64) -> Option<Vec<u8>> {
        if self.entry_expired(key, now_ms) {
            if let Some(old) = self.entries.remove(key) {
                self.bytes = self.bytes.saturating_sub(old.size);
            }
            return None;
        }
        let entry = self.entries.get_mut(key)?;
        entry.last_access_ms = now_ms;
        entry.hits = entry.hits.saturating_add(1);
        Some(entry.value.clone())
    }

    /// Remove a cache entry.
    pub fn delete(&mut self, key: &Value) -> bool {
        match self.entries.remove(key) {
            Some(old) => {
                self.bytes = self.bytes.saturating_sub(old.size);
                true
            }
            None => false,
        }
    }

    /// Entries with `lo <= key < hi`, in key order, after lazy expiry at `now_ms`.
    pub fn range(&mut self, lo: &Value, hi: &Value, now_ms: u64) -> Vec<(Value, Vec<u8>)> {
        self.expire(now_ms);
        self.entries
            .range(lo.clone()..hi.clone())
            .map(|(key, entry)| (key.clone(), entry.value.clone()))
            .collect()
    }

    /// Entries in key order after lazy expiry at `now_ms`.
    pub fn list(&mut self, now_ms: u64) -> Vec<(Value, Vec<u8>)> {
        self.expire(now_ms);
        self.entries
            .iter()
            .map(|(key, entry)| (key.clone(), entry.value.clone()))
            .collect()
    }

    fn expire(&mut self, now_ms: u64) {
        let mut freed = 0u64;
        self.entries.retain(|_, entry| {
            let live = !entry_expired(entry, now_ms);
            if !live {
                freed += entry.size;
            }
            live
        });
        self.bytes = self.bytes.saturating_sub(freed);
    }

    fn entry_expired(&self, key: &Value, now_ms: u64) -> bool {
        self.entries
            .get(key)
            .is_some_and(|entry| entry_expired(entry, now_ms))
    }

    /// Whether either capacity bound is currently exceeded. Uses the O(1) running byte counter.
    fn over_capacity(&self) -> bool {
        self.max_entries
            .is_some_and(|m| self.entries.len() as u64 > m)
            || self.max_bytes.is_some_and(|m| self.bytes > m)
    }

    /// The running accounted byte total (test accessor for the O(1) counter).
    #[cfg(test)]
    fn total_bytes_for_test(&self) -> u64 {
        self.bytes
    }

    /// Pick the next entry to evict under the active policy, or `None` for [`EvictionPolicy::None`] or an
    /// empty map. LRU/LFU/FIFO/TTL-priority break ties by key order for determinism; Random rotates a
    /// cursor.
    fn pick_victim(&mut self) -> Option<Value> {
        if self.entries.is_empty() {
            return None;
        }
        match self.eviction {
            EvictionPolicy::None => None,
            EvictionPolicy::Lru => self
                .entries
                .iter()
                .min_by(|a, b| {
                    a.1.last_access_ms
                        .cmp(&b.1.last_access_ms)
                        .then_with(|| a.0.cmp(b.0))
                })
                .map(|(k, _)| k.clone()),
            EvictionPolicy::Lfu => self
                .entries
                .iter()
                .min_by(|a, b| a.1.hits.cmp(&b.1.hits).then_with(|| a.0.cmp(b.0)))
                .map(|(k, _)| k.clone()),
            EvictionPolicy::Fifo => self
                .entries
                .iter()
                .min_by(|a, b| a.1.seq.cmp(&b.1.seq).then_with(|| a.0.cmp(b.0)))
                .map(|(k, _)| k.clone()),
            EvictionPolicy::TtlPriority => self
                .entries
                .iter()
                // Entries with an expiry sort before those without (None = never expires = evict last).
                .min_by(|a, b| {
                    let ax = a.1.expires_at_ms.unwrap_or(u64::MAX);
                    let bx = b.1.expires_at_ms.unwrap_or(u64::MAX);
                    ax.cmp(&bx).then_with(|| a.0.cmp(b.0))
                })
                .map(|(k, _)| k.clone()),
            EvictionPolicy::Random => {
                let idx = (self.evict_cursor % self.entries.len() as u64) as usize;
                self.evict_cursor = self.evict_cursor.wrapping_add(1);
                self.entries.keys().nth(idx).cloned()
            }
        }
    }

    /// Evict entries until both capacity bounds hold, returning the evicted `(key, value)` pairs in
    /// eviction order. A no-op under [`EvictionPolicy::None`] (bounds are advisory there).
    fn enforce_capacity(&mut self) -> Vec<(Value, Vec<u8>)> {
        let mut evicted = Vec::new();
        if self.eviction == EvictionPolicy::None {
            return evicted;
        }
        while self.over_capacity() {
            let Some(victim) = self.pick_victim() else {
                break;
            };
            if let Some(entry) = self.entries.remove(&victim) {
                self.bytes = self.bytes.saturating_sub(entry.size);
                evicted.push((victim, entry.value));
            } else {
                break;
            }
        }
        evicted
    }

    // ---- write-behind dirty buffer -------------------------------------------------

    /// Buffer a put for asynchronous write-back, coalescing with any pending op at `key`.
    pub fn mark_dirty_put(&mut self, key: Value, value: Vec<u8>) {
        self.dirty.insert(key, Some(value));
    }

    /// Buffer a delete for asynchronous write-back, coalescing with any pending op at `key`.
    pub fn mark_dirty_delete(&mut self, key: Value) {
        self.dirty.insert(key, None);
    }

    /// Number of distinct keys with a pending (coalesced) backing-map mutation.
    pub fn pending_len(&self) -> usize {
        self.dirty.len()
    }

    /// Whether any write-behind mutation is buffered.
    pub fn has_pending(&self) -> bool {
        !self.dirty.is_empty()
    }

    /// Remove and return up to `max` buffered mutations in key order (`None` drains the whole buffer).
    /// Each item is `(key, Some(bytes))` for a put or `(key, None)` for a delete.
    pub fn take_flush_batch(&mut self, max: Option<u64>) -> Vec<(Value, Option<Vec<u8>>)> {
        let take = max
            .map(|m| usize::try_from(m).unwrap_or(usize::MAX))
            .unwrap_or(usize::MAX);
        let keys: Vec<Value> = self.dirty.keys().take(take).cloned().collect();
        keys.into_iter()
            .map(|k| {
                let op = self.dirty.remove(&k).flatten();
                (k, op)
            })
            .collect()
    }

    /// Whether the cache has crossed `pct`% (1..=100) of either capacity bound - the soft flush
    /// threshold. With no bound set, or `pct == 0`, this is always `false` (only the hard bound applies).
    pub fn over_high_water(&self, pct: u8) -> bool {
        if pct == 0 {
            return false;
        }
        let pct = u128::from(pct.min(100));
        let crossed = |bound: Option<u64>, cur: u64| {
            bound.is_some_and(|b| u128::from(cur) * 100 >= u128::from(b) * pct)
        };
        crossed(self.max_entries, self.entries.len() as u64) || crossed(self.max_bytes, self.bytes)
    }

    /// Proactively reclaim expired entries, returning the count reclaimed. Reads also expire lazily;
    /// this is the host GC drive-point that bounds memory between reads on a quiet cache.
    pub fn sweep_expired(&mut self, now_ms: u64) -> usize {
        let before = self.entries.len();
        self.expire(now_ms);
        before - self.entries.len()
    }
}

/// Read through a versioned backing map on cache miss and populate the ephemeral map.
pub fn ephemeral_kv_get_read_through<S: ObjectStore>(
    cache: &mut EphemeralKvMap,
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    key: &Value,
    opts: EphemeralPutOptions,
    now_ms: u64,
) -> Result<Option<Vec<u8>>> {
    if let Some(value) = cache.get(key, now_ms) {
        return Ok(Some(value));
    }
    let Some(value) = kv_get(loom, ns, collection, key)? else {
        return Ok(None);
    };
    cache.put(key.clone(), value.clone(), opts, now_ms)?;
    Ok(Some(value))
}

/// Write synchronously through the versioned backing map, then populate the ephemeral map.
#[allow(clippy::too_many_arguments)]
pub fn ephemeral_kv_put_write_through<S: ObjectStore>(
    cache: &mut EphemeralKvMap,
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    key: Value,
    value: Vec<u8>,
    opts: EphemeralPutOptions,
    now_ms: u64,
) -> Result<()> {
    kv_put(loom, ns, collection, key.clone(), value.clone())?;
    cache.put(key, value, opts, now_ms)
}

fn entry_expired(entry: &EphemeralEntry, now_ms: u64) -> bool {
    if entry
        .expires_at_ms
        .is_some_and(|deadline| now_ms >= deadline)
    {
        return true;
    }
    entry.idle_ttl_ms.is_some_and(|idle| {
        entry
            .last_access_ms
            .checked_add(idle)
            .is_none_or(|deadline| now_ms >= deadline)
    })
}

fn deadline(now_ms: u64, ttl_ms: Option<u64>, label: &str) -> Result<Option<u64>> {
    validate_positive(ttl_ms, label)?;
    ttl_ms
        .map(|ttl| {
            now_ms
                .checked_add(ttl)
                .ok_or_else(|| LoomError::invalid(format!("{label} deadline overflows")))
        })
        .transpose()
}

fn validate_positive(value: Option<u64>, label: &str) -> Result<()> {
    if value == Some(0) {
        Err(LoomError::invalid(format!(
            "{label} must be greater than zero"
        )))
    } else {
        Ok(())
    }
}

fn map_path(collection: &str) -> String {
    facet_path(FacetKind::Kv, collection)
}

fn collection_key(collection: &str) -> String {
    hex::encode(collection.as_bytes())
}

fn structured_value_dir(collection: &str) -> String {
    facet_path(
        FacetKind::Kv,
        &format!(".values/{}", collection_key(collection)),
    )
}

pub(crate) fn structured_value_path(collection: &str, digest: &Digest) -> String {
    facet_path(
        FacetKind::Kv,
        &format!(".values/{}/{}", collection_key(collection), digest.to_hex()),
    )
}

#[derive(Clone, Copy)]
struct KvProllyRoot {
    entries: Option<Digest>,
    anchors: Option<Digest>,
}

fn encode_structured_root<S: ObjectStore>(
    loom: &mut Loom<S>,
    collection: &str,
    map: &KvMap,
) -> Result<(Vec<u8>, BTreeSet<String>)> {
    let algo = loom.store().digest_algo();
    let mut paths = BTreeSet::new();
    let entries = map
        .entries
        .iter()
        .map(|(key, value)| {
            let digest = content_address_with(algo, value);
            paths.insert(structured_value_path(collection, &digest));
            (
                kv_ordered_key(key),
                encode_kv_entry(key, &digest, value.len()),
            )
        })
        .collect::<Vec<_>>();
    let anchors = map
        .anchors
        .iter()
        .map(|(key, anchor)| (kv_ordered_key(key), encode_kv_anchor(key, *anchor)))
        .collect::<Vec<_>>();
    let entries_root = crate::prolly::build(loom.store_mut(), &entries)?;
    let anchors_root = crate::prolly::build(loom.store_mut(), &anchors)?;
    Ok((
        cbor::encode(&cbor::Value::Array(vec![
            cbor::Value::Text(PROLLY_MAP_ROOT_SCHEMA.to_string()),
            cbor::Value::Uint(u64::from(algo.code())),
            optional_digest_value(entries_root.as_ref()),
            optional_digest_value(anchors_root.as_ref()),
        ])),
        paths,
    ))
}

fn decode_prolly_root_as_map<S: ObjectStore, F>(
    store: &S,
    algo: crate::digest::Algo,
    root: Vec<cbor::Value>,
    mut read_component: F,
) -> Result<Option<KvMap>>
where
    F: FnMut(&Digest) -> Result<Vec<u8>>,
{
    let prolly = decode_prolly_root(algo, root)?;
    let mut map = KvMap::new();
    if let Some(anchors_root) = prolly.anchors {
        let mut previous_key: Option<Vec<u8>> = None;
        for (key, value) in crate::prolly::entries(store, &anchors_root)? {
            if previous_key
                .as_ref()
                .is_some_and(|previous| previous >= &key)
            {
                return Err(LoomError::corrupt("KV anchor keys are not strictly sorted"));
            }
            previous_key = Some(key.clone());
            let (decoded_key, anchor) = decode_kv_anchor(&value)?;
            if key != kv_ordered_key(&decoded_key) {
                return Err(LoomError::corrupt("KV anchor key codec mismatch"));
            }
            if map.anchors.insert(decoded_key, anchor).is_some() {
                return Err(LoomError::corrupt("duplicate KV structured root anchor"));
            }
        }
    }
    if let Some(entries_root) = prolly.entries {
        let mut previous_key: Option<Vec<u8>> = None;
        for (key, value) in crate::prolly::entries(store, &entries_root)? {
            if previous_key
                .as_ref()
                .is_some_and(|previous| previous >= &key)
            {
                return Err(LoomError::corrupt("KV entry keys are not strictly sorted"));
            }
            previous_key = Some(key.clone());
            let (decoded_key, digest, len) = decode_kv_entry(algo, &value)?;
            if key != kv_ordered_key(&decoded_key) {
                return Err(LoomError::corrupt("KV entry key codec mismatch"));
            }
            if !map.anchors.contains_key(&decoded_key) {
                return Err(LoomError::corrupt("live KV entry is missing an anchor"));
            }
            let value = read_component(&digest)?;
            if value.len() as u64 != len {
                return Err(LoomError::integrity_failure("KV value length mismatch"));
            }
            let actual = content_address_with(algo, &value);
            if actual != digest {
                return Err(LoomError::integrity_failure("KV value digest mismatch"));
            }
            if map.entries.insert(decoded_key, value).is_some() {
                return Err(LoomError::corrupt("duplicate KV structured root entry"));
            }
        }
    }
    Ok(Some(map))
}

fn digest_from_bytes(algo: crate::digest::Algo, bytes: Vec<u8>) -> Result<Digest> {
    let bytes: [u8; DIGEST_LEN] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| LoomError::corrupt("digest field is not 32 bytes"))?;
    Ok(Digest::of(algo, bytes))
}

fn kv_ordered_key(key: &Value) -> Vec<u8> {
    crate::tabular::encode_pk_values(std::slice::from_ref(key))
}

fn optional_digest_value(digest: Option<&Digest>) -> cbor::Value {
    digest.map_or(cbor::Value::Null, cbor::digest_value)
}

fn optional_digest_from_value(
    algo: crate::digest::Algo,
    value: cbor::Value,
) -> Result<Option<Digest>> {
    match value {
        cbor::Value::Null => Ok(None),
        cbor::Value::Bytes(bytes) => digest_from_bytes(algo, bytes).map(Some),
        _ => Err(LoomError::corrupt("expected KV prolly root digest or null")),
    }
}

fn encode_kv_entry(key: &Value, digest: &Digest, len: usize) -> Vec<u8> {
    cbor::encode(&cbor::Value::Array(vec![
        cell_value(key),
        cbor::Value::Bytes(digest.bytes().to_vec()),
        cbor::Value::Uint(len as u64),
    ]))
}

fn decode_kv_entry(algo: crate::digest::Algo, bytes: &[u8]) -> Result<(Value, Digest, u64)> {
    let mut fields = cbor::Fields::new(cbor::decode_array(bytes)?);
    let key = cell_from(fields.next_field()?)?;
    let digest = digest_from_bytes(algo, fields.bytes()?)?;
    let len = fields.uint()?;
    fields.end()?;
    Ok((key, digest, len))
}

fn encode_kv_anchor(key: &Value, anchor: u64) -> Vec<u8> {
    cbor::encode(&cbor::Value::Array(vec![
        cell_value(key),
        cbor::Value::Uint(anchor),
    ]))
}

fn decode_kv_anchor(bytes: &[u8]) -> Result<(Value, u64)> {
    let mut fields = cbor::Fields::new(cbor::decode_array(bytes)?);
    let key = cell_from(fields.next_field()?)?;
    let anchor = fields.uint()?;
    fields.end()?;
    if anchor == 0 {
        return Err(LoomError::corrupt("KV anchor must be greater than zero"));
    }
    Ok((key, anchor))
}

fn decode_prolly_root(algo: crate::digest::Algo, root: Vec<cbor::Value>) -> Result<KvProllyRoot> {
    let mut fields = cbor::Fields::new(root);
    let schema = fields.text()?;
    if schema != PROLLY_MAP_ROOT_SCHEMA {
        return Err(LoomError::corrupt("not a KV prolly root"));
    }
    let root_algo = crate::digest::Algo::from_code(cbor::u8_from(fields.uint()?)?)?;
    if root_algo != algo {
        return Err(LoomError::corrupt(
            "KV structured root digest profile mismatch",
        ));
    }
    let entries = optional_digest_from_value(algo, fields.next_field()?)?;
    let anchors = optional_digest_from_value(algo, fields.next_field()?)?;
    fields.end()?;
    Ok(KvProllyRoot { entries, anchors })
}

pub(crate) fn prolly_roots_from_storage_bytes(
    algo: crate::digest::Algo,
    bytes: &[u8],
) -> Result<Option<Vec<Digest>>> {
    let Ok(root) = cbor::decode_array(bytes) else {
        return Ok(None);
    };
    match root.first() {
        Some(cbor::Value::Text(schema)) if schema == PROLLY_MAP_ROOT_SCHEMA => {}
        _ => return Ok(None),
    }
    let root = decode_prolly_root(algo, root)?;
    Ok(Some(
        root.entries
            .into_iter()
            .chain(root.anchors)
            .collect::<Vec<_>>(),
    ))
}

fn read_prolly_root<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
) -> Result<Option<KvProllyRoot>> {
    let bytes = match loom.read_file(ns, &map_path(collection)) {
        Ok(bytes) => bytes,
        Err(error) if error.code == Code::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let root = cbor::decode_array(&bytes)?;
    match root.first() {
        Some(cbor::Value::Text(schema)) if schema == PROLLY_MAP_ROOT_SCHEMA => {
            decode_prolly_root(loom.store().digest_algo(), root).map(Some)
        }
        _ => Ok(None),
    }
}

fn encode_prolly_root(algo: crate::digest::Algo, root: KvProllyRoot) -> Vec<u8> {
    cbor::encode(&cbor::Value::Array(vec![
        cbor::Value::Text(PROLLY_MAP_ROOT_SCHEMA.to_string()),
        cbor::Value::Uint(u64::from(algo.code())),
        optional_digest_value(root.entries.as_ref()),
        optional_digest_value(root.anchors.as_ref()),
    ]))
}

fn write_prolly_root<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    root: KvProllyRoot,
) -> Result<()> {
    loom.create_directory_reserved(ns, &facet_root(FacetKind::Kv), true)?;
    loom.create_directory_reserved(ns, &structured_value_dir(collection), true)?;
    let bytes = encode_prolly_root(loom.store().digest_algo(), root);
    loom.write_file_reserved(ns, &map_path(collection), &bytes, 0o100644)
}

fn kv_anchor_from_root<S: ObjectStore>(
    loom: &Loom<S>,
    root: &KvProllyRoot,
    key: &Value,
) -> Result<u64> {
    let Some(anchors_root) = root.anchors else {
        return Ok(0);
    };
    let Some(encoded) = crate::prolly::get(loom.store(), &anchors_root, &kv_ordered_key(key))?
    else {
        return Ok(0);
    };
    let (decoded_key, anchor) = decode_kv_anchor(&encoded)?;
    if decoded_key != *key {
        return Err(LoomError::corrupt("KV anchor key codec mismatch"));
    }
    Ok(anchor)
}

fn advance_kv_anchor(anchor: u64) -> Result<u64> {
    anchor
        .checked_add(1)
        .ok_or_else(|| LoomError::new(Code::Conflict, "KV mutation anchor exhausted"))
}

fn kv_put_prolly<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    mut root: KvProllyRoot,
    key: Value,
    value: Vec<u8>,
) -> Result<()> {
    let anchor = advance_kv_anchor(kv_anchor_from_root(loom, &root, &key)?)?;
    let digest = content_address_with(loom.store().digest_algo(), &value);
    loom.create_directory_reserved(ns, &facet_root(FacetKind::Kv), true)?;
    loom.create_directory_reserved(ns, &structured_value_dir(collection), true)?;
    loom.write_file_reserved(
        ns,
        &structured_value_path(collection, &digest),
        &value,
        0o100644,
    )?;
    let ordered_key = kv_ordered_key(&key);
    root.entries = Some(crate::prolly::insert(
        loom.store_mut(),
        root.entries.as_ref(),
        &ordered_key,
        &encode_kv_entry(&key, &digest, value.len()),
    )?);
    root.anchors = Some(crate::prolly::insert(
        loom.store_mut(),
        root.anchors.as_ref(),
        &ordered_key,
        &encode_kv_anchor(&key, anchor),
    )?);
    write_prolly_root(loom, ns, collection, root)
}

fn kv_delete_prolly<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    mut root: KvProllyRoot,
    key: &Value,
) -> Result<bool> {
    let Some(entries_root) = root.entries else {
        return Ok(false);
    };
    let ordered_key = kv_ordered_key(key);
    if crate::prolly::get(loom.store(), &entries_root, &ordered_key)?.is_none() {
        return Ok(false);
    }
    let anchor = advance_kv_anchor(kv_anchor_from_root(loom, &root, key)?)?;
    root.entries = crate::prolly::remove(loom.store_mut(), &entries_root, &ordered_key)?;
    root.anchors = Some(crate::prolly::insert(
        loom.store_mut(),
        root.anchors.as_ref(),
        &ordered_key,
        &encode_kv_anchor(key, anchor),
    )?);
    write_prolly_root(loom, ns, collection, root)?;
    Ok(true)
}

pub(crate) fn decode_kv_storage_with_components<F>(
    _algo: crate::digest::Algo,
    bytes: &[u8],
    _read_component: F,
) -> Result<KvMap>
where
    F: FnMut(&Digest) -> Result<Vec<u8>>,
{
    let root = cbor::decode_array(bytes)?;
    if matches!(
        root.first(),
        Some(cbor::Value::Text(schema)) if schema == PROLLY_MAP_ROOT_SCHEMA
    ) {
        return Err(LoomError::corrupt(
            "KV prolly root requires an object store decoder",
        ));
    }
    if matches!(root.first(), Some(cbor::Value::Text(_))) {
        return Err(LoomError::corrupt("unsupported KV storage root schema"));
    }
    KvMap::decode(bytes)
}

pub(crate) fn decode_kv_storage_with_store<S: ObjectStore, F>(
    store: &S,
    algo: crate::digest::Algo,
    _collection: &str,
    bytes: &[u8],
    read_component: F,
) -> Result<KvMap>
where
    F: FnMut(&Digest) -> Result<Vec<u8>>,
{
    let root = cbor::decode_array(bytes)?;
    if matches!(
        root.first(),
        Some(cbor::Value::Text(schema)) if schema == PROLLY_MAP_ROOT_SCHEMA
    ) {
        return decode_prolly_root_as_map(store, algo, root, read_component)?
            .ok_or_else(|| LoomError::corrupt("KV prolly root did not decode"));
    }
    decode_kv_storage_with_components(algo, bytes, read_component)
}

fn decode_kv_storage<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    bytes: &[u8],
) -> Result<KvMap> {
    decode_kv_storage_with_store(
        loom.store(),
        loom.store().digest_algo(),
        collection,
        bytes,
        |digest| loom.read_file_reserved(ns, &structured_value_path(collection, digest)),
    )
}

/// Replace the whole map under `collection` in `ns` as one atomic collection-scoped mutation.
/// Every retained entry anchor advances, invalidating every exact token in the collection.
pub fn replace_kv_map<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    map: &KvMap,
) -> Result<()> {
    let current = load_or_empty(loom, ns, collection)?;
    let mut map = map.clone();
    map.anchors = current.anchors.clone();
    let keys: BTreeSet<Value> = current
        .anchors
        .keys()
        .chain(map.entries.keys())
        .cloned()
        .collect();
    for key in keys {
        map.advance_anchor(&key)?;
    }
    stage_kv(loom, ns, collection, &map)
}

fn stage_kv<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    map: &KvMap,
) -> Result<()> {
    loom.create_directory_reserved(ns, &facet_root(FacetKind::Kv), true)?;
    loom.create_directory_reserved(ns, &structured_value_dir(collection), true)?;
    for value in map.entries.values() {
        let digest = content_address_with(loom.store().digest_algo(), value);
        loom.write_file_reserved(
            ns,
            &structured_value_path(collection, &digest),
            value,
            0o100644,
        )?;
    }
    let (root, live_paths) = encode_structured_root(loom, collection, map)?;
    let value_prefix = format!("{}/", structured_value_dir(collection));
    for path in loom.staged_paths(ns) {
        if path.starts_with(&value_prefix) && !live_paths.contains(&path) {
            loom.remove_file_reserved(ns, &path)?;
        }
    }
    loom.write_file_reserved(ns, &map_path(collection), &root, 0o100644)
}

/// Load the map named `collection` from `ns`'s current working tree, or `NOT_FOUND`.
pub fn get_kv<S: ObjectStore>(loom: &Loom<S>, ns: WorkspaceId, collection: &str) -> Result<KvMap> {
    decode_kv_storage(
        loom,
        ns,
        collection,
        &loom.read_file(ns, &map_path(collection))?,
    )
}

/// Load the map named `collection`, or an empty map when it does not exist yet. The facade reads treat an
/// absent map as empty rather than an error (a missing key and a missing map are both "absent").
fn load_or_empty<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
) -> Result<KvMap> {
    match loom.read_file(ns, &map_path(collection)) {
        Ok(bytes) => decode_kv_storage(loom, ns, collection, &bytes),
        Err(e) if e.code == Code::NotFound => Ok(KvMap::new()),
        Err(e) => Err(e),
    }
}

/// Put `value` at typed `key` in the map named `collection` in `ns` (selected by the caller's workspace
/// selector), creating the map and the `kv` facet if absent, and stage the updated map. A later put at
/// the same key replaces the value.
pub fn kv_put<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    key: Value,
    value: Vec<u8>,
) -> Result<()> {
    if let Some(root) = read_prolly_root(loom, ns, collection)? {
        return kv_put_prolly(loom, ns, collection, root, key, value);
    }
    let mut map = load_or_empty(loom, ns, collection)?;
    map.entries.insert(key.clone(), value);
    map.advance_anchor(&key)?;
    stage_kv(loom, ns, collection, &map)
}

/// Return an owner-issued exact-comparison token for the current entry.
pub fn kv_exact_token<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    key: &Value,
) -> Result<Option<KvExactToken>> {
    if let Some(root) = read_prolly_root(loom, ns, collection)? {
        return if kv_get(loom, ns, collection, key)?.is_some() {
            Ok(Some(kv_exact_token_for(
                ns,
                collection,
                key,
                kv_anchor_from_root(loom, &root, key)?,
            )))
        } else {
            Ok(None)
        };
    }
    let map = load_or_empty(loom, ns, collection)?;
    Ok(map
        .get(key)
        .map(|_| kv_exact_token_for(ns, collection, key, map.anchor(key))))
}

/// Conditionally put `value` at `key` using one atomic map read and write.
pub fn kv_put_conditioned<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    key: Value,
    value: Vec<u8>,
    condition: KvCondition,
) -> Result<()> {
    if let Some(root) = read_prolly_root(loom, ns, collection)? {
        let current = kv_get(loom, ns, collection, &key)?;
        let anchor = kv_anchor_from_root(loom, &root, &key)?;
        check_kv_condition(ns, collection, &key, current.as_deref(), anchor, condition)?;
        return kv_put_prolly(loom, ns, collection, root, key, value);
    }
    let mut map = load_or_empty(loom, ns, collection)?;
    check_kv_condition(
        ns,
        collection,
        &key,
        map.get(&key),
        map.anchor(&key),
        condition,
    )?;
    map.entries.insert(key.clone(), value);
    map.advance_anchor(&key)?;
    stage_kv(loom, ns, collection, &map)
}

/// The value at typed `key` in `collection`, or `None` when the key or the map is absent.
pub fn kv_get<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    key: &Value,
) -> Result<Option<Vec<u8>>> {
    if let Some(root) = read_prolly_root(loom, ns, collection)? {
        let Some(entries_root) = root.entries else {
            return Ok(None);
        };
        let Some(encoded) = crate::prolly::get(loom.store(), &entries_root, &kv_ordered_key(key))?
        else {
            return Ok(None);
        };
        let (decoded_key, digest, len) = decode_kv_entry(loom.store().digest_algo(), &encoded)?;
        if decoded_key != *key {
            return Err(LoomError::corrupt("KV entry key codec mismatch"));
        }
        let value = loom.read_file_reserved(ns, &structured_value_path(collection, &digest))?;
        if value.len() as u64 != len {
            return Err(LoomError::integrity_failure("KV value length mismatch"));
        }
        let actual = content_address_with(loom.store().digest_algo(), &value);
        if actual != digest {
            return Err(LoomError::integrity_failure("KV value digest mismatch"));
        }
        return Ok(Some(value));
    }
    Ok(load_or_empty(loom, ns, collection)?
        .get(key)
        .map(<[u8]>::to_vec))
}

/// Remove typed `key` from `collection`; returns whether it was present. A no-op (absent key or map) does not
/// write.
pub fn kv_delete<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    key: &Value,
) -> Result<bool> {
    if let Some(root) = read_prolly_root(loom, ns, collection)? {
        return kv_delete_prolly(loom, ns, collection, root, key);
    }
    let mut map = load_or_empty(loom, ns, collection)?;
    let present = map.delete(key);
    if present {
        map.advance_anchor(key)?;
        stage_kv(loom, ns, collection, &map)?;
    }
    Ok(present)
}

/// Conditionally remove `key`; returns whether it was present when the condition passed.
pub fn kv_delete_conditioned<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    key: &Value,
    condition: KvCondition,
) -> Result<bool> {
    if let Some(root) = read_prolly_root(loom, ns, collection)? {
        let current = kv_get(loom, ns, collection, key)?;
        let anchor = kv_anchor_from_root(loom, &root, key)?;
        check_kv_condition(ns, collection, key, current.as_deref(), anchor, condition)?;
        return kv_delete_prolly(loom, ns, collection, root, key);
    }
    let mut map = load_or_empty(loom, ns, collection)?;
    check_kv_condition(
        ns,
        collection,
        key,
        map.get(key),
        map.anchor(key),
        condition,
    )?;
    let present = map.delete(key);
    if present {
        map.advance_anchor(key)?;
        stage_kv(loom, ns, collection, &map)?;
    }
    Ok(present)
}

fn check_kv_condition(
    ns: WorkspaceId,
    collection: &str,
    key: &Value,
    current: Option<&[u8]>,
    anchor: u64,
    condition: KvCondition,
) -> Result<()> {
    match condition {
        KvCondition::Any => Ok(()),
        KvCondition::Absent if current.is_none() => Ok(()),
        KvCondition::Absent => Err(LoomError::new(
            Code::AlreadyExists,
            "KV entry already exists",
        )),
        KvCondition::Exact(token)
            if current
                .is_some_and(|_| token == kv_exact_token_for(ns, collection, key, anchor)) =>
        {
            Ok(())
        }
        KvCondition::Exact(_) => Err(LoomError::new(
            Code::Conflict,
            "KV exact condition did not match",
        )),
    }
}

fn kv_exact_token_for(ns: WorkspaceId, collection: &str, key: &Value, anchor: u64) -> KvExactToken {
    KvExactToken(cbor::encode(&cbor::Value::Array(vec![
        cbor::Value::Bytes(ns.as_bytes().to_vec()),
        cbor::Value::Text(collection.to_owned()),
        cell_value(key),
        cbor::Value::Uint(anchor),
    ])))
}

/// The whole map named `collection` in key order, or an empty map when absent.
pub fn kv_list<S: ObjectStore>(loom: &Loom<S>, ns: WorkspaceId, collection: &str) -> Result<KvMap> {
    load_or_empty(loom, ns, collection)
}

/// The entries of `collection` with `lo <= key < hi`, in key order (half-open), as a sub-map.
pub fn kv_range<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    lo: &Value,
    hi: &Value,
) -> Result<KvMap> {
    if let Some(root) = read_prolly_root(loom, ns, collection)? {
        let mut out = KvMap::new();
        let Some(entries_root) = root.entries else {
            return Ok(out);
        };
        let mut cursor = crate::prolly::ProllyCursor::open_range(
            loom.store(),
            &entries_root,
            Some(&kv_ordered_key(lo)),
            Some(kv_ordered_key(hi)),
        )?;
        while let Some((ordered_key, encoded)) = cursor.next()? {
            let (key, digest, len) = decode_kv_entry(loom.store().digest_algo(), &encoded)?;
            if ordered_key != kv_ordered_key(&key) {
                return Err(LoomError::corrupt("KV entry key codec mismatch"));
            }
            let value = loom.read_file_reserved(ns, &structured_value_path(collection, &digest))?;
            if value.len() as u64 != len {
                return Err(LoomError::integrity_failure("KV value length mismatch"));
            }
            if content_address_with(loom.store().digest_algo(), &value) != digest {
                return Err(LoomError::integrity_failure("KV value digest mismatch"));
            }
            out.put(key, value);
        }
        return Ok(out);
    }
    let map = load_or_empty(loom, ns, collection)?;
    let mut out = KvMap::new();
    for (k, v) in map.range(lo, hi) {
        out.put(k.clone(), v.to_vec());
    }
    Ok(out)
}

/// The KV map collection names present in `ns`'s current working tree, sorted and de-duplicated.
/// Enumeration is within the workspace, not a global index. Reserved names beginning with `.` (such
/// as the `.config` subtree that holds durable map configs) are not collections and are excluded.
pub fn kv_list_collections<S: ObjectStore>(loom: &Loom<S>, ns: WorkspaceId) -> Result<Vec<String>> {
    let prefix = format!("{}/", facet_root(FacetKind::Kv));
    let mut out: Vec<String> = loom
        .staged_paths(ns)
        .into_iter()
        .filter_map(|p| {
            let rest = p.strip_prefix(&prefix)?;
            if rest.contains('/') || rest.starts_with('.') {
                return None;
            }
            Some(rest.to_string())
        })
        .collect();
    out.sort();
    out.dedup();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::memory::MemoryStore;
    use crate::workspace::{FacetKind, WorkspaceId};

    #[test]
    fn list_collections_enumerates_map_names_sorted() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Kv, None, WorkspaceId::from_bytes([5; 16]))
            .unwrap();
        assert!(kv_list_collections(&loom, ns).unwrap().is_empty());
        kv_put(
            &mut loom,
            ns,
            "beta",
            Value::Text("k".into()),
            b"1".to_vec(),
        )
        .unwrap();
        kv_put(
            &mut loom,
            ns,
            "alpha",
            Value::Text("k1".into()),
            b"1".to_vec(),
        )
        .unwrap();
        kv_put(
            &mut loom,
            ns,
            "alpha",
            Value::Text("k2".into()),
            b"2".to_vec(),
        )
        .unwrap();
        assert_eq!(
            kv_list_collections(&loom, ns).unwrap(),
            vec!["alpha", "beta"]
        );
    }

    #[test]
    fn typed_keys_scan_in_value_order_not_lexical() {
        let mut m = KvMap::new();
        m.put(Value::Int(10), b"ten".to_vec());
        m.put(Value::Int(2), b"two".to_vec());
        m.put(Value::Int(1), b"one".to_vec());
        // Integer order: 1, 2, 10 (the `2` vs `10` footgun that opaque-bytes-only would get wrong).
        let keys: Vec<i64> = m
            .iter()
            .map(|(k, _)| match k {
                Value::Int(i) => *i,
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(keys, [1, 2, 10]);
        assert_eq!(m.get(&Value::Int(2)), Some(&b"two"[..]));
        assert!(m.delete(&Value::Int(2)));
        assert_eq!(m.get(&Value::Int(2)), None);
    }

    #[test]
    fn range_scan_is_half_open() {
        let mut m = KvMap::new();
        for i in 0..10 {
            m.put(Value::Int(i), vec![i as u8]);
        }
        let r: Vec<i64> = m
            .range(&Value::Int(3), &Value::Int(6))
            .into_iter()
            .map(|(k, _)| match k {
                Value::Int(i) => *i,
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(r, [3, 4, 5]); // 6 excluded
    }

    #[test]
    fn encode_round_trips_and_versions() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Kv, None, WorkspaceId::from_bytes([3; 16]))
            .unwrap();
        let mut m = KvMap::new();
        m.put(Value::Text("a".into()), b"1".to_vec());
        m.put(Value::Text("b".into()), b"2".to_vec());
        assert_eq!(KvMap::decode(&m.encode()).unwrap().len(), 2);

        replace_kv_map(&mut loom, ns, "main", &m).unwrap();
        let c1 = loom.commit(ns, "nas", "two keys", 1).unwrap();
        m.put(Value::Text("c".into()), b"3".to_vec());
        replace_kv_map(&mut loom, ns, "main", &m).unwrap();
        loom.commit(ns, "nas", "three keys", 2).unwrap();
        assert_eq!(get_kv(&loom, ns, "main").unwrap().len(), 3);
        loom.checkout_commit(ns, c1).unwrap();
        assert_eq!(get_kv(&loom, ns, "main").unwrap().len(), 2);
    }

    #[test]
    fn durable_storage_uses_structured_root_and_value_components() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Kv, None, WorkspaceId::from_bytes([12; 16]))
            .unwrap();

        kv_put(&mut loom, ns, "m", Value::Text("b".into()), b"two".to_vec()).unwrap();
        kv_put(&mut loom, ns, "m", Value::Text("a".into()), b"one".to_vec()).unwrap();

        let root = loom.read_file_reserved(ns, &map_path("m")).unwrap();
        let mut fields = cbor::Fields::new(cbor::decode_array(&root).unwrap());
        assert_eq!(fields.text().unwrap(), PROLLY_MAP_ROOT_SCHEMA);
        assert_eq!(
            fields.uint().unwrap(),
            u64::from(loom.store().digest_algo().code())
        );
        let entries_root =
            digest_from_bytes(loom.store().digest_algo(), fields.bytes().unwrap()).unwrap();
        let anchors_root =
            digest_from_bytes(loom.store().digest_algo(), fields.bytes().unwrap()).unwrap();
        fields.end().unwrap();
        let entries = crate::prolly::entries(loom.store(), &entries_root).unwrap();
        let anchors = crate::prolly::entries(loom.store(), &anchors_root).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(anchors.len(), 2);

        let (first_key, _, _) = decode_kv_entry(loom.store().digest_algo(), &entries[0].1).unwrap();
        assert_eq!(entries[0].0, kv_ordered_key(&Value::Text("a".into())));
        assert_eq!(first_key, Value::Text("a".into()));
        let value_digest = content_address_with(loom.store().digest_algo(), b"one");
        assert_eq!(
            loom.read_file_reserved(ns, &structured_value_path("m", &value_digest))
                .unwrap(),
            b"one"
        );
        assert_eq!(
            kv_list(&loom, ns, "m")
                .unwrap()
                .iter()
                .map(|(k, v)| (k.clone(), v.to_vec()))
                .collect::<Vec<_>>(),
            vec![
                (Value::Text("a".into()), b"one".to_vec()),
                (Value::Text("b".into()), b"two".to_vec())
            ]
        );
    }

    #[test]
    fn structured_root_bytes_are_canonical_for_key_order() {
        let mut left = Loom::new(MemoryStore::new());
        let left_ns = left
            .registry_mut()
            .create(FacetKind::Kv, None, WorkspaceId::from_bytes([13; 16]))
            .unwrap();
        kv_put(&mut left, left_ns, "m", Value::Int(2), b"two".to_vec()).unwrap();
        kv_put(&mut left, left_ns, "m", Value::Int(1), b"one".to_vec()).unwrap();

        let mut right = Loom::new(MemoryStore::new());
        let right_ns = right
            .registry_mut()
            .create(FacetKind::Kv, None, WorkspaceId::from_bytes([14; 16]))
            .unwrap();
        kv_put(&mut right, right_ns, "m", Value::Int(1), b"one".to_vec()).unwrap();
        kv_put(&mut right, right_ns, "m", Value::Int(2), b"two".to_vec()).unwrap();

        assert_eq!(
            left.read_file_reserved(left_ns, &map_path("m")).unwrap(),
            right.read_file_reserved(right_ns, &map_path("m")).unwrap()
        );
    }

    #[test]
    fn one_key_update_shares_most_prolly_nodes() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Kv, None, WorkspaceId::from_bytes([16; 16]))
            .unwrap();
        for i in 0..400 {
            kv_put(
                &mut loom,
                ns,
                "m",
                Value::Text(format!("key-{i:04}")),
                format!("value-{i}").into_bytes(),
            )
            .unwrap();
        }
        let before_root = kv_entries_root(&loom, ns, "m");
        let before_nodes: BTreeSet<_> = crate::prolly::reachable_nodes(loom.store(), &before_root)
            .unwrap()
            .into_iter()
            .collect();

        kv_put(
            &mut loom,
            ns,
            "m",
            Value::Text("key-0200".into()),
            b"changed".to_vec(),
        )
        .unwrap();
        let after_root = kv_entries_root(&loom, ns, "m");
        let after_nodes: BTreeSet<_> = crate::prolly::reachable_nodes(loom.store(), &after_root)
            .unwrap()
            .into_iter()
            .collect();
        let shared = before_nodes.intersection(&after_nodes).count();
        assert!(
            shared * 2 > before_nodes.len(),
            "expected a one-key KV update to share most prolly nodes: shared={shared} of {}",
            before_nodes.len()
        );
    }

    #[test]
    fn structured_root_rejects_bad_value_components() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Kv, None, WorkspaceId::from_bytes([15; 16]))
            .unwrap();
        kv_put(&mut loom, ns, "m", Value::Text("a".into()), b"one".to_vec()).unwrap();
        let digest = content_address_with(loom.store().digest_algo(), b"one");
        loom.write_file_reserved(ns, &structured_value_path("m", &digest), b"bad", 0o100644)
            .unwrap();

        assert_eq!(
            get_kv(&loom, ns, "m").unwrap_err().code,
            Code::IntegrityFailure
        );
    }

    fn kv_entries_root(loom: &Loom<MemoryStore>, ns: WorkspaceId, collection: &str) -> Digest {
        let root = loom.read_file_reserved(ns, &map_path(collection)).unwrap();
        let mut fields = cbor::Fields::new(cbor::decode_array(&root).unwrap());
        assert_eq!(fields.text().unwrap(), PROLLY_MAP_ROOT_SCHEMA);
        assert_eq!(
            fields.uint().unwrap(),
            u64::from(loom.store().digest_algo().code())
        );
        digest_from_bytes(loom.store().digest_algo(), fields.bytes().unwrap()).unwrap()
    }

    #[test]
    fn facade_put_get_delete_and_absent() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Kv, None, WorkspaceId::from_bytes([4; 16]))
            .unwrap();

        // An absent map reads as absent, not an error.
        assert_eq!(kv_get(&loom, ns, "m", &Value::Int(1)).unwrap(), None);
        assert_eq!(kv_list(&loom, ns, "m").unwrap().len(), 0);

        kv_put(&mut loom, ns, "m", Value::Int(1), b"one".to_vec()).unwrap();
        kv_put(&mut loom, ns, "m", Value::Int(2), b"two".to_vec()).unwrap();
        assert_eq!(
            kv_get(&loom, ns, "m", &Value::Int(1)).unwrap().as_deref(),
            Some(&b"one"[..])
        );
        // A later put replaces the value.
        kv_put(&mut loom, ns, "m", Value::Int(1), b"uno".to_vec()).unwrap();
        assert_eq!(
            kv_get(&loom, ns, "m", &Value::Int(1)).unwrap().as_deref(),
            Some(&b"uno"[..])
        );

        // Delete reports presence and is a no-op when absent.
        assert!(kv_delete(&mut loom, ns, "m", &Value::Int(1)).unwrap());
        assert!(!kv_delete(&mut loom, ns, "m", &Value::Int(1)).unwrap());
        assert_eq!(kv_get(&loom, ns, "m", &Value::Int(1)).unwrap(), None);
    }

    #[test]
    fn conditional_mutations_preserve_entry_on_failed_condition() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Kv, None, WorkspaceId::from_bytes([9; 16]))
            .unwrap();
        let key = Value::Text("entry".into());

        kv_put_conditioned(
            &mut loom,
            ns,
            "m",
            key.clone(),
            b"one".to_vec(),
            KvCondition::Absent,
        )
        .unwrap();
        let token = kv_exact_token(&loom, ns, "m", &key).unwrap().unwrap();

        let error = kv_put_conditioned(
            &mut loom,
            ns,
            "m",
            key.clone(),
            b"two".to_vec(),
            KvCondition::Absent,
        )
        .unwrap_err();
        assert_eq!(error.code, Code::AlreadyExists);
        assert_eq!(
            kv_get(&loom, ns, "m", &key).unwrap().as_deref(),
            Some(&b"one"[..])
        );

        kv_put_conditioned(
            &mut loom,
            ns,
            "m",
            key.clone(),
            b"two".to_vec(),
            KvCondition::Exact(token.clone()),
        )
        .unwrap();

        let error =
            kv_delete_conditioned(&mut loom, ns, "m", &key, KvCondition::Exact(token)).unwrap_err();
        assert_eq!(error.code, Code::Conflict);
        assert_eq!(
            kv_get(&loom, ns, "m", &key).unwrap().as_deref(),
            Some(&b"two"[..])
        );
    }

    #[test]
    fn exact_tokens_stale_after_every_durable_mutation_and_legacy_promotes() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Kv, None, WorkspaceId::from_bytes([10; 16]))
            .unwrap();
        let key = Value::Text("entry".into());

        let legacy = cbor::encode(&cbor::Value::Array(vec![cbor::Value::Array(vec![
            cell_value(&key),
            cbor::Value::Bytes(b"one".to_vec()),
        ])]));
        loom.create_directory_reserved(ns, &facet_root(FacetKind::Kv), true)
            .unwrap();
        loom.write_file_reserved(ns, &map_path("m"), &legacy, 0o100644)
            .unwrap();
        assert_eq!(get_kv(&loom, ns, "m").unwrap().anchor(&key), 0);

        let legacy_map = get_kv(&loom, ns, "m").unwrap();
        replace_kv_map(&mut loom, ns, "m", &legacy_map).unwrap();
        let root = loom.read_file_reserved(ns, &map_path("m")).unwrap();
        let mut root_fields = cbor::Fields::new(cbor::decode_array(&root).unwrap());
        assert_eq!(root_fields.text().unwrap(), PROLLY_MAP_ROOT_SCHEMA);
        let promoted = get_kv(&loom, ns, "m").unwrap();
        assert_eq!(promoted.anchor(&key), 1);
        assert_eq!(promoted.get(&key), Some(&b"one"[..]));

        let token = kv_exact_token(&loom, ns, "m", &key).unwrap().unwrap();
        kv_put(
            &mut loom,
            ns,
            "m",
            Value::Text("other".into()),
            b"other".to_vec(),
        )
        .unwrap();
        kv_put_conditioned(
            &mut loom,
            ns,
            "m",
            key.clone(),
            b"one".to_vec(),
            KvCondition::Exact(token),
        )
        .unwrap();
        let token = kv_exact_token(&loom, ns, "m", &key).unwrap().unwrap();
        kv_put(&mut loom, ns, "m", key.clone(), b"one".to_vec()).unwrap();
        assert_eq!(get_kv(&loom, ns, "m").unwrap().anchor(&key), 3);
        assert_eq!(
            kv_put_conditioned(
                &mut loom,
                ns,
                "m",
                key.clone(),
                b"two".to_vec(),
                KvCondition::Exact(token),
            )
            .unwrap_err()
            .code,
            Code::Conflict
        );

        let token = kv_exact_token(&loom, ns, "m", &key).unwrap().unwrap();
        assert!(kv_delete(&mut loom, ns, "m", &key).unwrap());
        kv_put(&mut loom, ns, "m", key.clone(), b"one".to_vec()).unwrap();
        assert_eq!(get_kv(&loom, ns, "m").unwrap().anchor(&key), 5);
        assert_eq!(
            kv_put_conditioned(
                &mut loom,
                ns,
                "m",
                key.clone(),
                b"two".to_vec(),
                KvCondition::Exact(token),
            )
            .unwrap_err()
            .code,
            Code::Conflict
        );
    }

    #[test]
    fn legacy_structured_root_is_rejected() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Kv, None, WorkspaceId::from_bytes([17; 16]))
            .unwrap();
        let key = Value::Text("legacy".into());
        let digest = content_address_with(loom.store().digest_algo(), b"one");
        loom.create_directory_reserved(ns, &facet_root(FacetKind::Kv), true)
            .unwrap();
        loom.create_directory_reserved(ns, &structured_value_dir("m"), true)
            .unwrap();
        loom.write_file_reserved(ns, &structured_value_path("m", &digest), b"one", 0o100644)
            .unwrap();
        let legacy_root = cbor::encode(&cbor::Value::Array(vec![
            cbor::Value::Text("loom.kv.structured-map-root.v1".to_string()),
            cbor::Value::Uint(u64::from(loom.store().digest_algo().code())),
            cbor::Value::Array(vec![cbor::Value::Array(vec![
                cell_value(&key),
                cbor::Value::Bytes(digest.bytes().to_vec()),
                cbor::Value::Uint(3),
            ])]),
            cbor::Value::Array(vec![cbor::Value::Array(vec![
                cell_value(&key),
                cbor::Value::Uint(1),
            ])]),
        ]));
        loom.write_file_reserved(ns, &map_path("m"), &legacy_root, 0o100644)
            .unwrap();
        assert_eq!(
            kv_get(&loom, ns, "m", &key).unwrap_err().code,
            Code::CorruptObject
        );
    }

    #[test]
    fn first_whole_map_replacement_assigns_generation_one() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Kv, None, WorkspaceId::from_bytes([11; 16]))
            .unwrap();
        let key = Value::Text("entry".into());
        let mut replacement = KvMap::new();
        replacement.put(key.clone(), b"one".to_vec());

        replace_kv_map(&mut loom, ns, "m", &replacement).unwrap();
        assert_eq!(get_kv(&loom, ns, "m").unwrap().anchor(&key), 1);
    }

    #[test]
    fn facade_range_is_half_open_in_value_order() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Kv, None, WorkspaceId::from_bytes([5; 16]))
            .unwrap();
        for i in 0..12 {
            kv_put(&mut loom, ns, "m", Value::Int(i), vec![i as u8]).unwrap();
        }
        let keys: Vec<i64> = kv_range(&loom, ns, "m", &Value::Int(2), &Value::Int(5))
            .unwrap()
            .iter()
            .map(|(k, _)| match k {
                Value::Int(i) => *i,
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(keys, [2, 3, 4]); // 5 excluded, numeric order
    }

    #[test]
    fn ephemeral_kv_expires_by_ttl_and_idle_ttl() {
        let mut cache = EphemeralKvMap::new();
        cache
            .put(
                Value::Text("a".into()),
                b"alpha".to_vec(),
                EphemeralPutOptions {
                    ttl_ms: Some(20),
                    idle_ttl_ms: Some(10),
                },
                100,
            )
            .unwrap();
        assert_eq!(
            cache.get(&Value::Text("a".into()), 105).as_deref(),
            Some(&b"alpha"[..])
        );
        assert_eq!(
            cache.get(&Value::Text("a".into()), 114).as_deref(),
            Some(&b"alpha"[..])
        );
        assert_eq!(cache.get(&Value::Text("a".into()), 120), None);
    }

    #[test]
    fn ephemeral_read_through_populates_from_versioned_backing() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Kv, None, WorkspaceId::from_bytes([6; 16]))
            .unwrap();
        kv_put(
            &mut loom,
            ns,
            "backing",
            Value::Text("k".into()),
            b"value".to_vec(),
        )
        .unwrap();

        let mut cache = EphemeralKvMap::new();
        let value = ephemeral_kv_get_read_through(
            &mut cache,
            &loom,
            ns,
            "backing",
            &Value::Text("k".into()),
            EphemeralPutOptions {
                ttl_ms: Some(50),
                idle_ttl_ms: None,
            },
            10,
        )
        .unwrap();
        assert_eq!(value.as_deref(), Some(&b"value"[..]));
        assert_eq!(
            cache.get(&Value::Text("k".into()), 20).as_deref(),
            Some(&b"value"[..])
        );
    }

    #[test]
    fn ephemeral_write_through_enters_versioned_history() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Kv, None, WorkspaceId::from_bytes([7; 16]))
            .unwrap();
        let mut cache = EphemeralKvMap::new();
        ephemeral_kv_put_write_through(
            &mut cache,
            &mut loom,
            ns,
            "backing",
            Value::Text("k".into()),
            b"value".to_vec(),
            EphemeralPutOptions::default(),
            10,
        )
        .unwrap();
        let tip = loom.commit(ns, "nas", "write-through", 11).unwrap();
        assert_eq!(
            kv_get(&loom, ns, "backing", &Value::Text("k".into()))
                .unwrap()
                .as_deref(),
            Some(&b"value"[..])
        );
        assert_eq!(
            cache.get(&Value::Text("k".into()), 12).as_deref(),
            Some(&b"value"[..])
        );
        loom.checkout_commit(ns, tip).unwrap();
        assert_eq!(
            kv_get(&loom, ns, "backing", &Value::Text("k".into()))
                .unwrap()
                .as_deref(),
            Some(&b"value"[..])
        );
    }

    #[test]
    fn configured_ephemeral_map_is_runtime_only_and_config_persists() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Kv, None, WorkspaceId::from_bytes([8; 16]))
            .unwrap();
        loom.configure_kv_map(
            ns,
            "cache",
            KvMapConfig {
                tier: KvTier::Ephemeral,
                default_put: EphemeralPutOptions {
                    ttl_ms: Some(100),
                    idle_ttl_ms: None,
                },
                read_through: false,
                write_through: false,
                max_entries: None,
                max_bytes: None,
                eviction: EvictionPolicy::None,
                on_evict: OnEvict::Drop,
                write_behind: false,
                write_around: false,
                back_pressure: BackPressure::Block,
                flush_high_water_pct: None,
                flush_batch: None,
            },
        )
        .unwrap();
        loom.kv_put_configured(
            ns,
            "cache",
            Value::Text("k".into()),
            b"cached".to_vec(),
            None,
            10,
        )
        .unwrap();
        assert_eq!(
            loom.kv_get_configured(ns, "cache", &Value::Text("k".into()), 20)
                .unwrap()
                .as_deref(),
            Some(&b"cached"[..])
        );
        let listed = loom.kv_list_configured(ns, "cache", 20).unwrap();
        assert_eq!(listed.get(&Value::Text("k".into())), Some(&b"cached"[..]));
        assert_eq!(
            kv_get(&loom, ns, "cache", &Value::Text("k".into())).unwrap(),
            None
        );

        let root = loom.save_state().unwrap();
        let store = loom.into_store();
        let mut loaded = Loom::new(store);
        loaded.load_state(root).unwrap();
        assert_eq!(loaded.kv_map_config(ns, "cache").tier, KvTier::Ephemeral);
        assert_eq!(
            loaded
                .kv_get_configured(ns, "cache", &Value::Text("k".into()), 20)
                .unwrap(),
            None
        );
    }

    #[test]
    fn configured_ephemeral_write_through_enters_backing_history() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Kv, None, WorkspaceId::from_bytes([9; 16]))
            .unwrap();
        loom.configure_kv_map(
            ns,
            "cache",
            KvMapConfig {
                tier: KvTier::Ephemeral,
                default_put: EphemeralPutOptions::default(),
                read_through: true,
                write_through: true,
                max_entries: None,
                max_bytes: None,
                eviction: EvictionPolicy::None,
                on_evict: OnEvict::Drop,
                write_behind: false,
                write_around: false,
                back_pressure: BackPressure::Block,
                flush_high_water_pct: None,
                flush_batch: None,
            },
        )
        .unwrap();
        loom.kv_put_configured(
            ns,
            "cache",
            Value::Text("k".into()),
            b"written".to_vec(),
            None,
            10,
        )
        .unwrap();
        let commit = loom.commit(ns, "nas", "write-through", 11).unwrap();
        assert_eq!(
            kv_get(&loom, ns, "cache", &Value::Text("k".into()))
                .unwrap()
                .as_deref(),
            Some(&b"written"[..])
        );
        loom.checkout_commit(ns, commit).unwrap();
        assert_eq!(
            loom.kv_get_configured(ns, "cache", &Value::Text("k".into()), 20)
                .unwrap()
                .as_deref(),
            Some(&b"written"[..])
        );
    }

    #[test]
    fn eviction_none_keeps_every_entry_over_capacity() {
        let mut cache = EphemeralKvMap::new();
        cache.set_limits(Some(2), None, EvictionPolicy::None);
        for i in 0..5 {
            cache
                .put(
                    Value::Int(i),
                    vec![i as u8],
                    EphemeralPutOptions::default(),
                    0,
                )
                .unwrap();
        }
        // None is advisory: the map grows past the bound rather than evicting.
        assert_eq!(cache.len(0), 5);
    }

    #[test]
    fn lru_evicts_the_least_recently_accessed() {
        let mut cache = EphemeralKvMap::new();
        cache.set_limits(Some(2), None, EvictionPolicy::Lru);
        cache
            .put(
                Value::Int(1),
                b"a".to_vec(),
                EphemeralPutOptions::default(),
                10,
            )
            .unwrap();
        cache
            .put(
                Value::Int(2),
                b"b".to_vec(),
                EphemeralPutOptions::default(),
                11,
            )
            .unwrap();
        // Touch key 1 so key 2 becomes the least-recently-used.
        assert_eq!(cache.get(&Value::Int(1), 12).as_deref(), Some(&b"a"[..]));
        cache
            .put(
                Value::Int(3),
                b"c".to_vec(),
                EphemeralPutOptions::default(),
                13,
            )
            .unwrap();
        assert_eq!(cache.len(13), 2);
        assert_eq!(cache.get(&Value::Int(2), 14), None, "LRU victim is key 2");
        assert!(cache.get(&Value::Int(1), 14).is_some());
        assert!(cache.get(&Value::Int(3), 14).is_some());
    }

    #[test]
    fn lfu_evicts_the_least_frequently_accessed() {
        let mut cache = EphemeralKvMap::new();
        cache.set_limits(Some(2), None, EvictionPolicy::Lfu);
        cache
            .put(
                Value::Int(1),
                b"a".to_vec(),
                EphemeralPutOptions::default(),
                0,
            )
            .unwrap();
        cache
            .put(
                Value::Int(2),
                b"b".to_vec(),
                EphemeralPutOptions::default(),
                0,
            )
            .unwrap();
        // Hit key 1 twice; key 2 stays at zero hits and is the victim.
        cache.get(&Value::Int(1), 0);
        cache.get(&Value::Int(1), 0);
        cache
            .put(
                Value::Int(3),
                b"c".to_vec(),
                EphemeralPutOptions::default(),
                0,
            )
            .unwrap();
        assert_eq!(cache.get(&Value::Int(2), 0), None, "LFU victim is key 2");
        assert!(cache.get(&Value::Int(1), 0).is_some());
    }

    #[test]
    fn max_bytes_bound_evicts_and_put_evicting_reports_victims() {
        let mut cache = EphemeralKvMap::new();
        // Each entry is key + value + ENTRY_OVERHEAD_BYTES (64), so one ~70-byte entry fits under 100
        // but two do not, forcing a single eviction on the second put.
        cache.set_limits(None, Some(100), EvictionPolicy::Lru);
        cache
            .put(
                Value::Int(1),
                vec![0u8; 3],
                EphemeralPutOptions::default(),
                10,
            )
            .unwrap();
        let evicted = cache
            .put_evicting(
                Value::Int(2),
                vec![0u8; 3],
                EphemeralPutOptions::default(),
                11,
            )
            .unwrap();
        assert_eq!(evicted.len(), 1, "one entry evicted to fit the byte bound");
        assert_eq!(evicted[0].0, Value::Int(1));
        assert!(cache.get(&Value::Int(2), 12).is_some());
        assert_eq!(cache.get(&Value::Int(1), 12), None);
    }

    #[test]
    fn fifo_evicts_the_oldest_written_regardless_of_access() {
        let mut cache = EphemeralKvMap::new();
        cache.set_limits(Some(2), None, EvictionPolicy::Fifo);
        cache
            .put(
                Value::Int(1),
                b"a".to_vec(),
                EphemeralPutOptions::default(),
                0,
            )
            .unwrap();
        cache
            .put(
                Value::Int(2),
                b"b".to_vec(),
                EphemeralPutOptions::default(),
                0,
            )
            .unwrap();
        // Access key 1 heavily; FIFO ignores access and still evicts the oldest-written (key 1).
        cache.get(&Value::Int(1), 0);
        cache.get(&Value::Int(1), 0);
        cache
            .put(
                Value::Int(3),
                b"c".to_vec(),
                EphemeralPutOptions::default(),
                0,
            )
            .unwrap();
        assert_eq!(
            cache.get(&Value::Int(1), 0),
            None,
            "FIFO victim is the oldest write"
        );
        assert!(cache.get(&Value::Int(2), 0).is_some());
        assert!(cache.get(&Value::Int(3), 0).is_some());
    }

    #[test]
    fn ttl_priority_evicts_the_soonest_to_expire() {
        let mut cache = EphemeralKvMap::new();
        cache.set_limits(Some(2), None, EvictionPolicy::TtlPriority);
        // key 1 expires at 100, key 2 at 50; key 2 is soonest-to-expire.
        cache
            .put(
                Value::Int(1),
                b"a".to_vec(),
                EphemeralPutOptions {
                    ttl_ms: Some(100),
                    idle_ttl_ms: None,
                },
                0,
            )
            .unwrap();
        cache
            .put(
                Value::Int(2),
                b"b".to_vec(),
                EphemeralPutOptions {
                    ttl_ms: Some(50),
                    idle_ttl_ms: None,
                },
                0,
            )
            .unwrap();
        cache
            .put(
                Value::Int(3),
                b"c".to_vec(),
                EphemeralPutOptions::default(),
                10,
            )
            .unwrap();
        assert_eq!(
            cache.get(&Value::Int(2), 10),
            None,
            "TTL-priority evicts the soonest-to-expire key"
        );
        assert!(cache.get(&Value::Int(1), 10).is_some());
    }

    #[test]
    fn byte_counter_tracks_inserts_replaces_and_deletes() {
        let mut cache = EphemeralKvMap::new();
        cache.set_limits(None, Some(1000), EvictionPolicy::Lru);
        cache
            .put(
                Value::Int(1),
                vec![0u8; 10],
                EphemeralPutOptions::default(),
                0,
            )
            .unwrap();
        let one = cache.total_bytes_for_test();
        assert!(
            one >= 10 + ENTRY_OVERHEAD_BYTES,
            "size counts value + overhead"
        );
        // Replacing the value updates the counter rather than double-counting.
        cache
            .put(
                Value::Int(1),
                vec![0u8; 20],
                EphemeralPutOptions::default(),
                0,
            )
            .unwrap();
        assert_eq!(
            cache.total_bytes_for_test(),
            one + 10,
            "replace adjusts by the delta"
        );
        // Delete returns the counter to zero.
        assert!(cache.delete(&Value::Int(1)));
        assert_eq!(cache.total_bytes_for_test(), 0);
    }

    #[test]
    fn configured_kv_config_syncs_via_clone() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Kv, None, WorkspaceId::from_bytes([13; 16]))
            .unwrap();
        loom.configure_kv_map(
            ns,
            "cache",
            KvMapConfig {
                tier: KvTier::Ephemeral,
                eviction: EvictionPolicy::Lru,
                max_entries: Some(100),
                ..KvMapConfig::EPHEMERAL
            },
        )
        .unwrap();
        let c1 = loom.commit(ns, "nas", "configure cache", 1).unwrap();
        // Clone to a fresh coordinator and check out: the durable config travels with the workspace
        // (the runtime cache entries do not).
        let mut dst = Loom::new(MemoryStore::new());
        let (dst_ns, _report) =
            crate::clone_workspace(&loom, ns, &mut dst, WorkspaceId::from_bytes([14; 16])).unwrap();
        dst.checkout_commit(dst_ns, c1).unwrap();
        let cfg = dst.kv_map_config(dst_ns, "cache");
        assert_eq!(
            cfg.tier,
            KvTier::Ephemeral,
            "tier config syncs to the clone"
        );
        assert_eq!(cfg.eviction, EvictionPolicy::Lru, "eviction policy syncs");
        assert_eq!(cfg.max_entries, Some(100), "capacity bound syncs");
    }

    // ---- write-behind cache mechanics (245-c3) -------------------------------------

    #[test]
    fn dirty_buffer_coalesces_and_drains_in_order() {
        let mut cache = EphemeralKvMap::new();
        cache.mark_dirty_put(Value::Int(2), b"two".to_vec());
        cache.mark_dirty_put(Value::Int(1), b"one".to_vec());
        // A re-put coalesces (last write wins); a later delete coalesces to a delete.
        cache.mark_dirty_put(Value::Int(1), b"ONE".to_vec());
        cache.mark_dirty_delete(Value::Int(2));
        assert_eq!(cache.pending_len(), 2);
        assert!(cache.has_pending());
        // A bounded batch drains in key order; `None` drains the rest.
        let first = cache.take_flush_batch(Some(1));
        assert_eq!(first, vec![(Value::Int(1), Some(b"ONE".to_vec()))]);
        let rest = cache.take_flush_batch(None);
        assert_eq!(rest, vec![(Value::Int(2), None)]);
        assert!(!cache.has_pending());
    }

    #[test]
    fn over_high_water_tracks_entry_bound() {
        let mut cache = EphemeralKvMap::new();
        cache.set_limits(Some(10), None, EvictionPolicy::Lru);
        for i in 0..8 {
            cache
                .put(
                    Value::Int(i),
                    b"v".to_vec(),
                    EphemeralPutOptions::default(),
                    0,
                )
                .unwrap();
        }
        // 8/10 = 80% >= 80% crossed; < 90% not crossed; pct 0 disables the soft threshold.
        assert!(cache.over_high_water(80));
        assert!(!cache.over_high_water(90));
        assert!(!cache.over_high_water(0));
    }

    fn ephemeral_loom(seed: u8) -> (Loom<MemoryStore>, WorkspaceId) {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Kv, None, WorkspaceId::from_bytes([seed; 16]))
            .unwrap();
        (loom, ns)
    }

    #[test]
    fn write_behind_buffers_then_flush_persists_to_backing() {
        let (mut loom, ns) = ephemeral_loom(20);
        loom.configure_kv_map(
            ns,
            "cache",
            KvMapConfig {
                write_behind: true,
                ..KvMapConfig::EPHEMERAL
            },
        )
        .unwrap();
        loom.kv_put_configured(ns, "cache", Value::Int(1), b"one".to_vec(), None, 0)
            .unwrap();
        // The value is hot in the cache but not yet in the backing versioned map.
        assert_eq!(
            loom.kv_get_configured(ns, "cache", &Value::Int(1), 0)
                .unwrap(),
            Some(b"one".to_vec())
        );
        assert_eq!(kv_get(&loom, ns, "cache", &Value::Int(1)).unwrap(), None);
        assert_eq!(loom.pending_flush_count(ns, "cache"), 1);
        // Flushing drains the buffer to the backing map.
        assert_eq!(loom.flush_pending(ns, "cache", None).unwrap(), 1);
        assert_eq!(
            kv_get(&loom, ns, "cache", &Value::Int(1)).unwrap(),
            Some(b"one".to_vec())
        );
        assert_eq!(loom.pending_flush_count(ns, "cache"), 0);
    }

    #[test]
    fn write_behind_block_drains_at_high_water() {
        let (mut loom, ns) = ephemeral_loom(21);
        loom.configure_kv_map(
            ns,
            "cache",
            KvMapConfig {
                write_behind: true,
                max_entries: Some(2),
                eviction: EvictionPolicy::Lru,
                back_pressure: BackPressure::Block,
                flush_high_water_pct: Some(100),
                ..KvMapConfig::EPHEMERAL
            },
        )
        .unwrap();
        loom.kv_put_configured(ns, "cache", Value::Int(1), b"a".to_vec(), None, 0)
            .unwrap();
        loom.kv_put_configured(ns, "cache", Value::Int(2), b"b".to_vec(), None, 0)
            .unwrap();
        // The second put crosses the high-water mark, so Block drains the whole queue synchronously.
        assert_eq!(loom.pending_flush_count(ns, "cache"), 0);
        assert_eq!(
            kv_get(&loom, ns, "cache", &Value::Int(1)).unwrap(),
            Some(b"a".to_vec())
        );
        assert_eq!(
            kv_get(&loom, ns, "cache", &Value::Int(2)).unwrap(),
            Some(b"b".to_vec())
        );
    }

    #[test]
    fn write_behind_assisted_leaves_bounded_backlog() {
        let (mut loom, ns) = ephemeral_loom(22);
        loom.configure_kv_map(
            ns,
            "cache",
            KvMapConfig {
                write_behind: true,
                max_entries: Some(4),
                back_pressure: BackPressure::Assisted,
                flush_high_water_pct: Some(50),
                flush_batch: Some(1),
                ..KvMapConfig::EPHEMERAL
            },
        )
        .unwrap();
        for i in 1..=3 {
            loom.kv_put_configured(ns, "cache", Value::Int(i), vec![i as u8], None, 0)
                .unwrap();
        }
        // Each over-high-water put flushes one batch, so the backlog stays bounded at one entry and the
        // earliest keys reach the backing map while the newest stays buffered.
        assert_eq!(loom.pending_flush_count(ns, "cache"), 1);
        assert_eq!(
            kv_get(&loom, ns, "cache", &Value::Int(1)).unwrap(),
            Some(vec![1u8])
        );
        assert_eq!(kv_get(&loom, ns, "cache", &Value::Int(3)).unwrap(), None);
    }

    #[test]
    fn write_behind_pressure_rejects_when_saturated() {
        let (mut loom, ns) = ephemeral_loom(23);
        loom.configure_kv_map(
            ns,
            "cache",
            KvMapConfig {
                write_behind: true,
                max_entries: Some(2),
                eviction: EvictionPolicy::Lru,
                back_pressure: BackPressure::Pressure,
                flush_high_water_pct: Some(100),
                ..KvMapConfig::EPHEMERAL
            },
        )
        .unwrap();
        loom.kv_put_configured(ns, "cache", Value::Int(1), b"a".to_vec(), None, 0)
            .unwrap();
        loom.kv_put_configured(ns, "cache", Value::Int(2), b"b".to_vec(), None, 0)
            .unwrap();
        // At the high-water mark the next write is rejected (back off and retry) rather than buffered.
        let err = loom
            .kv_put_configured(ns, "cache", Value::Int(3), b"c".to_vec(), None, 0)
            .unwrap_err();
        assert_eq!(err.code, Code::Locked);
    }

    #[test]
    fn write_around_persists_backing_and_skips_cache() {
        let (mut loom, ns) = ephemeral_loom(24);
        loom.configure_kv_map(
            ns,
            "cache",
            KvMapConfig {
                write_around: true,
                ..KvMapConfig::EPHEMERAL
            },
        )
        .unwrap();
        loom.kv_put_configured(ns, "cache", Value::Int(1), b"x".to_vec(), None, 0)
            .unwrap();
        // Backing has the value; the cache was not populated (no read-through, so a get misses).
        assert_eq!(
            kv_get(&loom, ns, "cache", &Value::Int(1)).unwrap(),
            Some(b"x".to_vec())
        );
        assert_eq!(
            loom.kv_get_configured(ns, "cache", &Value::Int(1), 0)
                .unwrap(),
            None
        );
        assert_eq!(loom.pending_flush_count(ns, "cache"), 0);
    }

    // ---- lifecycle: GC sweep, stateless downgrade, checkout invalidation (245-c4) --

    #[test]
    fn sweep_expired_reclaims_proactively() {
        let mut cache = EphemeralKvMap::new();
        cache
            .put(
                Value::Int(1),
                b"a".to_vec(),
                EphemeralPutOptions {
                    ttl_ms: Some(10),
                    idle_ttl_ms: None,
                },
                0,
            )
            .unwrap();
        cache
            .put(
                Value::Int(2),
                b"b".to_vec(),
                EphemeralPutOptions::default(),
                0,
            )
            .unwrap();
        // At now=100 key 1 (ttl 10) has expired; the sweep reclaims exactly it and is then idempotent.
        assert_eq!(cache.sweep_expired(100), 1);
        assert_eq!(cache.sweep_expired(100), 0);
        assert_eq!(cache.len(100), 1);
    }

    #[test]
    fn config_for_stateless_downgrades_write_behind() {
        let behind = KvMapConfig {
            write_behind: true,
            ..KvMapConfig::EPHEMERAL
        };
        let st = behind.for_stateless();
        assert!(!st.write_behind, "write-behind is dropped");
        assert!(st.write_through, "downgraded to synchronous write-through");
        // A non-write-behind config is returned unchanged.
        let through = KvMapConfig {
            write_through: true,
            ..KvMapConfig::EPHEMERAL
        };
        assert_eq!(through.for_stateless(), through);
    }

    #[test]
    fn checkout_invalidates_ephemeral_cache() {
        let (mut loom, ns) = ephemeral_loom(26);
        loom.configure_kv_map(
            ns,
            "cache",
            KvMapConfig {
                read_through: true,
                ..KvMapConfig::EPHEMERAL
            },
        )
        .unwrap();
        // Commit so the config (a reserved file) is in history and survives the checkout.
        let c1 = loom.commit(ns, "nas", "configure", 1).unwrap();
        // Heat the cache with a cache-only put (nothing reaches the backing map).
        loom.kv_put_configured(ns, "cache", Value::Int(1), b"hot".to_vec(), None, 0)
            .unwrap();
        assert_eq!(
            loom.kv_get_configured(ns, "cache", &Value::Int(1), 0)
                .unwrap(),
            Some(b"hot".to_vec())
        );
        // Checking out drops the cache; the config persists, so a read-through now misses (empty backing).
        loom.checkout_commit(ns, c1).unwrap();
        assert_eq!(loom.kv_map_config(ns, "cache").tier, KvTier::Ephemeral);
        assert_eq!(
            loom.kv_get_configured(ns, "cache", &Value::Int(1), 0)
                .unwrap(),
            None
        );
    }
}
