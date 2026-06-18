//! Activation helpers for installed inference inventory records.

use std::path::Path;

use loom_types::{
    HardwareReport, InferenceModelKind, LoomError, ModelFitReason, Result, RuntimeKind,
};
#[cfg(any(test, not(feature = "candle-cpu")))]
use sha2::{Digest, Sha256};

#[cfg(feature = "llama-cpp")]
use crate::GgufAdapter;
use crate::catalog::{CuratedModelSpec, curated_models};
use crate::compat::evaluate_installed_model_fit;
use crate::inventory::InstalledModelRecord;
#[cfg(any(test, not(feature = "candle-cpu")))]
use crate::{Llm, LlmRequest, LlmResponse, TextEmbedding, TextEmbeddingModel};
use crate::{LlmHandle, TextEmbeddingHandle};

pub fn activate_text_embedding(
    record: &InstalledModelRecord,
    hardware: &HardwareReport,
    cache_dir: &Path,
) -> Result<TextEmbeddingHandle> {
    if record.model.kind != InferenceModelKind::TextEmbedding {
        return Err(LoomError::unsupported(format!(
            "installed model {:?} is not a text embedding model",
            record.model.repo_id
        )));
    }
    if record.runtime == RuntimeKind::CoreMl {
        return Err(core_ml_provider_bridge_unavailable(record));
    }
    require_runnable(record, hardware, cache_dir)?;
    if !matches!(record.runtime, RuntimeKind::CandleSafetensors) {
        return Err(LoomError::unsupported(format!(
            "runtime {} cannot activate a text embedding provider",
            record.runtime.as_str()
        )));
    }
    let spec = matching_curated_model(record).ok_or_else(|| {
        LoomError::unsupported(format!(
            "model {:?} is not in the curated text embedding activation catalog",
            record.model.repo_id
        ))
    })?;
    let dimension = spec.embedding_dimension.ok_or_else(|| {
        LoomError::unsupported(format!(
            "model {:?} does not declare an embedding dimension",
            record.model.repo_id
        ))
    })?;
    #[cfg(feature = "candle-cpu")]
    {
        crate::candle_runtime::activate_candle_text_embedding(record, cache_dir, dimension)
    }
    #[cfg(not(feature = "candle-cpu"))]
    {
        activate_text_embedding_smoke_after_checks(record, dimension)
    }
}

#[cfg(any(test, not(feature = "candle-cpu")))]
fn activate_text_embedding_smoke_after_checks(
    record: &InstalledModelRecord,
    dimension: usize,
) -> Result<TextEmbeddingHandle> {
    let weights_digest = first_weight_digest(record);
    Ok(TextEmbeddingHandle::with_provider(Box::new(
        FileBackedTextEmbedding {
            model: TextEmbeddingModel::new(model_id(record), dimension, weights_digest),
        },
    )))
}

#[cfg(test)]
fn activate_text_embedding_smoke(
    record: &InstalledModelRecord,
    hardware: &HardwareReport,
    cache_dir: &Path,
) -> Result<TextEmbeddingHandle> {
    require_runnable(record, hardware, cache_dir)?;
    let spec = matching_curated_model(record).ok_or_else(|| {
        LoomError::unsupported(format!(
            "model {:?} is not in the curated text embedding activation catalog",
            record.model.repo_id
        ))
    })?;
    let dimension = spec.embedding_dimension.ok_or_else(|| {
        LoomError::unsupported(format!(
            "model {:?} does not declare an embedding dimension",
            record.model.repo_id
        ))
    })?;
    activate_text_embedding_smoke_after_checks(record, dimension)
}

pub fn activate_llm(
    record: &InstalledModelRecord,
    hardware: &HardwareReport,
    cache_dir: &Path,
) -> Result<LlmHandle> {
    if record.model.kind != InferenceModelKind::Llm {
        return Err(LoomError::unsupported(format!(
            "installed model {:?} is not an llm model",
            record.model.repo_id
        )));
    }
    if record.runtime == RuntimeKind::CoreMl {
        return Err(core_ml_provider_bridge_unavailable(record));
    }
    require_runnable(record, hardware, cache_dir)?;
    if !matches!(
        record.runtime,
        RuntimeKind::CandleSafetensors | RuntimeKind::CandleGguf | RuntimeKind::LlamaCpp
    ) {
        return Err(LoomError::unsupported(format!(
            "runtime {} cannot activate an llm provider",
            record.runtime.as_str()
        )));
    }
    let spec = matching_curated_model(record).ok_or_else(|| {
        LoomError::unsupported(format!(
            "model {:?} is not in the curated llm activation catalog",
            record.model.repo_id
        ))
    })?;
    if spec.context_window.is_none() {
        return Err(LoomError::unsupported(format!(
            "model {:?} does not declare an llm context window",
            record.model.repo_id
        )));
    }
    if record.runtime == RuntimeKind::LlamaCpp {
        #[cfg(feature = "llama-cpp")]
        {
            let bundle_dir = crate::default_llama_cpp_bundle_dir(hardware.target_triple.as_deref());
            return crate::LlamaCppDynamicAdapter::new(bundle_dir)?.load_llm(record, cache_dir);
        }
        #[cfg(not(feature = "llama-cpp"))]
        {
            return Err(LoomError::unsupported(
                "llama.cpp dynamic loading is not compiled",
            ));
        }
    }
    #[cfg(feature = "candle-cpu")]
    {
        if record.model.repo_id == "Qwen/Qwen2.5-0.5B-Instruct" {
            return crate::candle_runtime::activate_candle_qwen2_llm(record, cache_dir);
        }
        Err(LoomError::unsupported(format!(
            "model {:?} does not have a candle llm activation provider",
            record.model.repo_id
        )))
    }
    #[cfg(not(feature = "candle-cpu"))]
    {
        activate_llm_smoke_after_checks(record)
    }
}

