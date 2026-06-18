//! Model compatibility evaluation for local and curated inference records.

use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path};

use loom_types::{
    HardwareReport, InferenceModelKind, ModelFitReason, ModelFitReport, ModelRef, RuntimeKind,
};
use sha2::{Digest, Sha256};

use crate::catalog::{CuratedModelSpec, curated_models};
use crate::inventory::{InstalledModelFile, InstalledModelRecord};

const LOCAL_RUNTIME_MEMORY_OVERHEAD_BYTES: u64 = 256 * 1024 * 1024;

pub fn evaluate_curated_model_fit(
    spec: CuratedModelSpec,
    hardware: &HardwareReport,
) -> ModelFitReport {
    let mut reasons = Vec::new();
    check_runtime(spec.runtime, hardware, &mut reasons);
    check_accelerator(spec.runtime, spec.requires_gpu, hardware, &mut reasons);
    check_memory(
        spec.minimum_memory_bytes,
        hardware.total_memory_bytes,
        &mut reasons,
    );
    check_format(
        spec.kind,
        spec.runtime,
        spec.files.iter().copied(),
        &mut reasons,
    );
    check_license_and_gating(Some(spec), &mut reasons);
    finish_report(
        spec.model_ref(),
        spec.runtime,
        spec.minimum_memory_bytes,
        reasons,
    )
}

pub fn evaluate_installed_model_fit(
    record: &InstalledModelRecord,
    hardware: &HardwareReport,
    cache_dir: Option<&Path>,
) -> ModelFitReport {
    let mut reasons = Vec::new();
    let spec = matching_curated_model(record);
    let estimated_memory_bytes = estimate_installed_memory(record, spec);

    check_runtime(record.runtime, hardware, &mut reasons);
    check_accelerator(
        record.runtime,
        spec.is_some_and(|spec| spec.requires_gpu),
        hardware,
        &mut reasons,
    );
    check_memory(
        estimated_memory_bytes,
        hardware.total_memory_bytes,
        &mut reasons,
    );
    check_format(
        record.model.kind,
        record.runtime,
        record.files.iter().map(|file| file.relative_path.as_str()),
        &mut reasons,
    );
    check_installed_files(&record.files, cache_dir, &mut reasons);
    check_license_and_gating(spec, &mut reasons);

    finish_report(
        record.model.clone(),
        record.runtime,
        estimated_memory_bytes,
        reasons,
    )
}

fn finish_report(
    model: ModelRef,
    runtime: RuntimeKind,
    estimated_memory_bytes: Option<u64>,
    mut reasons: Vec<ModelFitReason>,
) -> ModelFitReport {
    reasons.sort();
    reasons.dedup();
    ModelFitReport {
        model,
        runtime,
        runnable: reasons.is_empty(),
        reasons,
        estimated_memory_bytes,
    }
}

fn matching_curated_model(record: &InstalledModelRecord) -> Option<CuratedModelSpec> {
    curated_models().iter().copied().find(|spec| {
        spec.kind == record.model.kind
            && spec.repo_id == record.model.repo_id
            && spec.runtime == record.runtime
            && spec.revision == record.model.revision.value()
    })
}

fn check_runtime(
    runtime: RuntimeKind,
    hardware: &HardwareReport,
    reasons: &mut Vec<ModelFitReason>,
) {
    if !hardware.supports_runtime(runtime) {
        reasons.push(ModelFitReason::RuntimeNotCompiled);
    }
}

fn check_accelerator(
    runtime: RuntimeKind,
    requires_gpu: bool,
    hardware: &HardwareReport,
    reasons: &mut Vec<ModelFitReason>,
) {
    let runtime_needs_gpu = matches!(runtime, RuntimeKind::Mlx | RuntimeKind::CoreMl);
    if (requires_gpu || runtime_needs_gpu) && !(hardware.metal_available || hardware.cuda_available)
    {
        reasons.push(ModelFitReason::MissingGpu);
    }
}

