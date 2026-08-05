//! The provider contract - the low-level store the engine builds on.
//!
//! This trait is synchronous; the asynchronous engine and binding APIs wrap it.

pub mod memory;

use crate::acl::AclStore;
use crate::digest::{Algo, Digest};
use crate::error::Result;
use crate::mutable_overlay::{
    MutableOverlayEntrySnapshot, OverlayEntryKind, OverlayGeneration, OverlayKey, OverlayKeyPrefix,
    OverlayOwnerToken, OverlayReadSnapshot, OverlaySnapshot,
};
use crate::workflow_transaction::{
    CommitReceipt, FacetWriteOp, WorkflowControlWrite, WorkflowTransaction, WriteOutcome,
};
use std::collections::BTreeMap;
use std::sync::Mutex;

/// A codec-agnostic compression intent the engine passes to a store on write. A store maps it to a
/// frame; a store that does not compress ignores it. The address is over plaintext, so the choice
/// never affects the digest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompressionHint {
    /// Store uncompressed.
    None,
    /// Favor speed.
    Fast,
    /// Favor ratio.
    #[default]
    Small,
}

/// A content-addressed object store.
pub trait ObjectStore {
    /// Store canonical object bytes and return their content address.
    ///
    /// Takes `&self` (interior mutability) so a store can be shared across threads. Storing an object
    /// that already exists is a no-op that returns the same [`Digest`]. The address is computed under
    /// the store's identity profile, so an implementation cannot store a mis-addressed object.
    fn put(&self, canonical: &[u8]) -> Result<Digest>;

    /// Like [`ObjectStore::put`], but with a compression `hint`. The default ignores it; a
    /// compressing store maps it to a frame. The address still hashes the canonical bytes under the
    /// store's identity profile.
    fn put_hint(&self, canonical: &[u8], hint: CompressionHint) -> Result<Digest> {
        let _ = hint;
        self.put(canonical)
    }

    /// Fetch canonical object bytes by address, or `None` if absent.
    fn get(&self, digest: &Digest) -> Result<Option<Vec<u8>>>;

    /// Whether the object exists.
    fn has(&self, digest: &Digest) -> Result<bool>;

    /// Number of distinct objects stored.
    fn len(&self) -> usize;

    /// Whether the store holds no objects.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The store's identity-profile digest algorithm: the algorithm every object
    /// address in this store uses. The engine reads it to compute content addresses, object identities,
    /// and prolly node ids under the store's profile rather than hard-coding BLAKE3. The default is
    /// [`Algo::Blake3`] (the default profile); a FIPS store returns [`Algo::Sha256`].
    fn digest_algo(&self) -> Algo {
        Algo::Blake3
    }

    fn put_mutable_overlay_value(
        &self,
        key: OverlayKey,
        payload: Vec<u8>,
    ) -> Result<OverlayOwnerToken> {
        let _ = key;
        let _ = payload;
        Ok(OverlayOwnerToken::from_bytes([0u8; 32]))
    }

    fn put_mutable_overlay_tombstone(&self, key: OverlayKey) -> Result<OverlayOwnerToken> {
        let _ = key;
        Ok(OverlayOwnerToken::from_bytes([0u8; 32]))
    }

    fn uses_mutable_overlay_current_records(&self) -> bool {
        false
    }

    fn mutable_overlay_current_entries(&self) -> Result<Vec<MutableOverlayEntrySnapshot>> {
        Ok(Vec::new())
    }

    fn mutable_overlay_current_entries_with_prefix(
        &self,
        key_prefix: &OverlayKeyPrefix,
    ) -> Result<Vec<MutableOverlayEntrySnapshot>> {
        let _ = key_prefix;
        Err(crate::LoomError::unsupported(
            "mutable overlay prefix enumeration is not supported by this store",
        ))
    }

    fn mutable_overlay_current_entry(
        &self,
        key: &crate::OverlayKey,
    ) -> Result<Option<MutableOverlayEntrySnapshot>> {
        let _ = key;
        Err(crate::LoomError::unsupported(
            "mutable overlay point reads are not supported by this store",
        ))
    }

    fn mutable_overlay_owner_token(
        &self,
        key: &crate::OverlayKey,
    ) -> Result<Option<crate::OverlayOwnerToken>> {
        Ok(self
            .mutable_overlay_current_entry(key)?
            .map(|entry| entry.owner_token))
    }

