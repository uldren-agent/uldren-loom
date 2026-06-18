//! Safe llama.cpp runtime bundle contracts.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use loom_types::{LoomError, Result};
use serde::{Deserialize, Serialize};

use crate::runtimes::GgufAdapter;
use crate::{InstalledModelRecord, LlmHandle};
#[cfg(feature = "llama-cpp")]
use crate::{Llm, LlmRequest, LlmResponse};

pub const LOOM_LLAMA_CPP_ADAPTER_ABI_VERSION: u32 = 1;
pub const LOOM_LLAMA_CPP_RUNTIME_INFO_SCHEMA_VERSION: u32 = 1;
pub const LOOM_LLAMA_CPP_REQUEST_SCHEMA_VERSION: u32 = 1;
pub const LOOM_LLAMA_CPP_RESPONSE_SCHEMA_VERSION: u32 = 1;
pub const LOOM_LLAMA_CPP_STATUS_OK: u32 = 0;
pub const LOOM_LLAMA_CPP_STATUS_INVALID_REQUEST: u32 = 1;
pub const LOOM_LLAMA_CPP_STATUS_BUFFER_TOO_SMALL: u32 = 2;
pub const LOOM_LLAMA_CPP_STATUS_RUNTIME_ERROR: u32 = 3;

pub const LOOM_LLAMA_CPP_SYMBOL_ABI_VERSION: &str = "loom_llama_cpp_adapter_abi_version";
pub const LOOM_LLAMA_CPP_SYMBOL_RUNTIME_INFO: &str = "loom_llama_cpp_adapter_runtime_info";
pub const LOOM_LLAMA_CPP_SYMBOL_LOAD_LLM: &str = "loom_llama_cpp_adapter_load_llm";
pub const LOOM_LLAMA_CPP_SYMBOL_COMPLETE: &str = "loom_llama_cpp_adapter_complete";
pub const LOOM_LLAMA_CPP_SYMBOL_RELEASE_HANDLE: &str = "loom_llama_cpp_adapter_release_handle";
pub const LOOM_LLAMA_CPP_SYMBOL_LAST_ERROR: &str = "loom_llama_cpp_adapter_last_error";

const REQUIRED_ADAPTER_SYMBOLS: [&str; 6] = [
    LOOM_LLAMA_CPP_SYMBOL_ABI_VERSION,
    LOOM_LLAMA_CPP_SYMBOL_RUNTIME_INFO,
    LOOM_LLAMA_CPP_SYMBOL_LOAD_LLM,
    LOOM_LLAMA_CPP_SYMBOL_COMPLETE,
    LOOM_LLAMA_CPP_SYMBOL_RELEASE_HANDLE,
    LOOM_LLAMA_CPP_SYMBOL_LAST_ERROR,
];

#[cfg(feature = "llama-cpp")]
const COMPLETION_RESPONSE_INITIAL_CAPACITY: usize = 4096;
#[cfg(feature = "llama-cpp")]
const COMPLETION_RESPONSE_MAX_CAPACITY: usize = 16 * 1024 * 1024;
#[cfg(feature = "llama-cpp")]
const COMPLETION_RESPONSE_RETRY_LIMIT: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct LlamaCppAdapterAbi {
    pub version: u32,
    pub library: String,
    pub symbols: Vec<String>,
}

