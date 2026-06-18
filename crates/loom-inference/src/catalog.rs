//! Curated inference model catalog.

use loom_types::{InferenceModelKind, ModelRef, RevisionRef, RuntimeKind};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct CuratedModelSpec {
    pub kind: InferenceModelKind,
    pub repo_id: &'static str,
    pub runtime: RuntimeKind,
    pub revision: &'static str,
    pub files: &'static [&'static str],
    pub minimum_memory_bytes: Option<u64>,
    pub requires_gpu: bool,
    pub license: Option<&'static str>,
    pub gated: Option<bool>,
    pub embedding_dimension: Option<usize>,
    pub context_window: Option<u32>,
    pub summary: &'static str,
}

impl CuratedModelSpec {
    pub fn model_ref(self) -> ModelRef {
        ModelRef::new(self.kind, self.repo_id)
            .with_revision(RevisionRef::Branch(self.revision.to_string()))
    }

    pub fn matches_kind(self, kind: Option<InferenceModelKind>) -> bool {
        kind.is_none_or(|kind| self.kind == kind)
    }

    pub fn matches_runtime(self, runtime: Option<RuntimeKind>) -> bool {
        runtime.is_none_or(|runtime| self.runtime == runtime)
    }
}

pub fn curated_models() -> &'static [CuratedModelSpec] {
    &CURATED_MODELS
}

const CURATED_MODELS: [CuratedModelSpec; 5] = [
    CuratedModelSpec {
        kind: InferenceModelKind::TextEmbedding,
        repo_id: "sentence-transformers/all-MiniLM-L6-v2",
        runtime: RuntimeKind::CandleSafetensors,
        revision: "main",
        files: &[
            "config.json",
            "model.safetensors",
            "special_tokens_map.json",
            "tokenizer.json",
            "tokenizer_config.json",
        ],
        minimum_memory_bytes: Some(512 * 1024 * 1024),
        requires_gpu: false,
        license: Some("Apache-2.0"),
        gated: Some(false),
        embedding_dimension: Some(384),
        context_window: None,
        summary: "Small Apache-2.0 embedding model with safetensors weights.",
    },
    CuratedModelSpec {
        kind: InferenceModelKind::TextEmbedding,
        repo_id: "BAAI/bge-small-en-v1.5",
        runtime: RuntimeKind::CandleSafetensors,
        revision: "main",
        files: &[
            "config.json",
            "model.safetensors",
            "special_tokens_map.json",
            "tokenizer.json",
            "tokenizer_config.json",
        ],
        minimum_memory_bytes: Some(768 * 1024 * 1024),
        requires_gpu: false,
        license: Some("MIT"),
        gated: Some(false),
        embedding_dimension: Some(384),
        context_window: None,
        summary: "Compact MIT-licensed embedding model with strong retrieval defaults.",
    },
    CuratedModelSpec {
        kind: InferenceModelKind::Llm,
        repo_id: "HuggingFaceTB/SmolLM2-135M-Instruct",
        runtime: RuntimeKind::CandleSafetensors,
        revision: "main",
        files: &[
            "config.json",
            "generation_config.json",
            "model.safetensors",
            "tokenizer.json",
            "tokenizer_config.json",
        ],
        minimum_memory_bytes: Some(1024 * 1024 * 1024),
        requires_gpu: false,
        license: None,
        gated: Some(false),
        embedding_dimension: None,
        context_window: Some(2048),
        summary: "Very small instruction LLM for CPU smoke tests and constrained hosts.",
    },
    CuratedModelSpec {
        kind: InferenceModelKind::Llm,
        repo_id: "Qwen/Qwen2.5-0.5B-Instruct",
        runtime: RuntimeKind::CandleSafetensors,
        revision: "main",
        files: &[
            "config.json",
            "generation_config.json",
            "merges.txt",
            "model.safetensors",
            "tokenizer.json",
            "tokenizer_config.json",
            "vocab.json",
        ],
        minimum_memory_bytes: Some(2 * 1024 * 1024 * 1024),
        requires_gpu: false,
        license: Some("Apache-2.0"),
        gated: Some(false),
        embedding_dimension: None,
        context_window: Some(32768),
        summary: "Small Apache-2.0 Qwen instruction model for local LLM activation work.",
    },
    CuratedModelSpec {
        kind: InferenceModelKind::Llm,
        repo_id: "Qwen/Qwen2.5-0.5B-Instruct-GGUF",
        runtime: RuntimeKind::LlamaCpp,
        revision: "main",
        files: &["qwen2.5-0.5b-instruct-q4_k_m.gguf"],
        minimum_memory_bytes: Some(1024 * 1024 * 1024),
        requires_gpu: false,
        license: Some("Apache-2.0"),
        gated: Some(false),
        embedding_dimension: None,
        context_window: Some(32768),
        summary: "Apache-2.0 Qwen GGUF Q4_K_M model for optional llama.cpp execution.",
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curated_catalog_has_both_model_kinds() {
        let models = curated_models();
        assert!(
            models
                .iter()
                .any(|model| model.kind == InferenceModelKind::Llm)
        );
        assert!(
            models
                .iter()
                .any(|model| model.kind == InferenceModelKind::TextEmbedding)
        );
    }

    #[test]
    fn curated_catalog_has_download_files() {
        for model in curated_models() {
            assert!(!model.files.is_empty());
            match model.runtime {
                RuntimeKind::CandleSafetensors => {
                    assert!(model.files.iter().any(|file| file.ends_with(".json")));
                    assert!(
                        model
                            .files
                            .iter()
                            .any(|file| file.ends_with(".safetensors"))
                    );
                }
                RuntimeKind::LlamaCpp => {
                    assert!(model.files.iter().any(|file| file.ends_with(".gguf")));
                }
                runtime => panic!("unexpected curated runtime: {runtime:?}"),
            }
            assert!(model.minimum_memory_bytes.is_some());
            assert!(!model.requires_gpu);
            assert_eq!(model.gated, Some(false));
            match model.kind {
                InferenceModelKind::TextEmbedding => assert!(model.embedding_dimension.is_some()),
                InferenceModelKind::Llm => assert!(model.context_window.is_some()),
            }
        }
    }

    #[test]
    fn curated_catalog_filters_by_kind_and_runtime() {
        let model = curated_models()[0];
        assert!(model.matches_kind(Some(model.kind)));
        assert!(!model.matches_kind(Some(InferenceModelKind::Llm)));
        assert!(model.matches_runtime(Some(model.runtime)));
        assert!(!model.matches_runtime(Some(RuntimeKind::Mlx)));
    }

    #[test]
    fn curated_catalog_includes_llama_cpp_gguf_model() {
        let model = curated_models()
            .iter()
            .find(|model| model.runtime == RuntimeKind::LlamaCpp)
            .unwrap();

        assert_eq!(model.kind, InferenceModelKind::Llm);
        assert_eq!(model.repo_id, "Qwen/Qwen2.5-0.5B-Instruct-GGUF");
        assert!(model.matches_runtime(Some(RuntimeKind::LlamaCpp)));
        assert!(model.files.iter().any(|file| file.ends_with(".gguf")));
    }
}