    fn mutable_overlay_generation(&self) -> Result<crate::OverlayGeneration> {
        Err(crate::LoomError::unsupported(
            "mutable overlay generation reads are not supported by this store",
        ))
    }

    fn retained_history_head(&self, key: &[u8]) -> Result<u64> {
        let _ = key;
        Err(crate::LoomError::unsupported(
            "retained history is not supported by this store",
        ))
    }

    fn retained_history_records(
        &self,
        key: &[u8],
        first_sequence: u64,
        max: usize,
    ) -> Result<Vec<Vec<u8>>> {
        let _ = key;
        let _ = first_sequence;
        let _ = max;
        Err(crate::LoomError::unsupported(
            "retained history is not supported by this store",
        ))
    }

    fn open_mutable_overlay_read_snapshot(
        &self,
        snapshot: OverlaySnapshot,
        owner: Option<&str>,
    ) -> Result<OverlayReadSnapshot> {
        let _ = owner;
        Ok(OverlayReadSnapshot::new(snapshot, None, None))
    }

    fn open_workflow_planning_snapshot(&self, owner: Option<&str>) -> Result<OverlayReadSnapshot> {
        let _ = owner;
        Err(crate::LoomError::unsupported(
            "coherent workflow planning snapshots are not supported by this store",
        ))
    }

    fn commit_workflow_transaction(&self, txn: WorkflowTransaction) -> Result<CommitReceipt> {
        let _ = txn;
        Err(crate::LoomError::unsupported(
            "workflow transactions are not supported by this store",
        ))
    }

    fn control_get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let _ = key;
        Err(crate::LoomError::unsupported(
            "control records are not supported by this store",
        ))
    }

    fn control_set(&self, key: &[u8], value: Vec<u8>) -> Result<()> {
        let _ = key;
        let _ = value;
        Err(crate::LoomError::unsupported(
            "control records are not supported by this store",
        ))
    }

    fn acl_store_control_write(&self, acl: &AclStore) -> WorkflowControlWrite {
        let _ = acl;
        WorkflowControlWrite::Put {
            key: b"acl".to_vec(),
            payload: Vec::new(),
        }
    }
}

#[derive(Debug)]
pub struct PlanningObjectStore<'a, S: ObjectStore> {
    base: &'a S,
    objects: Mutex<BTreeMap<[u8; 32], Vec<u8>>>,
    controls: Mutex<BTreeMap<Vec<u8>, Option<Vec<u8>>>>,
    direct_controls: Mutex<BTreeMap<Vec<u8>, Option<Vec<u8>>>>,
    current: Mutex<BTreeMap<Vec<u8>, MutableOverlayEntrySnapshot>>,
    workflow_transactions: Mutex<Vec<WorkflowTransaction>>,
}

impl<'a, S: ObjectStore> PlanningObjectStore<'a, S> {
    pub fn new(base: &'a S) -> Self {
        Self {
            base,
            objects: Mutex::new(BTreeMap::new()),
            controls: Mutex::new(BTreeMap::new()),
            direct_controls: Mutex::new(BTreeMap::new()),
            current: Mutex::new(BTreeMap::new()),
            workflow_transactions: Mutex::new(Vec::new()),
        }
    }

    pub fn objects(&self) -> Result<Vec<(Digest, Vec<u8>)>> {
        let objects = self.objects.lock().map_err(|_| {
            crate::LoomError::new(crate::Code::Internal, "planning object store lock poisoned")
        })?;
        Ok(objects
            .values()
            .cloned()
            .map(|bytes| (Digest::hash(self.digest_algo(), &bytes), bytes))
            .collect())
    }

    pub fn workflow_transactions(&self) -> Result<Vec<WorkflowTransaction>> {
        self.workflow_transactions
            .lock()
            .map_err(|_| {
                crate::LoomError::new(
                    crate::Code::Internal,
                    "planning workflow transaction lock poisoned",
                )
            })
            .map(|transactions| transactions.clone())
    }

    pub fn direct_control_writes(&self) -> Result<BTreeMap<Vec<u8>, Option<Vec<u8>>>> {
        self.direct_controls
            .lock()
            .map_err(|_| {
                crate::LoomError::new(
                    crate::Code::Internal,
                    "planning direct control lock poisoned",
                )
            })
            .map(|controls| controls.clone())
    }
}

