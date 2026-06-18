//! Durable configured inference instance records.

use std::collections::{BTreeMap, BTreeSet};

use loom_types::{
    Code, InferenceInstanceDescriptor, InferenceInstanceSettings, InferenceModelKind, LoomError,
    ModelRef, Result, RuntimeKind,
};
use serde::{Deserialize, Serialize};

pub const INSTANCE_STORE_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct VectorWorkspaceBinding {
    pub store: String,
    pub workspace: String,
    pub embedding_instance: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct InferenceInstanceState {
    pub version: u32,
    pub instances: Vec<InferenceInstanceDescriptor>,
    pub vector_bindings: Vec<VectorWorkspaceBinding>,
}

impl Default for InferenceInstanceState {
    fn default() -> Self {
        Self {
            version: INSTANCE_STORE_VERSION,
            instances: Vec::new(),
            vector_bindings: Vec::new(),
        }
    }
}

impl InferenceInstanceState {
    pub fn upsert_instance(&mut self, instance: InferenceInstanceDescriptor) {
        if let Some(existing) = self
            .instances
            .iter_mut()
            .find(|existing| existing.name == instance.name)
        {
            *existing = instance;
        } else {
            self.instances.push(instance);
        }
        self.instances
            .sort_by(|left, right| left.name.cmp(&right.name));
    }

    pub fn remove_instance(&mut self, name: &str) -> Option<InferenceInstanceDescriptor> {
        let index = self
            .instances
            .iter()
            .position(|instance| instance.name == name)?;
        Some(self.instances.remove(index))
    }

    pub fn find_instance(&self, name: &str) -> Option<&InferenceInstanceDescriptor> {
        self.instances.iter().find(|instance| instance.name == name)
    }

    pub fn find_instance_mut(&mut self, name: &str) -> Option<&mut InferenceInstanceDescriptor> {
        self.instances
            .iter_mut()
            .find(|instance| instance.name == name)
    }

    pub fn upsert_vector_binding(&mut self, binding: VectorWorkspaceBinding) {
        if let Some(existing) = self.vector_bindings.iter_mut().find(|existing| {
            existing.store == binding.store && existing.workspace == binding.workspace
        }) {
            *existing = binding;
        } else {
            self.vector_bindings.push(binding);
        }
        self.vector_bindings.sort_by(|left, right| {
            (&left.store, &left.workspace).cmp(&(&right.store, &right.workspace))
        });
    }

    pub fn instance_ref_count(&self, name: &str) -> usize {
        self.vector_bindings
            .iter()
            .filter(|binding| binding.embedding_instance == name)
            .count()
    }
}

pub fn build_instance_descriptor(
    name: impl Into<String>,
    kind: InferenceModelKind,
    model: ModelRef,
    runtime: RuntimeKind,
    preset: Option<String>,
    overrides: BTreeMap<String, String>,
) -> Result<InferenceInstanceDescriptor> {
    let name = name.into();
    validate_instance_name(&name)?;
    validate_model_kind(kind, &model)?;
    validate_preset(preset.as_deref())?;
    validate_override_keys(&overrides)?;
    let resolved_settings = resolve_instance_settings(kind, runtime, preset.as_deref(), &overrides);
    Ok(InferenceInstanceDescriptor {
        name,
        kind,
        model,
        runtime,
        preset,
        settings: InferenceInstanceSettings { overrides },
        resolved_settings,
    })
}

pub fn update_instance_descriptor(
    mut instance: InferenceInstanceDescriptor,
    preset: Option<String>,
    overrides: BTreeMap<String, String>,
) -> Result<InferenceInstanceDescriptor> {
    validate_preset(preset.as_deref())?;
    validate_override_keys(&overrides)?;
    if preset.is_some() {
        instance.preset = preset;
    }
    for (key, value) in overrides {
        instance.settings.overrides.insert(key, value);
    }
    instance.resolved_settings = resolve_instance_settings(
        instance.kind,
        instance.runtime,
        instance.preset.as_deref(),
        &instance.settings.overrides,
    );
    Ok(instance)
}

fn validate_instance_name(name: &str) -> Result<()> {
    if name.is_empty()
        || !name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        return Err(LoomError::invalid(format!(
            "invalid inference instance name {name:?}"
        )));
    }
    Ok(())
}

fn validate_model_kind(kind: InferenceModelKind, model: &ModelRef) -> Result<()> {
    if model.kind != kind {
        return Err(LoomError::invalid(format!(
            "instance kind {} does not match model kind {}",
            kind.as_str(),
            model.kind.as_str()
        )));
    }
    Ok(())
}

fn validate_preset(preset: Option<&str>) -> Result<()> {
    match preset {
        None | Some("fast" | "balanced" | "quality" | "deterministic") => Ok(()),
        Some(value) => Err(LoomError::invalid(format!(
            "unknown inference preset {value:?}"
        ))),
    }
}

