//! Runtime adapter contracts and availability probes.

use std::path::Path;

use loom_types::{HardwareReport, ModelRef, Result, RuntimeKind};
use serde::{Deserialize, Serialize};

use crate::{InstalledModelRecord, LlmHandle, TextEmbeddingHandle};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct RuntimeSupportReport {
    pub runtime: RuntimeKind,
    pub available: bool,
    pub reasons: Vec<String>,
    pub guidance: Vec<String>,
}

pub fn probe_runtime_support(hardware: &HardwareReport) -> Vec<RuntimeSupportReport> {
    [
        RuntimeKind::CandleSafetensors,
        RuntimeKind::CandleGguf,
        RuntimeKind::Mlx,
        RuntimeKind::LlamaCpp,
        RuntimeKind::Ollama,
        RuntimeKind::OpenAiCompatibleHttp,
        RuntimeKind::HostedApi,
        RuntimeKind::CoreMl,
    ]
    .into_iter()
    .map(|runtime| probe_one_runtime(runtime, hardware))
    .collect()
}

pub fn probe_one_runtime(runtime: RuntimeKind, hardware: &HardwareReport) -> RuntimeSupportReport {
    let mut reasons = Vec::new();
    let mut guidance = Vec::new();
    if !hardware.supports_runtime(runtime) {
        reasons.push("runtime-not-compiled".to_string());
    }
    match runtime {
        RuntimeKind::Mlx => {
            if hardware.os != "macos" {
                reasons.push("mlx-requires-macos".to_string());
            }
            if !hardware.metal_available {
                reasons.push("mlx-requires-metal".to_string());
            }
            guidance
                .push("enable the optional MLX runtime profile on Apple silicon hosts".to_string());
        }
        RuntimeKind::CoreMl => {
            if hardware.os != "macos" {
                reasons.push("core-ml-requires-macos".to_string());
            }
            guidance.push(
                "enable the optional Core ML runtime profile for Apple built-in inference"
                    .to_string(),
            );
        }
        RuntimeKind::CandleGguf => {
            guidance
                .push("enable the Candle GGUF runtime profile for local GGUF weights".to_string());
        }
        RuntimeKind::LlamaCpp => {
            guidance.push(
                "enable the optional llama.cpp runtime profile for native GGUF execution"
                    .to_string(),
            );
        }
        RuntimeKind::Ollama => {
            guidance.push(
                "enable the Ollama adapter and point it at a reachable local daemon".to_string(),
            );
        }
        RuntimeKind::OpenAiCompatibleHttp | RuntimeKind::HostedApi => {
            guidance
                .push("configure a hosted provider instance with explicit settings".to_string());
        }
        RuntimeKind::CandleSafetensors => {
            guidance.push(
                "enable the Candle safetensors runtime profile for local safetensors weights"
                    .to_string(),
            );
            if hardware.candle_cpu_compiled {
                guidance.push("Candle CPU execution is compiled into this binary".to_string());
            }
            if hardware.candle_cuda_compiled && hardware.cuda_available {
                guidance.push("Candle CUDA execution is compiled and CUDA is visible".to_string());
            } else if hardware.candle_cuda_compiled {
                guidance
                    .push("Candle CUDA execution is compiled, but CUDA is not visible".to_string());
            }
        }
    }
    RuntimeSupportReport {
        runtime,
        available: reasons.is_empty(),
        reasons,
        guidance,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct OllamaModelInfo {
    pub name: String,
    pub digest: Option<String>,
    pub size_bytes: Option<u64>,
    pub modified_at: Option<String>,
}

pub trait OllamaAdapter {
    fn list_models(&self) -> Result<Vec<OllamaModelInfo>>;

    fn show_model(&self, name: &str) -> Result<Option<OllamaModelInfo>>;

    fn pull_model(&self, name: &str) -> Result<()>;

    fn delete_model(&self, name: &str) -> Result<()>;
}

pub trait MlxAdapter {
    fn load_llm(&self, record: &InstalledModelRecord, cache_dir: &Path) -> Result<LlmHandle>;

    fn load_text_embedding(
        &self,
        record: &InstalledModelRecord,
        cache_dir: &Path,
    ) -> Result<TextEmbeddingHandle>;
}

pub trait CoreMlAdapter {
    fn load_llm(&self, record: &InstalledModelRecord, cache_dir: &Path) -> Result<LlmHandle>;

    fn load_text_embedding(
        &self,
        record: &InstalledModelRecord,
        cache_dir: &Path,
    ) -> Result<TextEmbeddingHandle>;
}

pub trait GgufAdapter {
    fn runtime_kind(&self) -> RuntimeKind;

    fn load_llm(&self, record: &InstalledModelRecord, cache_dir: &Path) -> Result<LlmHandle>;
}

pub fn ollama_model_ref(name: &str) -> ModelRef {
    ModelRef::new(
        loom_types::InferenceModelKind::Llm,
        format!("ollama/{name}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hardware(
        os: &str,
        metal_available: bool,
        compiled_runtimes: Vec<RuntimeKind>,
    ) -> HardwareReport {
        HardwareReport {
            cpu_arch: "aarch64".to_string(),
            os: os.to_string(),
            target_triple: None,
            cpu_count: 8,
            total_memory_bytes: Some(16 * 1024 * 1024 * 1024),
            metal_available,
            cuda_available: false,
            candle_cpu_compiled: compiled_runtimes.contains(&RuntimeKind::CandleSafetensors),
            candle_cuda_compiled: false,
            browser_storage_quota_bytes: None,
            compiled_runtimes,
            hf_home: None,
            hf_cache_dir: None,
        }
    }

    #[test]
    fn mlx_probe_requires_macos_metal_and_compiled_runtime() {
        let report = probe_one_runtime(
            RuntimeKind::Mlx,
            &hardware("linux", false, vec![RuntimeKind::Mlx]),
        );

        assert!(!report.available);
        assert!(report.reasons.contains(&"mlx-requires-macos".to_string()));
        assert!(report.reasons.contains(&"mlx-requires-metal".to_string()));
    }

    #[test]
    fn gguf_probe_uses_compiled_runtime_flag() {
        let report = probe_one_runtime(
            RuntimeKind::CandleGguf,
            &hardware("macos", true, vec![RuntimeKind::CandleGguf]),
        );

        assert!(report.available);
        assert!(report.reasons.is_empty());
    }

    #[test]
    fn mlx_probe_is_available_on_macos_metal_when_runtime_is_compiled() {
        let report = probe_one_runtime(
            RuntimeKind::Mlx,
            &hardware("macos", true, vec![RuntimeKind::Mlx]),
        );

        assert!(report.available);
        assert!(report.reasons.is_empty());
    }

    #[test]
    fn llama_cpp_probe_uses_compiled_runtime_flag() {
        let report = probe_one_runtime(
            RuntimeKind::LlamaCpp,
            &hardware("linux", false, vec![RuntimeKind::LlamaCpp]),
        );

        assert!(report.available);
        assert!(report.reasons.is_empty());
    }

    #[test]
    fn core_ml_probe_requires_macos_and_compiled_runtime() {
        let report = probe_one_runtime(
            RuntimeKind::CoreMl,
            &hardware("linux", false, vec![RuntimeKind::CoreMl]),
        );

        assert!(!report.available);
        assert!(
            report
                .reasons
                .contains(&"core-ml-requires-macos".to_string())
        );
    }

    #[test]
    fn candle_safetensors_probe_reports_cuda_compile_state() {
        let mut hardware = hardware("linux", false, Vec::new());
        hardware.cuda_available = true;
        hardware.candle_cuda_compiled = true;

        let report = probe_one_runtime(RuntimeKind::CandleSafetensors, &hardware);

        assert!(report.available);
        assert!(report.reasons.is_empty());
        assert!(
            report
                .guidance
                .contains(&"Candle CUDA execution is compiled and CUDA is visible".to_string())
        );
    }

    #[test]
    fn ollama_model_ref_is_llm_scoped() {
        let model = ollama_model_ref("llama3.2");

        assert_eq!(model.kind, loom_types::InferenceModelKind::Llm);
        assert_eq!(model.repo_id, "ollama/llama3.2");
    }
}
