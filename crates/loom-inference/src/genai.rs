//! Hosted provider adapters backed by `genai`.

use loom_types::{Code, LoomError, Result};
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::runtime::{Builder, Runtime};

use crate::{Llm, LlmRequest, LlmResponse, Message, Role, TextEmbedding, TextEmbeddingModel};

#[derive(Debug)]
pub struct GenaiLlm {
    model_id: String,
    settings: GenaiLlmSettings,
    backend: Arc<dyn GenaiBackend>,
}

impl GenaiLlm {
    pub fn new(model_id: impl Into<String>) -> Result<Self> {
        Self::with_settings(model_id, GenaiLlmSettings::default())
    }

    pub fn with_settings(model_id: impl Into<String>, settings: GenaiLlmSettings) -> Result<Self> {
        Ok(Self {
            model_id: model_id.into(),
            settings,
            backend: Arc::new(GenaiClientBackend::new()?),
        })
    }

    pub fn with_client(model_id: impl Into<String>, client: ::genai::Client) -> Result<Self> {
        Self::with_client_and_settings(model_id, client, GenaiLlmSettings::default())
    }

    pub fn with_client_and_settings(
        model_id: impl Into<String>,
        client: ::genai::Client,
        settings: GenaiLlmSettings,
    ) -> Result<Self> {
        Ok(Self {
            model_id: model_id.into(),
            settings,
            backend: Arc::new(GenaiClientBackend::with_client(client)?),
        })
    }

    #[cfg(test)]
    fn with_backend(model_id: impl Into<String>, backend: Arc<dyn GenaiBackend>) -> Self {
        Self {
            model_id: model_id.into(),
            settings: GenaiLlmSettings::default(),
            backend,
        }
    }

    #[cfg(test)]
    fn with_backend_and_settings(
        model_id: impl Into<String>,
        settings: GenaiLlmSettings,
        backend: Arc<dyn GenaiBackend>,
    ) -> Self {
        Self {
            model_id: model_id.into(),
            settings,
            backend,
        }
    }
}

impl Llm for GenaiLlm {
    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn complete(&self, request: &LlmRequest) -> Result<LlmResponse> {
        let chat_request = self.settings.to_chat_request(request);
        let options = self.settings.to_chat_options(request);
        self.backend
            .complete(self.model_id(), chat_request, &options)
    }
}

#[derive(Debug)]
pub struct GenaiTextEmbedding {
    model: TextEmbeddingModel,
    settings: GenaiTextEmbeddingSettings,
    backend: Arc<dyn GenaiBackend>,
}

impl GenaiTextEmbedding {
    pub fn new(model_id: impl Into<String>, dimension: usize) -> Result<Self> {
        Self::with_settings(model_id, dimension, GenaiTextEmbeddingSettings::default())
    }

    pub fn with_settings(
        model_id: impl Into<String>,
        dimension: usize,
        settings: GenaiTextEmbeddingSettings,
    ) -> Result<Self> {
        let dimension = settings.effective_dimension(dimension)?;
        Ok(Self {
            model: TextEmbeddingModel::new(model_id, dimension, None),
            settings,
            backend: Arc::new(GenaiClientBackend::new()?),
        })
    }

    pub fn with_client(
        model_id: impl Into<String>,
        dimension: usize,
        client: ::genai::Client,
    ) -> Result<Self> {
        Self::with_client_and_settings(
            model_id,
            dimension,
            client,
            GenaiTextEmbeddingSettings::default(),
        )
    }

    pub fn with_client_and_settings(
        model_id: impl Into<String>,
        dimension: usize,
        client: ::genai::Client,
        settings: GenaiTextEmbeddingSettings,
    ) -> Result<Self> {
        let dimension = settings.effective_dimension(dimension)?;
        Ok(Self {
            model: TextEmbeddingModel::new(model_id, dimension, None),
            settings,
            backend: Arc::new(GenaiClientBackend::with_client(client)?),
        })
    }

    #[cfg(test)]
    fn with_backend(
        model_id: impl Into<String>,
        dimension: usize,
        backend: Arc<dyn GenaiBackend>,
    ) -> Self {
        Self {
            model: TextEmbeddingModel::new(model_id, dimension, None),
            settings: GenaiTextEmbeddingSettings::default(),
            backend,
        }
    }

    #[cfg(test)]
    fn with_backend_and_settings(
        model_id: impl Into<String>,
        dimension: usize,
        settings: GenaiTextEmbeddingSettings,
        backend: Arc<dyn GenaiBackend>,
    ) -> Result<Self> {
        let dimension = settings.effective_dimension(dimension)?;
        Ok(Self {
            model: TextEmbeddingModel::new(model_id, dimension, None),
            settings,
            backend,
        })
    }
}

impl TextEmbedding for GenaiTextEmbedding {
    fn model_id(&self) -> &str {
        &self.model.model_id
    }

    fn dimension(&self) -> usize {
        self.model.dimension
    }

    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let options = self.settings.to_embed_options(self.dimension());
        let vectors = self
            .backend
            .embed(self.model_id(), texts, self.dimension(), &options)?;
        validate_embedding_vectors(texts.len(), self.dimension(), vectors)
    }
}

trait GenaiBackend: core::fmt::Debug + Send + Sync {
    fn complete(
        &self,
        model_id: &str,
        chat_request: ::genai::chat::ChatRequest,
        options: &::genai::chat::ChatOptions,
    ) -> Result<LlmResponse>;

