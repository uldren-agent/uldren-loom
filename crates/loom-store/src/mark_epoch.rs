use crate::maintenance_policy::{MAINTENANCE_POLICY_KEY, MAINTENANCE_RUN_KEY};
use crate::{FileStore, corrupt};
use loom_core::error::{Code, LoomError, Result};
use loom_core::{Algo, Digest, Loom, ReachabilityMarkState, ReachabilityMarkStep};
use std::collections::{BTreeSet, VecDeque};

const MARK_EPOCH_KEY: &[u8] = b"maintenance/v1/reachability-mark/active";
const MARK_EPOCH_MAGIC: &[u8; 8] = b"LMARKEP1";
const MARK_EPOCH_VERSION: u16 = 2;
const MAX_DIGEST_LIST: usize = 1_000_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReachabilityMarkEpoch {
    pub epoch: u64,
    pub base_generation: u64,
    pub reference_root: Option<Digest>,
    pub control_fingerprint: Option<Digest>,
    pub derived_roots: BTreeSet<Digest>,
    pub state: ReachabilityMarkState,
}

impl ReachabilityMarkEpoch {
    pub fn retain_set(&self) -> BTreeSet<[u8; 32]> {
        self.state
            .marked
            .iter()
            .map(|digest| *digest.bytes())
            .collect()
    }
}

impl FileStore {
    pub fn begin_reachability_mark_epoch(
        &self,
        reference_root: Option<Digest>,
        derived_roots: BTreeSet<Digest>,
        state: ReachabilityMarkState,
    ) -> Result<ReachabilityMarkEpoch> {
        let status = self.maintenance_status()?;
        let control_fingerprint = self.control_reachability_fingerprint()?;
        let next_epoch = self
            .active_reachability_mark_epoch()?
            .map(|epoch| epoch.epoch.saturating_add(1))
            .unwrap_or(status.last_validated_mark_epoch.saturating_add(1))
            .max(status.last_validated_mark_epoch.saturating_add(1));
        let epoch = ReachabilityMarkEpoch {
            epoch: next_epoch,
            base_generation: status.generation,
            reference_root,
            control_fingerprint,
            derived_roots,
            state,
        };
        self.save_reachability_mark_epoch(&epoch)?;
        Ok(epoch)
    }

    pub fn active_reachability_mark_epoch(&self) -> Result<Option<ReachabilityMarkEpoch>> {
        self.control_get(MARK_EPOCH_KEY)?
            .map(|bytes| decode_mark_epoch(&bytes, self.digest_algo))
            .transpose()
    }

    pub fn save_reachability_mark_epoch(&self, epoch: &ReachabilityMarkEpoch) -> Result<()> {
        self.control_set(MARK_EPOCH_KEY, encode_mark_epoch(epoch))
    }

    pub fn complete_reachability_mark_epoch(&self, epoch: &ReachabilityMarkEpoch) -> Result<()> {
        if !epoch.state.completed {
            return Err(LoomError::invalid(
                "reachability mark epoch is not complete",
            ));
        }
        if self.reference_root() != epoch.reference_root {
            return Err(LoomError::new(
                Code::Conflict,
                "reachability mark epoch reference root changed",
            ));
        }
        if self.control_reachability_fingerprint()? != epoch.control_fingerprint {
            return Err(LoomError::new(
                Code::Conflict,
                "reachability mark epoch control root changed",
            ));
        }
        if self.derived_artifact_roots()? != epoch.derived_roots {
            return Err(LoomError::new(
                Code::Conflict,
                "reachability mark epoch derived roots changed",
            ));
        }
        let mut map = self.control_map()?;
        map.insert(MARK_EPOCH_KEY.to_vec(), encode_mark_epoch(epoch));
        self.write_control_map_validating_mark_epoch(map, epoch.epoch)
    }

