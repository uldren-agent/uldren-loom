//! Behavioral conformance for the `Document` text/binary surface: a canonical `list_binary` CBOR
//! vector (`[id, doc]` pairs) plus a put/get/list round-trip over a [`MemoryStore`].

use loom_core::{
    FacetKind, Loom, MemoryStore, Result, WorkspaceId, document_get_binary, document_get_text,
    document_list_binary, document_put_binary, document_put_text,
};

const COLLECTION: &str = "notes";

pub struct DocumentBlobCanonicalVector {
    pub name: &'static str,
    /// Canonical CBOR for `list_binary("notes")` after the two fixed writes below.
    pub list_binary: &'static str,
}

/// Pinned canonical `list_binary` CBOR for the fixture. Every backend must reproduce these bytes.
pub const DOCUMENT_BLOB_CANONICAL_VECTOR: DocumentBlobCanonicalVector =
    DocumentBlobCanonicalVector {
        name: "two-notes",
        list_binary: "828261614568656c6c6f82616245776f726c64",
    };

pub fn run_document_blob_behavior() -> Result<()> {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = loom.registry_mut().create(
        FacetKind::Document,
        None,
        WorkspaceId::from_bytes([0x64; 16]),
    )?;

    // put_text then get_text round-trips, and get_binary sees the same bytes and a stable digest.
    let text_digest = document_put_text(&mut loom, ns, COLLECTION, "a", "hello", None)?;
    let got = document_get_text(&loom, ns, COLLECTION, "a")?.expect("text present");
    assert_eq!(got.text, "hello");
    assert_eq!(got.digest, text_digest);
    let bin = document_get_binary(&loom, ns, COLLECTION, "a")?.expect("binary present");
    assert_eq!(bin.bytes, b"hello");
    assert_eq!(bin.digest, text_digest);

    // put_binary at a second id.
    document_put_binary(&mut loom, ns, COLLECTION, "b", b"world".to_vec(), None)?;

    // Absent id is None.
    assert_eq!(document_get_binary(&loom, ns, COLLECTION, "missing")?, None);

    // list_binary is the canonical CBOR of the id→doc map; pinned.
    assert_eq!(
        hex::encode(document_list_binary(&loom, ns, COLLECTION)?),
        DOCUMENT_BLOB_CANONICAL_VECTOR.list_binary,
        "document list_binary canonical bytes mismatch for '{}'",
        DOCUMENT_BLOB_CANONICAL_VECTOR.name
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_blob_behavior_passes() {
        run_document_blob_behavior().expect("document text/binary behavior must pass");
    }
}
