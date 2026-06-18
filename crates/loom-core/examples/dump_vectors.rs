use loom_core::Digest;
use loom_core::object::{Commit, EntryKind, Object, ObjectType, Tag, TreeEntry, content_address};
use std::collections::BTreeMap;

fn d(s: &str) -> Digest {
    Digest::blake3(s.as_bytes())
}

fn main() {
    let empty_tree = Object::tree(vec![]).unwrap();
    let empty_cl = Object::ChunkList {
        total_size: 0,
        entries: vec![],
    };
    println!("EMPTY_TREE {}", empty_tree.digest());
    println!("EMPTY_CHUNKLIST {}", empty_cl.digest());

    // A fully-specified sample tree (fixed inputs => fixed digest).
    let sample_tree = Object::tree(vec![
        TreeEntry {
            name: "README.md".into(),
            kind: EntryKind::Blob,
            target: content_address(b"# loom\n"),
            mode: 0o100644,
        },
        TreeEntry {
            name: "src".into(),
            kind: EntryKind::Tree,
            target: empty_tree.digest(),
            mode: 0o040000,
        },
    ])
    .unwrap();
    println!("SAMPLE_TREE {}", sample_tree.digest());

    let mut meta = BTreeMap::new();
    meta.insert("build.id".to_string(), "deadbeef".to_string());
    let commit = Object::Commit(Commit {
        tree: sample_tree.digest(),
        parents: vec![d("parent")],
        author: "Nas <nas@jarwin.xyz>".into(),
        timestamp_ms: 1_700_000_000_000,
        message: "init".into(),
        meta,
    });
    println!("SAMPLE_COMMIT {}", commit.digest());

    let tag = Object::Tag(Tag {
        target: commit.digest(),
        target_type: ObjectType::Commit,
        name: "v1.0.0".into(),
        tagger: "Nas <nas@jarwin.xyz>".into(),
        timestamp_ms: 1_700_000_000_001,
        message: "release".into(),
    });
    println!("SAMPLE_TAG {}", tag.digest());
}