    fn embed(
        &self,
        model_id: &str,
        texts: &[String],
        dimension: usize,
        options: &::genai::embed::EmbedOptions,
    ) -> Result<Vec<Vec<f32>>>;
}

#[derive(Debug)]
struct GenaiClientBackend {
    client: ::genai::Client,
    runtime: Runtime,
}

impl GenaiClientBackend {
    fn new() -> Result<Self> {
        Self::with_client(::genai::Client::default())
    }

    fn with_client(client: ::genai::Client) -> Result<Self> {
        Ok(Self {
            client,
            runtime: build_runtime()?,
        })
    }
}

impl GenaiBackend for GenaiClientBackend {
    fn complete(
        &self,
        model_id: &str,
        chat_request: ::genai::chat::ChatRequest,
        options: &::genai::chat::ChatOptions,
    ) -> Result<LlmResponse> {
        let response = self
            .runtime
            .block_on(self.client.exec_chat(model_id, chat_request, Some(options)))
            .map_err(genai_error)?;
        let stop_reason = response
            .stop_reason
            .as_ref()
            .map(|reason| reason.raw().to_string());
        Ok(LlmResponse {
            model: response.model_iden.model_name.to_string(),
            content: response
                .into_first_text()
                .ok_or_else(|| LoomError::unsupported("hosted provider returned no text"))?,
            stop_reason,
        })
    }

    fn embed(
        &self,
        model_id: &str,
        texts: &[String],
        _dimension: usize,
        options: &::genai::embed::EmbedOptions,
    ) -> Result<Vec<Vec<f32>>> {
        let response = self
            .runtime
            .block_on(
                self.client
                    .embed_batch(model_id, texts.to_vec(), Some(options)),
            )
            .map_err(genai_error)?;
        Ok(response.into_vectors())
    }
}

#[derive(Debug, Clone, Default)]
pub struct GenaiLlmSettings {
    pub max_tokens: Option<u32>,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub stop_sequences: Vec<String>,
    pub reasoning_effort: Option<GenaiReasoningEffort>,
    pub verbosity: Option<GenaiVerbosity>,
    pub seed: Option<u64>,
    pub service_tier: Option<GenaiServiceTier>,
    pub cache_control: Option<GenaiCacheControl>,
    pub prompt_cache_key: Option<String>,
    pub response_format: Option<GenaiResponseFormat>,
    pub tool_choice: Option<GenaiToolChoice>,
    pub extra_headers: BTreeMap<String, String>,
    pub extra_body: Option<serde_json::Value>,
    pub previous_response_id: Option<String>,
    pub store: Option<bool>,
    pub capture_usage: Option<bool>,
    pub capture_raw_body: Option<bool>,
    pub normalize_reasoning_content: Option<bool>,
}

impl GenaiLlmSettings {
    pub fn from_resolved_settings(settings: &BTreeMap<String, String>) -> Result<Self> {
        Ok(Self {
            max_tokens: optional_u32(settings, "max_tokens")?,
            temperature: optional_f64(settings, "temperature")?,
            top_p: optional_f64(settings, "top_p")?,
            stop_sequences: optional_string_list(settings, "stop")?,
            reasoning_effort: optional_parse(settings, "reasoning_effort")?,
            verbosity: optional_parse(settings, "verbosity")?,
            seed: optional_u64(settings, "seed")?,
            service_tier: optional_parse(settings, "service_tier")?,
            cache_control: optional_parse(settings, "cache_control")?,
            prompt_cache_key: settings.get("prompt_cache_key").cloned(),
            response_format: response_format_from_settings(settings)?,
            tool_choice: optional_parse(settings, "tool_choice")?,
            extra_headers: prefixed_settings(settings, "extra.header."),
            extra_body: optional_json_value(settings, "extra_body")?,
            previous_response_id: settings.get("previous_response_id").cloned(),
            store: optional_bool(settings, "store")?,
            capture_usage: optional_bool(settings, "capture_usage")?,
            capture_raw_body: optional_bool(settings, "capture_raw_body")?,
            normalize_reasoning_content: optional_bool(settings, "normalize_reasoning_content")?,
        })
    }

    fn to_chat_request(&self, request: &LlmRequest) -> ::genai::chat::ChatRequest {
        let mut chat_request = chat_request_from_llm_request(request);
        if let Some(previous_response_id) = &self.previous_response_id {
            chat_request = chat_request.with_previous_response_id(previous_response_id.clone());
        }
        if let Some(store) = self.store {
            chat_request = chat_request.with_store(store);
        }
        chat_request
    }

