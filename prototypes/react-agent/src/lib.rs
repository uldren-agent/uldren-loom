//! Shared pieces for the ReAct prototype's two modes:
//! - `react-agent` (src/main.rs): classic **text** ReAct (model emits `Action: tool[input]`, we parse).
//! - `react-tools` (src/bin/react_tools.rs): the OpenAI **function-calling** API (structured
//!   `tool_calls`), which is more robust than text parsing.
//!
//! Both share the tools, the deterministic stand-in embedder, and the **real Loom vector backend**.

use anyhow::{Context, Result};
use loom_core::{
    Loom, MemoryStore, MetaFilter, Metric, WorkspaceId, NsType, Value, VectorSet, get_vector_set,
    put_vector_set,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::env;

pub const EMBED_DIM: usize = 32;

/// An OpenAI-compatible chat message (shared by both modes; `tool_calls`/`tool_call_id` are used by
/// the function-calling mode only).
#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Msg {
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl Msg {
    pub fn new(role: &str, content: impl Into<String>) -> Self {
        Msg { role: role.into(), content: Some(content.into()), ..Default::default() }
    }
}

/// Endpoint config from the environment (LM Studio defaults).
pub struct Config {
    pub base_url: String,
    pub token: String,
    pub model: String,
}
impl Config {
    pub fn from_env() -> Self {
        Config {
            base_url: env::var("LOOM_LLM_BASE_URL")
                .unwrap_or_else(|_| "http://localhost:1234/v1".into()),
            token: env::var("LOOM_LLM_TOKEN").unwrap_or_else(|_| "lm-studio".into()),
            model: env::var("LOOM_LLM_MODEL").unwrap_or_else(|_| "local-model".into()),
        }
    }
}

/// A deterministic stand-in embedder (a hashed bag of words). Real embeddings come from a real
/// provider; this exists only so the prototype can do a real exact search end to end.
pub fn embed(text: &str, dim: usize) -> Vec<f32> {
    let mut v = vec![0f32; dim];
    for tok in text.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|t| !t.is_empty()) {
        let mut h = 0xcbf29ce484222325u64; // FNV-1a
        for b in tok.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        v[(h as usize) % dim] += 1.0;
    }
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in &mut v {
            *x /= norm;
        }
    }
    v
}

/// A small knowledge base backed by a real Loom `vector` workspace (committed, then queried via the
/// vector facet's exact search).
pub struct Kb {
    loom: Loom<MemoryStore>,
    ns: WorkspaceId,
}

impl Kb {
    pub fn seed() -> Result<Self> {
        let facts = [
            ("loom-spec", "Uldren Loom is a content-addressed, versioned store; its specs live in the specs/ folder."),
            ("react", "ReAct interleaves reasoning (Thought) with tool use (Action and Observation)."),
            ("vector", "The vector facet does exact nearest-neighbour search by default, HNSW above a count threshold."),
            ("sync", "Loom sync transfers content-addressed objects; derived indexes are rebuilt locally, not synced."),
            ("tabular", "Tables are versioned blobs; GlueSQL is the default SQL engine over them."),
        ];
        let mut set = VectorSet::new(EMBED_DIM, Metric::Cosine);
        for (id, text) in facts {
            let mut meta = BTreeMap::new();
            meta.insert("text".to_string(), Value::Text(text.to_string()));
            set.upsert(id, embed(text, EMBED_DIM), meta)?;
        }
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom.registry_mut().create(NsType::Vector, None, WorkspaceId::from_bytes([42; 16]))?;
        put_vector_set(&mut loom, ns, "kb", &set)?;
        loom.commit(ns, "agent", "seed kb", 1)?;
        Ok(Self { loom, ns })
    }

    pub fn search(&self, query: &str) -> String {
        let set = match get_vector_set(&self.loom, self.ns, "kb") {
            Ok(s) => s,
            Err(e) => return format!("error: {e}"),
        };
        match set.search(&embed(query, EMBED_DIM), 1, &MetaFilter::All) {
            Ok(hits) if !hits.is_empty() => {
                let hit = &hits[0];
                let text = set
                    .get(&hit.id)
                    .and_then(|(_, m)| m.get("text"))
                    .map(|v| match v {
                        Value::Text(t) => t.clone(),
                        other => format!("{other:?}"),
                    })
                    .unwrap_or_default();
                format!("{text} (id={}, score={:.3})", hit.id, hit.score)
            }
            Ok(_) => "no match".into(),
            Err(e) => format!("error: {e}"),
        }
    }
}

pub fn tool_calc(input: &str) -> String {
    let t: Vec<&str> = input.split_whitespace().collect();
    if t.len() != 3 {
        return "error: expected 'A op B' (e.g. 47 * 19)".into();
    }
    match (t[0].parse::<f64>(), t[2].parse::<f64>()) {
        (Ok(a), Ok(b)) => match t[1] {
            "+" => (a + b).to_string(),
            "-" => (a - b).to_string(),
            "*" => (a * b).to_string(),
            "/" if b != 0.0 => (a / b).to_string(),
            "/" => "error: division by zero".into(),
            op => format!("error: unknown operator {op:?}"),
        },
        _ => "error: operands must be numbers".into(),
    }
}

/// Run a named tool with a string input (text mode) - `calc` takes `A op B`, `loom_search` a query.
pub fn run_tool(kb: &Kb, name: &str, input: &str) -> String {
    match name {
        "calc" => tool_calc(input),
        "loom_search" => kb.search(input),
        other => format!("error: unknown tool {other:?}"),
    }
}

/// POST an OpenAI-compatible chat request body and return the raw JSON response (both modes use this;
/// they build different request bodies).
pub async fn post_chat(
    client: &reqwest::Client,
    cfg: &Config,
    body: &serde_json::Value,
) -> Result<serde_json::Value> {
    let resp = client
        .post(format!("{}/chat/completions", cfg.base_url))
        .bearer_auth(&cfg.token)
        .json(body)
        .send()
        .await
        .context("request to LLM failed (is LM Studio running with the server enabled?)")?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        anyhow::bail!("LLM returned {status}: {text}");
    }
    serde_json::from_str(&text).context("decoding LLM response")
}