#[cfg(any(test, not(feature = "candle-cpu")))]
fn activate_llm_smoke_after_checks(record: &InstalledModelRecord) -> Result<LlmHandle> {
    Ok(LlmHandle::with_provider(Box::new(FileBackedLlm {
        model_id: model_id(record),
        weights_digest: first_weight_digest(record),
    })))
}

#[cfg(test)]
fn activate_llm_smoke(
    record: &InstalledModelRecord,
    hardware: &HardwareReport,
    cache_dir: &Path,
) -> Result<LlmHandle> {
    require_runnable(record, hardware, cache_dir)?;
    let spec = matching_curated_model(record).ok_or_else(|| {
        LoomError::unsupported(format!(
            "model {:?} is not in the curated llm activation catalog",
            record.model.repo_id
        ))
    })?;
    if spec.context_window.is_none() {
        return Err(LoomError::unsupported(format!(
            "model {:?} does not declare an llm context window",
            record.model.repo_id
        )));
    }
    activate_llm_smoke_after_checks(record)
}

fn require_runnable(
    record: &InstalledModelRecord,
    hardware: &HardwareReport,
    cache_dir: &Path,
) -> Result<()> {
    let report = evaluate_installed_model_fit(record, hardware, Some(cache_dir));
    if report.runnable {
        return Ok(());
    }
    Err(LoomError::unsupported(format!(
        "installed model {:?} is not runnable: {}",
        record.model.repo_id,
        report
            .reasons
            .iter()
            .map(fit_reason_label)
            .collect::<Vec<_>>()
            .join(",")
    )))
}

fn fit_reason_label(reason: &ModelFitReason) -> &'static str {
    match reason {
        ModelFitReason::RuntimeNotCompiled => "runtime-not-compiled",
        ModelFitReason::UnsupportedModelKind => "unsupported-model-kind",
        ModelFitReason::UnsupportedFormat => "unsupported-format",
        ModelFitReason::MissingGpu => "missing-gpu",
        ModelFitReason::InsufficientMemory => "insufficient-memory",
        ModelFitReason::MissingTokenizer => "missing-tokenizer",
        ModelFitReason::MissingConfig => "missing-config",
        ModelFitReason::MissingWeights => "missing-weights",
        ModelFitReason::MissingFile(_) => "missing-file",
        ModelFitReason::MissingDigest(_) => "missing-digest",
        ModelFitReason::UnsupportedDigest(_) => "unsupported-digest",
        ModelFitReason::DigestMismatch(_) => "digest-mismatch",
        ModelFitReason::GatedModel => "gated-model",
        ModelFitReason::GatedStatusUnknown => "gated-status-unknown",
        ModelFitReason::LicenseBlocked => "license-blocked",
        ModelFitReason::LicenseUnknown => "license-unknown",
    }
}

fn core_ml_provider_bridge_unavailable(record: &InstalledModelRecord) -> LoomError {
    LoomError::unsupported(format!(
        "runtime core-ml requires a curated Core ML model-family provider bridge before {:?} can activate",
        record.model.repo_id
    ))
}

fn matching_curated_model(record: &InstalledModelRecord) -> Option<CuratedModelSpec> {
    curated_models().iter().copied().find(|spec| {
        spec.kind == record.model.kind
            && spec.repo_id == record.model.repo_id
            && spec.runtime == record.runtime
            && spec.revision == record.model.revision.value()
    })
}

pub(crate) fn first_weight_digest(record: &InstalledModelRecord) -> Option<String> {
    record
        .files
        .iter()
        .find(|file| {
            file.relative_path.ends_with(".safetensors")
                || file.relative_path.ends_with(".bin")
                || file.relative_path.ends_with(".gguf")
        })
        .and_then(|file| file.digest.clone())
}

