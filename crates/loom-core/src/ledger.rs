//! The ledger facet - a versioned, append-only, tamper-evident log. Each entry is chained to the
//! previous by a hash under the store's identity profile, so altering any past entry
//! changes the head hash and `verify` detects it. Pure-Rust, `wasm32`-clean, deterministic. A
//! segment-native root versions, branches, and syncs through the engine.
//!
//! - **Tamper-evidence:** the per-entry hash chain is the authoritative structure.
//! - **Profile purity:** the chain hash uses the ledger's [`Algo`] - BLAKE3 for the
//!   default profile, SHA-256 for the FIPS profile - so a FIPS store's audit log contains no BLAKE3 in
//!   its cryptographic path. A ledger carries its algorithm; build a FIPS ledger with
//!   [`Ledger::with_algo`] (or load one with [`get_ledger`], which adopts the store's profile).
//! - **Merge:** a ledger branch is **fast-forward-only**; true divergence is reconciled by *replaying*
//!   one side's entries onto the other (new hashes), never a silent interleave - an append-only chain
//!   cannot be order-independently merged. That policy is enforced at the ref/engine level; this facet
//!   provides the chain integrity it relies on.

use crate::acl::AclRight;
use crate::cbor::{self, Value};
use crate::change_set::ChangeGapState;
use crate::digest::{Algo, Digest};
use crate::error::{Code, LoomError, Result};
use crate::object::{EntryKind, Object, TreeEntry};
use crate::provider::ObjectStore;
use crate::vcs::{Loom, StagedEntry, normalize_path};
use crate::workspace::{FacetKind, WorkspaceId, facet_path, facet_root};

/// The chain seed for entry 0 (`prev` of the genesis entry): 32 zero bytes.
const GENESIS: [u8; 32] = [0u8; 32];
const LEDGER_SEGMENT_V1: &str = "loom.ledger.segment.v1";
const LEDGER_SEGMENT_INDEX_V1: &str = "loom.ledger.segment_index.v1";
const LEDGER_HEAD_V1: &str = "loom.ledger.head.v1";
const LEDGER_MANIFEST_V1: &str = "loom.ledger.manifest.v1";
const LEDGER_RETENTION_V1: &str = "loom.ledger.retention.v1";
const LEDGER_CHECKPOINT_PAYLOAD_V1: &str = "loom.ledger.checkpoint.payload.v1";
const LEDGER_SIGNED_CHECKPOINT_V1: &str = "loom.ledger.signed_checkpoint.v1";
const LEDGER_PROOF_TREE_V1: &str = "loom.ledger.proof.tree.v1";
const LEDGER_INCLUSION_PROOF_V1: &str = "loom.ledger.proof.inclusion.v1";
const LEDGER_CONSISTENCY_PROOF_V1: &str = "loom.ledger.proof.consistency.v1";
const LEDGER_PROOF_EMPTY_V1: &str = "loom.ledger.proof.empty.v1";
const LEDGER_PROOF_LEAF_V1: &str = "loom.ledger.proof.leaf.v1";
const LEDGER_PROOF_NODE_V1: &str = "loom.ledger.proof.node.v1";
pub const LEDGER_CHECKPOINT_SIGNATURE_PURPOSE: &str = "ledger.checkpoint";
const LEDGER_ROOT_MANIFEST_ENTRY: &str = "manifest";
const LEDGER_ROOT_HEAD_ENTRY: &str = "head";
const LEDGER_ROOT_SEGMENT_INDEX_ENTRY: &str = "segment_index";
const LEDGER_ROOT_RETENTION_ENTRY: &str = "retention";
const LEDGER_ROOT_CHECKPOINT_ENTRY: &str = "checkpoint";

/// `hash_i = H_algo(prev_hash_bytes || payload_i)`, where `prev` is `GENESIS` for entry 0 and `H_algo`
/// is the store's identity-profile hash (BLAKE3 default, SHA-256 FIPS). The chain hash is a
/// facet-internal tamper-evidence structure stored inside the ledger object (distinct from the ledger
/// object's own content address, which already uses the profile via `store.put`); making it
/// profile-aware means a FIPS ledger's chain has no BLAKE3 in it.
fn chain(algo: Algo, prev: &[u8; 32], payload: &[u8]) -> Digest {
    let mut buf = Vec::with_capacity(32 + payload.len());
    buf.extend_from_slice(prev);
    buf.extend_from_slice(payload);
    Digest::hash(algo, &buf)
}

fn algo_from_str(value: &str) -> Result<Algo> {
    match value {
        "blake3" => Ok(Algo::Blake3),
        "sha256" => Ok(Algo::Sha256),
        other => Err(LoomError::unsupported(format!(
            "unsupported ledger digest algorithm {other:?}"
        ))),
    }
}

fn digest_from_value(value: Value, algo: Algo) -> Result<Digest> {
    let bytes = cbor::as_bytes(value)?;
    let arr: [u8; 32] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| LoomError::corrupt("ledger digest field is not 32 bytes"))?;
    Ok(Digest::of(algo, arr))
}

fn optional_digest_value(digest: Option<Digest>) -> Value {
    digest.map_or(Value::Null, |digest| cbor::digest_value(&digest))
}

fn optional_digest_from_value(value: Value, algo: Algo) -> Result<Option<Digest>> {
    match value {
        Value::Null => Ok(None),
        other => digest_from_value(other, algo).map(Some),
    }
}

fn optional_u64_value(value: Option<u64>) -> Value {
    value.map_or(Value::Null, Value::Uint)
}

fn optional_u64_from_value(value: Value) -> Result<Option<u64>> {
    match value {
        Value::Null => Ok(None),
        other => cbor::as_uint(other).map(Some),
    }
}

