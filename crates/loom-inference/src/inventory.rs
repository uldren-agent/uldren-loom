//! Installed-model records and live Hugging Face cache discovery.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use loom_types::{ModelRef, Result, RevisionRef, RuntimeKind};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::catalog::{CuratedModelSpec, curated_models};

pub const INVENTORY_VERSION: u32 = 1;
const SMALL_FILE_DIGEST_LIMIT_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct InstalledModelFile {
    pub relative_path: String,
    pub size_bytes: u64,
    pub digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct InstalledModelRecord {
    pub model: ModelRef,
    pub runtime: RuntimeKind,
    pub files: Vec<InstalledModelFile>,
    pub active_provider_refs: Vec<String>,
}

impl InstalledModelRecord {
    pub fn same_install_key(&self, other: &Self) -> bool {
        self.model == other.model && self.runtime == other.runtime
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct InstallInventory {
    pub version: u32,
    pub models: Vec<InstalledModelRecord>,
}

impl Default for InstallInventory {
    fn default() -> Self {
        Self {
            version: INVENTORY_VERSION,
            models: Vec::new(),
        }
    }
}

impl InstallInventory {
    pub fn upsert(&mut self, record: InstalledModelRecord) {
        if let Some(existing) = self
            .models
            .iter_mut()
            .find(|existing| existing.same_install_key(&record))
        {
            *existing = record;
        } else {
            self.models.push(record);
            self.models.sort_by(|left, right| {
                (
                    left.model.kind,
                    &left.model.repo_id,
                    &left.model.revision,
                    left.runtime,
                )
                    .cmp(&(
                        right.model.kind,
                        &right.model.repo_id,
                        &right.model.revision,
                        right.runtime,
                    ))
            });
        }
    }

    pub fn remove(
        &mut self,
        model: &ModelRef,
        runtime: RuntimeKind,
    ) -> Option<InstalledModelRecord> {
        let index = self
            .models
            .iter()
            .position(|record| record.model == *model && record.runtime == runtime)?;
        Some(self.models.remove(index))
    }

    pub fn find(&self, model: &ModelRef, runtime: RuntimeKind) -> Option<&InstalledModelRecord> {
        self.models
            .iter()
            .find(|record| record.model == *model && record.runtime == runtime)
    }
}

pub fn discover_installed_models(cache_dir: &Path) -> Result<InstallInventory> {
    let mut inventory = InstallInventory::default();
    if !cache_dir.is_dir() {
        return Ok(inventory);
    }
    for spec in curated_models() {
        if let Some(record) = discover_curated_model(cache_dir, *spec)? {
            inventory.upsert(record);
        }
    }
    Ok(inventory)
}

pub fn discover_installed_model(
    cache_dir: &Path,
    model: &ModelRef,
    runtime: RuntimeKind,
) -> Result<Option<InstalledModelRecord>> {
    Ok(discover_installed_models(cache_dir)?.remove(model, runtime))
}

fn discover_curated_model(
    cache_dir: &Path,
    spec: CuratedModelSpec,
) -> Result<Option<InstalledModelRecord>> {
    let repo_dir = repo_cache_dir(cache_dir, spec.repo_id);
    let Some(snapshot_dir) =
        snapshot_dir(&repo_dir, &RevisionRef::Branch(spec.revision.to_string()))
    else {
        return Ok(None);
    };
    let mut files = Vec::with_capacity(spec.files.len());
    for file in spec.files {
        let Some(path) = safe_join(&snapshot_dir, file) else {
            return Ok(None);
        };
        let metadata = match fs::metadata(&path) {
            Ok(metadata) if metadata.is_file() => metadata,
            Ok(_) => return Ok(None),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        files.push(InstalledModelFile {
            relative_path: relative_cache_path(cache_dir, &path),
            size_bytes: metadata.len(),
            digest: cache_file_digest(&path, metadata.len())?,
        });
    }
    Ok(Some(InstalledModelRecord {
        model: spec.model_ref(),
        runtime: spec.runtime,
        files,
        active_provider_refs: Vec::new(),
    }))
}

fn repo_cache_dir(cache_dir: &Path, repo_id: &str) -> PathBuf {
    cache_dir.join(format!("models--{}", repo_id.replace('/', "--")))
}

fn snapshot_dir(repo_dir: &Path, revision: &RevisionRef) -> Option<PathBuf> {
    match revision {
        RevisionRef::Commit(commit) => {
            let path = repo_dir.join("snapshots").join(commit);
            path.is_dir().then_some(path)
        }
        RevisionRef::Branch(name) | RevisionRef::Tag(name) => {
            let ref_path = safe_join(&repo_dir.join("refs"), name)?;
            if let Ok(commit) = fs::read_to_string(&ref_path) {
                let commit = commit.trim();
                let path = repo_dir.join("snapshots").join(commit);
                if path.is_dir() {
                    return Some(path);
                }
            }
            let path = safe_join(&repo_dir.join("snapshots"), name)?;
            path.is_dir().then_some(path)
        }
    }
}

fn safe_join(root: &Path, relative: &str) -> Option<PathBuf> {
    let relative = Path::new(relative);
    if relative.components().all(|component| {
        matches!(
            component,
            std::path::Component::Normal(_) | std::path::Component::CurDir
        )
    }) {
        Some(root.join(relative))
    } else {
        None
    }
}

fn relative_cache_path(cache_dir: &Path, path: &Path) -> String {
    path.strip_prefix(cache_dir)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn cache_file_digest(path: &Path, size_bytes: u64) -> Result<Option<String>> {
    if let Some(digest) = symlink_sha256_digest(path) {
        return Ok(Some(digest));
    }
    if size_bytes <= SMALL_FILE_DIGEST_LIMIT_BYTES {
        return sha256_digest(path).map(|digest| Some(format!("sha256:{digest}")));
    }
    Ok(None)
}

fn symlink_sha256_digest(path: &Path) -> Option<String> {
    let target = fs::read_link(path).ok()?;
    let blob = target.file_name()?.to_str()?;
    (blob.len() == 64 && blob.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| format!("sha256:{blob}"))
}

fn sha256_digest(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_types::{InferenceModelKind, RevisionRef};

    fn record(repo_id: &str, runtime: RuntimeKind) -> InstalledModelRecord {
        InstalledModelRecord {
            model: ModelRef::new(InferenceModelKind::TextEmbedding, repo_id)
                .with_revision(RevisionRef::Commit("abc123".to_string())),
            runtime,
            files: vec![InstalledModelFile {
                relative_path: "snapshots/abc123/model.safetensors".to_string(),
                size_bytes: 12,
                digest: Some("sha256:abc".to_string()),
            }],
            active_provider_refs: vec!["vector:emb".to_string()],
        }
    }

    #[test]
    fn upsert_replaces_matching_model_and_runtime() {
        let mut inventory = InstallInventory::default();
        let mut first = record("BAAI/bge-small-en-v1.5", RuntimeKind::CandleSafetensors);
        inventory.upsert(first.clone());
        first.files[0].size_bytes = 24;
        inventory.upsert(first);

        assert_eq!(inventory.models.len(), 1);
        assert_eq!(inventory.models[0].files[0].size_bytes, 24);
    }

    #[test]
    fn upsert_keeps_distinct_runtimes() {
        let mut inventory = InstallInventory::default();
        inventory.upsert(record(
            "BAAI/bge-small-en-v1.5",
            RuntimeKind::CandleSafetensors,
        ));
        inventory.upsert(record("BAAI/bge-small-en-v1.5", RuntimeKind::CandleGguf));

        assert_eq!(inventory.models.len(), 2);
    }

    #[test]
    fn discovers_curated_model_from_hf_cache_snapshot() {
        let root = std::env::temp_dir().join(format!(
            "loom-inference-cache-discovery-{}",
            std::process::id()
        ));
        let cache_dir = root.join("hub");
        let repo_dir = cache_dir.join("models--sentence-transformers--all-MiniLM-L6-v2");
        let snapshot = repo_dir.join("snapshots").join("abc123");
        fs::create_dir_all(&snapshot).unwrap();
        fs::create_dir_all(repo_dir.join("refs")).unwrap();
        fs::write(repo_dir.join("refs").join("main"), "abc123\n").unwrap();
        for file in curated_models()[0].files {
            fs::write(snapshot.join(file), format!("file:{file}")).unwrap();
        }

        let inventory = discover_installed_models(&cache_dir).unwrap();

        let record = inventory
            .find(
                &ModelRef::new(
                    InferenceModelKind::TextEmbedding,
                    "sentence-transformers/all-MiniLM-L6-v2",
                ),
                RuntimeKind::CandleSafetensors,
            )
            .unwrap();
        assert_eq!(record.files.len(), curated_models()[0].files.len());
        assert!(
            record.files[0]
                .relative_path
                .starts_with("models--sentence-transformers--all-MiniLM-L6-v2/snapshots/abc123/")
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn discovery_requires_all_curated_files() {
        let root = std::env::temp_dir().join(format!(
            "loom-inference-cache-discovery-partial-{}",
            std::process::id()
        ));
        let cache_dir = root.join("hub");
        let repo_dir = cache_dir.join("models--BAAI--bge-small-en-v1.5");
        let snapshot = repo_dir.join("snapshots").join("abc123");
        fs::create_dir_all(&snapshot).unwrap();
        fs::create_dir_all(repo_dir.join("refs")).unwrap();
        fs::write(repo_dir.join("refs").join("main"), "abc123\n").unwrap();
        fs::write(snapshot.join("config.json"), "{}").unwrap();

        let inventory = discover_installed_models(&cache_dir).unwrap();

        assert!(inventory.models.is_empty());
        let _ = fs::remove_dir_all(root);
    }
}
