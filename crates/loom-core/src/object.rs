//! Content objects and their canonical serialization.
//!
//! Every object has exactly one canonical byte form; its [`Digest`] is the hash of that form. The
//! canonical form is Loom Canonical CBOR v1 (`loom_codec`): a positional array
//! `[epoch, type, ...fields]` that binds the object type into the hash for domain separation. There
//! are five object types: [`Object::Blob`], [`Object::ChunkList`], [`Object::Tree`],
//! [`Object::Commit`], and [`Object::Tag`]. Strict-canonical encode and decode make the model
//! round-trip: `Object::decode(obj.canonical()) == obj` and `decode(bytes).canonical() == bytes`.
//!
//! A large directory is sharded as a prolly tree without adding an object type: its shard nodes are
//! themselves `Tree` objects, and an interior shard node uses [`EntryKind::TreeShard`] entries whose
//! `name` is the maximum entry name in the child subtree and whose `target` is the child shard Tree.
//!
//! A file's *content address* is the identity-profile hash of its whole content
//! ([`content_address_with`]) and is what a Tree `Blob` entry references; it is distinct from a
//! stored object's *object address* ([`Object::digest_with`]), which hashes the canonical
//! (type-tagged) form. Chunking is a storage detail below identity, so chunk size never changes a
//! file/Tree/Commit address.

use crate::cbor::{
    Fields, Value, as_array as array, as_digest as digest_field, as_text as text_field,
    digest_value, err as cbor_err, u8_from as u8_field,
};
use crate::digest::{Algo, Digest};
use crate::error::{LoomError, Result};
use std::collections::BTreeMap;

/// The object-type tags. Discriminants are the on-disk `object_type` byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ObjectType {
    /// Opaque file content (or one chunk of it).
    Blob = 0x01,
    /// Ordered list of chunk digests composing a large file.
    ChunkList = 0x02,
    /// A directory: name -> entry map with metadata.
    Tree = 0x03,
    /// A snapshot: root tree + parents + author + message.
    Commit = 0x04,
    /// A named, optionally-signed pointer to another object.
    Tag = 0x05,
}

impl ObjectType {
    fn from_u8(b: u8) -> Result<Self> {
        Ok(match b {
            0x01 => Self::Blob,
            0x02 => Self::ChunkList,
            0x03 => Self::Tree,
            0x04 => Self::Commit,
            0x05 => Self::Tag,
            other => {
                return Err(LoomError::corrupt(format!(
                    "unknown object type {other:#x}"
                )));
            }
        })
    }
}

/// The kind of a [`TreeEntry`]. A file is [`EntryKind::Blob`] regardless of whether it is stored as
/// one Blob or a ChunkList; `ChunkList` is a storage object, never a Tree-entry kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum EntryKind {
    /// A sub-directory (its `target` is a Tree object digest).
    Tree = 0x01,
    /// A file (its `target` is the file's content address).
    Blob = 0x02,
    /// A symlink (its `target` is a Blob holding the link path).
    Symlink = 0x03,
    /// A nested Loom (its `target` is the sub-Loom's root Commit digest).
    Subloom = 0x04,
    /// An interior node of a prolly-sharded directory: `target` is a child shard `Tree` and `name`
    /// is the maximum entry name in that child's subtree. Present only inside a sharded directory's
    /// shard Trees.
    TreeShard = 0x05,
    /// A table in a workspace SQL facet: `target` is the table `Tree` whose entries are the schema Blob, the
    /// row-map prolly root, and any secondary-index prolly roots.
    Table = 0x06,
    /// An entry whose `target` is a `prolly`-tree map root (a table's row map or a secondary index),
    /// not a framed object. Present only inside a table `Tree`; walked via `prolly::reachable_nodes`.
    ProllyMap = 0x07,
    /// An append-log stream in a workspace queue facet: `target` is the stream root `Tree` whose
    /// entries are the metadata Blob and the entry-map prolly root.
    Stream = 0x08,
    /// A structured time-series collection: `target` is a Tree whose entries include collection
    /// metadata and a point-field prolly root.
    TimeSeries = 0x09,
    /// A structured graph collection: `target` is a Tree whose entries include metadata and graph
    /// component prolly roots.
    Graph = 0x0a,
    /// A structured ledger collection: `target` is a Tree whose entries include manifest, head,
    /// segment-index, and immutable segment roots.
    Ledger = 0x0b,
    /// A structured columnar dataset: `target` is a Tree whose entries include manifest metadata and
    /// durable segment payload roots.
    Columnar = 0x0c,
    /// A structured document collection: `target` is a Tree whose entries include a manifest Blob
    /// and a document-id prolly map.
    Document = 0x0d,
}

