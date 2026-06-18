//! Inference capability: a provider-abstracted seam for LLM completion requests.
//!
//! Inference is a first-class loom capability, not an MCP-only feature. Any subsystem - a program, a
//! trigger, a scheduled task, GraphRAG, or a serving surface - tickets an [`InferenceRequest`] through
//! an [`InferenceProvider`]; the provider is the swappable backend. The MCP `sampling/createMessage`
//! surface is one provider (it lives in `loom-mcp`, which holds the client peer). `loom-core` defines
//! the seam and a no-provider default; it links no model itself, so the capability is decoupled from
//! both MCP and any single consumer (see 0043).
//!
//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use crate::error::{Code, LoomError, Result};
use crate::workspace::WorkspaceId;
use crate::{Loom, ObjectStore};

pub use loom_inference::{
    Llm, LlmHandle, LlmRequest, LlmResponse, LlmStream, LlmStreamEvent, TextEmbedding,
    TextEmbeddingHandle, TextEmbeddingModel,
};

pub const INFERENCE_INSTANCE_STATE_PATH: &str = ".loom/inference/instances.json";

pub fn inference_instance_state<S: ObjectStore>(
    loom: &Loom<S>,
    workspace: WorkspaceId,
) -> Result<loom_inference::InferenceInstanceState> {
    match loom.read_file_reserved(workspace, INFERENCE_INSTANCE_STATE_PATH) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| {
            LoomError::corrupt(format!("invalid inference instance state: {error}"))
        }),
        Err(error) if error.code == Code::NotFound => {
            Ok(loom_inference::InferenceInstanceState::default())
        }
        Err(error) => Err(error),
    }
}

pub fn put_inference_instance_state<S: ObjectStore>(
    loom: &mut Loom<S>,
    workspace: WorkspaceId,
    state: &loom_inference::InferenceInstanceState,
) -> Result<()> {
    let bytes = serde_json::to_vec(state).map_err(|error| {
        LoomError::corrupt(format!("invalid inference instance state: {error}"))
    })?;
    loom.create_directory_reserved(workspace, ".loom/inference", true)?;
    loom.write_file_reserved(workspace, INFERENCE_INSTANCE_STATE_PATH, &bytes, 0o100644)
}

/// The author of a turn in an inference exchange. A system prompt is carried separately on the
/// request (see [`InferenceRequest::system_prompt`]), mirroring how hosted sampling APIs separate it
/// from the user/assistant transcript.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// A caller / end-user turn.
    User,
    /// A prior model turn.
    Assistant,
}

/// One message in the conversation submitted for completion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// Who authored the turn.
    pub role: Role,
    /// The turn's text content.
    pub content: String,
}

impl Message {
    /// A user turn.
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
        }
    }

    /// An assistant turn.
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
        }
    }
}

/// Soft model-selection preferences. A provider maps these to a concrete model; every field is
/// optional so a caller states only what it cares about. The three priorities are each on a 0.0..=1.0
/// scale, matching the axes hosted sampling surfaces expose.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ModelPreferences {
    /// Ordered model-name hints, most preferred first (substrings a provider may match).
    pub hints: Vec<String>,
    /// Priority on minimizing cost (0.0..=1.0).
    pub cost_priority: Option<f32>,
    /// Priority on minimizing latency (0.0..=1.0).
    pub speed_priority: Option<f32>,
    /// Priority on maximizing capability (0.0..=1.0).
    pub intelligence_priority: Option<f32>,
}

/// A provider-agnostic completion request: everything a backend needs to fulfil one call.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InferenceRequest {
    /// The conversation turns, in order.
    pub messages: Vec<Message>,
    /// Optional system prompt applied ahead of the messages.
    pub system_prompt: Option<String>,
    /// Upper bound on tokens to generate (a hint; a provider may cap lower).
    pub max_tokens: Option<u32>,
    /// Sampling temperature override (0.0..=2.0), if the caller wants one.
    pub temperature: Option<f32>,
    /// Soft model-selection preferences.
    pub preferences: ModelPreferences,
}

/// A completion result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InferenceResponse {
    /// The concrete model the provider used.
    pub model: String,
    /// The generated assistant content.
    pub content: String,
    /// Why generation stopped, if the provider reported it (free-form; e.g. "end_turn",
    /// "max_tokens", "stop_sequence").
    pub stop_reason: Option<String>,
}

/// Compatibility name for the text embedding model profile recorded by the vector facet.
pub type EmbeddingModel = TextEmbeddingModel;

/// A swappable inference backend. Implementors live outside `loom-core` (an MCP client peer, a remote
/// API client, a local model runtime); core only owns the contract.
pub trait InferenceProvider: Send + Sync {
    /// Stable provider identifier (e.g. "mcp-sampling", "local", or a vendor id).
    fn id(&self) -> &str;

    /// Run one completion request to completion.
    fn infer(&self, request: &InferenceRequest) -> Result<InferenceResponse>;
}

/// A swappable embedding backend. Core owns the contract; model runtimes and remote clients live in
/// provider crates or host bindings.
pub trait EmbeddingProvider: Send + Sync {
    fn model_id(&self) -> &str;

    fn dimension(&self) -> usize;

    fn weights_digest(&self) -> Option<&str> {
        None
    }

    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
}

