//! Safe MLX runtime bundle contracts.

use std::fs;
use std::path::{Path, PathBuf};

use loom_types::{LoomError, Result};
use serde::{Deserialize, Serialize};

use crate::runtimes::MlxAdapter;
use crate::{InstalledModelRecord, LlmHandle, TextEmbeddingHandle};

pub const LOOM_MLX_ADAPTER_ABI_VERSION: u32 = 1;
pub const LOOM_MLX_RUNTIME_INFO_SCHEMA_VERSION: u32 = 1;
pub const LOOM_MLX_ADAPTER_LIBRARY: &str = "libloom_mlx_adapter.dylib";
pub const MLX_LIBRARY: &str = "libmlx.dylib";
pub const MLX_C_LIBRARY: &str = "libmlxc.dylib";
pub const MLX_METAL_LIBRARY: &str = "mlx.metallib";

pub const LOOM_MLX_SYMBOL_ABI_VERSION: &str = "loom_mlx_adapter_abi_version";
pub const LOOM_MLX_SYMBOL_RUNTIME_INFO: &str = "loom_mlx_adapter_runtime_info";
pub const LOOM_MLX_SYMBOL_LOAD_LLM: &str = "loom_mlx_adapter_load_llm";
pub const LOOM_MLX_SYMBOL_LOAD_TEXT_EMBEDDING: &str = "loom_mlx_adapter_load_text_embedding";
pub const LOOM_MLX_SYMBOL_RELEASE_HANDLE: &str = "loom_mlx_adapter_release_handle";
pub const LOOM_MLX_SYMBOL_LAST_ERROR: &str = "loom_mlx_adapter_last_error";