fn workspace_id_bytes(bytes: &[u8], field: &str) -> Result<[u8; 16]> {
    bytes
        .try_into()
        .map_err(|_| LoomError::corrupt(format!("{field} is not 16 bytes")))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedgerAppendMode {
    Draft,
    Authoritative,
}

impl LedgerAppendMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Authoritative => "authoritative",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "draft" => Ok(Self::Draft),
            "authoritative" => Ok(Self::Authoritative),
            other => Err(LoomError::corrupt(format!(
                "unsupported ledger append mode {other:?}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerSegmentEntry {
    pub seq: u64,
    pub payload: Vec<u8>,
    pub prev_hash: Digest,
    pub entry_hash: Digest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerSegment {
    pub algo: Algo,
    pub first_seq: u64,
    pub entries: Vec<LedgerSegmentEntry>,
}

impl LedgerSegment {
    pub fn build(algo: Algo, first_seq: u64, prev: Option<Digest>, payloads: Vec<Vec<u8>>) -> Self {
        let mut prev_hash = prev.map_or(GENESIS, |digest| *digest.bytes());
        let entries = payloads
            .into_iter()
            .enumerate()
            .map(|(offset, payload)| {
                let entry_hash = chain(algo, &prev_hash, &payload);
                let entry = LedgerSegmentEntry {
                    seq: first_seq + offset as u64,
                    payload,
                    prev_hash: Digest::of(algo, prev_hash),
                    entry_hash,
                };
                prev_hash = *entry_hash.bytes();
                entry
            })
            .collect();
        Self {
            algo,
            first_seq,
            entries,
        }
    }

    pub fn last_seq(&self) -> Option<u64> {
        self.entries.last().map(|entry| entry.seq)
    }

    pub fn head(&self) -> Option<Digest> {
        self.entries.last().map(|entry| entry.entry_hash)
    }

    pub fn encode(&self) -> Vec<u8> {
        cbor::encode(&Value::Array(vec![
            Value::Text(LEDGER_SEGMENT_V1.to_string()),
            Value::Uint(1),
            Value::Text(self.algo.as_str().to_string()),
            Value::Uint(self.first_seq),
            Value::Array(
                self.entries
                    .iter()
                    .map(|entry| {
                        Value::Array(vec![
                            Value::Uint(entry.seq),
                            Value::Bytes(entry.payload.clone()),
                            cbor::digest_value(&entry.prev_hash),
                            cbor::digest_value(&entry.entry_hash),
                        ])
                    })
                    .collect(),
            ),
        ]))
    }

    pub fn digest(&self) -> Digest {
        Digest::hash(self.algo, &self.encode())
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let mut fields = cbor::Fields::new(cbor::decode_array(bytes)?);
        let tag = fields.text()?;
        if tag != LEDGER_SEGMENT_V1 {
            return Err(LoomError::corrupt("ledger segment tag mismatch"));
        }
        if fields.uint()? != 1 {
            return Err(LoomError::unsupported("unsupported ledger segment version"));
        }
        let algo = algo_from_str(&fields.text()?)?;
        let first_seq = fields.uint()?;
        let encoded_entries = fields.array()?;
        fields.end()?;

        let mut prev = if first_seq == 0 { GENESIS } else { [0u8; 32] };
        let mut entries = Vec::with_capacity(encoded_entries.len());
        for (offset, item) in encoded_entries.into_iter().enumerate() {
            let mut entry_fields = cbor::Fields::new(cbor::as_array(item)?);
            let seq = entry_fields.uint()?;
            let payload = entry_fields.bytes()?;
            let prev_hash = digest_from_value(entry_fields.next_field()?, algo)?;
            let entry_hash = digest_from_value(entry_fields.next_field()?, algo)?;
            entry_fields.end()?;
            let expected_seq = first_seq + offset as u64;
            if seq != expected_seq {
                return Err(LoomError::corrupt(
                    "ledger segment sequence is not contiguous",
                ));
            }
            if offset == 0 && first_seq > 0 {
                prev = *prev_hash.bytes();
            }
            if prev_hash.bytes() != &prev {
                return Err(LoomError::corrupt(
                    "ledger segment predecessor hash mismatch",
                ));
            }
            let expected_hash = chain(algo, &prev, &payload);
            if entry_hash != expected_hash {
                return Err(LoomError::integrity_failure(format!(
                    "ledger segment chain broken at entry {seq}"
                )));
            }
            prev = *entry_hash.bytes();
            entries.push(LedgerSegmentEntry {
                seq,
                payload,
                prev_hash,
                entry_hash,
            });
        }
        Ok(Self {
            algo,
            first_seq,
            entries,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerSegmentIndexEntry {
    pub first_seq: u64,
    pub last_seq: u64,
    pub segment_root: Digest,
    pub entry_count: u64,
    pub head_hash: Digest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerSegmentIndex {
    pub algo: Algo,
    pub entries: Vec<LedgerSegmentIndexEntry>,
}

impl LedgerSegmentIndex {
    pub fn new(algo: Algo, entries: Vec<LedgerSegmentIndexEntry>) -> Result<Self> {
        let index = Self { algo, entries };
        index.validate()?;
        Ok(index)
    }

    pub fn from_segments(segments: &[LedgerSegment]) -> Result<Self> {
        let Some(first) = segments.first() else {
            return Ok(Self {
                algo: Algo::Blake3,
                entries: Vec::new(),
            });
        };
        let algo = first.algo;
        let entries = segments
            .iter()
            .map(|segment| {
                if segment.algo != algo {
                    return Err(LoomError::invalid(
                        "ledger segment index mixes digest profiles",
                    ));
                }
                let last_seq = segment.last_seq().ok_or_else(|| {
                    LoomError::invalid("ledger segment index cannot reference empty segment")
                })?;
                let head_hash = segment.head().ok_or_else(|| {
                    LoomError::invalid("ledger segment index cannot reference empty segment")
                })?;
                Ok(LedgerSegmentIndexEntry {
                    first_seq: segment.first_seq,
                    last_seq,
                    segment_root: segment.digest(),
                    entry_count: segment.entries.len() as u64,
                    head_hash,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Self::new(algo, entries)
    }

    pub fn encode(&self) -> Vec<u8> {
        cbor::encode(&Value::Array(vec![
            Value::Text(LEDGER_SEGMENT_INDEX_V1.to_string()),
            Value::Uint(1),
            Value::Text(self.algo.as_str().to_string()),
            Value::Array(
                self.entries
                    .iter()
                    .map(|entry| {
                        Value::Array(vec![
                            Value::Uint(entry.first_seq),
                            Value::Uint(entry.last_seq),
                            cbor::digest_value(&entry.segment_root),
                            Value::Uint(entry.entry_count),
                            cbor::digest_value(&entry.head_hash),
                        ])
                    })
                    .collect(),
            ),
        ]))
    }

    pub fn digest(&self) -> Digest {
        Digest::hash(self.algo, &self.encode())
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let mut fields = cbor::Fields::new(cbor::decode_array(bytes)?);
        let tag = fields.text()?;
        if tag != LEDGER_SEGMENT_INDEX_V1 {
            return Err(LoomError::corrupt("ledger segment index tag mismatch"));
        }
        if fields.uint()? != 1 {
            return Err(LoomError::unsupported(
                "unsupported ledger segment index version",
            ));
        }
        let algo = algo_from_str(&fields.text()?)?;
        let entries = fields
            .array()?
            .into_iter()
            .map(|item| {
                let mut entry_fields = cbor::Fields::new(cbor::as_array(item)?);
                let entry = LedgerSegmentIndexEntry {
                    first_seq: entry_fields.uint()?,
                    last_seq: entry_fields.uint()?,
                    segment_root: digest_from_value(entry_fields.next_field()?, algo)?,
                    entry_count: entry_fields.uint()?,
                    head_hash: digest_from_value(entry_fields.next_field()?, algo)?,
                };
                entry_fields.end()?;
                Ok(entry)
            })
            .collect::<Result<Vec<_>>>()?;
        fields.end()?;
        Self::new(algo, entries)
    }

    fn validate(&self) -> Result<()> {
        let mut previous_last = None;
        for entry in &self.entries {
            if entry.entry_count == 0 {
                return Err(LoomError::invalid("ledger segment index entry is empty"));
            }
            if entry.first_seq > entry.last_seq {
                return Err(LoomError::invalid("ledger segment index range is inverted"));
            }
            if entry.last_seq - entry.first_seq + 1 != entry.entry_count {
                return Err(LoomError::invalid("ledger segment index count mismatch"));
            }
            if let Some(previous) = previous_last {
                if entry.first_seq <= previous {
                    return Err(LoomError::invalid("ledger segment index ranges overlap"));
                }
                if entry.first_seq != previous + 1 {
                    return Err(LoomError::invalid("ledger segment index ranges have gaps"));
                }
            } else if entry.first_seq != 0 {
                return Err(LoomError::invalid(
                    "ledger segment index must start at zero",
                ));
            }
            previous_last = Some(entry.last_seq);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerHeadMetadata {
    pub algo: Algo,
    pub latest_seq: Option<u64>,
    pub latest_segment_root: Option<Digest>,
    pub chain_head: Option<Digest>,
    pub append_mode: LedgerAppendMode,
    pub retention_horizon: Option<u64>,
    pub latest_checkpoint_root: Option<Digest>,
}

impl LedgerHeadMetadata {
    pub fn encode(&self) -> Vec<u8> {
        cbor::encode(&Value::Array(vec![
            Value::Text(LEDGER_HEAD_V1.to_string()),
            Value::Uint(1),
            Value::Text(self.algo.as_str().to_string()),
            optional_u64_value(self.latest_seq),
            optional_digest_value(self.latest_segment_root),
            optional_digest_value(self.chain_head),
            Value::Text(self.append_mode.as_str().to_string()),
            optional_u64_value(self.retention_horizon),
            optional_digest_value(self.latest_checkpoint_root),
        ]))
    }

    pub fn digest(&self) -> Digest {
        Digest::hash(self.algo, &self.encode())
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let mut fields = cbor::Fields::new(cbor::decode_array(bytes)?);
        let tag = fields.text()?;
        if tag != LEDGER_HEAD_V1 {
            return Err(LoomError::corrupt("ledger head tag mismatch"));
        }
        if fields.uint()? != 1 {
            return Err(LoomError::unsupported("unsupported ledger head version"));
        }
        let algo = algo_from_str(&fields.text()?)?;
        let latest_seq = optional_u64_from_value(fields.next_field()?)?;
        let latest_segment_root = optional_digest_from_value(fields.next_field()?, algo)?;
        let chain_head = optional_digest_from_value(fields.next_field()?, algo)?;
        let append_mode = LedgerAppendMode::parse(&fields.text()?)?;
        let retention_horizon = optional_u64_from_value(fields.next_field()?)?;
        let latest_checkpoint_root = optional_digest_from_value(fields.next_field()?, algo)?;
        fields.end()?;
        if latest_seq.is_none() != chain_head.is_none() {
            return Err(LoomError::corrupt(
                "ledger head sequence and chain head disagree",
            ));
        }
        Ok(Self {
            algo,
            latest_seq,
            latest_segment_root,
            chain_head,
            append_mode,
            retention_horizon,
            latest_checkpoint_root,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerManifest {
    pub algo: Algo,
    pub head_root: Digest,
    pub segment_index_root: Digest,
    pub checkpoint_root: Option<Digest>,
    pub proof_root: Option<Digest>,
    pub retention_root: Option<Digest>,
}

impl LedgerManifest {
    pub fn encode(&self) -> Vec<u8> {
        cbor::encode(&Value::Array(vec![
            Value::Text(LEDGER_MANIFEST_V1.to_string()),
            Value::Uint(1),
            Value::Text(self.algo.as_str().to_string()),
            cbor::digest_value(&self.head_root),
            cbor::digest_value(&self.segment_index_root),
            optional_digest_value(self.checkpoint_root),
            optional_digest_value(self.proof_root),
            optional_digest_value(self.retention_root),
        ]))
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let mut fields = cbor::Fields::new(cbor::decode_array(bytes)?);
        let tag = fields.text()?;
        if tag != LEDGER_MANIFEST_V1 {
            return Err(LoomError::corrupt("ledger manifest tag mismatch"));
        }
        if fields.uint()? != 1 {
            return Err(LoomError::unsupported(
                "unsupported ledger manifest version",
            ));
        }
        let algo = algo_from_str(&fields.text()?)?;
        let manifest = Self {
            algo,
            head_root: digest_from_value(fields.next_field()?, algo)?,
            segment_index_root: digest_from_value(fields.next_field()?, algo)?,
            checkpoint_root: optional_digest_from_value(fields.next_field()?, algo)?,
            proof_root: optional_digest_from_value(fields.next_field()?, algo)?,
            retention_root: optional_digest_from_value(fields.next_field()?, algo)?,
        };
        fields.end()?;
        Ok(manifest)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedgerRangeState {
    Retained,
    PlannedPrune,
    Pruned,
    LegalHold,
}

impl LedgerRangeState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Retained => "retained",
            Self::PlannedPrune => "planned_prune",
            Self::Pruned => "pruned",
            Self::LegalHold => "legal_hold",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "retained" => Ok(Self::Retained),
            "planned_prune" => Ok(Self::PlannedPrune),
            "pruned" => Ok(Self::Pruned),
            "legal_hold" => Ok(Self::LegalHold),
            other => Err(LoomError::corrupt(format!(
                "unsupported ledger range state {other:?}"
            ))),
        }
    }
}

impl From<LedgerRangeState> for ChangeGapState {
    fn from(value: LedgerRangeState) -> Self {
        match value {
            LedgerRangeState::Retained | LedgerRangeState::LegalHold => Self::Retained,
            LedgerRangeState::PlannedPrune => Self::PlannedPrune,
            LedgerRangeState::Pruned => Self::Gap,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerRetentionRange {
    pub first_seq: u64,
    pub last_seq: u64,
    pub state: LedgerRangeState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerRetentionMetadata {
    pub algo: Algo,
    pub ranges: Vec<LedgerRetentionRange>,
}

impl LedgerRetentionMetadata {
    pub fn new(algo: Algo, ranges: Vec<LedgerRetentionRange>) -> Result<Self> {
        let metadata = Self { algo, ranges };
        metadata.validate()?;
        Ok(metadata)
    }

    pub fn encode(&self) -> Vec<u8> {
        cbor::encode(&Value::Array(vec![
            Value::Text(LEDGER_RETENTION_V1.to_string()),
            Value::Uint(1),
            Value::Text(self.algo.as_str().to_string()),
            Value::Array(
                self.ranges
                    .iter()
                    .map(|range| {
                        Value::Array(vec![
                            Value::Uint(range.first_seq),
                            Value::Uint(range.last_seq),
                            Value::Text(range.state.as_str().to_string()),
                        ])
                    })
                    .collect(),
            ),
        ]))
    }

    pub fn digest(&self) -> Digest {
        Digest::hash(self.algo, &self.encode())
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let mut fields = cbor::Fields::new(cbor::decode_array(bytes)?);
        let tag = fields.text()?;
        if tag != LEDGER_RETENTION_V1 {
            return Err(LoomError::corrupt("ledger retention tag mismatch"));
        }
        if fields.uint()? != 1 {
            return Err(LoomError::unsupported(
                "unsupported ledger retention version",
            ));
        }
        let algo = algo_from_str(&fields.text()?)?;
        let encoded_ranges = fields.array()?;
        fields.end()?;

        let mut ranges = Vec::with_capacity(encoded_ranges.len());
        for item in encoded_ranges {
            let mut range_fields = cbor::Fields::new(cbor::as_array(item)?);
            let first_seq = range_fields.uint()?;
            let last_seq = range_fields.uint()?;
            let state = LedgerRangeState::parse(&range_fields.text()?)?;
            range_fields.end()?;
            ranges.push(LedgerRetentionRange {
                first_seq,
                last_seq,
                state,
            });
        }
        Self::new(algo, ranges)
    }

    fn validate(&self) -> Result<()> {
        let mut previous_last = None;
        for range in &self.ranges {
            if range.first_seq > range.last_seq {
                return Err(LoomError::invalid("ledger retention range is inverted"));
            }
            if let Some(last) = previous_last
                && range.first_seq <= last
            {
                return Err(LoomError::invalid("ledger retention ranges overlap"));
            }
            previous_last = Some(range.last_seq);
        }
        Ok(())
    }

    fn state_for_half_open_range(&self, start: u64, end: u64) -> LedgerRangeState {
        let mut state = LedgerRangeState::Retained;
        for range in &self.ranges {
            if start <= range.last_seq && end > range.first_seq {
                match range.state {
                    LedgerRangeState::Pruned => return LedgerRangeState::Pruned,
                    LedgerRangeState::LegalHold => state = LedgerRangeState::LegalHold,
                    LedgerRangeState::PlannedPrune if state == LedgerRangeState::Retained => {
                        state = LedgerRangeState::PlannedPrune;
                    }
                    LedgerRangeState::Retained | LedgerRangeState::PlannedPrune => {}
                }
            }
        }
        state
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerRangeEntry {
    pub seq: u64,
    pub payload: Vec<u8>,
    pub entry_hash: Digest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerRangeScan {
    pub start: u64,
    pub end: u64,
    pub state: LedgerRangeState,
    pub entries: Vec<LedgerRangeEntry>,
}

impl LedgerRangeScan {
    pub fn change_gap_state(&self) -> ChangeGapState {
        self.state.into()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerCheckpointPayload {
    pub algo: Algo,
    pub namespace: WorkspaceId,
    pub collection: String,
    pub latest_seq: Option<u64>,
    pub chain_head: Option<Digest>,
    pub head_root: Digest,
    pub segment_index_root: Digest,
    pub latest_segment_root: Option<Digest>,
    pub retention_root: Option<Digest>,
    pub append_mode: LedgerAppendMode,
}

impl LedgerCheckpointPayload {
    pub fn encode(&self) -> Vec<u8> {
        cbor::encode(&Value::Array(vec![
            Value::Text(LEDGER_CHECKPOINT_PAYLOAD_V1.to_string()),
            Value::Uint(1),
            Value::Text(self.algo.as_str().to_string()),
            Value::Bytes(self.namespace.as_bytes().to_vec()),
            Value::Text(self.collection.clone()),
            optional_u64_value(self.latest_seq),
            optional_digest_value(self.chain_head),
            cbor::digest_value(&self.head_root),
            cbor::digest_value(&self.segment_index_root),
            optional_digest_value(self.latest_segment_root),
            optional_digest_value(self.retention_root),
            Value::Text(self.append_mode.as_str().to_string()),
        ]))
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let mut fields = cbor::Fields::new(cbor::decode_array(bytes)?);
        let tag = fields.text()?;
        if tag != LEDGER_CHECKPOINT_PAYLOAD_V1 {
            return Err(LoomError::corrupt("ledger checkpoint payload tag mismatch"));
        }
        if fields.uint()? != 1 {
            return Err(LoomError::unsupported(
                "unsupported ledger checkpoint payload version",
            ));
        }
        let algo = algo_from_str(&fields.text()?)?;
        let payload = Self {
            algo,
            namespace: WorkspaceId::from_bytes(workspace_id_bytes(
                fields.bytes()?.as_slice(),
                "ledger checkpoint namespace",
            )?),
            collection: fields.text()?,
            latest_seq: optional_u64_from_value(fields.next_field()?)?,
            chain_head: optional_digest_from_value(fields.next_field()?, algo)?,
            head_root: digest_from_value(fields.next_field()?, algo)?,
            segment_index_root: digest_from_value(fields.next_field()?, algo)?,
            latest_segment_root: optional_digest_from_value(fields.next_field()?, algo)?,
            retention_root: optional_digest_from_value(fields.next_field()?, algo)?,
            append_mode: LedgerAppendMode::parse(&fields.text()?)?,
        };
        fields.end()?;
        Ok(payload)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerCheckpointSignature {
    pub principal: WorkspaceId,
    pub key_id: WorkspaceId,
    pub suite: String,
    pub signature: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerSignedCheckpoint {
    pub payload: LedgerCheckpointPayload,
    pub signatures: Vec<LedgerCheckpointSignature>,
}

impl LedgerSignedCheckpoint {
    pub fn new(payload: LedgerCheckpointPayload) -> Self {
        Self {
            payload,
            signatures: Vec::new(),
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        cbor::encode(&Value::Array(vec![
            Value::Text(LEDGER_SIGNED_CHECKPOINT_V1.to_string()),
            Value::Uint(1),
            Value::Bytes(self.payload.encode()),
            Value::Array(
                self.signatures
                    .iter()
                    .map(|signature| {
                        Value::Array(vec![
                            Value::Bytes(signature.principal.as_bytes().to_vec()),
                            Value::Bytes(signature.key_id.as_bytes().to_vec()),
                            Value::Text(signature.suite.clone()),
                            Value::Bytes(signature.signature.clone()),
                        ])
                    })
                    .collect(),
            ),
        ]))
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let mut fields = cbor::Fields::new(cbor::decode_array(bytes)?);
        let tag = fields.text()?;
        if tag != LEDGER_SIGNED_CHECKPOINT_V1 {
            return Err(LoomError::corrupt("ledger signed checkpoint tag mismatch"));
        }
        if fields.uint()? != 1 {
            return Err(LoomError::unsupported(
                "unsupported ledger signed checkpoint version",
            ));
        }
        let payload = LedgerCheckpointPayload::decode(&fields.bytes()?)?;
        let encoded_signatures = fields.array()?;
        fields.end()?;
        let mut signatures = Vec::with_capacity(encoded_signatures.len());
        for encoded in encoded_signatures {
            let mut signature_fields = cbor::Fields::new(cbor::as_array(encoded)?);
            signatures.push(LedgerCheckpointSignature {
                principal: WorkspaceId::from_bytes(workspace_id_bytes(
                    signature_fields.bytes()?.as_slice(),
                    "ledger checkpoint principal",
                )?),
                key_id: WorkspaceId::from_bytes(workspace_id_bytes(
                    signature_fields.bytes()?.as_slice(),
                    "ledger checkpoint key id",
                )?),
                suite: signature_fields.text()?,
                signature: signature_fields.bytes()?,
            });
            signature_fields.end()?;
        }
        Ok(Self {
            payload,
            signatures,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerProofTree {
    pub algo: Algo,
    pub namespace: WorkspaceId,
    pub collection: String,
    pub tree_size: u64,
    pub root_hash: Digest,
}

impl LedgerProofTree {
    pub fn encode(&self) -> Vec<u8> {
        cbor::encode(&Value::Array(vec![
            Value::Text(LEDGER_PROOF_TREE_V1.to_string()),
            Value::Uint(1),
            Value::Text(self.algo.as_str().to_string()),
            Value::Bytes(self.namespace.as_bytes().to_vec()),
            Value::Text(self.collection.clone()),
            Value::Uint(self.tree_size),
            cbor::digest_value(&self.root_hash),
        ]))
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let mut fields = cbor::Fields::new(cbor::decode_array(bytes)?);
        let tag = fields.text()?;
        if tag != LEDGER_PROOF_TREE_V1 {
            return Err(LoomError::corrupt("ledger proof tree tag mismatch"));
        }
        if fields.uint()? != 1 {
            return Err(LoomError::unsupported(
                "unsupported ledger proof tree version",
            ));
        }
        let algo = algo_from_str(&fields.text()?)?;
        let tree = Self {
            algo,
            namespace: WorkspaceId::from_bytes(workspace_id_bytes(
                fields.bytes()?.as_slice(),
                "ledger proof tree namespace",
            )?),
            collection: fields.text()?,
            tree_size: fields.uint()?,
            root_hash: digest_from_value(fields.next_field()?, algo)?,
        };
        fields.end()?;
        Ok(tree)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerInclusionProof {
    pub tree: LedgerProofTree,
    pub seq: u64,
    pub leaf_hash: Digest,
    pub path: Vec<Digest>,
}

impl LedgerInclusionProof {
    pub fn encode(&self) -> Vec<u8> {
        cbor::encode(&Value::Array(vec![
            Value::Text(LEDGER_INCLUSION_PROOF_V1.to_string()),
            Value::Uint(1),
            Value::Bytes(self.tree.encode()),
            Value::Uint(self.seq),
            cbor::digest_value(&self.leaf_hash),
            Value::Array(self.path.iter().map(cbor::digest_value).collect::<Vec<_>>()),
        ]))
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let mut fields = cbor::Fields::new(cbor::decode_array(bytes)?);
        let tag = fields.text()?;
        if tag != LEDGER_INCLUSION_PROOF_V1 {
            return Err(LoomError::corrupt("ledger inclusion proof tag mismatch"));
        }
        if fields.uint()? != 1 {
            return Err(LoomError::unsupported(
                "unsupported ledger inclusion proof version",
            ));
        }
        let tree = LedgerProofTree::decode(&fields.bytes()?)?;
        let seq = fields.uint()?;
        let leaf_hash = digest_from_value(fields.next_field()?, tree.algo)?;
        let path = fields
            .array()?
            .into_iter()
            .map(|value| digest_from_value(value, tree.algo))
            .collect::<Result<Vec<_>>>()?;
        fields.end()?;
        Ok(Self {
            tree,
            seq,
            leaf_hash,
            path,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerConsistencyProof {
    pub algo: Algo,
    pub namespace: WorkspaceId,
    pub collection: String,
    pub first_tree_size: u64,
    pub second_tree_size: u64,
    pub first_root_hash: Digest,
    pub second_root_hash: Digest,
    pub path: Vec<Digest>,
}

impl LedgerConsistencyProof {
    pub fn encode(&self) -> Vec<u8> {
        cbor::encode(&Value::Array(vec![
            Value::Text(LEDGER_CONSISTENCY_PROOF_V1.to_string()),
            Value::Uint(1),
            Value::Text(self.algo.as_str().to_string()),
            Value::Bytes(self.namespace.as_bytes().to_vec()),
            Value::Text(self.collection.clone()),
            Value::Uint(self.first_tree_size),
            Value::Uint(self.second_tree_size),
            cbor::digest_value(&self.first_root_hash),
            cbor::digest_value(&self.second_root_hash),
            Value::Array(self.path.iter().map(cbor::digest_value).collect::<Vec<_>>()),
        ]))
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let mut fields = cbor::Fields::new(cbor::decode_array(bytes)?);
        let tag = fields.text()?;
        if tag != LEDGER_CONSISTENCY_PROOF_V1 {
            return Err(LoomError::corrupt("ledger consistency proof tag mismatch"));
        }
        if fields.uint()? != 1 {
            return Err(LoomError::unsupported(
                "unsupported ledger consistency proof version",
            ));
        }
        let algo = algo_from_str(&fields.text()?)?;
        let proof = Self {
            algo,
            namespace: WorkspaceId::from_bytes(workspace_id_bytes(
                fields.bytes()?.as_slice(),
                "ledger consistency proof namespace",
            )?),
            collection: fields.text()?,
            first_tree_size: fields.uint()?,
            second_tree_size: fields.uint()?,
            first_root_hash: digest_from_value(fields.next_field()?, algo)?,
            second_root_hash: digest_from_value(fields.next_field()?, algo)?,
            path: fields
                .array()?
                .into_iter()
                .map(|value| digest_from_value(value, algo))
                .collect::<Result<Vec<_>>>()?,
        };
        fields.end()?;
        Ok(proof)
    }
}

/// A versioned tamper-evident hash-chained log. The chain hash uses `algo` (the identity profile).
#[derive(Debug, Clone)]
pub struct Ledger {
    algo: Algo,
    payloads: Vec<Vec<u8>>,
    hashes: Vec<Digest>, // hashes[i] = chain(algo, hashes[i-1] | GENESIS, payloads[i])
}

impl Default for Ledger {
    fn default() -> Self {
        Self::with_algo(Algo::Blake3)
    }
}

impl Ledger {
    /// An empty ledger under the default profile (BLAKE3 chain hash).
    pub fn new() -> Self {
        Self::default()
    }
    /// An empty ledger whose chain hash uses `algo` (use the store's identity profile, e.g.
    /// `Ledger::with_algo(loom.store().digest_algo())`, so a FIPS store's ledger is SHA-256 chained).
    pub fn with_algo(algo: Algo) -> Self {
        Self {
            algo,
            payloads: Vec::new(),
            hashes: Vec::new(),
        }
    }
    /// The identity-profile algorithm this ledger's chain hash uses.
    pub fn algo(&self) -> Algo {
        self.algo
    }
    /// Number of entries (also the seq the next append will get).
    pub fn len(&self) -> usize {
        self.payloads.len()
    }
    /// Whether the ledger has no entries.
    pub fn is_empty(&self) -> bool {
        self.payloads.is_empty()
    }

    /// Append `payload`, chaining it to the current head; returns its seq (the previous length).
    pub fn append(&mut self, payload: Vec<u8>) -> usize {
        let prev = self.hashes.last().map_or(GENESIS, |d| *d.bytes());
        let hash = chain(self.algo, &prev, &payload);
        let seq = self.payloads.len();
        self.payloads.push(payload);
        self.hashes.push(hash);
        seq
    }

    /// The payload at `seq`, or `None`.
    pub fn get(&self, seq: usize) -> Option<&[u8]> {
        self.payloads.get(seq).map(Vec::as_slice)
    }

    /// The chain hash of entry `seq`, or `None`.
    pub fn entry_hash(&self, seq: usize) -> Option<Digest> {
        self.hashes.get(seq).copied()
    }

    /// The head hash (the chain hash of the last entry) - the value an external party attests to;
    /// `None` for an empty ledger.
    pub fn head(&self) -> Option<Digest> {
        self.hashes.last().copied()
    }

    /// Recompute the chain from the genesis and confirm every stored hash matches - detects any altered
    /// payload or broken link (`INTEGRITY_FAILURE`).
    pub fn verify(&self) -> Result<()> {
        let mut prev = GENESIS;
        for (i, payload) in self.payloads.iter().enumerate() {
            let expect = chain(self.algo, &prev, payload);
            if self.hashes.get(i) != Some(&expect) {
                return Err(LoomError::integrity_failure(format!(
                    "ledger chain broken at entry {i}"
                )));
            }
            prev = *expect.bytes();
        }
        Ok(())
    }

    /// Canonical bytes: each entry's payload followed by its 32-byte chain hash, in seq order.
    pub fn encode(&self) -> Vec<u8> {
        let items = self
            .payloads
            .iter()
            .zip(&self.hashes)
            .map(|(payload, hash)| {
                Value::Array(vec![
                    Value::Bytes(payload.clone()),
                    cbor::digest_value(hash),
                ])
            })
            .collect();
        cbor::encode(&Value::Array(items))
    }

    /// Parse a ledger from [`Ledger::encode`] output under identity profile `algo` (structure only; call
    /// [`Ledger::verify`] to check the chain integrity). The encoded form stores raw 32-byte hashes with
    /// no algorithm tag, so the profile must be supplied by the caller (the store's `digest_algo()`); the
    /// decoded chain hashes are tagged with `algo` so `head`/`entry_hash` display correctly.
    pub fn decode(bytes: &[u8], algo: Algo) -> Result<Self> {
        let mut l = Ledger::with_algo(algo);
        for item in cbor::decode_array(bytes)? {
            let mut f = cbor::Fields::new(cbor::as_array(item)?);
            l.payloads.push(f.bytes()?);
            l.hashes.push(Digest::of(algo, *f.digest()?.bytes()));
            f.end()?;
        }
        Ok(l)
    }
}

fn ledger_path(collection: &str) -> String {
    facet_path(FacetKind::Ledger, collection)
}

fn collection_from_ledger_path(path: &str) -> Result<&str> {
    let prefix = format!("{}/", facet_root(FacetKind::Ledger));
    let collection = path
        .strip_prefix(&prefix)
        .ok_or_else(|| LoomError::corrupt("ledger path is outside the ledger facet"))?;
    if collection.is_empty() || collection.contains('/') {
        return Err(LoomError::corrupt(
            "ledger path does not name one collection",
        ));
    }
    Ok(collection)
}

/// Stage `ledger` under `collection` in `ns` as a segment-native root Tree; `commit`
/// snapshots it. The ledger's chain-hash algorithm must match the store's identity profile - a
/// BLAKE3-chained ledger in a FIPS store would smuggle BLAKE3 into a FIPS audit
/// log - so a mismatch is rejected (`INVALID_ARGUMENT`) rather than silently stored.
pub fn put_ledger<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    ledger: &Ledger,
) -> Result<()> {
    loom.authorize_collection(ns, FacetKind::Ledger, collection, AclRight::Write)?;
    put_ledger_unchecked(loom, ns, collection, ledger)
}

fn put_ledger_unchecked<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    ledger: &Ledger,
) -> Result<()> {
    let store_algo = loom.store().digest_algo();
    if ledger.algo() != store_algo {
        return Err(LoomError::invalid(format!(
            "ledger chain uses {} but the store's identity profile is {}; build it with Ledger::with_algo({})",
            ledger.algo().as_str(),
            store_algo.as_str(),
            store_algo.as_str(),
        )));
    }
    loom.create_directory_reserved(ns, &facet_root(FacetKind::Ledger), true)?;
    stage_ledger_reserved(loom, ns, &ledger_path(collection), ledger)
}

fn stage_ledger_reserved<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    path: &str,
    ledger: &Ledger,
) -> Result<()> {
    stage_ledger_reserved_with_retention(loom, ns, path, ledger, None, LedgerAppendMode::Draft)
}

fn stage_ledger_reserved_with_retention<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    path: &str,
    ledger: &Ledger,
    retention: Option<&LedgerRetentionMetadata>,
    append_mode: LedgerAppendMode,
) -> Result<()> {
    let path = normalize_path(path)?;
    let collection = collection_from_ledger_path(&path)?;
    let root = build_ledger_root_tree(loom, ns, collection, ledger, retention, append_mode, None)?;
    loom.work
        .entry(ns)
        .or_default()
        .insert(path, StagedEntry::Ledger(root));
    Ok(())
}

fn stage_ledger_reserved_with_checkpoint<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    path: &str,
    ledger: &Ledger,
    retention: Option<&LedgerRetentionMetadata>,
    append_mode: LedgerAppendMode,
    checkpoint: &LedgerSignedCheckpoint,
) -> Result<()> {
    let path = normalize_path(path)?;
    let collection = collection_from_ledger_path(&path)?;
    let root = build_ledger_root_tree(
        loom,
        ns,
        collection,
        ledger,
        retention,
        append_mode,
        Some(checkpoint),
    )?;
    loom.work
        .entry(ns)
        .or_default()
        .insert(path, StagedEntry::Ledger(root));
    Ok(())
}

fn build_ledger_root_tree<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    ledger: &Ledger,
    retention: Option<&LedgerRetentionMetadata>,
    append_mode: LedgerAppendMode,
    checkpoint: Option<&LedgerSignedCheckpoint>,
) -> Result<Digest> {
    let algo = ledger.algo();
    if let Some(retention) = retention {
        validate_retention_against_ledger(retention, ledger)?;
    }
    let mut entries = Vec::new();
    let mut index_entries = Vec::new();
    if !ledger.is_empty() {
        let segment = LedgerSegment::build(algo, 0, None, ledger.payloads.clone());
        let segment_bytes = segment.encode();
        let segment_root = loom.store_content(ns, &segment_bytes)?;
        let last_seq = segment
            .last_seq()
            .ok_or_else(|| LoomError::corrupt("non-empty ledger built an empty segment"))?;
        let head_hash = segment
            .head()
            .ok_or_else(|| LoomError::corrupt("non-empty ledger built an empty segment"))?;
        index_entries.push(LedgerSegmentIndexEntry {
            first_seq: segment.first_seq,
            last_seq,
            segment_root,
            entry_count: segment.entries.len() as u64,
            head_hash,
        });
        entries.push(TreeEntry {
            name: format!("segment_{:020}", segment.first_seq),
            kind: EntryKind::Blob,
            target: segment_root,
            mode: 0,
        });
    }
    let segment_index = LedgerSegmentIndex::new(algo, index_entries)?;
    let segment_index_root = loom.store_content(ns, &segment_index.encode())?;
    let latest_seq = match ledger.len().checked_sub(1) {
        Some(seq) => Some(
            u64::try_from(seq)
                .map_err(|_| LoomError::invalid("ledger length exceeds u64 sequence range"))?,
        ),
        None => None,
    };
    let head = LedgerHeadMetadata {
        algo,
        latest_seq,
        latest_segment_root: segment_index.entries.last().map(|entry| entry.segment_root),
        chain_head: ledger.head(),
        append_mode,
        retention_horizon: None,
        latest_checkpoint_root: None,
    };
    let head_root = loom.store_content(ns, &head.encode())?;
    let retention_root = retention
        .map(|retention| loom.store_content(ns, &retention.encode()))
        .transpose()?;
    let mut checkpoint_root = None;
    if let Some(checkpoint) = checkpoint {
        let expected_payload = LedgerCheckpointPayload {
            algo,
            namespace: ns,
            collection: collection.to_string(),
            latest_seq,
            chain_head: ledger.head(),
            head_root,
            segment_index_root,
            latest_segment_root: segment_index.entries.last().map(|entry| entry.segment_root),
            retention_root,
            append_mode,
        };
        if checkpoint.payload != expected_payload {
            return Err(LoomError::invalid(
                "ledger checkpoint payload does not match ledger root",
            ));
        }
        let root = loom.store_content(ns, &checkpoint.encode())?;
        checkpoint_root = Some(root);
        entries.push(TreeEntry {
            name: LEDGER_ROOT_CHECKPOINT_ENTRY.to_string(),
            kind: EntryKind::Blob,
            target: root,
            mode: 0,
        });
    }
    let head = LedgerHeadMetadata {
        latest_checkpoint_root: checkpoint_root,
        ..head
    };
    let head_root = loom.store_content(ns, &head.encode())?;
    let manifest = LedgerManifest {
        algo,
        head_root,
        segment_index_root,
        checkpoint_root,
        proof_root: None,
        retention_root,
    };
    let manifest_root = loom.store_content(ns, &manifest.encode())?;
    entries.push(TreeEntry {
        name: LEDGER_ROOT_HEAD_ENTRY.to_string(),
        kind: EntryKind::Blob,
        target: head_root,
        mode: 0,
    });
    entries.push(TreeEntry {
        name: LEDGER_ROOT_MANIFEST_ENTRY.to_string(),
        kind: EntryKind::Blob,
        target: manifest_root,
        mode: 0,
    });
    entries.push(TreeEntry {
        name: LEDGER_ROOT_SEGMENT_INDEX_ENTRY.to_string(),
        kind: EntryKind::Blob,
        target: segment_index_root,
        mode: 0,
    });
    if let Some(retention_root) = retention_root {
        entries.push(TreeEntry {
            name: LEDGER_ROOT_RETENTION_ENTRY.to_string(),
            kind: EntryKind::Blob,
            target: retention_root,
            mode: 0,
        });
    }
    loom.put_object(&Object::tree(entries)?)
}

fn validate_retention_against_ledger(
    retention: &LedgerRetentionMetadata,
    ledger: &Ledger,
) -> Result<()> {
    if retention.algo != ledger.algo() {
        return Err(LoomError::invalid(
            "ledger retention metadata uses a different digest profile",
        ));
    }
    let Some(latest_seq) = ledger.len().checked_sub(1).map(|seq| seq as u64) else {
        if retention.ranges.is_empty() {
            return Ok(());
        }
        return Err(LoomError::invalid(
            "empty ledger cannot declare retention ranges",
        ));
    };
    for range in &retention.ranges {
        if range.last_seq > latest_seq {
            return Err(LoomError::invalid(
                "ledger retention range extends past the ledger head",
            ));
        }
    }
    Ok(())
}

fn checkpoint_payload_from_state(
    state: &LedgerRootState,
    ns: WorkspaceId,
    collection: &str,
) -> LedgerCheckpointPayload {
    let base_head = LedgerHeadMetadata {
        latest_checkpoint_root: None,
        ..state.head.clone()
    };
    LedgerCheckpointPayload {
        algo: state.head.algo,
        namespace: ns,
        collection: collection.to_string(),
        latest_seq: state.head.latest_seq,
        chain_head: state.head.chain_head,
        head_root: base_head.digest(),
        segment_index_root: state.manifest.segment_index_root,
        latest_segment_root: state.head.latest_segment_root,
        retention_root: state.manifest.retention_root,
        append_mode: state.head.append_mode,
    }
}

fn validate_checkpoint_payload(
    payload: &LedgerCheckpointPayload,
    state: &LedgerRootState,
    ns: WorkspaceId,
    collection: &str,
) -> Result<()> {
    if payload != &checkpoint_payload_from_state(state, ns, collection) {
        return Err(LoomError::corrupt(
            "ledger checkpoint payload does not match ledger root",
        ));
    }
    Ok(())
}

fn ledger_proof_empty_hash(algo: Algo, ns: WorkspaceId, collection: &str) -> Digest {
    Digest::hash(
        algo,
        &cbor::encode(&Value::Array(vec![
            Value::Text(LEDGER_PROOF_EMPTY_V1.to_string()),
            Value::Bytes(ns.as_bytes().to_vec()),
            Value::Text(collection.to_string()),
        ])),
    )
}

fn ledger_proof_leaf_hash(
    algo: Algo,
    ns: WorkspaceId,
    collection: &str,
    entry: &LedgerSegmentEntry,
) -> Digest {
    Digest::hash(
        algo,
        &cbor::encode(&Value::Array(vec![
            Value::Text(LEDGER_PROOF_LEAF_V1.to_string()),
            Value::Bytes(ns.as_bytes().to_vec()),
            Value::Text(collection.to_string()),
            Value::Uint(entry.seq),
            cbor::digest_value(&entry.entry_hash),
        ])),
    )
}

fn ledger_proof_node_hash(algo: Algo, left: Digest, right: Digest) -> Digest {
    Digest::hash(
        algo,
        &cbor::encode(&Value::Array(vec![
            Value::Text(LEDGER_PROOF_NODE_V1.to_string()),
            cbor::digest_value(&left),
            cbor::digest_value(&right),
        ])),
    )
}

fn largest_power_of_two_less_than(n: usize) -> usize {
    debug_assert!(n > 1);
    1usize << ((usize::BITS - (n - 1).leading_zeros() - 1) as usize)
}

fn ledger_merkle_root(algo: Algo, leaves: &[Digest]) -> Digest {
    if leaves.len() == 1 {
        return leaves[0];
    }
    let split = largest_power_of_two_less_than(leaves.len());
    let left = ledger_merkle_root(algo, &leaves[..split]);
    let right = ledger_merkle_root(algo, &leaves[split..]);
    ledger_proof_node_hash(algo, left, right)
}

fn ledger_merkle_inclusion_path(
    algo: Algo,
    leaves: &[Digest],
    index: usize,
) -> Result<Vec<Digest>> {
    if index >= leaves.len() {
        return Err(LoomError::invalid(
            "ledger inclusion sequence is outside tree",
        ));
    }
    if leaves.len() == 1 {
        return Ok(Vec::new());
    }
    let split = largest_power_of_two_less_than(leaves.len());
    if index < split {
        let mut path = ledger_merkle_inclusion_path(algo, &leaves[..split], index)?;
        path.push(ledger_merkle_root(algo, &leaves[split..]));
        Ok(path)
    } else {
        let mut path = ledger_merkle_inclusion_path(algo, &leaves[split..], index - split)?;
        path.push(ledger_merkle_root(algo, &leaves[..split]));
        Ok(path)
    }
}

fn ledger_merkle_inclusion_root(
    algo: Algo,
    leaf: Digest,
    index: usize,
    tree_size: usize,
    path: &[Digest],
) -> Result<Digest> {
    if index >= tree_size {
        return Err(LoomError::invalid(
            "ledger inclusion sequence is outside tree",
        ));
    }
    if tree_size == 1 {
        if path.is_empty() {
            return Ok(leaf);
        }
        return Err(LoomError::invalid("ledger inclusion proof has extra nodes"));
    }
    let (last, rest) = path
        .split_last()
        .ok_or_else(|| LoomError::invalid("ledger inclusion proof is incomplete"))?;
    let split = largest_power_of_two_less_than(tree_size);
    if index < split {
        let left = ledger_merkle_inclusion_root(algo, leaf, index, split, rest)?;
        Ok(ledger_proof_node_hash(algo, left, *last))
    } else {
        let right =
            ledger_merkle_inclusion_root(algo, leaf, index - split, tree_size - split, rest)?;
        Ok(ledger_proof_node_hash(algo, *last, right))
    }
}

fn ledger_merkle_consistency_path(
    algo: Algo,
    leaves: &[Digest],
    first_size: usize,
) -> Result<Vec<Digest>> {
    if first_size == 0 || first_size > leaves.len() {
        return Err(LoomError::invalid("invalid ledger consistency tree size"));
    }
    if first_size == leaves.len() {
        return Ok(Vec::new());
    }

    fn subproof(algo: Algo, leaves: &[Digest], first_size: usize, complete: bool) -> Vec<Digest> {
        if first_size == leaves.len() {
            if complete {
                Vec::new()
            } else {
                vec![ledger_merkle_root(algo, leaves)]
            }
        } else {
            let split = largest_power_of_two_less_than(leaves.len());
            if first_size <= split {
                let mut path = subproof(algo, &leaves[..split], first_size, complete);
                path.push(ledger_merkle_root(algo, &leaves[split..]));
                path
            } else {
                let mut path = subproof(algo, &leaves[split..], first_size - split, false);
                path.push(ledger_merkle_root(algo, &leaves[..split]));
                path
            }
        }
    }

    Ok(subproof(algo, leaves, first_size, true))
}

fn ledger_merkle_verify_consistency(proof: &LedgerConsistencyProof) -> Result<()> {
    if proof.first_tree_size == 0
        || proof.first_tree_size > proof.second_tree_size
        || proof.second_tree_size == 0
    {
        return Err(LoomError::invalid("invalid ledger consistency tree size"));
    }
    if proof.first_tree_size == proof.second_tree_size {
        if !proof.path.is_empty() || proof.first_root_hash != proof.second_root_hash {
            return Err(LoomError::integrity_failure(
                "ledger consistency proof roots differ for equal tree sizes",
            ));
        }
        return Ok(());
    }
    if proof.path.is_empty() {
        return Err(LoomError::invalid("ledger consistency proof is empty"));
    }

    let mut first_node = proof.first_tree_size - 1;
    let mut second_node = proof.second_tree_size - 1;
    while first_node & 1 == 1 {
        first_node >>= 1;
        second_node >>= 1;
    }

    let mut offset = 0usize;
    let (mut first_hash, mut second_hash) = if first_node == 0 {
        (proof.first_root_hash, proof.first_root_hash)
    } else {
        offset = 1;
        (proof.path[0], proof.path[0])
    };

    for node in &proof.path[offset..] {
        if second_node == 0 {
            return Err(LoomError::invalid(
                "ledger consistency proof has extra nodes",
            ));
        }
        if first_node & 1 == 1 || first_node == second_node {
            first_hash = ledger_proof_node_hash(proof.algo, *node, first_hash);
            second_hash = ledger_proof_node_hash(proof.algo, *node, second_hash);
            while first_node != 0 && first_node & 1 == 0 {
                first_node >>= 1;
                second_node >>= 1;
            }
        } else {
            second_hash = ledger_proof_node_hash(proof.algo, second_hash, *node);
        }
        first_node >>= 1;
        second_node >>= 1;
    }

    if first_hash != proof.first_root_hash || second_hash != proof.second_root_hash {
        return Err(LoomError::integrity_failure(
            "ledger consistency proof root mismatch",
        ));
    }
    Ok(())
}

fn ledger_proof_leaves(state: &LedgerRootState, ns: WorkspaceId, collection: &str) -> Vec<Digest> {
    state
        .segments
        .iter()
        .flat_map(|segment| {
            segment
                .entries
                .iter()
                .map(move |entry| ledger_proof_leaf_hash(state.head.algo, ns, collection, entry))
        })
        .collect()
}

/// Load the ledger named `collection` from `ns`'s current working tree, or `NOT_FOUND`. The chain is
/// decoded under the store's identity profile so `verify` recomputes with the right hash.
pub fn get_ledger<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
) -> Result<Ledger> {
    loom.authorize_collection(ns, FacetKind::Ledger, collection, AclRight::Read)?;
    read_ledger_reserved(loom, ns, &ledger_path(collection))
}

fn read_ledger_reserved<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    path: &str,
) -> Result<Ledger> {
    read_ledger_root_state_reserved(loom, ns, path)?.to_ledger()
}

fn read_ledger_root_state_reserved<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    path: &str,
) -> Result<LedgerRootState> {
    let path = normalize_path(path)?;
    let collection = collection_from_ledger_path(&path)?.to_string();
    let root = match loom.work.get(&ns).and_then(|work| work.get(&path)) {
        Some(StagedEntry::Ledger(root)) => *root,
        Some(_) => return Err(LoomError::invalid(format!("{path:?} is not a ledger"))),
        None => return Err(LoomError::not_found(format!("ledger {path:?} not staged"))),
    };
    ledger_root_state(loom, root, ns, &collection)
}

struct LedgerRootState {
    head: LedgerHeadMetadata,
    manifest: LedgerManifest,
    index: LedgerSegmentIndex,
    segments: Vec<LedgerSegment>,
    retention: Option<LedgerRetentionMetadata>,
    checkpoint: Option<LedgerSignedCheckpoint>,
}

impl LedgerRootState {
    fn to_ledger(&self) -> Result<Ledger> {
        let mut ledger = Ledger::with_algo(self.head.algo);
        for segment in &self.segments {
            for entry in &segment.entries {
                ledger.payloads.push(entry.payload.clone());
                ledger.hashes.push(entry.entry_hash);
            }
        }
        if self.head.latest_seq
            != ledger
                .len()
                .checked_sub(1)
                .map(u64::try_from)
                .transpose()
                .map_err(|_| LoomError::corrupt("ledger sequence exceeds u64 range"))?
            || self.head.chain_head != ledger.head()
            || self.head.latest_segment_root
                != self.index.entries.last().map(|entry| entry.segment_root)
        {
            return Err(LoomError::corrupt(
                "ledger head does not match segment index",
            ));
        }
        ledger.verify()?;
        Ok(ledger)
    }
}

fn ledger_root_state<S: ObjectStore>(
    loom: &Loom<S>,
    root: Digest,
    ns: WorkspaceId,
    collection: &str,
) -> Result<LedgerRootState> {
    let Object::Tree(entries) = loom.get_object(&root)? else {
        return Err(LoomError::corrupt("ledger root is not a Tree"));
    };
    let mut manifest_root = None;
    let mut head_root = None;
    let mut segment_index_root = None;
    let mut retention_entry_root = None;
    let mut checkpoint_entry_root = None;
    let mut segment_roots = std::collections::BTreeSet::new();
    for entry in entries {
        match entry.name.as_str() {
            LEDGER_ROOT_MANIFEST_ENTRY if entry.kind == EntryKind::Blob => {
                manifest_root = Some(entry.target)
            }
            LEDGER_ROOT_HEAD_ENTRY if entry.kind == EntryKind::Blob => {
                head_root = Some(entry.target)
            }
            LEDGER_ROOT_SEGMENT_INDEX_ENTRY if entry.kind == EntryKind::Blob => {
                segment_index_root = Some(entry.target)
            }
            LEDGER_ROOT_RETENTION_ENTRY if entry.kind == EntryKind::Blob => {
                retention_entry_root = Some(entry.target)
            }
            LEDGER_ROOT_CHECKPOINT_ENTRY if entry.kind == EntryKind::Blob => {
                checkpoint_entry_root = Some(entry.target)
            }
            name if name.starts_with("segment_") && entry.kind == EntryKind::Blob => {
                segment_roots.insert(entry.target);
            }
            _ => return Err(LoomError::corrupt("invalid ledger root entry")),
        }
    }
    let manifest_root =
        manifest_root.ok_or_else(|| LoomError::corrupt("ledger root has no manifest"))?;
    let head_root = head_root.ok_or_else(|| LoomError::corrupt("ledger root has no head"))?;
    let segment_index_root =
        segment_index_root.ok_or_else(|| LoomError::corrupt("ledger root has no segment index"))?;
    let manifest = LedgerManifest::decode(&loom.load_content(manifest_root)?)?;
    let algo = manifest.algo;
    if manifest.head_root != head_root || manifest.segment_index_root != segment_index_root {
        return Err(LoomError::corrupt(
            "ledger manifest roots do not match ledger root",
        ));
    }
    if manifest.retention_root != retention_entry_root {
        return Err(LoomError::corrupt(
            "ledger manifest retention root does not match ledger root",
        ));
    }
    if manifest.checkpoint_root != checkpoint_entry_root {
        return Err(LoomError::corrupt(
            "ledger manifest checkpoint root does not match ledger root",
        ));
    }
    let head = LedgerHeadMetadata::decode(&loom.load_content(head_root)?)?;
    let index = LedgerSegmentIndex::decode(&loom.load_content(segment_index_root)?)?;
    if head.algo != algo || index.algo != algo {
        return Err(LoomError::corrupt("ledger root mixes digest profiles"));
    }
    if head.latest_checkpoint_root != manifest.checkpoint_root {
        return Err(LoomError::corrupt(
            "ledger head checkpoint root does not match manifest",
        ));
    }
    let retention = match manifest.retention_root {
        Some(root) => {
            let retention = LedgerRetentionMetadata::decode(&loom.load_content(root)?)?;
            if retention.algo != algo {
                return Err(LoomError::corrupt("ledger root mixes digest profiles"));
            }
            Some(retention)
        }
        None => None,
    };
    let checkpoint = match manifest.checkpoint_root {
        Some(root) => {
            let checkpoint = LedgerSignedCheckpoint::decode(&loom.load_content(root)?)?;
            if checkpoint.payload.algo != algo {
                return Err(LoomError::corrupt("ledger root mixes digest profiles"));
            }
            Some(checkpoint)
        }
        None => None,
    };
    let mut segments = Vec::with_capacity(index.entries.len());
    for index_entry in &index.entries {
        if !segment_roots.contains(&index_entry.segment_root) {
            return Err(LoomError::corrupt(
                "ledger segment is not reachable from root",
            ));
        }
        let bytes = loom.load_content(index_entry.segment_root)?;
        if Digest::hash(algo, &bytes) != index_entry.segment_root {
            return Err(LoomError::corrupt(
                "ledger segment content address mismatch",
            ));
        }
        let segment = LedgerSegment::decode(&bytes)?;
        if segment.first_seq != index_entry.first_seq
            || segment.last_seq() != Some(index_entry.last_seq)
            || segment.entries.len() as u64 != index_entry.entry_count
            || segment.head() != Some(index_entry.head_hash)
        {
            return Err(LoomError::corrupt(
                "ledger segment index does not match segment",
            ));
        }
        segments.push(segment);
    }
    let state = LedgerRootState {
        head,
        manifest,
        index,
        segments,
        retention,
        checkpoint,
    };
    let ledger = state.to_ledger()?;
    if let Some(retention) = &state.retention {
        validate_retention_against_ledger(retention, &ledger)?;
    }
    if let Some(checkpoint) = &state.checkpoint {
        validate_checkpoint_payload(&checkpoint.payload, &state, ns, collection)?;
    }
    Ok(state)
}

/// Load the ledger named `collection`, or a new empty ledger under the store's identity profile when it does
/// not exist yet. Facade reads treat an absent ledger as empty rather than an error.
fn load_or_new_unchecked<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
) -> Result<Ledger> {
    match read_ledger_reserved(loom, ns, &ledger_path(collection)) {
        Ok(ledger) => Ok(ledger),
        Err(e) if e.code == Code::NotFound => Ok(Ledger::with_algo(loom.store().digest_algo())),
        Err(e) => Err(e),
    }
}

fn validate_ledger_append_mode<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    mode: LedgerAppendMode,
) -> Result<()> {
    if mode == LedgerAppendMode::Draft {
        return Ok(());
    }
    let branch = loom.registry().head_branch(ns)?;
    let ref_name = format!("branch/{branch}");
    let Some(policy) = loom.protected_ref_policy_unchecked(ns, &ref_name)? else {
        return Err(LoomError::new(
            Code::PermissionDenied,
            format!("authoritative ledger append requires protected fast-forward ref {ref_name:?}"),
        ));
    };
    if !policy.fast_forward_only {
        return Err(LoomError::new(
            Code::PermissionDenied,
            format!("authoritative ledger append requires protected fast-forward ref {ref_name:?}"),
        ));
    }
    Ok(())
}

/// Append `payload` to ledger `collection` in `ns`, creating the ledger (under the store's identity profile)
/// and the `ledger` facet if absent, and stage it. Returns the new entry's zero-based sequence.
pub fn ledger_append<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    payload: Vec<u8>,
) -> Result<u64> {
    ledger_append_with_mode(loom, ns, collection, payload, LedgerAppendMode::Draft)
}

pub fn ledger_append_with_mode<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    payload: Vec<u8>,
    mode: LedgerAppendMode,
) -> Result<u64> {
    loom.authorize_collection(ns, FacetKind::Ledger, collection, AclRight::Write)?;
    validate_ledger_append_mode(loom, ns, mode)?;
    let (mut ledger, retention) =
        match read_ledger_root_state_reserved(loom, ns, &ledger_path(collection)) {
            Ok(state) => (state.to_ledger()?, state.retention),
            Err(e) if e.code == Code::NotFound => {
                (Ledger::with_algo(loom.store().digest_algo()), None)
            }
            Err(e) => return Err(e),
        };
    let seq = ledger.append(payload);
    let store_algo = loom.store().digest_algo();
    if ledger.algo() != store_algo {
        return Err(LoomError::invalid(format!(
            "ledger chain uses {} but the store's identity profile is {}; build it with Ledger::with_algo({})",
            ledger.algo().as_str(),
            store_algo.as_str(),
            store_algo.as_str(),
        )));
    }
    loom.create_directory_reserved(ns, &facet_root(FacetKind::Ledger), true)?;
    stage_ledger_reserved_with_retention(
        loom,
        ns,
        &ledger_path(collection),
        &ledger,
        retention.as_ref(),
        mode,
    )?;
    Ok(seq as u64)
}

pub fn ledger_set_retention_ranges<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    ranges: Vec<LedgerRetentionRange>,
) -> Result<()> {
    loom.authorize_collection(ns, FacetKind::Ledger, collection, AclRight::Write)?;
    let (ledger, append_mode) =
        match read_ledger_root_state_reserved(loom, ns, &ledger_path(collection)) {
            Ok(state) => (state.to_ledger()?, state.head.append_mode),
            Err(e) if e.code == Code::NotFound => (
                Ledger::with_algo(loom.store().digest_algo()),
                LedgerAppendMode::Draft,
            ),
            Err(e) => return Err(e),
        };
    let retention = LedgerRetentionMetadata::new(ledger.algo(), ranges)?;
    loom.create_directory_reserved(ns, &facet_root(FacetKind::Ledger), true)?;
    stage_ledger_reserved_with_retention(
        loom,
        ns,
        &ledger_path(collection),
        &ledger,
        Some(&retention),
        append_mode,
    )
}

pub fn ledger_checkpoint_payload<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
) -> Result<LedgerCheckpointPayload> {
    loom.authorize_collection(ns, FacetKind::Ledger, collection, AclRight::Read)?;
    Ok(checkpoint_payload_from_state(
        &read_ledger_root_state_reserved(loom, ns, &ledger_path(collection))?,
        ns,
        collection,
    ))
}

pub fn ledger_checkpoint_payload_bytes<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
) -> Result<Vec<u8>> {
    Ok(ledger_checkpoint_payload(loom, ns, collection)?.encode())
}

pub fn ledger_attach_checkpoint_signature<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    principal: WorkspaceId,
    key_id: WorkspaceId,
    suite: &str,
    signature: Vec<u8>,
) -> Result<()> {
    loom.authorize_collection(ns, FacetKind::Ledger, collection, AclRight::Write)?;
    let state = read_ledger_root_state_reserved(loom, ns, &ledger_path(collection))?;
    let payload = checkpoint_payload_from_state(&state, ns, collection);
    let identity = loom.identity_store().ok_or_else(|| {
        LoomError::new(Code::PermissionDenied, "identity store is not configured")
    })?;
    identity.verify_principal_signature(
        principal,
        key_id,
        suite,
        LEDGER_CHECKPOINT_SIGNATURE_PURPOSE,
        &payload.encode(),
        &signature,
    )?;
    let mut checkpoint = state
        .checkpoint
        .clone()
        .unwrap_or_else(|| LedgerSignedCheckpoint::new(payload.clone()));
    if checkpoint.payload != payload {
        return Err(LoomError::corrupt(
            "ledger checkpoint payload does not match ledger root",
        ));
    }
    if let Some(existing) = checkpoint.signatures.iter_mut().find(|existing| {
        existing.principal == principal && existing.key_id == key_id && existing.suite == suite
    }) {
        existing.signature = signature;
    } else {
        checkpoint.signatures.push(LedgerCheckpointSignature {
            principal,
            key_id,
            suite: suite.to_string(),
            signature,
        });
    }
    let ledger = state.to_ledger()?;
    loom.create_directory_reserved(ns, &facet_root(FacetKind::Ledger), true)?;
    stage_ledger_reserved_with_checkpoint(
        loom,
        ns,
        &ledger_path(collection),
        &ledger,
        state.retention.as_ref(),
        state.head.append_mode,
        &checkpoint,
    )
}

pub fn ledger_verify_checkpoint_signatures<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
) -> Result<usize> {
    loom.authorize_collection(ns, FacetKind::Ledger, collection, AclRight::Read)?;
    let state = read_ledger_root_state_reserved(loom, ns, &ledger_path(collection))?;
    let Some(checkpoint) = state.checkpoint.as_ref() else {
        return Ok(0);
    };
    validate_checkpoint_payload(&checkpoint.payload, &state, ns, collection)?;
    let identity = loom.identity_store().ok_or_else(|| {
        LoomError::new(Code::PermissionDenied, "identity store is not configured")
    })?;
    let payload = checkpoint.payload.encode();
    for signature in &checkpoint.signatures {
        identity.verify_principal_signature(
            signature.principal,
            signature.key_id,
            &signature.suite,
            LEDGER_CHECKPOINT_SIGNATURE_PURPOSE,
            &payload,
            &signature.signature,
        )?;
    }
    Ok(checkpoint.signatures.len())
}

pub fn ledger_proof_tree<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
) -> Result<LedgerProofTree> {
    loom.authorize_collection(ns, FacetKind::Ledger, collection, AclRight::Read)?;
    let state = read_ledger_root_state_reserved(loom, ns, &ledger_path(collection))?;
    let leaves = ledger_proof_leaves(&state, ns, collection);
    let root_hash = if leaves.is_empty() {
        ledger_proof_empty_hash(state.head.algo, ns, collection)
    } else {
        ledger_merkle_root(state.head.algo, &leaves)
    };
    Ok(LedgerProofTree {
        algo: state.head.algo,
        namespace: ns,
        collection: collection.to_string(),
        tree_size: leaves.len() as u64,
        root_hash,
    })
}

pub fn ledger_inclusion_proof<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    seq: u64,
) -> Result<LedgerInclusionProof> {
    loom.authorize_collection(ns, FacetKind::Ledger, collection, AclRight::Read)?;
    let state = read_ledger_root_state_reserved(loom, ns, &ledger_path(collection))?;
    let leaves = ledger_proof_leaves(&state, ns, collection);
    let index = usize::try_from(seq)
        .map_err(|_| LoomError::invalid("ledger inclusion sequence exceeds usize range"))?;
    if leaves.is_empty() || index >= leaves.len() {
        return Err(LoomError::not_found("ledger inclusion sequence is absent"));
    }
    let root_hash = ledger_merkle_root(state.head.algo, &leaves);
    Ok(LedgerInclusionProof {
        tree: LedgerProofTree {
            algo: state.head.algo,
            namespace: ns,
            collection: collection.to_string(),
            tree_size: leaves.len() as u64,
            root_hash,
        },
        seq,
        leaf_hash: leaves[index],
        path: ledger_merkle_inclusion_path(state.head.algo, &leaves, index)?,
    })
}

pub fn ledger_verify_inclusion_proof(proof: &LedgerInclusionProof) -> Result<()> {
    if proof.tree.tree_size == 0 {
        return Err(LoomError::invalid("ledger inclusion proof has empty tree"));
    }
    let index = usize::try_from(proof.seq)
        .map_err(|_| LoomError::invalid("ledger inclusion sequence exceeds usize range"))?;
    let tree_size = usize::try_from(proof.tree.tree_size)
        .map_err(|_| LoomError::invalid("ledger inclusion tree size exceeds usize range"))?;
    let root = ledger_merkle_inclusion_root(
        proof.tree.algo,
        proof.leaf_hash,
        index,
        tree_size,
        &proof.path,
    )?;
    if root != proof.tree.root_hash {
        return Err(LoomError::integrity_failure(
            "ledger inclusion proof root mismatch",
        ));
    }
    Ok(())
}

pub fn ledger_consistency_proof<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    first_tree_size: u64,
    second_tree_size: u64,
) -> Result<LedgerConsistencyProof> {
    loom.authorize_collection(ns, FacetKind::Ledger, collection, AclRight::Read)?;
    let state = read_ledger_root_state_reserved(loom, ns, &ledger_path(collection))?;
    let leaves = ledger_proof_leaves(&state, ns, collection);
    let first = usize::try_from(first_tree_size)
        .map_err(|_| LoomError::invalid("ledger consistency first size exceeds usize range"))?;
    let second = usize::try_from(second_tree_size)
        .map_err(|_| LoomError::invalid("ledger consistency second size exceeds usize range"))?;
    if first == 0 || first > second || second > leaves.len() {
        return Err(LoomError::invalid("invalid ledger consistency tree size"));
    }
    let first_root_hash = ledger_merkle_root(state.head.algo, &leaves[..first]);
    let second_root_hash = ledger_merkle_root(state.head.algo, &leaves[..second]);
    Ok(LedgerConsistencyProof {
        algo: state.head.algo,
        namespace: ns,
        collection: collection.to_string(),
        first_tree_size,
        second_tree_size,
        first_root_hash,
        second_root_hash,
        path: ledger_merkle_consistency_path(state.head.algo, &leaves[..second], first)?,
    })
}

pub fn ledger_verify_consistency_proof(proof: &LedgerConsistencyProof) -> Result<()> {
    ledger_merkle_verify_consistency(proof)
}

pub fn ledger_range<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    start: u64,
    end: u64,
) -> Result<LedgerRangeScan> {
    loom.authorize_collection(ns, FacetKind::Ledger, collection, AclRight::Read)?;
    if start > end {
        return Err(LoomError::invalid("ledger range start is after end"));
    }
    let state = match read_ledger_root_state_reserved(loom, ns, &ledger_path(collection)) {
        Ok(state) => state,
        Err(e) if e.code == Code::NotFound => {
            return Ok(LedgerRangeScan {
                start,
                end,
                state: LedgerRangeState::Retained,
                entries: Vec::new(),
            });
        }
        Err(e) => return Err(e),
    };
    let range_state = state
        .retention
        .as_ref()
        .map_or(LedgerRangeState::Retained, |retention| {
            retention.state_for_half_open_range(start, end)
        });
    if range_state == LedgerRangeState::Pruned {
        return Err(LoomError::retained_gap(format!(
            "ledger range [{start},{end}) is pruned"
        )));
    }
    let mut entries = Vec::new();
    for segment in &state.segments {
        let Some(last_seq) = segment.last_seq() else {
            continue;
        };
        if start > last_seq || end <= segment.first_seq {
            continue;
        }
        for entry in &segment.entries {
            if entry.seq >= start && entry.seq < end {
                entries.push(LedgerRangeEntry {
                    seq: entry.seq,
                    payload: entry.payload.clone(),
                    entry_hash: entry.entry_hash,
                });
            }
        }
    }
    Ok(LedgerRangeScan {
        start,
        end,
        state: range_state,
        entries,
    })
}

/// The payload at `seq` in ledger `collection`, or `None` when the sequence or ledger is absent.
pub fn ledger_get<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    seq: u64,
) -> Result<Option<Vec<u8>>> {
    loom.authorize_collection(ns, FacetKind::Ledger, collection, AclRight::Read)?;
    let Some(end) = seq.checked_add(1) else {
        return Ok(None);
    };
    Ok(ledger_range(loom, ns, collection, seq, end)?
        .entries
        .into_iter()
        .next()
        .map(|entry| entry.payload))
}

/// The head chain hash of ledger `collection` (the value an external party attests to), or `None` when the
/// ledger is absent or empty.
pub fn ledger_head<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
) -> Result<Option<Digest>> {
    loom.authorize_collection(ns, FacetKind::Ledger, collection, AclRight::Read)?;
    Ok(load_or_new_unchecked(loom, ns, collection)?.head())
}

/// The number of entries in ledger `collection` (0 when absent).
pub fn ledger_len<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
) -> Result<u64> {
    loom.authorize_collection(ns, FacetKind::Ledger, collection, AclRight::Read)?;
    Ok(load_or_new_unchecked(loom, ns, collection)?.len() as u64)
}

/// Recompute ledger `collection`'s chain from genesis and confirm every stored hash matches; an altered
/// payload or broken link is `INTEGRITY_FAILURE`. An absent (empty) ledger verifies trivially.
pub fn ledger_verify<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
) -> Result<()> {
    loom.authorize_collection(ns, FacetKind::Ledger, collection, AclRight::Read)?;
    load_or_new_unchecked(loom, ns, collection)?.verify()
}

/// The ledger collection names present in `ns`'s current working tree, sorted and de-duplicated.
/// Enumeration is within the workspace, not a global index. Reserved names beginning with `.` are
/// excluded.
pub fn ledger_list_collections<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
) -> Result<Vec<String>> {
    loom.authorize_collection(ns, FacetKind::Ledger, "", AclRight::Read)?;
    let prefix = format!("{}/", facet_root(FacetKind::Ledger));
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
    use crate::acl::{AclRight, AclSubject};
    use crate::error::Code;
    use crate::identity::{
        IDENTITY_SIGNATURE_SUITE_ED25519, IdentityPublicKeySpec, IdentityStore,
        principal_signature_payload,
    };
    use crate::provider::memory::MemoryStore;
    use crate::vcs::ProtectedRefPolicy;
    use crate::workspace::{FacetKind, WorkspaceId};

    #[test]
    fn list_collections_enumerates_ledger_names_sorted() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Ledger, None, WorkspaceId::from_bytes([8; 16]))
            .unwrap();
        assert!(ledger_list_collections(&loom, ns).unwrap().is_empty());
        ledger_append(&mut loom, ns, "audit", b"a".to_vec()).unwrap();
        ledger_append(&mut loom, ns, "events", b"b".to_vec()).unwrap();
        ledger_append(&mut loom, ns, "audit", b"c".to_vec()).unwrap();
        assert_eq!(
            ledger_list_collections(&loom, ns).unwrap(),
            vec!["audit", "events"]
        );
    }

    #[test]
    fn facade_stages_segment_native_ledger_root() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Ledger, None, WorkspaceId::from_bytes([29; 16]))
            .unwrap();
        ledger_append(&mut loom, ns, "audit", b"e0".to_vec()).unwrap();
        ledger_append(&mut loom, ns, "audit", b"e1".to_vec()).unwrap();

        let path = ledger_path("audit");
        let root = match loom.work.get(&ns).unwrap().get(&path).unwrap() {
            StagedEntry::Ledger(root) => *root,
            other => panic!("ledger staged as {other:?}"),
        };
        let Object::Tree(entries) = loom.get_object(&root).unwrap() else {
            panic!("ledger root was not staged as a Tree");
        };
        assert!(entries.iter().any(|entry| {
            entry.name == LEDGER_ROOT_MANIFEST_ENTRY && entry.kind == EntryKind::Blob
        }));
        assert!(entries.iter().any(|entry| {
            entry.name == LEDGER_ROOT_HEAD_ENTRY && entry.kind == EntryKind::Blob
        }));
        assert!(entries.iter().any(|entry| {
            entry.name == LEDGER_ROOT_SEGMENT_INDEX_ENTRY && entry.kind == EntryKind::Blob
        }));
        assert!(
            entries.iter().any(|entry| {
                entry.name.starts_with("segment_") && entry.kind == EntryKind::Blob
            })
        );
        assert_eq!(
            get_ledger(&loom, ns, "audit").unwrap().get(1),
            Some(&b"e1"[..])
        );
        assert_eq!(
            loom.read_file_reserved(ns, &path).unwrap_err().code,
            Code::InvalidArgument
        );
    }

    #[test]
    fn segment_native_model_round_trips_canonical_records() {
        let segment =
            LedgerSegment::build(Algo::Blake3, 0, None, vec![b"e0".to_vec(), b"e1".to_vec()]);
        let segment_bytes = segment.encode();
        let decoded_segment = LedgerSegment::decode(&segment_bytes).unwrap();
        assert_eq!(decoded_segment, segment);
        assert_eq!(decoded_segment.first_seq, 0);
        assert_eq!(decoded_segment.last_seq(), Some(1));

        let index = LedgerSegmentIndex::from_segments(std::slice::from_ref(&segment)).unwrap();
        let index_bytes = index.encode();
        let decoded_index = LedgerSegmentIndex::decode(&index_bytes).unwrap();
        assert_eq!(decoded_index, index);
        assert_eq!(decoded_index.entries[0].segment_root, segment.digest());

        let head = LedgerHeadMetadata {
            algo: Algo::Blake3,
            latest_seq: Some(1),
            latest_segment_root: Some(segment.digest()),
            chain_head: segment.head(),
            append_mode: LedgerAppendMode::Authoritative,
            retention_horizon: Some(0),
            latest_checkpoint_root: None,
        };
        let head_bytes = head.encode();
        let decoded_head = LedgerHeadMetadata::decode(&head_bytes).unwrap();
        assert_eq!(decoded_head, head);

        let manifest = LedgerManifest {
            algo: Algo::Blake3,
            head_root: head.digest(),
            segment_index_root: index.digest(),
            checkpoint_root: None,
            proof_root: None,
            retention_root: None,
        };
        assert_eq!(
            LedgerManifest::decode(&manifest.encode()).unwrap(),
            manifest
        );
        assert_eq!(segment_bytes, decoded_segment.encode());
        assert_eq!(index_bytes, decoded_index.encode());
        assert_eq!(head_bytes, decoded_head.encode());
    }

    #[test]
    fn retention_metadata_round_trips_and_rejects_overlap() {
        let retention = LedgerRetentionMetadata::new(
            Algo::Blake3,
            vec![
                LedgerRetentionRange {
                    first_seq: 0,
                    last_seq: 1,
                    state: LedgerRangeState::Retained,
                },
                LedgerRetentionRange {
                    first_seq: 2,
                    last_seq: 3,
                    state: LedgerRangeState::Pruned,
                },
            ],
        )
        .unwrap();
        let bytes = retention.encode();
        let decoded = LedgerRetentionMetadata::decode(&bytes).unwrap();
        assert_eq!(decoded, retention);
        assert_eq!(bytes, decoded.encode());

        let overlap = LedgerRetentionMetadata::new(
            Algo::Blake3,
            vec![
                LedgerRetentionRange {
                    first_seq: 0,
                    last_seq: 1,
                    state: LedgerRangeState::Retained,
                },
                LedgerRetentionRange {
                    first_seq: 1,
                    last_seq: 2,
                    state: LedgerRangeState::PlannedPrune,
                },
            ],
        );
        assert_eq!(overlap.unwrap_err().code, Code::InvalidArgument);
    }

    #[test]
    fn ledger_range_scans_and_applies_retention_state() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Ledger, None, WorkspaceId::from_bytes([30; 16]))
            .unwrap();
        ledger_append(&mut loom, ns, "audit", b"e0".to_vec()).unwrap();
        ledger_append(&mut loom, ns, "audit", b"e1".to_vec()).unwrap();
        ledger_append(&mut loom, ns, "audit", b"e2".to_vec()).unwrap();

        let scan = ledger_range(&loom, ns, "audit", 1, 3).unwrap();
        assert_eq!(scan.state, LedgerRangeState::Retained);
        assert_eq!(scan.change_gap_state(), ChangeGapState::Retained);
        assert_eq!(
            scan.entries
                .iter()
                .map(|entry| (entry.seq, entry.payload.as_slice()))
                .collect::<Vec<_>>(),
            vec![(1, &b"e1"[..]), (2, &b"e2"[..])]
        );

        ledger_set_retention_ranges(
            &mut loom,
            ns,
            "audit",
            vec![LedgerRetentionRange {
                first_seq: 1,
                last_seq: 1,
                state: LedgerRangeState::Pruned,
            }],
        )
        .unwrap();
        assert_eq!(
            ledger_range(&loom, ns, "audit", 1, 2).unwrap_err().code,
            Code::RetainedGap
        );
        assert_eq!(
            ledger_get(&loom, ns, "audit", 1).unwrap_err().code,
            Code::RetainedGap
        );
        let visible = ledger_range(&loom, ns, "audit", 2, 3).unwrap();
        assert_eq!(visible.entries[0].payload, b"e2");
        assert_eq!(visible.change_gap_state(), ChangeGapState::Retained);
    }

    #[test]
    fn ledger_append_preserves_retention_metadata() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Ledger, None, WorkspaceId::from_bytes([31; 16]))
            .unwrap();
        ledger_append(&mut loom, ns, "audit", b"e0".to_vec()).unwrap();
        ledger_append(&mut loom, ns, "audit", b"e1".to_vec()).unwrap();
        ledger_set_retention_ranges(
            &mut loom,
            ns,
            "audit",
            vec![LedgerRetentionRange {
                first_seq: 0,
                last_seq: 0,
                state: LedgerRangeState::Pruned,
            }],
        )
        .unwrap();
        ledger_append(&mut loom, ns, "audit", b"e2".to_vec()).unwrap();

        assert_eq!(
            ledger_range(&loom, ns, "audit", 0, 1).unwrap_err().code,
            Code::RetainedGap
        );
        let visible = ledger_range(&loom, ns, "audit", 1, 3).unwrap();
        assert_eq!(
            visible
                .entries
                .iter()
                .map(|entry| entry.payload.as_slice())
                .collect::<Vec<_>>(),
            vec![&b"e1"[..], &b"e2"[..]]
        );
    }

    #[test]
    fn signed_checkpoints_verify_and_are_invalidated_by_append() {
        use ed25519_dalek::Signer as _;

        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Ledger, None, WorkspaceId::from_bytes([35; 16]))
            .unwrap();
        ledger_append(&mut loom, ns, "audit", b"e0".to_vec()).unwrap();
        ledger_append(&mut loom, ns, "audit", b"e1".to_vec()).unwrap();

        let signer = WorkspaceId::from_bytes([36; 16]);
        let key_id = WorkspaceId::from_bytes([37; 16]);
        let signing_key = ed25519_dalek::SigningKey::from_bytes(&[11u8; 32]);
        let mut identity = IdentityStore::new(signer);
        identity
            .add_public_key(
                signer,
                IdentityPublicKeySpec {
                    id: key_id,
                    label: "ledger-checkpoint".to_string(),
                    algorithm: IDENTITY_SIGNATURE_SUITE_ED25519.to_string(),
                    public_key: signing_key.verifying_key().to_bytes().to_vec(),
                },
            )
            .unwrap();
        loom.set_identity_store(identity);

        let payload = ledger_checkpoint_payload_bytes(&loom, ns, "audit").unwrap();
        let wrong_purpose = principal_signature_payload(
            signer,
            key_id,
            IDENTITY_SIGNATURE_SUITE_ED25519,
            "authority.handoff",
            &payload,
        )
        .unwrap();
        let wrong_signature = signing_key.sign(&wrong_purpose);
        assert_eq!(
            ledger_attach_checkpoint_signature(
                &mut loom,
                ns,
                "audit",
                signer,
                key_id,
                IDENTITY_SIGNATURE_SUITE_ED25519,
                wrong_signature.to_bytes().to_vec(),
            )
            .unwrap_err()
            .code,
            Code::AuthenticationFailed
        );

        let signed_payload = principal_signature_payload(
            signer,
            key_id,
            IDENTITY_SIGNATURE_SUITE_ED25519,
            LEDGER_CHECKPOINT_SIGNATURE_PURPOSE,
            &payload,
        )
        .unwrap();
        let signature = signing_key.sign(&signed_payload);
        ledger_attach_checkpoint_signature(
            &mut loom,
            ns,
            "audit",
            signer,
            key_id,
            IDENTITY_SIGNATURE_SUITE_ED25519,
            signature.to_bytes().to_vec(),
        )
        .unwrap();
        assert_eq!(
            ledger_verify_checkpoint_signatures(&loom, ns, "audit").unwrap(),
            1
        );
        let state = read_ledger_root_state_reserved(&loom, ns, &ledger_path("audit")).unwrap();
        assert!(state.head.latest_checkpoint_root.is_some());
        assert_eq!(
            state.head.latest_checkpoint_root,
            state.manifest.checkpoint_root
        );

        ledger_append(&mut loom, ns, "audit", b"e2".to_vec()).unwrap();
        assert_eq!(
            ledger_verify_checkpoint_signatures(&loom, ns, "audit").unwrap(),
            0
        );
    }

    #[test]
    fn derived_proofs_verify_without_becoming_source_identity() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Ledger, None, WorkspaceId::from_bytes([38; 16]))
            .unwrap();
        for payload in [b"e0", b"e1", b"e2", b"e3", b"e4"] {
            ledger_append(&mut loom, ns, "audit", payload.to_vec()).unwrap();
        }

        let tree = ledger_proof_tree(&loom, ns, "audit").unwrap();
        assert_eq!(tree.tree_size, 5);
        assert_eq!(LedgerProofTree::decode(&tree.encode()).unwrap(), tree);
        let state = read_ledger_root_state_reserved(&loom, ns, &ledger_path("audit")).unwrap();
        assert!(state.manifest.proof_root.is_none());

        let inclusion = ledger_inclusion_proof(&loom, ns, "audit", 3).unwrap();
        assert_eq!(
            LedgerInclusionProof::decode(&inclusion.encode()).unwrap(),
            inclusion
        );
        ledger_verify_inclusion_proof(&inclusion).unwrap();
        let mut bad_inclusion = inclusion.clone();
        bad_inclusion.leaf_hash = tree.root_hash;
        assert_eq!(
            ledger_verify_inclusion_proof(&bad_inclusion)
                .unwrap_err()
                .code,
            Code::IntegrityFailure
        );

        let consistency = ledger_consistency_proof(&loom, ns, "audit", 3, 5).unwrap();
        assert_eq!(
            LedgerConsistencyProof::decode(&consistency.encode()).unwrap(),
            consistency
        );
        ledger_verify_consistency_proof(&consistency).unwrap();
        let mut bad_consistency = consistency.clone();
        bad_consistency.second_root_hash = consistency.first_root_hash;
        assert_eq!(
            ledger_verify_consistency_proof(&bad_consistency)
                .unwrap_err()
                .code,
            Code::IntegrityFailure
        );
        assert_eq!(
            ledger_consistency_proof(&loom, ns, "audit", 0, 5)
                .unwrap_err()
                .code,
            Code::InvalidArgument
        );
    }

    #[test]
    fn authoritative_append_requires_fast_forward_protected_ref() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Ledger, None, WorkspaceId::from_bytes([32; 16]))
            .unwrap();
        assert_eq!(
            ledger_append(&mut loom, ns, "audit", b"draft".to_vec()).unwrap(),
            0
        );
        assert_eq!(
            ledger_append_with_mode(
                &mut loom,
                ns,
                "audit",
                b"authoritative".to_vec(),
                LedgerAppendMode::Authoritative
            )
            .unwrap_err()
            .code,
            Code::PermissionDenied
        );
        loom.set_protected_ref_policy(ns, "branch/main", ProtectedRefPolicy::default())
            .unwrap();
        assert_eq!(
            ledger_append_with_mode(
                &mut loom,
                ns,
                "audit",
                b"authoritative".to_vec(),
                LedgerAppendMode::Authoritative
            )
            .unwrap_err()
            .code,
            Code::PermissionDenied
        );
        loom.set_protected_ref_policy(
            ns,
            "branch/main",
            ProtectedRefPolicy {
                fast_forward_only: true,
                ..ProtectedRefPolicy::default()
            },
        )
        .unwrap();
        assert_eq!(
            ledger_append_with_mode(
                &mut loom,
                ns,
                "audit",
                b"authoritative".to_vec(),
                LedgerAppendMode::Authoritative
            )
            .unwrap(),
            1
        );
        let state = read_ledger_root_state_reserved(&loom, ns, &ledger_path("audit")).unwrap();
        assert_eq!(state.head.append_mode, LedgerAppendMode::Authoritative);
        ledger_set_retention_ranges(&mut loom, ns, "audit", Vec::new()).unwrap();
        let state = read_ledger_root_state_reserved(&loom, ns, &ledger_path("audit")).unwrap();
        assert_eq!(state.head.append_mode, LedgerAppendMode::Authoritative);
    }

    #[test]
    fn segment_native_model_rejects_bad_tags_sequences_and_hashes() {
        let segment = LedgerSegment::build(Algo::Blake3, 0, None, vec![b"e0".to_vec()]);
        let mut fields = cbor::decode_array(&segment.encode()).unwrap();
        fields[0] = Value::Text("not.ledger.segment".to_string());
        assert_eq!(
            LedgerSegment::decode(&cbor::encode(&Value::Array(fields)))
                .unwrap_err()
                .code,
            Code::CorruptObject
        );

        let mut fields = cbor::decode_array(&segment.encode()).unwrap();
        let mut entries = cbor::as_array(fields.pop().unwrap()).unwrap();
        let mut entry = cbor::as_array(entries.pop().unwrap()).unwrap();
        entry[0] = Value::Uint(7);
        entries.push(Value::Array(entry));
        fields.push(Value::Array(entries));
        assert_eq!(
            LedgerSegment::decode(&cbor::encode(&Value::Array(fields)))
                .unwrap_err()
                .code,
            Code::CorruptObject
        );

        let mut fields = cbor::decode_array(&segment.encode()).unwrap();
        let mut entries = cbor::as_array(fields.pop().unwrap()).unwrap();
        let mut entry = cbor::as_array(entries.pop().unwrap()).unwrap();
        entry[3] = Value::Bytes([1u8; 32].to_vec());
        entries.push(Value::Array(entry));
        fields.push(Value::Array(entries));
        assert_eq!(
            LedgerSegment::decode(&cbor::encode(&Value::Array(fields)))
                .unwrap_err()
                .code,
            Code::IntegrityFailure
        );
    }

    #[test]
    fn segment_index_rejects_overlaps_and_count_mismatch() {
        let segment = LedgerSegment::build(Algo::Blake3, 0, None, vec![b"e0".to_vec()]);
        let entry = LedgerSegmentIndexEntry {
            first_seq: 0,
            last_seq: 0,
            segment_root: segment.digest(),
            entry_count: 1,
            head_hash: segment.head().unwrap(),
        };
        let overlap = LedgerSegmentIndex::new(Algo::Blake3, vec![entry.clone(), entry.clone()]);
        assert_eq!(overlap.unwrap_err().code, Code::InvalidArgument);

        let mismatch = LedgerSegmentIndex::new(
            Algo::Blake3,
            vec![LedgerSegmentIndexEntry {
                entry_count: 2,
                ..entry
            }],
        );
        assert_eq!(mismatch.unwrap_err().code, Code::InvalidArgument);
    }

    #[test]
    fn append_chains_and_verifies() {
        let mut l = Ledger::new();
        assert_eq!(l.append(b"a".to_vec()), 0);
        assert_eq!(l.append(b"b".to_vec()), 1);
        assert_eq!(l.len(), 2);
        assert_eq!(l.get(1), Some(&b"b"[..]));
        assert!(l.head().is_some());
        l.verify().unwrap();
        // The head changes if the same payloads are appended in a different order (chain order matters).
        let mut other = Ledger::new();
        other.append(b"b".to_vec());
        other.append(b"a".to_vec());
        assert_ne!(l.head(), other.head());
    }

    #[test]
    fn authenticated_ledger_operations_are_acl_checked() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Ledger, None, WorkspaceId::from_bytes([25; 16]))
            .unwrap();
        let root = WorkspaceId::from_bytes([1; 16]);
        let mut identity = IdentityStore::new(root);
        identity.set_passphrase(root, "root", b"12345678").unwrap();
        let session = identity
            .authenticate_passphrase(root, "root", "session")
            .unwrap();
        loom.set_identity_store(identity);
        loom.set_session(session.id);

        assert_eq!(
            ledger_append(&mut loom, ns, "audit", b"e0".to_vec())
                .unwrap_err()
                .code,
            Code::PermissionDenied
        );

        loom.acl_store_mut()
            .allow(
                AclSubject::Principal(root),
                Some(ns),
                Some(FacetKind::Ledger),
                [AclRight::Write, AclRight::Read],
            )
            .unwrap();

        assert_eq!(
            ledger_append(&mut loom, ns, "audit", b"e0".to_vec()).unwrap(),
            0
        );
        assert_eq!(
            ledger_get(&loom, ns, "audit", 0).unwrap().as_deref(),
            Some(&b"e0"[..])
        );
    }

    #[test]
    fn tampering_is_detected_by_verify() {
        let mut l = Ledger::new();
        l.append(b"first".to_vec());
        l.append(b"second".to_vec());
        // Corrupt a stored payload without re-chaining: verify must catch it.
        l.payloads[0] = b"FIRST".to_vec();
        assert_eq!(
            l.verify().unwrap_err().code,
            crate::error::Code::IntegrityFailure
        );
    }

    #[test]
    fn encode_round_trips_and_versions() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Ledger, None, WorkspaceId::from_bytes([9; 16]))
            .unwrap();
        let mut l = Ledger::new();
        l.append(b"e0".to_vec());
        l.append(b"e1".to_vec());
        let decoded = Ledger::decode(&l.encode(), Algo::Blake3).unwrap();
        assert_eq!(decoded.len(), 2);
        decoded.verify().unwrap();
        assert_eq!(decoded.head(), l.head());

        put_ledger(&mut loom, ns, "audit", &l).unwrap();
        let c1 = loom.commit(ns, "nas", "two", 1).unwrap();
        l.append(b"e2".to_vec());
        put_ledger(&mut loom, ns, "audit", &l).unwrap();
        loom.commit(ns, "nas", "three", 2).unwrap();
        assert_eq!(get_ledger(&loom, ns, "audit").unwrap().len(), 3);
        loom.checkout_commit(ns, c1).unwrap();
        assert_eq!(get_ledger(&loom, ns, "audit").unwrap().len(), 2);
    }

    /// A FIPS ledger chains with SHA-256: every stored hash is `SHA-256(prev || payload)`, the head is
    /// tagged `sha256`, and it differs from the default-profile (BLAKE3) head over identical payloads -
    /// so a FIPS store's audit log has no BLAKE3 in its cryptographic path.
    #[test]
    fn fips_ledger_chains_with_sha256() {
        let mut l = Ledger::with_algo(Algo::Sha256);
        l.append(b"e0".to_vec());
        l.append(b"e1".to_vec());
        assert_eq!(l.algo(), Algo::Sha256);
        // Recompute the chain by hand under SHA-256 and confirm the stored hashes match.
        let h0 = chain(Algo::Sha256, &GENESIS, b"e0");
        let h1 = chain(Algo::Sha256, h0.bytes(), b"e1");
        assert_eq!(l.entry_hash(0), Some(h0));
        assert_eq!(l.entry_hash(1), Some(h1));
        assert_eq!(l.head().unwrap().algo(), Algo::Sha256);
        l.verify().unwrap();
        // The default profile produces a different head from the same payloads.
        let mut b = Ledger::new();
        b.append(b"e0".to_vec());
        b.append(b"e1".to_vec());
        assert_eq!(b.head().unwrap().algo(), Algo::Blake3);
        assert_ne!(l.head().unwrap().bytes(), b.head().unwrap().bytes());
    }

    /// The encoded form carries no algorithm tag, so decoding under the wrong profile recomputes a
    /// different chain and `verify` fails - which is why `get_ledger` decodes under the store's profile.
    #[test]
    fn decode_must_use_the_right_profile() {
        let mut l = Ledger::with_algo(Algo::Sha256);
        l.append(b"x".to_vec());
        l.append(b"y".to_vec());
        let bytes = l.encode();
        // Correct profile: verifies and the head round-trips.
        let ok = Ledger::decode(&bytes, Algo::Sha256).unwrap();
        ok.verify().unwrap();
        assert_eq!(ok.head(), l.head());
        // Wrong profile: structure decodes but the chain recomputation no longer matches.
        let wrong = Ledger::decode(&bytes, Algo::Blake3).unwrap();
        assert_eq!(
            wrong.verify().unwrap_err().code,
            crate::error::Code::IntegrityFailure
        );
    }

    /// `put_ledger` refuses a ledger whose chain algorithm disagrees with the store's identity profile,
    /// so a BLAKE3-chained ledger cannot be smuggled into a FIPS store (or vice versa).
    #[test]
    fn put_ledger_rejects_profile_mismatch() {
        let mut loom = Loom::new(MemoryStore::new()); // default profile (BLAKE3)
        let ns = loom
            .registry_mut()
            .create(FacetKind::Ledger, None, WorkspaceId::from_bytes([7; 16]))
            .unwrap();
        let mut fips = Ledger::with_algo(Algo::Sha256);
        fips.append(b"audit".to_vec());
        let err = put_ledger(&mut loom, ns, "audit", &fips).unwrap_err();
        assert_eq!(err.code, crate::error::Code::InvalidArgument);
    }

    #[test]
    fn facade_append_get_head_verify_and_absent() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Ledger, None, WorkspaceId::from_bytes([10; 16]))
            .unwrap();

        // An absent ledger is empty: no head, len 0, verifies trivially, get is absent.
        assert!(ledger_head(&loom, ns, "audit").unwrap().is_none());
        assert_eq!(ledger_len(&loom, ns, "audit").unwrap(), 0);
        ledger_verify(&loom, ns, "audit").unwrap();
        assert_eq!(ledger_get(&loom, ns, "audit", 0).unwrap(), None);

        assert_eq!(
            ledger_append(&mut loom, ns, "audit", b"e0".to_vec()).unwrap(),
            0
        );
        assert_eq!(
            ledger_append(&mut loom, ns, "audit", b"e1".to_vec()).unwrap(),
            1
        );
        assert_eq!(ledger_len(&loom, ns, "audit").unwrap(), 2);
        assert_eq!(
            ledger_get(&loom, ns, "audit", 1).unwrap().as_deref(),
            Some(&b"e1"[..])
        );
        // The head is profile-tagged (default BLAKE3) and the chain verifies.
        assert_eq!(
            ledger_head(&loom, ns, "audit").unwrap().unwrap().algo(),
            Algo::Blake3
        );
        ledger_verify(&loom, ns, "audit").unwrap();
    }
}
