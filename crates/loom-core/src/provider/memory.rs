//! In-memory object store, used as the reference implementation for differential testing.
//!
//! Backed by a `BTreeMap` keyed by digest bytes, so iteration order is deterministic.

use std::collections::BTreeMap;
use std::sync::Mutex;

use super::ObjectStore;
use crate::digest::Digest;
use crate::error::Result;

/// A simple, deterministic in-memory [`ObjectStore`]. Interior mutability via a `Mutex`, so the store
/// is `Send + Sync` and shareable across threads - matching the [`ObjectStore`] contract (and letting a
/// type-erased `Arc<dyn ObjectStore + Send + Sync>` wrap it, e.g. the lazy SQL base snapshot). A clone
/// is an independent copy (the data is cloned, not the lock).
#[derive(Debug, Default)]
pub struct MemoryStore {
    objects: Mutex<BTreeMap<[u8; crate::digest::DIGEST_LEN], Vec<u8>>>,
}

impl Clone for MemoryStore {
    fn clone(&self) -> Self {
        Self {
            objects: Mutex::new(self.objects.lock().expect("memory store lock").clone()),
        }
    }
}

impl MemoryStore {
    /// Create an empty store.
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(
        &self,
    ) -> std::sync::MutexGuard<'_, BTreeMap<[u8; crate::digest::DIGEST_LEN], Vec<u8>>> {
        self.objects.lock().expect("memory store lock")
    }
}

impl ObjectStore for MemoryStore {
    fn put(&self, canonical: &[u8]) -> Result<Digest> {
        let digest = Digest::blake3(canonical);
        self.lock()
            .entry(*digest.bytes())
            .or_insert_with(|| canonical.to_vec());
        Ok(digest)
    }

    fn get(&self, digest: &Digest) -> Result<Option<Vec<u8>>> {
        Ok(self.lock().get(digest.bytes()).cloned())
    }

    fn has(&self, digest: &Digest) -> Result<bool> {
        Ok(self.lock().contains_key(digest.bytes()))
    }

    fn len(&self) -> usize {
        self.lock().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::Object;

    #[test]
    fn put_get_has_roundtrip() {
        let store = MemoryStore::new();
        assert!(store.is_empty());

        let obj = Object::Blob(b"hello loom".to_vec());
        let canonical = obj.canonical();
        let digest = store.put(&canonical).unwrap();

        assert_eq!(digest, obj.digest());
        assert!(store.has(&digest).unwrap());
        assert_eq!(
            store.get(&digest).unwrap().as_deref(),
            Some(canonical.as_slice())
        );
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn put_is_idempotent() {
        let store = MemoryStore::new();
        let canonical = Object::Blob(b"dup".to_vec()).canonical();
        let d1 = store.put(&canonical).unwrap();
        let d2 = store.put(&canonical).unwrap();
        assert_eq!(d1, d2);
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn missing_object_returns_none() {
        let store = MemoryStore::new();
        let absent = Object::Blob(b"absent".to_vec()).digest();
        assert!(!store.has(&absent).unwrap());
        assert_eq!(store.get(&absent).unwrap(), None);
    }
}
