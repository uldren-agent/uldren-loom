//! Native Hugging Face model acquisition.

use std::path::{Path, PathBuf};

use hf_hub::{Repo, RepoType, api::tokio::ApiBuilder};
use loom_types::{Code, LoomError, ModelRef, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HfDownloadRequest {
    pub model: ModelRef,
    pub files: Vec<String>,
    pub token: Option<String>,
}

impl HfDownloadRequest {
    pub fn new(model: ModelRef, files: Vec<String>) -> Result<Self> {
        if files.is_empty() {
            return Err(LoomError::invalid(
                "hugging face download requires at least one file",
            ));
        }
        Ok(Self {
            model,
            files,
            token: None,
        })
    }

    pub fn with_token(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HfDownloadedFile {
    pub model: ModelRef,
    pub file: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HfDownloadEvent {
    Started { file: String },
    Finished { file: String, path: PathBuf },
    Failed { file: String, message: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HfCachePaths {
    pub hf_home: Option<PathBuf>,
    pub cache_dir: PathBuf,
    pub token_file: PathBuf,
}

impl HfCachePaths {
    pub fn from_env() -> Result<Self> {
        let hf_home = std::env::var_os("HF_HOME").map(PathBuf::from);
        let home = home_dir()?;
        Ok(Self::resolve(hf_home, &home))
    }

    pub fn resolve(hf_home: Option<PathBuf>, home: &Path) -> Self {
        let cache_dir = match &hf_home {
            Some(path) => path.join("hub"),
            None => home.join(".cache").join("huggingface").join("hub"),
        };
        let token_file = cache_dir
            .parent()
            .map(|path| path.join("token"))
            .unwrap_or_else(|| PathBuf::from("token"));
        Self {
            hf_home,
            cache_dir,
            token_file,
        }
    }
}

#[derive(Clone, Debug)]
pub struct HfDownloader {
    api: hf_hub::api::tokio::Api,
}

impl HfDownloader {
    pub fn from_env(token: Option<String>) -> Result<Self> {
        let api = ApiBuilder::from_env()
            .with_progress(false)
            .with_token(token)
            .with_user_agent("uldren-loom", env!("CARGO_PKG_VERSION"))
            .build()
            .map_err(map_hf_error)?;
        Ok(Self { api })
    }

    pub async fn download_request(
        &self,
        request: &HfDownloadRequest,
    ) -> Result<Vec<HfDownloadedFile>> {
        self.download_request_with_events(request, |_| {}).await
    }

    pub async fn download_request_with_events<F>(
        &self,
        request: &HfDownloadRequest,
        mut on_event: F,
    ) -> Result<Vec<HfDownloadedFile>>
    where
        F: FnMut(HfDownloadEvent),
    {
        let repo = self.repo(&request.model);
        let mut downloaded = Vec::with_capacity(request.files.len());
        for file in &request.files {
            on_event(HfDownloadEvent::Started { file: file.clone() });
            let path = match repo.get(file).await {
                Ok(path) => path,
                Err(error) => {
                    let error = map_hf_error(error);
                    on_event(HfDownloadEvent::Failed {
                        file: file.clone(),
                        message: error.message.clone(),
                    });
                    return Err(error);
                }
            };
            on_event(HfDownloadEvent::Finished {
                file: file.clone(),
                path: path.clone(),
            });
            downloaded.push(HfDownloadedFile {
                model: request.model.clone(),
                file: file.clone(),
                path,
            });
        }
        Ok(downloaded)
    }

    pub async fn download_file(&self, model: &ModelRef, file: &str) -> Result<HfDownloadedFile> {
        let request = HfDownloadRequest::new(model.clone(), vec![file.to_string()])?;
        let mut files = self.download_request(&request).await?;
        files
            .pop()
            .ok_or_else(|| LoomError::new(Code::Internal, "download completed without a file"))
    }

    fn repo(&self, model: &ModelRef) -> hf_hub::api::tokio::ApiRepo {
        self.api.repo(Repo::with_revision(
            model.repo_id.clone(),
            RepoType::Model,
            model.revision.value().to_string(),
        ))
    }
}

fn home_dir() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| LoomError::invalid("home directory is unavailable"))
}

fn map_hf_error(error: hf_hub::api::tokio::ApiError) -> LoomError {
    LoomError::new(Code::Io, format!("hugging face request failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_types::{InferenceModelKind, RevisionRef};

    #[test]
    fn request_requires_files() {
        let model = ModelRef::new(
            InferenceModelKind::TextEmbedding,
            "sentence-transformers/all-MiniLM-L6-v2",
        );
        assert_eq!(
            HfDownloadRequest::new(model, Vec::new()).unwrap_err().code,
            Code::InvalidArgument
        );
    }

    #[test]
    fn request_keeps_revision_and_token() {
        let model = ModelRef::new(InferenceModelKind::Llm, "Qwen/Qwen2.5-0.5B-Instruct-GGUF")
            .with_revision(RevisionRef::Commit("0123456789abcdef".to_string()));
        let request = HfDownloadRequest::new(model, vec!["model.gguf".to_string()])
            .unwrap()
            .with_token("secret");

        assert_eq!(request.files, vec!["model.gguf"]);
        assert_eq!(request.model.revision.value(), "0123456789abcdef");
        assert_eq!(request.token.as_deref(), Some("secret"));
    }

    #[test]
    fn cache_paths_use_hf_home_when_present() {
        let paths =
            HfCachePaths::resolve(Some(PathBuf::from("/models/hf")), Path::new("/home/alice"));
        assert_eq!(paths.hf_home.as_deref(), Some(Path::new("/models/hf")));
        assert_eq!(paths.cache_dir, PathBuf::from("/models/hf/hub"));
        assert_eq!(paths.token_file, PathBuf::from("/models/hf/token"));
    }

    #[test]
    fn cache_paths_use_standard_home_default() {
        let paths = HfCachePaths::resolve(None, Path::new("/home/alice"));
        assert_eq!(
            paths.cache_dir,
            PathBuf::from("/home/alice/.cache/huggingface/hub")
        );
        assert_eq!(
            paths.token_file,
            PathBuf::from("/home/alice/.cache/huggingface/token")
        );
    }
}
