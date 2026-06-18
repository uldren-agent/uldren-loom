//! Behavioral conformance for the `FileSystem` directory/metadata surface: canonical
//! `loom.fs.stat.v1` / `loom.fs.dir-listing.v1` CBOR vectors plus an engine
//! create_directory/stat/list_directory/remove_directory round-trip over a [`MemoryStore`].

use loom_core::{FacetKind, FileKind, Loom, MemoryStore, Result, WorkspaceId};
use loom_wire::fs::{dir_listing_to_cbor, fs_stat_to_cbor};

pub struct FsCanonicalVectors {
    /// Canonical CBOR for the file `docs/readme.txt` (`[path, kind, size, mode]`).
    pub file_stat: &'static str,
    /// Canonical CBOR for the directory `docs` (`[path, kind, size, mode]`).
    pub dir_stat: &'static str,
    /// Canonical CBOR for `list_directory("docs")` (`[[name, kind]]`).
    pub dir_listing: &'static str,
}

/// Pinned canonical CBOR for the fixed fixture below. Every backend must reproduce these bytes.
pub const FS_CANONICAL_VECTORS: FsCanonicalVectors = FsCanonicalVectors {
    file_stat: "846f646f63732f726561646d652e74787400051981a4",
    dir_stat: "8464646f63730100194000",
    dir_listing: "81826a726561646d652e74787400",
};

pub fn run_fsdir_behavior() -> Result<()> {
    let mut loom = Loom::new(MemoryStore::new());
    let ns =
        loom.registry_mut()
            .create(FacetKind::Files, None, WorkspaceId::from_bytes([0x66; 16]))?;

    // create_directory records `docs` as a directory; stat reports it.
    loom.create_directory(ns, "docs", false)?;
    let dir_stat = loom.stat(ns, "docs")?;
    assert_eq!(dir_stat.kind, FileKind::Directory);
    assert_eq!(dir_stat.size, 0);
    assert_eq!(
        hex::encode(fs_stat_to_cbor(&dir_stat)?),
        FS_CANONICAL_VECTORS.dir_stat,
        "directory stat canonical bytes mismatch"
    );

    // Write a file inside; stat reports its size and mode.
    loom.write_file(ns, "docs/readme.txt", b"hello", 0o100644)?;
    let file_stat = loom.stat(ns, "docs/readme.txt")?;
    assert_eq!(file_stat.kind, FileKind::File);
    assert_eq!(file_stat.size, 5);
    assert_eq!(
        hex::encode(fs_stat_to_cbor(&file_stat)?),
        FS_CANONICAL_VECTORS.file_stat,
        "file stat canonical bytes mismatch"
    );

    // list_directory returns the child, name-sorted.
    let listing = loom.list_directory(ns, "docs")?;
    assert_eq!(listing.len(), 1);
    assert_eq!(listing[0].name, "readme.txt");
    assert_eq!(listing[0].kind, FileKind::File);
    assert_eq!(
        hex::encode(dir_listing_to_cbor(&listing)?),
        FS_CANONICAL_VECTORS.dir_listing,
        "directory listing canonical bytes mismatch"
    );

    // A non-empty directory needs `recursive`; then it is gone.
    assert!(loom.remove_directory(ns, "docs", false).is_err());
    loom.remove_directory(ns, "docs", true)?;
    assert!(loom.stat(ns, "docs").is_err());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fsdir_behavior_passes() {
        run_fsdir_behavior().expect("filesystem dir/metadata behavior must pass");
    }
}
