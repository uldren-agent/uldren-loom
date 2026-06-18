//! Embedded coordination primitives for one linearization authority.

use crate::error::{LoomError, Result};
use loom_types::Fence;
use std::collections::BTreeMap;

/// The compatibility class of a lock acquisition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockMode {
    Exclusive,
    Shared,
    Semaphore { permits: u32, capacity: u32 },
}

/// A lock owner within one coordinator.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct LockOwner {
    pub principal: String,
    pub session: String,
}

/// A successful acquisition token. Mutating enforced paths carry `fence`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockToken {
    pub key: Vec<u8>,
    pub owner: LockOwner,
    pub mode: LockMode,
    pub fence: Fence,
    pub lease_deadline_ms: u64,
}

/// Runtime lock register plus durable-local fence high-waters for one embedded coordinator.
#[derive(Debug, Default)]
pub struct LockCoordinator {
    locks: BTreeMap<Vec<u8>, LockState>,
    next_fence_by_key: BTreeMap<Vec<u8>, u64>,
    applied_fence_by_key: BTreeMap<Vec<u8>, u64>,
}

#[derive(Debug)]
struct LockState {
    holders: Vec<Holder>,
}

#[derive(Debug)]
struct Holder {
    owner: LockOwner,
    mode: LockMode,
    fence: Fence,
    lease_deadline_ms: u64,
    reentrancy: u32,
}

impl LockCoordinator {
    /// Acquire immediately or return `LOCKED`; callers implement bounded waiting above this layer.
    pub fn try_acquire(
        &mut self,
        key: impl Into<Vec<u8>>,
        owner: LockOwner,
        mode: LockMode,
        lease_ms: u64,
        now_ms: u64,
    ) -> Result<LockToken> {
        validate_mode(mode)?;
        if lease_ms == 0 {
            return Err(LoomError::invalid("lock lease must be greater than zero"));
        }
        let key = key.into();
        self.expire_key(&key, now_ms);
        let deadline = now_ms
            .checked_add(lease_ms)
            .ok_or_else(|| LoomError::invalid("lock lease deadline overflows"))?;

        if let Some(holder) = self.find_holder_mut(&key, &owner) {
            if holder.mode != mode {
                return Err(LoomError::locked(
                    "lock is held by the same owner in another mode",
                ));
            }
            holder.reentrancy = holder
                .reentrancy
                .checked_add(1)
                .ok_or_else(|| LoomError::invalid("lock reentrancy overflows"))?;
            holder.lease_deadline_ms = deadline;
            return Ok(token_for(&key, holder));
        }

        if !self.compatible(&key, mode) {
            return Err(LoomError::locked("lock is held by another owner"));
        }

        let fence = self.issue_fence(&key);
        let holder = Holder {
            owner,
            mode,
            fence,
            lease_deadline_ms: deadline,
            reentrancy: 1,
        };
        let token = token_for(&key, &holder);
        self.locks
            .entry(key)
            .or_insert_with(|| LockState {
                holders: Vec::new(),
            })
            .holders
            .push(holder);
        Ok(token)
    }

    /// Extend a live lease for the holder represented by `token`.
    pub fn refresh(&mut self, token: &LockToken, lease_ms: u64, now_ms: u64) -> Result<LockToken> {
        if lease_ms == 0 {
            return Err(LoomError::invalid("lock lease must be greater than zero"));
        }
        let deadline = now_ms
            .checked_add(lease_ms)
            .ok_or_else(|| LoomError::invalid("lock lease deadline overflows"))?;
        let holder = self.holder_for_token_mut(token)?;
        if holder.lease_deadline_ms <= now_ms {
            self.remove_holder(token);
            return Err(LoomError::lock_lease_expired("lock lease expired"));
        }
        holder.lease_deadline_ms = deadline;
        Ok(token_for(&token.key, holder))
    }

