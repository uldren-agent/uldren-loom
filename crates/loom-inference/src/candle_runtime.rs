//! Candle-backed local inference providers.

use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::bert::{BertModel, Config as BertConfig};
use candle_transformers::models::qwen2::{Config as Qwen2Config, ModelForCausalLM};
use loom_types::{Code, LoomError, Result};
use tokenizers::Tokenizer;

use crate::activation::{first_weight_digest, model_id};
use crate::inventory::InstalledModelRecord;
use crate::{
    Llm, LlmHandle, LlmRequest, LlmResponse, Role, TextEmbedding, TextEmbeddingHandle,
    TextEmbeddingModel,
};

fn candle_device() -> Result<Device> {
    #[cfg(feature = "candle-cuda")]
    {
        Device::cuda_if_available(0).map_err(candle_error)
    }
    #[cfg(not(feature = "candle-cuda"))]
    {
        Ok(Device::Cpu)
    }
}

pub fn activate_candle_text_embedding(
    record: &InstalledModelRecord,
    cache_dir: &Path,
    dimension: usize,
) -> Result<TextEmbeddingHandle> {
    let provider = CandleBertTextEmbedding::load(record, cache_dir, dimension)?;
    Ok(TextEmbeddingHandle::with_provider(Box::new(provider)))
}

pub fn activate_candle_qwen2_llm(
    record: &InstalledModelRecord,
    cache_dir: &Path,
) -> Result<LlmHandle> {
    let provider = CandleQwen2Llm::load(record, cache_dir)?;
    Ok(LlmHandle::with_provider(Box::new(provider)))
}

struct CandleBertTextEmbedding {
    model: TextEmbeddingModel,
    tokenizer: Tokenizer,
    bert: BertModel,
    device: Device,
}

impl CandleBertTextEmbedding {
    fn load(record: &InstalledModelRecord, cache_dir: &Path, dimension: usize) -> Result<Self> {
        let config_path = required_file(record, cache_dir, |path| path.ends_with("config.json"))?;
        let tokenizer_path =
            required_file(record, cache_dir, |path| path.ends_with("tokenizer.json"))?;
        let weights_path = required_file(record, cache_dir, |path| path.ends_with(".safetensors"))?;

        let config = read_bert_config(&config_path)?;
        if config.hidden_size != dimension {
            return Err(LoomError::unsupported(format!(
                "model hidden size {} does not match curated embedding dimension {dimension}",
                config.hidden_size
            )));
        }

        let tokenizer = Tokenizer::from_file(&tokenizer_path)
            .map_err(|e| LoomError::corrupt(format!("invalid tokenizer.json: {e}")))?;
        let weights = fs::read(&weights_path)?;
        let device = candle_device()?;
        let vb = VarBuilder::from_buffered_safetensors(weights, DType::F32, &device)
            .map_err(candle_error)?;
        let bert = BertModel::load(vb, &config).map_err(candle_error)?;

        Ok(Self {
            model: TextEmbeddingModel::new(
                model_id(record),
                dimension,
                first_weight_digest(record),
            ),
            tokenizer,
            bert,
            device,
        })
    }

    fn embed_one(&self, text: &str) -> Result<Vec<f32>> {
        let encoding = self
            .tokenizer
            .encode(text, true)
            .map_err(|e| LoomError::invalid(format!("failed to tokenize text: {e}")))?;
        let token_ids = Tensor::new(encoding.get_ids(), &self.device)
            .and_then(|tensor| tensor.unsqueeze(0))
            .map_err(candle_error)?;
        let token_type_ids = Tensor::new(encoding.get_type_ids(), &self.device)
            .and_then(|tensor| tensor.unsqueeze(0))
            .map_err(candle_error)?;
        let attention_mask = Tensor::new(encoding.get_attention_mask(), &self.device)
            .and_then(|tensor| tensor.unsqueeze(0))
            .map_err(candle_error)?;
        let hidden_states = self
            .bert
            .forward(&token_ids, &token_type_ids, Some(&attention_mask))
            .map_err(candle_error)?;
        mean_pool(&hidden_states, &attention_mask, self.model.dimension)
    }
}

impl TextEmbedding for CandleBertTextEmbedding {
    fn model_id(&self) -> &str {
        &self.model.model_id
    }

