//! Browser-compatible model acquisition without native Hugging Face clients.

use std::collections::BTreeMap;

use loom_types::{Code, LoomError, ModelRef, Result, RuntimeKind};
use sha2::{Digest, Sha256};

use crate::inventory::{InstalledModelFile, InstalledModelRecord};

pub const DEFAULT_BROWSER_MODEL_FILE_LIMIT_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserDownloadRequest {
    pub model: ModelRef,
    pub runtime: RuntimeKind,
    pub files: Vec<String>,
    pub max_file_bytes: u64,
}

impl BrowserDownloadRequest {
    pub fn new(model: ModelRef, runtime: RuntimeKind, files: Vec<String>) -> Result<Self> {
        if files.is_empty() {
            return Err(LoomError::invalid(
                "browser inference download requires at least one file",
            ));
        }
        Ok(Self {
            model,
            runtime,
            files,
            max_file_bytes: DEFAULT_BROWSER_MODEL_FILE_LIMIT_BYTES,
        })
    }

    pub fn with_max_file_bytes(mut self, max_file_bytes: u64) -> Self {
        self.max_file_bytes = max_file_bytes;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserStoredFile {
    pub file: String,
    pub storage_key: String,
    pub size_bytes: u64,
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserAcquiredModel {
    pub record: InstalledModelRecord,
    pub files: Vec<BrowserStoredFile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserDownloadEvent {
    Started { file: String },
    Stored { file: String, storage_key: String },
    Failed { file: String, message: String },
}

pub trait BrowserModelFetch {
    fn fetch_model_file(&mut self, model: &ModelRef, file: &str) -> Result<Vec<u8>>;
}

pub trait BrowserModelStorage {
    fn put_model_file(&mut self, storage_key: &str, bytes: &[u8]) -> Result<()>;

    fn get_model_file(&self, storage_key: &str) -> Result<Option<Vec<u8>>>;

    fn remove_model_file(&mut self, storage_key: &str) -> Result<()>;
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InMemoryBrowserModelStorage {
    files: BTreeMap<String, Vec<u8>>,
}

impl InMemoryBrowserModelStorage {
    pub fn keys(&self) -> Vec<&str> {
        self.files.keys().map(String::as_str).collect()
    }
}

impl BrowserModelStorage for InMemoryBrowserModelStorage {
    fn put_model_file(&mut self, storage_key: &str, bytes: &[u8]) -> Result<()> {
        self.files.insert(storage_key.to_string(), bytes.to_vec());
        Ok(())
    }

    fn get_model_file(&self, storage_key: &str) -> Result<Option<Vec<u8>>> {
        Ok(self.files.get(storage_key).cloned())
    }

    fn remove_model_file(&mut self, storage_key: &str) -> Result<()> {
        self.files.remove(storage_key);
        Ok(())
    }
}

pub struct BrowserModelAcquirer<S> {
    storage: S,
}

impl<S> BrowserModelAcquirer<S>
where
    S: BrowserModelStorage,
{
    pub fn new(storage: S) -> Self {
        Self { storage }
    }

    pub fn storage(&self) -> &S {
        &self.storage
    }

    pub fn storage_mut(&mut self) -> &mut S {
        &mut self.storage
    }

    pub fn acquire<F, E>(
        &mut self,
        request: &BrowserDownloadRequest,
        fetch: &mut F,
        mut on_event: E,
    ) -> Result<BrowserAcquiredModel>
    where
        F: BrowserModelFetch,
        E: FnMut(BrowserDownloadEvent),
    {
        let mut stored_files = Vec::with_capacity(request.files.len());
        let mut installed_files = Vec::with_capacity(request.files.len());
        for file in &request.files {
            on_event(BrowserDownloadEvent::Started { file: file.clone() });
            let bytes = match fetch.fetch_model_file(&request.model, file) {
                Ok(bytes) => bytes,
                Err(error) => {
                    on_event(BrowserDownloadEvent::Failed {
                        file: file.clone(),
                        message: error.message.clone(),
                    });
                    return Err(error);
                }
            };
            let size_bytes = bytes.len() as u64;
            if size_bytes > request.max_file_bytes {
                let error = LoomError::new(
                    Code::ResourceExhausted,
                    format!(
                        "browser inference file {file:?} is {size_bytes} bytes, limit is {}",
                        request.max_file_bytes
                    ),
                );
                on_event(BrowserDownloadEvent::Failed {
                    file: file.clone(),
                    message: error.message.clone(),
                });
                return Err(error);
            }
            let digest = sha256_digest(&bytes);
            let storage_key = browser_storage_key(&request.model, file)?;
            self.storage.put_model_file(&storage_key, &bytes)?;
            on_event(BrowserDownloadEvent::Stored {
                file: file.clone(),
                storage_key: storage_key.clone(),
            });
            stored_files.push(BrowserStoredFile {
                file: file.clone(),
                storage_key: storage_key.clone(),
                size_bytes,
                digest: digest.clone(),
            });
            installed_files.push(InstalledModelFile {
                relative_path: storage_key,
                size_bytes,
                digest: Some(digest),
            });
        }
        Ok(BrowserAcquiredModel {
            record: InstalledModelRecord {
                model: request.model.clone(),
                runtime: request.runtime,
                files: installed_files,
                active_provider_refs: Vec::new(),
            },
            files: stored_files,
        })
    }
}

pub fn browser_storage_key(model: &ModelRef, file: &str) -> Result<String> {
    if file.is_empty()
        || file
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(LoomError::invalid(format!(
            "invalid browser inference file path {file:?}"
        )));
    }
    let model_hash = model_hash(model)?;
    Ok(format!("models/{model_hash}/{file}"))
}

fn model_hash(model: &ModelRef) -> Result<String> {
    let bytes = model.canonical_bytes()?;
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    Ok(hex::encode(hasher.finalize()))
}

fn sha256_digest(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("sha256:{}", hex::encode(hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use loom_types::{InferenceModelKind, RevisionRef};

    use super::*;

    #[derive(Default)]
    struct FixtureFetch {
        files: BTreeMap<String, Vec<u8>>,
    }

    impl BrowserModelFetch for FixtureFetch {
        fn fetch_model_file(&mut self, _model: &ModelRef, file: &str) -> Result<Vec<u8>> {
            self.files
                .get(file)
                .cloned()
                .ok_or_else(|| LoomError::not_found(format!("missing fixture file {file}")))
        }
    }

    fn model() -> ModelRef {
        ModelRef::new(
            InferenceModelKind::TextEmbedding,
            "sentence-transformers/all-MiniLM-L6-v2",
        )
        .with_revision(RevisionRef::Branch("main".to_string()))
    }

    #[test]
    fn browser_acquisition_stores_files_and_returns_inventory_record() {
        let mut fetch = FixtureFetch::default();
        fetch
            .files
            .insert("config.json".to_string(), b"{}".to_vec());
        fetch
            .files
            .insert("model.safetensors".to_string(), b"weights".to_vec());
        let request = BrowserDownloadRequest::new(
            model(),
            RuntimeKind::CandleSafetensors,
            vec!["config.json".to_string(), "model.safetensors".to_string()],
        )
        .unwrap();
        let mut events = Vec::new();
        let mut acquirer = BrowserModelAcquirer::new(InMemoryBrowserModelStorage::default());

        let acquired = acquirer
            .acquire(&request, &mut fetch, |event| events.push(event))
            .unwrap();

        assert_eq!(acquired.record.model.repo_id, request.model.repo_id);
        assert_eq!(acquired.record.files.len(), 2);
        assert_eq!(acquired.files.len(), 2);
        assert!(
            acquired.record.files[0]
                .relative_path
                .starts_with("models/")
        );
        assert!(
            acquired.record.files[0]
                .digest
                .as_deref()
                .unwrap()
                .starts_with("sha256:")
        );
        assert_eq!(acquirer.storage().keys().len(), 2);
        assert!(matches!(
            events.first(),
            Some(BrowserDownloadEvent::Started { file }) if file == "config.json"
        ));
        assert!(events.iter().any(|event| matches!(
            event,
            BrowserDownloadEvent::Stored { file, .. } if file == "model.safetensors"
        )));
    }

    #[test]
    fn browser_acquisition_enforces_file_limit() {
        let mut fetch = FixtureFetch::default();
        fetch
            .files
            .insert("model.safetensors".to_string(), vec![7; 5]);
        let request = BrowserDownloadRequest::new(
            model(),
            RuntimeKind::CandleSafetensors,
            vec!["model.safetensors".to_string()],
        )
        .unwrap()
        .with_max_file_bytes(4);
        let mut acquirer = BrowserModelAcquirer::new(InMemoryBrowserModelStorage::default());

        let err = acquirer.acquire(&request, &mut fetch, |_| {}).unwrap_err();

        assert_eq!(err.code, Code::ResourceExhausted);
        assert!(acquirer.storage().keys().is_empty());
    }

    #[test]
    fn browser_storage_key_rejects_parent_paths() {
        let err = browser_storage_key(&model(), "../model.safetensors").unwrap_err();

        assert_eq!(err.code, Code::InvalidArgument);
    }
}