impl EntryKind {
    pub(crate) fn from_u8(b: u8) -> Result<Self> {
        Ok(match b {
            0x01 => Self::Tree,
            0x02 => Self::Blob,
            0x03 => Self::Symlink,
            0x04 => Self::Subloom,
            0x05 => Self::TreeShard,
            0x06 => Self::Table,
            0x07 => Self::ProllyMap,
            0x08 => Self::Stream,
            0x09 => Self::TimeSeries,
            0x0a => Self::Graph,
            0x0b => Self::Ledger,
            0x0c => Self::Columnar,
            0x0d => Self::Document,
            other => {
                return Err(LoomError::corrupt(format!(
                    "unknown tree-entry kind {other:#x}"
                )));
            }
        })
    }
}

/// One Tree entry. The default identity profile hashes `name`/`kind`/`target`/`mode`; volatile
/// metadata such as `mtime` is excluded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeEntry {
    /// Entry name (UTF-8, unique within the Tree, no `/` or NUL).
    pub name: String,
    /// What the entry points at.
    pub kind: EntryKind,
    /// For [`EntryKind::Blob`], the file's content address; otherwise the referenced object digest.
    pub target: Digest,
    /// POSIX-style mode bits (type + permissions).
    pub mode: u32,
}

/// One entry of a [`Object::ChunkList`]: a chunk's object digest and its byte length.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkRef {
    /// Object digest of the chunk Blob (or nested ChunkList).
    pub target: Digest,
    /// Logical byte length of this chunk.
    pub size: u64,
}

/// A commit object: a snapshot plus its lineage and authorship. Two independently authored commits
/// of the same tree differ in metadata and so have different digests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    /// Root Tree of the snapshot.
    pub tree: Digest,
    /// Parent commit digests, in order (empty for a root commit; >1 for a merge).
    pub parents: Vec<Digest>,
    /// Author identity string.
    pub author: String,
    /// Authoring time, milliseconds since the Unix epoch.
    pub timestamp_ms: u64,
    /// Commit message.
    pub message: String,
    /// Ordered metadata (e.g. `build.id` or `reviewed-by`). Encoded sorted by key.
    pub meta: BTreeMap<String, String>,
}

/// A tag object: a named, (optionally) annotated pointer to another object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    /// The tagged object's digest.
    pub target: Digest,
    /// The tagged object's type.
    pub target_type: ObjectType,
    /// Tag name (advisory; the reference store's `tag/` ref is authoritative).
    pub name: String,
    /// Tagger identity string.
    pub tagger: String,
    /// Tagging time, milliseconds since the Unix epoch.
    pub timestamp_ms: u64,
    /// Tag message.
    pub message: String,
}

/// A content object.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Object {
    /// Opaque bytes (a small file, or one chunk of a large file).
    Blob(Vec<u8>),
    /// An ordered list of chunks reassembling to a large file.
    ChunkList {
        /// Logical byte length of the assembled file.
        total_size: u64,
        /// Ordered chunk references.
        entries: Vec<ChunkRef>,
    },
    /// A directory: a name to entry map, stored in canonical (name-ascending) order. A directory too
    /// large for one Tree object is sharded as a prolly tree whose shard nodes are also `Tree`
    /// objects: interior nodes hold [`EntryKind::TreeShard`] entries, leaf nodes hold ordinary
    /// entries.
    Tree(Vec<TreeEntry>),
    /// A commit.
    Commit(Commit),
    /// A tag.
    Tag(Tag),
}

impl Object {
    /// Build a [`Object::Tree`], canonicalizing entry order (by raw-UTF-8 `name`, ascending) and
    /// rejecting duplicate names.
    pub fn tree(mut entries: Vec<TreeEntry>) -> Result<Self> {
        entries.sort_by(|a, b| a.name.as_bytes().cmp(b.name.as_bytes()));
        for w in entries.windows(2) {
            if w[0].name == w[1].name {
                return Err(LoomError::invalid(format!(
                    "duplicate tree entry name {:?}",
                    w[0].name
                )));
            }
        }
        Ok(Object::Tree(entries))
    }

