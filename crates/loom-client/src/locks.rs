use loom_core::{LockCoordinator, LockMode, LockOwner, LockToken};
use loom_types::{Code, LoomError};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Durable publication boundary for embedded lock fence state.
pub trait LocksPersistence: Send + Sync {
    fn persist(&self, coordinator: &LockCoordinator) -> Result<(), LoomError>;
}

/// One shared semantic authority for coordination locks.
pub trait LocksAuthority: Send + Sync {
    fn acquire(
        &self,
        owner: LockOwner,
        key: Vec<u8>,
        mode: LockMode,
        lease_ms: u64,
        wait_ms: u64,
    ) -> Result<LockToken, LoomError>;

    fn refresh(
        &self,
        owner: &LockOwner,
        token: &LockToken,
        lease_ms: u64,
    ) -> Result<LockToken, LoomError>;

    fn release(&self, owner: &LockOwner, token: &LockToken) -> Result<(), LoomError>;

    fn break_key(&self, key: &[u8]) -> Result<usize, LoomError>;

    fn apply_fenced_write(&self, owner: &LockOwner, token: &LockToken) -> Result<(), LoomError>;

    fn close_owner(&self, owner: &LockOwner) -> Result<usize, LoomError>;
}

#[derive(Default)]
struct VolatileLocksPersistence;

impl LocksPersistence for VolatileLocksPersistence {
    fn persist(&self, _coordinator: &LockCoordinator) -> Result<(), LoomError> {
        Ok(())
    }
}

/// In-process lock authority with an injectable durable fence publisher.
pub struct InProcessLocksAuthority {
    coordinator: Mutex<LockCoordinator>,
    changed: Condvar,
    persistence: Arc<dyn LocksPersistence>,
}

impl Default for InProcessLocksAuthority {
    fn default() -> Self {
        Self::with_persistence(Arc::new(VolatileLocksPersistence))
    }
}

impl InProcessLocksAuthority {
    pub fn with_persistence(persistence: Arc<dyn LocksPersistence>) -> Self {
        Self {
            coordinator: Mutex::new(LockCoordinator::default()),
            changed: Condvar::new(),
            persistence,
        }
    }

    pub fn from_coordinator(
        coordinator: LockCoordinator,
        persistence: Arc<dyn LocksPersistence>,
    ) -> Self {
        Self {
            coordinator: Mutex::new(coordinator),
            changed: Condvar::new(),
            persistence,
        }
    }

    fn publish_outcome<T>(
        &self,
        coordinator: &mut LockCoordinator,
        previous: LockCoordinator,
        outcome: Result<T, LoomError>,
    ) -> Result<(Result<T, LoomError>, bool), LoomError> {
        let changed = *coordinator != previous;
        if changed {
            if let Err(err) = self.persistence.persist(coordinator) {
                *coordinator = previous;
                return Err(err);
            }
        }
        Ok((outcome, changed))
    }
}

impl LocksAuthority for InProcessLocksAuthority {
    fn acquire(
        &self,
        owner: LockOwner,
        key: Vec<u8>,
        mode: LockMode,
        lease_ms: u64,
        wait_ms: u64,
    ) -> Result<LockToken, LoomError> {
        let started = Instant::now();
        let wait = Duration::from_millis(wait_ms);
        let mut coordinator = self
            .coordinator
            .lock()
            .map_err(|_| LoomError::new(Code::Internal, "lock authority poisoned"))?;
        loop {
            let previous = coordinator.clone();
            let outcome =
                coordinator.try_acquire(key.clone(), owner.clone(), mode, lease_ms, now_ms());
            let (outcome, changed) = self.publish_outcome(&mut coordinator, previous, outcome)?;
            if changed {
                self.changed.notify_all();
            }
            match outcome {
                Ok(token) => {
                    return Ok(token);
                }
                Err(err) if err.code == Code::Locked && wait_ms > 0 => {
                    let Some(remaining) = wait.checked_sub(started.elapsed()) else {
                        return Err(err);
                    };
                    let current_ms = now_ms();
                    let lease_wait = coordinator
                        .earliest_live_lease_deadline(&key, current_ms)
                        .map(|deadline| Duration::from_millis(deadline.saturating_sub(current_ms)))
                        .unwrap_or(remaining);
                    let interval = remaining.min(lease_wait);
                    let (next, _) = self
                        .changed
                        .wait_timeout(coordinator, interval)
                        .map_err(|_| LoomError::new(Code::Internal, "lock authority poisoned"))?;
                    coordinator = next;
                }
                Err(err) => return Err(err),
            }
        }
    }