fn check_memory(
    estimated_memory_bytes: Option<u64>,
    total_memory_bytes: Option<u64>,
    reasons: &mut Vec<ModelFitReason>,
) {
    if let (Some(estimated), Some(total)) = (estimated_memory_bytes, total_memory_bytes)
        && estimated > total
    {
        reasons.push(ModelFitReason::InsufficientMemory);
    }
}

fn check_format<'a>(
    kind: InferenceModelKind,
    runtime: RuntimeKind,
    files: impl IntoIterator<Item = &'a str>,
    reasons: &mut Vec<ModelFitReason>,
) {
    let profile = FileProfile::from_paths(files);
    match runtime {
        RuntimeKind::CandleSafetensors | RuntimeKind::Mlx => {
            if !matches!(
                kind,
                InferenceModelKind::Llm | InferenceModelKind::TextEmbedding
            ) {
                reasons.push(ModelFitReason::UnsupportedModelKind);
            }
            if !profile.has_config {
                reasons.push(ModelFitReason::MissingConfig);
            }
            if !profile.has_tokenizer {
                reasons.push(ModelFitReason::MissingTokenizer);
            }
            if !profile.has_safetensors && !profile.has_pytorch_bin {
                reasons.push(ModelFitReason::MissingWeights);
                if profile.has_gguf {
                    reasons.push(ModelFitReason::UnsupportedFormat);
                }
            }
        }
        RuntimeKind::CandleGguf | RuntimeKind::LlamaCpp => {
            if !matches!(kind, InferenceModelKind::Llm) {
                reasons.push(ModelFitReason::UnsupportedModelKind);
            }
            if !profile.has_gguf {
                reasons.push(ModelFitReason::MissingWeights);
                if profile.has_safetensors || profile.has_pytorch_bin {
                    reasons.push(ModelFitReason::UnsupportedFormat);
                }
            }
        }
        RuntimeKind::CoreMl => {
            if !matches!(
                kind,
                InferenceModelKind::Llm | InferenceModelKind::TextEmbedding
            ) {
                reasons.push(ModelFitReason::UnsupportedModelKind);
            }
            if !profile.has_core_ml_model {
                reasons.push(ModelFitReason::UnsupportedFormat);
            }
        }
        RuntimeKind::Ollama | RuntimeKind::OpenAiCompatibleHttp | RuntimeKind::HostedApi => {}
    }
}

fn check_installed_files(
    files: &[InstalledModelFile],
    cache_dir: Option<&Path>,
    reasons: &mut Vec<ModelFitReason>,
) {
    for file in files {
        match file.digest.as_deref() {
            Some(digest) if digest.starts_with("sha256:") => {
                if let Some(cache_dir) = cache_dir {
                    check_file_digest(cache_dir, file, digest, reasons);
                }
            }
            Some(_) => reasons.push(ModelFitReason::UnsupportedDigest(
                file.relative_path.clone(),
            )),
            None => reasons.push(ModelFitReason::MissingDigest(file.relative_path.clone())),
        }
    }
}

fn check_file_digest(
    cache_dir: &Path,
    file: &InstalledModelFile,
    expected_digest: &str,
    reasons: &mut Vec<ModelFitReason>,
) {
    if !is_safe_relative_path(&file.relative_path) {
        reasons.push(ModelFitReason::MissingFile(file.relative_path.clone()));
        return;
    }
    let path = cache_dir.join(&file.relative_path);
    if let Some(actual) = symlink_sha256_digest(&path) {
        if actual == expected_digest {
            return;
        }
        reasons.push(ModelFitReason::DigestMismatch(file.relative_path.clone()));
        return;
    }
    match sha256_digest(&path) {
        Ok(actual) if format!("sha256:{actual}") == expected_digest => {}
        Ok(_) => reasons.push(ModelFitReason::DigestMismatch(file.relative_path.clone())),
        Err(_) => reasons.push(ModelFitReason::MissingFile(file.relative_path.clone())),
    }
}