    fn dimension(&self) -> usize {
        self.model.dimension
    }

    fn weights_digest(&self) -> Option<&str> {
        self.model.weights_digest.as_deref()
    }

    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        texts.iter().map(|text| self.embed_one(text)).collect()
    }
}

struct CandleQwen2Llm {
    model_id: String,
    tokenizer: Tokenizer,
    model: Mutex<ModelForCausalLM>,
    device: Device,
    eos_token_ids: Vec<u32>,
}

impl CandleQwen2Llm {
    fn load(record: &InstalledModelRecord, cache_dir: &Path) -> Result<Self> {
        let config_path = required_file(record, cache_dir, |path| path.ends_with("config.json"))?;
        let tokenizer_path =
            required_file(record, cache_dir, |path| path.ends_with("tokenizer.json"))?;
        let weights_path = required_file(record, cache_dir, |path| path.ends_with(".safetensors"))?;

        let config = read_qwen2_config(&config_path)?;
        let tokenizer = Tokenizer::from_file(&tokenizer_path)
            .map_err(|e| LoomError::corrupt(format!("invalid tokenizer.json: {e}")))?;
        let eos_token_ids = qwen_eos_token_ids(&tokenizer);
        let weights = fs::read(&weights_path)?;
        let device = candle_device()?;
        let vb = VarBuilder::from_buffered_safetensors(weights, DType::F32, &device)
            .map_err(candle_error)?;
        let model = ModelForCausalLM::new(&config, vb).map_err(candle_error)?;

        Ok(Self {
            model_id: model_id(record),
            tokenizer,
            model: Mutex::new(model),
            device,
            eos_token_ids,
        })
    }

    fn generate(&self, request: &LlmRequest) -> Result<String> {
        let prompt = qwen_chat_prompt(request);
        let encoding = self
            .tokenizer
            .encode(prompt.as_str(), true)
            .map_err(|e| LoomError::invalid(format!("failed to tokenize prompt: {e}")))?;
        let mut tokens = encoding.get_ids().to_vec();
        if tokens.is_empty() {
            return Err(LoomError::invalid("llm prompt produced no tokens"));
        }

        let max_tokens = request.max_tokens.unwrap_or(64).clamp(1, 512);
        let mut generated = Vec::new();
        let mut model = self
            .model
            .lock()
            .map_err(|_| LoomError::new(Code::Internal, "llm model lock is poisoned"))?;
        model.clear_kv_cache();
        let mut seqlen_offset = 0_usize;

        for step in 0..max_tokens {
            let input_tokens = if step == 0 {
                tokens.as_slice()
            } else {
                &tokens[tokens.len() - 1..]
            };
            let input = Tensor::new(input_tokens, &self.device)
                .and_then(|tensor| tensor.reshape((1, input_tokens.len())))
                .map_err(candle_error)?;
            let logits = model
                .forward(&input, seqlen_offset)
                .and_then(|tensor| tensor.squeeze(0))
                .and_then(|tensor| tensor.squeeze(0))
                .map_err(candle_error)?;
            seqlen_offset = seqlen_offset.saturating_add(input_tokens.len());
            let next = logits
                .argmax(0)
                .and_then(|tensor| tensor.to_scalar::<u32>())
                .map_err(candle_error)?;
            if self.eos_token_ids.contains(&next) {
                break;
            }
            generated.push(next);
            tokens.push(next);
        }
        model.clear_kv_cache();

        self.tokenizer
            .decode(&generated, true)
            .map_err(|e| LoomError::corrupt(format!("failed to decode generated tokens: {e}")))
    }
}

impl Llm for CandleQwen2Llm {
    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn complete(&self, request: &LlmRequest) -> Result<LlmResponse> {
        Ok(LlmResponse {
            model: self.model_id.clone(),
            content: self.generate(request)?,
            stop_reason: Some("max_tokens_or_eos".to_string()),
        })
    }
}

fn read_bert_config(path: &Path) -> Result<BertConfig> {
    let bytes = fs::read(path)?;
    serde_json::from_slice(&bytes)
        .map_err(|e| LoomError::corrupt(format!("invalid BERT config.json: {e}")))
}

fn read_qwen2_config(path: &Path) -> Result<Qwen2Config> {
    let bytes = fs::read(path)?;
    serde_json::from_slice(&bytes)
        .map_err(|e| LoomError::corrupt(format!("invalid Qwen2 config.json: {e}")))
}