    /// Release one reentrant hold for `token`.
    pub fn release(&mut self, token: &LockToken, now_ms: u64) -> Result<()> {
        let holder = self.holder_for_token_mut(token)?;
        if holder.lease_deadline_ms <= now_ms {
            self.remove_holder(token);
            return Err(LoomError::lock_lease_expired("lock lease expired"));
        }
        if holder.reentrancy > 1 {
            holder.reentrancy -= 1;
            return Ok(());
        }
        self.remove_holder(token);
        Ok(())
    }

    /// Remove all live holders for `key` without changing fence counters.
    pub fn break_key(&mut self, key: &[u8], now_ms: u64) -> usize {
        self.expire_key(key, now_ms);
        self.locks
            .remove(key)
            .map(|state| state.holders.len())
            .unwrap_or(0)
    }

    /// Record that a fenced write was applied to `key`.
    pub fn apply_fence(&mut self, key: &[u8], fence: Fence) -> Result<()> {
        let Some(sequence) = fence.embedded_sequence() else {
            return Err(LoomError::invalid(
                "lock fence is not from the embedded authority",
            ));
        };
        let current = self.applied_fence_by_key.get(key).copied().unwrap_or(0);
        if sequence < current {
            return Err(LoomError::fencing_stale(format!(
                "fence {sequence} is below applied high-water {current}"
            )));
        }
        self.applied_fence_by_key.insert(key.to_vec(), sequence);
        Ok(())
    }

    /// Validate a live lock token and record that its fence was applied to a protected write.
    pub fn apply_fenced_write(&mut self, token: &LockToken, now_ms: u64) -> Result<()> {
        {
            let holder = self.holder_for_token_mut(token)?;
            if holder.lease_deadline_ms <= now_ms {
                self.remove_holder(token);
                return Err(LoomError::lock_lease_expired("lock lease expired"));
            }
        }
        self.apply_fence(&token.key, token.fence)
    }

    /// Highest fence applied to `key`, if any.
    pub fn applied_fence(&self, key: &[u8]) -> Option<Fence> {
        self.applied_fence_by_key
            .get(key)
            .copied()
            .map(Fence::embedded)
    }

    /// Persistable fence counter snapshot.
    pub fn fence_counters(&self) -> Vec<(Vec<u8>, u64)> {
        self.next_fence_by_key
            .iter()
            .map(|(key, value)| (key.clone(), *value))
            .collect()
    }

    /// Persistable applied-high-water snapshot.
    pub fn applied_fences(&self) -> Vec<(Vec<u8>, u64)> {
        self.applied_fence_by_key
            .iter()
            .map(|(key, value)| (key.clone(), *value))
            .collect()
    }

    /// Restore durable-local fence high-waters for one coordinator.
    pub fn restore_fences(
        next: impl IntoIterator<Item = (Vec<u8>, u64)>,
        applied: impl IntoIterator<Item = (Vec<u8>, u64)>,
    ) -> Self {
        Self {
            locks: BTreeMap::new(),
            next_fence_by_key: next.into_iter().collect(),
            applied_fence_by_key: applied.into_iter().collect(),
        }
    }

    fn compatible(&self, key: &[u8], requested: LockMode) -> bool {
        let Some(state) = self.locks.get(key) else {
            return true;
        };
        match requested {
            LockMode::Exclusive => state.holders.is_empty(),
            LockMode::Shared => state
                .holders
                .iter()
                .all(|holder| holder.mode == LockMode::Shared),
            LockMode::Semaphore { permits, capacity } => {
                let mut used = 0u32;
                for holder in &state.holders {
                    let LockMode::Semaphore {
                        permits: held,
                        capacity: held_capacity,
                    } = holder.mode
                    else {
                        return false;
                    };
                    if held_capacity != capacity {
                        return false;
                    }
                    used = match used.checked_add(held) {
                        Some(value) => value,
                        None => return false,
                    };
                }
                used.checked_add(permits)
                    .is_some_and(|total| total <= capacity)
            }
        }
    }

    fn expire_key(&mut self, key: &[u8], now_ms: u64) {
        if let Some(state) = self.locks.get_mut(key) {
            state
                .holders
                .retain(|holder| holder.lease_deadline_ms > now_ms);
        }
        if self
            .locks
            .get(key)
            .is_some_and(|state| state.holders.is_empty())
        {
            self.locks.remove(key);
        }
    }

