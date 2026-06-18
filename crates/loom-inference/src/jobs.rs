//! Inference download jobs.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use loom_types::{Code, DownloadJob, DownloadState, LoomError, ModelRef, Result, RuntimeKind};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::inventory::InstalledModelFile;

pub const JOB_STORE_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadJobPlan {
    pub model: ModelRef,
    pub runtime: RuntimeKind,
    pub files: Vec<String>,
    pub total_bytes: Option<u64>,
}

impl DownloadJobPlan {
    pub fn new(model: ModelRef, runtime: RuntimeKind, files: Vec<String>) -> Result<Self> {
        if files.is_empty() {
            return Err(LoomError::invalid(
                "inference download job requires at least one file",
            ));
        }
        Ok(Self {
            model,
            runtime,
            files,
            total_bytes: None,
        })
    }

    pub fn with_total_bytes(mut self, total_bytes: u64) -> Self {
        self.total_bytes = Some(total_bytes);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadArtifact {
    pub file: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadEvent {
    StateChanged {
        job_id: String,
        state: DownloadState,
    },
    FileStarted {
        job_id: String,
        file: String,
    },
    FileFinished {
        job_id: String,
        file: String,
        path: PathBuf,
        size_bytes: u64,
        digest: String,
    },
    Retry {
        job_id: String,
        file: String,
        attempt: u32,
        message: String,
    },
    Failed {
        job_id: String,
        message: String,
    },
}

pub trait DownloadExecutor {
    fn download(&mut self, model: &ModelRef, file: &str) -> Result<DownloadArtifact>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct DownloadJobs {
    pub version: u32,
    pub jobs: Vec<DownloadJob>,
}

impl Default for DownloadJobs {
    fn default() -> Self {
        Self {
            version: JOB_STORE_VERSION,
            jobs: Vec::new(),
        }
    }
}

impl DownloadJobs {
    pub fn upsert(&mut self, job: DownloadJob) {
        if let Some(existing) = self.jobs.iter_mut().find(|existing| existing.id == job.id) {
            *existing = job;
        } else {
            self.jobs.push(job);
            self.jobs.sort_by(|left, right| left.id.cmp(&right.id));
        }
    }

    pub fn find(&self, job_id: &str) -> Option<&DownloadJob> {
        self.jobs.iter().find(|job| job.id == job_id)
    }

    pub fn find_mut(&mut self, job_id: &str) -> Option<&mut DownloadJob> {
        self.jobs.iter_mut().find(|job| job.id == job_id)
    }
}

#[derive(Debug)]
pub struct CacheLock {
    path: PathBuf,
}

impl Drop for CacheLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[derive(Debug, Clone)]
pub struct DownloadJobManager {
    jobs: Arc<Mutex<DownloadJobs>>,
    cache_dir: PathBuf,
    max_retries: u32,
}

impl DownloadJobManager {
    pub fn new(cache_dir: impl Into<PathBuf>) -> Self {
        let cache_dir = cache_dir.into();
        Self {
            jobs: Arc::new(Mutex::new(DownloadJobs::default())),
            cache_dir,
            max_retries: 2,
        }
    }

    pub fn with_jobs(cache_dir: impl Into<PathBuf>, jobs: DownloadJobs) -> Self {
        Self {
            jobs: Arc::new(Mutex::new(jobs)),
            cache_dir: cache_dir.into(),
            max_retries: 2,
        }
    }

    pub fn with_max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }

    pub fn acquire_cache_lock(&self) -> Result<CacheLock> {
        let lock_path = metadata_root(&self.cache_dir).join("cache.lock");
        if let Some(parent) = lock_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_path)
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    LoomError::locked(format!(
                        "inference cache lock is already held: {}",
                        lock_path.display()
                    ))
                } else {
                    LoomError::from(error)
                }
            })?;
        writeln!(file, "pid={}", std::process::id())?;
        Ok(CacheLock { path: lock_path })
    }

    pub fn enqueue(&self, plan: DownloadJobPlan) -> Result<DownloadJob> {
        let mut jobs = self.jobs()?;
        let next = jobs.jobs.len() + 1;
        let job = DownloadJob {
            id: format!("download-{next:06}"),
            model: plan.model,
            runtime: plan.runtime,
            state: DownloadState::Queued,
            requested_files: plan.files,
            downloaded_bytes: 0,
            total_bytes: plan.total_bytes,
            failure: None,
        };
        jobs.upsert(job.clone());
        Ok(job)
    }

    pub fn enqueue_with_id(
        &self,
        id: impl Into<String>,
        plan: DownloadJobPlan,
    ) -> Result<DownloadJob> {
        let id = id.into();
        let mut jobs = self.jobs()?;
        if jobs.find(&id).is_some() {
            return Err(LoomError::new(
                Code::AlreadyExists,
                format!("inference download job {id} already exists"),
            ));
        }
        let job = DownloadJob {
            id,
            model: plan.model,
            runtime: plan.runtime,
            state: DownloadState::Queued,
            requested_files: plan.files,
            downloaded_bytes: 0,
            total_bytes: plan.total_bytes,
            failure: None,
        };
        jobs.upsert(job.clone());
        Ok(job)
    }

    pub fn list(&self) -> Result<Vec<DownloadJob>> {
        Ok(self.jobs()?.jobs.clone())
    }

    pub fn status(&self, job_id: &str) -> Result<DownloadJob> {
        self.jobs()?.find(job_id).cloned().ok_or_else(|| {
            LoomError::not_found(format!(
                "inference download job {job_id} is not active in this process"
            ))
        })
    }

    pub fn cancel(&self, job_id: &str) -> Result<DownloadJob> {
        let job = self.status(job_id)?;
        if job.state.is_terminal() {
            return Ok(job);
        }
        self.transition(job_id, DownloadState::Cancelled)
    }

    pub fn run<E, F>(&self, job_id: &str, executor: &mut E, mut on_event: F) -> Result<DownloadJob>
    where
        E: DownloadExecutor,
        F: FnMut(DownloadEvent),
    {
        let _lock = self.acquire_cache_lock()?;
        let job = self.transition(job_id, DownloadState::Resolving)?;
        emit_state(&mut on_event, &job);
        let job = self.transition(job_id, DownloadState::Downloading)?;
        emit_state(&mut on_event, &job);

        let mut downloaded_bytes = 0_u64;
        for file in &job.requested_files {
            if let Some(cancelled) = self.cancelled(job_id)? {
                emit_state(&mut on_event, &cancelled);
                return Ok(cancelled);
            }
            on_event(DownloadEvent::FileStarted {
                job_id: job.id.clone(),
                file: file.clone(),
            });
            let artifact = match self.download_with_retry(&job, executor, file, &mut on_event)? {
                Some(artifact) => artifact,
                None => return self.status(job_id),
            };
            let verified = match self.verify_artifact_or_fail(&job.id, &artifact, &mut on_event)? {
                Some(verified) => verified,
                None => return self.status(job_id),
            };
            let digest = required_digest(&verified)?;
            downloaded_bytes = downloaded_bytes.saturating_add(verified.size_bytes);
            self.set_progress(job_id, downloaded_bytes, None, None)?;
            on_event(DownloadEvent::FileFinished {
                job_id: job.id.clone(),
                file: artifact.file.clone(),
                path: artifact.path.clone(),
                size_bytes: verified.size_bytes,
                digest,
            });
        }

        let job = self.transition(job_id, DownloadState::Verifying)?;
        emit_state(&mut on_event, &job);
        let job = self.transition(job_id, DownloadState::Installed)?;
        emit_state(&mut on_event, &job);
        Ok(job)
    }

    #[cfg(all(feature = "native-hf", not(target_arch = "wasm32")))]
    pub async fn run_hf<F>(
        &self,
        job_id: &str,
        downloader: &crate::hf::HfDownloader,
        mut on_event: F,
    ) -> Result<DownloadJob>
    where
        F: FnMut(DownloadEvent),
    {
        let _lock = self.acquire_cache_lock()?;
        let job = self.transition(job_id, DownloadState::Resolving)?;
        emit_state(&mut on_event, &job);
        let job = self.transition(job_id, DownloadState::Downloading)?;
        emit_state(&mut on_event, &job);

        let mut downloaded_bytes = 0_u64;
        for file in &job.requested_files {
            if let Some(cancelled) = self.cancelled(job_id)? {
                emit_state(&mut on_event, &cancelled);
                return Ok(cancelled);
            }
            on_event(DownloadEvent::FileStarted {
                job_id: job.id.clone(),
                file: file.clone(),
            });
            let artifact = self
                .download_hf_with_retry(&job, downloader, file, &mut on_event)
                .await?;
            let Some(artifact) = artifact else {
                return self.status(job_id);
            };
            let verified = match self.verify_artifact_or_fail(&job.id, &artifact, &mut on_event)? {
                Some(verified) => verified,
                None => return self.status(job_id),
            };
            let digest = required_digest(&verified)?;
            downloaded_bytes = downloaded_bytes.saturating_add(verified.size_bytes);
            self.set_progress(job_id, downloaded_bytes, None, None)?;
            on_event(DownloadEvent::FileFinished {
                job_id: job.id.clone(),
                file: artifact.file.clone(),
                path: artifact.path.clone(),
                size_bytes: verified.size_bytes,
                digest,
            });
        }

        let job = self.transition(job_id, DownloadState::Verifying)?;
        emit_state(&mut on_event, &job);
        let job = self.transition(job_id, DownloadState::Installed)?;
        emit_state(&mut on_event, &job);
        Ok(job)
    }

    fn download_with_retry<E, F>(
        &self,
        job: &DownloadJob,
        executor: &mut E,
        file: &str,
        on_event: &mut F,
    ) -> Result<Option<DownloadArtifact>>
    where
        E: DownloadExecutor,
        F: FnMut(DownloadEvent),
    {
        let mut attempt = 0_u32;
        loop {
            if self.cancelled(&job.id)?.is_some() {
                return Ok(None);
            }
            match executor.download(&job.model, file) {
                Ok(artifact) => return Ok(Some(artifact)),
                Err(error) if attempt < self.max_retries => {
                    attempt += 1;
                    on_event(DownloadEvent::Retry {
                        job_id: job.id.clone(),
                        file: file.to_string(),
                        attempt,
                        message: error.message,
                    });
                }
                Err(error) => {
                    return self
                        .fail(
                            &job.id,
                            format!("failed to download {file}: {}", error.message),
                            on_event,
                        )
                        .map(|_| None);
                }
            }
        }
    }

    #[cfg(all(feature = "native-hf", not(target_arch = "wasm32")))]
    async fn download_hf_with_retry<F>(
        &self,
        job: &DownloadJob,
        downloader: &crate::hf::HfDownloader,
        file: &str,
        on_event: &mut F,
    ) -> Result<Option<DownloadArtifact>>
    where
        F: FnMut(DownloadEvent),
    {
        let mut attempt = 0_u32;
        loop {
            if self.cancelled(&job.id)?.is_some() {
                return Ok(None);
            }
            match downloader.download_file(&job.model, file).await {
                Ok(file) => {
                    return Ok(Some(DownloadArtifact {
                        file: file.file,
                        path: file.path,
                    }));
                }
                Err(error) if attempt < self.max_retries => {
                    attempt += 1;
                    on_event(DownloadEvent::Retry {
                        job_id: job.id.clone(),
                        file: file.to_string(),
                        attempt,
                        message: error.message,
                    });
                }
                Err(error) => {
                    return self
                        .fail(
                            &job.id,
                            format!("failed to download {file}: {}", error.message),
                            on_event,
                        )
                        .map(|_| None);
                }
            }
        }
    }

    fn transition(&self, job_id: &str, next: DownloadState) -> Result<DownloadJob> {
        let mut jobs = self.jobs()?;
        let job = jobs.find_mut(job_id).ok_or_else(|| {
            LoomError::not_found(format!(
                "inference download job {job_id} is not active in this process"
            ))
        })?;
        if job.state == next {
            return Ok(job.clone());
        }
        if !job.state.can_transition_to(next) {
            return Err(LoomError::invalid(format!(
                "cannot transition inference download job {job_id} from {} to {}",
                job.state.as_str(),
                next.as_str()
            )));
        }
        job.state = next;
        if next != DownloadState::Failed {
            job.failure = None;
        }
        let job = job.clone();
        Ok(job)
    }

    fn cancelled(&self, job_id: &str) -> Result<Option<DownloadJob>> {
        let job = self.status(job_id)?;
        if job.state == DownloadState::Cancelled {
            Ok(Some(job))
        } else {
            Ok(None)
        }
    }

    fn set_progress(
        &self,
        job_id: &str,
        downloaded_bytes: u64,
        total_bytes: Option<u64>,
        failure: Option<String>,
    ) -> Result<DownloadJob> {
        let mut jobs = self.jobs()?;
        let job = jobs.find_mut(job_id).ok_or_else(|| {
            LoomError::not_found(format!(
                "inference download job {job_id} is not active in this process"
            ))
        })?;
        job.downloaded_bytes = downloaded_bytes;
        if total_bytes.is_some() {
            job.total_bytes = total_bytes;
        }
        job.failure = failure;
        let job = job.clone();
        Ok(job)
    }

    fn fail<F>(
        &self,
        job_id: &str,
        message: impl Into<String>,
        mut on_event: F,
    ) -> Result<DownloadJob>
    where
        F: FnMut(DownloadEvent),
    {
        let message = message.into();
        let mut jobs = self.jobs()?;
        let job = jobs.find_mut(job_id).ok_or_else(|| {
            LoomError::not_found(format!(
                "inference download job {job_id} is not active in this process"
            ))
        })?;
        if !job.state.can_transition_to(DownloadState::Failed) {
            return Err(LoomError::invalid(format!(
                "cannot transition inference download job {job_id} from {} to failed",
                job.state.as_str()
            )));
        }
        job.state = DownloadState::Failed;
        job.failure = Some(message.clone());
        let job = job.clone();
        on_event(DownloadEvent::Failed {
            job_id: job.id.clone(),
            message,
        });
        Ok(job)
    }

    fn jobs(&self) -> Result<MutexGuard<'_, DownloadJobs>> {
        self.jobs
            .lock()
            .map_err(|_| LoomError::new(Code::Internal, "inference download state lock poisoned"))
    }

    fn verify_artifact(&self, artifact: &DownloadArtifact) -> Result<InstalledModelFile> {
        let metadata = fs::metadata(&artifact.path)?;
        if !metadata.is_file() {
            return Err(LoomError::invalid(format!(
                "downloaded model artifact {} is not a file",
                artifact.path.display()
            )));
        }
        let digest = file_digest(&artifact.path)?;
        Ok(InstalledModelFile {
            relative_path: relative_path(&artifact.path, &self.cache_dir),
            size_bytes: metadata.len(),
            digest: Some(format!("sha256:{digest}")),
        })
    }

    fn verify_artifact_or_fail<F>(
        &self,
        job_id: &str,
        artifact: &DownloadArtifact,
        on_event: &mut F,
    ) -> Result<Option<InstalledModelFile>>
    where
        F: FnMut(DownloadEvent),
    {
        match self.verify_artifact(artifact) {
            Ok(file) => Ok(Some(file)),
            Err(error) => self
                .fail(
                    job_id,
                    format!("failed to verify {}: {}", artifact.file, error.message),
                    on_event,
                )
                .map(|_| None),
        }
    }
}