const REQUIRED_MLX_FILES: [&str; 3] = [MLX_LIBRARY, MLX_C_LIBRARY, MLX_METAL_LIBRARY];
const REQUIRED_ADAPTER_SYMBOLS: [&str; 6] = [
    LOOM_MLX_SYMBOL_ABI_VERSION,
    LOOM_MLX_SYMBOL_RUNTIME_INFO,
    LOOM_MLX_SYMBOL_LOAD_LLM,
    LOOM_MLX_SYMBOL_LOAD_TEXT_EMBEDDING,
    LOOM_MLX_SYMBOL_RELEASE_HANDLE,
    LOOM_MLX_SYMBOL_LAST_ERROR,
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct MlxAdapterAbi {
    pub version: u32,
    pub library: String,
    pub symbols: Vec<String>,
}

impl MlxAdapterAbi {
    pub fn current() -> Self {
        Self {
            version: LOOM_MLX_ADAPTER_ABI_VERSION,
            library: LOOM_MLX_ADAPTER_LIBRARY.to_string(),
            symbols: REQUIRED_ADAPTER_SYMBOLS
                .iter()
                .map(|symbol| (*symbol).to_string())
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MlxRuntimeInfo {
    pub schema_version: u32,
    pub adapter_abi_version: u32,
    pub runtime: String,
    #[serde(default)]
    pub mlx_version: Option<String>,
    #[serde(default)]
    pub metal_available: Option<bool>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub supported_model_families: Vec<String>,
}

impl MlxRuntimeInfo {
    pub fn parse(json: &str) -> Result<Self> {
        let info = serde_json::from_str::<Self>(json).map_err(|error| {
            LoomError::corrupt(format!("invalid MLX runtime info JSON: {error}"))
        })?;
        info.validate()?;
        Ok(info)
    }

    fn validate(&self) -> Result<()> {
        if self.schema_version != LOOM_MLX_RUNTIME_INFO_SCHEMA_VERSION {
            return Err(LoomError::unsupported(format!(
                "MLX runtime info schema version {} does not match required version {LOOM_MLX_RUNTIME_INFO_SCHEMA_VERSION}",
                self.schema_version
            )));
        }
        if self.adapter_abi_version != LOOM_MLX_ADAPTER_ABI_VERSION {
            return Err(LoomError::unsupported(format!(
                "MLX runtime info adapter ABI version {} does not match required version {LOOM_MLX_ADAPTER_ABI_VERSION}",
                self.adapter_abi_version
            )));
        }
        if self.runtime.trim().is_empty() {
            return Err(LoomError::corrupt("MLX runtime info runtime is empty"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct MlxBundleLayout {
    pub root: PathBuf,
    pub adapter_library: PathBuf,
    pub mlx_library: PathBuf,
    pub mlx_c_library: PathBuf,
    pub metal_library: PathBuf,
    pub manifest: PathBuf,
}

impl MlxBundleLayout {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self {
            adapter_library: root.join(LOOM_MLX_ADAPTER_LIBRARY),
            mlx_library: root.join(MLX_LIBRARY),
            mlx_c_library: root.join(MLX_C_LIBRARY),
            metal_library: root.join(MLX_METAL_LIBRARY),
            manifest: root.join("manifest.txt"),
            root,
        }
    }

    pub fn inspect(&self) -> MlxBundleInspection {
        inspect_mlx_bundle_layout(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct MlxBundleInspection {
    pub layout: MlxBundleLayout,
    pub status: MlxBundleStatus,
    pub files: Vec<MlxBundleFile>,
    pub abi: MlxAdapterAbi,
}

impl MlxBundleInspection {
    pub fn is_loadable(&self) -> bool {
        matches!(self.status, MlxBundleStatus::Loadable)
    }

    pub fn require_loadable(&self) -> Result<()> {
        if self.is_loadable() {
            return Ok(());
        }
        Err(LoomError::unsupported(format!(
            "MLX runtime bundle is not loadable: {}",
            self.status.as_str()
        )))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MlxBundleStatus {
    Loadable,
    UnsupportedHost { os: String, arch: String },
    MissingDirectory,
    StaticOnlyPrefix { archives: Vec<String> },
    MissingRuntimeFiles { files: Vec<String> },
    MissingAdapterLibrary,
}

impl MlxBundleStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            MlxBundleStatus::Loadable => "loadable",
            MlxBundleStatus::UnsupportedHost { .. } => "unsupported-host",
            MlxBundleStatus::MissingDirectory => "missing-directory",
            MlxBundleStatus::StaticOnlyPrefix { .. } => "static-only-prefix",
            MlxBundleStatus::MissingRuntimeFiles { .. } => "missing-runtime-files",
            MlxBundleStatus::MissingAdapterLibrary => "missing-adapter-library",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct MlxBundleFile {
    pub name: String,
    pub path: PathBuf,
    pub size_bytes: u64,
}

pub fn inspect_mlx_bundle(root: impl AsRef<Path>) -> MlxBundleInspection {
    MlxBundleLayout::new(root.as_ref()).inspect()
}

pub fn default_mlx_bundle_dir(target_triple: Option<&str>) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("native")
        .join("mlx")
        .join(target_triple.unwrap_or(default_target_triple()))
}

fn inspect_mlx_bundle_layout(layout: &MlxBundleLayout) -> MlxBundleInspection {
    let files = bundle_files(&layout.root);
    let status = mlx_bundle_status(layout);
    MlxBundleInspection {
        layout: layout.clone(),
        status,
        files,
        abi: MlxAdapterAbi::current(),
    }
}

fn mlx_bundle_status(layout: &MlxBundleLayout) -> MlxBundleStatus {
    if !host_supports_mlx() {
        return MlxBundleStatus::UnsupportedHost {
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
        };
    }
    if !layout.root.is_dir() {
        return MlxBundleStatus::MissingDirectory;
    }
    let missing = missing_runtime_files(layout);
    if !missing.is_empty() {
        let archives = static_archives(&layout.root);
        if !archives.is_empty() {
            return MlxBundleStatus::StaticOnlyPrefix { archives };
        }
        return MlxBundleStatus::MissingRuntimeFiles { files: missing };
    }
    if !layout.adapter_library.is_file() {
        return MlxBundleStatus::MissingAdapterLibrary;
    }
    MlxBundleStatus::Loadable
}

fn host_supports_mlx() -> bool {
    std::env::consts::OS == "macos" && std::env::consts::ARCH == "aarch64"
}

fn missing_runtime_files(layout: &MlxBundleLayout) -> Vec<String> {
    REQUIRED_MLX_FILES
        .iter()
        .filter(|name| !layout.root.join(name).is_file())
        .map(|name| (*name).to_string())
        .collect()
}

fn static_archives(root: &Path) -> Vec<String> {
    ["libmlx.a", "libmlxc.a"]
        .into_iter()
        .filter(|name| root.join(name).is_file())
        .map(str::to_string)
        .collect()
}

fn bundle_files(root: &Path) -> Vec<MlxBundleFile> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut files = entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let metadata = entry.metadata().ok()?;
            if !metadata.is_file() {
                return None;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            Some(MlxBundleFile {
                name,
                path: entry.path(),
                size_bytes: metadata.len(),
            })
        })
        .collect::<Vec<_>>();
    files.sort_by(|left, right| left.name.cmp(&right.name));
    files
}

fn default_target_triple() -> &'static str {
    option_env!("TARGET").unwrap_or("aarch64-apple-darwin")
}

#[derive(Debug)]
pub struct MlxDynamicAdapter {
    layout: MlxBundleLayout,
    abi: MlxAdapterAbi,
    #[cfg(feature = "mlx")]
    loaded: LoadedMlxAdapter,
}

impl MlxDynamicAdapter {
    pub fn new(bundle_dir: impl Into<PathBuf>) -> Result<Self> {
        let layout = MlxBundleLayout::new(bundle_dir);
        let inspection = layout.inspect();
        inspection.require_loadable()?;
        #[cfg(not(feature = "mlx"))]
        {
            Err(LoomError::unsupported(
                "MLX dynamic loading requires the loom-inference `mlx` feature",
            ))
        }
        #[cfg(feature = "mlx")]
        {
            let loaded = load_mlx_adapter(&layout)?;
            Ok(Self {
                layout,
                abi: MlxAdapterAbi::current(),
                loaded,
            })
        }
    }

    pub fn layout(&self) -> &MlxBundleLayout {
        &self.layout
    }

    pub fn abi(&self) -> &MlxAdapterAbi {
        &self.abi
    }

    #[cfg(feature = "mlx")]
    pub fn loaded_abi_version(&self) -> u32 {
        self.loaded.abi_version
    }

    #[cfg(feature = "mlx")]
    pub fn loaded_library_path(&self) -> &Path {
        self.loaded.library.path()
    }

    #[cfg(feature = "mlx")]
    pub fn runtime_info(&self) -> &MlxRuntimeInfo {
        &self.loaded.runtime_info
    }
}

#[cfg(feature = "mlx")]
#[derive(Debug)]
struct LoadedMlxAdapter {
    library: loom_native::NativeLibrary,
    abi_version: u32,
    runtime_info: MlxRuntimeInfo,
}

#[cfg(feature = "mlx")]
fn load_mlx_adapter(layout: &MlxBundleLayout) -> Result<LoadedMlxAdapter> {
    let library = loom_native::NativeLibrary::open(&layout.adapter_library)?;
    let abi_version = library
        .load_u32_function_name(LOOM_MLX_SYMBOL_ABI_VERSION)?
        .call();
    if abi_version != LOOM_MLX_ADAPTER_ABI_VERSION {
        return Err(LoomError::unsupported(format!(
            "MLX adapter ABI version {abi_version} does not match required version {LOOM_MLX_ADAPTER_ABI_VERSION}"
        )));
    }
    library.require_symbol_names(&REQUIRED_ADAPTER_SYMBOLS)?;
    let runtime_info_json = library
        .load_static_utf8_function_name(LOOM_MLX_SYMBOL_RUNTIME_INFO)?
        .call_string()?;
    let runtime_info = MlxRuntimeInfo::parse(&runtime_info_json)?;
    Ok(LoadedMlxAdapter {
        library,
        abi_version,
        runtime_info,
    })
}

impl MlxAdapter for MlxDynamicAdapter {
    fn load_llm(&self, _record: &InstalledModelRecord, _cache_dir: &Path) -> Result<LlmHandle> {
        Err(LoomError::unsupported(
            "MLX adapter ABI is defined, but MLX symbol loading is not linked in this build",
        ))
    }

    fn load_text_embedding(
        &self,
        _record: &InstalledModelRecord,
        _cache_dir: &Path,
    ) -> Result<TextEmbeddingHandle> {
        Err(LoomError::unsupported(
            "MLX adapter ABI is defined, but MLX symbol loading is not linked in this build",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "mlx")]
    use std::process::Command;
    #[cfg(feature = "mlx")]
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("loom-inference-{name}-{}", std::process::id()))
    }

    fn reset_dir(path: &Path) {
        if path.exists() {
            fs::remove_dir_all(path).unwrap();
        }
        fs::create_dir_all(path).unwrap();
    }

    fn write_file(path: &Path) {
        fs::write(path, b"x").unwrap();
    }

    #[cfg(feature = "mlx")]
    fn unique_temp_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "loom-inference-{name}-{}-{nanos}",
            std::process::id()
        ))
    }

    #[cfg(all(feature = "mlx", unix))]
    fn write_adapter_fixture(root: &Path, abi_version: u32, runtime_info: &str) -> Option<()> {
        fs::create_dir_all(root).unwrap();
        let source = root.join("loom_mlx_adapter_fixture.c");
        fs::write(
            &source,
            format!(
                r#"#include <stdint.h>
uint32_t loom_mlx_adapter_abi_version(void) {{ return {abi_version}u; }}
const char *loom_mlx_adapter_runtime_info(void) {{ return {runtime_info:?}; }}
void *loom_mlx_adapter_load_llm(void) {{ return 0; }}
void *loom_mlx_adapter_load_text_embedding(void) {{ return 0; }}
void loom_mlx_adapter_release_handle(void *handle) {{ (void)handle; }}
const char *loom_mlx_adapter_last_error(void) {{ return ""; }}
"#
            ),
        )
        .unwrap();
        let mut command = Command::new("cc");
        if cfg!(target_os = "macos") {
            command.arg("-dynamiclib");
        } else {
            command.args(["-shared", "-fPIC"]);
        }
        let output_result = command
            .arg(&source)
            .arg("-o")
            .arg(root.join(LOOM_MLX_ADAPTER_LIBRARY))
            .output();
        let output_result = match output_result {
            Ok(output_result) => output_result,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
            Err(error) => panic!("failed to run cc: {error}"),
        };
        assert!(
            output_result.status.success(),
            "cc failed: {}",
            String::from_utf8_lossy(&output_result.stderr)
        );
        Some(())
    }

    fn host_can_run_mlx_tests() -> bool {
        std::env::consts::OS == "macos" && std::env::consts::ARCH == "aarch64"
    }

    #[test]
    fn current_abi_names_required_symbols() {
        let abi = MlxAdapterAbi::current();

        assert_eq!(abi.version, LOOM_MLX_ADAPTER_ABI_VERSION);
        assert_eq!(abi.library, LOOM_MLX_ADAPTER_LIBRARY);
        assert!(
            abi.symbols
                .contains(&LOOM_MLX_SYMBOL_ABI_VERSION.to_string())
        );
        assert!(abi.symbols.contains(&LOOM_MLX_SYMBOL_LOAD_LLM.to_string()));
        assert!(
            abi.symbols
                .contains(&LOOM_MLX_SYMBOL_LOAD_TEXT_EMBEDDING.to_string())
        );
        assert!(
            abi.symbols
                .contains(&LOOM_MLX_SYMBOL_RUNTIME_INFO.to_string())
        );
    }

    #[test]
    fn runtime_info_requires_supported_schema() {
        let json = r#"{"schema_version":2,"adapter_abi_version":1,"runtime":"mlx"}"#;
        let error = MlxRuntimeInfo::parse(json).unwrap_err();

        assert_eq!(error.code, loom_types::Code::Unsupported);
        assert!(error.message.contains("schema version"));
    }

    #[test]
    fn runtime_info_requires_adapter_abi_match() {
        let json = r#"{"schema_version":1,"adapter_abi_version":2,"runtime":"mlx"}"#;
        let error = MlxRuntimeInfo::parse(json).unwrap_err();

        assert_eq!(error.code, loom_types::Code::Unsupported);
        assert!(error.message.contains("adapter ABI version"));
    }

    #[test]
    fn inspection_reports_missing_directory() {
        if !host_can_run_mlx_tests() {
            return;
        }
        let root = temp_dir("mlx-missing");
        if root.exists() {
            fs::remove_dir_all(&root).unwrap();
        }

        let inspection = inspect_mlx_bundle(&root);

        assert_eq!(inspection.status, MlxBundleStatus::MissingDirectory);
    }

    #[test]
    fn inspection_reports_static_only_prefix() {
        if !host_can_run_mlx_tests() {
            return;
        }
        let root = temp_dir("mlx-static");
        reset_dir(&root);
        write_file(&root.join("libmlx.a"));
        write_file(&root.join("libmlxc.a"));

        let inspection = inspect_mlx_bundle(&root);

        assert_eq!(
            inspection.status,
            MlxBundleStatus::StaticOnlyPrefix {
                archives: vec!["libmlx.a".to_string(), "libmlxc.a".to_string()]
            }
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn inspection_requires_loom_adapter_library_after_runtime_files() {
        if !host_can_run_mlx_tests() {
            return;
        }
        let root = temp_dir("mlx-runtime-only");
        reset_dir(&root);
        for name in REQUIRED_MLX_FILES {
            write_file(&root.join(name));
        }

        let inspection = inspect_mlx_bundle(&root);

        assert_eq!(inspection.status, MlxBundleStatus::MissingAdapterLibrary);
        assert!(!inspection.is_loadable());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn inspection_accepts_complete_bundle_shape() {
        if !host_can_run_mlx_tests() {
            return;
        }
        let root = temp_dir("mlx-complete");
        reset_dir(&root);
        for name in REQUIRED_MLX_FILES {
            write_file(&root.join(name));
        }
        write_file(&root.join(LOOM_MLX_ADAPTER_LIBRARY));

        let inspection = inspect_mlx_bundle(&root);

        assert_eq!(inspection.status, MlxBundleStatus::Loadable);
        assert!(inspection.is_loadable());
        assert_eq!(inspection.files.len(), 4);
        fs::remove_dir_all(&root).unwrap();
    }

    #[cfg(all(feature = "mlx", unix))]
    #[test]
    fn dynamic_adapter_loader_validates_fixture_abi() {
        let root = unique_temp_dir("mlx-adapter-load");
        reset_dir(&root);
        for name in REQUIRED_MLX_FILES {
            write_file(&root.join(name));
        }
        let runtime_info = r#"{"schema_version":1,"adapter_abi_version":1,"runtime":"mlx","mlx_version":"fixture","metal_available":true,"capabilities":["llm","text-embedding"],"supported_model_families":["mlx"]}"#;
        if write_adapter_fixture(&root, LOOM_MLX_ADAPTER_ABI_VERSION, runtime_info).is_none() {
            return;
        }

        let layout = MlxBundleLayout::new(&root);
        let loaded = load_mlx_adapter(&layout).unwrap();

        assert_eq!(loaded.abi_version, LOOM_MLX_ADAPTER_ABI_VERSION);
        assert_eq!(loaded.runtime_info.runtime, "mlx");
        assert_eq!(loaded.runtime_info.mlx_version.as_deref(), Some("fixture"));
        assert_eq!(loaded.runtime_info.metal_available, Some(true));
        assert_eq!(loaded.library.path(), layout.adapter_library.as_path());
        fs::remove_dir_all(&root).unwrap();
    }

    #[cfg(all(feature = "mlx", unix))]
    #[test]
    fn dynamic_adapter_loader_rejects_wrong_abi() {
        let root = unique_temp_dir("mlx-adapter-wrong-abi");
        reset_dir(&root);
        for name in REQUIRED_MLX_FILES {
            write_file(&root.join(name));
        }
        let runtime_info = r#"{"schema_version":1,"adapter_abi_version":1,"runtime":"mlx"}"#;
        if write_adapter_fixture(&root, LOOM_MLX_ADAPTER_ABI_VERSION + 1, runtime_info).is_none() {
            return;
        }

        let layout = MlxBundleLayout::new(&root);
        let error = load_mlx_adapter(&layout).unwrap_err();

        assert_eq!(error.code, loom_types::Code::Unsupported);
        assert!(error.message.contains("does not match required version"));
        fs::remove_dir_all(&root).unwrap();
    }

    #[cfg(all(feature = "mlx", unix))]
    #[test]
    fn dynamic_adapter_loader_rejects_wrong_runtime_info_schema() {
        let root = unique_temp_dir("mlx-adapter-wrong-runtime-info");
        reset_dir(&root);
        for name in REQUIRED_MLX_FILES {
            write_file(&root.join(name));
        }
        let runtime_info = r#"{"schema_version":2,"adapter_abi_version":1,"runtime":"mlx"}"#;
        if write_adapter_fixture(&root, LOOM_MLX_ADAPTER_ABI_VERSION, runtime_info).is_none() {
            return;
        }

        let layout = MlxBundleLayout::new(&root);
        let error = load_mlx_adapter(&layout).unwrap_err();

        assert_eq!(error.code, loom_types::Code::Unsupported);
        assert!(error.message.contains("schema version"));
        fs::remove_dir_all(&root).unwrap();
    }
}
