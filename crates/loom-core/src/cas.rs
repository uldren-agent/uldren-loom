//! The content-addressed store facet: put opaque bytes, get them back by their content [`Digest`].
//! It surfaces the object model's blob store as a facade.
//!
//! Pure-Rust, `wasm32`-clean: it reuses the engine's content-addressed file machinery (chunking,
//! versioning, sync, GC reachability) with **no new on-disk type and no derived index**.
//!
//! Each blob is written into the workspace's `cas` facet tree, so:
//! - the address **is** the content hash (`content_address`), giving integrity-by-construction and
//!   global dedup (identical bytes -> identical path/content);
//! - the tree itself is the reachable-digest manifest, so `commit`/`branch`/`checkout`/`sync` version
//!   the *set of reachable blobs* for free, isolated per workspace;
//! - nothing is derived, so nothing is ever rebuilt.

use crate::acl::AclRight;
use crate::digest::Digest;
use crate::error::{Code, LoomError, Result};
use crate::object::content_address_with;
use crate::provider::ObjectStore;
use crate::vcs::Loom;
use crate::workspace::{FacetKind, WorkspaceId, facet_path, facet_root};

fn blob_path(digest: &Digest) -> String {
    facet_path(FacetKind::Cas, &digest.to_hex())
}

/// Store `bytes` in the `cas` workspace `ns` and return their content address.
/// Idempotent: putting identical bytes yields the same [`Digest`] and the same stored path (dedup).
pub fn cas_put<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    bytes: &[u8],
) -> Result<Digest> {
    let digest = content_address_with(loom.store().digest_algo(), bytes);
    loom.authorize_facet_path(ns, FacetKind::Cas, &digest.to_hex(), AclRight::Write)?;
    cas_put_digest_unchecked(loom, ns, bytes, digest)
}

pub(crate) fn cas_put_unchecked<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    bytes: &[u8],
) -> Result<Digest> {
    let digest = content_address_with(loom.store().digest_algo(), bytes);
    cas_put_digest_unchecked(loom, ns, bytes, digest)
}

fn cas_put_digest_unchecked<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    bytes: &[u8],
    digest: Digest,
) -> Result<Digest> {
    loom.create_directory_reserved(ns, &facet_root(FacetKind::Cas), true)?;
    loom.write_file_reserved(ns, &blob_path(&digest), bytes, 0o100644)?;
    Ok(digest)
}

/// Fetch the blob addressed by `digest` from `ns`, or `None` if absent. Every read **verifies the
/// digest**; a mismatch is `INTEGRITY_FAILURE`.
pub fn cas_get<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    digest: &Digest,
) -> Result<Option<Vec<u8>>> {
    loom.authorize_facet_path(ns, FacetKind::Cas, &digest.to_hex(), AclRight::Read)?;
    cas_get_unchecked(loom, ns, digest)
}

