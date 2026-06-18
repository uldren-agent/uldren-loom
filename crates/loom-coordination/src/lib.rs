//! Reusable coordination contracts for one Loom authority.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, Weak};

use loom_types::{Code, LoomError, Result};

static LOCAL_STORE_WRITE_LOCKS: LazyLock<Mutex<BTreeMap<PathBuf, Weak<Mutex<()>>>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));

pub fn local_store_write_lock(path: &Path) -> Arc<Mutex<()>> {
    let key = local_store_write_lock_key(path);
    let Ok(mut locks) = LOCAL_STORE_WRITE_LOCKS.lock() else {
        return Arc::new(Mutex::new(()));
    };
    if let Some(lock) = locks.get(&key).and_then(Weak::upgrade) {
        return lock;
    }
    locks.retain(|_, lock| lock.strong_count() > 0);
    let lock = Arc::new(Mutex::new(()));
    locks.insert(key, Arc::downgrade(&lock));
    lock
}

pub fn local_store_write_lock_key(path: &Path) -> PathBuf {
    if let Ok(path) = std::fs::canonicalize(path) {
        return path;
    }
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(file_name)) => std::fs::canonicalize(parent)
            .map(|parent| parent.join(file_name))
            .unwrap_or_else(|_| path.to_path_buf()),
        _ => path.to_path_buf(),
    }
}

