use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CommandCase {
    pub id: usize,
    pub command: String,
    pub label: Label,
    pub category: String,
    #[serde(default)]
    pub hidden_script: bool,
    #[serde(default)]
    pub script_source: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Label {
    Benign,
    Malicious,
}

pub fn load(path: &Path) -> Result<Vec<CommandCase>> {
    let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut out = Vec::new();
    for (idx, line) in BufReader::new(file).lines().enumerate() {
        let line = line.with_context(|| format!("reading line {}", idx + 1))?;
        if line.trim().is_empty() {
            continue;
        }
        out.push(serde_json::from_str(&line).with_context(|| format!("parsing line {}", idx + 1))?);
    }
    Ok(out)
}
