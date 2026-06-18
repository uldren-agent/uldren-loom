//! Integration tests against the public `loom-core` surface.

use loom_core::{Digest, MemoryStore, Object, ObjectStore};

#[test]
fn end_to_end_store_a_blob() {
    let store = MemoryStore::new();
    let obj = Object::Blob(b"the quick brown fox".to_vec());

    let digest = store.put(&obj.canonical()).unwrap();

    // The address is the object's content digest, round-trips through text form, and fetches back.
    assert_eq!(digest, obj.digest());
    assert_eq!(Digest::parse(&digest.to_string()).unwrap(), digest);

    let fetched = store.get(&digest).unwrap().expect("object present");
    assert_eq!(fetched, obj.canonical());
}

#[test]
fn distinct_content_yields_distinct_digests() {
    let a = Object::Blob(b"a".to_vec()).digest();
    let b = Object::Blob(b"b".to_vec()).digest();
    assert_ne!(a, b);
}