    fn to_chat_options(&self, request: &LlmRequest) -> ::genai::chat::ChatOptions {
        let mut options = ::genai::chat::ChatOptions::default();
        if let Some(max_tokens) = request.max_tokens.or(self.max_tokens) {
            options = options.with_max_tokens(max_tokens);
        }
        if let Some(temperature) = request.temperature.map(f64::from).or(self.temperature) {
            options = options.with_temperature(temperature);
        }
        if let Some(top_p) = self.top_p {
            options = options.with_top_p(top_p);
        }
        if !self.stop_sequences.is_empty() {
            options = options.with_stop_sequences(self.stop_sequences.clone());
        }
        if let Some(reasoning_effort) = self.reasoning_effort {
            options = options.with_reasoning_effort(reasoning_effort.into());
        }
        if let Some(verbosity) = self.verbosity {
            options = options.with_verbosity(verbosity.into());
        }
        if let Some(seed) = self.seed {
            options = options.with_seed(seed);
        }
        if let Some(service_tier) = self.service_tier {
            options = options.with_service_tier(service_tier.into());
        }
        if let Some(cache_control) = self.cache_control {
            options = options.with_cache_control(cache_control.into());
        }
        if let Some(prompt_cache_key) = &self.prompt_cache_key {
            options = options.with_prompt_cache_key(prompt_cache_key.clone());
        }
        if let Some(response_format) = &self.response_format {
            options = options.with_response_format(response_format.to_genai());
        }
        if let Some(tool_choice) = &self.tool_choice {
            options = options.with_tool_choice(tool_choice.to_genai());
        }
        if !self.extra_headers.is_empty() {
            let headers = ::genai::Headers::from(
                self.extra_headers
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect::<Vec<_>>(),
            );
            options = options.with_extra_headers(headers);
        }
        if let Some(extra_body) = &self.extra_body {
            options = options.with_extra_body(extra_body.clone());
        }
        if let Some(capture_usage) = self.capture_usage {
            options = options.with_capture_usage(capture_usage);
        }
        if let Some(capture_raw_body) = self.capture_raw_body {
            options = options.with_capture_raw_body(capture_raw_body);
        }
        if let Some(normalize_reasoning_content) = self.normalize_reasoning_content {
            options = options.with_normalize_reasoning_content(normalize_reasoning_content);
        }
        options
    }
}

#[derive(Debug, Clone, Default)]
pub struct GenaiTextEmbeddingSettings {
    pub dimensions: Option<usize>,
    pub encoding_format: Option<String>,
    pub user: Option<String>,
    pub embedding_type: Option<String>,
    pub truncate: Option<String>,
    pub capture_usage: Option<bool>,
    pub capture_raw_body: Option<bool>,
}

impl GenaiTextEmbeddingSettings {
    pub fn from_resolved_settings(settings: &BTreeMap<String, String>) -> Result<Self> {
        Ok(Self {
            dimensions: optional_usize(settings, "dimensions")?,
            encoding_format: settings.get("encoding_format").cloned(),
            user: settings.get("user").cloned(),
            embedding_type: settings.get("embedding_type").cloned(),
            truncate: settings.get("truncate").cloned(),
            capture_usage: optional_bool(settings, "capture_usage")?,
            capture_raw_body: optional_bool(settings, "capture_raw_body")?,
        })
    }

    fn effective_dimension(&self, dimension: usize) -> Result<usize> {
        match self.dimensions {
            Some(settings_dimension) if settings_dimension != dimension => Err(LoomError::new(
                Code::InvalidArgument,
                format!(
                    "text embedding dimensions setting {settings_dimension} does not match model dimension {dimension}"
                ),
            )),
            Some(settings_dimension) => Ok(settings_dimension),
            None => Ok(dimension),
        }
    }

