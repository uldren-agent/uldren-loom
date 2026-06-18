//! The provider contract - the low-level store the engine builds on.
//!
//! This trait is synchronous; the asynchronous engine and binding APIs wrap it.

pub mod memory;

use crate::digest::{Algo, Digest};
use crate::error::Result;

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
}