pub(crate) fn model_id(record: &InstalledModelRecord) -> String {
    format!(
        "{}@{}#{}",
        record.model.repo_id,
        record.model.revision.value(),
        record.runtime.as_str()
    )
}

#[cfg(any(test, not(feature = "candle-cpu")))]
#[derive(Debug, Clone)]
struct FileBackedTextEmbedding {
    model: TextEmbeddingModel,
}

#[cfg(any(test, not(feature = "candle-cpu")))]
impl TextEmbedding for FileBackedTextEmbedding {
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
        Ok(texts
            .iter()
            .map(|text| {
                deterministic_vector(
                    self.model_id(),
                    self.weights_digest(),
                    text,
                    self.dimension(),
                )
            })
            .collect())
    }
}

#[cfg(any(test, not(feature = "candle-cpu")))]
#[derive(Debug, Clone)]
struct FileBackedLlm {
    model_id: String,
    weights_digest: Option<String>,
}

#[cfg(any(test, not(feature = "candle-cpu")))]
impl Llm for FileBackedLlm {
    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn complete(&self, request: &LlmRequest) -> Result<LlmResponse> {
        let user_text = request
            .messages
            .iter()
            .rev()
            .find(|message| matches!(message.role, crate::Role::User))
            .map(|message| message.content.as_str())
            .unwrap_or("");
        let digest = self.weights_digest.as_deref().unwrap_or("unknown");
        Ok(LlmResponse {
            model: self.model_id.clone(),
            content: format!("local-smoke:{digest}:{user_text}"),
            stop_reason: Some("end_turn".to_string()),
        })
    }
}

#[cfg(any(test, not(feature = "candle-cpu")))]
fn deterministic_vector(
    model_id: &str,
    weights_digest: Option<&str>,
    text: &str,
    dimension: usize,
) -> Vec<f32> {
    let mut vector = Vec::with_capacity(dimension);
    let mut counter = 0_u64;
    while vector.len() < dimension {
        let mut hasher = Sha256::new();
        hasher.update(model_id.as_bytes());
        hasher.update([0]);
        if let Some(weights_digest) = weights_digest {
            hasher.update(weights_digest.as_bytes());
        }
        hasher.update([0]);
        hasher.update(text.as_bytes());
        hasher.update(counter.to_le_bytes());
        let digest = hasher.finalize();
        for chunk in digest.chunks_exact(4) {
            if vector.len() == dimension {
                break;
            }
            let raw = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            let centered = (raw as f32 / u32::MAX as f32) - 0.5;
            vector.push(centered);
        }
        counter = counter.saturating_add(1);
    }
    normalize(vector)
}