impl LlamaCppAdapterAbi {
    pub fn current() -> Self {
        Self {
            version: LOOM_LLAMA_CPP_ADAPTER_ABI_VERSION,
            library: llama_cpp_adapter_library().to_string(),
            symbols: REQUIRED_ADAPTER_SYMBOLS
                .iter()
                .map(|symbol| (*symbol).to_string())
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LlamaCppRuntimeInfo {
    pub schema_version: u32,
    pub adapter_abi_version: u32,
    pub runtime: String,
    #[serde(default)]
    pub llama_cpp_version: Option<String>,
    #[serde(default)]
    pub backends: Vec<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub supported_model_families: Vec<String>,
}

impl LlamaCppRuntimeInfo {
    pub fn parse(json: &str) -> Result<Self> {
        let info = serde_json::from_str::<Self>(json).map_err(|error| {
            LoomError::corrupt(format!("invalid llama.cpp runtime info JSON: {error}"))
        })?;
        info.validate()?;
        Ok(info)
    }

    fn validate(&self) -> Result<()> {
        if self.schema_version != LOOM_LLAMA_CPP_RUNTIME_INFO_SCHEMA_VERSION {
            return Err(LoomError::unsupported(format!(
                "llama.cpp runtime info schema version {} does not match required version {LOOM_LLAMA_CPP_RUNTIME_INFO_SCHEMA_VERSION}",
                self.schema_version
            )));
        }
        if self.adapter_abi_version != LOOM_LLAMA_CPP_ADAPTER_ABI_VERSION {
            return Err(LoomError::unsupported(format!(
                "llama.cpp runtime info adapter ABI version {} does not match required version {LOOM_LLAMA_CPP_ADAPTER_ABI_VERSION}",
                self.adapter_abi_version
            )));
        }
        if self.runtime.trim().is_empty() {
            return Err(LoomError::corrupt(
                "llama.cpp runtime info runtime is empty",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct LlamaCppBundleLayout {
    pub root: PathBuf,
    pub adapter_library: PathBuf,
    pub llama_library: PathBuf,
    pub ggml_library: PathBuf,
    pub ggml_base_library: PathBuf,
    pub ggml_cpu_library: PathBuf,
    pub backends_dir: PathBuf,
    pub manifest: PathBuf,
}

impl LlamaCppBundleLayout {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self {
            adapter_library: root.join(llama_cpp_adapter_library()),
            llama_library: root.join(runtime_library_name("llama")),
            ggml_library: root.join(runtime_library_name("ggml")),
            ggml_base_library: root.join(runtime_library_name("ggml-base")),
            ggml_cpu_library: root.join(runtime_library_name("ggml-cpu")),
            backends_dir: root.join("backends"),
            manifest: root.join("manifest.txt"),
            root,
        }
    }

    pub fn inspect(&self) -> LlamaCppBundleInspection {
        inspect_llama_cpp_bundle_layout(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct LlamaCppBundleInspection {
    pub layout: LlamaCppBundleLayout,
    pub status: LlamaCppBundleStatus,
    pub files: Vec<LlamaCppBundleFile>,
    pub abi: LlamaCppAdapterAbi,
}

impl LlamaCppBundleInspection {
    pub fn is_loadable(&self) -> bool {
        matches!(self.status, LlamaCppBundleStatus::Loadable)
    }

    pub fn require_loadable(&self) -> Result<()> {
        if self.is_loadable() {
            return Ok(());
        }
        Err(LoomError::unsupported(format!(
            "llama.cpp runtime bundle is not loadable: {}",
            self.status.as_str()
        )))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LlamaCppBundleStatus {
    Loadable,
    UnsupportedHost { os: String, arch: String },
    MissingDirectory,
    MissingRuntimeFiles { files: Vec<String> },
    MissingAdapterLibrary,
}

impl LlamaCppBundleStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            LlamaCppBundleStatus::Loadable => "loadable",
            LlamaCppBundleStatus::UnsupportedHost { .. } => "unsupported-host",
            LlamaCppBundleStatus::MissingDirectory => "missing-directory",
            LlamaCppBundleStatus::MissingRuntimeFiles { .. } => "missing-runtime-files",
            LlamaCppBundleStatus::MissingAdapterLibrary => "missing-adapter-library",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct LlamaCppBundleFile {
    pub name: String,
    pub path: PathBuf,
    pub size_bytes: u64,
}

#[derive(Debug)]
pub struct LlamaCppDynamicAdapter {
    inner: Arc<LlamaCppDynamicAdapterInner>,
}

#[derive(Debug)]
struct LlamaCppDynamicAdapterInner {
    library_path: PathBuf,
    runtime_info: LlamaCppRuntimeInfo,
    #[cfg(feature = "llama-cpp")]
    library: loom_native::NativeLibrary,
}

impl LlamaCppDynamicAdapter {
    pub fn new(bundle_dir: impl AsRef<Path>) -> Result<Self> {
        load_llama_cpp_adapter(&LlamaCppBundleLayout::new(bundle_dir.as_ref()))
    }

    pub fn loaded_library_path(&self) -> &Path {
        &self.inner.library_path
    }

    pub fn runtime_info(&self) -> &LlamaCppRuntimeInfo {
        &self.inner.runtime_info
    }
}

impl GgufAdapter for LlamaCppDynamicAdapter {
    fn runtime_kind(&self) -> loom_types::RuntimeKind {
        loom_types::RuntimeKind::LlamaCpp
    }

    fn load_llm(&self, record: &InstalledModelRecord, cache_dir: &Path) -> Result<LlmHandle> {
        #[cfg(feature = "llama-cpp")]
        {
            self.inner.load_llm(record, cache_dir)
        }
        #[cfg(not(feature = "llama-cpp"))]
        {
            let _ = (record, cache_dir);
            Err(LoomError::unsupported(
                "llama.cpp dynamic loading is not compiled",
            ))
        }
    }
}

pub fn inspect_llama_cpp_bundle(root: impl AsRef<Path>) -> LlamaCppBundleInspection {
    LlamaCppBundleLayout::new(root.as_ref()).inspect()
}

pub fn default_llama_cpp_bundle_dir(target_triple: Option<&str>) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("native")
        .join("llama-cpp")
        .join(target_triple.unwrap_or(default_target_triple()))
}

pub fn llama_cpp_adapter_library() -> &'static str {
    runtime_library_name("loom_llama_cpp_adapter")
}

fn inspect_llama_cpp_bundle_layout(layout: &LlamaCppBundleLayout) -> LlamaCppBundleInspection {
    let files = bundle_files(&layout.root);
    let status = llama_cpp_bundle_status(layout);
    LlamaCppBundleInspection {
        layout: layout.clone(),
        status,
        files,
        abi: LlamaCppAdapterAbi::current(),
    }
}

fn llama_cpp_bundle_status(layout: &LlamaCppBundleLayout) -> LlamaCppBundleStatus {
    if !host_supports_llama_cpp() {
        return LlamaCppBundleStatus::UnsupportedHost {
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
        };
    }
    if !layout.root.is_dir() {
        return LlamaCppBundleStatus::MissingDirectory;
    }
    let missing = missing_runtime_files(layout);
    if !missing.is_empty() {
        return LlamaCppBundleStatus::MissingRuntimeFiles { files: missing };
    }
    if !layout.adapter_library.is_file() {
        return LlamaCppBundleStatus::MissingAdapterLibrary;
    }
    LlamaCppBundleStatus::Loadable
}

fn host_supports_llama_cpp() -> bool {
    matches!(std::env::consts::OS, "linux" | "macos" | "windows")
}

fn missing_runtime_files(layout: &LlamaCppBundleLayout) -> Vec<String> {
    [
        &layout.llama_library,
        &layout.ggml_library,
        &layout.ggml_base_library,
        &layout.ggml_cpu_library,
    ]
    .into_iter()
    .filter(|path| !path.is_file())
    .filter_map(|path| path.file_name()?.to_str().map(str::to_string))
    .collect()
}

fn bundle_files(root: &Path) -> Vec<LlamaCppBundleFile> {
    let mut files = Vec::new();
    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            files.push(LlamaCppBundleFile {
                name: name.to_string(),
                path,
                size_bytes: metadata.len(),
            });
        }
    }
    files.sort_by(|left, right| left.name.cmp(&right.name));
    files
}

fn runtime_library_name(stem: &str) -> &'static str {
    match (std::env::consts::OS, stem) {
        ("windows", "loom_llama_cpp_adapter") => "loom_llama_cpp_adapter.dll",
        ("windows", "llama") => "llama.dll",
        ("windows", "ggml") => "ggml.dll",
        ("windows", "ggml-base") => "ggml-base.dll",
        ("windows", "ggml-cpu") => "ggml-cpu.dll",
        ("macos", "loom_llama_cpp_adapter") => "libloom_llama_cpp_adapter.dylib",
        ("macos", "llama") => "libllama.dylib",
        ("macos", "ggml") => "libggml.dylib",
        ("macos", "ggml-base") => "libggml-base.dylib",
        ("macos", "ggml-cpu") => "libggml-cpu.dylib",
        (_, "loom_llama_cpp_adapter") => "libloom_llama_cpp_adapter.so",
        (_, "llama") => "libllama.so",
        (_, "ggml") => "libggml.so",
        (_, "ggml-base") => "libggml-base.so",
        (_, "ggml-cpu") => "libggml-cpu.so",
        _ => unreachable!("unsupported llama.cpp runtime library stem"),
    }
}

fn default_target_triple() -> &'static str {
    option_env!("TARGET").unwrap_or("unknown")
}

#[cfg(feature = "llama-cpp")]
fn load_llama_cpp_adapter(layout: &LlamaCppBundleLayout) -> Result<LlamaCppDynamicAdapter> {
    layout.inspect().require_loadable()?;
    let library = loom_native::NativeLibrary::open(&layout.adapter_library)?;
    let abi_version = library
        .load_u32_function_name(LOOM_LLAMA_CPP_SYMBOL_ABI_VERSION)?
        .call();
    if abi_version != LOOM_LLAMA_CPP_ADAPTER_ABI_VERSION {
        return Err(LoomError::unsupported(format!(
            "llama.cpp adapter ABI version {abi_version} does not match required version {LOOM_LLAMA_CPP_ADAPTER_ABI_VERSION}"
        )));
    }
    library.require_symbol_names(&REQUIRED_ADAPTER_SYMBOLS)?;
    let runtime_info_json = library
        .load_static_utf8_function_name(LOOM_LLAMA_CPP_SYMBOL_RUNTIME_INFO)?
        .call_string()?;
    let runtime_info = LlamaCppRuntimeInfo::parse(&runtime_info_json)?;
    Ok(LlamaCppDynamicAdapter {
        inner: Arc::new(LlamaCppDynamicAdapterInner {
            library_path: layout.adapter_library.clone(),
            runtime_info,
            library,
        }),
    })
}

#[cfg(not(feature = "llama-cpp"))]
fn load_llama_cpp_adapter(layout: &LlamaCppBundleLayout) -> Result<LlamaCppDynamicAdapter> {
    let inspection = layout.inspect();
    Err(LoomError::unsupported(format!(
        "llama.cpp dynamic loading is not compiled; bundle status is {}",
        inspection.status.as_str()
    )))
}

#[cfg(feature = "llama-cpp")]
#[derive(Debug, Serialize)]
struct LlamaCppLoadLlmRequest<'a> {
    schema_version: u32,
    model_id: String,
    repo_id: &'a str,
    revision: String,
    runtime: &'static str,
    cache_dir: String,
    gguf_path: Option<String>,
    files: Vec<LlamaCppModelFile<'a>>,
}

#[cfg(feature = "llama-cpp")]
#[derive(Debug, Serialize)]
struct LlamaCppModelFile<'a> {
    relative_path: &'a str,
    absolute_path: String,
    size_bytes: u64,
    digest: Option<&'a str>,
}

#[cfg(feature = "llama-cpp")]
#[derive(Debug, Serialize)]
struct LlamaCppCompletionRequest<'a> {
    schema_version: u32,
    model_id: &'a str,
    messages: Vec<LlamaCppMessage<'a>>,
    system_prompt: Option<&'a str>,
    max_tokens: Option<u32>,
    temperature: Option<f32>,
    preferences: LlamaCppPreferences<'a>,
}

#[cfg(feature = "llama-cpp")]
#[derive(Debug, Serialize)]
struct LlamaCppMessage<'a> {
    role: &'static str,
    content: &'a str,
}

#[cfg(feature = "llama-cpp")]
#[derive(Debug, Serialize)]
struct LlamaCppPreferences<'a> {
    hints: Vec<&'a str>,
    cost_priority: Option<f32>,
    speed_priority: Option<f32>,
    intelligence_priority: Option<f32>,
}

#[cfg(feature = "llama-cpp")]
#[derive(Debug, Deserialize)]
struct LlamaCppCompletionResponse {
    schema_version: u32,
    model: String,
    content: String,
    #[serde(default)]
    stop_reason: Option<String>,
}

#[cfg(feature = "llama-cpp")]
impl LlamaCppCompletionResponse {
    fn into_llm_response(self) -> Result<LlmResponse> {
        if self.schema_version != LOOM_LLAMA_CPP_RESPONSE_SCHEMA_VERSION {
            return Err(LoomError::unsupported(format!(
                "llama.cpp completion response schema version {} does not match required version {LOOM_LLAMA_CPP_RESPONSE_SCHEMA_VERSION}",
                self.schema_version
            )));
        }
        if self.model.trim().is_empty() {
            return Err(LoomError::corrupt(
                "llama.cpp completion response model is empty",
            ));
        }
        Ok(LlmResponse {
            model: self.model,
            content: self.content,
            stop_reason: self.stop_reason,
        })
    }
}

#[cfg(feature = "llama-cpp")]
#[derive(Debug)]
struct LlamaCppLlm {
    adapter: Arc<LlamaCppDynamicAdapterInner>,
    handle: u64,
    model_id: String,
}

#[cfg(feature = "llama-cpp")]
impl LlamaCppLlm {
    fn complete_with_adapter(&self, request: &LlmRequest) -> Result<LlmResponse> {
        #[cfg(feature = "llama-cpp")]
        {
            let request_json = serde_json::to_string(&completion_request(&self.model_id, request))
                .map_err(|error| {
                    LoomError::corrupt(format!(
                        "failed to encode llama.cpp completion request: {error}"
                    ))
                })?;
            let response_json = self.adapter.call_complete(self.handle, &request_json)?;
            let response = serde_json::from_str::<LlamaCppCompletionResponse>(&response_json)
                .map_err(|error| {
                    LoomError::corrupt(format!(
                        "invalid llama.cpp completion response JSON: {error}"
                    ))
                })?;
            response.into_llm_response()
        }
        #[cfg(not(feature = "llama-cpp"))]
        {
            let _ = request;
            Err(LoomError::unsupported(
                "llama.cpp dynamic loading is not compiled",
            ))
        }
    }
}

#[cfg(feature = "llama-cpp")]
impl Llm for LlamaCppLlm {
    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn complete(&self, request: &LlmRequest) -> Result<LlmResponse> {
        self.complete_with_adapter(request)
    }
}

#[cfg(feature = "llama-cpp")]
impl Drop for LlamaCppLlm {
    fn drop(&mut self) {
        #[cfg(feature = "llama-cpp")]
        self.adapter.release_handle(self.handle);
    }
}

#[cfg(feature = "llama-cpp")]
impl LlamaCppDynamicAdapterInner {
    fn load_llm(
        self: &Arc<Self>,
        record: &InstalledModelRecord,
        cache_dir: &Path,
    ) -> Result<LlmHandle> {
        let load_json =
            serde_json::to_string(&load_llm_request(record, cache_dir)).map_err(|error| {
                LoomError::corrupt(format!("failed to encode llama.cpp load request: {error}"))
            })?;
        let load = self
            .library
            .load_json_handle_function_name(LOOM_LLAMA_CPP_SYMBOL_LOAD_LLM)?
            .call(&load_json)?;
        if load.status != LOOM_LLAMA_CPP_STATUS_OK {
            return Err(self.status_error("load llm", load.status));
        }
        if load.handle == 0 {
            return Err(LoomError::corrupt(
                "llama.cpp adapter returned a zero llm handle",
            ));
        }
        Ok(LlmHandle::with_provider(Box::new(LlamaCppLlm {
            adapter: Arc::clone(self),
            handle: load.handle,
            model_id: model_id(record),
        })))
    }