impl<S: ObjectStore> ObjectStore for PlanningObjectStore<'_, S> {
    fn put(&self, canonical: &[u8]) -> Result<Digest> {
        let digest = Digest::hash(self.digest_algo(), canonical);
        self.objects
            .lock()
            .map_err(|_| {
                crate::LoomError::new(crate::Code::Internal, "planning object store lock poisoned")
            })?
            .entry(*digest.bytes())
            .or_insert_with(|| canonical.to_vec());
        Ok(digest)
    }

    fn get(&self, digest: &Digest) -> Result<Option<Vec<u8>>> {
        if let Some(bytes) = self
            .objects
            .lock()
            .map_err(|_| {
                crate::LoomError::new(crate::Code::Internal, "planning object store lock poisoned")
            })?
            .get(digest.bytes())
            .cloned()
        {
            return Ok(Some(bytes));
        }
        self.base.get(digest)
    }

    fn has(&self, digest: &Digest) -> Result<bool> {
        if self
            .objects
            .lock()
            .map_err(|_| {
                crate::LoomError::new(crate::Code::Internal, "planning object store lock poisoned")
            })?
            .contains_key(digest.bytes())
        {
            return Ok(true);
        }
        self.base.has(digest)
    }

    fn len(&self) -> usize {
        self.base.len()
            + self
                .objects
                .lock()
                .map(|objects| objects.len())
                .unwrap_or_default()
    }

    fn digest_algo(&self) -> Algo {
        self.base.digest_algo()
    }

    fn uses_mutable_overlay_current_records(&self) -> bool {
        self.base.uses_mutable_overlay_current_records()
    }

    fn mutable_overlay_current_entry(
        &self,
        key: &crate::OverlayKey,
    ) -> Result<Option<MutableOverlayEntrySnapshot>> {
        if let Some(entry) = self
            .current
            .lock()
            .map_err(|_| {
                crate::LoomError::new(crate::Code::Internal, "planning current lock poisoned")
            })?
            .get(key.as_bytes())
            .cloned()
        {
            return Ok(Some(entry));
        }
        self.base.mutable_overlay_current_entry(key)
    }

    fn mutable_overlay_generation(&self) -> Result<OverlayGeneration> {
        let base = self.base.mutable_overlay_generation()?.as_u64();
        let planned = self
            .workflow_transactions
            .lock()
            .map_err(|_| {
                crate::LoomError::new(
                    crate::Code::Internal,
                    "planning workflow transaction lock poisoned",
                )
            })?
            .len() as u64;
        Ok(OverlayGeneration::new(base.saturating_add(planned)))
    }

    fn retained_history_head(&self, key: &[u8]) -> Result<u64> {
        self.base.retained_history_head(key)
    }

    fn retained_history_records(
        &self,
        key: &[u8],
        first_sequence: u64,
        max: usize,
    ) -> Result<Vec<Vec<u8>>> {
        self.base.retained_history_records(key, first_sequence, max)
    }

    fn open_workflow_planning_snapshot(&self, owner: Option<&str>) -> Result<OverlayReadSnapshot> {
        self.base.open_workflow_planning_snapshot(owner)
    }

    fn commit_workflow_transaction(&self, mut txn: WorkflowTransaction) -> Result<CommitReceipt> {
        txn.validate()?;
        if let Some(expected) = txn.expected_generation {
            let current = self.mutable_overlay_generation()?;
            if current != expected {
                return Err(crate::LoomError::new(
                    crate::Code::Conflict,
                    "workflow transaction overlay generation is stale",
                ));
            }
        }
        let mut direct_controls = self.direct_controls.lock().map_err(|_| {
            crate::LoomError::new(
                crate::Code::Internal,
                "planning direct control lock poisoned",
            )
        })?;
        if !direct_controls.is_empty() {
            let mut controls = direct_controls
                .iter()
                .map(|(key, value)| match value {
                    Some(payload) => WorkflowControlWrite::Put {
                        key: key.clone(),
                        payload: payload.clone(),
                    },
                    None => WorkflowControlWrite::Delete { key: key.clone() },
                })
                .collect::<Vec<_>>();
            controls.extend(txn.owner_state.controls);
            txn.owner_state.controls = controls;
            direct_controls.clear();
        }
        let generation = OverlayGeneration::new(self.mutable_overlay_generation()?.as_u64() + 1);
        for control in &txn.owner_state.controls {
            match control {
                WorkflowControlWrite::Put { key, payload } => {
                    self.controls
                        .lock()
                        .map_err(|_| {
                            crate::LoomError::new(
                                crate::Code::Internal,
                                "planning control lock poisoned",
                            )
                        })?
                        .insert(key.clone(), Some(payload.clone()));
                }
                WorkflowControlWrite::Delete { key } => {
                    self.controls
                        .lock()
                        .map_err(|_| {
                            crate::LoomError::new(
                                crate::Code::Internal,
                                "planning control lock poisoned",
                            )
                        })?
                        .insert(key.clone(), None);
                }
                WorkflowControlWrite::AppendRetained { .. } => {}
            }
        }
        let mut outcomes = Vec::new();
        let mut current = self.current.lock().map_err(|_| {
            crate::LoomError::new(crate::Code::Internal, "planning current lock poisoned")
        })?;
        for write in &txn.writes {
            let kind = match write.op {
                FacetWriteOp::Put { .. } => OverlayEntryKind::Value,
                FacetWriteOp::Delete => OverlayEntryKind::Tombstone,
            };
            let payload = match &write.op {
                FacetWriteOp::Put { payload } => payload.clone(),
                FacetWriteOp::Delete => Vec::new(),
            };
            let owner_token = OverlayOwnerToken::from_bytes([0u8; 32]);
            let entry = MutableOverlayEntrySnapshot {
                generation,
                key: write.target.clone(),
                owner_token: owner_token.clone(),
                kind,
                payload,
            };
            current.insert(write.target.as_bytes().to_vec(), entry);
            outcomes.push(WriteOutcome {
                facet: write.facet,
                target: write.target.clone(),
                owner_token,
                change: kind,
            });
        }
        self.workflow_transactions
            .lock()
            .map_err(|_| {
                crate::LoomError::new(
                    crate::Code::Internal,
                    "planning workflow transaction lock poisoned",
                )
            })?
            .push(txn.clone());
        Ok(CommitReceipt {
            generation,
            root_after: Digest::hash(self.digest_algo(), b"planning workflow transaction"),
            writes: outcomes,
            operation_identities: txn
                .prepared_operations
                .iter()
                .map(|operation| operation.operation_id.clone())
                .collect(),
            revision_identities: txn
                .revision_metadata
                .iter()
                .map(|revision| crate::RevisionReceipt {
                    entity_id: revision.entity_id.clone(),
                    revision_id: revision.revision_id.clone(),
                })
                .collect(),
            audit_sequences: Vec::new(),
            retained_sequences: txn
                .owner_state
                .controls
                .iter()
                .filter_map(|write| match write {
                    crate::WorkflowControlWrite::AppendRetained {
                        key,
                        expected_next_sequence,
                        records,
                    } => Some(crate::RetainedSequenceReceipt {
                        key: key.clone(),
                        first_sequence: *expected_next_sequence,
                        last_sequence: expected_next_sequence
                            .saturating_add(records.len() as u64)
                            .saturating_sub(1),
                    }),
                    _ => None,
                })
                .collect(),
            delivery_receipts: txn
                .delivery_intents
                .iter()
                .map(|delivery| crate::DeliveryReceipt {
                    stream_id: delivery.stream_id.clone(),
                    sequence: delivery.sequence,
                    envelope_id: delivery.envelope_id.clone(),
                    payload_digest: delivery.payload_digest,
                })
                .collect(),
            post_commit_delta: txn
                .post_commit_delta
                .as_ref()
                .map(crate::PostCommitDeltaReceipt::from),
            replayed: false,
        })
    }

    fn control_get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        if let Some(value) = self
            .controls
            .lock()
            .map_err(|_| {
                crate::LoomError::new(crate::Code::Internal, "planning control lock poisoned")
            })?
            .get(key)
            .cloned()
        {
            return Ok(value);
        }
        self.base.control_get(key)
    }

    fn control_set(&self, key: &[u8], value: Vec<u8>) -> Result<()> {
        self.controls
            .lock()
            .map_err(|_| {
                crate::LoomError::new(crate::Code::Internal, "planning control lock poisoned")
            })?
            .insert(key.to_vec(), Some(value.clone()));
        self.direct_controls
            .lock()
            .map_err(|_| {
                crate::LoomError::new(
                    crate::Code::Internal,
                    "planning direct control lock poisoned",
                )
            })?
            .insert(key.to_vec(), Some(value));
        Ok(())
    }

    fn acl_store_control_write(&self, acl: &AclStore) -> WorkflowControlWrite {
        self.base.acl_store_control_write(acl)
    }
}