    /// The object-type tag for this object.
    pub fn object_type(&self) -> ObjectType {
        match self {
            Object::Blob(_) => ObjectType::Blob,
            Object::ChunkList { .. } => ObjectType::ChunkList,
            Object::Tree(_) => ObjectType::Tree,
            Object::Commit(_) => ObjectType::Commit,
            Object::Tag(_) => ObjectType::Tag,
        }
    }

    /// Canonical serialization: Loom Canonical CBOR v1 object framing `[epoch, type, ...fields]`.
    pub fn canonical(&self) -> Vec<u8> {
        let (type_code, fields) = self.cbor_fields();
        // Objects never carry a non-finite float, and a Commit's `meta` is a `BTreeMap` (no duplicate
        // keys), so the only two encode error paths cannot occur here.
        loom_codec::encode_object(type_code, &fields)
            .expect("loom objects encode to canonical CBOR")
    }

    /// The object's type code and its positional CBOR fields (everything after `[epoch, type]`).
    fn cbor_fields(&self) -> (u16, Vec<Value>) {
        let ty = self.object_type() as u16;
        match self {
            Object::Blob(payload) => (ty, vec![Value::Bytes(payload.clone())]),
            Object::ChunkList {
                total_size,
                entries,
            } => {
                let items = entries
                    .iter()
                    .map(|e| Value::Array(vec![digest_value(&e.target), Value::Uint(e.size)]))
                    .collect();
                (ty, vec![Value::Uint(*total_size), Value::Array(items)])
            }
            Object::Tree(entries) => {
                let items = entries
                    .iter()
                    .map(|e| {
                        Value::Array(vec![
                            Value::Text(e.name.clone()),
                            Value::Uint(e.kind as u64),
                            digest_value(&e.target),
                            Value::Uint(u64::from(e.mode)),
                        ])
                    })
                    .collect();
                (ty, vec![Value::Array(items)])
            }
            Object::Commit(c) => {
                let parents = c.parents.iter().map(digest_value).collect();
                let meta = c
                    .meta
                    .iter()
                    .map(|(k, v)| (Value::Text(k.clone()), Value::Text(v.clone())))
                    .collect();
                (
                    ty,
                    vec![
                        digest_value(&c.tree),
                        Value::Array(parents),
                        Value::Text(c.author.clone()),
                        Value::Uint(c.timestamp_ms),
                        Value::Text(c.message.clone()),
                        Value::Map(meta),
                    ],
                )
            }
            Object::Tag(t) => (
                ty,
                vec![
                    digest_value(&t.target),
                    Value::Uint(t.target_type as u64),
                    Value::Text(t.name.clone()),
                    Value::Text(t.tagger.clone()),
                    Value::Uint(t.timestamp_ms),
                    Value::Text(t.message.clone()),
                ],
            ),
        }
    }

    /// The object address under an explicit identity profile: the profile's hash of
    /// the canonical (type-tagged) form. The engine passes its store's
    /// [`crate::ObjectStore::digest_algo`] so object identity uses the store profile.
    pub fn digest_with(&self, algo: Algo) -> Digest {
        Digest::hash(algo, &self.canonical())
    }

    /// The object address under the default (BLAKE3) profile. Convenience for the default profile and
    /// callers that are BLAKE3 by construction (e.g. conformance vectors, `loom hash`).
    pub fn digest(&self) -> Digest {
        self.digest_with(Algo::Blake3)
    }