fn validate_override_keys(overrides: &BTreeMap<String, String>) -> Result<()> {
    for key in overrides.keys() {
        if key.is_empty() || key.starts_with('.') || key.ends_with('.') {
            return Err(LoomError::invalid(format!(
                "invalid inference setting key {key:?}"
            )));
        }
        if key.starts_with("extra.") || allowed_setting_keys().contains(key.as_str()) {
            continue;
        }
        return Err(LoomError::new(
            Code::InvalidArgument,
            format!("unknown inference setting key {key:?}"),
        ));
    }
    Ok(())
}

fn allowed_setting_keys() -> BTreeSet<&'static str> {
    BTreeSet::from([
        "api_key_env",
        "batch_size",
        "cache_control",
        "capture_raw_body",
        "capture_usage",
        "device",
        "dimensions",
        "dtype",
        "embedding_type",
        "encoding_format",
        "effort",
        "endpoint",
        "extra_body",
        "keep_alive",
        "max_tokens",
        "min_p",
        "mirostat",
        "mirostat_eta",
        "mirostat_tau",
        "normalize",
        "num_ctx",
        "num_gpu",
        "num_predict",
        "num_thread",
        "provider",
        "prompt_cache_key",
        "previous_response_id",
        "reasoning_effort",
        "repeat_last_n",
        "repeat_penalty",
        "response_format",
        "response_json_schema",
        "response_json_schema_description",
        "response_json_schema_name",
        "seed",
        "service_tier",
        "stop",
        "store",
        "temperature",
        "tfs_z",
        "think",
        "tool_choice",
        "top_k",
        "top_p",
        "truncate",
        "user",
        "verbosity",
    ])
}

fn resolve_instance_settings(
    kind: InferenceModelKind,
    runtime: RuntimeKind,
    preset: Option<&str>,
    overrides: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut resolved = BTreeMap::new();
    resolved.insert("runtime".to_string(), runtime.as_str().to_string());
    match preset.unwrap_or("balanced") {
        "fast" => {
            resolved.insert("effort".to_string(), "fast".to_string());
            insert_kind_defaults(kind, &mut resolved, "128", "32");
        }
        "quality" => {
            resolved.insert("effort".to_string(), "quality".to_string());
            insert_kind_defaults(kind, &mut resolved, "1024", "8");
        }
        "deterministic" => {
            resolved.insert("effort".to_string(), "deterministic".to_string());
            insert_kind_defaults(kind, &mut resolved, "256", "16");
            if kind == InferenceModelKind::Llm {
                resolved.insert("temperature".to_string(), "0".to_string());
                resolved.insert("top_k".to_string(), "1".to_string());
            }
        }
        _ => {
            resolved.insert("effort".to_string(), "balanced".to_string());
            insert_kind_defaults(kind, &mut resolved, "512", "16");
        }
    }
    for (key, value) in overrides {
        resolved.insert(key.clone(), value.clone());
    }
    resolved
}

fn insert_kind_defaults(
    kind: InferenceModelKind,
    resolved: &mut BTreeMap<String, String>,
    max_tokens: &str,
    batch_size: &str,
) {
    match kind {
        InferenceModelKind::Llm => {
            resolved.insert("max_tokens".to_string(), max_tokens.to_string());
        }
        InferenceModelKind::TextEmbedding => {
            resolved.insert("batch_size".to_string(), batch_size.to_string());
            resolved.insert("normalize".to_string(), "true".to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_types::RevisionRef;

    fn model(kind: InferenceModelKind) -> ModelRef {
        ModelRef::new(kind, "sentence-transformers/all-MiniLM-L6-v2")
            .with_revision(RevisionRef::Branch("main".to_string()))
    }

    #[test]
    fn build_instance_resolves_preset_and_overrides() {
        let mut overrides = BTreeMap::new();
        overrides.insert("batch_size".to_string(), "4".to_string());
        let instance = build_instance_descriptor(
            "fast-embed",
            InferenceModelKind::TextEmbedding,
            model(InferenceModelKind::TextEmbedding),
            RuntimeKind::CandleSafetensors,
            Some("fast".to_string()),
            overrides,
        )
        .unwrap();

        assert_eq!(instance.resolved_settings["effort"], "fast");
        assert_eq!(instance.resolved_settings["batch_size"], "4");
        assert_eq!(instance.resolved_settings["normalize"], "true");
    }

    #[test]
    fn delete_ref_count_tracks_vector_bindings() {
        let mut state = InferenceInstanceState::default();
        state.vector_bindings.push(VectorWorkspaceBinding {
            store: "store.loom".to_string(),
            workspace: "main".to_string(),
            embedding_instance: "fast-embed".to_string(),
        });

        assert_eq!(state.instance_ref_count("fast-embed"), 1);
        assert_eq!(state.instance_ref_count("other"), 0);
    }
}