fn qwen_chat_prompt(request: &LlmRequest) -> String {
    let mut prompt = String::new();
    if let Some(system_prompt) = request.system_prompt.as_deref() {
        push_qwen_message(&mut prompt, "system", system_prompt);
    }
    for message in &request.messages {
        let role = match message.role {
            Role::User => "user",
            Role::Assistant => "assistant",
        };
        push_qwen_message(&mut prompt, role, &message.content);
    }
    prompt.push_str("<|im_start|>assistant\n");
    prompt
}

fn push_qwen_message(prompt: &mut String, role: &str, content: &str) {
    prompt.push_str("<|im_start|>");
    prompt.push_str(role);
    prompt.push('\n');
    prompt.push_str(content);
    prompt.push_str("<|im_end|>\n");
}

fn qwen_eos_token_ids(tokenizer: &Tokenizer) -> Vec<u32> {
    ["<|im_end|>", "<|endoftext|>"]
        .into_iter()
        .filter_map(|token| tokenizer.token_to_id(token))
        .collect()
}

fn mean_pool(
    hidden_states: &Tensor,
    attention_mask: &Tensor,
    dimension: usize,
) -> Result<Vec<f32>> {
    let mask = attention_mask
        .to_dtype(DType::F32)
        .and_then(|tensor| tensor.unsqueeze(2))
        .map_err(candle_error)?;
    let summed = hidden_states
        .broadcast_mul(&mask)
        .and_then(|tensor| tensor.sum(1))
        .map_err(candle_error)?;
    let counts = mask.sum(1).map_err(candle_error)?;
    let pooled = summed
        .broadcast_div(&counts)
        .and_then(|tensor| tensor.squeeze(0))
        .map_err(candle_error)?;
    let mut vector = pooled.to_vec1::<f32>().map_err(candle_error)?;
    if vector.len() != dimension {
        return Err(LoomError::new(
            Code::Internal,
            format!(
                "candle text embedding returned dimension {}, expected {dimension}",
                vector.len()
            ),
        ));
    }
    normalize(&mut vector);
    Ok(vector)
}

fn normalize(vector: &mut [f32]) {
    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm > 0.0 {
        for value in vector {
            *value /= norm;
        }
    }
}

fn required_file(
    record: &InstalledModelRecord,
    cache_dir: &Path,
    matches: impl Fn(&str) -> bool,
) -> Result<PathBuf> {
    let file = record
        .files
        .iter()
        .find(|file| matches(&file.relative_path))
        .ok_or_else(|| LoomError::unsupported("installed model is missing a required file"))?;
    safe_cache_path(cache_dir, &file.relative_path)
}

fn safe_cache_path(cache_dir: &Path, relative_path: &str) -> Result<PathBuf> {
    let mut has_component = false;
    for component in Path::new(relative_path).components() {
        match component {
            Component::Normal(_) => has_component = true,
            _ => {
                return Err(LoomError::invalid(format!(
                    "installed model file path {relative_path:?} is not a safe relative path"
                )));
            }
        }
    }
    if !has_component {
        return Err(LoomError::invalid(
            "installed model file path is not a safe relative path",
        ));
    }
    Ok(cache_dir.join(relative_path))
}

fn candle_error(error: candle_core::Error) -> LoomError {
    LoomError::new(Code::Internal, format!("candle runtime failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::InstalledModelFile;

    #[test]
    fn rejects_unsafe_relative_path() {
        let record = InstalledModelRecord {
            model: loom_types::ModelRef::new(
                loom_types::InferenceModelKind::TextEmbedding,
                "sentence-transformers/all-MiniLM-L6-v2",
            ),
            runtime: loom_types::RuntimeKind::CandleSafetensors,
            files: vec![InstalledModelFile {
                relative_path: "../model.safetensors".to_string(),
                size_bytes: 0,
                digest: Some("sha256:abc".to_string()),
            }],
            active_provider_refs: Vec::new(),
        };

        let err = required_file(&record, Path::new("/cache"), |path| {
            path.ends_with(".safetensors")
        })
        .unwrap_err();

        assert_eq!(err.code, loom_types::Code::InvalidArgument);
    }
}