    /// Parse a canonical byte form back into an [`Object`].
    ///
    /// Non-canonical encodings (trailing bytes, out-of-order Tree entries or Commit meta keys,
    /// invalid UTF-8, unknown tags) are rejected as `CORRUPT_OBJECT` so a logical object can never
    /// have two byte forms.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let (type_code, fields) = loom_codec::decode_object(bytes).map_err(cbor_err)?;
        let ty = ObjectType::from_u8(u8_field(u64::from(type_code))?)?;
        let mut f = Fields::new(fields);
        let obj = match ty {
            ObjectType::Blob => Object::Blob(f.bytes()?),
            ObjectType::ChunkList => {
                let total_size = f.uint()?;
                let mut entries = Vec::new();
                for item in f.array()? {
                    let mut e = Fields::new(array(item)?);
                    let target = e.digest()?;
                    let size = e.uint()?;
                    e.end()?;
                    entries.push(ChunkRef { target, size });
                }
                Object::ChunkList {
                    total_size,
                    entries,
                }
            }
            ObjectType::Tree => {
                let mut entries: Vec<TreeEntry> = Vec::new();
                for item in f.array()? {
                    let mut e = Fields::new(array(item)?);
                    let name = e.text()?;
                    let kind = EntryKind::from_u8(u8_field(e.uint()?)?)?;
                    let target = e.digest()?;
                    let mode = u32::try_from(e.uint()?)
                        .map_err(|_| LoomError::corrupt("tree entry mode out of range"))?;
                    e.end()?;
                    if let Some(prev) = entries.last()
                        && prev.name.as_bytes() >= name.as_bytes()
                    {
                        return Err(LoomError::corrupt(
                            "tree entries not in canonical ascending order",
                        ));
                    }
                    entries.push(TreeEntry {
                        name,
                        kind,
                        target,
                        mode,
                    });
                }
                Object::Tree(entries)
            }
            ObjectType::Commit => {
                let tree = f.digest()?;
                let mut parents = Vec::new();
                for p in f.array()? {
                    parents.push(digest_field(p)?);
                }
                let author = f.text()?;
                let timestamp_ms = f.uint()?;
                let message = f.text()?;
                // The codec already enforced ascending, duplicate-free map keys.
                let mut meta = BTreeMap::new();
                for (k, v) in f.map()? {
                    meta.insert(text_field(k)?, text_field(v)?);
                }
                Object::Commit(Commit {
                    tree,
                    parents,
                    author,
                    timestamp_ms,
                    message,
                    meta,
                })
            }
            ObjectType::Tag => {
                let target = f.digest()?;
                let target_type = ObjectType::from_u8(u8_field(f.uint()?)?)?;
                let name = f.text()?;
                let tagger = f.text()?;
                let timestamp_ms = f.uint()?;
                let message = f.text()?;
                Object::Tag(Tag {
                    target,
                    target_type,
                    name,
                    tagger,
                    timestamp_ms,
                    message,
                })
            }
        };
        f.end()?;
        Ok(obj)
    }
}

/// The content address of file content under an explicit identity profile: the
/// profile's hash of the whole content, independent of chunking. This is what a Tree
/// [`EntryKind::Blob`] entry references; it differs from the Blob *object's* address
/// ([`Object::digest_with`]), which hashes the canonical (type-tagged) form. The engine passes its
/// store's [`crate::ObjectStore::digest_algo`] so file-content addresses use the store profile.
pub fn content_address_with(algo: Algo, content: &[u8]) -> Digest {
    Digest::hash(algo, content)
}