/// A shared, type-erased object store. Lets a component own a readable store without being generic
/// over the concrete backend - the lazy SQL base snapshot holds one of these: an
/// owned, lock-free read view of a `.loom` whose backend (`FileStore`, in-memory, OPFS) the SQL layer
/// need not name. `ObjectStore` is object-safe (every method takes `&self`), so the trait object
/// dispatches its reads through the `Arc`.
impl ObjectStore for std::sync::Arc<dyn ObjectStore + Send + Sync> {
    fn put(&self, canonical: &[u8]) -> Result<Digest> {
        (**self).put(canonical)
    }
    fn put_hint(&self, canonical: &[u8], hint: CompressionHint) -> Result<Digest> {
        (**self).put_hint(canonical, hint)
    }
    fn get(&self, digest: &Digest) -> Result<Option<Vec<u8>>> {
        (**self).get(digest)
    }
    fn has(&self, digest: &Digest) -> Result<bool> {
        (**self).has(digest)
    }
    fn len(&self) -> usize {
        (**self).len()
    }
    fn is_empty(&self) -> bool {
        (**self).is_empty()
    }
    fn digest_algo(&self) -> Algo {
        (**self).digest_algo()
    }
    fn put_mutable_overlay_value(
        &self,
        key: OverlayKey,
        payload: Vec<u8>,
    ) -> Result<OverlayOwnerToken> {
        (**self).put_mutable_overlay_value(key, payload)
    }
    fn put_mutable_overlay_tombstone(&self, key: OverlayKey) -> Result<OverlayOwnerToken> {
        (**self).put_mutable_overlay_tombstone(key)
    }
    fn uses_mutable_overlay_current_records(&self) -> bool {
        (**self).uses_mutable_overlay_current_records()
    }
    fn mutable_overlay_current_entries(&self) -> Result<Vec<MutableOverlayEntrySnapshot>> {
        (**self).mutable_overlay_current_entries()
    }