/// The inference capability handle: holds the active provider (if any) and dispatches to it.
///
/// A loom is constructed with [`Inference::none`] (the decoupled default - core links no model); a
/// host installs a backend with [`Inference::with_provider`]. With no provider, [`Inference::infer`]
/// returns [`crate::error::Code::Unsupported`] rather than guessing, so callers (programs, triggers,
/// scheduled tasks, GraphRAG, serving surfaces) get a stable signal that inference is unavailable.
pub struct Inference {
    provider: Option<Box<dyn InferenceProvider>>,
}

impl Inference {
    /// A capability with no backend installed. Every [`Inference::infer`] returns `UNSUPPORTED` until
    /// a provider is set.
    pub fn none() -> Self {
        Self { provider: None }
    }

    /// A capability backed by `provider`.
    pub fn with_provider(provider: Box<dyn InferenceProvider>) -> Self {
        Self {
            provider: Some(provider),
        }
    }

    /// The active provider id, or `None` when no backend is installed.
    pub fn provider_id(&self) -> Option<&str> {
        self.provider.as_deref().map(InferenceProvider::id)
    }

    /// Whether a backend is installed.
    pub fn is_available(&self) -> bool {
        self.provider.is_some()
    }

    /// Run a completion through the active provider, or `UNSUPPORTED` when none is installed.
    pub fn infer(&self, request: &InferenceRequest) -> Result<InferenceResponse> {
        match self.provider.as_deref() {
            Some(provider) => provider.infer(request),
            None => Err(LoomError::unsupported(
                "no inference provider is configured for this loom",
            )),
        }
    }
}

/// Embedding capability handle with the same no-provider default as [`Inference`].
pub struct Embeddings {
    provider: Option<Box<dyn EmbeddingProvider>>,
}

impl Embeddings {
    pub fn none() -> Self {
        Self { provider: None }
    }

    pub fn with_provider(provider: Box<dyn EmbeddingProvider>) -> Self {
        Self {
            provider: Some(provider),
        }
    }

    pub fn model(&self) -> Option<EmbeddingModel> {
        self.provider.as_deref().map(|provider| {
            EmbeddingModel::new(
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
                "no embedding provider is configured for this loom",
            ));
        };
        let vectors = provider.embed(texts)?;
        if vectors.len() != texts.len() {
            return Err(LoomError::new(
                crate::error::Code::Internal,
                format!(
                    "embedding provider returned {} vectors for {} texts",
                    vectors.len(),
                    texts.len()
                ),
            ));
        }
        let dim = provider.dimension();
        if let Some(vector) = vectors.iter().find(|vector| vector.len() != dim) {
            return Err(LoomError::new(
                crate::error::Code::Internal,
                format!(
                    "embedding provider returned dimension {}, expected {dim}",
                    vector.len()
                ),
            ));
        }
        Ok(vectors)
    }
}

impl Default for Embeddings {
    fn default() -> Self {
        Self::none()
    }
}

impl core::fmt::Debug for Embeddings {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Embeddings")
            .field("model", &self.model())
            .finish()
    }
}

impl Default for Inference {
    fn default() -> Self {
        Self::none()
    }
}

impl core::fmt::Debug for Inference {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Inference")
            .field("provider", &self.provider_id())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Code;

    /// A trivial provider that echoes the last user turn back, to exercise the dispatch path.
    struct Echo;

    struct FixedEmbedding;

    impl InferenceProvider for Echo {
        fn id(&self) -> &str {
            "echo"
        }

        fn infer(&self, request: &InferenceRequest) -> Result<InferenceResponse> {
            let content = request
                .messages
                .last()
                .map(|m| m.content.clone())
                .unwrap_or_default();
            Ok(InferenceResponse {
                model: "echo-1".to_string(),
                content,
                stop_reason: Some("end_turn".to_string()),
            })
        }
    }

    impl EmbeddingProvider for FixedEmbedding {
        fn model_id(&self) -> &str {
            "fixed-embedding"
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

    #[test]
    fn no_provider_is_unsupported() {
        let inference = Inference::none();
        assert!(!inference.is_available());
        assert_eq!(inference.provider_id(), None);
        let err = inference.infer(&InferenceRequest::default()).unwrap_err();
        assert_eq!(err.code, Code::Unsupported);
    }

    #[test]
    fn installed_provider_dispatches() {
        let inference = Inference::with_provider(Box::new(Echo));
        assert!(inference.is_available());
        assert_eq!(inference.provider_id(), Some("echo"));
        let request = InferenceRequest {
            messages: vec![Message::user("ping")],
            ..Default::default()
        };
        let response = inference.infer(&request).unwrap();
        assert_eq!(response.content, "ping");
        assert_eq!(response.model, "echo-1");
        assert_eq!(response.stop_reason.as_deref(), Some("end_turn"));
    }

    #[test]
    fn embedding_provider_dispatches_and_reports_model() {
        let embeddings = Embeddings::with_provider(Box::new(FixedEmbedding));
        assert!(embeddings.is_available());
        assert_eq!(
            embeddings.model(),
            Some(EmbeddingModel::new(
                "fixed-embedding",
                2,
                Some("sha256:fixed".to_string())
            ))
        );
        let vectors = embeddings
            .embed(&["a".to_string(), "bc".to_string()])
            .unwrap();
        assert_eq!(vectors, vec![vec![1.0, 97.0], vec![2.0, 197.0]]);
    }

    #[test]
    fn no_embedding_provider_is_unsupported() {
        let embeddings = Embeddings::none();
        assert!(!embeddings.is_available());
        assert_eq!(embeddings.model(), None);
        let err = embeddings.embed(&["text".to_string()]).unwrap_err();
        assert_eq!(err.code, Code::Unsupported);
    }
}
