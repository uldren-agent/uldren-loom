use crate::maze::{Dir, Maze, Pos};
use crate::render::VisibleMaze;
use crate::run::Scenario;
use anyhow::{bail, Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::env;
use std::time::Duration;

#[derive(Clone, Copy)]
pub enum OracleKind {
    Deterministic,
    LmStudio,
}

impl OracleKind {
    pub fn parse(s: String) -> Result<Self> {
        match s.as_str() {
            "deterministic" => Ok(Self::Deterministic),
            "lm-studio" | "lmstudio" | "llm" => Ok(Self::LmStudio),
            _ => bail!("oracle must be deterministic or lm-studio"),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Deterministic => "deterministic",
            Self::LmStudio => "lm-studio",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct OracleRequest {
    pub scenario_name: String,
    pub maze_width: usize,
    pub maze_height: usize,
    pub rat: Pos,
    pub exit: Option<Pos>,
    pub path_limit: usize,
    pub legal_moves: Vec<LegalMove>,
    pub visible: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct LegalMove {
    pub dir: &'static str,
    pub to: Pos,
    pub reaches_exit: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OracleAnswer {
    pub path: Vec<Dir>,
    pub confidence: Option<f64>,
    pub raw: String,
}

#[allow(async_fn_in_trait)]
pub trait Oracle {
    async fn ask(
        &mut self,
        maze: &Maze,
        scenario: &Scenario,
        rat: Pos,
        visible: &VisibleMaze,
        path_limit: usize,
    ) -> Result<OracleAnswer>;
}

pub struct DeterministicOracle;

impl Oracle for DeterministicOracle {
    async fn ask(
        &mut self,
        maze: &Maze,
        _scenario: &Scenario,
        rat: Pos,
        _visible: &VisibleMaze,
        path_limit: usize,
    ) -> Result<OracleAnswer> {
        let mut path = maze
            .shortest_path(rat)
            .context("maze has no route to exit")?;
        path.truncate(path_limit);
        Ok(OracleAnswer {
            raw: serde_json::to_string(
                &json!({ "path": path_as_strings(&path), "confidence": 1.0 }),
            )?,
            path,
            confidence: Some(1.0),
        })
    }
}

pub struct LmStudioOracle {
    base_url: String,
    token: String,
    model: String,
    api: LmStudioApi,
    client: Client,
}

impl LmStudioOracle {
    pub fn from_env() -> Result<Self> {
        let timeout_secs = env::var("LOOM_LLM_TIMEOUT_SECS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(120);
        Ok(Self {
            base_url: env::var("LOOM_LLM_BASE_URL")
                .unwrap_or_else(|_| "http://localhost:1234".to_string()),
            token: env::var("LOOM_LLM_TOKEN").unwrap_or_else(|_| "lm-studio".to_string()),
            model: env::var("LOOM_LLM_MODEL").unwrap_or_else(|_| "local-model".to_string()),
            api: LmStudioApi::from_env()?,
            client: Client::builder()
                .timeout(Duration::from_secs(timeout_secs))
                .build()
                .context("building LM Studio HTTP client")?,
        })
    }
}

#[derive(Clone, Copy)]
enum LmStudioApi {
    RestV1,
    RestV0,
    OpenAi,
}

impl LmStudioApi {
    fn from_env() -> Result<Self> {
        match env::var("LOOM_LLM_API")
            .unwrap_or_else(|_| "rest-v1".to_string())
            .as_str()
        {
            "rest-v1" | "api-v1" | "native" => Ok(Self::RestV1),
            "rest-v0" | "api-v0" => Ok(Self::RestV0),
            "openai" | "openai-chat" | "chat-completions" => Ok(Self::OpenAi),
            other => bail!("LOOM_LLM_API must be rest-v1, rest-v0, or openai, got {other:?}"),
        }
    }
}

impl Oracle for LmStudioOracle {
    async fn ask(
        &mut self,
        maze: &Maze,
        scenario: &Scenario,
        rat: Pos,
        visible: &VisibleMaze,
        path_limit: usize,
    ) -> Result<OracleAnswer> {
        let request = OracleRequest {
            scenario_name: scenario.name(),
            maze_width: maze.width(),
            maze_height: maze.height(),
            rat,
            exit: scenario.exit_known.then_some(maze.exit()),
            path_limit,
            legal_moves: legal_moves(maze, rat),
            visible: visible.ascii.clone(),
        };
        let user_prompt = user_prompt(&request);
        let body = match self.api {
            LmStudioApi::RestV1 => json!({
                "model": self.model,
                "system_prompt": system_prompt(),
                "input": user_prompt,
                "temperature": 0.0,
                "max_output_tokens": max_tokens_for_path(path_limit),
                "reasoning": "off",
                "store": false,
                "stream": false
            }),
            LmStudioApi::RestV0 | LmStudioApi::OpenAi => json!({
                "model": self.model,
                "temperature": 0.0,
                "max_tokens": max_tokens_for_path(path_limit),
                "stream": false,
                "messages": [
                    { "role": "system", "content": system_prompt() },
                    { "role": "user", "content": user_prompt }
                ]
            }),
        };

        let http_resp = self
            .client
            .post(self.endpoint())
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await
            .context("sending LM Studio request")?;
        let status = http_resp.status();
        let body = http_resp
            .text()
            .await
            .context("reading LM Studio response")?;
        if !status.is_success() {
            bail!("LM Studio returned {status}: {body}");
        }

        let resp: Value = serde_json::from_str(&body).context("parsing LM Studio response")?;
        let raw = self.response_text(&resp)?;
        parse_answer(&raw)
    }
}

impl LmStudioOracle {
    fn endpoint(&self) -> String {
        let base = self
            .base_url
            .trim_end_matches('/')
            .trim_end_matches("/v1")
            .to_string();
        match self.api {
            LmStudioApi::RestV1 => format!("{base}/api/v1/chat"),
            LmStudioApi::RestV0 => format!("{base}/api/v0/chat/completions"),
            LmStudioApi::OpenAi => format!("{base}/v1/chat/completions"),
        }
    }

    fn response_text(&self, resp: &Value) -> Result<String> {
        let raw = match self.api {
            LmStudioApi::RestV1 => resp
                .get("output")
                .and_then(Value::as_array)
                .and_then(|items| {
                    items.iter().find_map(|item| {
                        (item.get("type").and_then(Value::as_str) == Some("message"))
                            .then(|| item.get("content").and_then(Value::as_str))
                            .flatten()
                    })
                }),
            LmStudioApi::RestV0 | LmStudioApi::OpenAi => {
                resp["choices"][0]["message"]["content"].as_str()
            }
        }
        .context("LM Studio response did not contain assistant text")?;
        Ok(raw.trim().to_string())
    }
}

fn system_prompt() -> &'static str {
    "You are a maze oracle. Return only strict JSON with shape {\"path\":[\"N\",\"E\",\"S\",\"W\"],\"confidence\":0.0}. Use N for y-1, E for x+1, S for y+1, W for x-1. Return a short useful prefix, not a full explanation. Do not include text outside the JSON."
}

fn user_prompt(request: &OracleRequest) -> String {
    let exit = match request.exit {
        Some(pos) => format!("exit coordinate: ({},{})", pos.x, pos.y),
        None => "exit coordinate: hidden unless X is visible in the maze".to_string(),
    };
    format!(
        "maze size: {}x{}\nrat coordinate: ({},{})\n{}\npath limit: at most {} directions\nlegal first moves: {}\nvisible maze:\n{}\nReturn the best path prefix from the rat toward the exit. The first direction must be one of the legal first moves.",
        request.maze_width,
        request.maze_height,
        request.rat.x,
        request.rat.y,
        exit,
        request.path_limit,
        legal_moves_text(&request.legal_moves),
        request.visible
    )
}

fn max_tokens_for_path(path_limit: usize) -> usize {
    64 + path_limit.saturating_mul(8).min(512)
}

pub fn parse_answer(raw: &str) -> Result<OracleAnswer> {
    let json_text = extract_json(raw);
    let value: Value = serde_json::from_str(json_text).context("oracle response was not JSON")?;
    let path_value = value
        .get("path")
        .and_then(Value::as_array)
        .context("oracle response missing path array")?;
    let mut path = Vec::with_capacity(path_value.len());
    for item in path_value {
        let dir = item
            .as_str()
            .and_then(Dir::parse)
            .with_context(|| format!("invalid direction {item}"))?;
        path.push(dir);
    }
    Ok(OracleAnswer {
        path,
        confidence: value.get("confidence").and_then(Value::as_f64),
        raw: raw.to_string(),
    })
}

fn path_as_strings(path: &[Dir]) -> Vec<&'static str> {
    path.iter().map(|dir| dir.as_str()).collect()
}

fn legal_moves(maze: &Maze, rat: Pos) -> Vec<LegalMove> {
    [Dir::N, Dir::E, Dir::S, Dir::W]
        .into_iter()
        .filter_map(|dir| {
            maze.step(rat, dir).map(|to| LegalMove {
                dir: dir.as_str(),
                to,
                reaches_exit: to == maze.exit(),
            })
        })
        .collect()
}

fn legal_moves_text(moves: &[LegalMove]) -> String {
    moves
        .iter()
        .map(|mov| {
            format!(
                "{} -> ({},{}){}",
                mov.dir,
                mov.to.x,
                mov.to.y,
                if mov.reaches_exit { " EXIT" } else { "" }
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn extract_json(raw: &str) -> &str {
    let trimmed = raw.trim();
    if trimmed.starts_with('{') && trimmed.ends_with('}') {
        return trimmed;
    }
    match (trimmed.find('{'), trimmed.rfind('}')) {
        (Some(start), Some(end)) if start < end => &trimmed[start..=end],
        _ => trimmed,
    }
}
