use std::collections::BTreeMap;
use std::fs;
use std::io::{Cursor, Read, Seek, Write};
use std::path::{Component, Path, PathBuf};

use flate2::{Compression, GzBuilder};
use loom_codec::Value as CborValue;
use loom_core::{
    AclRight, Bundle, ColumnType, FacetKind, FileKind, Loom, ObjectStore, Predicate, Schema, Table,
    Value, bundle_export, bundle_import, get_table, put_table,
};
use loom_interchange::{
    ArchiveEntry, ArchiveEntryKind, ArchiveKind, ArchiveManifest, ExportReport, FidelityIssue,
    FidelitySeverity, ImportBatch, ImportCheckpoint, ImportExecutionBatch, ImportReport,
    ImportReportInput,
};
use loom_store::{FileStore, save_loom};
use loom_substrate::meetings::{
    AnnotationRecord, AnnotationStatus, Coverage as MeetingsCoverage, ImportRunRecord,
    InputProfile, MeetingRecord, MeetingRecordInput, MeetingStatus, MeetingsProfileSnapshot,
    MeetingsProfileSnapshotParts, SourceRecord, SourceRecordInput, SpanKind, SpanRecord,
    meetings_profile_key,
};
use loom_substrate::versioning::{
    BodyRef, ProfileRevisionUpdate, ProfileTransaction, ProfileTransactionState,
    REVISION_INDEX_DIR, RevisionIndex, revision_index_path,
};
use loom_types::{Algo, Code, Digest, LoomError, Result, WorkspaceId};
use zip::result::ZipError;

pub mod profiles;
pub mod transfer;
pub use profiles::*;

const RETAINED_IMPORT_SOURCE_SCHEMA: &str = "loom.interchange.retained-source.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportInputKind {
    File,
    Directory,
    ArchiveCandidate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedImportInput {
    pub source_scope: String,
    pub kind: ImportInputKind,
    pub source_digest: Digest,
    pub size_bytes: u64,
    pub item_count: u64,
    pub path: PathBuf,
    pub bytes: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetainedImportSource {
    pub workspace: WorkspaceId,
    pub profile: String,
    pub source_scope: String,
    pub source_digest: Digest,
    pub manifest_key: Vec<u8>,
    pub manifest_digest: Digest,
    pub payload_keys: Vec<Vec<u8>>,
    pub bytes_retained: u64,
    pub item_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportBatchSubmission {
    pub batch: ImportBatch,
    pub batch_digest: Digest,
    pub control_key: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportExecutionBatchResult {
    pub batch: ImportExecutionBatch,
    pub batch_digest: Digest,
    pub report: ImportReport,
    pub changed: bool,
    pub control_key: Vec<u8>,
}

impl ResolvedImportInput {
    pub fn checkpoint(&self, profile: &str, checkpoint_id: &str) -> Result<ImportCheckpoint> {
        let mut checkpoint = ImportCheckpoint::new(
            checkpoint_id,
            profile,
            &self.source_scope,
            self.source_digest.to_string().into_bytes(),
        )?;
        checkpoint.observed_ids.push(self.source_scope.clone());
        checkpoint.completed_units.push(format!(
            "{}:{}:{}",
            import_input_kind_label(self.kind),
            self.item_count,
            self.size_bytes
        ));
        checkpoint.profile_state_digest = Some(self.source_digest);
        Ok(checkpoint)
    }
}

pub fn import_batch_control_key(ns: WorkspaceId, digest: &Digest) -> Vec<u8> {
    format!("studio/imports/{ns}/batches/{digest}").into_bytes()
}

pub fn import_execution_batch_control_key(ns: WorkspaceId, digest: &Digest) -> Vec<u8> {
    format!("studio/imports/{ns}/execution-batches/{digest}").into_bytes()
}

pub fn import_checkpoint_control_key(
    ns: WorkspaceId,
    profile: &str,
    checkpoint_id: &str,
) -> Vec<u8> {
    format!(
        "studio/imports/{ns}/checkpoints/{}/{}",
        path_token(profile),
        path_token(checkpoint_id)
    )
    .into_bytes()
}

pub fn retained_import_source_manifest_key(
    ns: WorkspaceId,
    profile: &str,
    digest: &Digest,
) -> Vec<u8> {
    format!(
        "studio/imports/{ns}/sources/{}/{digest}/manifest",
        path_token(profile)
    )
    .into_bytes()
}

pub fn retained_import_source_payload_key(
    ns: WorkspaceId,
    profile: &str,
    source_digest: &Digest,
    payload_digest: &Digest,
) -> Vec<u8> {
    format!(
        "studio/imports/{ns}/sources/{}/{source_digest}/payloads/{payload_digest}",
        path_token(profile)
    )
    .into_bytes()
}

pub fn persist_import_batch_submission(
    loom: &mut Loom<FileStore>,
    ns: WorkspaceId,
    batch_bytes: &[u8],
    audit_principal: Option<WorkspaceId>,
) -> Result<ImportBatchSubmission> {
    let batch = ImportBatch::decode(batch_bytes)?;
    let batch_digest = Digest::hash(loom.store().digest_algo(), batch_bytes);
    let control_key = import_batch_control_key(ns, &batch_digest);
    loom.store().control_set_audited(
        &control_key,
        batch_bytes.to_vec(),
        audit_principal.or(Some(ns)),
        "import.submit_batch",
        Some(&batch.source_scope),
    )?;
    Ok(ImportBatchSubmission {
        batch,
        batch_digest,
        control_key,
    })
}

pub fn execute_import_execution_batch(
    loom: &mut Loom<FileStore>,
    ns: WorkspaceId,
    batch_bytes: &[u8],
    dry_run: bool,
    audit_principal: Option<WorkspaceId>,
) -> Result<ImportExecutionBatchResult> {
    let batch = ImportExecutionBatch::decode(batch_bytes)?;
    if batch.payloads.is_empty() {
        return Err(LoomError::invalid(
            "import execution batch requires at least one snapshot payload",
        ));
    }
    let profile_key = import_execution_profile_key(&batch).to_string();
    if profile_key != "drive" && batch.payloads.len() != 1 {
        return Err(LoomError::invalid(
            "import execution batch requires exactly one snapshot payload",
        ));
    }
    let payload = &batch.payloads[0];
    let workspace_id = ns.to_string();
    let source_scope = batch.source_scope.as_str();
    let report = match profile_key.as_str() {
        "redmine" => import_redmine_bytes_with_field_policy(
            loom,
            ns,
            &workspace_id,
            source_scope,
            &payload.bytes,
            dry_run,
            TicketImportFieldPolicy::Infer,
        )?,
        "asana" => import_asana_bytes_with_field_policy(
            loom,
            ns,
            &workspace_id,
            source_scope,
            &payload.bytes,
            dry_run,
            TicketImportFieldPolicy::Infer,
        )?,
        "jira" => import_jira_bytes_with_field_policy(
            loom,
            ns,
            &workspace_id,
            source_scope,
            &payload.bytes,
            dry_run,
            TicketImportFieldPolicy::Infer,
        )?,
        "confluence" | "confluence-storage" | "confluence-adf" => {
            let default_space = batch.default_space.as_deref().unwrap_or("imported");
            import_confluence_bytes(
                loom,
                ns,
                &workspace_id,
                source_scope,
                default_space,
                &payload.bytes,
                dry_run,
            )?
        }
        "notion" => {
            let default_space = batch.default_space.as_deref().unwrap_or("imported");
            import_notion_bytes(
                loom,
                ns,
                &workspace_id,
                source_scope,
                default_space,
                &payload.bytes,
                dry_run,
            )?
        }
        "markdown" => {
            let default_space = batch.default_space.as_deref().unwrap_or("imported");
            execute_markdown_archive_payload(
                loom,
                ns,
                &workspace_id,
                source_scope,
                default_space,
                payload,
                dry_run,
            )?
        }
        "slack" => import_slack_bytes(
            loom,
            ns,
            &workspace_id,
            source_scope,
            &payload.bytes,
            dry_run,
        )?,
        "drive" => execute_drive_payload(loom, ns, &workspace_id, source_scope, &batch, dry_run)?,
        "meetings" | "granola-api" | "granola-app" | "granola-mcp" | "csv" => {
            let profile = parse_meetings_input_profile(import_meetings_input_profile(&batch))?;
            import_meetings_bytes(loom, ns, profile, &payload.bytes, dry_run)?.report
        }
        other => {
            return Err(LoomError::new(
                Code::Unsupported,
                format!("import execution profile {other} is not source-backed"),
            ));
        }
    };
    let batch_digest = Digest::hash(loom.store().digest_algo(), batch_bytes);
    let control_key = import_execution_batch_control_key(ns, &batch_digest);
    if !dry_run {
        loom.store().control_set_audited(
            &control_key,
            batch_bytes.to_vec(),
            audit_principal.or(Some(ns)),
            "import.execute_batch",
            Some(&batch.source_scope),
        )?;
        save_loom(loom)?;
    }
    let changed = !dry_run && report.operations_applied > 0;
    Ok(ImportExecutionBatchResult {
        batch,
        batch_digest,
        report,
        changed,
        control_key,
    })
}

fn execute_markdown_archive_payload(
    loom: &mut Loom<FileStore>,
    ns: WorkspaceId,
    workspace_id: &str,
    source_scope: &str,
    default_space: &str,
    payload: &loom_interchange::ImportExecutionPayload,
    dry_run: bool,
) -> Result<ImportReport> {
    let root = std::env::temp_dir().join(format!(
        "loom-markdown-execution-{}-{}",
        std::process::id(),
        now_ms()
    ));
    fs::create_dir_all(&root).map_err(|e| {
        LoomError::new(
            Code::Io,
            format!(
                "create markdown execution directory {}: {e}",
                root.display()
            ),
        )
    })?;
    let result = (|| {
        materialize_markdown_archive_payload(payload, &root)?;
        import_markdown_path(
            loom,
            ns,
            workspace_id,
            source_scope,
            &root,
            default_space,
            dry_run,
        )
    })();
    let cleanup = fs::remove_dir_all(&root);
    match (result, cleanup) {
        (Ok(report), Ok(())) => Ok(report),
        (Ok(_), Err(error)) => Err(LoomError::new(
            Code::Io,
            format!(
                "remove markdown execution directory {}: {error}",
                root.display()
            ),
        )),
        (Err(error), _) => Err(error),
    }
}

fn execute_drive_payload(
    loom: &mut Loom<FileStore>,
    ns: WorkspaceId,
    workspace_id: &str,
    source_scope: &str,
    batch: &ImportExecutionBatch,
    dry_run: bool,
) -> Result<ImportReport> {
    let root = std::env::temp_dir().join(format!(
        "loom-drive-exec-{}-{}",
        std::process::id(),
        now_ms()
    ));
    match fs::create_dir_all(&root) {
        Ok(()) => {}
        Err(error) => {
            return Err(LoomError::new(
                Code::Io,
                format!(
                    "create drive execution directory {}: {error}",
                    root.display()
                ),
            ));
        }
    }
    let result = (|| {
        for sidecar in batch.payloads.iter().skip(1) {
            let target = safe_join(&root, &sidecar.payload_id)?;
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|error| {
                    LoomError::new(
                        Code::Io,
                        format!(
                            "create drive execution sidecar directory {}: {error}",
                            parent.display()
                        ),
                    )
                })?;
            }
            fs::write(&target, &sidecar.bytes).map_err(|error| {
                LoomError::new(
                    Code::Io,
                    format!(
                        "write drive execution sidecar {}: {error}",
                        target.display()
                    ),
                )
            })?;
        }
        let snapshot = &batch.payloads[0];
        import_drive_bytes(
            loom,
            ns,
            workspace_id,
            source_scope,
            &snapshot.bytes,
            &root,
            dry_run,
        )
    })();
    match (result, fs::remove_dir_all(&root)) {
        (Ok(report), Ok(())) => Ok(report),
        (Ok(report), Err(error)) if error.kind() == std::io::ErrorKind::NotFound => Ok(report),
        (Ok(_), Err(error)) => Err(LoomError::new(
            Code::Io,
            format!(
                "remove drive execution directory {}: {error}",
                root.display()
            ),
        )),
        (Err(error), _) => Err(error),
    }
}

fn materialize_markdown_archive_payload(
    payload: &loom_interchange::ImportExecutionPayload,
    root: &Path,
) -> Result<()> {
    match markdown_archive_kind(payload)? {
        ArchiveKind::Zip => materialize_zip_payload(&payload.bytes, root),
        ArchiveKind::Tar => materialize_tar_payload(Cursor::new(&payload.bytes), root),
        ArchiveKind::TarGzip => materialize_tar_payload(
            flate2::read::GzDecoder::new(Cursor::new(&payload.bytes)),
            root,
        ),
        ArchiveKind::TarZstd => {
            #[cfg(feature = "zstd")]
            {
                let decoder = zstd::stream::read::Decoder::new(Cursor::new(&payload.bytes))
                    .map_err(|e| LoomError::invalid(format!("read zstd archive: {e}")))?;
                materialize_tar_payload(decoder, root)
            }
            #[cfg(not(feature = "zstd"))]
            {
                Err(LoomError::unsupported(
                    "tar-zstd Markdown execution payload is not supported in this build",
                ))
            }
        }
        ArchiveKind::Gzip => Err(LoomError::unsupported(
            "single-file gzip is not a Markdown vault execution payload",
        )),
    }
}

fn markdown_archive_kind(
    payload: &loom_interchange::ImportExecutionPayload,
) -> Result<ArchiveKind> {
    let media_type = payload.media_type.to_ascii_lowercase();
    let payload_id = payload.payload_id.to_ascii_lowercase();
    if media_type == "application/zip" || payload_id.ends_with(".zip") {
        Ok(ArchiveKind::Zip)
    } else if media_type == "application/x-tar" || payload_id.ends_with(".tar") {
        Ok(ArchiveKind::Tar)
    } else if matches!(
        media_type.as_str(),
        "application/gzip" | "application/x-gzip" | "application/tar+gzip"
    ) || payload_id.ends_with(".tar.gz")
        || payload_id.ends_with(".tgz")
    {
        Ok(ArchiveKind::TarGzip)
    } else if media_type == "application/tar+zstd"
        || payload_id.ends_with(".tar.zstd")
        || payload_id.ends_with(".tar.zst")
    {
        Ok(ArchiveKind::TarZstd)
    } else {
        Err(LoomError::unsupported(format!(
            "Markdown execution payload {} has unsupported media type {}",
            payload.payload_id, payload.media_type
        )))
    }
}

fn materialize_zip_payload(bytes: &[u8], root: &Path) -> Result<()> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(zip_archive_error)?;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|e| zip_entry_error(index, e))?;
        let path = archive_loom_path(entry.name())?;
        if entry.encrypted() {
            return Err(LoomError::unsupported(format!(
                "encrypted zip entry {path} is not supported"
            )));
        }
        if entry.is_symlink() {
            return Err(LoomError::unsupported(format!(
                "zip symlink entry {path} is not supported"
            )));
        }
        let target = safe_join(root, &path)?;
        if entry.is_dir() {
            fs::create_dir_all(&target).map_err(|e| {
                LoomError::new(
                    Code::Io,
                    format!(
                        "create markdown archive directory {}: {e}",
                        target.display()
                    ),
                )
            })?;
        } else {
            let mut file_bytes = Vec::new();
            entry
                .read_to_end(&mut file_bytes)
                .map_err(|e| LoomError::new(Code::Io, format!("read zip entry {path}: {e}")))?;
            materialize_markdown_payload_file(&target, &file_bytes)?;
        }
    }
    Ok(())
}

fn materialize_tar_payload<R: Read>(reader: R, root: &Path) -> Result<()> {
    let mut archive = tar::Archive::new(reader);
    let entries = archive
        .entries()
        .map_err(|e| LoomError::invalid(format!("read tar archive: {e}")))?;
    for entry in entries {
        let mut entry =
            entry.map_err(|e| LoomError::invalid(format!("read tar archive entry: {e}")))?;
        let path = archive_loom_path(
            entry
                .path()
                .map_err(|e| LoomError::invalid(format!("read tar entry path: {e}")))?
                .to_str()
                .ok_or_else(|| LoomError::invalid("tar entry path is not valid UTF-8"))?,
        )?;
        let target = safe_join(root, &path)?;
        if entry.header().entry_type().is_dir() {
            fs::create_dir_all(&target).map_err(|e| {
                LoomError::new(
                    Code::Io,
                    format!(
                        "create markdown archive directory {}: {e}",
                        target.display()
                    ),
                )
            })?;
        } else if entry.header().entry_type().is_file() {
            let mut file_bytes = Vec::new();
            entry
                .read_to_end(&mut file_bytes)
                .map_err(|e| LoomError::new(Code::Io, format!("read tar entry {path}: {e}")))?;
            materialize_markdown_payload_file(&target, &file_bytes)?;
        } else {
            return Err(LoomError::unsupported(format!(
                "unsupported tar entry type at {path}"
            )));
        }
    }
    Ok(())
}

fn materialize_markdown_payload_file(target: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            LoomError::new(
                Code::Io,
                format!(
                    "create markdown archive directory {}: {e}",
                    parent.display()
                ),
            )
        })?;
    }
    fs::write(target, bytes).map_err(|e| {
        LoomError::new(
            Code::Io,
            format!("write markdown archive file {}: {e}", target.display()),
        )
    })
}

fn import_execution_profile_key(batch: &ImportExecutionBatch) -> &str {
    match batch.profile.as_str() {
        "tickets" | "pages" | "chat" | "meetings" => batch.source_system.as_str(),
        profile => profile,
    }
}

fn import_meetings_input_profile(batch: &ImportExecutionBatch) -> &str {
    match batch.source_system.as_str() {
        "granola-api" | "granola-app" | "granola-mcp" | "csv" | "generic" => {
            batch.source_system.as_str()
        }
        _ => batch.profile.as_str(),
    }
}

pub fn persist_import_checkpoint(
    loom: &mut Loom<FileStore>,
    ns: WorkspaceId,
    checkpoint: &ImportCheckpoint,
    audit_principal: Option<WorkspaceId>,
) -> Result<Vec<u8>> {
    let encoded = checkpoint.encode()?;
    let key = import_checkpoint_control_key(ns, &checkpoint.profile, &checkpoint.checkpoint_id);
    loom.store().control_set_audited(
        &key,
        encoded,
        audit_principal.or(Some(ns)),
        "import.checkpoint",
        Some(&checkpoint.checkpoint_id),
    )?;
    Ok(key)
}

pub fn retain_import_input(
    loom: &mut Loom<FileStore>,
    ns: WorkspaceId,
    profile: &str,
    input: &ResolvedImportInput,
    audit_principal: Option<WorkspaceId>,
) -> Result<RetainedImportSource> {
    let manifest_key = retained_import_source_manifest_key(ns, profile, &input.source_digest);
    let mut payload_keys = Vec::new();
    let mut entries = Vec::new();
    let mut bytes_retained = 0u64;
    match input.kind {
        ImportInputKind::File | ImportInputKind::ArchiveCandidate => {
            let bytes = input
                .bytes
                .as_ref()
                .ok_or_else(|| LoomError::invalid("retained import file input has no bytes"))?;
            let payload_digest = Digest::hash(loom.store().digest_algo(), bytes);
            let payload_key = retained_import_source_payload_key(
                ns,
                profile,
                &input.source_digest,
                &payload_digest,
            );
            loom.store().control_set_audited(
                &payload_key,
                bytes.clone(),
                audit_principal.or(Some(ns)),
                "import.retain_source.payload",
                Some(&input.source_scope),
            )?;
            bytes_retained = bytes.len() as u64;
            payload_keys.push(payload_key.clone());
            entries.push(retained_source_entry_value(
                "",
                bytes.len() as u64,
                payload_digest,
                &payload_key,
            ));
        }
        ImportInputKind::Directory => {
            let file_entries =
                collect_retained_directory_entries(&input.path, loom.store().digest_algo())?;
            for entry in file_entries {
                let payload_key = retained_import_source_payload_key(
                    ns,
                    profile,
                    &input.source_digest,
                    &entry.digest,
                );
                loom.store().control_set_audited(
                    &payload_key,
                    entry.bytes,
                    audit_principal.or(Some(ns)),
                    "import.retain_source.payload",
                    Some(&entry.relative_path),
                )?;
                bytes_retained = bytes_retained.saturating_add(entry.size);
                payload_keys.push(payload_key.clone());
                entries.push(retained_source_entry_value(
                    &entry.relative_path,
                    entry.size,
                    entry.digest,
                    &payload_key,
                ));
            }
        }
    }
    let manifest = retained_source_manifest_value(input, ns, profile, &entries);
    let manifest_bytes = loom_codec::encode(&manifest).map_err(codec_error)?;
    let manifest_digest = Digest::hash(loom.store().digest_algo(), &manifest_bytes);
    loom.store().control_set_audited(
        &manifest_key,
        manifest_bytes,
        audit_principal.or(Some(ns)),
        "import.retain_source.manifest",
        Some(&input.source_scope),
    )?;
    Ok(RetainedImportSource {
        workspace: ns,
        profile: profile.to_string(),
        source_scope: input.source_scope.clone(),
        source_digest: input.source_digest,
        manifest_key,
        manifest_digest,
        payload_keys,
        bytes_retained,
        item_count: input.item_count,
    })
}

pub fn resolve_import_input(path: &Path, algo: Algo) -> Result<ResolvedImportInput> {
    let source_scope = path.to_string_lossy().to_string();
    resolve_import_input_with_scope(path, &source_scope, algo)
}

pub fn resolve_import_input_with_scope(
    path: &Path,
    source_scope: &str,
    algo: Algo,
) -> Result<ResolvedImportInput> {
    let metadata = fs::metadata(path).map_err(|e| {
        LoomError::new(
            Code::Io,
            format!("read import input metadata {}: {e}", path.display()),
        )
    })?;
    if metadata.is_dir() {
        let (fingerprint, size_bytes, item_count) = directory_fingerprint(path, algo)?;
        return Ok(ResolvedImportInput {
            source_scope: source_scope.to_string(),
            kind: ImportInputKind::Directory,
            source_digest: Digest::hash(algo, &fingerprint),
            size_bytes,
            item_count,
            path: path.to_path_buf(),
            bytes: None,
        });
    }
    if !metadata.is_file() {
        return Err(LoomError::invalid(format!(
            "import input {} is neither a regular file nor a directory",
            path.display()
        )));
    }
    let bytes = fs::read(path).map_err(|e| {
        LoomError::new(
            Code::Io,
            format!("read import input {}: {e}", path.display()),
        )
    })?;
    let kind = if is_archive_candidate(path) {
        ImportInputKind::ArchiveCandidate
    } else {
        ImportInputKind::File
    };
    Ok(ResolvedImportInput {
        source_scope: source_scope.to_string(),
        kind,
        source_digest: Digest::hash(algo, &bytes),
        size_bytes: bytes.len() as u64,
        item_count: 1,
        path: path.to_path_buf(),
        bytes: Some(bytes),
    })
}

fn import_input_kind_label(kind: ImportInputKind) -> &'static str {
    match kind {
        ImportInputKind::File => "file",
        ImportInputKind::Directory => "directory",
        ImportInputKind::ArchiveCandidate => "archive",
    }
}

fn is_archive_candidate(path: &Path) -> bool {
    let lower = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    lower.ends_with(".zip")
        || lower.ends_with(".tar")
        || lower.ends_with(".tar.gz")
        || lower.ends_with(".tgz")
        || lower.ends_with(".tar.zstd")
        || lower.ends_with(".tar.zst")
        || lower.ends_with(".gz")
}

fn directory_fingerprint(root: &Path, algo: Algo) -> Result<(Vec<u8>, u64, u64)> {
    let mut files = Vec::new();
    collect_directory_fingerprint_entries(root, root, algo, &mut files)?;
    files.sort_by(|left, right| left.0.cmp(&right.0));
    let size_bytes = files.iter().map(|(_, size, _)| *size).sum();
    let item_count = files.len() as u64;
    let value = CborValue::Array(
        files
            .into_iter()
            .map(|(path, size, digest)| {
                CborValue::Array(vec![
                    CborValue::Text(path),
                    CborValue::Uint(size),
                    CborValue::Text(digest.to_string()),
                ])
            })
            .collect(),
    );
    let bytes = loom_codec::encode(&value).map_err(codec_error)?;
    Ok((bytes, size_bytes, item_count))
}

fn collect_directory_fingerprint_entries(
    root: &Path,
    current: &Path,
    algo: Algo,
    files: &mut Vec<(String, u64, Digest)>,
) -> Result<()> {
    for entry in fs::read_dir(current).map_err(|e| {
        LoomError::new(
            Code::Io,
            format!("read import input directory {}: {e}", current.display()),
        )
    })? {
        let entry = entry.map_err(|e| LoomError::new(Code::Io, e.to_string()))?;
        let path = entry.path();
        let metadata = entry.metadata().map_err(|e| {
            LoomError::new(
                Code::Io,
                format!("read import input metadata {}: {e}", path.display()),
            )
        })?;
        if metadata.is_dir() {
            collect_directory_fingerprint_entries(root, &path, algo, files)?;
        } else if metadata.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(|e| LoomError::invalid(e.to_string()))?
                .to_string_lossy()
                .replace('\\', "/");
            let bytes = fs::read(&path).map_err(|e| {
                LoomError::new(
                    Code::Io,
                    format!("read import input {}: {e}", path.display()),
                )
            })?;
            files.push((relative, bytes.len() as u64, Digest::hash(algo, &bytes)));
        }
    }
    Ok(())
}