    fn issue_fence(&mut self, key: &[u8]) -> Fence {
        let counter = self.next_fence_by_key.entry(key.to_vec()).or_insert(0);
        *counter = counter.saturating_add(1);
        Fence::embedded(*counter)
    }

    fn find_holder_mut(&mut self, key: &[u8], owner: &LockOwner) -> Option<&mut Holder> {
        self.locks
            .get_mut(key)?
            .holders
            .iter_mut()
            .find(|holder| holder.owner == *owner)
    }

    fn holder_for_token_mut(&mut self, token: &LockToken) -> Result<&mut Holder> {
        self.locks
            .get_mut(&token.key)
            .and_then(|state| {
                state.holders.iter_mut().find(|holder| {
                    holder.owner == token.owner
                        && holder.mode == token.mode
                        && holder.fence == token.fence
                })
            })
            .ok_or_else(|| LoomError::lock_not_held("lock is not held by this token"))
    }

    fn remove_holder(&mut self, token: &LockToken) {
        if let Some(state) = self.locks.get_mut(&token.key) {
            state.holders.retain(|holder| {
                holder.owner != token.owner
                    || holder.mode != token.mode
                    || holder.fence != token.fence
            });
        }
        if self
            .locks
            .get(&token.key)
            .is_some_and(|state| state.holders.is_empty())
        {
            self.locks.remove(&token.key);
        }
    }
}

fn validate_mode(mode: LockMode) -> Result<()> {
    match mode {
        LockMode::Exclusive | LockMode::Shared => Ok(()),
        LockMode::Semaphore { permits, capacity } => {
            if permits == 0 || capacity == 0 || permits > capacity {
                Err(LoomError::invalid("invalid semaphore permits or capacity"))
            } else {
                Ok(())
            }
        }
    }
}

