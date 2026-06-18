//! Inference provider contracts and local activation handles.

use loom_types::{Code, LoomError, Result};

pub mod activation;
pub mod browser;
pub mod catalog;
pub mod compat;
#[cfg(feature = "genai")]
pub mod genai;
pub mod hardware;
pub mod instances;
pub mod inventory;
pub mod jobs;
pub mod llama_cpp;
pub mod mlx;
pub mod runtimes;

#[cfg(feature = "candle-cpu")]
pub mod candle_runtime;

#[cfg(all(feature = "native-hf", not(target_arch = "wasm32")))]
pub mod hf;
#[cfg(feature = "ollama-rs")]
pub mod ollama;

pub use activation::{activate_llm, activate_text_embedding};
pub use browser::{
    BrowserAcquiredModel, BrowserDownloadEvent, BrowserDownloadRequest, BrowserModelAcquirer,
    BrowserModelFetch, BrowserModelStorage, BrowserStoredFile,
    DEFAULT_BROWSER_MODEL_FILE_LIMIT_BYTES, InMemoryBrowserModelStorage, browser_storage_key,
};
pub use catalog::{CuratedModelSpec, curated_models};
pub use compat::{evaluate_curated_model_fit, evaluate_installed_model_fit};
#[cfg(feature = "genai")]
pub use genai::{GenaiLlm, GenaiTextEmbedding};
pub use hardware::{HardwareProbeInputs, compiled_runtimes, probe_hardware, probe_hardware_with};
#[cfg(all(feature = "native-hf", not(target_arch = "wasm32")))]
pub use hf::{HfCachePaths, HfDownloadEvent, HfDownloadRequest, HfDownloadedFile, HfDownloader};
pub use instances::{
    InferenceInstanceState, VectorWorkspaceBinding, build_instance_descriptor,
    update_instance_descriptor,
};
pub use inventory::{
    InstallInventory, InstalledModelFile, InstalledModelRecord, discover_installed_model,
    discover_installed_models,
};
pub use jobs::{
    DownloadArtifact, DownloadEvent, DownloadExecutor, DownloadJobManager, DownloadJobPlan,
    DownloadJobs,
};
pub use llama_cpp::{
    LOOM_LLAMA_CPP_ADAPTER_ABI_VERSION, LOOM_LLAMA_CPP_REQUEST_SCHEMA_VERSION,
    LOOM_LLAMA_CPP_RESPONSE_SCHEMA_VERSION, LOOM_LLAMA_CPP_RUNTIME_INFO_SCHEMA_VERSION,
    LOOM_LLAMA_CPP_STATUS_BUFFER_TOO_SMALL, LOOM_LLAMA_CPP_STATUS_INVALID_REQUEST,
    LOOM_LLAMA_CPP_STATUS_OK, LOOM_LLAMA_CPP_STATUS_RUNTIME_ERROR,
    LOOM_LLAMA_CPP_SYMBOL_ABI_VERSION, LOOM_LLAMA_CPP_SYMBOL_COMPLETE,
    LOOM_LLAMA_CPP_SYMBOL_LAST_ERROR, LOOM_LLAMA_CPP_SYMBOL_LOAD_LLM,
    LOOM_LLAMA_CPP_SYMBOL_RELEASE_HANDLE, LOOM_LLAMA_CPP_SYMBOL_RUNTIME_INFO, LlamaCppAdapterAbi,
    LlamaCppBundleFile, LlamaCppBundleInspection, LlamaCppBundleLayout, LlamaCppBundleStatus,
    LlamaCppDynamicAdapter, LlamaCppRuntimeInfo, default_llama_cpp_bundle_dir,
    inspect_llama_cpp_bundle, llama_cpp_adapter_library,
};
pub use mlx::{
    LOOM_MLX_ADAPTER_ABI_VERSION, LOOM_MLX_ADAPTER_LIBRARY, LOOM_MLX_RUNTIME_INFO_SCHEMA_VERSION,
    LOOM_MLX_SYMBOL_ABI_VERSION, LOOM_MLX_SYMBOL_LAST_ERROR, LOOM_MLX_SYMBOL_LOAD_LLM,
    LOOM_MLX_SYMBOL_LOAD_TEXT_EMBEDDING, LOOM_MLX_SYMBOL_RELEASE_HANDLE,
    LOOM_MLX_SYMBOL_RUNTIME_INFO, MLX_C_LIBRARY, MLX_LIBRARY, MLX_METAL_LIBRARY, MlxAdapterAbi,
    MlxBundleFile, MlxBundleInspection, MlxBundleLayout, MlxBundleStatus, MlxDynamicAdapter,
    MlxRuntimeInfo, default_mlx_bundle_dir, inspect_mlx_bundle,
};
#[cfg(feature = "ollama-rs")]
pub use ollama::{OllamaRsAdapter, OllamaRsLlm, OllamaRsTextEmbedding};
pub use runtimes::{
    CoreMlAdapter, GgufAdapter, MlxAdapter, OllamaAdapter, OllamaModelInfo, RuntimeSupportReport,
    ollama_model_ref, probe_one_runtime, probe_runtime_support,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

impl Message {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ModelPreferences {
    pub hints: Vec<String>,
    pub cost_priority: Option<f32>,
    pub speed_priority: Option<f32>,
    pub intelligence_priority: Option<f32>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct LlmRequest {
    pub messages: Vec<Message>,
    pub system_prompt: Option<String>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub preferences: ModelPreferences,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmResponse {
    pub model: String,
    pub content: String,
    pub stop_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LlmStreamEvent {
    Token(String),
    Done(LlmResponse),
}

pub trait LlmStream: Send {
    fn next(&mut self) -> Result<Option<LlmStreamEvent>>;
}

pub trait Llm: Send + Sync {
    fn model_id(&self) -> &str;

    fn complete(&self, request: &LlmRequest) -> Result<LlmResponse>;

    fn stream(&self, _request: &LlmRequest) -> Result<Box<dyn LlmStream>> {
        Err(LoomError::unsupported(
            "llm provider does not support streaming",
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEmbeddingModel {
    pub model_id: String,
    pub dimension: usize,
    pub weights_digest: Option<String>,
}

impl TextEmbeddingModel {
    pub fn new(
        model_id: impl Into<String>,
        dimension: usize,
        weights_digest: Option<String>,
    ) -> Self {
        Self {
            model_id: model_id.into(),
            dimension,
            weights_digest,
        }
    }
}

pub trait TextEmbedding: Send + Sync {
    fn model_id(&self) -> &str;

    fn dimension(&self) -> usize;

    fn weights_digest(&self) -> Option<&str> {
        None
    }

    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
}

pub struct LlmHandle {
    provider: Option<Box<dyn Llm>>,
}

impl LlmHandle {
    pub fn none() -> Self {
        Self { provider: None }
    }

    pub fn with_provider(provider: Box<dyn Llm>) -> Self {
        Self {
            provider: Some(provider),
        }
    }

    pub fn model_id(&self) -> Option<&str> {
        self.provider.as_deref().map(Llm::model_id)
    }

    pub fn is_available(&self) -> bool {
        self.provider.is_some()
    }

    pub fn complete(&self, request: &LlmRequest) -> Result<LlmResponse> {
        match self.provider.as_deref() {
            Some(provider) => provider.complete(request),
            None => Err(LoomError::unsupported("no llm provider is configured")),
        }
    }

    pub fn stream(&self, request: &LlmRequest) -> Result<Box<dyn LlmStream>> {
        match self.provider.as_deref() {
            Some(provider) => provider.stream(request),
            None => Err(LoomError::unsupported("no llm provider is configured")),
        }
    }
}

impl Default for LlmHandle {
    fn default() -> Self {
        Self::none()
    }
}

impl core::fmt::Debug for LlmHandle {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LlmHandle")
            .field("model", &self.model_id())
            .finish()
    }
}

pub struct TextEmbeddingHandle {
    provider: Option<Box<dyn TextEmbedding>>,
}

impl TextEmbeddingHandle {
    pub fn none() -> Self {
        Self { provider: None }
    }

    pub fn with_provider(provider: Box<dyn TextEmbedding>) -> Self {
        Self {
            provider: Some(provider),
        }
    }

    pub fn model(&self) -> Option<TextEmbeddingModel> {
        self.provider.as_deref().map(|provider| {
            TextEmbeddingModel::new(
                provider.model_id(),
                provider.dimension(),
                provider.weights_digest().map(str::to_string),
            )
        })
    }

    pub fn is_available(&self) -> bool {
        self.provider.is_some()
    }

    pub fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let Some(provider) = self.provider.as_deref() else {
            return Err(LoomError::unsupported(
                "no text embedding provider is configured",
            ));
        };
        let vectors = provider.embed(texts)?;
        if vectors.len() != texts.len() {
            return Err(LoomError::new(
                Code::Internal,
                format!(
                    "text embedding provider returned {} vectors for {} texts",
                    vectors.len(),
                    texts.len()
                ),
            ));
        }
        let dimension = provider.dimension();
        if let Some(vector) = vectors.iter().find(|vector| vector.len() != dimension) {
            return Err(LoomError::new(
                Code::Internal,
                format!(
                    "text embedding provider returned dimension {}, expected {dimension}",
                    vector.len()
                ),
            ));
        }
        Ok(vectors)
    }
}

impl Default for TextEmbeddingHandle {
    fn default() -> Self {
        Self::none()
    }
}

impl core::fmt::Debug for TextEmbeddingHandle {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TextEmbeddingHandle")
            .field("model", &self.model())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EchoLlm;

    impl Llm for EchoLlm {
        fn model_id(&self) -> &str {
            "echo-llm"
        }

        fn complete(&self, request: &LlmRequest) -> Result<LlmResponse> {
            let content = request
                .messages
                .last()
                .map(|message| message.content.clone())
                .unwrap_or_default();
            Ok(LlmResponse {
                model: self.model_id().to_string(),
                content,
                stop_reason: Some("end_turn".to_string()),
            })
        }
    }

    struct FixedEmbedding;

    impl TextEmbedding for FixedEmbedding {
        fn model_id(&self) -> &str {
            "fixed-text-embedding"
        }

        fn dimension(&self) -> usize {
            2
        }

        fn weights_digest(&self) -> Option<&str> {
            Some("sha256:fixed")
        }

        fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
            Ok(texts
                .iter()
                .map(|text| vec![text.len() as f32, text.bytes().map(f32::from).sum()])
                .collect())
        }
    }

    struct BadEmbedding;

    impl TextEmbedding for BadEmbedding {
        fn model_id(&self) -> &str {
            "bad-text-embedding"
        }

        fn dimension(&self) -> usize {
            3
        }

        fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
            Ok(texts.iter().map(|_| vec![1.0, 2.0]).collect())
        }
    }

    #[test]
    fn llm_handle_without_provider_is_unsupported() {
        let handle = LlmHandle::none();
        assert!(!handle.is_available());
        assert_eq!(handle.model_id(), None);
        assert_eq!(
            handle.complete(&LlmRequest::default()).unwrap_err().code,
            Code::Unsupported
        );
    }

    #[test]
    fn llm_handle_dispatches_to_provider() {
        let handle = LlmHandle::with_provider(Box::new(EchoLlm));
        let request = LlmRequest {
            messages: vec![Message::user("hello")],
            ..LlmRequest::default()
        };
        let response = handle.complete(&request).unwrap();
        assert_eq!(handle.model_id(), Some("echo-llm"));
        assert_eq!(response.content, "hello");
    }

    #[test]
    fn text_embedding_handle_without_provider_is_unsupported() {
        let handle = TextEmbeddingHandle::none();
        assert!(!handle.is_available());
        assert_eq!(
            handle.embed(&["hello".to_string()]).unwrap_err().code,
            Code::Unsupported
        );
    }

    #[test]
    fn text_embedding_handle_reports_model_and_embeds() {
        let handle = TextEmbeddingHandle::with_provider(Box::new(FixedEmbedding));
        let model = handle.model().unwrap();
        assert_eq!(model.model_id, "fixed-text-embedding");
        assert_eq!(model.dimension, 2);
        assert_eq!(model.weights_digest.as_deref(), Some("sha256:fixed"));

        let vectors = handle.embed(&["a".to_string(), "bb".to_string()]).unwrap();
        assert_eq!(vectors, vec![vec![1.0, 97.0], vec![2.0, 196.0]]);
    }

    #[test]
    fn text_embedding_handle_rejects_wrong_dimension() {
        let handle = TextEmbeddingHandle::with_provider(Box::new(BadEmbedding));
        let err = handle.embed(&["hello".to_string()]).unwrap_err();
        assert_eq!(err.code, Code::Internal);
    }
}