    fn call_complete(&self, handle: u64, request_json: &str) -> Result<String> {
        let complete = self
            .library
            .load_json_buffer_function_name(LOOM_LLAMA_CPP_SYMBOL_COMPLETE)?;
        let mut capacity = COMPLETION_RESPONSE_INITIAL_CAPACITY;
        for _ in 0..COMPLETION_RESPONSE_RETRY_LIMIT {
            let response = complete.call(handle, request_json, capacity)?;
            match response.status {
                LOOM_LLAMA_CPP_STATUS_OK => {
                    return response.json.ok_or_else(|| {
                        LoomError::corrupt("llama.cpp adapter returned no completion JSON")
                    });
                }
                LOOM_LLAMA_CPP_STATUS_BUFFER_TOO_SMALL => {
                    capacity = next_response_capacity(response.required_len, capacity)?;
                }
                status => return Err(self.status_error("complete", status)),
            }
        }
        Err(LoomError::unsupported(
            "llama.cpp adapter completion response exceeded retry limit",
        ))
    }

    fn release_handle(&self, handle: u64) {
        if handle == 0 {
            return;
        }
        if let Ok(release) = self
            .library
            .load_u64_function_name(LOOM_LLAMA_CPP_SYMBOL_RELEASE_HANDLE)
        {
            release.call(handle);
        }
    }