    fn to_embed_options(&self, dimension: usize) -> ::genai::embed::EmbedOptions {
        let mut options = ::genai::embed::EmbedOptions::new().with_dimensions(dimension);
        if let Some(encoding_format) = &self.encoding_format {
            options = options.with_encoding_format(encoding_format.clone());
        }
        if let Some(user) = &self.user {
            options = options.with_user(user.clone());
        }
        if let Some(embedding_type) = &self.embedding_type {
            options = options.with_embedding_type(embedding_type.clone());
        }
        if let Some(truncate) = &self.truncate {
            options = options.with_truncate(truncate.clone());
        }
        options = options.with_capture_usage(self.capture_usage.unwrap_or(true));
        if let Some(capture_raw_body) = self.capture_raw_body {
            options = options.with_capture_raw_body(capture_raw_body);
        }
        options
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum GenaiResponseFormat {
    JsonMode,
    JsonSpec {
        name: String,
        description: Option<String>,
        schema: serde_json::Value,
    },
}

impl GenaiResponseFormat {
    fn to_genai(&self) -> ::genai::chat::ChatResponseFormat {
        match self {
            Self::JsonMode => ::genai::chat::ChatResponseFormat::JsonMode,
            Self::JsonSpec {
                name,
                description,
                schema,
            } => {
                let mut spec = ::genai::chat::JsonSpec::new(name.clone(), schema.clone());
                if let Some(description) = description {
                    spec = spec.with_description(description.clone());
                }
                ::genai::chat::ChatResponseFormat::JsonSpec(spec)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GenaiToolChoice {
    Auto,
    None,
    Required,
    Tool(String),
}

impl GenaiToolChoice {
    fn to_genai(&self) -> ::genai::chat::ToolChoice {
        match self {
            Self::Auto => ::genai::chat::ToolChoice::Auto,
            Self::None => ::genai::chat::ToolChoice::None,
            Self::Required => ::genai::chat::ToolChoice::Required,
            Self::Tool(name) => ::genai::chat::ToolChoice::tool(name.clone()),
        }
    }
}

impl std::str::FromStr for GenaiToolChoice {
    type Err = LoomError;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "auto" => Ok(Self::Auto),
            "none" => Ok(Self::None),
            "required" => Ok(Self::Required),
            value if value.starts_with("tool:") => {
                let name = value.trim_start_matches("tool:");
                if name.is_empty() {
                    Err(invalid_setting("tool_choice", value))
                } else {
                    Ok(Self::Tool(name.to_string()))
                }
            }
            _ => Err(invalid_setting("tool_choice", value)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenaiReasoningEffort {
    None,
    Minimal,
    Low,
    Medium,
    High,
    XHigh,
    Max,
    Budget(u32),
}

impl std::str::FromStr for GenaiReasoningEffort {
    type Err = LoomError;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "none" => Ok(Self::None),
            "minimal" => Ok(Self::Minimal),
            "low" => Ok(Self::Low),
            "medium" => Ok(Self::Medium),
            "high" => Ok(Self::High),
            "xhigh" => Ok(Self::XHigh),
            "max" => Ok(Self::Max),
            _ => value
                .parse::<u32>()
                .map(Self::Budget)
                .map_err(|_| invalid_setting("reasoning_effort", value)),
        }
    }
}

impl From<GenaiReasoningEffort> for ::genai::chat::ReasoningEffort {
    fn from(value: GenaiReasoningEffort) -> Self {
        match value {
            GenaiReasoningEffort::None => Self::None,
            GenaiReasoningEffort::Minimal => Self::Minimal,
            GenaiReasoningEffort::Low => Self::Low,
            GenaiReasoningEffort::Medium => Self::Medium,
            GenaiReasoningEffort::High => Self::High,
            GenaiReasoningEffort::XHigh => Self::XHigh,
            GenaiReasoningEffort::Max => Self::Max,
            GenaiReasoningEffort::Budget(value) => Self::Budget(value),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenaiVerbosity {
    Low,
    Medium,
    High,
}

impl std::str::FromStr for GenaiVerbosity {
    type Err = LoomError;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "low" => Ok(Self::Low),
            "medium" => Ok(Self::Medium),
            "high" => Ok(Self::High),
            _ => Err(invalid_setting("verbosity", value)),
        }
    }
}

impl From<GenaiVerbosity> for ::genai::chat::Verbosity {
    fn from(value: GenaiVerbosity) -> Self {
        match value {
            GenaiVerbosity::Low => Self::Low,
            GenaiVerbosity::Medium => Self::Medium,
            GenaiVerbosity::High => Self::High,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenaiServiceTier {
    Flex,
    Auto,
    Default,
}

impl std::str::FromStr for GenaiServiceTier {
    type Err = LoomError;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "flex" => Ok(Self::Flex),
            "auto" => Ok(Self::Auto),
            "default" => Ok(Self::Default),
            _ => Err(invalid_setting("service_tier", value)),
        }
    }
}

impl From<GenaiServiceTier> for ::genai::chat::ServiceTier {
    fn from(value: GenaiServiceTier) -> Self {
        match value {
            GenaiServiceTier::Flex => Self::Flex,
            GenaiServiceTier::Auto => Self::Auto,
            GenaiServiceTier::Default => Self::Default,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenaiCacheControl {
    Ephemeral,
    Memory,
    Ephemeral5m,
    Ephemeral1h,
    Ephemeral24h,
}

impl std::str::FromStr for GenaiCacheControl {
    type Err = LoomError;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "ephemeral" => Ok(Self::Ephemeral),
            "memory" => Ok(Self::Memory),
            "ephemeral-5m" => Ok(Self::Ephemeral5m),
            "ephemeral-1h" => Ok(Self::Ephemeral1h),
            "ephemeral-24h" => Ok(Self::Ephemeral24h),
            _ => Err(invalid_setting("cache_control", value)),
        }
    }
}

impl From<GenaiCacheControl> for ::genai::chat::CacheControl {
    fn from(value: GenaiCacheControl) -> Self {
        match value {
            GenaiCacheControl::Ephemeral => Self::Ephemeral,
            GenaiCacheControl::Memory => Self::Memory,
            GenaiCacheControl::Ephemeral5m => Self::Ephemeral5m,
            GenaiCacheControl::Ephemeral1h => Self::Ephemeral1h,
            GenaiCacheControl::Ephemeral24h => Self::Ephemeral24h,
        }
    }
}

fn validate_embedding_vectors(
    input_count: usize,
    dimension: usize,
    vectors: Vec<Vec<f32>>,
) -> Result<Vec<Vec<f32>>> {
    if vectors.len() != input_count {
        return Err(LoomError::corrupt(format!(
            "hosted embedding provider returned {} vectors for {} inputs",
            vectors.len(),
            input_count
        )));
    }
    for vector in &vectors {
        if vector.len() != dimension {
            return Err(LoomError::corrupt(format!(
                "hosted embedding provider returned dimension {}, expected {}",
                vector.len(),
                dimension
            )));
        }
    }
    Ok(vectors)
}

fn build_runtime() -> Result<Runtime> {
    Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| LoomError::new(Code::Internal, format!("create tokio runtime: {error}")))
}

fn genai_error(error: ::genai::Error) -> LoomError {
    LoomError::new(Code::Io, format!("genai provider error: {error}"))
}

fn optional_parse<T>(settings: &BTreeMap<String, String>, key: &'static str) -> Result<Option<T>>
where
    T: std::str::FromStr<Err = LoomError>,
{
    settings.get(key).map(|value| value.parse()).transpose()
}

fn optional_bool(settings: &BTreeMap<String, String>, key: &'static str) -> Result<Option<bool>> {
    settings
        .get(key)
        .map(|value| match value.as_str() {
            "true" => Ok(true),
            "false" => Ok(false),
            _ => Err(invalid_setting(key, value)),
        })
        .transpose()
}

fn optional_u32(settings: &BTreeMap<String, String>, key: &'static str) -> Result<Option<u32>> {
    settings
        .get(key)
        .map(|value| value.parse().map_err(|_| invalid_setting(key, value)))
        .transpose()
}

fn optional_u64(settings: &BTreeMap<String, String>, key: &'static str) -> Result<Option<u64>> {
    settings
        .get(key)
        .map(|value| value.parse().map_err(|_| invalid_setting(key, value)))
        .transpose()
}

fn optional_usize(settings: &BTreeMap<String, String>, key: &'static str) -> Result<Option<usize>> {
    settings
        .get(key)
        .map(|value| value.parse().map_err(|_| invalid_setting(key, value)))
        .transpose()
}

fn optional_f64(settings: &BTreeMap<String, String>, key: &'static str) -> Result<Option<f64>> {
    settings
        .get(key)
        .map(|value| value.parse().map_err(|_| invalid_setting(key, value)))
        .transpose()
}

fn optional_string_list(
    settings: &BTreeMap<String, String>,
    key: &'static str,
) -> Result<Vec<String>> {
    let Some(value) = settings.get(key) else {
        return Ok(Vec::new());
    };
    if value.trim_start().starts_with('[') {
        return serde_json::from_str::<Vec<String>>(value).map_err(|_| invalid_setting(key, value));
    }
    Ok(value
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect())
}

fn optional_json_value(
    settings: &BTreeMap<String, String>,
    key: &'static str,
) -> Result<Option<serde_json::Value>> {
    settings
        .get(key)
        .map(|value| serde_json::from_str(value).map_err(|_| invalid_setting(key, value)))
        .transpose()
}

fn prefixed_settings(
    settings: &BTreeMap<String, String>,
    prefix: &str,
) -> BTreeMap<String, String> {
    settings
        .iter()
        .filter_map(|(key, value)| {
            let header = key.strip_prefix(prefix)?;
            if header.is_empty() {
                None
            } else {
                Some((header.to_string(), value.clone()))
            }
        })
        .collect()
}

fn response_format_from_settings(
    settings: &BTreeMap<String, String>,
) -> Result<Option<GenaiResponseFormat>> {
    if let Some(schema_value) = settings.get("response_json_schema") {
        let schema = serde_json::from_str(schema_value)
            .map_err(|_| invalid_setting("response_json_schema", schema_value))?;
        return Ok(Some(GenaiResponseFormat::JsonSpec {
            name: settings
                .get("response_json_schema_name")
                .cloned()
                .unwrap_or_else(|| "loom_response".to_string()),
            description: settings.get("response_json_schema_description").cloned(),
            schema,
        }));
    }
    match settings.get("response_format").map(String::as_str) {
        Some("json") | Some("json-mode") => Ok(Some(GenaiResponseFormat::JsonMode)),
        Some("text") | None => Ok(None),
        Some(value) => Err(invalid_setting("response_format", value)),
    }
}

fn invalid_setting(key: &'static str, value: &str) -> LoomError {
    LoomError::new(
        Code::InvalidArgument,
        format!("invalid genai setting {key}={value:?}"),
    )
}

fn chat_request_from_llm_request(request: &LlmRequest) -> ::genai::chat::ChatRequest {
    let mut chat_request = ::genai::chat::ChatRequest::default();
    if let Some(system_prompt) = &request.system_prompt {
        chat_request = chat_request.with_system(system_prompt.clone());
    }
    chat_request.append_messages(request.messages.iter().map(chat_message))
}

fn chat_message(message: &Message) -> ::genai::chat::ChatMessage {
    match message.role {
        Role::User => ::genai::chat::ChatMessage::user(message.content.clone()),
        Role::Assistant => ::genai::chat::ChatMessage::assistant(message.content.clone()),
    }
}

#[cfg(test)]
fn chat_options_from_llm_request(request: &LlmRequest) -> ::genai::chat::ChatOptions {
    GenaiLlmSettings::default().to_chat_options(request)
}

#[cfg(test)]
fn embed_options_for_dimension(dimension: usize) -> ::genai::embed::EmbedOptions {
    GenaiTextEmbeddingSettings::default().to_embed_options(dimension)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Default)]
    struct FakeBackend {
        vectors: Vec<Vec<f32>>,
        expected_chat: Option<ExpectedChatOptions>,
        expected_embed: Option<ExpectedEmbedOptions>,
    }

    #[derive(Debug, Default)]
    struct ExpectedChatOptions {
        max_tokens: Option<u32>,
        temperature: Option<f64>,
        top_p: Option<f64>,
        stop_sequences: Vec<String>,
        previous_response_id: Option<&'static str>,
        store: Option<bool>,
        reasoning_effort: Option<&'static str>,
        verbosity: Option<&'static str>,
        seed: Option<u64>,
        service_tier: Option<&'static str>,
        prompt_cache_key: Option<&'static str>,
        response_json_mode: bool,
        tool_choice: Option<&'static str>,
        extra_headers: Vec<(&'static str, &'static str)>,
        extra_body: Option<serde_json::Value>,
        capture_usage: Option<bool>,
        capture_raw_body: Option<bool>,
        normalize_reasoning_content: Option<bool>,
    }

    #[derive(Debug)]
    struct ExpectedEmbedOptions {
        dimensions: Option<usize>,
        encoding_format: Option<&'static str>,
        user: Option<&'static str>,
        embedding_type: Option<&'static str>,
        truncate: Option<&'static str>,
        capture_usage: Option<bool>,
        capture_raw_body: Option<bool>,
    }

    fn assert_optional_f64_eq(actual: Option<f64>, expected: Option<f64>) {
        match (actual, expected) {
            (Some(actual), Some(expected)) => assert!((actual - expected).abs() < 0.000001),
            _ => assert_eq!(actual, expected),
        }
    }

    impl GenaiBackend for FakeBackend {
        fn complete(
            &self,
            model_id: &str,
            chat_request: ::genai::chat::ChatRequest,
            options: &::genai::chat::ChatOptions,
        ) -> Result<LlmResponse> {
            if let Some(expected) = &self.expected_chat {
                assert_eq!(options.max_tokens, expected.max_tokens);
                assert_optional_f64_eq(options.temperature, expected.temperature);
                assert_eq!(options.top_p, expected.top_p);
                assert_eq!(options.stop_sequences, expected.stop_sequences);
                assert_eq!(
                    chat_request.previous_response_id.as_deref(),
                    expected.previous_response_id
                );
                assert_eq!(chat_request.store, expected.store);
                assert_eq!(
                    options.reasoning_effort.as_ref().map(ToString::to_string),
                    expected.reasoning_effort.map(str::to_string)
                );
                assert_eq!(
                    options.verbosity.as_ref().map(ToString::to_string),
                    expected.verbosity.map(str::to_string)
                );
                assert_eq!(options.seed, expected.seed);
                assert_eq!(
                    options.service_tier.as_ref().map(ToString::to_string),
                    expected.service_tier.map(str::to_string)
                );
                assert_eq!(
                    options.prompt_cache_key.as_deref(),
                    expected.prompt_cache_key
                );
                assert_eq!(
                    matches!(
                        options.response_format,
                        Some(::genai::chat::ChatResponseFormat::JsonMode)
                    ),
                    expected.response_json_mode
                );
                assert_eq!(
                    tool_choice_name(options.tool_choice.as_ref()),
                    expected.tool_choice
                );
                assert_eq!(
                    headers_to_pairs(options.extra_headers.as_ref()),
                    expected.extra_headers
                );
                assert_eq!(options.extra_body, expected.extra_body);
                assert_eq!(options.capture_usage, expected.capture_usage);
                assert_eq!(options.capture_raw_body, expected.capture_raw_body);
                assert_eq!(
                    options.normalize_reasoning_content,
                    expected.normalize_reasoning_content
                );
            }
            let last_user = chat_request
                .messages
                .iter()
                .rev()
                .find(|message| message.role == ::genai::chat::ChatRole::User)
                .and_then(|message| message.content.first_text())
                .unwrap_or("");
            Ok(LlmResponse {
                model: model_id.to_string(),
                content: format!("hosted:{last_user}"),
                stop_reason: Some("stop".to_string()),
            })
        }

        fn embed(
            &self,
            _model_id: &str,
            _texts: &[String],
            _dimension: usize,
            options: &::genai::embed::EmbedOptions,
        ) -> Result<Vec<Vec<f32>>> {
            if let Some(expected) = &self.expected_embed {
                assert_eq!(options.dimensions, expected.dimensions);
                assert_eq!(options.encoding_format.as_deref(), expected.encoding_format);
                assert_eq!(options.user.as_deref(), expected.user);
                assert_eq!(options.embedding_type.as_deref(), expected.embedding_type);
                assert_eq!(options.truncate.as_deref(), expected.truncate);
                assert_eq!(options.capture_usage, expected.capture_usage);
                assert_eq!(options.capture_raw_body, expected.capture_raw_body);
            }
            Ok(self.vectors.clone())
        }
    }

    fn tool_choice_name(choice: Option<&::genai::chat::ToolChoice>) -> Option<&str> {
        match choice {
            Some(::genai::chat::ToolChoice::Auto) => Some("auto"),
            Some(::genai::chat::ToolChoice::None) => Some("none"),
            Some(::genai::chat::ToolChoice::Required) => Some("required"),
            Some(::genai::chat::ToolChoice::Tool { name }) => Some(name.as_str()),
            None => None,
        }
    }

    fn headers_to_pairs(headers: Option<&::genai::Headers>) -> Vec<(&str, &str)> {
        let mut pairs = headers
            .into_iter()
            .flat_map(|headers| headers.iter())
            .map(|(key, value)| (key.as_str(), value.as_str()))
            .collect::<Vec<_>>();
        pairs.sort_unstable();
        pairs
    }

    #[test]
    fn maps_llm_request_to_genai_chat_request_and_options() {
        let request = LlmRequest {
            messages: vec![
                Message::user("hello"),
                Message::assistant("hi"),
                Message::user("again"),
            ],
            system_prompt: Some("answer tersely".to_string()),
            max_tokens: Some(12),
            temperature: Some(0.3),
            ..LlmRequest::default()
        };

        let chat_request = chat_request_from_llm_request(&request);
        let options = chat_options_from_llm_request(&request);

        assert_eq!(chat_request.system.as_deref(), Some("answer tersely"));
        assert_eq!(chat_request.messages.len(), 3);
        assert_eq!(options.max_tokens, Some(12));
        assert!((options.temperature.unwrap() - 0.3).abs() < 0.000001);
    }

    #[test]
    fn embedding_options_preserve_dimension() {
        let options = embed_options_for_dimension(384);

        assert_eq!(options.dimensions, Some(384));
        assert_eq!(options.capture_usage, Some(true));
    }

    #[test]
    fn llm_settings_parse_and_map_to_genai_options() {
        let settings = GenaiLlmSettings::from_resolved_settings(&BTreeMap::from([
            ("max_tokens".to_string(), "99".to_string()),
            ("temperature".to_string(), "0.7".to_string()),
            ("top_p".to_string(), "0.8".to_string()),
            ("stop".to_string(), r#"["END","STOP"]"#.to_string()),
            ("reasoning_effort".to_string(), "64".to_string()),
            ("verbosity".to_string(), "low".to_string()),
            ("seed".to_string(), "42".to_string()),
            ("service_tier".to_string(), "flex".to_string()),
            ("cache_control".to_string(), "ephemeral-1h".to_string()),
            ("prompt_cache_key".to_string(), "tenant-a".to_string()),
            ("response_format".to_string(), "json-mode".to_string()),
            ("tool_choice".to_string(), "tool:search".to_string()),
            ("extra.header.x-tenant".to_string(), "tenant-a".to_string()),
            (
                "extra_body".to_string(),
                r#"{"metadata":{"tenant":"a"}}"#.to_string(),
            ),
            ("previous_response_id".to_string(), "resp-1".to_string()),
            ("store".to_string(), "true".to_string()),
            ("capture_usage".to_string(), "true".to_string()),
            ("capture_raw_body".to_string(), "false".to_string()),
            (
                "normalize_reasoning_content".to_string(),
                "true".to_string(),
            ),
        ]))
        .unwrap();

        let options = settings.to_chat_options(&LlmRequest {
            max_tokens: Some(7),
            temperature: Some(0.2),
            ..LlmRequest::default()
        });

        assert_eq!(options.max_tokens, Some(7));
        assert_optional_f64_eq(options.temperature, Some(0.2));
        assert_eq!(options.top_p, Some(0.8));
        assert_eq!(options.stop_sequences, vec!["END", "STOP"]);
        assert_eq!(
            options.reasoning_effort.as_ref().map(ToString::to_string),
            Some("64".to_string())
        );
        assert_eq!(
            options.verbosity.as_ref().map(ToString::to_string),
            Some("low".to_string())
        );
        assert_eq!(options.seed, Some(42));
        assert_eq!(
            options.service_tier.as_ref().map(ToString::to_string),
            Some("flex".to_string())
        );
        assert_eq!(options.prompt_cache_key.as_deref(), Some("tenant-a"));
        assert!(matches!(
            options.response_format,
            Some(::genai::chat::ChatResponseFormat::JsonMode)
        ));
        assert_eq!(
            tool_choice_name(options.tool_choice.as_ref()),
            Some("search")
        );
        assert_eq!(
            headers_to_pairs(options.extra_headers.as_ref()),
            vec![("x-tenant", "tenant-a")]
        );
        assert_eq!(
            options.extra_body,
            Some(serde_json::json!({"metadata": {"tenant": "a"}}))
        );
        assert_eq!(options.capture_usage, Some(true));
        assert_eq!(options.capture_raw_body, Some(false));
        assert_eq!(options.normalize_reasoning_content, Some(true));

        let chat_request = settings.to_chat_request(&LlmRequest::default());
        assert_eq!(chat_request.previous_response_id.as_deref(), Some("resp-1"));
        assert_eq!(chat_request.store, Some(true));
    }

    #[test]
    fn llm_settings_parse_structured_response_format() {
        let settings = GenaiLlmSettings::from_resolved_settings(&BTreeMap::from([
            (
                "response_json_schema_name".to_string(),
                "answer".to_string(),
            ),
            (
                "response_json_schema_description".to_string(),
                "structured answer".to_string(),
            ),
            (
                "response_json_schema".to_string(),
                r#"{"type":"object","properties":{"answer":{"type":"string"}}}"#.to_string(),
            ),
        ]))
        .unwrap();

        let options = settings.to_chat_options(&LlmRequest::default());

        let Some(::genai::chat::ChatResponseFormat::JsonSpec(spec)) = options.response_format
        else {
            panic!("expected structured response format");
        };
        assert_eq!(spec.name, "answer");
        assert_eq!(spec.description.as_deref(), Some("structured answer"));
        assert_eq!(
            spec.schema,
            serde_json::json!({"type": "object", "properties": {"answer": {"type": "string"}}})
        );
    }

    #[test]
    fn text_embedding_settings_parse_and_map_to_genai_options() {
        let settings = GenaiTextEmbeddingSettings::from_resolved_settings(&BTreeMap::from([
            ("dimensions".to_string(), "3".to_string()),
            ("encoding_format".to_string(), "float".to_string()),
            ("user".to_string(), "user-1".to_string()),
            ("embedding_type".to_string(), "search_query".to_string()),
            ("truncate".to_string(), "END".to_string()),
            ("capture_usage".to_string(), "false".to_string()),
            ("capture_raw_body".to_string(), "true".to_string()),
        ]))
        .unwrap();

        let options = settings.to_embed_options(settings.effective_dimension(3).unwrap());

        assert_eq!(options.dimensions, Some(3));
        assert_eq!(options.encoding_format.as_deref(), Some("float"));
        assert_eq!(options.user.as_deref(), Some("user-1"));
        assert_eq!(options.embedding_type.as_deref(), Some("search_query"));
        assert_eq!(options.truncate.as_deref(), Some("END"));
        assert_eq!(options.capture_usage, Some(false));
        assert_eq!(options.capture_raw_body, Some(true));
    }

    #[test]
    fn llm_provider_completes_through_backend() {
        let llm = GenaiLlm::with_backend(
            "openai::gpt-test",
            Arc::new(FakeBackend {
                vectors: Vec::new(),
                ..FakeBackend::default()
            }),
        );

        let response = llm
            .complete(&LlmRequest {
                messages: vec![Message::user("ping")],
                ..LlmRequest::default()
            })
            .unwrap();

        assert_eq!(llm.model_id(), "openai::gpt-test");
        assert_eq!(response.model, "openai::gpt-test");
        assert_eq!(response.content, "hosted:ping");
        assert_eq!(response.stop_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn llm_provider_projects_settings_to_backend_options() {
        let llm = GenaiLlm::with_backend_and_settings(
            "openai::gpt-test",
            GenaiLlmSettings {
                max_tokens: Some(99),
                temperature: Some(0.7),
                top_p: Some(0.8),
                stop_sequences: vec!["END".to_string()],
                reasoning_effort: Some(GenaiReasoningEffort::High),
                verbosity: Some(GenaiVerbosity::Medium),
                seed: Some(9),
                service_tier: Some(GenaiServiceTier::Auto),
                prompt_cache_key: Some("tenant-a".to_string()),
                capture_usage: Some(true),
                capture_raw_body: Some(false),
                normalize_reasoning_content: Some(true),
                cache_control: None,
                response_format: Some(GenaiResponseFormat::JsonMode),
                tool_choice: Some(GenaiToolChoice::Required),
                extra_headers: BTreeMap::from([("x-tenant".to_string(), "tenant-a".to_string())]),
                extra_body: Some(serde_json::json!({"metadata": {"tenant": "a"}})),
                previous_response_id: Some("resp-1".to_string()),
                store: Some(true),
            },
            Arc::new(FakeBackend {
                expected_chat: Some(ExpectedChatOptions {
                    max_tokens: Some(11),
                    temperature: Some(0.1),
                    top_p: Some(0.8),
                    stop_sequences: vec!["END".to_string()],
                    reasoning_effort: Some("high"),
                    verbosity: Some("medium"),
                    seed: Some(9),
                    service_tier: Some("auto"),
                    prompt_cache_key: Some("tenant-a"),
                    previous_response_id: Some("resp-1"),
                    store: Some(true),
                    response_json_mode: true,
                    tool_choice: Some("required"),
                    extra_headers: vec![("x-tenant", "tenant-a")],
                    extra_body: Some(serde_json::json!({"metadata": {"tenant": "a"}})),
                    capture_usage: Some(true),
                    capture_raw_body: Some(false),
                    normalize_reasoning_content: Some(true),
                }),
                ..FakeBackend::default()
            }),
        );

        let response = llm
            .complete(&LlmRequest {
                messages: vec![Message::user("ping")],
                max_tokens: Some(11),
                temperature: Some(0.1),
                ..LlmRequest::default()
            })
            .unwrap();

        assert_eq!(response.content, "hosted:ping");
    }

    #[test]
    fn text_embedding_provider_embeds_through_backend() {
        let embeddings = GenaiTextEmbedding::with_backend(
            "text-embedding-test",
            3,
            Arc::new(FakeBackend {
                vectors: vec![vec![1.0, 0.0, 0.0], vec![0.0, 1.0, 0.0]],
                ..FakeBackend::default()
            }),
        );

        let vectors = embeddings
            .embed(&["alpha".to_string(), "beta".to_string()])
            .unwrap();

        assert_eq!(embeddings.model_id(), "text-embedding-test");
        assert_eq!(embeddings.dimension(), 3);
        assert_eq!(vectors.len(), 2);
    }

    #[test]
    fn text_embedding_provider_projects_settings_to_backend_options() {
        let embeddings = GenaiTextEmbedding::with_backend_and_settings(
            "text-embedding-test",
            3,
            GenaiTextEmbeddingSettings {
                dimensions: Some(3),
                encoding_format: Some("float".to_string()),
                user: Some("user-1".to_string()),
                embedding_type: Some("search_document".to_string()),
                truncate: Some("END".to_string()),
                capture_usage: Some(false),
                capture_raw_body: Some(true),
            },
            Arc::new(FakeBackend {
                vectors: vec![vec![1.0, 0.0, 0.0]],
                expected_embed: Some(ExpectedEmbedOptions {
                    dimensions: Some(3),
                    encoding_format: Some("float"),
                    user: Some("user-1"),
                    embedding_type: Some("search_document"),
                    truncate: Some("END"),
                    capture_usage: Some(false),
                    capture_raw_body: Some(true),
                }),
                ..FakeBackend::default()
            }),
        )
        .unwrap();

        let vectors = embeddings.embed(&["alpha".to_string()]).unwrap();

        assert_eq!(vectors, vec![vec![1.0, 0.0, 0.0]]);
    }

    #[test]
    fn text_embedding_provider_rejects_dimension_mismatch() {
        let embeddings = GenaiTextEmbedding::with_backend(
            "text-embedding-test",
            3,
            Arc::new(FakeBackend {
                vectors: vec![vec![1.0, 0.0]],
                ..FakeBackend::default()
            }),
        );

        let err = embeddings.embed(&["alpha".to_string()]).unwrap_err();

        assert_eq!(err.code, Code::CorruptObject);
        assert!(err.message.contains("expected 3"));
    }
}