fn emit_state<F>(on_event: &mut F, job: &DownloadJob)
where
    F: FnMut(DownloadEvent),
{
    on_event(DownloadEvent::StateChanged {
        job_id: job.id.clone(),
        state: job.state,
    });
}

fn required_digest(file: &InstalledModelFile) -> Result<String> {
    file.digest.clone().ok_or_else(|| {
        LoomError::new(
            Code::Internal,
            "download verification did not produce a digest",
        )
    })
}

fn file_digest(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn relative_path(path: &Path, cache_dir: &Path) -> String {
    path.strip_prefix(cache_dir)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn metadata_root(cache_dir: &Path) -> PathBuf {
    cache_dir.parent().unwrap_or(cache_dir).join("loom")
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_types::{InferenceModelKind, RevisionRef};
    use std::time::{SystemTime, UNIX_EPOCH};

    struct FileExecutor {
        root: PathBuf,
        fail_once: bool,
        calls: u32,
    }

    impl DownloadExecutor for FileExecutor {
        fn download(&mut self, _model: &ModelRef, file: &str) -> Result<DownloadArtifact> {
            self.calls += 1;
            if self.fail_once {
                self.fail_once = false;
                return Err(LoomError::new(Code::Io, "transient"));
            }
            let path = self.root.join(file);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(&path, format!("model-file:{file}")).unwrap();
            Ok(DownloadArtifact {
                file: file.to_string(),
                path,
            })
        }
    }

    struct DirectoryArtifactExecutor {
        root: PathBuf,
    }

    impl DownloadExecutor for DirectoryArtifactExecutor {
        fn download(&mut self, _model: &ModelRef, file: &str) -> Result<DownloadArtifact> {
            let path = self.root.join(file);
            fs::create_dir_all(&path).unwrap();
            Ok(DownloadArtifact {
                file: file.to_string(),
                path,
            })
        }
    }

    struct CancelAfterFirstFileExecutor {
        manager: DownloadJobManager,
        root: PathBuf,
        calls: u32,
    }

    impl DownloadExecutor for CancelAfterFirstFileExecutor {
        fn download(&mut self, _model: &ModelRef, file: &str) -> Result<DownloadArtifact> {
            self.calls += 1;
            let path = self.root.join(file);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(&path, format!("model-file:{file}")).unwrap();
            if self.calls == 1 {
                self.manager.cancel("job-a").unwrap();
            }
            Ok(DownloadArtifact {
                file: file.to_string(),
                path,
            })
        }
    }

    fn model() -> ModelRef {
        ModelRef::new(InferenceModelKind::TextEmbedding, "BAAI/bge-small-en-v1.5")
            .with_revision(RevisionRef::Commit("abc123".to_string()))
    }

    fn temp_root(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "loom-inference-jobs-{name}-{}-{nanos}",
            std::process::id()
        ))
    }

    fn manager(root: &Path) -> DownloadJobManager {
        DownloadJobManager::new(root.join("hub"))
    }

    fn plan() -> DownloadJobPlan {
        DownloadJobPlan::new(
            model(),
            RuntimeKind::CandleSafetensors,
            vec!["snapshots/abc123/model.safetensors".to_string()],
        )
        .unwrap()
    }

    fn two_file_plan() -> DownloadJobPlan {
        DownloadJobPlan::new(
            model(),
            RuntimeKind::CandleSafetensors,
            vec![
                "snapshots/abc123/config.json".to_string(),
                "snapshots/abc123/model.safetensors".to_string(),
            ],
        )
        .unwrap()
    }

    #[test]
    fn plan_requires_files() {
        assert_eq!(
            DownloadJobPlan::new(model(), RuntimeKind::CandleSafetensors, Vec::new())
                .unwrap_err()
                .code,
            Code::InvalidArgument
        );
    }

    #[test]
    fn enqueue_tracks_active_job_in_process_memory() {
        let root = temp_root("enqueue");
        let manager = manager(&root);
        let job = manager.enqueue_with_id("job-a", plan()).unwrap();

        assert_eq!(job.state, DownloadState::Queued);
        assert_eq!(manager.status("job-a").unwrap(), job);
        assert_eq!(
            manager.enqueue_with_id("job-a", plan()).unwrap_err().code,
            Code::AlreadyExists
        );
        assert_eq!(
            DownloadJobManager::new(root.join("hub"))
                .status("job-a")
                .unwrap_err()
                .code,
            Code::NotFound
        );
    }

    #[test]
    fn run_download_verifies_files_and_tracks_progress_in_memory() {
        let root = temp_root("install");
        let manager = manager(&root);
        manager.enqueue_with_id("job-a", plan()).unwrap();
        let mut executor = FileExecutor {
            root: root.join("hub"),
            fail_once: false,
            calls: 0,
        };
        let mut events = Vec::new();

        let job = manager
            .run("job-a", &mut executor, |event| events.push(event))
            .unwrap();

        assert_eq!(job.state, DownloadState::Installed);
        assert_eq!(manager.status("job-a").unwrap().downloaded_bytes, 45);
        assert!(events.iter().any(|event| matches!(
            event,
            DownloadEvent::StateChanged {
                state: DownloadState::Installed,
                ..
            }
        )));
        assert!(!root.join("loom").join("installed-models.json").exists());
    }

    #[test]
    fn run_retries_transient_download_failure() {
        let root = temp_root("retry");
        let manager = manager(&root).with_max_retries(1);
        manager.enqueue_with_id("job-a", plan()).unwrap();
        let mut executor = FileExecutor {
            root: root.join("hub"),
            fail_once: true,
            calls: 0,
        };
        let mut events = Vec::new();

        let job = manager
            .run("job-a", &mut executor, |event| events.push(event))
            .unwrap();

        assert_eq!(job.state, DownloadState::Installed);
        assert_eq!(executor.calls, 2);
        assert!(events.iter().any(|event| matches!(
            event,
            DownloadEvent::Retry {
                attempt: 1,
                message,
                ..
            } if message == "transient"
        )));
    }

    #[test]
    fn run_tracks_failure_after_retry_budget_in_memory() {
        let root = temp_root("failure");
        let manager = manager(&root).with_max_retries(0);
        manager.enqueue_with_id("job-a", plan()).unwrap();
        let mut executor = FileExecutor {
            root: root.join("hub"),
            fail_once: true,
            calls: 0,
        };

        let job = manager.run("job-a", &mut executor, |_| {}).unwrap();

        assert_eq!(job.state, DownloadState::Failed);
        assert_eq!(
            manager.status("job-a").unwrap().failure.as_deref(),
            Some("failed to download snapshots/abc123/model.safetensors: transient")
        );
    }

    #[test]
    fn run_tracks_failure_after_verification_error_in_memory() {
        let root = temp_root("verify-failure");
        let manager = manager(&root);
        manager.enqueue_with_id("job-a", plan()).unwrap();
        let mut executor = DirectoryArtifactExecutor {
            root: root.join("hub"),
        };
        let mut events = Vec::new();

        let job = manager
            .run("job-a", &mut executor, |event| events.push(event))
            .unwrap();

        assert_eq!(job.state, DownloadState::Failed);
        assert!(job.failure.unwrap().contains("failed to verify"));
        assert!(!root.join("loom").join("installed-models.json").exists());
        assert!(events.iter().any(|event| matches!(
            event,
            DownloadEvent::Failed {
                message,
                ..
            } if message.contains("not a file")
        )));
    }

    #[test]
    fn cancel_marks_queued_job_terminal() {
        let root = temp_root("cancel");
        let manager = manager(&root);
        manager.enqueue_with_id("job-a", plan()).unwrap();

        let job = manager.cancel("job-a").unwrap();

        assert_eq!(job.state, DownloadState::Cancelled);
        assert!(manager.status("job-a").unwrap().state.is_terminal());
    }

    #[test]
    fn run_observes_cancel_between_files() {
        let root = temp_root("cancel-during-run");
        let manager = manager(&root);
        manager.enqueue_with_id("job-a", two_file_plan()).unwrap();
        let mut executor = CancelAfterFirstFileExecutor {
            manager: manager.clone(),
            root: root.join("hub"),
            calls: 0,
        };
        let mut events = Vec::new();

        let job = manager
            .run("job-a", &mut executor, |event| events.push(event))
            .unwrap();

        assert_eq!(job.state, DownloadState::Cancelled);
        assert_eq!(executor.calls, 1);
        assert!(!root.join("loom").join("installed-models.json").exists());
        assert!(events.iter().any(|event| matches!(
            event,
            DownloadEvent::StateChanged {
                state: DownloadState::Cancelled,
                ..
            }
        )));
    }

    #[test]
    fn cache_lock_rejects_concurrent_holder_and_releases_on_drop() {
        let root = temp_root("lock");
        let manager = manager(&root);
        let first = manager.acquire_cache_lock().unwrap();

        assert_eq!(manager.acquire_cache_lock().unwrap_err().code, Code::Locked);
        drop(first);
        assert!(manager.acquire_cache_lock().is_ok());
    }
}