    fn status_error(&self, operation: &str, status: u32) -> LoomError {
        let last_error = self
            .library
            .load_static_utf8_function_name(LOOM_LLAMA_CPP_SYMBOL_LAST_ERROR)
            .and_then(|last_error| last_error.call_string())
            .unwrap_or_else(|_| "no adapter error reported".to_string());
        match status {
            LOOM_LLAMA_CPP_STATUS_INVALID_REQUEST => LoomError::invalid(format!(
                "llama.cpp adapter {operation} failed: {last_error}"
            )),
            LOOM_LLAMA_CPP_STATUS_RUNTIME_ERROR => LoomError::unsupported(format!(
                "llama.cpp adapter {operation} failed: {last_error}"
            )),
            _ => LoomError::unsupported(format!(
                "llama.cpp adapter {operation} returned status {status}: {last_error}"
            )),
        }
    }
}

#[cfg(feature = "llama-cpp")]
fn next_response_capacity(required_len: usize, current_capacity: usize) -> Result<usize> {
    let next = required_len
        .max(current_capacity.saturating_mul(2))
        .max(COMPLETION_RESPONSE_INITIAL_CAPACITY);
    if next > COMPLETION_RESPONSE_MAX_CAPACITY {
        return Err(LoomError::unsupported(format!(
            "llama.cpp adapter completion response requires {required_len} bytes, above the configured limit"
        )));
    }
    Ok(next)
}

#[cfg(feature = "llama-cpp")]
fn load_llm_request<'a>(
    record: &'a InstalledModelRecord,
    cache_dir: &Path,
) -> LlamaCppLoadLlmRequest<'a> {
    LlamaCppLoadLlmRequest {
        schema_version: LOOM_LLAMA_CPP_REQUEST_SCHEMA_VERSION,
        model_id: model_id(record),
        repo_id: &record.model.repo_id,
        revision: record.model.revision.value().to_string(),
        runtime: record.runtime.as_str(),
        cache_dir: cache_dir.display().to_string(),
        gguf_path: record
            .files
            .iter()
            .find(|file| file.relative_path.ends_with(".gguf"))
            .map(|file| cache_dir.join(&file.relative_path).display().to_string()),
        files: record
            .files
            .iter()
            .map(|file| LlamaCppModelFile {
                relative_path: &file.relative_path,
                absolute_path: cache_dir.join(&file.relative_path).display().to_string(),
                size_bytes: file.size_bytes,
                digest: file.digest.as_deref(),
            })
            .collect(),
    }
}