    fn refresh(
        &self,
        owner: &LockOwner,
        token: &LockToken,
        lease_ms: u64,
    ) -> Result<LockToken, LoomError> {
        require_token_owner(owner, token)?;
        let mut coordinator = self
            .coordinator
            .lock()
            .map_err(|_| LoomError::new(Code::Internal, "lock authority poisoned"))?;
        let previous = coordinator.clone();
        let outcome = coordinator.refresh(token, lease_ms, now_ms());
        let (outcome, changed) = self.publish_outcome(&mut coordinator, previous, outcome)?;
        if changed {
            self.changed.notify_all();
        }
        outcome
    }

    fn release(&self, owner: &LockOwner, token: &LockToken) -> Result<(), LoomError> {
        require_token_owner(owner, token)?;
        let mut coordinator = self
            .coordinator
            .lock()
            .map_err(|_| LoomError::new(Code::Internal, "lock authority poisoned"))?;
        let previous = coordinator.clone();
        let outcome = coordinator.release(token, now_ms());
        let (outcome, changed) = self.publish_outcome(&mut coordinator, previous, outcome)?;
        drop(coordinator);
        if changed {
            self.changed.notify_all();
        }
        outcome
    }

    fn break_key(&self, key: &[u8]) -> Result<usize, LoomError> {
        let mut coordinator = self
            .coordinator
            .lock()
            .map_err(|_| LoomError::new(Code::Internal, "lock authority poisoned"))?;
        let previous = coordinator.clone();
        let outcome = Ok(coordinator.break_key(key, now_ms()));
        let (outcome, changed) = self.publish_outcome(&mut coordinator, previous, outcome)?;
        drop(coordinator);
        if changed {
            self.changed.notify_all();
        }
        outcome
    }

    fn apply_fenced_write(&self, owner: &LockOwner, token: &LockToken) -> Result<(), LoomError> {
        require_token_owner(owner, token)?;
        let mut coordinator = self
            .coordinator
            .lock()
            .map_err(|_| LoomError::new(Code::Internal, "lock authority poisoned"))?;
        let previous = coordinator.clone();
        let outcome = coordinator.apply_fenced_write(token, now_ms());
        let (outcome, changed) = self.publish_outcome(&mut coordinator, previous, outcome)?;
        drop(coordinator);
        if changed {
            self.changed.notify_all();
        }
        outcome
    }

    fn close_owner(&self, owner: &LockOwner) -> Result<usize, LoomError> {
        let mut coordinator = self
            .coordinator
            .lock()
            .map_err(|_| LoomError::new(Code::Internal, "lock authority poisoned"))?;
        let previous = coordinator.clone();
        let outcome = Ok(coordinator.release_owner(owner, now_ms()));
        let (outcome, changed) = self.publish_outcome(&mut coordinator, previous, outcome)?;
        drop(coordinator);
        if changed {
            self.changed.notify_all();
        }
        outcome
    }
}