struct RetainedDirectoryEntry {
    relative_path: String,
    digest: Digest,
    size: u64,
    bytes: Vec<u8>,
}

fn collect_retained_directory_entries(
    root: &Path,
    algo: Algo,
) -> Result<Vec<RetainedDirectoryEntry>> {
    let mut entries = Vec::new();
    collect_retained_directory_entries_at(root, root, algo, &mut entries)?;
    entries.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok(entries)
}

fn collect_retained_directory_entries_at(
    root: &Path,
    current: &Path,
    algo: Algo,
    entries: &mut Vec<RetainedDirectoryEntry>,
) -> Result<()> {
    for entry in fs::read_dir(current).map_err(|e| {
        LoomError::new(
            Code::Io,
            format!("read retained import directory {}: {e}", current.display()),
        )
    })? {
        let entry = entry.map_err(|e| LoomError::new(Code::Io, e.to_string()))?;
        let path = entry.path();
        let metadata = entry.metadata().map_err(|e| {
            LoomError::new(
                Code::Io,
                format!("read retained import metadata {}: {e}", path.display()),
            )
        })?;
        if metadata.is_dir() {
            collect_retained_directory_entries_at(root, &path, algo, entries)?;
        } else if metadata.is_file() {
            let relative_path = path
                .strip_prefix(root)
                .map_err(|e| LoomError::invalid(e.to_string()))?
                .to_string_lossy()
                .replace('\\', "/");
            let bytes = fs::read(&path).map_err(|e| {
                LoomError::new(
                    Code::Io,
                    format!("read retained import file {}: {e}", path.display()),
                )
            })?;
            entries.push(RetainedDirectoryEntry {
                relative_path,
                digest: Digest::hash(algo, &bytes),
                size: bytes.len() as u64,
                bytes,
            });
        }
    }
    Ok(())
}

fn retained_source_manifest_value(
    input: &ResolvedImportInput,
    ns: WorkspaceId,
    profile: &str,
    entries: &[CborValue],
) -> CborValue {
    CborValue::Array(vec![
        CborValue::Text(RETAINED_IMPORT_SOURCE_SCHEMA.to_string()),
        CborValue::Array(vec![
            CborValue::Text(ns.to_string()),
            CborValue::Text(profile.to_string()),
            CborValue::Text(input.source_scope.clone()),
            CborValue::Text(import_input_kind_label(input.kind).to_string()),
            CborValue::Text(input.source_digest.to_string()),
            CborValue::Uint(input.size_bytes),
            CborValue::Uint(input.item_count),
            CborValue::Array(entries.to_vec()),
        ]),
    ])
}

fn retained_source_entry_value(
    relative_path: &str,
    size: u64,
    digest: Digest,
    payload_key: &[u8],
) -> CborValue {
    CborValue::Array(vec![
        CborValue::Text(relative_path.to_string()),
        CborValue::Uint(size),
        CborValue::Text(digest.to_string()),
        CborValue::Bytes(payload_key.to_vec()),
    ])
}

#[derive(Debug, serde::Deserialize)]
struct MeetingsImportSnapshotJson {
    snapshot_version: u64,
    profile: Option<String>,
    source_system: String,
    source_scope: String,
    observed_at: u64,
    coverage: String,
    source_cursor: Option<String>,
    source_sidecar_digest: Option<String>,
    coverage_gaps: Option<Vec<String>>,
    retry_windows: Option<Vec<String>>,
    resume_state: Option<String>,
    items: Vec<MeetingsImportItemJson>,
}

#[derive(Debug, serde::Deserialize)]
struct MeetingsImportItemJson {
    source_entity_id: String,
    source_digest: String,
    source_sidecar: Option<serde_json::Value>,
    source_created_at: Option<u64>,
    source_updated_at: Option<u64>,
    source_sidecar_digest: Option<String>,
    source_state: Option<String>,
    meeting_id: Option<String>,
    title: Option<String>,
    owner: Option<String>,
    calendar_event: Option<String>,
    attendees: Option<Vec<String>>,
    folder_refs: Option<Vec<String>>,
    summary_text: Option<String>,
    summary_markdown_digest: Option<String>,
    transcript_spans: Option<Vec<MeetingsImportSpanJson>>,
    annotations: Option<Vec<MeetingsImportAnnotationJson>>,
    tasks: Option<Vec<MeetingsImportStructuredItemJson>>,
    action_items: Option<Vec<MeetingsImportStructuredItemJson>>,
    topics: Option<Vec<MeetingsImportStructuredItemJson>>,
    decisions: Option<Vec<MeetingsImportStructuredItemJson>>,
    questions: Option<Vec<MeetingsImportStructuredItemJson>>,
    risks: Option<Vec<MeetingsImportStructuredItemJson>>,
    artifacts: Option<Vec<MeetingsImportStructuredItemJson>>,
    references: Option<Vec<MeetingsImportStructuredItemJson>>,
}