pub(crate) fn cas_get_unchecked<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    digest: &Digest,
) -> Result<Option<Vec<u8>>> {
    match loom.read_file_reserved(ns, &blob_path(digest)) {
        Ok(bytes) => {
            let actual = content_address_with(loom.store().digest_algo(), &bytes);
            if &actual != digest {
                return Err(LoomError::integrity_failure(format!(
                    "cas blob {digest} hashes to {actual}"
                )));
            }
            Ok(Some(bytes))
        }
        Err(e) if e.code == Code::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Whether a blob addressed by `digest` is present in `ns`.
pub fn cas_has<S: ObjectStore>(loom: &Loom<S>, ns: WorkspaceId, digest: &Digest) -> Result<bool> {
    loom.authorize_facet_path(ns, FacetKind::Cas, &digest.to_hex(), AclRight::Read)?;
    let path = blob_path(digest);
    Ok(loom.staged_paths(ns).iter().any(|p| p == &path))
}

/// Drop the blob addressed by `digest` from `ns`'s current working tree, making it unreachable going
/// forward; returns whether it was present. CAS stays immutable: this unlinks a reference, it does not
/// mutate content. The bytes are reclaimed by GC once no commit, branch, or other workspace still
/// references them (a digest dropped here is restored by checking out an earlier commit that held it).
/// Removing an absent digest is a no-op (`Ok(false)`).
pub fn cas_delete<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    digest: &Digest,
) -> Result<bool> {
    loom.authorize_facet_path(ns, FacetKind::Cas, &digest.to_hex(), AclRight::Write)?;
    let path = blob_path(digest);
    let present = loom.staged_paths(ns).iter().any(|p| p == &path);
    if present {
        loom.remove_file_reserved(ns, &path)?;
    }
    Ok(present)
}

/// The digests reachable in `ns`'s current working tree, sorted. Enumeration is *within* the
/// workspace, not a global index; a malformed `cas` facet path that is
/// not a valid digest is skipped.
pub fn cas_list<S: ObjectStore>(loom: &Loom<S>, ns: WorkspaceId) -> Result<Vec<Digest>> {
    loom.authorize_collection(ns, FacetKind::Cas, "", AclRight::Read)?;
    let prefix = format!("{}/", facet_root(FacetKind::Cas));
    let algo = loom.store().digest_algo();
    let mut out: Vec<Digest> = loom
        .staged_paths(ns)
        .into_iter()
        .filter_map(|p| {
            // `blob_path` names files by bare hex (`to_hex`), so rebuild the digest under the store's
            // identity profile (the address algorithm is fixed per store).
            let hex = p.strip_prefix(&prefix)?;
            let raw = hex::decode(hex).ok()?;
            let bytes: [u8; 32] = raw.as_slice().try_into().ok()?;
            Some(Digest::of(algo, bytes))
        })
        .collect();
    out.sort_by_key(|d| d.to_hex());
    out.dedup();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acl::{AclRight, AclSubject};
    use crate::error::Code;
    use crate::identity::IdentityStore;
    use crate::provider::memory::MemoryStore;
    use crate::workspace::{FacetKind, WorkspaceId};

    fn cas_ns() -> (Loom<MemoryStore>, WorkspaceId) {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Cas, None, WorkspaceId::from_bytes([9; 16]))
            .unwrap();
        (loom, ns)
    }

    #[test]
    fn put_get_has_round_trip_and_dedup() {
        let (mut loom, ns) = cas_ns();
        let d1 = cas_put(&mut loom, ns, b"hello cas").unwrap();
        // Address is the content hash; get returns the exact bytes.
        assert_eq!(
            cas_get(&loom, ns, &d1).unwrap().as_deref(),
            Some(&b"hello cas"[..])
        );
        assert!(cas_has(&loom, ns, &d1).unwrap());
        // Putting identical bytes is idempotent: same digest, still one entry (dedup, G3).
        let d2 = cas_put(&mut loom, ns, b"hello cas").unwrap();
        assert_eq!(d1, d2);
        assert_eq!(cas_list(&loom, ns).unwrap(), vec![d1]);
        // A digest never put is absent.
        let other = content_address_with(loom.store().digest_algo(), b"not stored");
        assert!(!cas_has(&loom, ns, &other).unwrap());
        assert_eq!(cas_get(&loom, ns, &other).unwrap(), None);
    }

    #[test]
    fn blobs_version_with_commits() {
        let (mut loom, ns) = cas_ns();
        let a = cas_put(&mut loom, ns, b"alpha").unwrap();
        let c1 = loom.commit(ns, "nas", "one blob", 1).unwrap();

        let b = cas_put(&mut loom, ns, b"beta").unwrap();
        loom.commit(ns, "nas", "two blobs", 2).unwrap();
        assert_eq!(cas_list(&loom, ns).unwrap().len(), 2);
        assert!(cas_has(&loom, ns, &a).unwrap() && cas_has(&loom, ns, &b).unwrap());

        // Checking out the first commit restores the reachable set (one blob); 'beta' is gone, 'alpha'
        // still resolves to identical bytes because its address is its content hash.
        loom.checkout_commit(ns, c1).unwrap();
        assert_eq!(cas_list(&loom, ns).unwrap(), vec![a]);
        assert_eq!(
            cas_get(&loom, ns, &a).unwrap().as_deref(),
            Some(&b"alpha"[..])
        );
        assert!(!cas_has(&loom, ns, &b).unwrap());
    }

    #[test]
    fn delete_unreferences_then_checkout_restores() {
        let (mut loom, ns) = cas_ns();
        let a = cas_put(&mut loom, ns, b"alpha").unwrap();
        let c1 = loom.commit(ns, "nas", "one blob", 1).unwrap();

        // Delete drops the reference from the current tree; it reports presence and is then a no-op.
        assert!(cas_delete(&mut loom, ns, &a).unwrap());
        assert!(!cas_has(&loom, ns, &a).unwrap());
        assert_eq!(cas_get(&loom, ns, &a).unwrap(), None);
        assert!(cas_list(&loom, ns).unwrap().is_empty());
        assert!(!cas_delete(&mut loom, ns, &a).unwrap());

        // CAS stays immutable: checking out the commit that held the blob restores it byte-for-byte.
        loom.checkout_commit(ns, c1).unwrap();
        assert!(cas_has(&loom, ns, &a).unwrap());
        assert_eq!(
            cas_get(&loom, ns, &a).unwrap().as_deref(),
            Some(&b"alpha"[..])
        );
    }

    #[test]
    fn empty_blob_is_addressable() {
        let (mut loom, ns) = cas_ns();
        let d = cas_put(&mut loom, ns, b"").unwrap();
        assert_eq!(cas_get(&loom, ns, &d).unwrap().as_deref(), Some(&b""[..]));
    }

    #[test]
    fn authenticated_cas_operations_are_acl_checked() {
        let (mut loom, ns) = cas_ns();
        let root = WorkspaceId::from_bytes([1; 16]);
        let mut identity = IdentityStore::new(root);
        identity.set_passphrase(root, "root", b"12345678").unwrap();
        let session = identity
            .authenticate_passphrase(root, "root", "session")
            .unwrap();
        loom.set_identity_store(identity);
        loom.set_session(session.id);

        assert_eq!(
            cas_put(&mut loom, ns, b"secret").unwrap_err().code,
            Code::PermissionDenied
        );

        loom.acl_store_mut()
            .allow(
                AclSubject::Principal(root),
                Some(ns),
                Some(FacetKind::Cas),
                [AclRight::Write],
            )
            .unwrap();
        let digest = cas_put(&mut loom, ns, b"secret").unwrap();
        assert_eq!(
            cas_get(&loom, ns, &digest).unwrap_err().code,
            Code::PermissionDenied
        );
        assert_eq!(
            cas_has(&loom, ns, &digest).unwrap_err().code,
            Code::PermissionDenied
        );
        assert_eq!(
            cas_list(&loom, ns).unwrap_err().code,
            Code::PermissionDenied
        );

        loom.acl_store_mut()
            .allow(
                AclSubject::Principal(root),
                Some(ns),
                Some(FacetKind::Cas),
                [AclRight::Read],
            )
            .unwrap();
        assert_eq!(
            cas_get(&loom, ns, &digest).unwrap().as_deref(),
            Some(&b"secret"[..])
        );
        assert!(cas_has(&loom, ns, &digest).unwrap());
        assert_eq!(cas_list(&loom, ns).unwrap(), vec![digest]);
        assert!(cas_delete(&mut loom, ns, &digest).unwrap());
    }
}
