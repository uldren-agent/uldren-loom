//! Canonical wire codecs for the open-file-handle facet, shared by the C ABI, the in-process client
//! service impl, and the server dispatch. The `open` mode crosses as exactly one byte - the
//! [`OpenMode`] wire tag (see [`OpenMode::to_wire_tag`]); a `stat` crosses as the CBOR array
//! `[size, mode]`.

use loom_codec::{Value as CborValue, decode, encode};
use loom_core::{DirEntry, FileKind, FileStat, OpenMode, Stat};
use loom_types::{Code, LoomError};

/// The wire tag for a [`FileKind`]: 0 file, 1 directory, 2 symlink.
fn file_kind_tag(kind: FileKind) -> u64 {
    match kind {
        FileKind::File => 0,
        FileKind::Directory => 1,
        FileKind::Symlink => 2,
    }
}

/// Decode a [`FileKind`] wire tag; an unknown tag is `CORRUPT_OBJECT`.
fn file_kind_from_tag(tag: u64) -> Result<FileKind, LoomError> {
    match tag {
        0 => Ok(FileKind::File),
        1 => Ok(FileKind::Directory),
        2 => Ok(FileKind::Symlink),
        other => Err(corrupt(format!("unknown file kind tag {other}"))),
    }
}

fn corrupt(message: impl Into<String>) -> LoomError {
    LoomError::new(Code::CorruptObject, message.into())
}

/// Encode a filesystem [`Stat`] as canonical CBOR `loom.fs.stat.v1`: `[path, kind, size, mode]`.
pub fn fs_stat_to_cbor(stat: &Stat) -> Result<Vec<u8>, LoomError> {
    encode(&CborValue::Array(vec![
        CborValue::Text(stat.path.clone()),
        CborValue::Uint(file_kind_tag(stat.kind)),
        CborValue::Uint(stat.size),
        CborValue::Uint(u64::from(stat.mode)),
    ]))
    .map_err(|err| corrupt(format!("cbor: {err}")))
}

/// Decode a filesystem [`Stat`] from canonical CBOR `loom.fs.stat.v1`.
pub fn fs_stat_from_cbor(bytes: &[u8]) -> Result<Stat, LoomError> {
    let value = decode(bytes).map_err(|err| corrupt(format!("cbor: {err}")))?;
    let CborValue::Array(items) = value else {
        return Err(corrupt("fs stat must be a CBOR array"));
    };
    let [
        CborValue::Text(path),
        CborValue::Uint(kind),
        CborValue::Uint(size),
        CborValue::Uint(mode),
    ] = items.as_slice()
    else {
        return Err(corrupt("fs stat must be [path, kind, size, mode]"));
    };
    Ok(Stat {
        path: path.clone(),
        kind: file_kind_from_tag(*kind)?,
        size: *size,
        mode: u32::try_from(*mode).map_err(|_| corrupt("fs stat mode out of range"))?,
    })
}

/// Encode a directory listing as canonical CBOR `loom.fs.dir-listing.v1`: an array of `[name, kind]`.
pub fn dir_listing_to_cbor(entries: &[DirEntry]) -> Result<Vec<u8>, LoomError> {
    let items = entries
        .iter()
        .map(|entry| {
            CborValue::Array(vec![
                CborValue::Text(entry.name.clone()),
                CborValue::Uint(file_kind_tag(entry.kind)),
            ])
        })
        .collect();
    encode(&CborValue::Array(items)).map_err(|err| corrupt(format!("cbor: {err}")))
}

/// Decode a directory listing from canonical CBOR `loom.fs.dir-listing.v1`.
pub fn dir_listing_from_cbor(bytes: &[u8]) -> Result<Vec<DirEntry>, LoomError> {
    let value = decode(bytes).map_err(|err| corrupt(format!("cbor: {err}")))?;
    let CborValue::Array(items) = value else {
        return Err(corrupt("dir listing must be a CBOR array"));
    };
    items
        .into_iter()
        .map(|item| {
            let CborValue::Array(fields) = item else {
                return Err(corrupt("dir entry must be a CBOR array"));
            };
            let [CborValue::Text(name), CborValue::Uint(kind)] = fields.as_slice() else {
                return Err(corrupt("dir entry must be [name, kind]"));
            };
            Ok(DirEntry {
                name: name.clone(),
                kind: file_kind_from_tag(*kind)?,
            })
        })
        .collect()
}