    pub fn validate_reachability_mark_epoch_current(
        &self,
        epoch: &ReachabilityMarkEpoch,
    ) -> Result<()> {
        if self.reference_root() != epoch.reference_root {
            return Err(LoomError::new(
                Code::Conflict,
                "reachability mark epoch reference root changed",
            ));
        }
        if self.control_reachability_fingerprint()? != epoch.control_fingerprint {
            return Err(LoomError::new(
                Code::Conflict,
                "reachability mark epoch control root changed",
            ));
        }
        if self.derived_artifact_roots()? != epoch.derived_roots {
            return Err(LoomError::new(
                Code::Conflict,
                "reachability mark epoch derived roots changed",
            ));
        }
        Ok(())
    }

    pub fn clear_reachability_mark_epoch(&self) -> Result<bool> {
        self.control_delete(MARK_EPOCH_KEY)
    }

    pub fn derived_artifact_roots(&self) -> Result<BTreeSet<Digest>> {
        Ok(self
            .derived_payload_digests()?
            .into_iter()
            .map(|bytes| Digest::of(self.digest_algo, bytes))
            .collect())
    }

    pub(crate) fn control_reachability_fingerprint_from_map(
        &self,
        map: &std::collections::BTreeMap<Vec<u8>, Vec<u8>>,
    ) -> Option<Digest> {
        let mut map = map.clone();
        map.remove(MARK_EPOCH_KEY);
        map.remove(MAINTENANCE_POLICY_KEY);
        map.remove(MAINTENANCE_RUN_KEY);
        if map.is_empty() {
            return None;
        }
        let bytes = crate::record_io::encode_control_map(&map);
        Some(Digest::hash(self.digest_algo, &bytes))
    }

    fn control_reachability_fingerprint(&self) -> Result<Option<Digest>> {
        let map = self.control_map()?;
        Ok(self.control_reachability_fingerprint_from_map(&map))
    }

    pub(crate) fn write_control_map_validating_mark_epoch(
        &self,
        map: std::collections::BTreeMap<Vec<u8>, Vec<u8>>,
        epoch: u64,
    ) -> Result<()> {
        if map.is_empty() {
            return self.commit_txn(&[], None, Some(None), Some(epoch));
        }
        let bytes = crate::record_io::encode_control_map(&map);
        let digest = Digest::hash(self.digest_algo, &bytes);
        let codec = self.default_codec;
        self.commit_txn(
            &[(digest, bytes.as_slice(), codec)],
            None,
            Some(Some(*digest.bytes())),
            Some(epoch),
        )
    }
}

pub fn begin_loom_reachability_mark_epoch(loom: &Loom<FileStore>) -> Result<ReachabilityMarkEpoch> {
    let store = loom.store();
    let reference_root = store.reference_root();
    let derived_roots = store.derived_artifact_roots()?;
    let pinned_roots = reference_root
        .into_iter()
        .chain(derived_roots.iter().copied())
        .collect::<Vec<_>>();
    let state = loom.begin_live_object_mark(pinned_roots)?;
    store.begin_reachability_mark_epoch(reference_root, derived_roots, state)
}

pub fn step_loom_reachability_mark_epoch(
    loom: &Loom<FileStore>,
    budget: usize,
) -> Result<ReachabilityMarkStep> {
    let store = loom.store();
    let mut epoch = store
        .active_reachability_mark_epoch()?
        .ok_or_else(|| LoomError::not_found("reachability mark epoch not found"))?;
    let step = loom.step_live_object_mark(&mut epoch.state, budget)?;
    if step.completed {
        store.complete_reachability_mark_epoch(&epoch)?;
    } else {
        store.save_reachability_mark_epoch(&epoch)?;
    }
    Ok(step)
}

fn encode_mark_epoch(epoch: &ReachabilityMarkEpoch) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(MARK_EPOCH_MAGIC);
    out.extend_from_slice(&MARK_EPOCH_VERSION.to_le_bytes());
    out.extend_from_slice(&epoch.epoch.to_le_bytes());
    out.extend_from_slice(&epoch.base_generation.to_le_bytes());
    out.push(u8::from(epoch.state.completed));
    put_optional_digest(&mut out, epoch.reference_root);
    put_optional_digest(&mut out, epoch.control_fingerprint);
    put_digest_set(&mut out, &epoch.derived_roots);
    put_digest_set(&mut out, &epoch.state.pinned);
    put_digest_set(&mut out, &epoch.state.marked);
    put_digest_queue(&mut out, &epoch.state.queue);
    put_digest_queue(&mut out, &epoch.state.stream_roots);
    out
}