#[cfg(any(test, not(feature = "candle-cpu")))]
fn normalize(mut vector: Vec<f32>) -> Vec<f32> {
    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm > 0.0 {
        for value in &mut vector {
            *value /= norm;
        }
    }
    vector
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use loom_types::{InferenceModelKind, ModelRef, RevisionRef};

    use super::*;
    use crate::{InstalledModelFile, InstalledModelRecord};

    fn hardware(runtime: RuntimeKind) -> HardwareReport {
        HardwareReport {
            cpu_arch: "aarch64".to_string(),
            os: "macos".to_string(),
            target_triple: None,
            cpu_count: 8,
            total_memory_bytes: Some(8 * 1024 * 1024 * 1024),
            metal_available: true,
            cuda_available: false,
            candle_cpu_compiled: runtime == RuntimeKind::CandleSafetensors,
            candle_cuda_compiled: false,
            browser_storage_quota_bytes: None,
            compiled_runtimes: vec![runtime],
            hf_home: None,
            hf_cache_dir: None,
        }
    }

    fn write_file(root: &Path, relative_path: &str, bytes: &[u8]) -> InstalledModelFile {
        let path = root.join(relative_path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
        InstalledModelFile {
            relative_path: relative_path.to_string(),
            size_bytes: bytes.len() as u64,
            digest: Some(format!("sha256:{}", sha256_file(&path))),
        }
    }

    fn sha256_file(path: &Path) -> String {
        let bytes = fs::read(path).unwrap();
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        hex::encode(hasher.finalize())
    }

    fn embedding_record(files: Vec<InstalledModelFile>) -> InstalledModelRecord {
        InstalledModelRecord {
            model: ModelRef::new(
                InferenceModelKind::TextEmbedding,
                "sentence-transformers/all-MiniLM-L6-v2",
            )
            .with_revision(RevisionRef::Branch("main".to_string())),
            runtime: RuntimeKind::CandleSafetensors,
            files,
            active_provider_refs: Vec::new(),
        }
    }

    fn llm_record(files: Vec<InstalledModelFile>) -> InstalledModelRecord {
        InstalledModelRecord {
            model: ModelRef::new(InferenceModelKind::Llm, "Qwen/Qwen2.5-0.5B-Instruct")
                .with_revision(RevisionRef::Branch("main".to_string())),
            runtime: RuntimeKind::CandleSafetensors,
            files,
            active_provider_refs: Vec::new(),
        }
    }

    fn core_ml_record(kind: InferenceModelKind) -> InstalledModelRecord {
        InstalledModelRecord {
            model: ModelRef::new(kind, "local/core-ml")
                .with_revision(RevisionRef::Branch("main".to_string())),
            runtime: RuntimeKind::CoreMl,
            files: Vec::new(),
            active_provider_refs: Vec::new(),
        }
    }

    #[test]
    fn activates_text_embedding_from_installed_record() {
        let root = std::env::temp_dir().join(format!(
            "loom-inference-activation-{}-embedding",
            std::process::id()
        ));
        let files = vec![
            write_file(&root, "snapshots/main/config.json", b"{}"),
            write_file(&root, "snapshots/main/tokenizer.json", b"{}"),
            write_file(&root, "snapshots/main/model.safetensors", b"weights"),
        ];
        let record = embedding_record(files);

        let handle = activate_text_embedding_smoke(
            &record,
            &hardware(RuntimeKind::CandleSafetensors),
            &root,
        )
        .unwrap();
        let model = handle.model().unwrap();
        let vectors = handle
            .embed(&["alpha".to_string(), "beta".to_string()])
            .unwrap();
        fs::remove_dir_all(root).unwrap();

        assert_eq!(model.dimension, 384);
        assert_eq!(
            model.weights_digest.as_deref(),
            record.files[2].digest.as_deref()
        );
        assert_eq!(vectors.len(), 2);
        assert_eq!(vectors[0].len(), 384);
        assert_ne!(vectors[0], vectors[1]);
    }

    #[test]
    fn activation_rejects_unrunnable_record() {
        let root = std::env::temp_dir().join(format!(
            "loom-inference-activation-{}-blocked",
            std::process::id()
        ));
        let record = embedding_record(vec![write_file(
            &root,
            "snapshots/main/model.safetensors",
            b"weights",
        )]);

        let err =
            activate_text_embedding(&record, &hardware(RuntimeKind::CandleSafetensors), &root)
                .unwrap_err();
        fs::remove_dir_all(root).unwrap();

        assert_eq!(err.code, loom_types::Code::Unsupported);
        assert!(err.message.contains("missing-config"));
        assert!(err.message.contains("missing-tokenizer"));
    }

    #[test]
    fn core_ml_text_embedding_activation_requires_provider_bridge() {
        let record = core_ml_record(InferenceModelKind::TextEmbedding);
        let err = activate_text_embedding(
            &record,
            &hardware(RuntimeKind::CoreMl),
            Path::new("/tmp/loom-core-ml-unavailable"),
        )
        .unwrap_err();

        assert_eq!(err.code, loom_types::Code::Unsupported);
        assert!(err.message.contains("Core ML model-family provider bridge"));
    }

    #[test]
    fn activates_llm_from_installed_record() {
        let root = std::env::temp_dir().join(format!(
            "loom-inference-activation-{}-llm",
            std::process::id()
        ));
        let files = vec![
            write_file(&root, "snapshots/main/config.json", b"{}"),
            write_file(&root, "snapshots/main/tokenizer.json", b"{}"),
            write_file(&root, "snapshots/main/model.safetensors", b"weights"),
        ];
        let record = llm_record(files);

        let handle =
            activate_llm_smoke(&record, &hardware(RuntimeKind::CandleSafetensors), &root).unwrap();
        let response = handle
            .complete(&LlmRequest {
                messages: vec![crate::Message::user("ping")],
                ..LlmRequest::default()
            })
            .unwrap();
        fs::remove_dir_all(root).unwrap();

        assert_eq!(
            response.model,
            "Qwen/Qwen2.5-0.5B-Instruct@main#candle-safetensors"
        );
        assert!(response.content.contains("ping"));
        assert!(response.content.contains("sha256:"));
    }

    #[test]
    fn core_ml_llm_activation_requires_provider_bridge() {
        let record = core_ml_record(InferenceModelKind::Llm);
        let err = activate_llm(
            &record,
            &hardware(RuntimeKind::CoreMl),
            Path::new("/tmp/loom-core-ml-unavailable"),
        )
        .unwrap_err();

        assert_eq!(err.code, loom_types::Code::Unsupported);
        assert!(err.message.contains("Core ML model-family provider bridge"));
    }
}