fn token_for(key: &[u8], holder: &Holder) -> LockToken {
    LockToken {
        key: key.to_vec(),
        owner: holder.owner.clone(),
        mode: holder.mode,
        fence: holder.fence,
        lease_deadline_ms: holder.lease_deadline_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Code;

    fn owner(name: &str) -> LockOwner {
        LockOwner {
            principal: name.to_string(),
            session: "s1".to_string(),
        }
    }

    #[test]
    fn exclusive_lock_excludes_other_owners_and_is_reentrant() {
        let mut c = LockCoordinator::default();
        let a = c
            .try_acquire(b"k", owner("a"), LockMode::Exclusive, 100, 10)
            .unwrap();
        let again = c
            .try_acquire(b"k", owner("a"), LockMode::Exclusive, 100, 20)
            .unwrap();
        assert_eq!(again.fence, a.fence);
        assert_eq!(again.lease_deadline_ms, 120);
        let err = c
            .try_acquire(b"k", owner("b"), LockMode::Exclusive, 100, 20)
            .unwrap_err();
        assert_eq!(err.code, Code::Locked);
        c.release(&again, 20).unwrap();
        assert!(
            c.try_acquire(b"k", owner("b"), LockMode::Exclusive, 100, 20)
                .is_err()
        );
        c.release(&again, 20).unwrap();
        assert!(
            c.try_acquire(b"k", owner("b"), LockMode::Exclusive, 100, 20)
                .is_ok()
        );
    }

    #[test]
    fn shared_locks_coexist_and_block_exclusive() {
        let mut c = LockCoordinator::default();
        c.try_acquire(b"k", owner("a"), LockMode::Shared, 100, 0)
            .unwrap();
        c.try_acquire(b"k", owner("b"), LockMode::Shared, 100, 0)
            .unwrap();
        let err = c
            .try_acquire(b"k", owner("c"), LockMode::Exclusive, 100, 0)
            .unwrap_err();
        assert_eq!(err.code, Code::Locked);
    }

    #[test]
    fn semaphore_locks_respect_capacity() {
        let mut c = LockCoordinator::default();
        c.try_acquire(
            b"k",
            owner("a"),
            LockMode::Semaphore {
                permits: 2,
                capacity: 3,
            },
            100,
            0,
        )
        .unwrap();
        c.try_acquire(
            b"k",
            owner("b"),
            LockMode::Semaphore {
                permits: 1,
                capacity: 3,
            },
            100,
            0,
        )
        .unwrap();
        let err = c
            .try_acquire(
                b"k",
                owner("c"),
                LockMode::Semaphore {
                    permits: 1,
                    capacity: 3,
                },
                100,
                0,
            )
            .unwrap_err();
        assert_eq!(err.code, Code::Locked);
    }

    #[test]
    fn expired_lease_releases_the_lock() {
        let mut c = LockCoordinator::default();
        let token = c
            .try_acquire(b"k", owner("a"), LockMode::Exclusive, 10, 0)
            .unwrap();
        let err = c.release(&token, 10).unwrap_err();
        assert_eq!(err.code, Code::LockLeaseExpired);
        assert!(
            c.try_acquire(b"k", owner("b"), LockMode::Exclusive, 10, 10)
                .is_ok()
        );
    }

    #[test]
    fn break_key_removes_holders_without_reusing_fences() {
        let mut c = LockCoordinator::default();
        let token = c
            .try_acquire(b"k", owner("a"), LockMode::Exclusive, 100, 0)
            .unwrap();
        assert_eq!(c.break_key(b"k", 10), 1);
        assert_eq!(
            c.apply_fenced_write(&token, 11).unwrap_err().code,
            Code::LockNotHeld
        );
        let next = c
            .try_acquire(b"k", owner("b"), LockMode::Exclusive, 100, 12)
            .unwrap();
        assert_eq!(next.fence, Fence::embedded(2));
    }

    #[test]
    fn stale_fences_are_rejected() {
        let mut c = LockCoordinator::default();
        c.apply_fence(b"k", Fence::embedded(7)).unwrap();
        c.apply_fence(b"k", Fence::embedded(7)).unwrap();
        let err = c.apply_fence(b"k", Fence::embedded(6)).unwrap_err();
        assert_eq!(err.code, Code::FencingStale);
        assert_eq!(c.applied_fence(b"k"), Some(Fence::embedded(7)));
        assert_eq!(
            c.apply_fence(b"k", Fence::new(1, 0, 8)).unwrap_err().code,
            Code::InvalidArgument
        );
    }

    #[test]
    fn fenced_write_requires_live_token() {
        let mut c = LockCoordinator::default();
        let token = c
            .try_acquire(b"k", owner("a"), LockMode::Exclusive, 10, 0)
            .unwrap();
        c.apply_fenced_write(&token, 9).unwrap();
        assert_eq!(c.applied_fence(b"k"), Some(token.fence));

        let err = c.apply_fenced_write(&token, 10).unwrap_err();
        assert_eq!(err.code, Code::LockLeaseExpired);

        let next = c
            .try_acquire(b"k", owner("b"), LockMode::Exclusive, 10, 10)
            .unwrap();
        let stale = LockToken {
            key: b"k".to_vec(),
            owner: next.owner.clone(),
            mode: next.mode,
            fence: token.fence,
            lease_deadline_ms: next.lease_deadline_ms,
        };
        assert_eq!(
            c.apply_fenced_write(&stale, 11).unwrap_err().code,
            Code::LockNotHeld
        );
    }

    #[test]
    fn fence_counters_survive_restore_without_live_holders() {
        let mut c = LockCoordinator::default();
        let a = c
            .try_acquire(b"k", owner("a"), LockMode::Exclusive, 100, 0)
            .unwrap();
        c.apply_fence(b"k", a.fence).unwrap();
        let mut restored = LockCoordinator::restore_fences(c.fence_counters(), c.applied_fences());
        let b = restored
            .try_acquire(b"k", owner("b"), LockMode::Exclusive, 100, 0)
            .unwrap();
        assert!(b.fence > a.fence);
        restored.apply_fence(b"k", b.fence).unwrap();
        assert_eq!(
            restored.apply_fence(b"k", a.fence).unwrap_err().code,
            Code::FencingStale
        );
    }
}