fn decode_mark_epoch(bytes: &[u8], algo: Algo) -> Result<ReachabilityMarkEpoch> {
    let mut cur = Cursor { bytes, pos: 0 };
    if cur.take(MARK_EPOCH_MAGIC.len())? != MARK_EPOCH_MAGIC {
        return Err(corrupt("reachability mark epoch magic"));
    }
    let version = cur.u16()?;
    if version != 1 && version != MARK_EPOCH_VERSION {
        return Err(corrupt("reachability mark epoch version"));
    }
    let epoch = cur.u64()?;
    let base_generation = cur.u64()?;
    let completed = match cur.u8()? {
        0 => false,
        1 => true,
        _ => return Err(corrupt("reachability mark epoch completed flag")),
    };
    let reference_root = cur.optional_digest(algo)?;
    let control_fingerprint = if version >= 2 {
        cur.optional_digest(algo)?
    } else {
        None
    };
    let derived_roots = cur.digest_set(algo)?;
    let pinned = cur.digest_set(algo)?;
    let marked = cur.digest_set(algo)?;
    let queue = cur.digest_queue(algo)?;
    let stream_roots = cur.digest_queue(algo)?;
    if cur.pos != bytes.len() {
        return Err(corrupt("reachability mark epoch trailing bytes"));
    }
    Ok(ReachabilityMarkEpoch {
        epoch,
        base_generation,
        reference_root,
        control_fingerprint,
        derived_roots,
        state: ReachabilityMarkState {
            pinned,
            marked,
            queue,
            stream_roots,
            completed,
        },
    })
}

fn put_optional_digest(out: &mut Vec<u8>, digest: Option<Digest>) {
    match digest {
        Some(digest) => {
            out.push(1);
            out.extend_from_slice(digest.bytes());
        }
        None => out.push(0),
    }
}

fn put_digest_set(out: &mut Vec<u8>, digests: &BTreeSet<Digest>) {
    out.extend_from_slice(&(digests.len() as u32).to_le_bytes());
    for digest in digests {
        out.extend_from_slice(digest.bytes());
    }
}

fn put_digest_queue(out: &mut Vec<u8>, digests: &VecDeque<Digest>) {
    out.extend_from_slice(&(digests.len() as u32).to_le_bytes());
    for digest in digests {
        out.extend_from_slice(digest.bytes());
    }
}

struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, len: usize) -> Result<&'a [u8]> {
        let end = self
            .pos
            .checked_add(len)
            .ok_or_else(|| corrupt("reachability mark epoch offset overflow"))?;
        let out = self
            .bytes
            .get(self.pos..end)
            .ok_or_else(|| corrupt("reachability mark epoch truncated"))?;
        self.pos = end;
        Ok(out)
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    fn optional_digest(&mut self, algo: Algo) -> Result<Option<Digest>> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(self.digest(algo)?)),
            _ => Err(corrupt("reachability mark epoch optional digest")),
        }
    }

    fn digest(&mut self, algo: Algo) -> Result<Digest> {
        Ok(Digest::of(algo, self.take(32)?.try_into().unwrap()))
    }

    fn digest_set(&mut self, algo: Algo) -> Result<BTreeSet<Digest>> {
        let len = self.digest_len()?;
        let mut out = BTreeSet::new();
        for _ in 0..len {
            if !out.insert(self.digest(algo)?) {
                return Err(corrupt("reachability mark epoch duplicate digest"));
            }
        }
        Ok(out)
    }

    fn digest_queue(&mut self, algo: Algo) -> Result<VecDeque<Digest>> {
        let len = self.digest_len()?;
        let mut out = VecDeque::with_capacity(len);
        for _ in 0..len {
            out.push_back(self.digest(algo)?);
        }
        Ok(out)
    }

    fn digest_len(&mut self) -> Result<usize> {
        let len = self.u32()? as usize;
        if len > MAX_DIGEST_LIST {
            return Err(corrupt("reachability mark epoch digest count"));
        }
        Ok(len)
    }
}