#[cfg(feature = "llama-cpp")]
fn completion_request<'a>(
    model_id: &'a str,
    request: &'a LlmRequest,
) -> LlamaCppCompletionRequest<'a> {
    LlamaCppCompletionRequest {
        schema_version: LOOM_LLAMA_CPP_REQUEST_SCHEMA_VERSION,
        model_id,
        messages: request
            .messages
            .iter()
            .map(|message| LlamaCppMessage {
                role: match message.role {
                    crate::Role::User => "user",
                    crate::Role::Assistant => "assistant",
                },
                content: &message.content,
            })
            .collect(),
        system_prompt: request.system_prompt.as_deref(),
        max_tokens: request.max_tokens,
        temperature: request.temperature,
        preferences: LlamaCppPreferences {
            hints: request
                .preferences
                .hints
                .iter()
                .map(String::as_str)
                .collect(),
            cost_priority: request.preferences.cost_priority,
            speed_priority: request.preferences.speed_priority,
            intelligence_priority: request.preferences.intelligence_priority,
        },
    }
}

#[cfg(feature = "llama-cpp")]
fn model_id(record: &InstalledModelRecord) -> String {
    format!(
        "{}@{}#{}",
        record.model.repo_id,
        record.model.revision.value(),
        record.runtime.as_str()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(all(feature = "llama-cpp", unix))]
    use loom_types::{InferenceModelKind, ModelRef, RevisionRef};
    #[cfg(all(feature = "llama-cpp", unix))]
    use std::process::Command;
    #[cfg(all(feature = "llama-cpp", unix))]
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn missing_bundle_reports_missing_directory_on_supported_hosts() {
        let inspection = inspect_llama_cpp_bundle("/tmp/loom-llama-cpp-missing");

        if host_supports_llama_cpp() {
            assert_eq!(inspection.status, LlamaCppBundleStatus::MissingDirectory);
        } else {
            assert!(matches!(
                inspection.status,
                LlamaCppBundleStatus::UnsupportedHost { .. }
            ));
        }
    }

    #[test]
    fn runtime_info_requires_supported_schema() {
        let err = LlamaCppRuntimeInfo::parse(
            r#"{"schema_version":2,"adapter_abi_version":1,"runtime":"llama.cpp"}"#,
        )
        .unwrap_err();

        assert_eq!(err.code, loom_types::Code::Unsupported);
    }

    #[test]
    fn runtime_info_requires_adapter_abi_match() {
        let err = LlamaCppRuntimeInfo::parse(
            r#"{"schema_version":1,"adapter_abi_version":2,"runtime":"llama.cpp"}"#,
        )
        .unwrap_err();

        assert_eq!(err.code, loom_types::Code::Unsupported);
    }

    #[test]
    fn runtime_info_accepts_current_schema() {
        let info = LlamaCppRuntimeInfo::parse(
            r#"{"schema_version":1,"adapter_abi_version":1,"runtime":"llama.cpp","backends":["cpu"],"capabilities":["llm"]}"#,
        )
        .unwrap();

        assert_eq!(info.runtime, "llama.cpp");
        assert_eq!(info.backends, vec!["cpu"]);
        assert_eq!(info.capabilities, vec!["llm"]);
    }

    #[cfg(all(feature = "llama-cpp", unix))]
    #[test]
    fn dynamic_adapter_loader_accepts_fixture() {
        let dir = temp_dir("adapter");
        prepare_runtime_fixture(&dir);
        let Some(_adapter_path) = fixture_adapter_library(&dir) else {
            return;
        };

        let adapter = LlamaCppDynamicAdapter::new(&dir).unwrap();

        assert_eq!(adapter.runtime_info().runtime, "llama.cpp fixture");
        assert_eq!(adapter.runtime_info().backends, vec!["cpu"]);
        assert_eq!(
            adapter.loaded_library_path(),
            dir.join(llama_cpp_adapter_library())
        );
    }

    #[cfg(all(feature = "llama-cpp", unix))]
    #[test]
    fn dynamic_adapter_loads_llm_and_completes_request() {
        let dir = temp_dir("llm");
        prepare_runtime_fixture(&dir);
        let Some(_adapter_path) = fixture_adapter_library(&dir) else {
            return;
        };
        let cache_dir = dir.join("cache");
        fs::create_dir_all(cache_dir.join("snapshots/main")).unwrap();
        fs::write(
            cache_dir.join("snapshots/main/qwen2.5-0.5b-instruct-q4_k_m.gguf"),
            b"gguf",
        )
        .unwrap();
        let record = InstalledModelRecord {
            model: ModelRef::new(InferenceModelKind::Llm, "Qwen/Qwen2.5-0.5B-Instruct-GGUF")
                .with_revision(RevisionRef::Branch("main".to_string())),
            runtime: loom_types::RuntimeKind::LlamaCpp,
            files: vec![crate::InstalledModelFile {
                relative_path: "snapshots/main/qwen2.5-0.5b-instruct-q4_k_m.gguf".to_string(),
                size_bytes: 4,
                digest: Some("sha256:abc".to_string()),
            }],
            active_provider_refs: Vec::new(),
        };
        let adapter = LlamaCppDynamicAdapter::new(&dir).unwrap();

        let handle = adapter.load_llm(&record, &cache_dir).unwrap();
        let response = handle
            .complete(&LlmRequest {
                messages: vec![crate::Message::user("ping")],
                max_tokens: Some(8),
                temperature: Some(0.2),
                ..LlmRequest::default()
            })
            .unwrap();

        assert_eq!(
            handle.model_id(),
            Some("Qwen/Qwen2.5-0.5B-Instruct-GGUF@main#llama-cpp")
        );
        assert_eq!(response.model, "llama.cpp fixture model");
        assert_eq!(response.content, "fixture completion");
        assert_eq!(response.stop_reason.as_deref(), Some("end_turn"));
    }

    #[cfg(all(feature = "llama-cpp", unix))]
    fn temp_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "loom-llama-cpp-{name}-{}-{nanos}",
            std::process::id()
        ))
    }

    #[cfg(all(feature = "llama-cpp", unix))]
    fn prepare_runtime_fixture(dir: &Path) {
        fs::create_dir_all(dir).unwrap();
        for name in [
            runtime_library_name("llama"),
            runtime_library_name("ggml"),
            runtime_library_name("ggml-base"),
            runtime_library_name("ggml-cpu"),
        ] {
            fs::write(dir.join(name), b"").unwrap();
        }
    }

    #[cfg(all(feature = "llama-cpp", unix))]
    fn fixture_adapter_library(dir: &Path) -> Option<PathBuf> {
        let source = dir.join("adapter.c");
        fs::write(
            &source,
            b"#include <stdint.h>\n#include <stddef.h>\n#include <string.h>\nstatic const char *last_error = \"\";\nuint32_t loom_llama_cpp_adapter_abi_version(void) { return 1u; }\nconst char *loom_llama_cpp_adapter_runtime_info(void) { return \"{\\\"schema_version\\\":1,\\\"adapter_abi_version\\\":1,\\\"runtime\\\":\\\"llama.cpp fixture\\\",\\\"backends\\\":[\\\"cpu\\\"],\\\"capabilities\\\":[\\\"llm\\\"]}\"; }\nuint32_t loom_llama_cpp_adapter_load_llm(const char *request_json, uint64_t *out_handle) { if (!request_json || !out_handle) { last_error = \"missing load input\"; return 1u; } *out_handle = 42u; return 0u; }\nuint32_t loom_llama_cpp_adapter_complete(uint64_t handle, const char *request_json, char *out_json, size_t out_json_len, size_t *required_len) { const char *json = \"{\\\"schema_version\\\":1,\\\"model\\\":\\\"llama.cpp fixture model\\\",\\\"content\\\":\\\"fixture completion\\\",\\\"stop_reason\\\":\\\"end_turn\\\"}\"; size_t len = strlen(json) + 1u; if (required_len) { *required_len = len; } if (handle != 42u || !request_json) { last_error = \"bad completion input\"; return 1u; } if (!out_json || out_json_len < len) { return 2u; } memcpy(out_json, json, len); return 0u; }\nvoid loom_llama_cpp_adapter_release_handle(uint64_t handle) { (void)handle; }\nconst char *loom_llama_cpp_adapter_last_error(void) { return last_error; }\n",
        )
        .unwrap();
        let output = dir.join(llama_cpp_adapter_library());
        let mut command = Command::new("cc");
        if cfg!(target_os = "macos") {
            command.arg("-dynamiclib");
        } else {
            command.args(["-shared", "-fPIC"]);
        }
        let output_result = command.arg(&source).arg("-o").arg(&output).output();
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
        Some(output)
    }
}