fn check_license_and_gating(spec: Option<CuratedModelSpec>, reasons: &mut Vec<ModelFitReason>) {
    match spec.and_then(|spec| spec.license) {
        Some(license) if permissive_license(license) => {}
        Some(_) => reasons.push(ModelFitReason::LicenseBlocked),
        None => reasons.push(ModelFitReason::LicenseUnknown),
    }
    match spec.and_then(|spec| spec.gated) {
        Some(false) => {}
        Some(true) => reasons.push(ModelFitReason::GatedModel),
        None => reasons.push(ModelFitReason::GatedStatusUnknown),
    }
}

fn estimate_installed_memory(
    record: &InstalledModelRecord,
    spec: Option<CuratedModelSpec>,
) -> Option<u64> {
    let file_total = record
        .files
        .iter()
        .try_fold(0_u64, |total, file| total.checked_add(file.size_bytes));
    let file_estimate = file_total.map(|file_total| match record.runtime {
        RuntimeKind::CandleSafetensors | RuntimeKind::Mlx => file_total
            .saturating_add(file_total / 2)
            .saturating_add(LOCAL_RUNTIME_MEMORY_OVERHEAD_BYTES),
        RuntimeKind::CandleGguf | RuntimeKind::LlamaCpp => {
            file_total.saturating_add(LOCAL_RUNTIME_MEMORY_OVERHEAD_BYTES)
        }
        RuntimeKind::CoreMl => file_total.saturating_add(LOCAL_RUNTIME_MEMORY_OVERHEAD_BYTES),
        RuntimeKind::Ollama | RuntimeKind::OpenAiCompatibleHttp | RuntimeKind::HostedApi => {
            file_total
        }
    });
    match (
        file_estimate,
        spec.and_then(|spec| spec.minimum_memory_bytes),
    ) {
        (Some(left), Some(right)) => Some(left.max(right)),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

fn permissive_license(license: &str) -> bool {
    let normalized = license.trim().to_ascii_lowercase();
    matches!(
        normalized.as_str(),
        "apache-2.0" | "mit" | "bsd-2-clause" | "bsd-3-clause" | "isc" | "zlib" | "cc0-1.0"
    )
}

fn sha256_digest(path: &Path) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn symlink_sha256_digest(path: &Path) -> Option<String> {
    let target = fs::read_link(path).ok()?;
    let blob = target.file_name()?.to_str()?;
    (blob.len() == 64 && blob.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| format!("sha256:{blob}"))
}

fn is_safe_relative_path(path: &str) -> bool {
    let mut has_component = false;
    for component in Path::new(path).components() {
        match component {
            Component::Normal(_) => has_component = true,
            _ => return false,
        }
    }
    has_component
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct FileProfile {
    has_config: bool,
    has_tokenizer: bool,
    has_safetensors: bool,
    has_pytorch_bin: bool,
    has_gguf: bool,
    has_core_ml_model: bool,
}

impl FileProfile {
    fn from_paths<'a>(paths: impl IntoIterator<Item = &'a str>) -> Self {
        let mut profile = Self::default();
        for path in paths {
            let file_name = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
            match file_name.as_str() {
                "config.json" => profile.has_config = true,
                "tokenizer.json" => profile.has_tokenizer = true,
                "pytorch_model.bin" => profile.has_pytorch_bin = true,
                _ if file_name.ends_with(".safetensors") => profile.has_safetensors = true,
                _ if file_name.ends_with(".gguf") => profile.has_gguf = true,
                _ if file_name.ends_with(".mlmodel") => profile.has_core_ml_model = true,
                _ if file_name.ends_with(".mlpackage") => profile.has_core_ml_model = true,
                _ => {}
            }
            if path.to_ascii_lowercase().contains(".mlmodelc/") {
                profile.has_core_ml_model = true;
            }
        }
        profile
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use loom_types::{InferenceModelKind, RevisionRef};

    use super::*;

    fn hardware(
        compiled_runtimes: Vec<RuntimeKind>,
        total_memory_bytes: Option<u64>,
    ) -> HardwareReport {
        HardwareReport {
            cpu_arch: "aarch64".to_string(),
            os: "macos".to_string(),
            target_triple: None,
            cpu_count: 8,
            total_memory_bytes,
            metal_available: true,
            cuda_available: false,
            candle_cpu_compiled: compiled_runtimes.contains(&RuntimeKind::CandleSafetensors),
            candle_cuda_compiled: false,
            browser_storage_quota_bytes: None,
            compiled_runtimes,
            hf_home: None,
            hf_cache_dir: None,
        }
    }

    fn installed_embedding(files: Vec<InstalledModelFile>) -> InstalledModelRecord {
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

    #[test]
    fn curated_model_reports_runtime_and_memory_blocks() {
        let report = evaluate_curated_model_fit(
            curated_models()[0],
            &hardware(Vec::new(), Some(128 * 1024 * 1024)),
        );

        assert!(!report.runnable);
        assert!(report.reasons.contains(&ModelFitReason::RuntimeNotCompiled));
        assert!(report.reasons.contains(&ModelFitReason::InsufficientMemory));
        assert_eq!(report.estimated_memory_bytes, Some(512 * 1024 * 1024));
    }

    #[test]
    fn curated_model_reports_gated_metadata_block() {
        let spec = CuratedModelSpec {
            gated: Some(true),
            license: Some("Apache-2.0"),
            ..curated_models()[0]
        };
        let report = evaluate_curated_model_fit(
            spec,
            &hardware(vec![RuntimeKind::CandleSafetensors], Some(u64::MAX)),
        );

        assert!(!report.runnable);
        assert_eq!(report.reasons, vec![ModelFitReason::GatedModel]);
    }

    #[test]
    fn curated_llama_cpp_gguf_model_fits_when_runtime_is_compiled() {
        let spec = curated_models()
            .iter()
            .copied()
            .find(|spec| spec.runtime == RuntimeKind::LlamaCpp)
            .unwrap();

        let report = evaluate_curated_model_fit(
            spec,
            &hardware(vec![RuntimeKind::LlamaCpp], Some(u64::MAX)),
        );

        assert!(report.runnable);
        assert!(report.reasons.is_empty());
    }

    #[test]
    fn installed_llama_cpp_gguf_model_fits_with_gguf_file() {
        let record = InstalledModelRecord {
            model: ModelRef::new(InferenceModelKind::Llm, "Qwen/Qwen2.5-0.5B-Instruct-GGUF")
                .with_revision(RevisionRef::Branch("main".to_string())),
            runtime: RuntimeKind::LlamaCpp,
            files: vec![InstalledModelFile {
                relative_path: "snapshots/main/qwen2.5-0.5b-instruct-q4_k_m.gguf".to_string(),
                size_bytes: 491 * 1024 * 1024,
                digest: Some("sha256:abc".to_string()),
            }],
            active_provider_refs: Vec::new(),
        };

        let report = evaluate_installed_model_fit(
            &record,
            &hardware(vec![RuntimeKind::LlamaCpp], Some(u64::MAX)),
            None,
        );

        assert!(report.runnable);
        assert!(report.reasons.is_empty());
    }

    #[test]
    fn installed_model_requires_known_digest_and_metadata() {
        let record = InstalledModelRecord {
            model: ModelRef::new(InferenceModelKind::TextEmbedding, "example/private-model")
                .with_revision(RevisionRef::Branch("main".to_string())),
            runtime: RuntimeKind::CandleSafetensors,
            files: vec![InstalledModelFile {
                relative_path: "snapshots/main/model.safetensors".to_string(),
                size_bytes: 32,
                digest: None,
            }],
            active_provider_refs: Vec::new(),
        };
        let report = evaluate_installed_model_fit(
            &record,
            &hardware(vec![RuntimeKind::CandleSafetensors], Some(u64::MAX)),
            None,
        );

        assert!(!report.runnable);
        assert!(report.reasons.contains(&ModelFitReason::MissingConfig));
        assert!(report.reasons.contains(&ModelFitReason::MissingTokenizer));
        assert!(report.reasons.contains(&ModelFitReason::MissingDigest(
            "snapshots/main/model.safetensors".to_string()
        )));
        assert!(report.reasons.contains(&ModelFitReason::LicenseUnknown));
        assert!(report.reasons.contains(&ModelFitReason::GatedStatusUnknown));
    }

    #[test]
    fn installed_model_passes_when_files_digests_and_metadata_match() {
        let root =
            std::env::temp_dir().join(format!("loom-inference-compat-{}-ok", std::process::id()));
        let snapshot = root.join("snapshots/main");
        fs::create_dir_all(&snapshot).unwrap();
        let files = [
            ("snapshots/main/config.json", "{}"),
            ("snapshots/main/tokenizer.json", "{}"),
            ("snapshots/main/model.safetensors", "weights"),
        ];
        for (relative_path, body) in files {
            fs::write(root.join(relative_path), body).unwrap();
        }
        let record = installed_embedding(
            ["config.json", "tokenizer.json", "model.safetensors"]
                .iter()
                .map(|file| {
                    let relative_path = format!("snapshots/main/{file}");
                    let path = root.join(&relative_path);
                    InstalledModelFile {
                        relative_path,
                        size_bytes: fs::metadata(&path).unwrap().len(),
                        digest: Some(format!("sha256:{}", sha256_digest(&path).unwrap())),
                    }
                })
                .collect(),
        );

        let report = evaluate_installed_model_fit(
            &record,
            &hardware(
                vec![RuntimeKind::CandleSafetensors],
                Some(8 * 1024 * 1024 * 1024),
            ),
            Some(&root),
        );
        fs::remove_dir_all(root).unwrap();

        assert!(report.runnable);
        assert!(report.reasons.is_empty());
        assert!(report.estimated_memory_bytes.is_some());
    }

    #[cfg(unix)]
    #[test]
    fn installed_model_uses_hf_symlink_digest_without_hashing_target() {
        let root = std::env::temp_dir().join(format!(
            "loom-inference-compat-{}-symlink",
            std::process::id()
        ));
        let snapshot = root.join("snapshots/main");
        fs::create_dir_all(&snapshot).unwrap();
        fs::write(snapshot.join("config.json"), "{}").unwrap();
        fs::write(snapshot.join("tokenizer.json"), "{}").unwrap();
        let digest = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        std::os::unix::fs::symlink(
            format!("../../blobs/{digest}"),
            snapshot.join("model.safetensors"),
        )
        .unwrap();
        let record = installed_embedding(vec![
            InstalledModelFile {
                relative_path: "snapshots/main/config.json".to_string(),
                size_bytes: 2,
                digest: Some(format!(
                    "sha256:{}",
                    sha256_digest(&snapshot.join("config.json")).unwrap()
                )),
            },
            InstalledModelFile {
                relative_path: "snapshots/main/tokenizer.json".to_string(),
                size_bytes: 2,
                digest: Some(format!(
                    "sha256:{}",
                    sha256_digest(&snapshot.join("tokenizer.json")).unwrap()
                )),
            },
            InstalledModelFile {
                relative_path: "snapshots/main/model.safetensors".to_string(),
                size_bytes: 4 * 1024 * 1024 * 1024,
                digest: Some(format!("sha256:{digest}")),
            },
        ]);

        let report = evaluate_installed_model_fit(
            &record,
            &hardware(
                vec![RuntimeKind::CandleSafetensors],
                Some(8 * 1024 * 1024 * 1024),
            ),
            Some(&root),
        );
        fs::remove_dir_all(root).unwrap();

        assert!(report.runnable);
        assert!(report.reasons.is_empty());
    }

    #[test]
    fn installed_model_reports_digest_mismatch() {
        let root = std::env::temp_dir().join(format!(
            "loom-inference-compat-{}-mismatch",
            std::process::id()
        ));
        let snapshot = root.join("snapshots/main");
        fs::create_dir_all(&snapshot).unwrap();
        fs::write(snapshot.join("config.json"), "{}").unwrap();
        fs::write(snapshot.join("tokenizer.json"), "{}").unwrap();
        fs::write(snapshot.join("model.safetensors"), "changed").unwrap();
        let record = installed_embedding(vec![
            InstalledModelFile {
                relative_path: "snapshots/main/config.json".to_string(),
                size_bytes: 2,
                digest: Some(format!(
                    "sha256:{}",
                    sha256_digest(&snapshot.join("config.json")).unwrap()
                )),
            },
            InstalledModelFile {
                relative_path: "snapshots/main/tokenizer.json".to_string(),
                size_bytes: 2,
                digest: Some(format!(
                    "sha256:{}",
                    sha256_digest(&snapshot.join("tokenizer.json")).unwrap()
                )),
            },
            InstalledModelFile {
                relative_path: "snapshots/main/model.safetensors".to_string(),
                size_bytes: 7,
                digest: Some("sha256:0000".to_string()),
            },
        ]);

        let report = evaluate_installed_model_fit(
            &record,
            &hardware(
                vec![RuntimeKind::CandleSafetensors],
                Some(8 * 1024 * 1024 * 1024),
            ),
            Some(&root),
        );
        fs::remove_dir_all(root).unwrap();

        assert!(!report.runnable);
        assert!(report.reasons.contains(&ModelFitReason::DigestMismatch(
            "snapshots/main/model.safetensors".to_string()
        )));
    }

    #[test]
    fn gguf_runtime_rejects_embedding_safetensors() {
        let record = InstalledModelRecord {
            model: ModelRef::new(
                InferenceModelKind::TextEmbedding,
                "sentence-transformers/all-MiniLM-L6-v2",
            )
            .with_revision(RevisionRef::Branch("main".to_string())),
            runtime: RuntimeKind::CandleGguf,
            files: vec![InstalledModelFile {
                relative_path: "snapshots/main/model.safetensors".to_string(),
                size_bytes: 12,
                digest: Some("sha256:abc".to_string()),
            }],
            active_provider_refs: Vec::new(),
        };

        let report = evaluate_installed_model_fit(
            &record,
            &hardware(vec![RuntimeKind::CandleGguf], Some(u64::MAX)),
            None,
        );

        assert!(!report.runnable);
        assert!(
            report
                .reasons
                .contains(&ModelFitReason::UnsupportedModelKind)
        );
        assert!(report.reasons.contains(&ModelFitReason::UnsupportedFormat));
    }

    #[test]
    fn core_ml_format_accepts_compiled_model_bundle_file() {
        let record = InstalledModelRecord {
            model: ModelRef::new(InferenceModelKind::TextEmbedding, "local/core-ml")
                .with_revision(RevisionRef::Branch("main".to_string())),
            runtime: RuntimeKind::CoreMl,
            files: vec![InstalledModelFile {
                relative_path: "TextEmbedding.mlmodelc/model.mil".to_string(),
                size_bytes: 128,
                digest: Some("sha256:abc".to_string()),
            }],
            active_provider_refs: Vec::new(),
        };

        let report = evaluate_installed_model_fit(
            &record,
            &hardware(vec![RuntimeKind::CoreMl], Some(u64::MAX)),
            None,
        );

        assert!(!report.reasons.contains(&ModelFitReason::UnsupportedFormat));
    }
}
