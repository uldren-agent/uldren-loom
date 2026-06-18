//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

/// Create-or-replace file `path` in the workspace working tree with `content` and `mode` (default
/// `0o100644`). The parent directory must exist.
#[napi]
pub fn write_file(
    loom_path: String,
    facet: String,
    workspace: String,
    path: String,
    content: Uint8Array,
    mode: Option<u32>,
    passphrase: Option<String>,
) -> napi::Result<()> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_typed_ns(&loom, &facet, &workspace)?;
    loom.write_file(ns, &path, &content, mode.unwrap_or(0o100644))
        .map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(())
}
/// Read file `path` from the workspace working tree.
#[napi]
pub fn read_file(
    loom_path: String,
    facet: String,
    workspace: String,
    path: String,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_typed_ns(&loom, &facet, &workspace)?;
    Ok(Uint8Array::from(loom.read_file(ns, &path).map_err(reason)?))
}
/// Append `content` to file `path`, creating it if absent (the parent directory must exist).
#[napi]
pub fn append_file(
    loom_path: String,
    facet: String,
    workspace: String,
    path: String,
    content: Uint8Array,
    passphrase: Option<String>,
) -> napi::Result<()> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_typed_ns(&loom, &facet, &workspace)?;
    loom.append_file(ns, &path, &content).map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(())
}
/// Remove file `path` from the workspace working tree.
#[napi]
pub fn remove_file(
    loom_path: String,
    facet: String,
    workspace: String,
    path: String,
    passphrase: Option<String>,
) -> napi::Result<()> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_typed_ns(&loom, &facet, &workspace)?;
    loom.remove_file(ns, &path).map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(())
}
/// Create a symbolic link at `linkPath` whose target is `target` (opaque; may be dangling). The parent
/// must exist; `linkPath` must be free.
#[napi]
pub fn symlink(
    loom_path: String,
    facet: String,
    workspace: String,
    target: String,
    link_path: String,
    passphrase: Option<String>,
) -> napi::Result<()> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_typed_ns(&loom, &facet, &workspace)?;
    loom.symlink(ns, &target, &link_path).map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(())
}
/// Read the target of the symbolic link at `path` (errors if absent or not a symlink).
#[napi]
pub fn read_link(
    loom_path: String,
    facet: String,
    workspace: String,
    path: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_typed_ns(&loom, &facet, &workspace)?;
    loom.read_link(ns, &path).map_err(reason)
}
/// Read up to `len` bytes from byte `offset` of file `path` (bounded chunk read; reads past the end
/// clamp). A missing file is an error.
#[napi]
pub fn read_at(
    loom_path: String,
    facet: String,
    workspace: String,
    path: String,
    offset: i64,
    len: i64,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_typed_ns(&loom, &facet, &workspace)?;
    let bytes = loom
        .read_at(ns, &path, as_u64(offset, "offset")?, as_u64(len, "len")?)
        .map_err(reason)?;
    Ok(Uint8Array::from(bytes))
}
/// Write `content` at byte `offset` of file `path`, creating it if absent and zero-filling any gap.
#[napi]
pub fn write_at(
    loom_path: String,
    facet: String,
    workspace: String,
    path: String,
    offset: i64,
    content: Uint8Array,
    passphrase: Option<String>,
) -> napi::Result<()> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_typed_ns(&loom, &facet, &workspace)?;
    loom.write_at(ns, &path, as_u64(offset, "offset")?, &content)
        .map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(())
}
/// Resize file `path` to `size`, zero-extending or dropping bytes; a missing file is created zero-filled.
#[napi]
pub fn truncate_file(
    loom_path: String,
    facet: String,
    workspace: String,
    path: String,
    size: i64,
    passphrase: Option<String>,
) -> napi::Result<()> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_typed_ns(&loom, &facet, &workspace)?;
    loom.truncate_file(ns, &path, as_u64(size, "size")?)
        .map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(())
}
/// Open a file handle on `path` with `mode` (`read`|`write`|`read_write`|`append`), returning the handle
/// id (valid until `fileClose`).
#[napi]
pub fn file_open(
    loom_path: String,
    facet: String,
    workspace: String,
    path: String,
    mode: String,
    passphrase: Option<String>,
) -> napi::Result<i64> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_typed_ns(&loom, &facet, &workspace)?;
    let handle = loom
        .file_open(ns, &path, parse_open_mode(&mode)?)
        .map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(handle as i64)
}
/// Sequentially read up to `len` bytes from handle `file` at its cursor, advancing it.
#[napi]
pub fn file_read(
    loom_path: String,
    file: i64,
    len: i64,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let bytes = loom
        .file_read(as_u64(file, "file")?, as_u64(len, "len")?)
        .map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(Uint8Array::from(bytes))
}
/// Positionally read up to `len` bytes at `offset` from handle `file` without moving its cursor.
#[napi]
pub fn file_read_at(
    loom_path: String,
    file: i64,
    offset: i64,
    len: i64,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let bytes = loom
        .file_read_at(
            as_u64(file, "file")?,
            as_u64(offset, "offset")?,
            as_u64(len, "len")?,
        )
        .map_err(reason)?;
    Ok(Uint8Array::from(bytes))
}
/// Sequentially write `content` to handle `file` at its cursor (or end of file for an append handle),
/// advancing it; returns the byte count.
#[napi]
pub fn file_write(
    loom_path: String,
    file: i64,
    content: Uint8Array,
    passphrase: Option<String>,
) -> napi::Result<i64> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let n = loom
        .file_write(as_u64(file, "file")?, &content)
        .map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(n as i64)
}
/// Positionally write `content` at `offset` of handle `file` without moving its cursor; returns the byte
/// count.
#[napi]
pub fn file_write_at(
    loom_path: String,
    file: i64,
    offset: i64,
    content: Uint8Array,
    passphrase: Option<String>,
) -> napi::Result<i64> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let n = loom
        .file_write_at(as_u64(file, "file")?, as_u64(offset, "offset")?, &content)
        .map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(n as i64)
}
/// Resize handle `file` to `size` bytes.
#[napi]
pub fn file_truncate(
    loom_path: String,
    file: i64,
    size: i64,
    passphrase: Option<String>,
) -> napi::Result<()> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    loom.file_truncate(as_u64(file, "file")?, as_u64(size, "size")?)
        .map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(())
}
/// Flush handle `file` (validates the handle; writes already apply per operation).
#[napi]
pub fn file_flush(loom_path: String, file: i64, passphrase: Option<String>) -> napi::Result<()> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    loom.file_flush(as_u64(file, "file")?).map_err(reason)?;
    Ok(())
}
/// The live size and mode of handle `file`.
#[napi]
pub fn file_stat(
    loom_path: String,
    file: i64,
    passphrase: Option<String>,
) -> napi::Result<FileStatJs> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let st = loom.file_stat(as_u64(file, "file")?).map_err(reason)?;
    Ok(FileStatJs {
        size: st.size as i64,
        mode: st.mode,
    })
}
/// Close handle `file`, releasing it (delete-on-last-close for an unlinked inode).
#[napi]
pub fn file_close(loom_path: String, file: i64, passphrase: Option<String>) -> napi::Result<()> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    loom.file_close(as_u64(file, "file")?).map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(())
}