    fn mutable_overlay_current_entries_with_prefix(
        &self,
        key_prefix: &OverlayKeyPrefix,
    ) -> Result<Vec<MutableOverlayEntrySnapshot>> {
        (**self).mutable_overlay_current_entries_with_prefix(key_prefix)
    }

    fn mutable_overlay_current_entry(
        &self,
        key: &crate::OverlayKey,
    ) -> Result<Option<MutableOverlayEntrySnapshot>> {
        (**self).mutable_overlay_current_entry(key)
    }
    fn mutable_overlay_owner_token(
        &self,
        key: &crate::OverlayKey,
    ) -> Result<Option<crate::OverlayOwnerToken>> {
        (**self).mutable_overlay_owner_token(key)
    }
    fn mutable_overlay_generation(&self) -> Result<crate::OverlayGeneration> {
        (**self).mutable_overlay_generation()
    }
    fn retained_history_head(&self, key: &[u8]) -> Result<u64> {
        (**self).retained_history_head(key)
    }
    fn retained_history_records(
        &self,
        key: &[u8],
        first_sequence: u64,
        max: usize,
    ) -> Result<Vec<Vec<u8>>> {
        (**self).retained_history_records(key, first_sequence, max)
    }
    fn open_mutable_overlay_read_snapshot(
        &self,
        snapshot: OverlaySnapshot,
        owner: Option<&str>,
    ) -> Result<OverlayReadSnapshot> {
        (**self).open_mutable_overlay_read_snapshot(snapshot, owner)
    }
    fn open_workflow_planning_snapshot(&self, owner: Option<&str>) -> Result<OverlayReadSnapshot> {
        (**self).open_workflow_planning_snapshot(owner)
    }
    fn commit_workflow_transaction(&self, txn: WorkflowTransaction) -> Result<CommitReceipt> {
        (**self).commit_workflow_transaction(txn)
    }
    fn control_get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        (**self).control_get(key)
    }
    fn control_set(&self, key: &[u8], value: Vec<u8>) -> Result<()> {
        (**self).control_set(key, value)
    }
    fn acl_store_control_write(&self, acl: &AclStore) -> WorkflowControlWrite {
        (**self).acl_store_control_write(acl)
    }
}
