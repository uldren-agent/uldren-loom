//! Hardware and local inference environment probing.

use std::path::{Path, PathBuf};

use loom_types::{HardwareReport, Result, RuntimeKind};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HardwareProbeInputs {
    pub hf_home: Option<PathBuf>,
    pub home: Option<PathBuf>,
    pub cpu_count: Option<u32>,
    pub total_memory_bytes: Option<u64>,
    pub cuda_visible_devices: Option<String>,
}

pub fn probe_hardware() -> Result<HardwareReport> {
    let snapshot = system_snapshot();
    probe_hardware_with(HardwareProbeInputs {
        hf_home: std::env::var_os("HF_HOME").map(PathBuf::from),
        home: std::env::var_os("HOME").map(PathBuf::from),
        cpu_count: snapshot.cpu_count,
        total_memory_bytes: snapshot.total_memory_bytes,
        cuda_visible_devices: std::env::var("CUDA_VISIBLE_DEVICES").ok(),
    })
}

pub fn probe_hardware_with(inputs: HardwareProbeInputs) -> Result<HardwareReport> {
    let (hf_home, hf_cache_dir) =
        resolve_hf_paths(inputs.hf_home.as_deref(), inputs.home.as_deref());
    Ok(HardwareReport {
        cpu_arch: std::env::consts::ARCH.to_string(),
        os: std::env::consts::OS.to_string(),
        target_triple: option_env!("TARGET").map(str::to_string),
        cpu_count: inputs.cpu_count.unwrap_or_else(available_parallelism),
        total_memory_bytes: inputs.total_memory_bytes,
        metal_available: metal_available(),
        cuda_available: cuda_available(inputs.cuda_visible_devices.as_deref()),
        candle_cpu_compiled: candle_cpu_compiled(),
        candle_cuda_compiled: candle_cuda_compiled(),
        browser_storage_quota_bytes: None,
        compiled_runtimes: compiled_runtimes(),
        hf_home,
        hf_cache_dir,
    })
}

pub fn compiled_runtimes() -> Vec<RuntimeKind> {
    let mut runtimes = Vec::new();
    if candle_cpu_compiled() || candle_cuda_compiled() {
        runtimes.push(RuntimeKind::CandleSafetensors);
    }
    if cfg!(feature = "mlx") {
        runtimes.push(RuntimeKind::Mlx);
    }
    if cfg!(feature = "core-ml") {
        runtimes.push(RuntimeKind::CoreMl);
    }
    if cfg!(feature = "llama-cpp") {
        runtimes.push(RuntimeKind::LlamaCpp);
    }
    if cfg!(feature = "ollama-rs") {
        runtimes.push(RuntimeKind::Ollama);
    }
    if cfg!(feature = "genai") {
        runtimes.push(RuntimeKind::OpenAiCompatibleHttp);
        runtimes.push(RuntimeKind::HostedApi);
    }
    runtimes
}

pub fn candle_cpu_compiled() -> bool {
    cfg!(feature = "candle-cpu")
}

pub fn candle_cuda_compiled() -> bool {
    cfg!(feature = "candle-cuda")
}

fn available_parallelism() -> u32 {
    std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1)
        .try_into()
        .unwrap_or(u32::MAX)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct SystemSnapshot {
    cpu_count: Option<u32>,
    total_memory_bytes: Option<u64>,
}

fn system_snapshot() -> SystemSnapshot {
    let mut system = sysinfo::System::new_all();
    system.refresh_memory();
    system.refresh_cpu_all();
    SystemSnapshot {
        cpu_count: nonzero_usize_to_u32(system.cpus().len()),
        total_memory_bytes: nonzero_u64(system.total_memory()),
    }
}

fn nonzero_usize_to_u32(value: usize) -> Option<u32> {
    if value == 0 {
        None
    } else {
        Some(value.try_into().unwrap_or(u32::MAX))
    }
}

fn nonzero_u64(value: u64) -> Option<u64> {
    if value == 0 { None } else { Some(value) }
}

fn resolve_hf_paths(
    hf_home: Option<&Path>,
    home: Option<&Path>,
) -> (Option<String>, Option<String>) {
    match (hf_home, home) {
        (Some(hf_home), _) => (
            Some(hf_home.to_string_lossy().into_owned()),
            Some(hf_home.join("hub").to_string_lossy().into_owned()),
        ),
        (None, Some(home)) => (
            None,
            Some(
                home.join(".cache")
                    .join("huggingface")
                    .join("hub")
                    .to_string_lossy()
                    .into_owned(),
            ),
        ),
        (None, None) => (None, None),
    }
}

fn metal_available() -> bool {
    cfg!(target_os = "macos")
}

fn cuda_available(cuda_visible_devices: Option<&str>) -> bool {
    match cuda_visible_devices {
        Some(value) => {
            let trimmed = value.trim();
            !trimmed.is_empty() && trimmed != "-1"
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_reports_cache_paths_from_hf_home() {
        let report = probe_hardware_with(HardwareProbeInputs {
            hf_home: Some(PathBuf::from("/models/hf")),
            home: Some(PathBuf::from("/home/alice")),
            cpu_count: Some(8),
            total_memory_bytes: Some(16),
            cuda_visible_devices: Some("0".to_string()),
        })
        .unwrap();

        assert_eq!(report.hf_home.as_deref(), Some("/models/hf"));
        assert_eq!(report.hf_cache_dir.as_deref(), Some("/models/hf/hub"));
        assert_eq!(report.cpu_count, 8);
        assert_eq!(report.total_memory_bytes, Some(16));
        assert!(report.cuda_available);
        assert_eq!(report.candle_cpu_compiled, cfg!(feature = "candle-cpu"));
        assert_eq!(report.candle_cuda_compiled, cfg!(feature = "candle-cuda"));
    }

    #[test]
    fn probe_reports_standard_hf_cache_when_home_is_known() {
        let report = probe_hardware_with(HardwareProbeInputs {
            hf_home: None,
            home: Some(PathBuf::from("/home/alice")),
            cpu_count: Some(4),
            total_memory_bytes: None,
            cuda_visible_devices: Some("-1".to_string()),
        })
        .unwrap();

        assert_eq!(
            report.hf_cache_dir.as_deref(),
            Some("/home/alice/.cache/huggingface/hub")
        );
        assert_eq!(report.cpu_count, 4);
        assert!(!report.cuda_available);
    }

    #[test]
    fn system_snapshot_reports_values_when_available() {
        let snapshot = system_snapshot();
        assert!(snapshot.cpu_count.unwrap_or(1) >= 1);
    }

    #[test]
    fn compiled_runtime_list_matches_features() {
        let runtimes = compiled_runtimes();
        assert_eq!(
            runtimes.contains(&RuntimeKind::CandleSafetensors),
            cfg!(feature = "candle-cpu") || cfg!(feature = "candle-cuda")
        );
        assert_eq!(
            runtimes.contains(&RuntimeKind::Ollama),
            cfg!(feature = "ollama-rs")
        );
        assert_eq!(
            runtimes.contains(&RuntimeKind::CoreMl),
            cfg!(feature = "core-ml")
        );
        assert_eq!(
            runtimes.contains(&RuntimeKind::LlamaCpp),
            cfg!(feature = "llama-cpp")
        );
        assert_eq!(
            runtimes.contains(&RuntimeKind::OpenAiCompatibleHttp),
            cfg!(feature = "genai")
        );
        assert_eq!(
            runtimes.contains(&RuntimeKind::HostedApi),
            cfg!(feature = "genai")
        );
    }
}