/// Decode the `FileHandle.open` `mode` argument: exactly one byte carrying the [`OpenMode`] wire tag.
/// An empty or multi-byte buffer, or an unknown tag, is `INVALID_ARGUMENT`.
pub fn open_mode_from_wire(bytes: &[u8]) -> Result<OpenMode, LoomError> {
    match bytes {
        [tag] => OpenMode::from_wire_tag(*tag),
        _ => Err(LoomError::invalid(
            "file open mode must be exactly one byte",
        )),
    }
}

/// Encode an open-handle stat as the CBOR array `[size, mode]`.
pub fn file_stat_to_cbor(stat: FileStat) -> Result<Vec<u8>, LoomError> {
    encode(&CborValue::Array(vec![
        CborValue::Uint(stat.size),
        CborValue::Uint(u64::from(stat.mode)),
    ]))
    .map_err(|err| LoomError::new(Code::CorruptObject, format!("cbor: {err}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_mode_tags_round_trip() {
        for tag in 0u8..=3 {
            let mode = open_mode_from_wire(&[tag]).unwrap();
            assert_eq!(mode.to_wire_tag(), tag);
        }
    }

    #[test]
    fn unknown_open_mode_tag_is_invalid_argument() {
        let err = open_mode_from_wire(&[4]).unwrap_err();
        assert_eq!(err.code, loom_types::Code::InvalidArgument);
    }

    #[test]
    fn empty_open_mode_is_invalid_argument() {
        let err = open_mode_from_wire(&[]).unwrap_err();
        assert_eq!(err.code, loom_types::Code::InvalidArgument);
    }

    #[test]
    fn multi_byte_open_mode_is_invalid_argument() {
        let err = open_mode_from_wire(&[0, 1]).unwrap_err();
        assert_eq!(err.code, loom_types::Code::InvalidArgument);
    }

    #[test]
    fn fs_stat_round_trips_every_kind() {
        for (kind, mode) in [
            (FileKind::File, 0o100644u32),
            (FileKind::Directory, 0o040000),
            (FileKind::Symlink, 0o120000),
        ] {
            let stat = Stat {
                path: "a/b/c".to_string(),
                kind,
                size: 1234,
                mode,
            };
            let bytes = fs_stat_to_cbor(&stat).unwrap();
            assert_eq!(fs_stat_from_cbor(&bytes).unwrap(), stat);
        }
    }

    #[test]
    fn dir_listing_round_trips_and_rejects_bad_tag() {
        let entries = vec![
            DirEntry {
                name: "dir".to_string(),
                kind: FileKind::Directory,
            },
            DirEntry {
                name: "file.txt".to_string(),
                kind: FileKind::File,
            },
            DirEntry {
                name: "link".to_string(),
                kind: FileKind::Symlink,
            },
        ];
        let bytes = dir_listing_to_cbor(&entries).unwrap();
        assert_eq!(dir_listing_from_cbor(&bytes).unwrap(), entries);

        let bad = encode(&CborValue::Array(vec![CborValue::Array(vec![
            CborValue::Text("x".to_string()),
            CborValue::Uint(9),
        ])]))
        .unwrap();
        assert_eq!(
            dir_listing_from_cbor(&bad).unwrap_err().code,
            Code::CorruptObject
        );
    }

    #[test]
    fn file_stat_encodes_as_size_mode_array() {
        let bytes = file_stat_to_cbor(FileStat {
            size: 42,
            mode: 0o100644,
        })
        .unwrap();
        let CborValue::Array(items) = loom_codec::decode(&bytes).unwrap() else {
            panic!("expected array");
        };
        assert_eq!(items.len(), 2);
        assert_eq!(items[0], CborValue::Uint(42));
        assert_eq!(items[1], CborValue::Uint(0o100644));
    }
}