#[derive(Debug, serde::Deserialize)]
struct MeetingsImportSpanJson {
    span_id: Option<String>,
    locator: Option<String>,
    speaker: Option<String>,
    language: Option<String>,
    text: Option<String>,
    text_digest: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct MeetingsImportAnnotationJson {
    annotation_id: Option<String>,
    kind: String,
    label: String,
    source_span_ids: Option<Vec<String>>,
    normalized_id: Option<String>,
    confidence_ppm: Option<u32>,
    evidence_digest: Option<String>,
    extractor: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct MeetingsImportStructuredItemJson {
    id: Option<String>,
    label: String,
    source_span_ids: Option<Vec<String>>,
    normalized_id: Option<String>,
    confidence_ppm: Option<u32>,
    evidence_digest: Option<String>,
    extractor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeetingsImportResult {
    pub report: ImportReport,
    pub changed: bool,
    pub payload_bytes: u64,
}

pub fn import_report_json(report: &ImportReport) -> Result<String> {
    let json = serde_json::json!({
        "profile": &report.profile,
        "source_scope": &report.source_scope,
        "commit": report.commit.map(|digest| digest.to_string()),
        "objects_added": report.objects_added,
        "bytes_in": report.bytes_in,
        "bytes_stored": report.bytes_stored,
        "rows_imported": report.rows_imported,
        "skipped": report.skipped,
        "operations_planned": report.operations_planned,
        "operations_applied": report.operations_applied,
        "dry_run": report.dry_run,
        "warnings": &report.warnings,
        "fidelity_issues": report.fidelity_issues.iter().map(|issue| serde_json::json!({
            "severity": format!("{:?}", issue.severity),
            "source_entity_id": &issue.source_entity_id,
            "field": &issue.field,
            "reason": &issue.reason,
            "source_digest": issue.source_digest.map(|digest| digest.to_string())
        })).collect::<Vec<_>>()
    });
    serde_json::to_string(&json).map_err(|e| LoomError::invalid(e.to_string()))
}

struct ParsedMeetingsImport {
    snapshot: MeetingsProfileSnapshot,
    checkpoint: ImportCheckpoint,
    payloads: Vec<MeetingsImportPayload>,
    source_scope: String,
    rows_imported: u64,
    operations_planned: u64,
}

struct MeetingsImportPayload {
    path: String,
    bytes: Vec<u8>,
}

pub fn import_meetings_bytes(
    loom: &mut Loom<FileStore>,
    workspace_id: WorkspaceId,
    input_profile: InputProfile,
    bytes: &[u8],
    dry_run: bool,
) -> Result<MeetingsImportResult> {
    let profile_id = workspace_id.to_string();
    let imported = parse_meetings_import_snapshot(loom, &profile_id, input_profile, bytes)?;
    let ParsedMeetingsImport {
        snapshot: imported_snapshot,
        checkpoint: mut imported_checkpoint,
        payloads,
        source_scope,
        rows_imported,
        operations_planned,
    } = imported;
    let key = meetings_profile_key(&profile_id)?;
    let previous = loom
        .store()
        .control_get(&key)?
        .map(|bytes| MeetingsProfileSnapshot::decode(&bytes))
        .transpose()?;
    let next = match previous {
        Some(ref previous) => merge_meetings_snapshot(previous.clone(), imported_snapshot)?,
        None => imported_snapshot,
    };
    let encoded = next.encode()?;
    imported_checkpoint.profile_state_digest =
        Some(Digest::hash(loom.store().digest_algo(), &encoded));
    let checkpoint_encoded = imported_checkpoint.encode()?;
    let changed = loom.store().control_get(&key)?.as_deref() != Some(encoded.as_slice());
    let payload_bytes = if dry_run {
        0
    } else {
        materialize_meetings_import_payloads(loom, workspace_id, &payloads)?
    };
    if !dry_run && changed {
        loom.store().control_set_audited(
            &key,
            encoded.clone(),
            Some(workspace_id),
            "meetings.import",
            Some(&profile_id),
        )?;
        update_meetings_revision_index(
            loom,
            workspace_id,
            &profile_id,
            previous.as_ref(),
            &next,
            &encoded,
        )?;
        loom.store().control_set_audited(
            &meetings_import_checkpoint_key(&profile_id, &imported_checkpoint.checkpoint_id),
            checkpoint_encoded,
            Some(workspace_id),
            "meetings.import.checkpoint",
            Some(&imported_checkpoint.checkpoint_id),
        )?;
    }
    if !dry_run && (changed || payload_bytes > 0) {
        save_loom(loom)?;
    }
    let mut report = ImportReport::new(ImportReportInput {
        profile: "meetings",
        source_scope: &source_scope,
        commit: None,
        objects_added: 0,
        bytes_in: bytes.len() as u64,
        bytes_stored: if dry_run {
            0
        } else {
            encoded.len() as u64 + payload_bytes
        },
        rows_imported,
        skipped: 0,
        operations_planned,
        operations_applied: if dry_run || !changed {
            0
        } else {
            operations_planned
        },
        dry_run,
    })?;
    if !changed {
        report
            .warnings
            .push("meetings snapshot already current".to_string());
    }
    Ok(MeetingsImportResult {
        report,
        changed,
        payload_bytes,
    })
}

pub fn load_meetings_snapshot(
    loom: &Loom<FileStore>,
    profile_id: &str,
) -> Result<Option<MeetingsProfileSnapshot>> {
    let key = meetings_profile_key(profile_id)?;
    loom.store()
        .control_get(&key)?
        .map(|bytes| MeetingsProfileSnapshot::decode(&bytes))
        .transpose()
}

fn parse_meetings_import_snapshot(
    loom: &Loom<FileStore>,
    profile_id: &str,
    input_profile: InputProfile,
    bytes: &[u8],
) -> Result<ParsedMeetingsImport> {
    let input: MeetingsImportSnapshotJson = serde_json::from_slice(bytes)
        .map_err(|e| LoomError::invalid(format!("parse meetings import JSON: {e}")))?;
    if input.snapshot_version != 1 {
        return Err(LoomError::invalid(format!(
            "unsupported meetings snapshot_version {}; expected 1",
            input.snapshot_version
        )));
    }
    if let Some(profile) = input.profile.as_deref()
        && profile != input_profile_label(input_profile)
        && profile != "meetings"
    {
        return Err(LoomError::invalid(format!(
            "meetings import profile {profile:?} does not match --input-profile {}",
            input_profile_label(input_profile)
        )));
    }
    let coverage = parse_meetings_coverage(&input.coverage)?;
    let mut sources = Vec::new();
    let mut meetings = Vec::new();
    let mut spans = Vec::new();
    let mut annotations = Vec::new();
    let mut payloads = Vec::new();
    let mut observed_ids = Vec::new();
    for item in &input.items {
        let source_digest = parse_digest(&item.source_digest)?;
        let source_id = item.source_entity_id.clone();
        observed_ids.push(source_id.clone());
        let mut source = SourceRecord::new(SourceRecordInput {
            source_id: &source_id,
            source_system: &input.source_system,
            external_id: &item.source_entity_id,
            source_digest,
            observed_at_ms: input.observed_at,
            access_scope: &input.source_scope,
            coverage,
        })?;
        source.source_created_at_ms = item.source_created_at;
        source.source_updated_at_ms = item.source_updated_at;
        source.owner_principal = item.owner.clone();
        source.sidecar_digest = item
            .source_sidecar_digest
            .as_deref()
            .or(input.source_sidecar_digest.as_deref())
            .map(parse_digest)
            .transpose()?;
        sources.push(source);
        if let Some(sidecar) = &item.source_sidecar {
            let bytes =
                serde_json::to_vec(sidecar).map_err(|e| LoomError::invalid(e.to_string()))?;
            payloads.push(MeetingsImportPayload {
                path: meetings_source_payload_path(
                    profile_id,
                    &item.source_entity_id,
                    "source.json",
                ),
                bytes,
            });
        }

        let meeting_id = item
            .meeting_id
            .clone()
            .unwrap_or_else(|| format!("meeting/{}", item.source_entity_id));
        let mut meeting = MeetingRecord::new(MeetingRecordInput {
            meeting_id: &meeting_id,
            title: item
                .title
                .as_deref()
                .filter(|title| !title.is_empty())
                .unwrap_or("Untitled meeting"),
            current_source_digest: source_digest,
            created_at_ms: item.source_created_at.unwrap_or(input.observed_at),
            updated_at_ms: item.source_updated_at.unwrap_or(input.observed_at),
        })?;
        meeting.calendar_event_ref = item.calendar_event.clone();
        meeting.owner_principal = item.owner.clone();
        meeting.attendee_refs = item.attendees.clone().unwrap_or_default();
        meeting.folder_refs = item.folder_refs.clone().unwrap_or_default();
        meeting.source_refs = vec![source_id.clone()];
        meeting.status = item
            .source_state
            .as_deref()
            .map(parse_meetings_source_state)
            .transpose()?
            .unwrap_or(MeetingStatus::Active);
        meeting.summary_ref = item
            .summary_markdown_digest
            .as_ref()
            .map(|_| format!("summary/{}", item.source_entity_id))
            .or_else(|| {
                item.summary_text
                    .as_ref()
                    .map(|_| format!("summary/{}", item.source_entity_id))
            });
        meetings.push(meeting);
        if let Some(summary) = &item.summary_text {
            payloads.push(MeetingsImportPayload {
                path: meetings_source_payload_path(
                    profile_id,
                    &item.source_entity_id,
                    "summary.txt",
                ),
                bytes: summary.as_bytes().to_vec(),
            });
        }

        if let Some(transcript_spans) = &item.transcript_spans {
            let mut transcript_jsonl = Vec::new();
            for (index, span) in transcript_spans.iter().enumerate() {
                let span_id = span
                    .span_id
                    .clone()
                    .unwrap_or_else(|| format!("span/{}/{index}", item.source_entity_id));
                let locator = span
                    .locator
                    .clone()
                    .unwrap_or_else(|| format!("transcript/{index}"));
                let mut record = SpanRecord::new(
                    span_id.clone(),
                    meeting_id.clone(),
                    source_id.clone(),
                    SpanKind::TranscriptEntry,
                    locator.clone(),
                )?;
                record.speaker_ref = span.speaker.clone();
                record.speaker_source = span.speaker.clone();
                record.language = span.language.clone();
                record.text_digest = match (span.text_digest.as_deref(), span.text.as_deref()) {
                    (Some(digest), _) => Some(parse_digest(digest)?),
                    (None, Some(text)) => {
                        Some(Digest::hash(loom.store().digest_algo(), text.as_bytes()))
                    }
                    (None, None) => None,
                };
                if let Some(text) = &span.text {
                    let line = serde_json::json!({
                        "span_id": &span_id,
                        "locator": &locator,
                        "speaker": &span.speaker,
                        "language": &span.language,
                        "text": text,
                    });
                    serde_json::to_writer(&mut transcript_jsonl, &line)
                        .map_err(|e| LoomError::invalid(e.to_string()))?;
                    transcript_jsonl.push(b'\n');
                }
                spans.push(record);
            }
            if !transcript_jsonl.is_empty() {
                payloads.push(MeetingsImportPayload {
                    path: meetings_source_payload_path(
                        profile_id,
                        &item.source_entity_id,
                        "transcript.jsonl",
                    ),
                    bytes: transcript_jsonl,
                });
            }
        }
        append_import_annotations(
            ImportAnnotationContext {
                loom,
                item,
                meeting_id: &meeting_id,
                source_id: &source_id,
                observed_at: input.observed_at,
            },
            &mut spans,
            &mut annotations,
        )?;
    }
    let mut import_run = ImportRunRecord::new(
        format!(
            "{}:{}:{}",
            input_profile_label(input_profile),
            input.source_scope,
            input.observed_at
        ),
        input_profile,
        input.source_scope.clone(),
        coverage,
        input.observed_at,
    )?;
    import_run.completed_at_ms = Some(input.observed_at);
    import_run.source_cursor = input.source_cursor.clone();
    import_run.source_sidecar_digest = input
        .source_sidecar_digest
        .as_deref()
        .map(parse_digest)
        .transpose()?;
    import_run.observed_ids = observed_ids;
    import_run.coverage_gaps = input.coverage_gaps.clone().unwrap_or_default();
    import_run.retry_windows = input.retry_windows.clone().unwrap_or_default();
    import_run.resume_state = input.resume_state.clone();
    let mut checkpoint = ImportCheckpoint::new(
        import_run.import_run_id.clone(),
        "meetings",
        input.source_scope.clone(),
        input.resume_state.clone().unwrap_or_default().into_bytes(),
    )?;
    checkpoint.observed_ids = import_run.observed_ids.clone();
    checkpoint.completed_units = import_run
        .observed_ids
        .iter()
        .map(|source_id| format!("source:{source_id}"))
        .collect();
    checkpoint.coverage_gaps = import_run.coverage_gaps.clone();
    checkpoint.retry_windows = import_run.retry_windows.clone();
    let rows_imported = input.items.len() as u64;
    let operations_planned = sources.len() as u64
        + meetings.len() as u64
        + spans.len() as u64
        + annotations.len() as u64
        + 1;
    let snapshot = MeetingsProfileSnapshot::new(
        profile_id,
        MeetingsProfileSnapshotParts {
            sources: merge_by(Vec::new(), sources, |source| &source.source_id),
            meetings: merge_by(Vec::new(), meetings, |meeting| &meeting.meeting_id),
            spans: merge_by(Vec::new(), spans, |span| &span.span_id),
            annotations: merge_by(Vec::new(), annotations, |annotation| {
                &annotation.annotation_id
            }),
            vocabulary_terms: Vec::new(),
            entity_merges: Vec::new(),
            promotions: Vec::new(),
            import_runs: vec![import_run],
            redactions: Vec::new(),
        },
    )?;
    Ok(ParsedMeetingsImport {
        snapshot,
        checkpoint,
        payloads,
        source_scope: input.source_scope,
        rows_imported,
        operations_planned,
    })
}

fn materialize_meetings_import_payloads(
    loom: &mut Loom<FileStore>,
    workspace_id: WorkspaceId,
    payloads: &[MeetingsImportPayload],
) -> Result<u64> {
    let mut bytes = 0u64;
    for payload in payloads {
        if let Some(parent) = payload.path.rsplit_once('/').map(|(parent, _)| parent) {
            loom.create_directory_reserved(workspace_id, parent, true)?;
        }
        loom.write_file_reserved(workspace_id, &payload.path, &payload.bytes, 0o100644)?;
        bytes = bytes.saturating_add(payload.bytes.len() as u64);
    }
    Ok(bytes)
}

pub fn meetings_source_payload_path(profile_id: &str, source_id: &str, leaf: &str) -> String {
    format!(
        ".loom/meetings/{}/sources/{}/{}",
        path_token(profile_id),
        path_token(source_id),
        leaf
    )
}

pub fn meetings_import_checkpoint_key(profile_id: &str, checkpoint_id: &str) -> Vec<u8> {
    format!(
        "interchange/checkpoints/meetings/{}/{}",
        path_token(profile_id),
        path_token(checkpoint_id)
    )
    .into_bytes()
}

pub fn validate_meetings_source_payload_leaf(leaf: &str) -> Result<()> {
    match leaf {
        "source.json" | "summary.txt" | "transcript.jsonl" => Ok(()),
        other => Err(LoomError::invalid(format!(
            "unsupported meetings source payload leaf {other:?}; supported leaves: source.json, summary.txt, transcript.jsonl"
        ))),
    }
}

fn path_token(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        let ch = byte as char;
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
            out.push(ch);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

struct ImportAnnotationContext<'a> {
    loom: &'a Loom<FileStore>,
    item: &'a MeetingsImportItemJson,
    meeting_id: &'a str,
    source_id: &'a str,
    observed_at: u64,
}

fn append_import_annotations(
    context: ImportAnnotationContext<'_>,
    spans: &mut Vec<SpanRecord>,
    annotations: &mut Vec<AnnotationRecord>,
) -> Result<()> {
    let item = context.item;
    if let Some(items) = &item.annotations {
        for (index, annotation) in items.iter().enumerate() {
            let source_span_ids = source_span_ids_for_annotation(
                &context,
                "annotation",
                index,
                annotation.source_span_ids.as_deref(),
                spans,
            )?;
            let annotation_id = annotation
                .annotation_id
                .clone()
                .unwrap_or_else(|| format!("annotation/{}/{index}", item.source_entity_id));
            let mut record = import_annotation_record(
                annotation_id,
                context.meeting_id,
                source_span_ids,
                &annotation.kind,
                &annotation.label,
                context.observed_at,
            )?;
            record.normalized_id = annotation.normalized_id.clone();
            record.confidence_ppm = annotation.confidence_ppm;
            record.evidence_digest = annotation
                .evidence_digest
                .as_deref()
                .map(parse_digest)
                .transpose()?;
            record.extractor = annotation.extractor.clone();
            annotations.push(record);
        }
    }
    append_import_structured_annotations(
        &context,
        spans,
        annotations,
        "Task",
        "tasks",
        item.tasks.as_deref(),
    )?;
    append_import_structured_annotations(
        &context,
        spans,
        annotations,
        "Task",
        "action_items",
        item.action_items.as_deref(),
    )?;
    append_import_structured_annotations(
        &context,
        spans,
        annotations,
        "Topic",
        "topics",
        item.topics.as_deref(),
    )?;
    append_import_structured_annotations(
        &context,
        spans,
        annotations,
        "Decision",
        "decisions",
        item.decisions.as_deref(),
    )?;
    append_import_structured_annotations(
        &context,
        spans,
        annotations,
        "Question",
        "questions",
        item.questions.as_deref(),
    )?;
    append_import_structured_annotations(
        &context,
        spans,
        annotations,
        "Risk",
        "risks",
        item.risks.as_deref(),
    )?;
    append_import_structured_annotations(
        &context,
        spans,
        annotations,
        "Artifact",
        "artifacts",
        item.artifacts.as_deref(),
    )?;
    append_import_structured_annotations(
        &context,
        spans,
        annotations,
        "Reference",
        "references",
        item.references.as_deref(),
    )
}

fn append_import_structured_annotations(
    context: &ImportAnnotationContext<'_>,
    spans: &mut Vec<SpanRecord>,
    annotations: &mut Vec<AnnotationRecord>,
    kind: &str,
    field_name: &str,
    items: Option<&[MeetingsImportStructuredItemJson]>,
) -> Result<()> {
    let Some(items) = items else {
        return Ok(());
    };
    let item = context.item;
    for (index, structured) in items.iter().enumerate() {
        let source_span_ids = source_span_ids_for_annotation(
            context,
            field_name,
            index,
            structured.source_span_ids.as_deref(),
            spans,
        )?;
        let annotation_id = structured
            .id
            .clone()
            .unwrap_or_else(|| format!("{field_name}/{}/{index}", item.source_entity_id));
        let mut record = import_annotation_record(
            annotation_id,
            context.meeting_id,
            source_span_ids,
            kind,
            &structured.label,
            context.observed_at,
        )?;
        record.normalized_id = structured.normalized_id.clone();
        record.confidence_ppm = structured.confidence_ppm;
        record.evidence_digest = structured
            .evidence_digest
            .as_deref()
            .map(parse_digest)
            .transpose()?;
        record.extractor = structured.extractor.clone();
        annotations.push(record);
    }
    Ok(())
}

fn source_span_ids_for_annotation(
    context: &ImportAnnotationContext<'_>,
    field_name: &str,
    index: usize,
    explicit_ids: Option<&[String]>,
    spans: &mut Vec<SpanRecord>,
) -> Result<Vec<String>> {
    if let Some(ids) = explicit_ids
        && !ids.is_empty()
    {
        return Ok(ids.to_vec());
    }
    let item = context.item;
    let span_id = format!(
        "span/{}/metadata/{field_name}/{index}",
        item.source_entity_id
    );
    if !spans.iter().any(|span| span.span_id == span_id) {
        let mut span = SpanRecord::new(
            span_id.clone(),
            context.meeting_id.to_string(),
            context.source_id.to_string(),
            SpanKind::MetadataField,
            format!("metadata/{field_name}/{index}"),
        )?;
        span.text_digest = Some(Digest::hash(
            context.loom.store().digest_algo(),
            format!("{field_name}:{index}").as_bytes(),
        ));
        spans.push(span);
    }
    Ok(vec![span_id])
}

fn import_annotation_record(
    annotation_id: String,
    meeting_id: &str,
    source_span_ids: Vec<String>,
    kind: &str,
    label: &str,
    observed_at: u64,
) -> Result<AnnotationRecord> {
    let mut record = AnnotationRecord::new(
        annotation_id,
        meeting_id.to_string(),
        source_span_ids,
        kind,
        label,
        observed_at,
    )?;
    record.status = AnnotationStatus::Observed;
    Ok(record)
}

fn merge_meetings_snapshot(
    existing: MeetingsProfileSnapshot,
    incoming: MeetingsProfileSnapshot,
) -> Result<MeetingsProfileSnapshot> {
    if existing.workspace_id != incoming.workspace_id {
        return Err(LoomError::new(
            Code::Conflict,
            "meetings snapshot workspace mismatch",
        ));
    }
    MeetingsProfileSnapshot::new(
        existing.workspace_id,
        MeetingsProfileSnapshotParts {
            sources: merge_by(existing.sources, incoming.sources, |source| {
                &source.source_id
            }),
            meetings: merge_by(existing.meetings, incoming.meetings, |meeting| {
                &meeting.meeting_id
            }),
            spans: merge_by(existing.spans, incoming.spans, |span| &span.span_id),
            annotations: merge_by(existing.annotations, incoming.annotations, |annotation| {
                &annotation.annotation_id
            }),
            vocabulary_terms: merge_by(
                existing.vocabulary_terms,
                incoming.vocabulary_terms,
                |term| &term.term_id,
            ),
            entity_merges: merge_by(existing.entity_merges, incoming.entity_merges, |merge| {
                &merge.merge_id
            }),
            promotions: merge_by(existing.promotions, incoming.promotions, |promotion| {
                &promotion.promotion_id
            }),
            import_runs: merge_by(existing.import_runs, incoming.import_runs, |run| {
                &run.import_run_id
            }),
            redactions: merge_by(existing.redactions, incoming.redactions, |redaction| {
                &redaction.redaction_id
            }),
        },
    )
}

fn update_meetings_revision_index(
    loom: &mut Loom<FileStore>,
    workspace_id: WorkspaceId,
    profile_id: &str,
    previous: Option<&MeetingsProfileSnapshot>,
    next: &MeetingsProfileSnapshot,
    snapshot_bytes: &[u8],
) -> Result<()> {
    let previous_meetings = previous
        .map(|snapshot| {
            snapshot
                .meetings
                .iter()
                .map(|meeting| Ok((meeting.meeting_id.as_str(), meeting.encode()?)))
                .collect::<Result<BTreeMap<_, _>>>()
        })
        .transpose()?
        .unwrap_or_default();
    let index_path = revision_index_path(profile_id)?;
    let index = match loom.read_file_reserved(workspace_id, &index_path) {
        Ok(bytes) => RevisionIndex::decode(&bytes)?,
        Err(err) if err.code == Code::NotFound => RevisionIndex::new(),
        Err(err) => return Err(err),
    };
    let root = Digest::hash(loom.store().digest_algo(), snapshot_bytes);
    let mut state = ProfileTransactionState::new(root, index);
    let mut updates = Vec::new();
    let mut changed = false;
    for meeting in &next.meetings {
        let body = meeting.encode()?;
        if previous_meetings
            .get(meeting.meeting_id.as_str())
            .is_some_and(|previous| previous == &body)
        {
            continue;
        }
        let entity_id = format!("meeting:{}", meeting.meeting_id);
        let expected_latest_revision = state
            .revision_index()
            .latest(&entity_id)
            .map(|entry| entry.revision)
            .unwrap_or(0);
        let revision = expected_latest_revision.saturating_add(1);
        let operation_id = format!("meetings:{profile_id}:{}:{revision}", meeting.meeting_id);
        let update = ProfileRevisionUpdate::new(
            entity_id,
            operation_id.clone(),
            BodyRef::new(
                Digest::hash(loom.store().digest_algo(), &body),
                body.len() as u64,
                "application/vnd.uldren.loom.meetings.meeting+cbor",
            )?,
            meeting.updated_at_ms,
            format!("meeting:{}:{revision}", meeting.meeting_id),
            Some(expected_latest_revision),
        )?;
        updates.push(update);
        changed = true;
    }
    if changed {
        state.apply(ProfileTransaction::new(profile_id, None, root, updates)?)?;
        loom.create_directory_reserved(workspace_id, REVISION_INDEX_DIR, true)?;
        let index_bytes = state.into_revision_index().encode()?;
        loom.write_file_reserved(workspace_id, &index_path, &index_bytes, 0o100644)?;
    }
    Ok(())
}

fn merge_by<T, F>(existing: Vec<T>, incoming: Vec<T>, id: F) -> Vec<T>
where
    F: Fn(&T) -> &str,
{
    let mut records = BTreeMap::new();
    for item in existing {
        records.insert(id(&item).to_string(), item);
    }
    for item in incoming {
        records.insert(id(&item).to_string(), item);
    }
    records.into_values().collect()
}

pub fn parse_meetings_input_profile(value: &str) -> Result<InputProfile> {
    match value {
        "generic" => Ok(InputProfile::Generic),
        "granola-api" => Ok(InputProfile::GranolaApi),
        "granola-app" => Ok(InputProfile::GranolaApp),
        "granola-mcp" => Ok(InputProfile::GranolaMcp),
        "csv" => Ok(InputProfile::Csv),
        other => Err(LoomError::invalid(format!(
            "unsupported meetings input profile {other:?}; expected generic, granola-api, granola-app, granola-mcp, or csv"
        ))),
    }
}

pub fn input_profile_label(profile: InputProfile) -> &'static str {
    match profile {
        InputProfile::Generic => "generic",
        InputProfile::GranolaApi => "granola-api",
        InputProfile::GranolaApp => "granola-app",
        InputProfile::GranolaMcp => "granola-mcp",
        InputProfile::Csv => "csv",
    }
}

fn parse_meetings_coverage(value: &str) -> Result<MeetingsCoverage> {
    match value {
        "complete" => Ok(MeetingsCoverage::Complete),
        "partial" => Ok(MeetingsCoverage::Partial),
        "degraded" => Ok(MeetingsCoverage::Degraded),
        other => Err(LoomError::invalid(format!(
            "unsupported meetings coverage {other:?}; expected complete, partial, or degraded"
        ))),
    }
}

fn parse_meetings_source_state(value: &str) -> Result<MeetingStatus> {
    match value {
        "active" => Ok(MeetingStatus::Active),
        "deleted_at_source" => Ok(MeetingStatus::DeletedAtSource),
        "redacted" => Ok(MeetingStatus::Redacted),
        "retained_metadata_only" => Ok(MeetingStatus::RetainedMetadataOnly),
        other => Err(LoomError::invalid(format!(
            "unsupported meetings source_state {other:?}; expected active, deleted_at_source, redacted, or retained_metadata_only"
        ))),
    }
}

fn parse_digest(value: &str) -> Result<Digest> {
    Digest::parse(value)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FsImportOptions {
    pub profile: String,
    pub source_scope: String,
    pub author: String,
    pub message: String,
    pub commit: bool,
    pub dry_run: bool,
}

impl FsImportOptions {
    pub fn new(source_scope: impl Into<String>) -> Self {
        Self {
            profile: "fs".to_string(),
            source_scope: source_scope.into(),
            author: "loom-interchange".to_string(),
            message: "import filesystem".to_string(),
            commit: false,
            dry_run: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FsExportOptions {
    pub profile: String,
    pub destination_scope: String,
    pub dry_run: bool,
    pub revision: Option<String>,
}

impl FsExportOptions {
    pub fn new(destination_scope: impl Into<String>) -> Self {
        Self {
            profile: "fs".to_string(),
            destination_scope: destination_scope.into(),
            dry_run: false,
            revision: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveImportOptions {
    pub profile: String,
    pub source_scope: String,
    pub archive_id: String,
    pub author: String,
    pub message: String,
    pub commit: bool,
    pub dry_run: bool,
    pub gzip_output_path: Option<String>,
}

impl ArchiveImportOptions {
    pub fn new(source_scope: impl Into<String>) -> Self {
        let source_scope = source_scope.into();
        Self {
            profile: "archive".to_string(),
            archive_id: source_scope.clone(),
            source_scope,
            author: "loom-interchange".to_string(),
            message: "import archive".to_string(),
            commit: false,
            dry_run: false,
            gzip_output_path: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveImportResult {
    pub manifest: ArchiveManifest,
    pub report: ImportReport,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveExportOptions {
    pub profile: String,
    pub destination_scope: String,
    pub dry_run: bool,
    pub revision: Option<String>,
}

impl ArchiveExportOptions {
    pub fn new(destination_scope: impl Into<String>) -> Self {
        Self {
            profile: "archive".to_string(),
            destination_scope: destination_scope.into(),
            dry_run: false,
            revision: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveExportResult {
    pub manifest: ArchiveManifest,
    pub report: ExportReport,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CarExportOptions {
    pub profile: String,
    pub destination_scope: String,
    pub dry_run: bool,
}

impl CarExportOptions {
    pub fn new(destination_scope: impl Into<String>) -> Self {
        Self {
            profile: "car".to_string(),
            destination_scope: destination_scope.into(),
            dry_run: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CarImportOptions {
    pub profile: String,
    pub source_scope: String,
    pub dry_run: bool,
}

impl CarImportOptions {
    pub fn new(source_scope: impl Into<String>) -> Self {
        Self {
            profile: "car".to_string(),
            source_scope: source_scope.into(),
            dry_run: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CarExportResult {
    pub root_cid_hex: String,
    pub blocks_written: u64,
    pub bytes_out: u64,
    pub report: ExportReport,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CarImportResult {
    pub workspace: Option<WorkspaceId>,
    pub root_cid_hex: String,
    pub blocks_read: u64,
    pub report: ImportReport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableImportMode {
    Snapshot,
    AppendOnly,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableCsvImportOptions {
    pub profile: String,
    pub source_scope: String,
    pub database: String,
    pub table: String,
    pub columns: Vec<(String, ColumnType)>,
    pub primary_key: Vec<String>,
    pub mode: TableImportMode,
    pub author: String,
    pub message: String,
    pub commit: bool,
    pub dry_run: bool,
}

impl TableCsvImportOptions {
    pub fn new(
        source_scope: impl Into<String>,
        database: impl Into<String>,
        table: impl Into<String>,
        columns: Vec<(String, ColumnType)>,
        primary_key: Vec<String>,
    ) -> Self {
        Self {
            profile: "table-csv".to_string(),
            source_scope: source_scope.into(),
            database: database.into(),
            table: table.into(),
            columns,
            primary_key,
            mode: TableImportMode::Snapshot,
            author: "loom-interchange".to_string(),
            message: "import table csv".to_string(),
            commit: false,
            dry_run: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableCsvExportOptions {
    pub profile: String,
    pub destination_scope: String,
    pub database: String,
    pub table: String,
    pub dry_run: bool,
}

impl TableCsvExportOptions {
    pub fn new(
        destination_scope: impl Into<String>,
        database: impl Into<String>,
        table: impl Into<String>,
    ) -> Self {
        Self {
            profile: "table-csv".to_string(),
            destination_scope: destination_scope.into(),
            database: database.into(),
            table: table.into(),
            dry_run: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableCsvExportBytesResult {
    pub bytes: Vec<u8>,
    pub report: ExportReport,
}

pub fn import_table_csv<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    src: &Path,
    options: &TableCsvImportOptions,
) -> Result<ImportReport> {
    let bytes = fs::read(src).map_err(|e| {
        LoomError::new(
            Code::Io,
            format!("read table CSV import {}: {e}", src.display()),
        )
    })?;
    import_table_csv_bytes(loom, ns, &bytes, options)
}

pub fn import_table_csv_bytes<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    bytes: &[u8],
    options: &TableCsvImportOptions,
) -> Result<ImportReport> {
    loom.authorize(ns, FacetKind::Sql, AclRight::Write)?;
    let objects_before = loom.store().len();
    let schema = table_csv_schema(options)?;
    let rows = parse_table_csv_rows(bytes, &schema)?;
    let table_path = table_csv_path(&options.database, &options.table)?;
    let mut table = match options.mode {
        TableImportMode::Snapshot => Table::new(schema),
        TableImportMode::AppendOnly => match get_table(loom, ns, &table_path) {
            Ok(existing) => {
                if existing.schema() != &schema {
                    return Err(LoomError::new(
                        Code::Conflict,
                        "append-only table CSV import schema mismatch",
                    ));
                }
                existing
            }
            Err(err) if err.code == Code::NotFound => Table::new(schema),
            Err(err) => return Err(err),
        },
    };
    let mut report = ImportReport::new(ImportReportInput {
        profile: &options.profile,
        source_scope: &options.source_scope,
        commit: None,
        objects_added: 0,
        bytes_in: bytes.len() as u64,
        bytes_stored: 0,
        rows_imported: rows.len() as u64,
        skipped: 0,
        operations_planned: rows.len() as u64,
        operations_applied: 0,
        dry_run: options.dry_run,
    })?;
    for row in rows {
        if matches!(options.mode, TableImportMode::AppendOnly) {
            let pk = table_primary_key(table.schema(), &row);
            if table.get(&pk).is_some() {
                return Err(LoomError::new(
                    Code::AlreadyExists,
                    "append-only table CSV import found an existing primary key",
                ));
            }
        }
        table.insert(row)?;
    }
    report.bytes_stored = table.encode().len() as u64;
    if !options.dry_run {
        loom.registry_mut().add_facet(ns, FacetKind::Sql)?;
        put_table(loom, ns, &table_path, &table)?;
        report.operations_applied = report.operations_planned;
        if options.commit {
            report.commit = Some(loom.commit(ns, &options.author, &options.message, now_ms())?);
        }
        report.objects_added = object_count_delta(objects_before, loom.store().len());
    }
    Ok(report)
}

pub fn export_table_csv<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    dst: &Path,
    options: &TableCsvExportOptions,
) -> Result<ExportReport> {
    let result = export_table_csv_bytes(loom, ns, options)?;
    if !options.dry_run {
        if let Some(parent) = dst.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent).map_err(|e| {
                LoomError::new(
                    Code::Io,
                    format!(
                        "create table CSV export directory {}: {e}",
                        parent.display()
                    ),
                )
            })?;
        }
        fs::write(dst, &result.bytes).map_err(|e| {
            LoomError::new(
                Code::Io,
                format!("write table CSV export {}: {e}", dst.display()),
            )
        })?;
    }
    Ok(result.report)
}

pub fn export_table_csv_bytes<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    options: &TableCsvExportOptions,
) -> Result<TableCsvExportBytesResult> {
    loom.authorize(ns, FacetKind::Sql, AclRight::Read)?;
    let table_path = table_csv_path(&options.database, &options.table)?;
    let table = get_table(loom, ns, &table_path)?;
    let bytes = write_table_csv(&table)?;
    let mut report = ExportReport::new(&options.profile, &options.destination_scope)?;
    report.dry_run = options.dry_run;
    report.rows_written = table.len() as u64;
    report.bytes_out = bytes.len() as u64;
    Ok(TableCsvExportBytesResult { bytes, report })
}

pub fn export_car<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    dst: &Path,
    options: &CarExportOptions,
) -> Result<CarExportResult> {
    let result = export_car_bytes(loom, ns, options)?;
    if !options.dry_run {
        if let Some(parent) = dst.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent).map_err(|e| {
                LoomError::new(
                    Code::Io,
                    format!("create CAR output directory {}: {e}", parent.display()),
                )
            })?;
        }
        fs::write(dst, &result.bytes).map_err(|e| {
            LoomError::new(Code::Io, format!("write CAR export {}: {e}", dst.display()))
        })?;
    }
    Ok(CarExportResult {
        root_cid_hex: result.root_cid_hex,
        blocks_written: result.blocks_written,
        bytes_out: result.bytes_out,
        report: result.report,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CarExportBytesResult {
    pub root_cid_hex: String,
    pub blocks_written: u64,
    pub bytes_out: u64,
    pub bytes: Vec<u8>,
    pub report: ExportReport,
}

pub fn export_car_bytes<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    options: &CarExportOptions,
) -> Result<CarExportBytesResult> {
    let bundle = bundle_export(loom, ns)?;
    let manifest = encode_car_manifest(&bundle)?;
    let root_digest = Digest::hash(bundle.digest_algo, &manifest);
    let root_cid = cid_bytes(root_digest);
    let mut bytes = Vec::new();
    write_car_record(&mut bytes, &encode_car_header(&root_cid)?)?;
    write_car_block(&mut bytes, &root_cid, &manifest)?;
    for object in &bundle.objects {
        let digest = Digest::hash(bundle.digest_algo, object);
        write_car_block(&mut bytes, &cid_bytes(digest), object)?;
    }
    let mut report = ExportReport::new(&options.profile, &options.destination_scope)?;
    report.dry_run = options.dry_run;
    report.rows_written = bundle.objects.len() as u64 + 1;
    report.bytes_out = bytes.len() as u64;
    Ok(CarExportBytesResult {
        root_cid_hex: hex_lower(&root_cid),
        blocks_written: bundle.objects.len() as u64 + 1,
        bytes_out: bytes.len() as u64,
        bytes,
        report,
    })
}

pub fn import_car<S: ObjectStore>(
    loom: &mut Loom<S>,
    src: &Path,
    options: &CarImportOptions,
) -> Result<CarImportResult> {
    let bytes = fs::read(src)
        .map_err(|e| LoomError::new(Code::Io, format!("read CAR import {}: {e}", src.display())))?;
    import_car_bytes(loom, &bytes, options)
}

pub fn import_car_bytes<S: ObjectStore>(
    loom: &mut Loom<S>,
    bytes: &[u8],
    options: &CarImportOptions,
) -> Result<CarImportResult> {
    let mut cursor = 0;
    let header = read_car_record(bytes, &mut cursor)?;
    let roots = decode_car_header(&header)?;
    if roots.len() != 1 {
        return Err(LoomError::invalid(
            "Loom CAR profile requires exactly one root",
        ));
    }
    let root_cid = roots[0].clone();
    let mut root_block = None;
    let mut blocks = Vec::new();
    while cursor < bytes.len() {
        let record = read_car_record(bytes, &mut cursor)?;
        let (cid, block) = split_car_block(&record)?;
        let digest = digest_from_cid(&cid)?;
        if Digest::hash(digest.algo(), &block) != digest {
            return Err(LoomError::corrupt("CAR block digest does not match CID"));
        }
        if cid == root_cid {
            root_block = Some(block);
        } else {
            blocks.push((cid, block));
        }
    }
    let Some(root_block) = root_block else {
        return Err(LoomError::corrupt("CAR root block missing"));
    };
    let manifest = decode_car_manifest(&root_block)?;
    if manifest.root_cid != root_cid {
        return Err(LoomError::corrupt(
            "CAR header root does not match Loom manifest root",
        ));
    }
    loom.authorize(manifest.ns_id, FacetKind::Vcs, AclRight::Write)?;
    let mut report = ImportReport::new(ImportReportInput {
        profile: &options.profile,
        source_scope: &options.source_scope,
        commit: None,
        objects_added: 0,
        bytes_in: bytes.len() as u64,
        bytes_stored: 0,
        rows_imported: 0,
        skipped: 0,
        operations_planned: manifest.object_cids.len() as u64,
        operations_applied: 0,
        dry_run: options.dry_run,
    })?;
    let mut object_bytes = Vec::with_capacity(manifest.object_cids.len());
    for expected in &manifest.object_cids {
        let Some((_, block)) = blocks.iter().find(|(cid, _)| cid == expected) else {
            return Err(LoomError::corrupt("CAR object block missing"));
        };
        object_bytes.push(block.clone());
        report.bytes_stored += block.len() as u64;
    }
    let workspace = if options.dry_run {
        None
    } else {
        let bundle = Bundle {
            digest_algo: manifest.digest_algo,
            ns_id: manifest.ns_id,
            facets: manifest.facets,
            ns_name: manifest.ns_name,
            branches: manifest.branches,
            tags: manifest.tags,
            objects: object_bytes,
        };
        let (ns, sync) = bundle_import(loom, &bundle)?;
        report.objects_added = sync.objects_transferred;
        report.skipped = sync.objects_skipped;
        report.operations_applied = report.operations_planned;
        Some(ns)
    };
    Ok(CarImportResult {
        workspace,
        root_cid_hex: hex_lower(&root_cid),
        blocks_read: blocks.len() as u64 + 1,
        report,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CarManifest {
    root_cid: Vec<u8>,
    digest_algo: loom_types::Algo,
    ns_id: WorkspaceId,
    facets: Vec<FacetKind>,
    ns_name: String,
    branches: Vec<(String, Digest)>,
    tags: Vec<(String, Digest)>,
    object_cids: Vec<Vec<u8>>,
}

const CAR_HEADER_VERSION: u64 = 1;
const CAR_MANIFEST_MAGIC: &str = "LMCAR";
const CAR_MANIFEST_VERSION: u64 = 1;
const CID_VERSION: u64 = 1;
const CID_RAW_CODEC: u64 = 0x55;

fn encode_car_header(root_cid: &[u8]) -> Result<Vec<u8>> {
    loom_codec::encode(&CborValue::Map(vec![
        (
            CborValue::Text("roots".to_string()),
            CborValue::Array(vec![CborValue::Bytes(root_cid.to_vec())]),
        ),
        (
            CborValue::Text("version".to_string()),
            CborValue::Uint(CAR_HEADER_VERSION),
        ),
    ]))
    .map_err(codec_error)
}

fn decode_car_header(bytes: &[u8]) -> Result<Vec<Vec<u8>>> {
    let CborValue::Map(entries) = loom_codec::decode(bytes).map_err(codec_error)? else {
        return Err(LoomError::corrupt("CAR header is not a map"));
    };
    let mut roots = None;
    let mut version = None;
    for (key, value) in entries {
        let CborValue::Text(key) = key else {
            return Err(LoomError::corrupt("CAR header key is not text"));
        };
        match key.as_str() {
            "roots" => {
                let CborValue::Array(values) = value else {
                    return Err(LoomError::corrupt("CAR roots is not an array"));
                };
                let mut decoded = Vec::with_capacity(values.len());
                for value in values {
                    let CborValue::Bytes(cid) = value else {
                        return Err(LoomError::corrupt("CAR root is not a CID byte string"));
                    };
                    decoded.push(cid);
                }
                roots = Some(decoded);
            }
            "version" => {
                let CborValue::Uint(value) = value else {
                    return Err(LoomError::corrupt("CAR version is not an integer"));
                };
                version = Some(value);
            }
            _ => {}
        }
    }
    if version != Some(CAR_HEADER_VERSION) {
        return Err(LoomError::corrupt("unsupported CAR header version"));
    }
    roots.ok_or_else(|| LoomError::corrupt("CAR header missing roots"))
}

fn encode_car_manifest(bundle: &Bundle) -> Result<Vec<u8>> {
    let object_cids = bundle
        .objects
        .iter()
        .map(|object| CborValue::Bytes(cid_bytes(Digest::hash(bundle.digest_algo, object))))
        .collect();
    loom_codec::encode(&CborValue::Array(vec![
        CborValue::Text(CAR_MANIFEST_MAGIC.to_string()),
        CborValue::Uint(CAR_MANIFEST_VERSION),
        CborValue::Uint(u64::from(bundle.digest_algo.code())),
        CborValue::Bytes(bundle.ns_id.as_bytes().to_vec()),
        CborValue::Array(
            bundle
                .facets
                .iter()
                .map(|facet| CborValue::Uint(u64::from(facet.stable_tag())))
                .collect(),
        ),
        CborValue::Text(bundle.ns_name.clone()),
        refs_to_value(&bundle.branches),
        refs_to_value(&bundle.tags),
        CborValue::Array(object_cids),
    ]))
    .map_err(codec_error)
}

fn decode_car_manifest(bytes: &[u8]) -> Result<CarManifest> {
    let CborValue::Array(values) = loom_codec::decode(bytes).map_err(codec_error)? else {
        return Err(LoomError::corrupt("Loom CAR manifest is not an array"));
    };
    let mut fields = values.into_iter();
    match fields.next() {
        Some(CborValue::Text(value)) if value == CAR_MANIFEST_MAGIC => {}
        _ => return Err(LoomError::corrupt("Loom CAR manifest has bad magic")),
    }
    match fields.next() {
        Some(CborValue::Uint(CAR_MANIFEST_VERSION)) => {}
        _ => return Err(LoomError::corrupt("unsupported Loom CAR manifest version")),
    }
    let digest_algo = match fields.next() {
        Some(CborValue::Uint(value)) => loom_types::Algo::from_code(
            u8::try_from(value)
                .map_err(|_| LoomError::corrupt("manifest digest algorithm out of range"))?,
        )?,
        _ => return Err(LoomError::corrupt("manifest missing digest algorithm")),
    };
    let ns_id = match fields.next() {
        Some(CborValue::Bytes(bytes)) => WorkspaceId::from_bytes(
            bytes
                .as_slice()
                .try_into()
                .map_err(|_| LoomError::corrupt("manifest workspace id is not 16 bytes"))?,
        ),
        _ => return Err(LoomError::corrupt("manifest missing workspace id")),
    };
    let facets = match fields.next() {
        Some(CborValue::Array(values)) => values
            .into_iter()
            .map(|value| match value {
                CborValue::Uint(tag) => FacetKind::from_stable_tag(
                    u8::try_from(tag)
                        .map_err(|_| LoomError::corrupt("manifest facet tag out of range"))?,
                )
                .ok_or_else(|| LoomError::corrupt("manifest facet tag is unknown")),
                _ => Err(LoomError::corrupt("manifest facet tag is not an integer")),
            })
            .collect::<Result<Vec<_>>>()?,
        _ => return Err(LoomError::corrupt("manifest missing facets")),
    };
    let ns_name = match fields.next() {
        Some(CborValue::Text(value)) => value,
        _ => return Err(LoomError::corrupt("manifest missing workspace name")),
    };
    let branches = refs_from_value(
        fields
            .next()
            .ok_or_else(|| LoomError::corrupt("manifest missing branches"))?,
    )?;
    let tags = refs_from_value(
        fields
            .next()
            .ok_or_else(|| LoomError::corrupt("manifest missing tags"))?,
    )?;
    let object_cids = match fields.next() {
        Some(CborValue::Array(values)) => values
            .into_iter()
            .map(|value| match value {
                CborValue::Bytes(cid) => {
                    digest_from_cid(&cid)?;
                    Ok(cid)
                }
                _ => Err(LoomError::corrupt("manifest object CID is not bytes")),
            })
            .collect::<Result<Vec<_>>>()?,
        _ => return Err(LoomError::corrupt("manifest missing object CIDs")),
    };
    if fields.next().is_some() {
        return Err(LoomError::corrupt("manifest has trailing fields"));
    }
    Ok(CarManifest {
        root_cid: cid_bytes(Digest::hash(digest_algo, bytes)),
        digest_algo,
        ns_id,
        facets,
        ns_name,
        branches,
        tags,
        object_cids,
    })
}

fn refs_to_value(refs: &[(String, Digest)]) -> CborValue {
    CborValue::Array(
        refs.iter()
            .map(|(name, digest)| {
                CborValue::Array(vec![
                    CborValue::Text(name.clone()),
                    CborValue::Bytes(cid_bytes(*digest)),
                ])
            })
            .collect(),
    )
}

fn refs_from_value(value: CborValue) -> Result<Vec<(String, Digest)>> {
    let CborValue::Array(values) = value else {
        return Err(LoomError::corrupt("manifest refs are not an array"));
    };
    values
        .into_iter()
        .map(|value| {
            let CborValue::Array(mut pair) = value else {
                return Err(LoomError::corrupt("manifest ref is not an array"));
            };
            if pair.len() != 2 {
                return Err(LoomError::corrupt("manifest ref has wrong field count"));
            }
            let cid_value = pair.pop().unwrap();
            let name_value = pair.pop().unwrap();
            let CborValue::Text(name) = name_value else {
                return Err(LoomError::corrupt("manifest ref name is not text"));
            };
            let CborValue::Bytes(cid) = cid_value else {
                return Err(LoomError::corrupt("manifest ref target is not CID bytes"));
            };
            Ok((name, digest_from_cid(&cid)?))
        })
        .collect()
}

fn cid_bytes(digest: Digest) -> Vec<u8> {
    let mut out = Vec::with_capacity(36);
    write_varint(&mut out, CID_VERSION);
    write_varint(&mut out, CID_RAW_CODEC);
    write_varint(&mut out, u64::from(digest.algo().code()));
    write_varint(&mut out, 32);
    out.extend_from_slice(digest.bytes());
    out
}

fn digest_from_cid(cid: &[u8]) -> Result<Digest> {
    let mut cursor = 0;
    let version = read_varint(cid, &mut cursor)?;
    if version != CID_VERSION {
        return Err(LoomError::corrupt("CID version is not v1"));
    }
    let codec = read_varint(cid, &mut cursor)?;
    if codec != CID_RAW_CODEC {
        return Err(LoomError::corrupt("CID codec is not raw"));
    }
    let algo = loom_types::Algo::from_code(
        u8::try_from(read_varint(cid, &mut cursor)?)
            .map_err(|_| LoomError::corrupt("CID multihash code out of range"))?,
    )?;
    let len = read_varint(cid, &mut cursor)?;
    if len != 32 {
        return Err(LoomError::corrupt("CID digest length is not 32 bytes"));
    }
    let digest: [u8; 32] = cid
        .get(cursor..cursor + 32)
        .ok_or_else(|| LoomError::corrupt("CID digest is truncated"))?
        .try_into()
        .unwrap();
    cursor += 32;
    if cursor != cid.len() {
        return Err(LoomError::corrupt("CID has trailing bytes"));
    }
    Ok(Digest::of(algo, digest))
}

fn write_car_block(out: &mut Vec<u8>, cid: &[u8], block: &[u8]) -> Result<()> {
    let mut record = Vec::with_capacity(cid.len() + block.len());
    record.extend_from_slice(cid);
    record.extend_from_slice(block);
    write_car_record(out, &record)
}

fn write_car_record(out: &mut Vec<u8>, record: &[u8]) -> Result<()> {
    write_varint(out, record.len() as u64);
    out.write_all(record)
        .map_err(|e| LoomError::new(Code::Io, format!("write CAR record: {e}")))
}

fn read_car_record(bytes: &[u8], cursor: &mut usize) -> Result<Vec<u8>> {
    let len = usize::try_from(read_varint(bytes, cursor)?)
        .map_err(|_| LoomError::corrupt("CAR record length is too large"))?;
    let end = cursor
        .checked_add(len)
        .ok_or_else(|| LoomError::corrupt("CAR record length overflow"))?;
    let record = bytes
        .get(*cursor..end)
        .ok_or_else(|| LoomError::corrupt("CAR record is truncated"))?
        .to_vec();
    *cursor = end;
    Ok(record)
}

fn split_car_block(record: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
    let mut cursor = 0;
    read_varint(record, &mut cursor)?;
    read_varint(record, &mut cursor)?;
    read_varint(record, &mut cursor)?;
    let digest_len = usize::try_from(read_varint(record, &mut cursor)?)
        .map_err(|_| LoomError::corrupt("CID digest length too large"))?;
    let cid_len = cursor
        .checked_add(digest_len)
        .ok_or_else(|| LoomError::corrupt("CID length overflow"))?;
    if cid_len > record.len() {
        return Err(LoomError::corrupt("CAR block CID is truncated"));
    }
    Ok((record[..cid_len].to_vec(), record[cid_len..].to_vec()))
}

fn write_varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push((value as u8) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

fn read_varint(bytes: &[u8], cursor: &mut usize) -> Result<u64> {
    let mut shift = 0;
    let mut value = 0u64;
    loop {
        let byte = *bytes
            .get(*cursor)
            .ok_or_else(|| LoomError::corrupt("varint is truncated"))?;
        *cursor += 1;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
        shift += 7;
        if shift >= 64 {
            return Err(LoomError::corrupt("varint is too large"));
        }
    }
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn codec_error(error: loom_codec::CodecError) -> LoomError {
    LoomError::corrupt(format!("canonical CBOR error: {error}"))
}

fn object_count_delta(before: usize, after: usize) -> u64 {
    after.saturating_sub(before).try_into().unwrap_or(u64::MAX)
}

pub fn import_fs<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    src: &Path,
    options: &FsImportOptions,
) -> Result<ImportReport> {
    loom.authorize(ns, FacetKind::Files, AclRight::Write)?;
    let objects_before = loom.store().len();
    let root = src.canonicalize().map_err(|e| {
        LoomError::new(
            Code::Io,
            format!("canonicalize import source {}: {e}", src.display()),
        )
    })?;
    if !root.is_dir() {
        return Err(LoomError::invalid(format!(
            "import source {} is not a directory",
            src.display()
        )));
    }

    let mut entries = Vec::new();
    collect_import_entries(&root, &root, &mut entries)?;
    let mut report = ImportReport::new(ImportReportInput {
        profile: &options.profile,
        source_scope: &options.source_scope,
        commit: None,
        objects_added: 0,
        bytes_in: 0,
        bytes_stored: 0,
        rows_imported: 0,
        skipped: 0,
        operations_planned: entries.len() as u64,
        operations_applied: 0,
        dry_run: options.dry_run,
    })?;

    for entry in &entries {
        match &entry.kind {
            ImportEntryKind::Directory => {
                if !options.dry_run {
                    loom.registry_mut().add_facet(ns, FacetKind::Files)?;
                    loom.create_directory(ns, &entry.loom_path, true)?;
                }
                report.operations_applied += u64::from(!options.dry_run);
            }
            ImportEntryKind::File { host_path } => {
                let bytes = fs::read(host_path).map_err(|e| {
                    LoomError::new(
                        Code::Io,
                        format!("read import file {}: {e}", host_path.display()),
                    )
                })?;
                report.bytes_in += bytes.len() as u64;
                report.bytes_stored += bytes.len() as u64;
                if !options.dry_run {
                    loom.registry_mut().add_facet(ns, FacetKind::Files)?;
                    if let Some(parent) = parent_path(&entry.loom_path) {
                        loom.create_directory(ns, &parent, true)?;
                    }
                    loom.write_file(ns, &entry.loom_path, &bytes, 0o100644)?;
                    report.operations_applied += 1;
                }
            }
        }
    }

    if options.commit && !options.dry_run {
        report.commit = Some(loom.commit(ns, &options.author, &options.message, now_ms())?);
    }
    report.objects_added = object_count_delta(objects_before, loom.store().len());

    Ok(report)
}

pub fn import_archive<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    archive_path: &Path,
    kind: ArchiveKind,
    options: &ArchiveImportOptions,
) -> Result<ArchiveImportResult> {
    let bytes = fs::read(archive_path).map_err(|e| {
        LoomError::new(
            Code::Io,
            format!("read archive import {}: {e}", archive_path.display()),
        )
    })?;
    import_archive_bytes(loom, ns, &bytes, archive_path, kind, options)
}

pub fn import_archive_bytes<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    archive_bytes: &[u8],
    source_path_hint: &Path,
    kind: ArchiveKind,
    options: &ArchiveImportOptions,
) -> Result<ArchiveImportResult> {
    loom.authorize(ns, FacetKind::Files, AclRight::Write)?;
    let objects_before = loom.store().len();
    let root_digest = Digest::blake3(archive_bytes);
    let archive_size = archive_bytes.len() as u64;
    let mut manifest = ArchiveManifest::new(&options.archive_id, kind, root_digest)?;
    let mut report = ImportReport::new(ImportReportInput {
        profile: &options.profile,
        source_scope: &options.source_scope,
        commit: None,
        objects_added: 0,
        bytes_in: archive_size,
        bytes_stored: 0,
        rows_imported: 0,
        skipped: 0,
        operations_planned: 0,
        operations_applied: 0,
        dry_run: options.dry_run,
    })?;

    match kind {
        ArchiveKind::Zip => import_zip_archive_reader(
            loom,
            ns,
            Cursor::new(archive_bytes),
            options,
            &mut manifest,
            &mut report,
        )?,
        ArchiveKind::Tar => import_tar_archive(
            loom,
            ns,
            Cursor::new(archive_bytes),
            options,
            &mut manifest,
            &mut report,
        )?,
        ArchiveKind::Gzip => import_gzip_archive_reader(
            loom,
            ns,
            flate2::read::GzDecoder::new(Cursor::new(archive_bytes)),
            source_path_hint,
            options,
            &mut manifest,
            &mut report,
        )?,
        ArchiveKind::TarGzip => import_tar_archive(
            loom,
            ns,
            flate2::read::GzDecoder::new(Cursor::new(archive_bytes)),
            options,
            &mut manifest,
            &mut report,
        )?,
        ArchiveKind::TarZstd => {
            #[cfg(feature = "zstd")]
            {
                let decoder = zstd::stream::read::Decoder::new(Cursor::new(archive_bytes))
                    .map_err(|e| LoomError::invalid(format!("read zstd archive: {e}")))?;
                import_tar_archive(loom, ns, decoder, options, &mut manifest, &mut report)?
            }
            #[cfg(not(feature = "zstd"))]
            {
                return Err(LoomError::unsupported(
                    "tar-zstd archive import is not supported in this build",
                ));
            }
        }
    }

    if options.commit && !options.dry_run {
        report.commit = Some(loom.commit(ns, &options.author, &options.message, now_ms())?);
    }
    report.objects_added = object_count_delta(objects_before, loom.store().len());

    Ok(ArchiveImportResult { manifest, report })
}

pub fn export_archive<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    archive_path: &Path,
    kind: ArchiveKind,
    options: &ArchiveExportOptions,
) -> Result<ArchiveExportResult> {
    let result = export_archive_bytes(loom, ns, kind, options)?;
    if !options.dry_run {
        if let Some(parent) = archive_path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent).map_err(|e| {
                LoomError::new(
                    Code::Io,
                    format!("create archive output directory {}: {e}", parent.display()),
                )
            })?;
        }
        fs::write(archive_path, &result.bytes).map_err(|e| {
            LoomError::new(
                Code::Io,
                format!("write archive export {}: {e}", archive_path.display()),
            )
        })?;
    }
    Ok(ArchiveExportResult {
        manifest: result.manifest,
        report: result.report,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveExportBytesResult {
    pub manifest: ArchiveManifest,
    pub bytes: Vec<u8>,
    pub report: ExportReport,
}

pub fn export_archive_bytes<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    kind: ArchiveKind,
    options: &ArchiveExportOptions,
) -> Result<ArchiveExportBytesResult> {
    loom.authorize(ns, FacetKind::Files, AclRight::Read)?;
    if matches!(kind, ArchiveKind::Gzip) {
        return Err(LoomError::unsupported(
            "single-file gzip export is not supported; use tar-gzip for a file tree",
        ));
    }
    let entries = archive_source_entries(loom, ns, options.revision.as_deref())?;
    let bytes = match kind {
        ArchiveKind::Tar => export_tar_bytes(&entries)?,
        ArchiveKind::TarGzip => {
            let tar = export_tar_bytes(&entries)?;
            let mut encoder = GzBuilder::new()
                .mtime(0)
                .write(Vec::new(), Compression::default());
            encoder
                .write_all(&tar)
                .map_err(|e| LoomError::new(Code::Io, format!("write tar.gz archive: {e}")))?;
            encoder
                .finish()
                .map_err(|e| LoomError::new(Code::Io, format!("finish tar.gz archive: {e}")))?
        }
        ArchiveKind::TarZstd => {
            #[cfg(feature = "zstd")]
            {
                zstd::stream::encode_all(export_tar_bytes(&entries)?.as_slice(), 0)
                    .map_err(|e| LoomError::new(Code::Io, format!("write tar.zstd archive: {e}")))?
            }
            #[cfg(not(feature = "zstd"))]
            {
                return Err(LoomError::unsupported(
                    "tar-zstd archive export is not supported in this build",
                ));
            }
        }
        ArchiveKind::Zip => export_zip_bytes(&entries)?,
        ArchiveKind::Gzip => unreachable!(),
    };
    let root_digest = Digest::blake3(&bytes);
    let mut manifest = ArchiveManifest::new(&options.destination_scope, kind, root_digest)?;
    let mut report = ExportReport::new(&options.profile, &options.destination_scope)?;
    report.dry_run = options.dry_run;
    report.bytes_out = bytes.len() as u64;
    for entry in &entries {
        manifest.entries.push(entry.manifest_entry()?);
        if matches!(entry.kind, ArchiveEntryKind::File) {
            report.files_written += u64::from(!options.dry_run);
        }
    }
    manifest.validate()?;
    Ok(ArchiveExportBytesResult {
        manifest,
        bytes,
        report,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ArchiveSourceEntry {
    path: String,
    kind: ArchiveEntryKind,
    bytes: Vec<u8>,
}

impl ArchiveSourceEntry {
    fn manifest_entry(&self) -> Result<ArchiveEntry> {
        let mut entry = ArchiveEntry::new(&self.path, self.kind, self.bytes.len() as u64)?;
        if matches!(self.kind, ArchiveEntryKind::File) {
            entry.digest = Some(Digest::blake3(&self.bytes));
        }
        Ok(entry)
    }
}

fn archive_source_entries<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    revision: Option<&str>,
) -> Result<Vec<ArchiveSourceEntry>> {
    let mut entries = Vec::new();
    if let Some(revision) = revision {
        for entry in loom.committed_fs_entries(ns, revision)? {
            match entry.kind {
                FileKind::Directory => entries.push(ArchiveSourceEntry {
                    path: entry.path,
                    kind: ArchiveEntryKind::Directory,
                    bytes: Vec::new(),
                }),
                FileKind::File => entries.push(ArchiveSourceEntry {
                    path: entry.path,
                    kind: ArchiveEntryKind::File,
                    bytes: entry.bytes,
                }),
                FileKind::Symlink => {
                    return Err(LoomError::unsupported(format!(
                        "archive export does not support symlink {}",
                        entry.path
                    )));
                }
            }
        }
    } else {
        collect_archive_source_entries(loom, ns, "", &mut entries)?;
    }
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(entries)
}

fn collect_archive_source_entries<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    dir: &str,
    out: &mut Vec<ArchiveSourceEntry>,
) -> Result<()> {
    let mut children = loom.list_directory(ns, dir)?;
    children.sort_by(|left, right| left.name.cmp(&right.name));
    for child in children {
        let path = if dir.is_empty() {
            child.name
        } else {
            format!("{dir}/{}", child.name)
        };
        match child.kind {
            FileKind::Directory => {
                out.push(ArchiveSourceEntry {
                    path: path.clone(),
                    kind: ArchiveEntryKind::Directory,
                    bytes: Vec::new(),
                });
                collect_archive_source_entries(loom, ns, &path, out)?;
            }
            FileKind::File => out.push(ArchiveSourceEntry {
                bytes: loom.read_file(ns, &path)?,
                path,
                kind: ArchiveEntryKind::File,
            }),
            FileKind::Symlink => {
                return Err(LoomError::unsupported(format!(
                    "archive export does not support symlink {path}"
                )));
            }
        }
    }
    Ok(())
}

fn export_tar_bytes(entries: &[ArchiveSourceEntry]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut archive = tar::Builder::new(&mut out);
        for entry in entries {
            let mut header = tar::Header::new_gnu();
            header.set_mtime(0);
            header.set_uid(0);
            header.set_gid(0);
            match entry.kind {
                ArchiveEntryKind::Directory => {
                    header.set_entry_type(tar::EntryType::Directory);
                    header.set_size(0);
                    header.set_mode(0o755);
                    header.set_cksum();
                    archive
                        .append_data(&mut header, &entry.path, std::io::empty())
                        .map_err(|e| {
                            LoomError::new(
                                Code::Io,
                                format!("write tar directory {}: {e}", entry.path),
                            )
                        })?;
                }
                ArchiveEntryKind::File => {
                    header.set_entry_type(tar::EntryType::Regular);
                    header.set_size(entry.bytes.len() as u64);
                    header.set_mode(0o644);
                    header.set_cksum();
                    archive
                        .append_data(&mut header, &entry.path, entry.bytes.as_slice())
                        .map_err(|e| {
                            LoomError::new(Code::Io, format!("write tar file {}: {e}", entry.path))
                        })?;
                }
                ArchiveEntryKind::Symlink => {
                    return Err(LoomError::unsupported(
                        "archive export does not support symlinks",
                    ));
                }
            }
        }
        archive
            .finish()
            .map_err(|e| LoomError::new(Code::Io, format!("finish tar archive: {e}")))?;
    }
    Ok(out)
}

fn export_zip_bytes(entries: &[ArchiveSourceEntry]) -> Result<Vec<u8>> {
    let cursor = std::io::Cursor::new(Vec::new());
    let mut archive = zip::ZipWriter::new(cursor);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored)
        .unix_permissions(0o644);
    for entry in entries {
        match entry.kind {
            ArchiveEntryKind::Directory => {
                archive
                    .add_directory(format!("{}/", entry.path.trim_end_matches('/')), options)
                    .map_err(|e| LoomError::new(Code::Io, format!("write zip directory: {e}")))?;
            }
            ArchiveEntryKind::File => {
                archive
                    .start_file(&entry.path, options)
                    .map_err(|e| LoomError::new(Code::Io, format!("write zip file: {e}")))?;
                archive
                    .write_all(&entry.bytes)
                    .map_err(|e| LoomError::new(Code::Io, format!("write zip file bytes: {e}")))?;
            }
            ArchiveEntryKind::Symlink => {
                return Err(LoomError::unsupported(
                    "archive export does not support symlinks",
                ));
            }
        }
    }
    archive
        .finish()
        .map(|cursor| cursor.into_inner())
        .map_err(|e| LoomError::new(Code::Io, format!("finish zip archive: {e}")))
}

pub fn export_fs<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    dst: &Path,
    options: &FsExportOptions,
) -> Result<ExportReport> {
    loom.authorize(ns, FacetKind::Files, AclRight::Read)?;
    if let Some(revision) = &options.revision {
        return export_fs_revision(loom, ns, revision, dst, options);
    }
    let mut report = ExportReport::new(&options.profile, &options.destination_scope)?;
    report.dry_run = options.dry_run;
    let mut stack = vec![String::new()];

    while let Some(dir) = stack.pop() {
        let target_dir = if dir.is_empty() {
            dst.to_path_buf()
        } else {
            safe_join(dst, &dir)?
        };
        if !options.dry_run {
            fs::create_dir_all(&target_dir).map_err(|e| {
                LoomError::new(
                    Code::Io,
                    format!("create export directory {}: {e}", target_dir.display()),
                )
            })?;
        }

        for entry in loom.list_directory(ns, &dir)? {
            let child = if dir.is_empty() {
                entry.name.clone()
            } else {
                format!("{dir}/{}", entry.name)
            };
            match entry.kind {
                FileKind::Directory => stack.push(child),
                FileKind::File => {
                    let bytes = loom.read_file(ns, &child)?;
                    let target = safe_join(dst, &child)?;
                    if !options.dry_run {
                        if let Some(parent) = target.parent() {
                            fs::create_dir_all(parent).map_err(|e| {
                                LoomError::new(
                                    Code::Io,
                                    format!("create export directory {}: {e}", parent.display()),
                                )
                            })?;
                        }
                        fs::write(&target, &bytes).map_err(|e| {
                            LoomError::new(
                                Code::Io,
                                format!("write export file {}: {e}", target.display()),
                            )
                        })?;
                    }
                    report.files_written += u64::from(!options.dry_run);
                    report.bytes_out += bytes.len() as u64;
                }
                FileKind::Symlink => {
                    report.fidelity_issues.push(FidelityIssue::new(
                        FidelitySeverity::Error,
                        child,
                        "symlink",
                        "filesystem export does not materialize symlinks yet",
                    )?);
                }
            }
        }
    }

    if !report.fidelity_issues.is_empty() {
        return Err(LoomError::unsupported(
            "filesystem export encountered unsupported symlinks",
        ));
    }

    Ok(report)
}

fn export_fs_revision<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    revision: &str,
    dst: &Path,
    options: &FsExportOptions,
) -> Result<ExportReport> {
    let mut report = ExportReport::new(&options.profile, &options.destination_scope)?;
    report.dry_run = options.dry_run;

    for entry in loom.committed_fs_entries(ns, revision)? {
        let target = safe_join(dst, &entry.path)?;
        match entry.kind {
            FileKind::Directory => {
                if !options.dry_run {
                    fs::create_dir_all(&target).map_err(|e| {
                        LoomError::new(
                            Code::Io,
                            format!("create export directory {}: {e}", target.display()),
                        )
                    })?;
                }
            }
            FileKind::File => {
                if !options.dry_run {
                    if let Some(parent) = target.parent() {
                        fs::create_dir_all(parent).map_err(|e| {
                            LoomError::new(
                                Code::Io,
                                format!("create export directory {}: {e}", parent.display()),
                            )
                        })?;
                    }
                    fs::write(&target, &entry.bytes).map_err(|e| {
                        LoomError::new(
                            Code::Io,
                            format!("write export file {}: {e}", target.display()),
                        )
                    })?;
                }
                report.files_written += u64::from(!options.dry_run);
                report.bytes_out += entry.bytes.len() as u64;
            }
            FileKind::Symlink => {
                report.fidelity_issues.push(FidelityIssue::new(
                    FidelitySeverity::Error,
                    entry.path,
                    "symlink",
                    "filesystem export does not materialize symlinks yet",
                )?);
            }
        }
    }

    if !report.fidelity_issues.is_empty() {
        return Err(LoomError::unsupported(
            "filesystem export encountered unsupported symlinks",
        ));
    }

    Ok(report)
}

fn import_zip_archive_reader<S: ObjectStore, R: Read + Seek>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    reader: R,
    options: &ArchiveImportOptions,
    manifest: &mut ArchiveManifest,
    report: &mut ImportReport,
) -> Result<()> {
    let mut archive = zip::ZipArchive::new(reader).map_err(zip_archive_error)?;

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|e| zip_entry_error(index, e))?;
        let path = archive_loom_path(entry.name())?;
        report.operations_planned += 1;
        if entry.encrypted() {
            return Err(LoomError::unsupported(format!(
                "encrypted zip entry {path} is not supported"
            )));
        }
        if entry.is_symlink() {
            return Err(LoomError::unsupported(format!(
                "zip symlink entry {path} is not supported"
            )));
        }

        if entry.is_dir() {
            manifest
                .entries
                .push(ArchiveEntry::new(&path, ArchiveEntryKind::Directory, 0)?);
            apply_archive_directory(loom, ns, &path, options, report)?;
        } else {
            let mut bytes = Vec::new();
            entry
                .read_to_end(&mut bytes)
                .map_err(|e| LoomError::new(Code::Io, format!("read zip entry {path}: {e}")))?;
            append_file_manifest_entry(manifest, &path, &bytes)?;
            apply_archive_file(loom, ns, &path, &bytes, options, report)?;
        }
    }

    manifest.validate()
}

fn zip_archive_error(error: ZipError) -> LoomError {
    match error {
        ZipError::UnsupportedArchive(_) | ZipError::CompressionMethodNotSupported(_) => {
            LoomError::unsupported(format!("unsupported zip archive: {error}"))
        }
        other => LoomError::invalid(format!("read zip archive: {other}")),
    }
}

fn zip_entry_error(index: usize, error: ZipError) -> LoomError {
    match error {
        ZipError::UnsupportedArchive(_) | ZipError::CompressionMethodNotSupported(_) => {
            LoomError::unsupported(format!("unsupported zip entry {index}: {error}"))
        }
        other => LoomError::invalid(format!("read zip entry {index}: {other}")),
    }
}

fn import_tar_archive<S: ObjectStore, R: Read>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    reader: R,
    options: &ArchiveImportOptions,
    manifest: &mut ArchiveManifest,
    report: &mut ImportReport,
) -> Result<()> {
    let mut archive = tar::Archive::new(reader);
    let entries = archive
        .entries()
        .map_err(|e| LoomError::invalid(format!("read tar archive: {e}")))?;

    for entry in entries {
        let mut entry =
            entry.map_err(|e| LoomError::invalid(format!("read tar archive entry: {e}")))?;
        let path = archive_loom_path(
            entry
                .path()
                .map_err(|e| LoomError::invalid(format!("read tar entry path: {e}")))?
                .to_str()
                .ok_or_else(|| LoomError::invalid("tar entry path is not valid UTF-8"))?,
        )?;
        report.operations_planned += 1;

        if entry.header().entry_type().is_dir() {
            manifest
                .entries
                .push(ArchiveEntry::new(&path, ArchiveEntryKind::Directory, 0)?);
            apply_archive_directory(loom, ns, &path, options, report)?;
        } else if entry.header().entry_type().is_file() {
            let mut bytes = Vec::new();
            entry
                .read_to_end(&mut bytes)
                .map_err(|e| LoomError::new(Code::Io, format!("read tar entry {path}: {e}")))?;
            append_file_manifest_entry(manifest, &path, &bytes)?;
            apply_archive_file(loom, ns, &path, &bytes, options, report)?;
        } else {
            return Err(LoomError::unsupported(format!(
                "unsupported tar entry type at {path}"
            )));
        }
    }

    manifest.validate()
}

fn import_gzip_archive_reader<S: ObjectStore, R: Read>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    mut decoder: R,
    source_path_hint: &Path,
    options: &ArchiveImportOptions,
    manifest: &mut ArchiveManifest,
    report: &mut ImportReport,
) -> Result<()> {
    let mut bytes = Vec::new();
    decoder.read_to_end(&mut bytes).map_err(|e| {
        LoomError::invalid(format!(
            "read gzip archive {}: {e}",
            source_path_hint.display()
        ))
    })?;
    let path = match &options.gzip_output_path {
        Some(path) => archive_loom_path(path)?,
        None => gzip_default_output_path(source_path_hint)?,
    };

    report.operations_planned += 1;
    append_file_manifest_entry(manifest, &path, &bytes)?;
    apply_archive_file(loom, ns, &path, &bytes, options, report)?;
    manifest.validate()
}

fn append_file_manifest_entry(
    manifest: &mut ArchiveManifest,
    path: &str,
    bytes: &[u8],
) -> Result<()> {
    let mut entry = ArchiveEntry::new(path, ArchiveEntryKind::File, bytes.len() as u64)?;
    entry.digest = Some(Digest::blake3(bytes));
    manifest.entries.push(entry);
    Ok(())
}

fn apply_archive_directory<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    path: &str,
    options: &ArchiveImportOptions,
    report: &mut ImportReport,
) -> Result<()> {
    if !options.dry_run {
        loom.registry_mut().add_facet(ns, FacetKind::Files)?;
        loom.create_directory(ns, path, true)?;
        report.operations_applied += 1;
    }
    Ok(())
}

fn apply_archive_file<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    path: &str,
    bytes: &[u8],
    options: &ArchiveImportOptions,
    report: &mut ImportReport,
) -> Result<()> {
    report.bytes_stored += bytes.len() as u64;
    if !options.dry_run {
        loom.registry_mut().add_facet(ns, FacetKind::Files)?;
        if let Some(parent) = parent_path(path) {
            loom.create_directory(ns, &parent, true)?;
        }
        loom.write_file(ns, path, bytes, 0o100644)?;
        report.operations_applied += 1;
    }
    Ok(())
}

#[derive(Debug)]
struct ImportEntry {
    loom_path: String,
    kind: ImportEntryKind,
}

#[derive(Debug)]
enum ImportEntryKind {
    Directory,
    File { host_path: PathBuf },
}

fn collect_import_entries(root: &Path, dir: &Path, entries: &mut Vec<ImportEntry>) -> Result<()> {
    let mut children = fs::read_dir(dir)
        .map_err(|e| LoomError::new(Code::Io, format!("read directory {}: {e}", dir.display())))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| LoomError::new(Code::Io, format!("read directory {}: {e}", dir.display())))?;
    children.sort_by_key(|entry| entry.file_name());

    for child in children {
        let file_type = child.file_type().map_err(|e| {
            LoomError::new(
                Code::Io,
                format!("stat import path {}: {e}", child.path().display()),
            )
        })?;
        let rel = relative_loom_path(root, &child.path())?;
        if file_type.is_dir() {
            entries.push(ImportEntry {
                loom_path: rel,
                kind: ImportEntryKind::Directory,
            });
            collect_import_entries(root, &child.path(), entries)?;
        } else if file_type.is_file() {
            entries.push(ImportEntry {
                loom_path: rel,
                kind: ImportEntryKind::File {
                    host_path: child.path(),
                },
            });
        } else {
            return Err(LoomError::unsupported(format!(
                "unsupported import path type {}",
                child.path().display()
            )));
        }
    }

    Ok(())
}

fn relative_loom_path(root: &Path, path: &Path) -> Result<String> {
    let rel = path.strip_prefix(root).map_err(|_| {
        LoomError::invalid(format!(
            "path {} is outside import root {}",
            path.display(),
            root.display()
        ))
    })?;
    let mut parts = Vec::new();
    for component in rel.components() {
        match component {
            Component::Normal(value) => {
                let segment = value
                    .to_str()
                    .ok_or_else(|| LoomError::invalid("import path is not valid UTF-8"))?;
                if segment.is_empty() || segment == "." || segment == ".." {
                    return Err(LoomError::invalid("invalid import path segment"));
                }
                parts.push(segment.to_string());
            }
            _ => return Err(LoomError::invalid("invalid import path component")),
        }
    }
    Ok(parts.join("/"))
}

fn parent_path(path: &str) -> Option<String> {
    path.rsplit_once('/')
        .map(|(parent, _)| parent.to_string())
        .filter(|parent| !parent.is_empty())
}

fn safe_join(root: &Path, loom_path: &str) -> Result<PathBuf> {
    let mut out = root.to_path_buf();
    for segment in loom_path.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return Err(LoomError::invalid("invalid export path segment"));
        }
        out.push(segment);
    }
    Ok(out)
}

fn archive_loom_path(path: &str) -> Result<String> {
    if path.is_empty() || path.contains('\\') || path.contains('\0') {
        return Err(LoomError::invalid("invalid archive entry path"));
    }
    let mut parts = Vec::new();
    for component in Path::new(path).components() {
        match component {
            Component::Normal(value) => {
                let segment = value
                    .to_str()
                    .ok_or_else(|| LoomError::invalid("archive entry path is not valid UTF-8"))?;
                if segment.is_empty() || segment == "." || segment == ".." {
                    return Err(LoomError::invalid("invalid archive entry path segment"));
                }
                parts.push(segment.to_string());
            }
            _ => {
                return Err(LoomError::invalid(
                    "archive entry path escapes the archive root",
                ));
            }
        }
    }
    if parts.is_empty() {
        return Err(LoomError::invalid("archive entry path is empty"));
    }
    Ok(parts.join("/"))
}

fn gzip_default_output_path(path: &Path) -> Result<String> {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| LoomError::invalid("gzip archive path has no valid UTF-8 file name"))?;
    let output = file_name
        .strip_suffix(".gz")
        .filter(|value| !value.is_empty())
        .unwrap_or(file_name);
    archive_loom_path(output)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CsvCell {
    text: String,
    quoted: bool,
}

fn table_csv_schema(options: &TableCsvImportOptions) -> Result<Schema> {
    let mut primary_key = Vec::new();
    for name in &options.primary_key {
        let index = options
            .columns
            .iter()
            .position(|(column, _)| column == name)
            .ok_or_else(|| {
                LoomError::invalid(format!("primary-key column {name:?} is not in the schema"))
            })?;
        primary_key.push(index);
    }
    Schema::new(options.columns.clone(), primary_key)
}

fn table_csv_path(database: &str, table: &str) -> Result<String> {
    let relative = format!("{}/tables/{}", path_token(database), path_token(table));
    Ok(loom_core::workspace::facet_path(FacetKind::Sql, &relative))
}

fn table_primary_key(schema: &Schema, row: &[Value]) -> Vec<Value> {
    schema
        .primary_key
        .iter()
        .map(|&index| row[index].clone())
        .collect()
}

fn parse_table_csv_rows(bytes: &[u8], schema: &Schema) -> Result<Vec<Vec<Value>>> {
    let rows = parse_csv(bytes)?;
    let (header, data) = rows
        .split_first()
        .ok_or_else(|| LoomError::invalid("table CSV input is empty"))?;
    let expected_header: Vec<&str> = schema
        .columns
        .iter()
        .map(|(name, _)| name.as_str())
        .collect();
    let actual_header: Vec<&str> = header.iter().map(|cell| cell.text.as_str()).collect();
    if actual_header != expected_header {
        return Err(LoomError::invalid(format!(
            "table CSV header mismatch: expected {}, found {}",
            expected_header.join(","),
            actual_header.join(",")
        )));
    }
    let mut output = Vec::new();
    for (index, row) in data.iter().enumerate() {
        if row.len() != schema.arity() {
            return Err(LoomError::invalid(format!(
                "table CSV row {} has {} fields, schema has {}",
                index + 2,
                row.len(),
                schema.arity()
            )));
        }
        let mut values = Vec::new();
        for (cell, (name, ty)) in row.iter().zip(&schema.columns) {
            values.push(parse_table_csv_cell(cell, *ty, name)?);
        }
        output.push(values);
    }
    Ok(output)
}

fn parse_csv(bytes: &[u8]) -> Result<Vec<Vec<CsvCell>>> {
    let input = std::str::from_utf8(bytes)
        .map_err(|e| LoomError::invalid(format!("table CSV input is not UTF-8: {e}")))?;
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut in_quotes = false;
    let mut at_field_start = true;
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if in_quotes {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(ch);
            }
            continue;
        }
        match ch {
            '"' if at_field_start => {
                quoted = true;
                in_quotes = true;
                at_field_start = false;
            }
            ',' => {
                row.push(CsvCell {
                    text: std::mem::take(&mut field),
                    quoted,
                });
                quoted = false;
                at_field_start = true;
            }
            '\n' => {
                row.push(CsvCell {
                    text: std::mem::take(&mut field),
                    quoted,
                });
                rows.push(std::mem::take(&mut row));
                quoted = false;
                at_field_start = true;
            }
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                row.push(CsvCell {
                    text: std::mem::take(&mut field),
                    quoted,
                });
                rows.push(std::mem::take(&mut row));
                quoted = false;
                at_field_start = true;
            }
            '"' => {
                return Err(LoomError::invalid(
                    "table CSV quote appears in an unquoted field",
                ));
            }
            other => {
                field.push(other);
                at_field_start = false;
            }
        }
    }
    if in_quotes {
        return Err(LoomError::invalid(
            "table CSV input has an unterminated quote",
        ));
    }
    if !field.is_empty() || quoted || !row.is_empty() {
        row.push(CsvCell {
            text: field,
            quoted,
        });
        rows.push(row);
    }
    Ok(rows)
}

fn parse_table_csv_cell(cell: &CsvCell, ty: ColumnType, name: &str) -> Result<Value> {
    if cell.text.is_empty() && !cell.quoted {
        return Ok(Value::Null);
    }
    let value = cell.text.as_str();
    Ok(match ty {
        ColumnType::Int => Value::Int(parse_scalar(value, name)?),
        ColumnType::Float => {
            let parsed: f64 = parse_scalar(value, name)?;
            if !parsed.is_finite() {
                return Err(LoomError::invalid(format!(
                    "column {name:?} rejects non-finite float"
                )));
            }
            Value::Float(parsed)
        }
        ColumnType::Text => Value::Text(value.to_string()),
        ColumnType::Bool => match value {
            "true" => Value::Bool(true),
            "false" => Value::Bool(false),
            _ => {
                return Err(LoomError::invalid(format!(
                    "column {name:?} expects true or false"
                )));
            }
        },
        ColumnType::I8 => Value::I8(parse_scalar(value, name)?),
        ColumnType::I16 => Value::I16(parse_scalar(value, name)?),
        ColumnType::I32 => Value::I32(parse_scalar(value, name)?),
        ColumnType::I128 => Value::I128(parse_scalar(value, name)?),
        ColumnType::U8 => Value::U8(parse_scalar(value, name)?),
        ColumnType::U16 => Value::U16(parse_scalar(value, name)?),
        ColumnType::U32 => Value::U32(parse_scalar(value, name)?),
        ColumnType::U64 => Value::U64(parse_scalar(value, name)?),
        ColumnType::U128 => Value::U128(parse_scalar(value, name)?),
        ColumnType::F32 => {
            let parsed: f32 = parse_scalar(value, name)?;
            if !parsed.is_finite() {
                return Err(LoomError::invalid(format!(
                    "column {name:?} rejects non-finite f32"
                )));
            }
            Value::F32(parsed)
        }
        ColumnType::Decimal => parse_decimal(value, name)?,
        ColumnType::Date => Value::Date(parse_scalar(value, name)?),
        ColumnType::Time => Value::Time(parse_scalar(value, name)?),
        ColumnType::Timestamp => Value::Timestamp(parse_scalar(value, name)?),
        ColumnType::Uuid => Value::Uuid(parse_scalar(value, name)?),
        other => {
            return Err(LoomError::unsupported(format!(
                "table CSV column {name:?} type {other:?} is not supported"
            )));
        }
    })
}

fn parse_scalar<T>(value: &str, name: &str) -> Result<T>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    value
        .parse()
        .map_err(|e| LoomError::invalid(format!("column {name:?} parse failed: {e}")))
}

fn parse_decimal(value: &str, name: &str) -> Result<Value> {
    let rest = value
        .strip_prefix('-')
        .or_else(|| value.strip_prefix('+'))
        .unwrap_or(value);
    let negative = value.starts_with('-');
    if rest.is_empty() {
        return Err(LoomError::invalid(format!(
            "column {name:?} expects a decimal"
        )));
    }
    let mut parts = rest.split('.');
    let whole = parts.next().unwrap_or_default();
    let fraction = parts.next();
    if parts.next().is_some() || whole.is_empty() && fraction.is_none_or(str::is_empty) {
        return Err(LoomError::invalid(format!(
            "column {name:?} expects a decimal"
        )));
    }
    if !whole.chars().all(|ch| ch.is_ascii_digit())
        || fraction.is_some_and(|part| !part.chars().all(|ch| ch.is_ascii_digit()))
    {
        return Err(LoomError::invalid(format!(
            "column {name:?} expects a decimal"
        )));
    }
    let scale = fraction.map_or(0, str::len) as u32;
    let mut digits = String::with_capacity(whole.len() + fraction.map_or(0, str::len));
    digits.push_str(whole);
    if let Some(fraction) = fraction {
        digits.push_str(fraction);
    }
    let trimmed = digits.trim_start_matches('0');
    let magnitude = if trimmed.is_empty() {
        0
    } else {
        trimmed
            .parse::<i128>()
            .map_err(|e| LoomError::invalid(format!("column {name:?} parse failed: {e}")))?
    };
    let mantissa = if negative { -magnitude } else { magnitude };
    Ok(Value::Decimal { mantissa, scale })
}

fn write_table_csv(table: &Table) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    write_csv_row(
        table
            .schema()
            .columns
            .iter()
            .map(|(name, _)| CsvOutCell::Text(name.as_str())),
        &mut out,
    );
    for row in table.scan(&Predicate::All) {
        write_csv_row(
            row.iter()
                .map(value_to_csv_cell)
                .collect::<Result<Vec<_>>>()?,
            &mut out,
        );
    }
    Ok(out)
}

enum CsvOutCell<'a> {
    Null,
    Text(&'a str),
    Owned(String),
}

fn write_csv_row<'a>(cells: impl IntoIterator<Item = CsvOutCell<'a>>, out: &mut Vec<u8>) {
    let mut first = true;
    for cell in cells {
        if first {
            first = false;
        } else {
            out.push(b',');
        }
        match cell {
            CsvOutCell::Null => {}
            CsvOutCell::Text(value) => write_csv_field(value, out),
            CsvOutCell::Owned(value) => write_csv_field(&value, out),
        }
    }
    out.push(b'\n');
}

fn write_csv_field(value: &str, out: &mut Vec<u8>) {
    let must_quote = value.is_empty()
        || value
            .bytes()
            .any(|byte| matches!(byte, b',' | b'"' | b'\r' | b'\n'));
    if !must_quote {
        out.extend_from_slice(value.as_bytes());
        return;
    }
    out.push(b'"');
    for byte in value.bytes() {
        if byte == b'"' {
            out.extend_from_slice(br#""""#);
        } else {
            out.push(byte);
        }
    }
    out.push(b'"');
}

fn value_to_csv_cell(value: &Value) -> Result<CsvOutCell<'_>> {
    Ok(match value {
        Value::Null => CsvOutCell::Null,
        Value::Bool(value) => CsvOutCell::Owned(value.to_string()),
        Value::Int(value) => CsvOutCell::Owned(value.to_string()),
        Value::Float(value) => {
            if !value.is_finite() {
                return Err(LoomError::invalid("cannot export non-finite float to CSV"));
            }
            CsvOutCell::Owned(value.to_string())
        }
        Value::Text(value) => CsvOutCell::Text(value),
        Value::I8(value) => CsvOutCell::Owned(value.to_string()),
        Value::I16(value) => CsvOutCell::Owned(value.to_string()),
        Value::I32(value) => CsvOutCell::Owned(value.to_string()),
        Value::I128(value) => CsvOutCell::Owned(value.to_string()),
        Value::U8(value) => CsvOutCell::Owned(value.to_string()),
        Value::U16(value) => CsvOutCell::Owned(value.to_string()),
        Value::U32(value) => CsvOutCell::Owned(value.to_string()),
        Value::U64(value) => CsvOutCell::Owned(value.to_string()),
        Value::U128(value) => CsvOutCell::Owned(value.to_string()),
        Value::F32(value) => {
            if !value.is_finite() {
                return Err(LoomError::invalid("cannot export non-finite f32 to CSV"));
            }
            CsvOutCell::Owned(value.to_string())
        }
        Value::Decimal { mantissa, scale } => CsvOutCell::Owned(format_decimal(*mantissa, *scale)),
        Value::Date(value) => CsvOutCell::Owned(value.to_string()),
        Value::Time(value) => CsvOutCell::Owned(value.to_string()),
        Value::Timestamp(value) => CsvOutCell::Owned(value.to_string()),
        Value::Uuid(value) => CsvOutCell::Owned(value.to_string()),
        other => {
            return Err(LoomError::unsupported(format!(
                "cannot export {other:?} to table CSV"
            )));
        }
    })
}

fn format_decimal(mantissa: i128, scale: u32) -> String {
    let mut digits = mantissa.to_string();
    let sign = if digits.starts_with('-') {
        digits.remove(0);
        "-"
    } else {
        ""
    };
    if scale == 0 {
        return format!("{sign}{digits}");
    }
    let scale = scale as usize;
    if digits.len() <= scale {
        let zeros = "0".repeat(scale + 1 - digits.len());
        digits = format!("{zeros}{digits}");
    }
    let split = digits.len() - scale;
    format!("{sign}{}.{}", &digits[..split], &digits[split..])
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use loom_core::{AclEffect, AclGrant, AclScope, AclSubject, IdentityStore, Loom, MemoryStore};
    use std::fs::File;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    fn create_test_workspace(loom: &mut Loom<MemoryStore>, byte: u8) -> WorkspaceId {
        loom.registry_mut()
            .create(
                FacetKind::Files,
                Some("main"),
                WorkspaceId::from_bytes([byte; 16]),
            )
            .unwrap()
    }

    fn authenticate_root(loom: &mut Loom<MemoryStore>, root: WorkspaceId) {
        let mut identity = IdentityStore::new(root);
        identity.set_passphrase(root, "root", b"12345678").unwrap();
        let session = identity
            .authenticate_passphrase(root, "root", "session")
            .unwrap();
        loom.set_identity_store(identity);
        loom.set_session(session.id);
    }

    #[test]
    fn resolve_import_input_hashes_file_and_detects_archive_candidate() {
        let temp = std::env::temp_dir().join(format!("loom-interchange-input-{}", now_ms()));
        fs::create_dir_all(&temp).unwrap();
        let file = temp.join("snapshot.json");
        fs::write(&file, br#"{"ok":true}"#).unwrap();

        let resolved = resolve_import_input(&file, Algo::Blake3).unwrap();

        assert_eq!(resolved.kind, ImportInputKind::File);
        assert_eq!(resolved.size_bytes, 11);
        assert_eq!(resolved.item_count, 1);
        assert_eq!(resolved.bytes.as_deref(), Some(&br#"{"ok":true}"#[..]));
        assert_eq!(
            resolved.source_digest,
            Digest::hash(Algo::Blake3, br#"{"ok":true}"#)
        );

        let archive = temp.join("export.zip");
        fs::write(&archive, b"zip bytes").unwrap();
        let resolved = resolve_import_input(&archive, Algo::Sha256).unwrap();

        assert_eq!(resolved.kind, ImportInputKind::ArchiveCandidate);
        assert_eq!(
            resolved.source_digest,
            Digest::hash(Algo::Sha256, b"zip bytes")
        );

        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn resolve_import_input_hashes_directory_independently_of_creation_order() {
        let temp = std::env::temp_dir().join(format!("loom-interchange-input-dir-{}", now_ms()));
        let left = temp.join("left");
        let right = temp.join("right");
        fs::create_dir_all(left.join("nested")).unwrap();
        fs::write(left.join("a.txt"), b"alpha").unwrap();
        fs::write(left.join("nested/b.txt"), b"beta").unwrap();
        fs::create_dir_all(right.join("nested")).unwrap();
        fs::write(right.join("nested/b.txt"), b"beta").unwrap();
        fs::write(right.join("a.txt"), b"alpha").unwrap();

        let left = resolve_import_input(&left, Algo::Blake3).unwrap();
        let right = resolve_import_input(&right, Algo::Blake3).unwrap();

        assert_eq!(left.kind, ImportInputKind::Directory);
        assert_eq!(right.kind, ImportInputKind::Directory);
        assert_eq!(left.bytes, None);
        assert_eq!(right.bytes, None);
        assert_eq!(left.size_bytes, 9);
        assert_eq!(right.size_bytes, 9);
        assert_eq!(left.item_count, 2);
        assert_eq!(right.item_count, 2);
        assert_eq!(left.source_digest, right.source_digest);

        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn resolved_import_input_builds_checkpoint() {
        let temp = std::env::temp_dir().join(format!("loom-interchange-checkpoint-{}", now_ms()));
        fs::create_dir_all(&temp).unwrap();
        let file = temp.join("snapshot.json");
        fs::write(&file, b"payload").unwrap();
        let resolved =
            resolve_import_input_with_scope(&file, "source://snapshot", Algo::Blake3).unwrap();

        let checkpoint = resolved.checkpoint("notion", "cp-1").unwrap();

        assert_eq!(checkpoint.checkpoint_id, "cp-1");
        assert_eq!(checkpoint.profile, "notion");
        assert_eq!(checkpoint.source_scope, "source://snapshot");
        assert_eq!(
            checkpoint.profile_state_digest,
            Some(resolved.source_digest)
        );
        assert_eq!(checkpoint.observed_ids, vec!["source://snapshot"]);
        assert_eq!(checkpoint.completed_units, vec!["file:1:7"]);
        assert_eq!(
            checkpoint.resume_state,
            resolved.source_digest.to_string().into_bytes()
        );

        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn retained_import_input_persists_file_payload_and_checkpoint() {
        let temp = std::env::temp_dir().join(format!("loom-interchange-retained-{}", now_ms()));
        let store_path = temp.join("retained.loom");
        fs::create_dir_all(&temp).unwrap();
        let file = temp.join("snapshot.json");
        fs::write(&file, b"payload").unwrap();
        let store = FileStore::create_with_profile(&store_path, Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        let ns = WorkspaceId::from_bytes([31; 16]);
        loom.registry_mut()
            .create(FacetKind::Vcs, Some("repo"), ns)
            .unwrap();
        let resolved =
            resolve_import_input_with_scope(&file, "source://snapshot", Algo::Blake3).unwrap();

        let retained = retain_import_input(&mut loom, ns, "jira", &resolved, None).unwrap();
        let mut checkpoint = resolved.checkpoint("jira", "cp-1").unwrap();
        checkpoint.profile_state_digest = Some(retained.manifest_digest);
        let checkpoint_key = persist_import_checkpoint(&mut loom, ns, &checkpoint, None).unwrap();

        assert_eq!(retained.bytes_retained, 7);
        assert_eq!(retained.payload_keys.len(), 1);
        assert_eq!(
            loom.store()
                .control_get(&retained.payload_keys[0])
                .unwrap()
                .as_deref(),
            Some(b"payload".as_slice())
        );
        assert!(
            loom.store()
                .control_get(&retained.manifest_key)
                .unwrap()
                .is_some()
        );
        assert_eq!(
            ImportCheckpoint::decode(
                &loom
                    .store()
                    .control_get(&checkpoint_key)
                    .unwrap()
                    .expect("checkpoint")
            )
            .unwrap(),
            checkpoint
        );

        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn retained_import_input_persists_directory_payloads() {
        let temp = std::env::temp_dir().join(format!("loom-interchange-retained-dir-{}", now_ms()));
        let src = temp.join("src");
        let store_path = temp.join("retained.loom");
        fs::create_dir_all(src.join("nested")).unwrap();
        fs::write(src.join("a.txt"), b"alpha").unwrap();
        fs::write(src.join("nested/b.txt"), b"beta").unwrap();
        let store = FileStore::create_with_profile(&store_path, Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        let ns = WorkspaceId::from_bytes([32; 16]);
        loom.registry_mut()
            .create(FacetKind::Vcs, Some("repo"), ns)
            .unwrap();
        let resolved = resolve_import_input_with_scope(&src, "source://dir", Algo::Blake3).unwrap();

        let retained = retain_import_input(&mut loom, ns, "markdown", &resolved, None).unwrap();

        assert_eq!(retained.item_count, 2);
        assert_eq!(retained.bytes_retained, 9);
        assert_eq!(retained.payload_keys.len(), 2);
        let payloads = retained
            .payload_keys
            .iter()
            .map(|key| loom.store().control_get(key).unwrap().expect("payload"))
            .collect::<Vec<_>>();
        assert!(payloads.iter().any(|payload| payload == b"alpha"));
        assert!(payloads.iter().any(|payload| payload == b"beta"));
        assert!(
            loom.store()
                .control_get(&retained.manifest_key)
                .unwrap()
                .is_some()
        );

        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn import_batch_submission_persists_canonical_bytes() {
        let temp = std::env::temp_dir().join(format!("loom-interchange-submission-{}", now_ms()));
        let store_path = temp.join("submission.loom");
        fs::create_dir_all(&temp).unwrap();
        let store = FileStore::create_with_profile(&store_path, Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        let ns = WorkspaceId::from_bytes([33; 16]);
        loom.registry_mut()
            .create(FacetKind::Vcs, Some("repo"), ns)
            .unwrap();
        let mut batch = ImportBatch::new(
            "tickets",
            "jira",
            "jira:site",
            100,
            loom_interchange::Coverage::Partial,
        )
        .unwrap();
        batch.items.push(
            loom_interchange::ImportBatchItem::new("CORE-1", Digest::hash(Algo::Blake3, b"CORE-1"))
                .unwrap(),
        );
        let bytes = batch.encode().unwrap();

        let submitted = persist_import_batch_submission(&mut loom, ns, &bytes, None).unwrap();

        assert_eq!(submitted.batch, batch);
        assert_eq!(submitted.batch_digest, Digest::hash(Algo::Blake3, &bytes));
        assert_eq!(
            loom.store()
                .control_get(&submitted.control_key)
                .unwrap()
                .as_deref(),
            Some(bytes.as_slice())
        );

        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn import_execution_batch_runs_source_backed_profile() {
        let temp = std::env::temp_dir().join(format!("loom-interchange-execution-{}", now_ms()));
        let store_path = temp.join("execution.loom");
        fs::create_dir_all(&temp).unwrap();
        let store = FileStore::create_with_profile(&store_path, Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        let ns = WorkspaceId::from_bytes([34; 16]);
        loom.registry_mut()
            .create(FacetKind::Vcs, Some("repo"), ns)
            .unwrap();
        let payload =
            include_bytes!("../../../specs/studio/fixtures/redmine/source/redmine-api-bundle.xml");
        let mut batch = ImportExecutionBatch::new(
            "tickets",
            "redmine",
            "redmine-api",
            100,
            loom_interchange::Coverage::Complete,
        )
        .unwrap();
        batch.payloads.push(
            loom_interchange::ImportExecutionPayload::new(
                "redmine-api-bundle.xml",
                "application/xml",
                payload.to_vec(),
                Algo::Blake3,
            )
            .unwrap(),
        );
        let bytes = batch.encode().unwrap();

        let result = execute_import_execution_batch(&mut loom, ns, &bytes, false, None).unwrap();

        assert_eq!(result.batch, batch);
        assert_eq!(result.batch_digest, Digest::hash(Algo::Blake3, &bytes));
        assert_eq!(result.report.profile, "redmine");
        assert_eq!(result.report.rows_imported, 4);
        assert!(result.changed);
        assert_eq!(
            loom.store()
                .control_get(&result.control_key)
                .unwrap()
                .as_deref(),
            Some(bytes.as_slice())
        );
        let reader = loom_tickets::TicketProfileReader::open(&loom, ns, &ns.to_string())
            .unwrap()
            .unwrap();
        let identity = loom_tickets::ExternalTicketIdentity::new("redmine", "issue:42").unwrap();
        assert!(
            reader
                .ticket_by_external_identity(&identity)
                .unwrap()
                .is_some()
        );

        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn import_execution_batch_runs_notion_api_profile() {
        let temp = std::env::temp_dir().join(format!("loom-interchange-notion-batch-{}", now_ms()));
        let store_path = temp.join("notion.loom");
        fs::create_dir_all(&temp).unwrap();
        let store = FileStore::create_with_profile(&store_path, Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        let ns = WorkspaceId::from_bytes([36; 16]);
        loom.registry_mut()
            .create(FacetKind::Vcs, Some("repo"), ns)
            .unwrap();
        let payload =
            include_bytes!("../../../specs/studio/fixtures/notion/source/notion-api-bundle.json");
        let mut batch = ImportExecutionBatch::new(
            "pages",
            "notion",
            "notion-api",
            100,
            loom_interchange::Coverage::Complete,
        )
        .unwrap();
        batch.payloads.push(
            loom_interchange::ImportExecutionPayload::new(
                "notion-api-bundle.json",
                "application/json",
                payload.to_vec(),
                Algo::Blake3,
            )
            .unwrap(),
        );
        let bytes = batch.encode().unwrap();

        let result = execute_import_execution_batch(&mut loom, ns, &bytes, false, None).unwrap();

        assert_eq!(result.batch, batch);
        assert_eq!(result.batch_digest, Digest::hash(Algo::Blake3, &bytes));
        assert_eq!(result.report.profile, "notion");
        assert_eq!(result.report.rows_imported, 3);
        assert!(result.changed);
        assert!(
            loom_pages::get_page(&loom, ns, &ns.to_string(), "page-intro")
                .unwrap()
                .is_some()
        );

        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn import_execution_batch_runs_asana_profile() {
        let temp = std::env::temp_dir().join(format!("loom-interchange-asana-batch-{}", now_ms()));
        let store_path = temp.join("asana.loom");
        fs::create_dir_all(&temp).unwrap();
        let store = FileStore::create_with_profile(&store_path, Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        let ns = WorkspaceId::from_bytes([37; 16]);
        loom.registry_mut()
            .create(FacetKind::Vcs, Some("repo"), ns)
            .unwrap();
        let payload = include_bytes!(
            "../../../specs/studio/fixtures/asana/source/asana-normalized-snapshot.json"
        );
        let mut batch = ImportExecutionBatch::new(
            "tickets",
            "asana",
            "asana-normalized",
            100,
            loom_interchange::Coverage::Complete,
        )
        .unwrap();
        batch.payloads.push(
            loom_interchange::ImportExecutionPayload::new(
                "asana-normalized-snapshot.json",
                "application/json",
                payload.to_vec(),
                Algo::Blake3,
            )
            .unwrap(),
        );
        let bytes = batch.encode().unwrap();

        let result = execute_import_execution_batch(&mut loom, ns, &bytes, false, None).unwrap();

        assert_eq!(result.batch, batch);
        assert_eq!(result.batch_digest, Digest::hash(Algo::Blake3, &bytes));
        assert_eq!(result.report.profile, "asana");
        assert_eq!(result.report.rows_imported, 6);
        assert!(result.changed);
        let reader = loom_tickets::TicketProfileReader::open(&loom, ns, &ns.to_string())
            .unwrap()
            .unwrap();
        let identity = loom_tickets::ExternalTicketIdentity::new("asana", "task:t-100").unwrap();
        assert!(
            reader
                .ticket_by_external_identity(&identity)
                .unwrap()
                .is_some()
        );

        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn import_execution_batch_runs_jira_profile() {
        let temp = std::env::temp_dir().join(format!("loom-interchange-jira-batch-{}", now_ms()));
        let store_path = temp.join("jira.loom");
        fs::create_dir_all(&temp).unwrap();
        let store = FileStore::create_with_profile(&store_path, Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        let ns = WorkspaceId::from_bytes([38; 16]);
        loom.registry_mut()
            .create(FacetKind::Vcs, Some("repo"), ns)
            .unwrap();
        let payload = include_bytes!(
            "../../../specs/studio/fixtures/jira/source/jira-normalized-snapshot.json"
        );
        let mut batch = ImportExecutionBatch::new(
            "tickets",
            "jira",
            "jira-normalized",
            100,
            loom_interchange::Coverage::Complete,
        )
        .unwrap();
        batch.payloads.push(
            loom_interchange::ImportExecutionPayload::new(
                "jira-normalized-snapshot.json",
                "application/json",
                payload.to_vec(),
                Algo::Blake3,
            )
            .unwrap(),
        );
        let bytes = batch.encode().unwrap();

        let result = execute_import_execution_batch(&mut loom, ns, &bytes, false, None).unwrap();

        assert_eq!(result.batch, batch);
        assert_eq!(result.batch_digest, Digest::hash(Algo::Blake3, &bytes));
        assert_eq!(result.report.profile, "jira");
        assert_eq!(result.report.rows_imported, 6);
        assert!(result.changed);
        let reader = loom_tickets::TicketProfileReader::open(&loom, ns, &ns.to_string())
            .unwrap()
            .unwrap();
        let identity = loom_tickets::ExternalTicketIdentity::new("jira", "issue:10042").unwrap();
        assert!(
            reader
                .ticket_by_external_identity(&identity)
                .unwrap()
                .is_some()
        );

        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn import_execution_batch_runs_confluence_profile() {
        let temp =
            std::env::temp_dir().join(format!("loom-interchange-confluence-batch-{}", now_ms()));
        let store_path = temp.join("confluence.loom");
        fs::create_dir_all(&temp).unwrap();
        let store = FileStore::create_with_profile(&store_path, Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        let ns = WorkspaceId::from_bytes([39; 16]);
        loom.registry_mut()
            .create(FacetKind::Vcs, Some("repo"), ns)
            .unwrap();
        let payload = include_bytes!(
            "../../../specs/studio/fixtures/confluence/source/confluence-normalized-snapshot.json"
        );
        let mut batch = ImportExecutionBatch::new(
            "pages",
            "confluence",
            "confluence-normalized",
            100,
            loom_interchange::Coverage::Complete,
        )
        .unwrap();
        batch.default_space = Some("default".to_string());
        batch.payloads.push(
            loom_interchange::ImportExecutionPayload::new(
                "confluence-normalized-snapshot.json",
                "application/json",
                payload.to_vec(),
                Algo::Blake3,
            )
            .unwrap(),
        );
        let bytes = batch.encode().unwrap();

        let result = execute_import_execution_batch(&mut loom, ns, &bytes, false, None).unwrap();

        assert_eq!(result.batch, batch);
        assert_eq!(result.batch_digest, Digest::hash(Algo::Blake3, &bytes));
        assert_eq!(result.report.profile, "confluence");
        assert_eq!(result.report.rows_imported, 4);
        assert!(result.changed);
        assert!(
            loom_pages::get_page(&loom, ns, &ns.to_string(), "home")
                .unwrap()
                .is_some()
        );

        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn import_execution_batch_runs_slack_profile() {
        let temp = std::env::temp_dir().join(format!("loom-interchange-slack-batch-{}", now_ms()));
        let store_path = temp.join("slack.loom");
        fs::create_dir_all(&temp).unwrap();
        let store = FileStore::create_with_profile(&store_path, Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        let ns = WorkspaceId::from_bytes([40; 16]);
        loom.registry_mut()
            .create(FacetKind::Vcs, Some("repo"), ns)
            .unwrap();
        let payload = include_bytes!(
            "../../../specs/studio/fixtures/slack/source/slack-normalized-snapshot.json"
        );
        let mut batch = ImportExecutionBatch::new(
            "chat",
            "slack",
            "slack-normalized",
            100,
            loom_interchange::Coverage::Complete,
        )
        .unwrap();
        batch.payloads.push(
            loom_interchange::ImportExecutionPayload::new(
                "slack-normalized-snapshot.json",
                "application/json",
                payload.to_vec(),
                Algo::Blake3,
            )
            .unwrap(),
        );
        let bytes = batch.encode().unwrap();

        let result = execute_import_execution_batch(&mut loom, ns, &bytes, false, None).unwrap();

        assert_eq!(result.batch, batch);
        assert_eq!(result.batch_digest, Digest::hash(Algo::Blake3, &bytes));
        assert_eq!(result.report.profile, "slack");
        assert_eq!(result.report.rows_imported, 4);
        assert!(result.changed);
        let channel =
            loom_chat::resolve_channel_id(&loom, ns, &ns.to_string(), "eng-imports").unwrap();
        let projection =
            loom_chat::channel_projection(&loom, ns, &ns.to_string(), &channel).unwrap();
        assert_eq!(projection.messages.len(), 4);

        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn import_execution_batch_runs_drive_profile_with_sidecars() {
        let temp = std::env::temp_dir().join(format!("loom-interchange-drive-batch-{}", now_ms()));
        let store_path = temp.join("drive.loom");
        fs::create_dir_all(&temp).unwrap();
        let store = FileStore::create_with_profile(&store_path, Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        let ns = WorkspaceId::from_bytes([41; 16]);
        loom.registry_mut()
            .create(FacetKind::Vcs, Some("repo"), ns)
            .unwrap();
        let payload = include_bytes!(
            "../../../specs/studio/fixtures/drive/source/drive-sharepoint-snapshot.json"
        );
        let sidecar =
            include_bytes!("../../../specs/studio/fixtures/drive/source/files/path-note.txt");
        let mut batch = ImportExecutionBatch::new(
            "drive",
            "drive",
            "drive://workspace/example",
            100,
            loom_interchange::Coverage::Complete,
        )
        .unwrap();
        batch.payloads.push(
            loom_interchange::ImportExecutionPayload::new(
                "drive-sharepoint-snapshot.json",
                "application/json",
                payload.to_vec(),
                Algo::Blake3,
            )
            .unwrap(),
        );
        batch.payloads.push(
            loom_interchange::ImportExecutionPayload::new(
                "files/path-note.txt",
                "text/plain",
                sidecar.to_vec(),
                Algo::Blake3,
            )
            .unwrap(),
        );
        let bytes = batch.encode().unwrap();

        let result = execute_import_execution_batch(&mut loom, ns, &bytes, false, None).unwrap();

        assert_eq!(result.batch, batch);
        assert_eq!(result.batch_digest, Digest::hash(Algo::Blake3, &bytes));
        assert_eq!(result.report.profile, "drive");
        assert_eq!(result.report.rows_imported, 6);
        assert!(result.changed);
        assert_eq!(
            loom_drive::read_file(&loom, ns, &ns.to_string(), "file-sidecar").unwrap(),
            sidecar
        );

        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn import_execution_batch_runs_markdown_archive() {
        let temp =
            std::env::temp_dir().join(format!("loom-interchange-markdown-batch-{}", now_ms()));
        let store_path = temp.join("markdown.loom");
        let archive = temp.join("vault.zip");
        fs::create_dir_all(&temp).unwrap();
        write_zip(
            &archive,
            &[
                (
                    "Intro.md",
                    include_bytes!("../../../specs/studio/fixtures/markdown/source/vault/Intro.md")
                        .as_slice(),
                ),
                (
                    "Embed.md",
                    include_bytes!("../../../specs/studio/fixtures/markdown/source/vault/Embed.md")
                        .as_slice(),
                ),
                (
                    "Guides/Guides.md",
                    include_bytes!(
                        "../../../specs/studio/fixtures/markdown/source/vault/Guides/Guides.md"
                    )
                    .as_slice(),
                ),
                (
                    "Guides/Setup.md",
                    include_bytes!(
                        "../../../specs/studio/fixtures/markdown/source/vault/Guides/Setup.md"
                    )
                    .as_slice(),
                ),
                (
                    ".obsidian/app.json",
                    include_bytes!(
                        "../../../specs/studio/fixtures/markdown/source/vault/.obsidian/app.json"
                    )
                    .as_slice(),
                ),
                (
                    ".obsidian/types.json",
                    include_bytes!(
                        "../../../specs/studio/fixtures/markdown/source/vault/.obsidian/types.json"
                    )
                    .as_slice(),
                ),
                (
                    "Board.canvas",
                    include_bytes!(
                        "../../../specs/studio/fixtures/markdown/source/vault/Board.canvas"
                    )
                    .as_slice(),
                ),
                (
                    "Sketch.excalidraw",
                    include_bytes!(
                        "../../../specs/studio/fixtures/markdown/source/vault/Sketch.excalidraw"
                    )
                    .as_slice(),
                ),
            ],
            &["Guides/", ".obsidian/", "Attachments/"],
        );
        let store = FileStore::create_with_profile(&store_path, Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        let ns = WorkspaceId::from_bytes([35; 16]);
        loom.registry_mut()
            .create(FacetKind::Vcs, Some("repo"), ns)
            .unwrap();
        let payload = fs::read(&archive).unwrap();
        let mut batch = ImportExecutionBatch::new(
            "pages",
            "markdown",
            "markdown-zip:vault.zip",
            100,
            loom_interchange::Coverage::Complete,
        )
        .unwrap();
        batch.default_space = Some("docs".to_string());
        batch.payloads.push(
            loom_interchange::ImportExecutionPayload::new(
                "vault.zip",
                "application/zip",
                payload,
                Algo::Blake3,
            )
            .unwrap(),
        );
        let bytes = batch.encode().unwrap();

        let result = execute_import_execution_batch(&mut loom, ns, &bytes, false, None).unwrap();

        assert_eq!(result.report.profile, "markdown");
        assert_eq!(result.report.rows_imported, 4);
        assert!(result.changed);
        let intro = loom_pages::get_page(&loom, ns, &ns.to_string(), "intro")
            .unwrap()
            .unwrap();
        assert_eq!(intro.title, "Intro");
        let embed = loom_pages::get_page(&loom, ns, &ns.to_string(), "embed")
            .unwrap()
            .unwrap();
        let body = loom_substrate::body::Body::decode(embed.body.as_deref().unwrap()).unwrap();
        assert_eq!(body.blocks.len(), 3);

        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn import_meetings_bytes_writes_snapshot_payloads_and_history() {
        let temp = std::env::temp_dir().join(format!("loom-interchange-meetings-{}", now_ms()));
        let store_path = temp.join("meetings.loom");
        fs::create_dir_all(&temp).unwrap();
        let store = FileStore::create_with_profile(&store_path, Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        let workspace = WorkspaceId::from_bytes([77; 16]);
        loom.registry_mut()
            .create(FacetKind::Vcs, Some("studio"), workspace)
            .unwrap();
        let source_digest = Digest::hash(Algo::Blake3, b"source").to_string();
        let input = serde_json::json!({
            "snapshot_version": 1,
            "profile": "granola-app",
            "source_system": "granola-app",
            "source_scope": "local-cache",
            "observed_at": 500,
            "coverage": "complete",
            "items": [{
                "source_entity_id": "note-1",
                "source_digest": source_digest,
                "source_sidecar": {"id": "note-1", "raw": true},
                "title": "Planning",
                "summary_text": "Planning summary",
                "transcript_spans": [{"text": "Capture decisions."}],
                "decisions": [{"label": "Use normalized meeting imports."}]
            }]
        });
        let bytes = serde_json::to_vec(&input).unwrap();

        let result = import_meetings_bytes(
            &mut loom,
            workspace,
            InputProfile::GranolaApp,
            &bytes,
            false,
        )
        .unwrap();

        assert!(result.changed);
        assert_eq!(result.report.profile, "meetings");
        assert_eq!(result.report.source_scope, "local-cache");
        assert_eq!(result.report.rows_imported, 1);
        assert_eq!(
            result.report.operations_applied,
            result.report.operations_planned
        );
        let profile_id = workspace.to_string();
        let snapshot = load_meetings_snapshot(&loom, &profile_id).unwrap().unwrap();
        assert_eq!(snapshot.sources[0].source_id, "note-1");
        assert_eq!(snapshot.meetings[0].meeting_id, "meeting/note-1");
        assert_eq!(snapshot.annotations[0].kind, "Decision");
        assert_eq!(
            loom.read_file_reserved(
                workspace,
                &meetings_source_payload_path(&profile_id, "note-1", "summary.txt")
            )
            .unwrap(),
            b"Planning summary"
        );
        let index = RevisionIndex::decode(
            &loom
                .read_file_reserved(workspace, &revision_index_path(&profile_id).unwrap())
                .unwrap(),
        )
        .unwrap();
        assert_eq!(index.history("meeting:meeting/note-1").len(), 1);

        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn import_meetings_bytes_is_idempotent_and_merges_updates() {
        let temp =
            std::env::temp_dir().join(format!("loom-interchange-meetings-idempotent-{}", now_ms()));
        let store_path = temp.join("meetings.loom");
        fs::create_dir_all(&temp).unwrap();
        let store = FileStore::create_with_profile(&store_path, Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        let workspace = WorkspaceId::from_bytes([78; 16]);
        loom.registry_mut()
            .create(FacetKind::Vcs, Some("studio"), workspace)
            .unwrap();
        let source_digest = Digest::hash(Algo::Blake3, b"source").to_string();
        let input = |title: &str, observed_at: u64| {
            serde_json::to_vec(&serde_json::json!({
                "snapshot_version": 1,
                "profile": "granola-app",
                "source_system": "granola-app",
                "source_scope": "local-cache",
                "observed_at": observed_at,
                "coverage": "complete",
                "items": [{
                    "source_entity_id": "note-1",
                    "source_digest": source_digest,
                    "source_updated_at": observed_at,
                    "title": title,
                    "summary_text": title,
                    "transcript_spans": [{"text": title}],
                    "decisions": [{"label": title}]
                }]
            }))
            .unwrap()
        };
        let first = input("Planning", 500);

        let first_result = import_meetings_bytes(
            &mut loom,
            workspace,
            InputProfile::GranolaApp,
            &first,
            false,
        )
        .unwrap();
        let retry_result = import_meetings_bytes(
            &mut loom,
            workspace,
            InputProfile::GranolaApp,
            &first,
            false,
        )
        .unwrap();
        let updated = input("Planning updated", 600);
        let update_result = import_meetings_bytes(
            &mut loom,
            workspace,
            InputProfile::GranolaApp,
            &updated,
            false,
        )
        .unwrap();

        assert!(first_result.changed);
        assert!(!retry_result.changed);
        assert_eq!(retry_result.report.operations_applied, 0);
        assert!(
            retry_result
                .report
                .warnings
                .iter()
                .any(|warning| warning == "meetings snapshot already current")
        );
        assert!(update_result.changed);
        let profile_id = workspace.to_string();
        let snapshot = load_meetings_snapshot(&loom, &profile_id).unwrap().unwrap();
        assert_eq!(snapshot.meetings.len(), 1);
        assert_eq!(snapshot.meetings[0].title, "Planning updated");
        let index = RevisionIndex::decode(
            &loom
                .read_file_reserved(workspace, &revision_index_path(&profile_id).unwrap())
                .unwrap(),
        )
        .unwrap();
        assert_eq!(index.history("meeting:meeting/note-1").len(), 2);

        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn import_meetings_bytes_preserves_partial_coverage_state() {
        let temp =
            std::env::temp_dir().join(format!("loom-interchange-meetings-coverage-{}", now_ms()));
        let store_path = temp.join("meetings.loom");
        fs::create_dir_all(&temp).unwrap();
        let store = FileStore::create_with_profile(&store_path, Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        let workspace = WorkspaceId::from_bytes([79; 16]);
        loom.registry_mut()
            .create(FacetKind::Vcs, Some("studio"), workspace)
            .unwrap();
        let source_digest = Digest::hash(Algo::Blake3, b"source").to_string();
        let sidecar_digest = Digest::hash(Algo::Blake3, b"sidecar").to_string();
        let input = serde_json::json!({
            "snapshot_version": 1,
            "profile": "granola-mcp",
            "source_system": "granola-mcp",
            "source_scope": "assistant-session",
            "observed_at": 700,
            "coverage": "partial",
            "source_cursor": "cursor-1",
            "source_sidecar_digest": sidecar_digest,
            "coverage_gaps": ["missing-transcript"],
            "retry_windows": ["after-rate-limit"],
            "resume_state": "resume-token",
            "items": [{
                "source_entity_id": "note-1",
                "source_digest": source_digest,
                "title": "Planning",
                "summary_text": "Summary"
            }]
        });
        let bytes = serde_json::to_vec(&input).unwrap();

        let result = import_meetings_bytes(
            &mut loom,
            workspace,
            InputProfile::GranolaMcp,
            &bytes,
            false,
        )
        .unwrap();

        assert!(result.changed);
        let profile_id = workspace.to_string();
        let snapshot = load_meetings_snapshot(&loom, &profile_id).unwrap().unwrap();
        let run = &snapshot.import_runs[0];
        assert_eq!(run.source_cursor.as_deref(), Some("cursor-1"));
        assert_eq!(run.coverage_gaps, vec!["missing-transcript"]);
        assert_eq!(run.retry_windows, vec!["after-rate-limit"]);
        assert_eq!(run.resume_state.as_deref(), Some("resume-token"));
        assert_eq!(run.observed_ids, vec!["note-1"]);
        assert_eq!(run.coverage, MeetingsCoverage::Partial);

        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn import_meetings_broad_granola_fixture_preserves_mapping_boundary() {
        let temp =
            std::env::temp_dir().join(format!("loom-interchange-meetings-fixture-{}", now_ms()));
        let store_path = temp.join("meetings.loom");
        fs::create_dir_all(&temp).unwrap();
        let store = FileStore::create_with_profile(&store_path, Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        let workspace = WorkspaceId::from_bytes([80; 16]);
        loom.registry_mut()
            .create(FacetKind::Vcs, Some("studio"), workspace)
            .unwrap();
        let fixture = include_bytes!(
            "../../../specs/studio/fixtures/meetings/source/granola-broad-snapshot.json"
        );
        let expected: serde_json::Value = serde_json::from_slice(include_bytes!(
            "../../../specs/studio/fixtures/meetings/expected/comparison.json"
        ))
        .unwrap();

        let result = import_meetings_bytes(
            &mut loom,
            workspace,
            InputProfile::GranolaApp,
            fixture,
            false,
        )
        .unwrap();

        assert!(result.changed);
        assert_eq!(result.report.profile, "meetings");
        assert_eq!(
            result.report.source_scope,
            expected["source_scope"].as_str().unwrap()
        );
        assert_eq!(
            result.report.rows_imported,
            expected["rows_imported"].as_u64().unwrap()
        );
        assert_eq!(
            result.report.operations_applied,
            result.report.operations_planned
        );
        assert!(result.payload_bytes > 0);

        let profile_id = workspace.to_string();
        assert_eq!(profile_id, expected["workspace_id"].as_str().unwrap());
        let snapshot = load_meetings_snapshot(&loom, &profile_id).unwrap().unwrap();
        assert_eq!(
            snapshot.sources.len() as u64,
            expected["sources"].as_u64().unwrap()
        );
        assert_eq!(
            snapshot.meetings.len() as u64,
            expected["meetings"].as_u64().unwrap()
        );
        assert_eq!(
            snapshot.spans.len() as u64,
            expected["spans"].as_u64().unwrap()
        );
        assert_eq!(
            snapshot.annotations.len() as u64,
            expected["annotations"].as_u64().unwrap()
        );
        assert_eq!(
            snapshot.import_runs.len() as u64,
            expected["import_runs"].as_u64().unwrap()
        );

        let run = &snapshot.import_runs[0];
        let expected_ids: Vec<String> = expected["observed_ids"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap().to_string())
            .collect();
        let expected_gaps: Vec<String> = expected["coverage_gaps"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap().to_string())
            .collect();
        let expected_retry_windows: Vec<String> = expected["retry_windows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap().to_string())
            .collect();
        assert_eq!(run.input_profile, InputProfile::GranolaApp);
        assert_eq!(run.coverage, MeetingsCoverage::Partial);
        assert_eq!(run.source_cursor.as_deref(), Some("cursor-page-2"));
        assert_eq!(run.observed_ids, expected_ids);
        assert_eq!(run.coverage_gaps, expected_gaps);
        assert_eq!(run.retry_windows, expected_retry_windows);
        assert_eq!(
            run.resume_state.as_deref(),
            expected["resume_state"].as_str()
        );
        let checkpoint = ImportCheckpoint::decode(
            &loom
                .store()
                .control_get(&meetings_import_checkpoint_key(
                    &profile_id,
                    &run.import_run_id,
                ))
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(checkpoint.checkpoint_id, run.import_run_id);
        assert_eq!(checkpoint.profile, "meetings");
        assert_eq!(checkpoint.source_scope, run.source_scope);
        assert_eq!(checkpoint.observed_ids, run.observed_ids);
        assert_eq!(checkpoint.coverage_gaps, run.coverage_gaps);
        assert_eq!(checkpoint.retry_windows, run.retry_windows);
        assert_eq!(
            checkpoint.resume_state,
            run.resume_state.clone().unwrap().into_bytes()
        );
        assert_eq!(
            checkpoint.completed_units,
            run.observed_ids
                .iter()
                .map(|source_id| format!("source:{source_id}"))
                .collect::<Vec<_>>()
        );
        assert!(checkpoint.profile_state_digest.is_some());

        let complete_source = snapshot
            .sources
            .iter()
            .find(|source| source.source_id == "granola-note-complete")
            .unwrap();
        assert_eq!(complete_source.source_system, "granola");
        assert_eq!(
            complete_source.owner_principal.as_deref(),
            Some("principal:alice@example.com")
        );
        assert!(complete_source.sidecar_digest.is_some());

        let complete_meeting = snapshot
            .meetings
            .iter()
            .find(|meeting| meeting.meeting_id == "meeting/granola-note-complete")
            .unwrap();
        assert_eq!(complete_meeting.title, "Design review");
        assert_eq!(
            complete_meeting.calendar_event_ref.as_deref(),
            Some("calendar:cal-design-1720")
        );
        assert_eq!(
            complete_meeting.attendee_refs,
            vec![
                "principal:bob@example.com".to_string(),
                "principal:carol@example.com".to_string()
            ]
        );
        assert_eq!(
            complete_meeting.folder_refs,
            vec![
                "granola-folder:folder-design".to_string(),
                "granola-folder:folder-root".to_string()
            ]
        );
        assert_eq!(
            complete_meeting.summary_ref.as_deref(),
            Some("summary/granola-note-complete")
        );
        let retained_meeting = snapshot
            .meetings
            .iter()
            .find(|meeting| meeting.meeting_id == "meeting/granola-csv-row-legacy")
            .unwrap();
        assert_eq!(retained_meeting.status, MeetingStatus::RetainedMetadataOnly);

        let mut titles: Vec<&str> = snapshot
            .meetings
            .iter()
            .map(|meeting| meeting.title.as_str())
            .collect();
        titles.sort_unstable();
        let mut expected_titles = expected["meeting_titles"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect::<Vec<_>>();
        expected_titles.sort_unstable();
        assert_eq!(titles, expected_titles);
        let annotation_kinds: std::collections::BTreeSet<&str> = snapshot
            .annotations
            .iter()
            .map(|annotation| annotation.kind.as_str())
            .collect();
        assert_eq!(
            annotation_kinds.into_iter().collect::<Vec<_>>(),
            expected["annotation_kinds"]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| value.as_str().unwrap())
                .collect::<Vec<_>>()
        );

        assert_eq!(
            loom.read_file_reserved(
                workspace,
                &meetings_source_payload_path(&profile_id, "granola-note-complete", "summary.txt")
            )
            .unwrap(),
            b"The team chose normalized meeting imports and retained raw Granola sidecars."
        );
        let transcript = loom
            .read_file_reserved(
                workspace,
                &meetings_source_payload_path(
                    &profile_id,
                    "granola-note-complete",
                    "transcript.jsonl",
                ),
            )
            .unwrap();
        assert_eq!(transcript.split(|byte| *byte == b'\n').count(), 4);
        let sidecar = loom
            .read_file_reserved(
                workspace,
                &meetings_source_payload_path(&profile_id, "granola-note-complete", "source.json"),
            )
            .unwrap();
        let sidecar_json: serde_json::Value = serde_json::from_slice(&sidecar).unwrap();
        assert_eq!(
            sidecar_json["calendar_event"]["starts_at"].as_str(),
            Some("2024-07-03T10:00:00Z")
        );
        for source_id in [
            "granola-note-complete",
            "granola-note-partial",
            "granola-csv-row-legacy",
        ] {
            loom.read_file_reserved(
                workspace,
                &meetings_source_payload_path(&profile_id, source_id, "source.json"),
            )
            .unwrap();
            loom.read_file_reserved(
                workspace,
                &meetings_source_payload_path(&profile_id, source_id, "summary.txt"),
            )
            .unwrap();
        }

        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn import_meetings_profile_fixtures_execute_each_supported_profile() {
        struct ProfileCase {
            name: &'static str,
            input_profile: InputProfile,
            fixture: &'static [u8],
            coverage: MeetingsCoverage,
            rows: u64,
            source_scope: &'static str,
            first_source_id: &'static str,
            source_system: &'static str,
        }

        let cases = [
            ProfileCase {
                name: "granola-api",
                input_profile: InputProfile::GranolaApi,
                fixture: include_bytes!(
                    "../../../specs/studio/fixtures/meetings/source/granola-api-snapshot.json"
                ),
                coverage: MeetingsCoverage::Partial,
                rows: 3,
                source_scope: "granola-api://account/acme/filter/updated-since-2024-07-01",
                first_source_id: "api-note-visible",
                source_system: "granola-api",
            },
            ProfileCase {
                name: "granola-mcp",
                input_profile: InputProfile::GranolaMcp,
                fixture: include_bytes!(
                    "../../../specs/studio/fixtures/meetings/source/granola-mcp-snapshot.json"
                ),
                coverage: MeetingsCoverage::Partial,
                rows: 1,
                source_scope: "mcp://granola/session/session-123/note/mcp-note-1",
                first_source_id: "mcp-note-1",
                source_system: "granola-mcp",
            },
            ProfileCase {
                name: "csv",
                input_profile: InputProfile::Csv,
                fixture: include_bytes!(
                    "../../../specs/studio/fixtures/meetings/source/granola-csv-snapshot.json"
                ),
                coverage: MeetingsCoverage::Degraded,
                rows: 2,
                source_scope: "file:///imports/granola-notes.csv",
                first_source_id: "csv-row-1",
                source_system: "granola-csv",
            },
        ];

        for (index, case) in cases.iter().enumerate() {
            let temp = std::env::temp_dir().join(format!(
                "loom-interchange-meetings-profile-{}-{}",
                case.name,
                now_ms()
            ));
            let store_path = temp.join("meetings.loom");
            fs::create_dir_all(&temp).unwrap();
            let store = FileStore::create_with_profile(&store_path, Algo::Blake3).unwrap();
            let mut loom = Loom::new(store);
            let workspace = WorkspaceId::from_bytes([(81 + index) as u8; 16]);
            loom.registry_mut()
                .create(FacetKind::Vcs, Some("studio"), workspace)
                .unwrap();

            let result = import_meetings_bytes(
                &mut loom,
                workspace,
                case.input_profile,
                case.fixture,
                false,
            )
            .unwrap();

            assert!(result.changed, "{}", case.name);
            assert_eq!(result.report.rows_imported, case.rows, "{}", case.name);
            assert_eq!(
                result.report.source_scope, case.source_scope,
                "{}",
                case.name
            );
            let profile_id = workspace.to_string();
            let snapshot = load_meetings_snapshot(&loom, &profile_id).unwrap().unwrap();
            let run = &snapshot.import_runs[0];
            assert_eq!(run.input_profile, case.input_profile, "{}", case.name);
            assert_eq!(run.coverage, case.coverage, "{}", case.name);
            assert_eq!(run.source_scope, case.source_scope, "{}", case.name);
            let first_source = snapshot
                .sources
                .iter()
                .find(|source| source.source_id == case.first_source_id)
                .unwrap();
            assert_eq!(
                first_source.source_system, case.source_system,
                "{}",
                case.name
            );
            loom.read_file_reserved(
                workspace,
                &meetings_source_payload_path(&profile_id, case.first_source_id, "source.json"),
            )
            .unwrap();
            if case.name == "granola-api" {
                let deleted = snapshot
                    .meetings
                    .iter()
                    .find(|meeting| meeting.meeting_id == "meeting/api-note-deleted")
                    .unwrap();
                assert_eq!(deleted.status, MeetingStatus::DeletedAtSource);
            }
            if case.name == "csv" {
                let retained = snapshot
                    .meetings
                    .iter()
                    .find(|meeting| meeting.meeting_id == "meeting/csv-row-2")
                    .unwrap();
                assert_eq!(retained.status, MeetingStatus::RetainedMetadataOnly);
            }

            fs::remove_dir_all(temp).unwrap();
        }
    }

    #[test]
    fn import_meetings_execution_fidelity_vectors_pass() {
        fn fixture_bytes(name: &str) -> &'static [u8] {
            match name {
                "granola-broad-snapshot.json" => include_bytes!(
                    "../../../specs/studio/fixtures/meetings/source/granola-broad-snapshot.json"
                ),
                "granola-api-snapshot.json" => include_bytes!(
                    "../../../specs/studio/fixtures/meetings/source/granola-api-snapshot.json"
                ),
                "granola-mcp-snapshot.json" => include_bytes!(
                    "../../../specs/studio/fixtures/meetings/source/granola-mcp-snapshot.json"
                ),
                "granola-csv-snapshot.json" => include_bytes!(
                    "../../../specs/studio/fixtures/meetings/source/granola-csv-snapshot.json"
                ),
                other => panic!("unknown meetings fixture {other}"),
            }
        }

        fn coverage_label(coverage: MeetingsCoverage) -> &'static str {
            match coverage {
                MeetingsCoverage::Complete => "complete",
                MeetingsCoverage::Partial => "partial",
                MeetingsCoverage::Degraded => "degraded",
            }
        }

        fn status_label(status: MeetingStatus) -> &'static str {
            match status {
                MeetingStatus::Active => "active",
                MeetingStatus::DeletedAtSource => "deleted_at_source",
                MeetingStatus::Redacted => "redacted",
                MeetingStatus::RetainedMetadataOnly => "retained_metadata_only",
            }
        }

        let vectors: serde_json::Value = serde_json::from_slice(include_bytes!(
            "../../../specs/studio/fixtures/meetings/expected/execution-fidelity.json"
        ))
        .unwrap();
        let cases = vectors["cases"].as_array().unwrap();
        for (index, case) in cases.iter().enumerate() {
            let temp = std::env::temp_dir().join(format!(
                "loom-interchange-meetings-vector-{}-{}",
                case["name"].as_str().unwrap(),
                now_ms()
            ));
            let store_path = temp.join("meetings.loom");
            fs::create_dir_all(&temp).unwrap();
            let store = FileStore::create_with_profile(&store_path, Algo::Blake3).unwrap();
            let mut loom = Loom::new(store);
            let workspace = WorkspaceId::from_bytes([(90 + index) as u8; 16]);
            loom.registry_mut()
                .create(FacetKind::Vcs, Some("studio"), workspace)
                .unwrap();
            let profile =
                parse_meetings_input_profile(case["input_profile"].as_str().unwrap()).unwrap();
            let fixture = fixture_bytes(case["fixture"].as_str().unwrap());

            let first =
                import_meetings_bytes(&mut loom, workspace, profile, fixture, false).unwrap();
            let retry =
                import_meetings_bytes(&mut loom, workspace, profile, fixture, false).unwrap();

            assert!(first.changed, "{}", case["name"]);
            assert!(!retry.changed, "{}", case["name"]);
            assert_eq!(retry.report.operations_applied, 0, "{}", case["name"]);
            assert!(
                retry
                    .report
                    .warnings
                    .iter()
                    .any(|warning| warning == vectors["idempotent_warning"].as_str().unwrap()),
                "{}",
                case["name"]
            );
            assert_eq!(
                first.report.source_scope,
                case["source_scope"].as_str().unwrap(),
                "{}",
                case["name"]
            );
            assert_eq!(
                first.report.rows_imported,
                case["rows_imported"].as_u64().unwrap(),
                "{}",
                case["name"]
            );

            let profile_id = workspace.to_string();
            let snapshot = load_meetings_snapshot(&loom, &profile_id).unwrap().unwrap();
            assert_eq!(
                snapshot.sources.len() as u64,
                case["sources"].as_u64().unwrap(),
                "{}",
                case["name"]
            );
            assert_eq!(
                snapshot.meetings.len() as u64,
                case["meetings"].as_u64().unwrap(),
                "{}",
                case["name"]
            );
            assert_eq!(
                snapshot.spans.len() as u64,
                case["spans"].as_u64().unwrap(),
                "{}",
                case["name"]
            );
            assert_eq!(
                snapshot.annotations.len() as u64,
                case["annotations"].as_u64().unwrap(),
                "{}",
                case["name"]
            );
            let run = &snapshot.import_runs[0];
            assert_eq!(
                coverage_label(run.coverage),
                case["coverage"].as_str().unwrap(),
                "{}",
                case["name"]
            );
            let checkpoint = ImportCheckpoint::decode(
                &loom
                    .store()
                    .control_get(&meetings_import_checkpoint_key(
                        &profile_id,
                        &run.import_run_id,
                    ))
                    .unwrap()
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(
                checkpoint.observed_ids, run.observed_ids,
                "{}",
                case["name"]
            );
            assert_eq!(
                checkpoint.coverage_gaps, run.coverage_gaps,
                "{}",
                case["name"]
            );
            assert_eq!(
                checkpoint.retry_windows, run.retry_windows,
                "{}",
                case["name"]
            );
            assert!(
                checkpoint.profile_state_digest.is_some(),
                "{}",
                case["name"]
            );

            for source_id in case["payload_sources"].as_array().unwrap() {
                loom.read_file_reserved(
                    workspace,
                    &meetings_source_payload_path(
                        &profile_id,
                        source_id.as_str().unwrap(),
                        "source.json",
                    ),
                )
                .unwrap();
            }
            let expected_statuses = case["meeting_statuses"].as_object().unwrap();
            for meeting in &snapshot.meetings {
                assert_eq!(
                    status_label(meeting.status),
                    expected_statuses[&meeting.meeting_id].as_str().unwrap(),
                    "{}",
                    case["name"]
                );
            }

            fs::remove_dir_all(temp).unwrap();
        }

        let temp = std::env::temp_dir().join(format!(
            "loom-interchange-meetings-vector-invalid-{}",
            now_ms()
        ));
        let store_path = temp.join("meetings.loom");
        fs::create_dir_all(&temp).unwrap();
        let store = FileStore::create_with_profile(&store_path, Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        let workspace = WorkspaceId::from_bytes([95; 16]);
        loom.registry_mut()
            .create(FacetKind::Vcs, Some("studio"), workspace)
            .unwrap();
        let source_digest = Digest::hash(Algo::Blake3, b"source").to_string();
        let invalid = serde_json::to_vec(&serde_json::json!({
            "snapshot_version": 1,
            "profile": "granola-app",
            "source_system": "granola-app",
            "source_scope": "local-cache",
            "observed_at": 500,
            "coverage": "complete",
            "items": [{
                "source_entity_id": "note-1",
                "source_digest": source_digest,
                "source_state": "missing",
                "title": "Invalid state"
            }]
        }))
        .unwrap();
        let err = import_meetings_bytes(
            &mut loom,
            workspace,
            InputProfile::GranolaApp,
            &invalid,
            false,
        )
        .unwrap_err();
        assert_eq!(err.code, Code::InvalidArgument);
        assert!(
            err.message
                .contains(vectors["invalid_source_state_message"].as_str().unwrap())
        );
        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn import_meetings_rejects_unknown_source_state() {
        let temp =
            std::env::temp_dir().join(format!("loom-interchange-meetings-state-{}", now_ms()));
        let store_path = temp.join("meetings.loom");
        fs::create_dir_all(&temp).unwrap();
        let store = FileStore::create_with_profile(&store_path, Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        let workspace = WorkspaceId::from_bytes([84; 16]);
        loom.registry_mut()
            .create(FacetKind::Vcs, Some("studio"), workspace)
            .unwrap();
        let source_digest = Digest::hash(Algo::Blake3, b"source").to_string();
        let input = serde_json::json!({
            "snapshot_version": 1,
            "profile": "granola-app",
            "source_system": "granola-app",
            "source_scope": "local-cache",
            "observed_at": 500,
            "coverage": "complete",
            "items": [{
                "source_entity_id": "note-1",
                "source_digest": source_digest,
                "source_state": "missing",
                "title": "Invalid state"
            }]
        });
        let bytes = serde_json::to_vec(&input).unwrap();

        let err = import_meetings_bytes(
            &mut loom,
            workspace,
            InputProfile::GranolaApp,
            &bytes,
            false,
        )
        .unwrap_err();

        assert_eq!(err.code, Code::InvalidArgument);
        assert!(err.message.contains("unsupported meetings source_state"));

        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn import_fs_writes_files_and_directories() {
        let temp = std::env::temp_dir().join(format!("loom-interchange-import-{}", now_ms()));
        fs::create_dir_all(temp.join("docs/empty")).unwrap();
        fs::write(temp.join("README.md"), b"hello").unwrap();
        fs::write(temp.join("docs/a.txt"), b"a").unwrap();

        let mut loom = Loom::new(MemoryStore::new());
        let ns = create_test_workspace(&mut loom, 1);
        let mut options = FsImportOptions::new("temp");
        options.commit = true;
        let report = import_fs(&mut loom, ns, &temp, &options).unwrap();

        assert_eq!(report.operations_planned, 4);
        assert_eq!(report.operations_applied, 4);
        assert!(report.objects_added > 0);
        assert!(report.commit.is_some());
        assert_eq!(loom.read_file(ns, "README.md").unwrap(), b"hello");
        assert_eq!(loom.read_file(ns, "docs/a.txt").unwrap(), b"a");
        assert!(loom.exists(ns, "docs/empty").unwrap());

        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn table_csv_import_export_preserves_decimal_and_empty_text() {
        let temp = std::env::temp_dir().join(format!("loom-interchange-table-{}", now_ms()));
        fs::create_dir_all(&temp).unwrap();
        let csv = temp.join("items.csv");
        fs::write(
            &csv,
            b"id,name,amount,active,day,ts\n1,\"\",123.4500,true,42,1000\n2,plain,-0.0500,,43,1001\n",
        )
        .unwrap();
        let out = temp.join("out.csv");
        let mut loom = Loom::new(MemoryStore::new());
        let ns = create_test_workspace(&mut loom, 25);
        let mut options = TableCsvImportOptions::new(
            "items.csv",
            "app",
            "items",
            vec![
                ("id".to_string(), ColumnType::Int),
                ("name".to_string(), ColumnType::Text),
                ("amount".to_string(), ColumnType::Decimal),
                ("active".to_string(), ColumnType::Bool),
                ("day".to_string(), ColumnType::Date),
                ("ts".to_string(), ColumnType::Timestamp),
            ],
            vec!["id".to_string()],
        );
        options.commit = true;

        let report = import_table_csv(&mut loom, ns, &csv, &options).unwrap();

        assert_eq!(report.rows_imported, 2);
        assert_eq!(report.operations_applied, 2);
        assert!(report.commit.is_some());
        let table = get_table(&loom, ns, &table_csv_path("app", "items").unwrap()).unwrap();
        assert_eq!(
            table.get(&[Value::Int(1)]).unwrap()[2],
            Value::Decimal {
                mantissa: 1_234_500,
                scale: 4
            }
        );
        assert_eq!(
            table.get(&[Value::Int(1)]).unwrap()[1],
            Value::Text(String::new())
        );
        assert_eq!(table.get(&[Value::Int(2)]).unwrap()[3], Value::Null);

        let export_options = TableCsvExportOptions::new("out.csv", "app", "items");
        let export = export_table_csv(&loom, ns, &out, &export_options).unwrap();

        assert_eq!(export.rows_written, 2);
        let exported = fs::read_to_string(&out).unwrap();
        assert!(exported.contains("1,\"\",123.4500,true,42,1000\n"));
        assert!(exported.contains("2,plain,-0.0500,,43,1001\n"));

        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn table_csv_import_rejects_invalid_decimal() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = create_test_workspace(&mut loom, 26);
        let options = TableCsvImportOptions::new(
            "bad.csv",
            "app",
            "items",
            vec![
                ("id".to_string(), ColumnType::Int),
                ("amount".to_string(), ColumnType::Decimal),
            ],
            vec!["id".to_string()],
        );

        let err =
            import_table_csv_bytes(&mut loom, ns, b"id,amount\n1,1e-3\n", &options).unwrap_err();

        assert_eq!(err.code, Code::InvalidArgument);
        assert!(err.message.contains("expects a decimal"));
    }

    #[test]
    fn table_csv_append_only_rejects_duplicate_primary_key() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = create_test_workspace(&mut loom, 27);
        let mut options = TableCsvImportOptions::new(
            "items.csv",
            "app",
            "items",
            vec![
                ("id".to_string(), ColumnType::Int),
                ("name".to_string(), ColumnType::Text),
            ],
            vec!["id".to_string()],
        );
        import_table_csv_bytes(&mut loom, ns, b"id,name\n1,a\n", &options).unwrap();
        options.mode = TableImportMode::AppendOnly;

        let err = import_table_csv_bytes(&mut loom, ns, b"id,name\n1,b\n", &options).unwrap_err();

        assert_eq!(err.code, Code::AlreadyExists);
    }

    #[test]
    fn table_csv_import_export_require_sql_acl() {
        let temp = std::env::temp_dir().join(format!("loom-interchange-table-acl-{}", now_ms()));
        fs::create_dir_all(&temp).unwrap();
        let out = temp.join("items.csv");
        let mut loom = Loom::new(MemoryStore::new());
        let ns = create_test_workspace(&mut loom, 28);
        let root = WorkspaceId::from_bytes([128; 16]);
        authenticate_root(&mut loom, root);
        let import_options = TableCsvImportOptions::new(
            "items.csv",
            "app",
            "items",
            vec![("id".to_string(), ColumnType::Int)],
            vec!["id".to_string()],
        );

        assert_eq!(
            import_table_csv_bytes(&mut loom, ns, b"id\n1\n", &import_options)
                .unwrap_err()
                .code,
            Code::PermissionDenied
        );
        let mut acl = loom.acl_store().clone();
        acl.grant(AclGrant {
            subject: AclSubject::Principal(root),
            workspace: Some(ns),
            domain: Some(FacetKind::Sql.into()),
            ref_glob: None,
            scopes: vec![AclScope::All],
            rights: [AclRight::Write].into_iter().collect(),
            effect: AclEffect::Allow,
            predicate: None,
        })
        .unwrap();
        loom.set_acl_store(acl);
        import_table_csv_bytes(&mut loom, ns, b"id\n1\n", &import_options).unwrap();

        let export_options = TableCsvExportOptions::new("items.csv", "app", "items");
        assert_eq!(
            export_table_csv(&loom, ns, &out, &export_options)
                .unwrap_err()
                .code,
            Code::PermissionDenied
        );
        let mut acl = loom.acl_store().clone();
        acl.grant(AclGrant {
            subject: AclSubject::Principal(root),
            workspace: Some(ns),
            domain: Some(FacetKind::Sql.into()),
            ref_glob: None,
            scopes: vec![AclScope::All],
            rights: [AclRight::Read].into_iter().collect(),
            effect: AclEffect::Allow,
            predicate: None,
        })
        .unwrap();
        loom.set_acl_store(acl);
        export_table_csv(&loom, ns, &out, &export_options).unwrap();
        assert_eq!(fs::read_to_string(&out).unwrap(), "id\n1\n");

        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn export_fs_materializes_files() {
        let temp = std::env::temp_dir().join(format!("loom-interchange-export-{}", now_ms()));
        let mut loom = Loom::new(MemoryStore::new());
        let ns = create_test_workspace(&mut loom, 2);
        loom.create_directory(ns, "docs", true).unwrap();
        loom.write_file(ns, "docs/a.txt", b"a", 0o100644).unwrap();

        let report = export_fs(&loom, ns, &temp, &FsExportOptions::new("temp")).unwrap();
        assert_eq!(report.files_written, 1);
        assert_eq!(report.bytes_out, 1);
        assert_eq!(fs::read(temp.join("docs/a.txt")).unwrap(), b"a");

        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn export_fs_revision_materializes_selected_commit() {
        let temp = std::env::temp_dir().join(format!("loom-interchange-export-rev-{}", now_ms()));
        let mut loom = Loom::new(MemoryStore::new());
        let ns = create_test_workspace(&mut loom, 8);
        loom.write_file(ns, "note.txt", b"v1", 0o100644).unwrap();
        let first = loom.commit(ns, "nas", "first", 1).unwrap();
        loom.tag_create(ns, "first", &first.to_string(), "nas", "", 2)
            .unwrap();
        loom.write_file(ns, "note.txt", b"v2", 0o100644).unwrap();

        let mut options = FsExportOptions::new("temp");
        options.revision = Some("tag:first".to_string());
        let report = export_fs(&loom, ns, &temp, &options).unwrap();

        assert_eq!(report.files_written, 1);
        assert_eq!(report.bytes_out, 2);
        assert_eq!(fs::read(temp.join("note.txt")).unwrap(), b"v1");
        assert_eq!(loom.read_file(ns, "note.txt").unwrap(), b"v2");

        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn dry_run_reports_without_writing() {
        let temp = std::env::temp_dir().join(format!("loom-interchange-dry-{}", now_ms()));
        fs::create_dir_all(&temp).unwrap();
        fs::write(temp.join("a.txt"), b"a").unwrap();

        let mut loom = Loom::new(MemoryStore::new());
        let ns = create_test_workspace(&mut loom, 3);
        let mut options = FsImportOptions::new("temp");
        options.dry_run = true;
        let report = import_fs(&mut loom, ns, &temp, &options).unwrap();

        assert_eq!(report.operations_planned, 1);
        assert_eq!(report.operations_applied, 0);
        assert_eq!(report.objects_added, 0);
        assert!(loom.read_file(ns, "a.txt").is_err());

        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn filesystem_import_export_require_file_acl() {
        let temp = std::env::temp_dir().join(format!("loom-interchange-fs-acl-{}", now_ms()));
        let src = temp.join("src");
        let dst = temp.join("dst");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("a.txt"), b"a").unwrap();

        let mut loom = Loom::new(MemoryStore::new());
        let ns = create_test_workspace(&mut loom, 16);
        let root = WorkspaceId::from_bytes([116; 16]);
        authenticate_root(&mut loom, root);

        let mut import_options = FsImportOptions::new("src");
        import_options.dry_run = true;
        assert_eq!(
            import_fs(&mut loom, ns, &src, &import_options)
                .unwrap_err()
                .code,
            Code::PermissionDenied
        );
        loom.acl_store_mut()
            .allow(
                AclSubject::Principal(root),
                Some(ns),
                Some(FacetKind::Files),
                [AclRight::Write],
            )
            .unwrap();
        import_fs(&mut loom, ns, &src, &import_options).unwrap();

        assert_eq!(
            export_fs(&loom, ns, &dst, &FsExportOptions::new("dst"))
                .unwrap_err()
                .code,
            Code::PermissionDenied
        );
        loom.acl_store_mut()
            .allow(
                AclSubject::Principal(root),
                Some(ns),
                Some(FacetKind::Files),
                [AclRight::Read],
            )
            .unwrap();
        let report = export_fs(&loom, ns, &dst, &FsExportOptions::new("dst")).unwrap();
        assert_eq!(report.files_written, 0);

        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn archive_import_export_require_file_acl() {
        let temp = std::env::temp_dir().join(format!("loom-interchange-archive-acl-{}", now_ms()));
        fs::create_dir_all(&temp).unwrap();
        let archive = temp.join("notes.zip");
        write_zip(&archive, &[("a.txt", b"a".as_slice())], &[]);
        let out = temp.join("out.zip");

        let mut loom = Loom::new(MemoryStore::new());
        let ns = create_test_workspace(&mut loom, 17);
        let root = WorkspaceId::from_bytes([117; 16]);
        authenticate_root(&mut loom, root);

        let mut import_options = ArchiveImportOptions::new("notes.zip");
        import_options.dry_run = true;
        assert_eq!(
            import_archive(&mut loom, ns, &archive, ArchiveKind::Zip, &import_options)
                .unwrap_err()
                .code,
            Code::PermissionDenied
        );
        loom.acl_store_mut()
            .allow(
                AclSubject::Principal(root),
                Some(ns),
                Some(FacetKind::Files),
                [AclRight::Write],
            )
            .unwrap();
        import_archive(&mut loom, ns, &archive, ArchiveKind::Zip, &import_options).unwrap();

        assert_eq!(
            export_archive(
                &loom,
                ns,
                &out,
                ArchiveKind::Zip,
                &ArchiveExportOptions::new("out.zip"),
            )
            .unwrap_err()
            .code,
            Code::PermissionDenied
        );
        loom.acl_store_mut()
            .allow(
                AclSubject::Principal(root),
                Some(ns),
                Some(FacetKind::Files),
                [AclRight::Read],
            )
            .unwrap();
        export_archive(
            &loom,
            ns,
            &out,
            ArchiveKind::Zip,
            &ArchiveExportOptions::new("out.zip"),
        )
        .unwrap();

        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn zip_archive_import_writes_files_and_manifest() {
        let temp = std::env::temp_dir().join(format!("loom-interchange-zip-{}", now_ms()));
        fs::create_dir_all(&temp).unwrap();
        let archive = temp.join("notes.zip");
        write_zip(&archive, &[("docs/a.txt", b"a".as_slice())], &["docs/"]);

        let mut loom = Loom::new(MemoryStore::new());
        let ns = create_test_workspace(&mut loom, 4);
        let result = import_archive(
            &mut loom,
            ns,
            &archive,
            ArchiveKind::Zip,
            &ArchiveImportOptions::new("notes.zip"),
        )
        .unwrap();

        assert_eq!(result.manifest.kind, ArchiveKind::Zip);
        assert_eq!(result.manifest.entries.len(), 2);
        assert_eq!(result.report.operations_planned, 2);
        assert_eq!(result.report.operations_applied, 2);
        assert!(result.report.objects_added > 0);
        assert_eq!(loom.read_file(ns, "docs/a.txt").unwrap(), b"a");

        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn tar_archive_import_writes_files_and_manifest() {
        let temp = std::env::temp_dir().join(format!("loom-interchange-tar-{}", now_ms()));
        fs::create_dir_all(&temp).unwrap();
        let archive = temp.join("notes.tar");
        write_tar(&archive, &[("docs/a.txt", b"a".as_slice())]);

        let mut loom = Loom::new(MemoryStore::new());
        let ns = create_test_workspace(&mut loom, 5);
        let result = import_archive(
            &mut loom,
            ns,
            &archive,
            ArchiveKind::Tar,
            &ArchiveImportOptions::new("notes.tar"),
        )
        .unwrap();

        assert_eq!(result.manifest.kind, ArchiveKind::Tar);
        assert_eq!(result.report.operations_applied, 1);
        assert_eq!(loom.read_file(ns, "docs/a.txt").unwrap(), b"a");

        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn gzip_archive_import_writes_single_file() {
        let temp = std::env::temp_dir().join(format!("loom-interchange-gzip-{}", now_ms()));
        fs::create_dir_all(&temp).unwrap();
        let archive = temp.join("notes.txt.gz");
        write_gzip(&archive, b"compressed");

        let mut loom = Loom::new(MemoryStore::new());
        let ns = create_test_workspace(&mut loom, 6);
        let result = import_archive(
            &mut loom,
            ns,
            &archive,
            ArchiveKind::Gzip,
            &ArchiveImportOptions::new("notes.txt.gz"),
        )
        .unwrap();

        assert_eq!(result.manifest.kind, ArchiveKind::Gzip);
        assert_eq!(result.manifest.entries[0].path, "notes.txt");
        assert_eq!(loom.read_file(ns, "notes.txt").unwrap(), b"compressed");

        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn tar_gzip_archive_import_writes_files_and_manifest() {
        let temp = std::env::temp_dir().join(format!("loom-interchange-targzip-{}", now_ms()));
        fs::create_dir_all(&temp).unwrap();
        let archive = temp.join("notes.tar.gz");
        write_tar_gzip(&archive, &[("docs/a.txt", b"a".as_slice())]);

        let mut loom = Loom::new(MemoryStore::new());
        let ns = create_test_workspace(&mut loom, 12);
        let result = import_archive(
            &mut loom,
            ns,
            &archive,
            ArchiveKind::TarGzip,
            &ArchiveImportOptions::new("notes.tar.gz"),
        )
        .unwrap();

        assert_eq!(result.manifest.kind, ArchiveKind::TarGzip);
        assert_eq!(result.report.operations_applied, 1);
        assert_eq!(loom.read_file(ns, "docs/a.txt").unwrap(), b"a");

        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn tar_archive_import_rejects_symlink_entries() {
        let temp = std::env::temp_dir().join(format!("loom-interchange-tar-symlink-{}", now_ms()));
        fs::create_dir_all(&temp).unwrap();
        let archive = temp.join("links.tar");
        write_tar_symlink(&archive, "docs/link", "../target");

        let mut loom = Loom::new(MemoryStore::new());
        let ns = create_test_workspace(&mut loom, 13);
        let err = import_archive(
            &mut loom,
            ns,
            &archive,
            ArchiveKind::Tar,
            &ArchiveImportOptions::new("links.tar"),
        )
        .unwrap_err();

        assert_eq!(err.code, Code::Unsupported);
        assert!(loom.read_file(ns, "docs/link").is_err());

        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn zip_archive_import_rejects_symlink_entries() {
        let temp = std::env::temp_dir().join(format!("loom-interchange-zip-symlink-{}", now_ms()));
        fs::create_dir_all(&temp).unwrap();
        let archive = temp.join("links.zip");
        write_zip_symlink(&archive, "docs/link", "../target");

        let mut loom = Loom::new(MemoryStore::new());
        let ns = create_test_workspace(&mut loom, 14);
        let err = import_archive(
            &mut loom,
            ns,
            &archive,
            ArchiveKind::Zip,
            &ArchiveImportOptions::new("links.zip"),
        )
        .unwrap_err();

        assert_eq!(err.code, Code::Unsupported);
        assert!(loom.read_file(ns, "docs/link").is_err());

        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn zip_archive_import_rejects_encrypted_entries() {
        let temp =
            std::env::temp_dir().join(format!("loom-interchange-zip-encrypted-{}", now_ms()));
        fs::create_dir_all(&temp).unwrap();
        let archive = temp.join("encrypted.zip");
        write_zip(&archive, &[("secret.txt", b"secret".as_slice())], &[]);
        mark_zip_entry_encrypted(&archive);

        let mut loom = Loom::new(MemoryStore::new());
        let ns = create_test_workspace(&mut loom, 15);
        let err = import_archive(
            &mut loom,
            ns,
            &archive,
            ArchiveKind::Zip,
            &ArchiveImportOptions::new("encrypted.zip"),
        )
        .unwrap_err();

        assert_eq!(err.code, Code::Unsupported);
        assert!(loom.read_file(ns, "secret.txt").is_err());

        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn tar_zstd_archive_export_is_deterministic_and_imports() {
        let temp = std::env::temp_dir().join(format!("loom-interchange-tarzstd-{}", now_ms()));
        fs::create_dir_all(&temp).unwrap();
        let first_archive = temp.join("first.tar.zstd");
        let second_archive = temp.join("second.tar.zstd");

        let mut src = Loom::new(MemoryStore::new());
        let ns = create_test_workspace(&mut src, 10);
        src.create_directory(ns, "docs", true).unwrap();
        src.write_file(ns, "docs/a.txt", b"alpha", 0o100644)
            .unwrap();

        let first = export_archive(
            &src,
            ns,
            &first_archive,
            ArchiveKind::TarZstd,
            &ArchiveExportOptions::new("first.tar.zstd"),
        )
        .unwrap();
        let second = export_archive(
            &src,
            ns,
            &second_archive,
            ArchiveKind::TarZstd,
            &ArchiveExportOptions::new("second.tar.zstd"),
        )
        .unwrap();
        assert_eq!(first.manifest.entries, second.manifest.entries);
        assert_eq!(
            fs::read(&first_archive).unwrap(),
            fs::read(&second_archive).unwrap()
        );

        let mut dst = Loom::new(MemoryStore::new());
        let dst_ns = create_test_workspace(&mut dst, 11);
        let imported = import_archive(
            &mut dst,
            dst_ns,
            &first_archive,
            ArchiveKind::TarZstd,
            &ArchiveImportOptions::new("first.tar.zstd"),
        )
        .unwrap();
        assert_eq!(imported.manifest.kind, ArchiveKind::TarZstd);
        assert_eq!(dst.read_file(dst_ns, "docs/a.txt").unwrap(), b"alpha");

        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn archive_import_rejects_path_traversal() {
        let temp = std::env::temp_dir().join(format!("loom-interchange-escape-{}", now_ms()));
        fs::create_dir_all(&temp).unwrap();
        let archive = temp.join("escape.zip");
        write_zip(&archive, &[("../escape.txt", b"bad".as_slice())], &[]);

        let mut loom = Loom::new(MemoryStore::new());
        let ns = create_test_workspace(&mut loom, 7);
        assert!(
            import_archive(
                &mut loom,
                ns,
                &archive,
                ArchiveKind::Zip,
                &ArchiveImportOptions::new("escape.zip"),
            )
            .is_err()
        );
        assert!(loom.read_file(ns, "escape.txt").is_err());

        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn car_export_is_deterministic_and_imports_workspace_graph() {
        let temp = std::env::temp_dir().join(format!("loom-interchange-car-{}", now_ms()));
        fs::create_dir_all(&temp).unwrap();
        let first_car = temp.join("first.car");
        let second_car = temp.join("second.car");

        let mut src = Loom::new(MemoryStore::new());
        let ns = create_test_workspace(&mut src, 9);
        src.create_directory(ns, "docs", true).unwrap();
        src.write_file(ns, "docs/a.txt", b"alpha", 0o100644)
            .unwrap();
        let tip = src.commit(ns, "nas", "snapshot", 1).unwrap();
        src.registry_mut().tag_create(ns, "v1", tip).unwrap();

        let first = export_car(&src, ns, &first_car, &CarExportOptions::new("first.car")).unwrap();
        let second =
            export_car(&src, ns, &second_car, &CarExportOptions::new("second.car")).unwrap();
        assert_eq!(first.blocks_written, second.blocks_written);
        assert_eq!(first.root_cid_hex, second.root_cid_hex);
        assert_eq!(
            fs::read(&first_car).unwrap(),
            fs::read(&second_car).unwrap()
        );

        let mut dst = Loom::new(MemoryStore::new());
        let imported =
            import_car(&mut dst, &first_car, &CarImportOptions::new("first.car")).unwrap();
        let imported_ns = imported.workspace.unwrap();
        assert_eq!(imported_ns, ns);
        assert_eq!(imported.root_cid_hex, first.root_cid_hex);
        assert_eq!(imported.blocks_read, first.blocks_written);
        assert_eq!(
            dst.registry().branch_tip(imported_ns, "main").unwrap(),
            Some(tip)
        );
        assert_eq!(
            dst.registry().tag_target(imported_ns, "v1").unwrap(),
            Some(tip)
        );
        dst.checkout_commit(imported_ns, tip).unwrap();
        assert_eq!(dst.read_file(imported_ns, "docs/a.txt").unwrap(), b"alpha");

        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn car_import_requires_vcs_write_acl() {
        let temp = std::env::temp_dir().join(format!("loom-interchange-car-acl-{}", now_ms()));
        fs::create_dir_all(&temp).unwrap();
        let car = temp.join("source.car");

        let mut src = Loom::new(MemoryStore::new());
        let ns = create_test_workspace(&mut src, 18);
        src.write_file(ns, "a.txt", b"a", 0o100644).unwrap();
        src.commit(ns, "nas", "snapshot", 1).unwrap();
        export_car(&src, ns, &car, &CarExportOptions::new("source.car")).unwrap();

        let mut dst = Loom::new(MemoryStore::new());
        let root = WorkspaceId::from_bytes([118; 16]);
        authenticate_root(&mut dst, root);
        assert_eq!(
            import_car(&mut dst, &car, &CarImportOptions::new("source.car"))
                .unwrap_err()
                .code,
            Code::PermissionDenied
        );
        dst.acl_store_mut()
            .allow(
                AclSubject::Principal(root),
                Some(ns),
                Some(FacetKind::Vcs),
                [AclRight::Write],
            )
            .unwrap();
        assert_eq!(
            import_car(&mut dst, &car, &CarImportOptions::new("source.car"))
                .unwrap_err()
                .code,
            Code::PermissionDenied
        );
        dst.acl_store_mut()
            .allow(AclSubject::Principal(root), None, None, [AclRight::Admin])
            .unwrap();
        let imported = import_car(&mut dst, &car, &CarImportOptions::new("source.car")).unwrap();
        assert_eq!(imported.workspace, Some(ns));

        fs::remove_dir_all(temp).unwrap();
    }

    fn write_zip(path: &Path, files: &[(&str, &[u8])], dirs: &[&str]) {
        let file = File::create(path).unwrap();
        let mut archive = zip::ZipWriter::new(file);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for dir in dirs {
            archive.add_directory(*dir, options).unwrap();
        }
        for (name, bytes) in files {
            archive.start_file(*name, options).unwrap();
            archive.write_all(bytes).unwrap();
        }
        archive.finish().unwrap();
    }

    fn write_tar(path: &Path, files: &[(&str, &[u8])]) {
        let file = File::create(path).unwrap();
        let mut archive = tar::Builder::new(file);
        for (name, bytes) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            archive.append_data(&mut header, *name, *bytes).unwrap();
        }
        archive.finish().unwrap();
    }

    fn write_tar_gzip(path: &Path, files: &[(&str, &[u8])]) {
        let file = File::create(path).unwrap();
        let encoder = GzEncoder::new(file, Compression::default());
        let mut archive = tar::Builder::new(encoder);
        for (name, bytes) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            archive.append_data(&mut header, *name, *bytes).unwrap();
        }
        archive.into_inner().unwrap().finish().unwrap();
    }

    fn write_tar_symlink(path: &Path, name: &str, target: &str) {
        let file = File::create(path).unwrap();
        let mut archive = tar::Builder::new(file);
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_size(0);
        header.set_mode(0o777);
        header.set_link_name(target).unwrap();
        header.set_cksum();
        archive
            .append_data(&mut header, name, std::io::empty())
            .unwrap();
        archive.finish().unwrap();
    }

    fn write_gzip(path: &Path, bytes: &[u8]) {
        let file = File::create(path).unwrap();
        let mut encoder = GzEncoder::new(file, Compression::default());
        encoder.write_all(bytes).unwrap();
        encoder.finish().unwrap();
    }

    fn write_zip_symlink(path: &Path, name: &str, target: &str) {
        let file = File::create(path).unwrap();
        let mut archive = zip::ZipWriter::new(file);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        archive.add_symlink(name, target, options).unwrap();
        archive.finish().unwrap();
    }

    fn mark_zip_entry_encrypted(path: &Path) {
        let mut bytes = fs::read(path).unwrap();
        let mut cursor = 0;
        while cursor + 4 <= bytes.len() {
            let signature = u32::from_le_bytes(bytes[cursor..cursor + 4].try_into().unwrap());
            match signature {
                0x0403_4b50 => {
                    bytes[cursor + 6] |= 0x01;
                    cursor += 30;
                }
                0x0201_4b50 => {
                    bytes[cursor + 8] |= 0x01;
                    cursor += 46;
                }
                _ => cursor += 1,
            }
        }
        fs::write(path, bytes).unwrap();
    }
}
