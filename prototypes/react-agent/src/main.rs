//! `react-agent` - classic **text** ReAct: the model emits `Thought` + `Action: tool[input]`, we
//! parse it, run the tool, feed back `Observation:`, and loop until `Final Answer:`. Shares the tools
//! and the real Loom vector backend with the `react-tools` (function-calling) binary via the lib.
//!
//!   export LOOM_LLM_BASE_URL="http://localhost:1234/v1"; export LOOM_LLM_TOKEN="lm-studio"
//!   export LOOM_LLM_MODEL="your-model-id"
//!   cargo run --bin react-agent -- "What is 47 * 19, and what is the Loom spec?"

use anyhow::{Context, Result};
use react_agent::{Config, Kb, Msg, post_chat, run_tool};
use serde_json::json;
use std::env;

const MAX_STEPS: usize = 8;
const MAX_NUDGES: usize = 2;

fn system_prompt() -> String {
    r#"You are a careful agent that solves a task using a strict ReAct loop.

Tools:
- calc[A op B]       : evaluate arithmetic, op is + - * / (e.g. calc[47 * 19]).
- loom_search[query] : semantic search over a small Loom knowledge base; returns the best matching fact.

Format - one step per turn, EXACTLY:

Thought: <short reasoning>
Action: <tool>[<input>]

Then STOP. The host runs the tool and replies:
Observation: <result>

When you have everything, finish with:
Final Answer: <answer>

Rules: emit ONE Thought and ONE Action (or a Final Answer) per turn. If the task has multiple parts,
take one Action at a time and keep going until you can give the Final Answer. Never write your own
Observation."#
        .to_string()
}

fn parse_action(text: &str) -> Option<(String, String)> {
    let line = text.lines().find(|l| l.to_lowercase().contains("action:"))?;
    let after = &line[line.to_lowercase().find("action:")? + "action:".len()..];
    let open = after.find('[')?;
    let close = after.rfind(']')?;
    if close <= open {
        return None;
    }
    let tool = after[..open].trim().trim_matches('*').trim().to_string();
    Some((tool, after[open + 1..close].to_string()))
}

fn final_answer(text: &str) -> Option<String> {
    let lower = text.to_lowercase();
    let idx = lower.find("final answer")?;
    let rest = text[idx + "final answer".len()..].trim_start_matches([':', '*', ' ', '\t']);
    Some(rest.trim().to_string())
}

#[tokio::main]
async fn main() -> Result<()> {
    let cfg = Config::from_env();
    let task = {
        let a = env::args().skip(1).collect::<Vec<_>>().join(" ");
        if a.is_empty() { "What is 47 * 19, and what is the Loom spec?".to_string() } else { a }
    };
    let kb = Kb::seed().context("seeding the Loom knowledge base")?;
    let client = reqwest::Client::new();
    let mut messages =
        vec![Msg::new("system", system_prompt()), Msg::new("user", format!("Task: {task}"))];
    println!("== ReAct agent (text) ==\nendpoint: {}\nmodel: {}\ntask: {task}\n", cfg.base_url, cfg.model);

    let mut nudges = 0;
    for step in 1..=MAX_STEPS {
        let body = json!({ "model": cfg.model, "messages": messages, "temperature": 0.0, "stop": ["Observation:"] });
        let resp = post_chat(&client, &cfg, &body).await?;
        let reply = resp["choices"][0]["message"]["content"].as_str().unwrap_or("").to_string();
        println!("{}", reply.trim_end());

        if let Some(answer) = final_answer(&reply) {
            println!("\n>>> DONE: {answer}");
            return Ok(());
        }
        if let Some((tool, input)) = parse_action(&reply) {
            let obs = run_tool(&kb, &tool, &input);
            println!("Observation: {obs}\n");
            messages.push(Msg::new("assistant", reply));
            messages.push(Msg::new("user", format!("Observation: {obs}")));
            continue;
        }
        if nudges < MAX_NUDGES {
            nudges += 1;
            messages.push(Msg::new("assistant", reply));
            messages.push(Msg::new(
                "user",
                "Format error: reply with exactly one line `Action: <tool>[<input>]` or \
                 `Final Answer: <answer>`.",
            ));
            continue;
        }
        println!("\n(step {step}: no Action or Final Answer after {MAX_NUDGES} nudges; stopping)");
        return Ok(());
    }
    println!("\n(reached MAX_STEPS={MAX_STEPS} without a Final Answer)");
    Ok(())
}
