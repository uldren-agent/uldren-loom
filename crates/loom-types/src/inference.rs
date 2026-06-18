//! Shared inference model, download, and hardware-report contracts.

use std::collections::BTreeMap;

use crate::error::{LoomError, Result};
use loom_codec::Value as CborValue;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InferenceModelKind {
    Llm,
    TextEmbedding,
}

impl InferenceModelKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            InferenceModelKind::Llm => "llm",
            InferenceModelKind::TextEmbedding => "text-embedding",
        }
    }

    pub const fn stable_tag(self) -> u8 {
        match self {
            InferenceModelKind::Llm => 1,
            InferenceModelKind::TextEmbedding => 2,
        }
    }

    pub fn from_stable_tag(tag: u8) -> Result<Self> {
        match tag {
            1 => Ok(InferenceModelKind::Llm),
            2 => Ok(InferenceModelKind::TextEmbedding),
            _ => Err(LoomError::invalid(format!(
                "unknown inference model kind tag {tag}"
            ))),
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "llm" => Ok(InferenceModelKind::Llm),
            "text-embedding" => Ok(InferenceModelKind::TextEmbedding),
            _ => Err(LoomError::invalid(format!(
                "unknown inference model kind {value:?}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RevisionRef {
    Branch(String),
    Tag(String),
    Commit(String),
}

impl RevisionRef {
    pub fn main() -> Self {
        Self::Branch("main".to_string())
    }

    pub fn value(&self) -> &str {
        match self {
            RevisionRef::Branch(value) | RevisionRef::Tag(value) | RevisionRef::Commit(value) => {
                value
            }
        }
    }

    pub const fn stable_tag(&self) -> u8 {
        match self {
            RevisionRef::Branch(_) => 1,
            RevisionRef::Tag(_) => 2,
            RevisionRef::Commit(_) => 3,
        }
    }

    pub fn to_cbor_value(&self) -> CborValue {
        CborValue::Array(vec![
            CborValue::Uint(u64::from(self.stable_tag())),
            CborValue::Text(self.value().to_string()),
        ])
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ModelRef {
    pub kind: InferenceModelKind,
    pub repo_id: String,
    pub revision: RevisionRef,
    pub file: Option<String>,
}

impl ModelRef {
    pub fn new(kind: InferenceModelKind, repo_id: impl Into<String>) -> Self {
        Self {
            kind,
            repo_id: repo_id.into(),
            revision: RevisionRef::main(),
            file: None,
        }
    }

    pub fn with_revision(mut self, revision: RevisionRef) -> Self {
        self.revision = revision;
        self
    }

    pub fn with_file(mut self, file: impl Into<String>) -> Self {
        self.file = Some(file.into());
        self
    }

    pub fn is_llm(&self) -> bool {
        self.kind == InferenceModelKind::Llm
    }

    pub fn is_text_embedding(&self) -> bool {
        self.kind == InferenceModelKind::TextEmbedding
    }

    pub fn to_cbor_value(&self) -> CborValue {
        CborValue::Array(vec![
            CborValue::Uint(1),
            CborValue::Uint(u64::from(self.kind.stable_tag())),
            CborValue::Text(self.repo_id.clone()),
            self.revision.to_cbor_value(),
            optional_text(self.file.as_deref()),
        ])
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_cbor_value())
            .map_err(|e| LoomError::corrupt(format!("failed to encode inference model ref: {e}")))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeKind {
    CandleSafetensors,
    CandleGguf,
    Mlx,
    Ollama,
    LlamaCpp,
    OpenAiCompatibleHttp,
    HostedApi,
    CoreMl,
}

impl RuntimeKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            RuntimeKind::CandleSafetensors => "candle-safetensors",
            RuntimeKind::CandleGguf => "candle-gguf",
            RuntimeKind::Mlx => "mlx",
            RuntimeKind::Ollama => "ollama",
            RuntimeKind::LlamaCpp => "llama-cpp",
            RuntimeKind::OpenAiCompatibleHttp => "openai-compatible-http",
            RuntimeKind::HostedApi => "hosted-api",
            RuntimeKind::CoreMl => "core-ml",
        }
    }

    pub const fn stable_tag(self) -> u8 {
        match self {
            RuntimeKind::CandleSafetensors => 1,
            RuntimeKind::CandleGguf => 2,
            RuntimeKind::Mlx => 3,
            RuntimeKind::Ollama => 4,
            RuntimeKind::LlamaCpp => 5,
            RuntimeKind::OpenAiCompatibleHttp => 6,
            RuntimeKind::HostedApi => 7,
            RuntimeKind::CoreMl => 8,
        }
    }

    pub fn from_stable_tag(tag: u8) -> Result<Self> {
        match tag {
            1 => Ok(RuntimeKind::CandleSafetensors),
            2 => Ok(RuntimeKind::CandleGguf),
            3 => Ok(RuntimeKind::Mlx),
            4 => Ok(RuntimeKind::Ollama),
            5 => Ok(RuntimeKind::LlamaCpp),
            6 => Ok(RuntimeKind::OpenAiCompatibleHttp),
            7 => Ok(RuntimeKind::HostedApi),
            8 => Ok(RuntimeKind::CoreMl),
            _ => Err(LoomError::invalid(format!(
                "unknown runtime kind tag {tag}"
            ))),
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "candle-safetensors" => Ok(RuntimeKind::CandleSafetensors),
            "candle-gguf" => Ok(RuntimeKind::CandleGguf),
            "mlx" => Ok(RuntimeKind::Mlx),
            "ollama" => Ok(RuntimeKind::Ollama),
            "llama-cpp" => Ok(RuntimeKind::LlamaCpp),
            "openai-compatible-http" => Ok(RuntimeKind::OpenAiCompatibleHttp),
            "hosted-api" => Ok(RuntimeKind::HostedApi),
            "core-ml" => Ok(RuntimeKind::CoreMl),
            _ => Err(LoomError::invalid(format!(
                "unknown runtime kind {value:?}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DownloadState {
    Queued,
    Resolving,
    Downloading,
    Verifying,
    Installed,
    Failed,
    Cancelled,
}

impl DownloadState {
    pub const fn as_str(self) -> &'static str {
        match self {
            DownloadState::Queued => "queued",
            DownloadState::Resolving => "resolving",
            DownloadState::Downloading => "downloading",
            DownloadState::Verifying => "verifying",
            DownloadState::Installed => "installed",
            DownloadState::Failed => "failed",
            DownloadState::Cancelled => "cancelled",
        }
    }

    pub const fn stable_tag(self) -> u8 {
        match self {
            DownloadState::Queued => 1,
            DownloadState::Resolving => 2,
            DownloadState::Downloading => 3,
            DownloadState::Verifying => 4,
            DownloadState::Installed => 5,
            DownloadState::Failed => 6,
            DownloadState::Cancelled => 7,
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            DownloadState::Installed | DownloadState::Failed | DownloadState::Cancelled
        )
    }

    pub const fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (DownloadState::Queued, DownloadState::Resolving)
                | (DownloadState::Queued, DownloadState::Cancelled)
                | (DownloadState::Resolving, DownloadState::Resolving)
                | (DownloadState::Resolving, DownloadState::Downloading)
                | (DownloadState::Resolving, DownloadState::Failed)
                | (DownloadState::Resolving, DownloadState::Cancelled)
                | (DownloadState::Downloading, DownloadState::Resolving)
                | (DownloadState::Downloading, DownloadState::Verifying)
                | (DownloadState::Downloading, DownloadState::Failed)
                | (DownloadState::Downloading, DownloadState::Cancelled)
                | (DownloadState::Verifying, DownloadState::Resolving)
                | (DownloadState::Verifying, DownloadState::Installed)
                | (DownloadState::Verifying, DownloadState::Failed)
                | (DownloadState::Verifying, DownloadState::Cancelled)
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct DownloadJob {
    pub id: String,
    pub model: ModelRef,
    pub runtime: RuntimeKind,
    pub state: DownloadState,
    pub requested_files: Vec<String>,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub failure: Option<String>,
}

impl DownloadJob {
    pub fn to_cbor_value(&self) -> CborValue {
        CborValue::Array(vec![
            CborValue::Uint(1),
            CborValue::Text(self.id.clone()),
            self.model.to_cbor_value(),
            CborValue::Uint(u64::from(self.runtime.stable_tag())),
            CborValue::Uint(u64::from(self.state.stable_tag())),
            CborValue::Array(
                self.requested_files
                    .iter()
                    .map(|file| CborValue::Text(file.clone()))
                    .collect(),
            ),
            CborValue::Uint(self.downloaded_bytes),
            optional_uint(self.total_bytes),
            optional_text(self.failure.as_deref()),
        ])
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct InferenceInstanceSettings {
    pub overrides: BTreeMap<String, String>,
}

impl InferenceInstanceSettings {
    pub fn empty() -> Self {
        Self {
            overrides: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct InferenceInstanceDescriptor {
    pub name: String,
    pub kind: InferenceModelKind,
    pub model: ModelRef,
    pub runtime: RuntimeKind,
    pub preset: Option<String>,
    pub settings: InferenceInstanceSettings,
    pub resolved_settings: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct HardwareReport {
    pub cpu_arch: String,
    pub os: String,
    pub target_triple: Option<String>,
    pub cpu_count: u32,
    pub total_memory_bytes: Option<u64>,
    pub metal_available: bool,
    pub cuda_available: bool,
    #[serde(default)]
    pub candle_cpu_compiled: bool,
    #[serde(default)]
    pub candle_cuda_compiled: bool,
    pub browser_storage_quota_bytes: Option<u64>,
    pub compiled_runtimes: Vec<RuntimeKind>,
    pub hf_home: Option<String>,
    pub hf_cache_dir: Option<String>,
}

impl HardwareReport {
    pub fn supports_runtime(&self, runtime: RuntimeKind) -> bool {
        match runtime {
            RuntimeKind::CandleSafetensors => {
                self.candle_cpu_compiled
                    || self.candle_cuda_compiled
                    || self.compiled_runtimes.contains(&runtime)
            }
            _ => self.compiled_runtimes.contains(&runtime),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ModelFitReport {
    pub model: ModelRef,
    pub runtime: RuntimeKind,
    pub runnable: bool,
    pub reasons: Vec<ModelFitReason>,
    pub estimated_memory_bytes: Option<u64>,
}

impl ModelFitReport {
    pub fn runnable(model: ModelRef, runtime: RuntimeKind) -> Self {
        Self {
            model,
            runtime,
            runnable: true,
            reasons: Vec::new(),
            estimated_memory_bytes: None,
        }
    }

    pub fn blocked(model: ModelRef, runtime: RuntimeKind, reasons: Vec<ModelFitReason>) -> Self {
        Self {
            model,
            runtime,
            runnable: false,
            reasons,
            estimated_memory_bytes: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModelFitReason {
    RuntimeNotCompiled,
    UnsupportedModelKind,
    UnsupportedFormat,
    MissingGpu,
    InsufficientMemory,
    MissingTokenizer,
    MissingConfig,
    MissingWeights,
    MissingFile(String),
    MissingDigest(String),
    UnsupportedDigest(String),
    DigestMismatch(String),
    GatedModel,
    GatedStatusUnknown,
    LicenseBlocked,
    LicenseUnknown,
}

fn optional_text(value: Option<&str>) -> CborValue {
    value
        .map(|value| CborValue::Text(value.to_string()))
        .unwrap_or(CborValue::Null)
}

fn optional_uint(value: Option<u64>) -> CborValue {
    value.map(CborValue::Uint).unwrap_or(CborValue::Null)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_kind_tags_are_stable_and_distinct() {
        assert_eq!(InferenceModelKind::Llm.as_str(), "llm");
        assert_eq!(InferenceModelKind::TextEmbedding.as_str(), "text-embedding");
        assert_eq!(InferenceModelKind::Llm.stable_tag(), 1);
        assert_eq!(InferenceModelKind::TextEmbedding.stable_tag(), 2);
        assert_eq!(
            InferenceModelKind::from_stable_tag(1).unwrap(),
            InferenceModelKind::Llm
        );
        assert_eq!(
            InferenceModelKind::from_stable_tag(2).unwrap(),
            InferenceModelKind::TextEmbedding
        );
        assert!(InferenceModelKind::from_stable_tag(3).is_err());
    }

    #[test]
    fn model_ref_routes_kind_without_cross_wiring() {
        let llm = ModelRef::new(InferenceModelKind::Llm, "Qwen/Qwen2.5-0.5B-Instruct-GGUF");
        let embedding = ModelRef::new(
            InferenceModelKind::TextEmbedding,
            "sentence-transformers/all-MiniLM-L6-v2",
        );

        assert!(llm.is_llm());
        assert!(!llm.is_text_embedding());
        assert!(embedding.is_text_embedding());
        assert!(!embedding.is_llm());
    }

    #[test]
    fn model_ref_canonical_bytes_change_with_revision() {
        let main = ModelRef::new(
            InferenceModelKind::TextEmbedding,
            "sentence-transformers/all-MiniLM-L6-v2",
        );
        let pinned = main
            .clone()
            .with_revision(RevisionRef::Commit("0123456789abcdef".to_string()));

        assert_ne!(
            main.canonical_bytes().unwrap(),
            pinned.canonical_bytes().unwrap()
        );
    }

    #[test]
    fn runtime_parse_roundtrips_stable_tags() {
        for runtime in [
            RuntimeKind::CandleSafetensors,
            RuntimeKind::CandleGguf,
            RuntimeKind::Mlx,
            RuntimeKind::Ollama,
            RuntimeKind::LlamaCpp,
            RuntimeKind::OpenAiCompatibleHttp,
            RuntimeKind::HostedApi,
            RuntimeKind::CoreMl,
        ] {
            assert_eq!(RuntimeKind::parse(runtime.as_str()).unwrap(), runtime);
            assert_eq!(
                RuntimeKind::from_stable_tag(runtime.stable_tag()).unwrap(),
                runtime
            );
        }
        assert!(RuntimeKind::parse("unknown").is_err());
    }

    #[test]
    fn download_state_allows_forward_terminal_and_active_resume_transitions() {
        assert!(DownloadState::Queued.can_transition_to(DownloadState::Resolving));
        assert!(DownloadState::Resolving.can_transition_to(DownloadState::Downloading));
        assert!(DownloadState::Downloading.can_transition_to(DownloadState::Verifying));
        assert!(DownloadState::Verifying.can_transition_to(DownloadState::Installed));
        assert!(DownloadState::Downloading.can_transition_to(DownloadState::Cancelled));
        assert!(DownloadState::Resolving.can_transition_to(DownloadState::Failed));
        assert!(DownloadState::Downloading.can_transition_to(DownloadState::Resolving));
        assert!(DownloadState::Verifying.can_transition_to(DownloadState::Resolving));

        assert!(!DownloadState::Queued.can_transition_to(DownloadState::Installed));
        assert!(!DownloadState::Installed.can_transition_to(DownloadState::Queued));
        assert!(!DownloadState::Failed.can_transition_to(DownloadState::Resolving));
        assert!(!DownloadState::Cancelled.can_transition_to(DownloadState::Resolving));
        assert!(DownloadState::Installed.is_terminal());
        assert!(DownloadState::Failed.is_terminal());
        assert!(DownloadState::Cancelled.is_terminal());
    }

    #[test]
    fn download_job_has_canonical_descriptor_value() {
        let job = DownloadJob {
            id: "job-1".to_string(),
            model: ModelRef::new(InferenceModelKind::Llm, "Qwen/Qwen2.5-0.5B-Instruct-GGUF")
                .with_file("qwen2.5-0.5b-instruct-q4_k_m.gguf"),
            runtime: RuntimeKind::CandleGguf,
            state: DownloadState::Downloading,
            requested_files: vec!["model.gguf".to_string()],
            downloaded_bytes: 42,
            total_bytes: Some(100),
            failure: None,
        };

        let encoded = loom_codec::encode(&job.to_cbor_value()).unwrap();
        let decoded = loom_codec::decode(&encoded).unwrap();
        assert_eq!(decoded, job.to_cbor_value());
    }

    #[test]
    fn hardware_report_checks_compiled_runtime() {
        let report = HardwareReport {
            cpu_arch: "aarch64".to_string(),
            os: "macos".to_string(),
            target_triple: Some("aarch64-apple-darwin".to_string()),
            cpu_count: 8,
            total_memory_bytes: Some(16 * 1024 * 1024 * 1024),
            metal_available: true,
            cuda_available: false,
            candle_cpu_compiled: false,
            candle_cuda_compiled: false,
            browser_storage_quota_bytes: None,
            compiled_runtimes: vec![RuntimeKind::CandleGguf, RuntimeKind::Mlx],
            hf_home: None,
            hf_cache_dir: None,
        };

        assert!(report.supports_runtime(RuntimeKind::Mlx));
        assert!(!report.supports_runtime(RuntimeKind::Ollama));
    }

    #[test]
    fn hardware_report_checks_candle_accelerator_flags() {
        let report = HardwareReport {
            cpu_arch: "x86_64".to_string(),
            os: "linux".to_string(),
            target_triple: Some("x86_64-unknown-linux-gnu".to_string()),
            cpu_count: 16,
            total_memory_bytes: Some(64 * 1024 * 1024 * 1024),
            metal_available: false,
            cuda_available: true,
            candle_cpu_compiled: false,
            candle_cuda_compiled: true,
            browser_storage_quota_bytes: None,
            compiled_runtimes: Vec::new(),
            hf_home: None,
            hf_cache_dir: None,
        };

        assert!(report.supports_runtime(RuntimeKind::CandleSafetensors));
    }
}