/// [`content_address_with`] under the default (BLAKE3) profile - for the default profile and callers
/// that are BLAKE3 by construction (conformance vectors).
pub fn content_address(content: &[u8]) -> Digest {
    content_address_with(Algo::Blake3, content)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Digest {
        Digest::blake3(s.as_bytes())
    }

    #[test]
    fn blob_canonical_framing() {
        // Empty blob: CBOR array [epoch=1, type=1, bytes("")] = 83 01 01 40.
        assert_eq!(
            Object::Blob(vec![]).canonical(),
            vec![0x83, 0x01, 0x01, 0x40]
        );
        // 3-byte blob "abc": [1, 1, bytes("abc")] = 83 01 01 43 'a' 'b' 'c'.
        assert_eq!(
            Object::Blob(b"abc".to_vec()).canonical(),
            vec![0x83, 0x01, 0x01, 0x43, b'a', b'b', b'c']
        );
    }

    #[test]
    fn blob_large_length_header() {
        // 200-byte blob: [1, 1, bytes(200)]; the byte-string header is 0x58 0xC8 (major 2, 1-byte len).
        let c = Object::Blob(vec![0u8; 200]).canonical();
        assert_eq!(&c[..5], &[0x83, 0x01, 0x01, 0x58, 0xC8]);
        assert_eq!(c.len(), 5 + 200);
    }

    #[test]
    fn digest_is_deterministic_and_type_tagged() {
        let a = Object::Blob(b"abc".to_vec()).digest();
        let b = Object::Blob(b"abc".to_vec()).digest();
        assert_eq!(a, b);
        // The blob object digest is over canonical bytes (with the type tag), so it differs from the
        // raw blake3 content address of "abc".
        assert_ne!(a, content_address(b"abc"));
    }

    #[test]
    fn content_address_is_raw_blake3() {
        assert_eq!(content_address(b"abc"), Digest::blake3(b"abc"));
    }

    fn sample_tree() -> Object {
        // Intentionally out of order to exercise canonicalization.
        Object::tree(vec![
            TreeEntry {
                name: "src".into(),
                kind: EntryKind::Tree,
                target: d("src-tree"),
                mode: 0o040000,
            },
            TreeEntry {
                name: "README.md".into(),
                kind: EntryKind::Blob,
                target: content_address(b"# loom"),
                mode: 0o100644,
            },
        ])
        .unwrap()
    }

    fn sample_commit() -> Object {
        let mut meta = BTreeMap::new();
        meta.insert("build.id".into(), "deadbeef".into());
        Object::Commit(Commit {
            tree: sample_tree().digest(),
            parents: vec![d("parent-a"), d("parent-b")],
            author: "Nas <nas@jarwin.xyz>".into(),
            timestamp_ms: 1_700_000_000_000,
            message: "init".into(),
            meta,
        })
    }

    fn sample_tag() -> Object {
        Object::Tag(Tag {
            target: sample_commit().digest(),
            target_type: ObjectType::Commit,
            name: "v1.0.0".into(),
            tagger: "Nas <nas@jarwin.xyz>".into(),
            timestamp_ms: 1_700_000_000_001,
            message: "release".into(),
        })
    }

    fn sample_chunklist() -> Object {
        Object::ChunkList {
            total_size: 3 * 64 * 1024,
            entries: vec![
                ChunkRef {
                    target: d("chunk-0"),
                    size: 64 * 1024,
                },
                ChunkRef {
                    target: d("chunk-1"),
                    size: 64 * 1024,
                },
                ChunkRef {
                    target: d("chunk-2"),
                    size: 64 * 1024,
                },
            ],
        }
    }

    #[test]
    fn roundtrip_all_object_types() {
        for obj in [
            Object::Blob(b"hello loom".to_vec()),
            sample_chunklist(),
            sample_tree(),
            sample_commit(),
            sample_tag(),
        ] {
            let bytes = obj.canonical();
            let back = Object::decode(&bytes).expect("decode");
            assert_eq!(back, obj, "decode(canonical(obj)) must equal obj");
            assert_eq!(
                back.canonical(),
                bytes,
                "canonical(decode(bytes)) must equal bytes"
            );
            assert_eq!(back.digest(), obj.digest());
        }
    }

    #[test]
    fn tree_entries_are_canonically_ordered() {
        // Build order does not matter; canonical form sorts by name.
        let t = sample_tree();
        let Object::Tree(entries) = &t else {
            panic!("expected tree")
        };
        assert_eq!(entries[0].name, "README.md");
        assert_eq!(entries[1].name, "src");
    }

    #[test]
    fn tree_rejects_duplicate_names() {
        let dup = Object::tree(vec![
            TreeEntry {
                name: "a".into(),
                kind: EntryKind::Blob,
                target: d("x"),
                mode: 0o100644,
            },
            TreeEntry {
                name: "a".into(),
                kind: EntryKind::Blob,
                target: d("y"),
                mode: 0o100644,
            },
        ]);
        assert!(dup.is_err());
    }

    #[test]
    fn decode_rejects_trailing_bytes() {
        let mut bytes = Object::Blob(b"abc".to_vec()).canonical();
        bytes.push(0xFF);
        assert!(Object::decode(&bytes).is_err());
    }

    #[test]
    fn decode_rejects_out_of_order_tree() {
        // A Tree whose entries are in descending name order is not canonical.
        let entry = |name: &str| {
            Value::Array(vec![
                Value::Text(name.into()),
                Value::Uint(EntryKind::Blob as u64),
                Value::Bytes(d("t").bytes().to_vec()),
                Value::Uint(0o100644),
            ])
        };
        let bytes = loom_codec::encode_object(
            ObjectType::Tree as u16,
            &[Value::Array(vec![entry("b"), entry("a")])],
        )
        .unwrap();
        assert!(Object::decode(&bytes).is_err());
    }

    #[test]
    fn decode_rejects_unknown_type() {
        // Valid object framing, unknown type code.
        let bytes = loom_codec::encode_object(0x7f, &[]).unwrap();
        assert!(Object::decode(&bytes).is_err());
    }
}