pub fn with_local_store_write_lock<T>(path: &Path, f: impl FnOnce() -> Result<T>) -> Result<T> {
    let lock = local_store_write_lock(path);
    let _guard = lock
        .lock()
        .map_err(|_| LoomError::new(Code::Internal, "local store write lock poisoned"))?;
    f()
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct AuthorityId(String);

impl AuthorityId {
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        validate_text("authority id", &value)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct AuthorityEpoch(u64);

impl AuthorityEpoch {
    pub fn new(value: u64) -> Result<Self> {
        if value == 0 {
            return Err(LoomError::invalid(
                "authority epoch must be greater than zero",
            ));
        }
        Ok(Self(value))
    }

    pub fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct CoordinationScope(Vec<String>);

impl CoordinationScope {
    pub fn new(parts: impl IntoIterator<Item = impl Into<String>>) -> Result<Self> {
        let parts = parts.into_iter().map(Into::into).collect::<Vec<String>>();
        if parts.is_empty() {
            return Err(LoomError::invalid(
                "coordination scope must have at least one part",
            ));
        }
        for part in &parts {
            validate_text("coordination scope part", part)?;
        }
        Ok(Self(parts))
    }

    pub fn parts(&self) -> &[String] {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct ActorId(String);

impl ActorId {
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        validate_text("actor id", &value)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct ProducerIdentity(String);

impl ProducerIdentity {
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        validate_text("producer identity", &value)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct TransactionIdentity(String);

impl TransactionIdentity {
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        validate_text("transaction identity", &value)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct FenceToken(u128);

impl FenceToken {
    pub fn get(self) -> u128 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct Sequence(u64);

impl Sequence {
    pub fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct ProducerEpoch(u64);

impl ProducerEpoch {
    pub fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct TransactionEpoch(u64);

impl TransactionEpoch {
    pub fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct GroupGeneration(u64);

impl GroupGeneration {
    pub fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransactionState {
    Active,
    Committed,
    Aborted,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransactionRecord {
    pub epoch: TransactionEpoch,
    pub state: TransactionState,
}

pub trait CoordinationAuthority {
    fn authority_id(&self) -> &AuthorityId;
    fn authority_epoch(&self) -> AuthorityEpoch;
    fn next_fence(&mut self, scope: &CoordinationScope) -> Result<FenceToken>;
    fn next_sequence(&mut self, scope: &CoordinationScope) -> Result<Sequence>;
    fn apply_fence(
        &mut self,
        scope: &CoordinationScope,
        actor: &ActorId,
        fence: FenceToken,
    ) -> Result<()>;
}

pub trait ProducerCoordinator {
    fn register_producer(
        &mut self,
        scope: &CoordinationScope,
        producer: &ProducerIdentity,
    ) -> Result<ProducerEpoch>;

    fn validate_producer_epoch(
        &self,
        scope: &CoordinationScope,
        producer: &ProducerIdentity,
        epoch: ProducerEpoch,
    ) -> Result<()>;
}

pub trait TransactionCoordinator {
    fn begin_transaction(
        &mut self,
        scope: &CoordinationScope,
        transaction: &TransactionIdentity,
    ) -> Result<TransactionEpoch>;

    fn commit_transaction(
        &mut self,
        scope: &CoordinationScope,
        transaction: &TransactionIdentity,
        epoch: TransactionEpoch,
    ) -> Result<()>;

    fn abort_transaction(
        &mut self,
        scope: &CoordinationScope,
        transaction: &TransactionIdentity,
        epoch: TransactionEpoch,
    ) -> Result<()>;
}

#[derive(Clone, Debug)]
pub struct SingleNodeCoordinator {
    authority_id: AuthorityId,
    authority_epoch: AuthorityEpoch,
    next_fence_by_scope: BTreeMap<CoordinationScope, u128>,
    applied_fence_by_actor: BTreeMap<(CoordinationScope, ActorId), u128>,
    next_sequence_by_scope: BTreeMap<CoordinationScope, u64>,
    producer_epoch_by_key: BTreeMap<(CoordinationScope, ProducerIdentity), u64>,
    group_generation_by_scope: BTreeMap<CoordinationScope, u64>,
    transaction_by_key: BTreeMap<(CoordinationScope, TransactionIdentity), TransactionRecord>,
}

impl SingleNodeCoordinator {
    pub fn new(authority_id: AuthorityId, authority_epoch: AuthorityEpoch) -> Self {
        Self {
            authority_id,
            authority_epoch,
            next_fence_by_scope: BTreeMap::new(),
            applied_fence_by_actor: BTreeMap::new(),
            next_sequence_by_scope: BTreeMap::new(),
            producer_epoch_by_key: BTreeMap::new(),
            group_generation_by_scope: BTreeMap::new(),
            transaction_by_key: BTreeMap::new(),
        }
    }

    pub fn next_group_generation(&mut self, scope: &CoordinationScope) -> Result<GroupGeneration> {
        let next = self
            .group_generation_by_scope
            .entry(scope.clone())
            .or_insert(1);
        let generation = *next;
        *next = next
            .checked_add(1)
            .ok_or_else(|| LoomError::invalid("group generation overflows"))?;
        Ok(GroupGeneration(generation))
    }

    pub fn transaction_record(
        &self,
        scope: &CoordinationScope,
        transaction: &TransactionIdentity,
    ) -> Option<&TransactionRecord> {
        self.transaction_by_key
            .get(&(scope.clone(), transaction.clone()))
    }

    pub fn producer_epoch(
        &self,
        scope: &CoordinationScope,
        producer: &ProducerIdentity,
    ) -> Option<ProducerEpoch> {
        self.producer_epoch_by_key
            .get(&(scope.clone(), producer.clone()))
            .copied()
            .map(ProducerEpoch)
    }
}

impl CoordinationAuthority for SingleNodeCoordinator {
    fn authority_id(&self) -> &AuthorityId {
        &self.authority_id
    }

    fn authority_epoch(&self) -> AuthorityEpoch {
        self.authority_epoch
    }

    fn next_fence(&mut self, scope: &CoordinationScope) -> Result<FenceToken> {
        let next = self.next_fence_by_scope.entry(scope.clone()).or_insert(1);
        let fence = *next;
        *next = next
            .checked_add(1)
            .ok_or_else(|| LoomError::invalid("fence token overflows"))?;
        Ok(FenceToken(fence))
    }

    fn next_sequence(&mut self, scope: &CoordinationScope) -> Result<Sequence> {
        let next = self
            .next_sequence_by_scope
            .entry(scope.clone())
            .or_insert(0);
        let sequence = *next;
        *next = next
            .checked_add(1)
            .ok_or_else(|| LoomError::invalid("sequence overflows"))?;
        Ok(Sequence(sequence))
    }

    fn apply_fence(
        &mut self,
        scope: &CoordinationScope,
        actor: &ActorId,
        fence: FenceToken,
    ) -> Result<()> {
        let key = (scope.clone(), actor.clone());
        let current = self.applied_fence_by_actor.get(&key).copied().unwrap_or(0);
        if fence.0 < current {
            return Err(LoomError::fencing_stale(format!(
                "fence {} is below applied high-water {current}",
                fence.0
            )));
        }
        self.applied_fence_by_actor.insert(key, fence.0);
        Ok(())
    }
}

impl ProducerCoordinator for SingleNodeCoordinator {
    fn register_producer(
        &mut self,
        scope: &CoordinationScope,
        producer: &ProducerIdentity,
    ) -> Result<ProducerEpoch> {
        let next = self
            .producer_epoch_by_key
            .entry((scope.clone(), producer.clone()))
            .or_insert(0);
        *next = next
            .checked_add(1)
            .ok_or_else(|| LoomError::invalid("producer epoch overflows"))?;
        Ok(ProducerEpoch(*next))
    }

    fn validate_producer_epoch(
        &self,
        scope: &CoordinationScope,
        producer: &ProducerIdentity,
        epoch: ProducerEpoch,
    ) -> Result<()> {
        let Some(current) = self
            .producer_epoch_by_key
            .get(&(scope.clone(), producer.clone()))
            .copied()
        else {
            return Err(LoomError::not_found("producer is not registered"));
        };
        if epoch.0 < current {
            return Err(LoomError::fencing_stale(format!(
                "producer epoch {} is below current epoch {current}",
                epoch.0
            )));
        }
        if epoch.0 > current {
            return Err(LoomError::invalid(format!(
                "producer epoch {} is above current epoch {current}",
                epoch.0
            )));
        }
        Ok(())
    }
}

impl TransactionCoordinator for SingleNodeCoordinator {
    fn begin_transaction(
        &mut self,
        scope: &CoordinationScope,
        transaction: &TransactionIdentity,
    ) -> Result<TransactionEpoch> {
        let key = (scope.clone(), transaction.clone());
        let next_epoch = match self.transaction_by_key.get(&key) {
            Some(record) => record
                .epoch
                .0
                .checked_add(1)
                .ok_or_else(|| LoomError::invalid("transaction epoch overflows"))?,
            None => 1,
        };
        self.transaction_by_key.insert(
            key,
            TransactionRecord {
                epoch: TransactionEpoch(next_epoch),
                state: TransactionState::Active,
            },
        );
        Ok(TransactionEpoch(next_epoch))
    }

    fn commit_transaction(
        &mut self,
        scope: &CoordinationScope,
        transaction: &TransactionIdentity,
        epoch: TransactionEpoch,
    ) -> Result<()> {
        complete_transaction(
            &mut self.transaction_by_key,
            scope,
            transaction,
            epoch,
            TransactionState::Committed,
        )
    }

    fn abort_transaction(
        &mut self,
        scope: &CoordinationScope,
        transaction: &TransactionIdentity,
        epoch: TransactionEpoch,
    ) -> Result<()> {
        complete_transaction(
            &mut self.transaction_by_key,
            scope,
            transaction,
            epoch,
            TransactionState::Aborted,
        )
    }
}

fn complete_transaction(
    transactions: &mut BTreeMap<(CoordinationScope, TransactionIdentity), TransactionRecord>,
    scope: &CoordinationScope,
    transaction: &TransactionIdentity,
    epoch: TransactionEpoch,
    terminal_state: TransactionState,
) -> Result<()> {
    let record = transactions
        .get_mut(&(scope.clone(), transaction.clone()))
        .ok_or_else(|| LoomError::not_found("transaction is not registered"))?;
    if epoch.0 < record.epoch.0 {
        return Err(LoomError::fencing_stale(format!(
            "transaction epoch {} is below current epoch {}",
            epoch.0, record.epoch.0
        )));
    }
    if epoch.0 > record.epoch.0 {
        return Err(LoomError::invalid(format!(
            "transaction epoch {} is above current epoch {}",
            epoch.0, record.epoch.0
        )));
    }
    if record.state != TransactionState::Active {
        return Err(LoomError::new(
            Code::Conflict,
            format!("transaction is already {:?}", record.state),
        ));
    }
    record.state = terminal_state;
    Ok(())
}

fn validate_text(name: &str, value: &str) -> Result<()> {
    if value.is_empty() {
        return Err(LoomError::invalid(format!("{name} must not be empty")));
    }
    if value
        .chars()
        .any(|ch| ch == '\0' || ch == '/' || ch.is_control())
    {
        return Err(LoomError::invalid(format!(
            "{name} must not contain '/', NUL, or control characters"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_types::Code;

    fn coordinator() -> SingleNodeCoordinator {
        SingleNodeCoordinator::new(
            AuthorityId::new("local").unwrap(),
            AuthorityEpoch::new(1).unwrap(),
        )
    }

    fn scope(parts: &[&str]) -> CoordinationScope {
        CoordinationScope::new(parts.iter().copied()).unwrap()
    }

    #[test]
    fn sequences_are_monotonic_per_scope() {
        let mut coordinator = coordinator();
        let a = scope(&["kafka", "work", "topic-a", "0"]);
        let b = scope(&["kafka", "work", "topic-b", "0"]);
        assert_eq!(coordinator.next_sequence(&a).unwrap().get(), 0);
        assert_eq!(coordinator.next_sequence(&a).unwrap().get(), 1);
        assert_eq!(coordinator.next_sequence(&b).unwrap().get(), 0);
    }

    #[test]
    fn stale_fences_are_rejected_per_actor() {
        let mut coordinator = coordinator();
        let scope = scope(&["kafka", "work"]);
        let actor = ActorId::new("producer-1").unwrap();
        let first = coordinator.next_fence(&scope).unwrap();
        let second = coordinator.next_fence(&scope).unwrap();
        coordinator.apply_fence(&scope, &actor, second).unwrap();
        let err = coordinator.apply_fence(&scope, &actor, first).unwrap_err();
        assert_eq!(err.code, Code::FencingStale);
    }

    #[test]
    fn producer_epoch_registration_fences_stale_producers() {
        let mut coordinator = coordinator();
        let scope = scope(&["kafka", "work"]);
        let producer = ProducerIdentity::new("producer-1").unwrap();
        let first = coordinator.register_producer(&scope, &producer).unwrap();
        let second = coordinator.register_producer(&scope, &producer).unwrap();
        assert_eq!(first.get(), 1);
        assert_eq!(second.get(), 2);
        coordinator
            .validate_producer_epoch(&scope, &producer, second)
            .unwrap();
        let err = coordinator
            .validate_producer_epoch(&scope, &producer, first)
            .unwrap_err();
        assert_eq!(err.code, Code::FencingStale);
    }

    #[test]
    fn group_generations_are_monotonic_per_group_scope() {
        let mut coordinator = coordinator();
        let group = scope(&["kafka", "work", "group", "analytics"]);
        assert_eq!(coordinator.next_group_generation(&group).unwrap().get(), 1);
        assert_eq!(coordinator.next_group_generation(&group).unwrap().get(), 2);
    }

    #[test]
    fn transaction_terminal_states_are_enforced() {
        let mut coordinator = coordinator();
        let scope = scope(&["kafka", "work"]);
        let transaction = TransactionIdentity::new("tx-1").unwrap();
        let epoch = coordinator.begin_transaction(&scope, &transaction).unwrap();
        coordinator
            .commit_transaction(&scope, &transaction, epoch)
            .unwrap();
        assert_eq!(
            coordinator
                .transaction_record(&scope, &transaction)
                .unwrap()
                .state,
            TransactionState::Committed
        );
        let err = coordinator
            .abort_transaction(&scope, &transaction, epoch)
            .unwrap_err();
        assert_eq!(err.code, Code::Conflict);
    }

    #[test]
    fn transaction_epochs_fence_stale_calls() {
        let mut coordinator = coordinator();
        let scope = scope(&["kafka", "work"]);
        let transaction = TransactionIdentity::new("tx-1").unwrap();
        let first = coordinator.begin_transaction(&scope, &transaction).unwrap();
        let second = coordinator.begin_transaction(&scope, &transaction).unwrap();
        assert_eq!(first.get(), 1);
        assert_eq!(second.get(), 2);
        let err = coordinator
            .commit_transaction(&scope, &transaction, first)
            .unwrap_err();
        assert_eq!(err.code, Code::FencingStale);
    }
}