fn require_token_owner(owner: &LockOwner, token: &LockToken) -> Result<(), LoomError> {
    if token.owner != *owner {
        return Err(LoomError::new(
            Code::PermissionDenied,
            "lock token belongs to another authenticated session",
        ));
    }
    Ok(())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::mpsc;
    use std::thread;

    fn owner(name: &str) -> LockOwner {
        LockOwner {
            principal: name.to_string(),
            session: format!("session-{name}"),
        }
    }

    #[derive(Default)]
    struct ControlledPersistence {
        fail: AtomicBool,
        calls: AtomicU64,
        last: Mutex<Option<LockCoordinator>>,
    }

    impl LocksPersistence for ControlledPersistence {
        fn persist(&self, coordinator: &LockCoordinator) -> Result<(), LoomError> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            if self.fail.load(Ordering::Relaxed) {
                Err(LoomError::new(
                    Code::Io,
                    "injected lock persistence failure",
                ))
            } else {
                *self.last.lock().expect("persistence snapshot") = Some(coordinator.clone());
                Ok(())
            }
        }
    }

    #[test]
    fn canonical_locks_shared_authority_contends_and_fences() {
        let authority = InProcessLocksAuthority::default();
        let first = authority
            .acquire(owner("a"), b"key".to_vec(), LockMode::Exclusive, 5_000, 0)
            .expect("first acquire");
        assert_eq!(
            authority
                .acquire(owner("b"), b"key".to_vec(), LockMode::Exclusive, 5_000, 0)
                .expect_err("contended acquire")
                .code,
            Code::Locked
        );
        authority.release(&owner("a"), &first).expect("release");
        let second = authority
            .acquire(owner("b"), b"key".to_vec(), LockMode::Exclusive, 5_000, 0)
            .expect("second acquire");
        assert!(second.fence > first.fence);
    }

    #[test]
    fn canonical_locks_close_owner_releases_and_rolls_back_on_persistence_failure() {
        let persistence = Arc::new(ControlledPersistence::default());
        let authority = InProcessLocksAuthority::with_persistence(persistence.clone());
        let token = authority
            .acquire(
                owner("session"),
                b"key".to_vec(),
                LockMode::Exclusive,
                5_000,
                0,
            )
            .unwrap();
        persistence.fail.store(true, Ordering::Relaxed);
        assert_eq!(
            authority
                .close_owner(&owner("session"))
                .expect_err("close publication fails")
                .code,
            Code::Io
        );
        persistence.fail.store(false, Ordering::Relaxed);
        authority
            .refresh(&owner("session"), &token, 5_000)
            .expect("holder restored exactly");
        assert_eq!(authority.close_owner(&owner("session")).unwrap(), 1);
        authority
            .acquire(
                owner("other"),
                b"key".to_vec(),
                LockMode::Exclusive,
                5_000,
                0,
            )
            .expect("waiter can acquire after close");
    }

    #[test]
    fn canonical_locks_bound_wait_and_release_wakeup() {
        let authority = Arc::new(InProcessLocksAuthority::default());
        let first = authority
            .acquire(owner("a"), b"key".to_vec(), LockMode::Exclusive, 5_000, 0)
            .expect("first acquire");
        let started = Instant::now();
        assert_eq!(
            authority
                .acquire(owner("b"), b"key".to_vec(), LockMode::Exclusive, 5_000, 20,)
                .expect_err("bounded wait")
                .code,
            Code::Locked
        );
        assert!(started.elapsed() >= Duration::from_millis(15));
        assert!(started.elapsed() < Duration::from_secs(1));

        let (ready_tx, ready_rx) = mpsc::channel();
        let waiter = Arc::clone(&authority);
        let join = thread::spawn(move || {
            ready_tx.send(()).expect("waiter ready");
            waiter.acquire(
                owner("b"),
                b"key".to_vec(),
                LockMode::Exclusive,
                5_000,
                1_000,
            )
        });
        ready_rx.recv().expect("waiter started");
        thread::sleep(Duration::from_millis(10));
        authority.release(&owner("a"), &first).expect("release");
        let acquired = join.join().expect("waiter join").expect("waiter acquire");
        assert_eq!(acquired.owner, owner("b"));
    }

    #[test]
    fn canonical_locks_waiter_retries_at_natural_lease_expiration() {
        let authority = InProcessLocksAuthority::default();
        authority
            .acquire(owner("a"), b"key".to_vec(), LockMode::Exclusive, 40, 0)
            .expect("first acquire");
        let started = Instant::now();
        let acquired = authority
            .acquire(
                owner("b"),
                b"key".to_vec(),
                LockMode::Exclusive,
                5_000,
                1_000,
            )
            .expect("acquire after expiration");
        assert_eq!(acquired.owner, owner("b"));
        assert!(started.elapsed() >= Duration::from_millis(20));
        assert!(started.elapsed() < Duration::from_millis(500));
    }

    #[test]
    fn canonical_locks_expired_errors_publish_holder_removal() {
        let persistence = Arc::new(ControlledPersistence::default());
        let authority = InProcessLocksAuthority::with_persistence(persistence.clone());
        let refresh_token = authority
            .acquire(
                owner("refresh"),
                b"refresh".to_vec(),
                LockMode::Exclusive,
                10,
                0,
            )
            .expect("refresh token");
        thread::sleep(Duration::from_millis(20));
        assert_eq!(
            authority
                .refresh(&owner("refresh"), &refresh_token, 1_000)
                .expect_err("expired refresh")
                .code,
            Code::LockLeaseExpired
        );
        assert_eq!(persistence.calls.load(Ordering::Relaxed), 2);
        assert_eq!(
            authority
                .release(&owner("refresh"), &refresh_token)
                .expect_err("removed refresh holder")
                .code,
            Code::LockNotHeld
        );

        let release_token = authority
            .acquire(
                owner("release"),
                b"release".to_vec(),
                LockMode::Exclusive,
                10,
                0,
            )
            .expect("release token");
        thread::sleep(Duration::from_millis(20));
        assert_eq!(
            authority
                .release(&owner("release"), &release_token)
                .expect_err("expired release")
                .code,
            Code::LockLeaseExpired
        );
        assert_eq!(persistence.calls.load(Ordering::Relaxed), 4);
        assert_eq!(
            authority
                .refresh(&owner("release"), &release_token, 1_000)
                .expect_err("removed release holder")
                .code,
            Code::LockNotHeld
        );
    }

    #[test]
    fn canonical_locks_reject_foreign_session_tokens() {
        let authority = InProcessLocksAuthority::default();
        let token = authority
            .acquire(owner("a"), b"key".to_vec(), LockMode::Exclusive, 5_000, 0)
            .expect("acquire");
        assert_eq!(
            authority
                .refresh(&owner("b"), &token, 5_000)
                .expect_err("foreign refresh")
                .code,
            Code::PermissionDenied
        );
        assert_eq!(
            authority
                .release(&owner("b"), &token)
                .expect_err("foreign release")
                .code,
            Code::PermissionDenied
        );
        authority
            .release(&owner("a"), &token)
            .expect("owner release");
    }

    #[test]
    fn canonical_locks_persistence_failure_rolls_back_grant_and_release() {
        let persistence = Arc::new(ControlledPersistence::default());
        persistence.fail.store(true, Ordering::Relaxed);
        let authority = InProcessLocksAuthority::with_persistence(persistence.clone());
        assert_eq!(
            authority
                .acquire(owner("a"), b"key".to_vec(), LockMode::Exclusive, 5_000, 0)
                .expect_err("failed grant publication")
                .code,
            Code::Io
        );

        persistence.fail.store(false, Ordering::Relaxed);
        let token = authority
            .acquire(owner("b"), b"key".to_vec(), LockMode::Exclusive, 5_000, 0)
            .expect("grant after rollback");
        assert_eq!(token.fence, loom_types::Fence::embedded(1));

        persistence.fail.store(true, Ordering::Relaxed);
        assert_eq!(
            authority
                .release(&owner("b"), &token)
                .expect_err("failed release publication")
                .code,
            Code::Io
        );
        assert_eq!(
            authority
                .acquire(owner("a"), b"key".to_vec(), LockMode::Exclusive, 5_000, 0)
                .expect_err("release rollback retains holder")
                .code,
            Code::Locked
        );
        persistence.fail.store(false, Ordering::Relaxed);
        authority
            .release(&owner("b"), &token)
            .expect("release retry");
        assert!(persistence.calls.load(Ordering::Relaxed) >= 4);
    }

    #[test]
    fn canonical_locks_expired_error_publication_failure_restores_prior_state() {
        let persistence = Arc::new(ControlledPersistence::default());
        let authority = InProcessLocksAuthority::with_persistence(persistence.clone());
        let refresh_token = authority
            .acquire(
                owner("refresh"),
                b"refresh".to_vec(),
                LockMode::Exclusive,
                10,
                0,
            )
            .expect("refresh token");
        thread::sleep(Duration::from_millis(20));
        persistence.fail.store(true, Ordering::Relaxed);
        assert_eq!(
            authority
                .refresh(&owner("refresh"), &refresh_token, 1_000)
                .expect_err("failed expired refresh publication")
                .code,
            Code::Io
        );
        persistence.fail.store(false, Ordering::Relaxed);
        assert_eq!(
            authority
                .refresh(&owner("refresh"), &refresh_token, 1_000)
                .expect_err("restored expired refresh holder")
                .code,
            Code::LockLeaseExpired
        );

        let release_token = authority
            .acquire(
                owner("release"),
                b"release".to_vec(),
                LockMode::Exclusive,
                10,
                0,
            )
            .expect("release token");
        thread::sleep(Duration::from_millis(20));
        persistence.fail.store(true, Ordering::Relaxed);
        assert_eq!(
            authority
                .release(&owner("release"), &release_token)
                .expect_err("failed expired release publication")
                .code,
            Code::Io
        );
        persistence.fail.store(false, Ordering::Relaxed);
        assert_eq!(
            authority
                .release(&owner("release"), &release_token)
                .expect_err("restored expired release holder")
                .code,
            Code::LockLeaseExpired
        );
    }

    #[test]
    fn canonical_locks_break_wakes_waiter() {
        let authority = Arc::new(InProcessLocksAuthority::default());
        authority
            .acquire(
                owner("holder"),
                b"key".to_vec(),
                LockMode::Exclusive,
                5_000,
                0,
            )
            .expect("holder acquire");
        let (ready_tx, ready_rx) = mpsc::channel();
        let waiter_authority = Arc::clone(&authority);
        let waiter = thread::spawn(move || {
            ready_tx.send(()).expect("waiter ready");
            waiter_authority.acquire(
                owner("waiter"),
                b"key".to_vec(),
                LockMode::Exclusive,
                5_000,
                1_000,
            )
        });
        ready_rx.recv().expect("waiter started");
        thread::sleep(Duration::from_millis(10));
        assert_eq!(authority.break_key(b"key").expect("break key"), 1);
        let acquired = waiter.join().expect("waiter join").expect("waiter acquire");
        assert_eq!(acquired.owner, owner("waiter"));
    }

    #[test]
    fn canonical_locks_apply_fenced_write_rejects_expired_and_stale_tokens() {
        let authority = InProcessLocksAuthority::default();
        let expired = authority
            .acquire(
                owner("expired"),
                b"expired".to_vec(),
                LockMode::Exclusive,
                10,
                0,
            )
            .expect("expired token");
        thread::sleep(Duration::from_millis(20));
        assert_eq!(
            authority
                .apply_fenced_write(&owner("expired"), &expired)
                .expect_err("expired fence")
                .code,
            Code::LockLeaseExpired
        );
        assert_eq!(
            authority
                .apply_fenced_write(&owner("expired"), &expired)
                .expect_err("removed expired token")
                .code,
            Code::LockNotHeld
        );

        let stale = authority
            .acquire(
                owner("stale"),
                b"stale".to_vec(),
                LockMode::Exclusive,
                5_000,
                0,
            )
            .expect("stale token");
        authority
            .release(&owner("stale"), &stale)
            .expect("release stale token");
        assert_eq!(
            authority
                .apply_fenced_write(&owner("stale"), &stale)
                .expect_err("stale fence")
                .code,
            Code::LockNotHeld
        );
    }

    #[test]
    fn canonical_locks_break_and_apply_failure_restore_exact_prior_state() {
        let persistence = Arc::new(ControlledPersistence::default());
        let authority = InProcessLocksAuthority::with_persistence(persistence.clone());
        let token = authority
            .acquire(
                owner("holder"),
                b"key".to_vec(),
                LockMode::Exclusive,
                5_000,
                0,
            )
            .expect("holder acquire");

        persistence.fail.store(true, Ordering::Relaxed);
        assert_eq!(
            authority.break_key(b"key").expect_err("failed break").code,
            Code::Io
        );
        persistence.fail.store(false, Ordering::Relaxed);
        authority
            .apply_fenced_write(&owner("holder"), &token)
            .expect("holder restored after failed break");

        let other = authority
            .acquire(
                owner("other"),
                b"other".to_vec(),
                LockMode::Exclusive,
                5_000,
                0,
            )
            .expect("other acquire");
        persistence.fail.store(true, Ordering::Relaxed);
        assert_eq!(
            authority
                .apply_fenced_write(&owner("other"), &other)
                .expect_err("failed apply")
                .code,
            Code::Io
        );
        persistence.fail.store(false, Ordering::Relaxed);
        assert_eq!(authority.break_key(b"other").expect("break other"), 1);
        let persisted = persistence
            .last
            .lock()
            .expect("persistence snapshot")
            .clone()
            .expect("persisted coordinator");
        assert_eq!(persisted.applied_fence(b"other"), None);
    }
}
