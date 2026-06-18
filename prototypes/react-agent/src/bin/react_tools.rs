//! `react-tools` - the **function-calling** ReAct variant. Instead of asking the model to print
//! `Action: tool[input]` and parsing text, we send the tools as JSON schemas via the OpenAI
//! `tools` / `tool_calls` API. The model returns structured `tool_calls`; we execute each and reply
//! with `role:"tool"` messages, looping until it answers in plain text. This is more robust than
//! text parsing (no format drift) and is the recommended shape for capable models.
//!
//!   export LOOM_LLM_BASE_URL="http://localhost:1234/v1"; export LOOM_LLM_TOKEN="lm-studio"
//!   export LOOM_LLM_MODEL="your-model-id"
//!   cargo run --bin react-tools -- "What is 47 * 19, and what is the Loom spec?"

use anyhow::{Context, Result};
use react_agent::{Config, Kb, Msg, post_chat, run_tool};
use serde_json::json;
use std::env;

const MAX_STEPS: usize = 8;

fn tools_schema() -> serde_json::Value {
    json!([
        {"type": "function", "function": {
            "name": "calc",
            "description": "Evaluate a simple arithmetic expression 'A op B' where op is + - * /.",
            "parameters": {"type": "object",
                "properties": {"expr": {"type": "string", "description": "e.g. '47 * 19'"}},
                "required": ["expr"]}
        }},
        {"type": "function", "function": {
            "name": "loom_search",
            "description": "Semantic search over a small Loom knowledge base; returns the best matching fact.",
            "parameters": {"type": "object",
                "properties": {"query": {"type": "string"}},
                "required": ["query"]}
        }}
    ])
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
    let mut messages = vec![
        Msg::new("system", "You are a helpful agent. Use the provided tools when they help; when you have the answer, reply in plain text (no tool call)."),
        Msg::new("user", format!("Task: {task}")),
    ];
    println!("== ReAct agent (function-calling) ==\nendpoint: {}\nmodel: {}\ntask: {task}\n", cfg.base_url, cfg.model);

    for step in 1..=MAX_STEPS {
        let body = json!({
            "model": cfg.model,
            "messages": messages,
            "tools": tools_schema(),
            "tool_choice": "auto",
            "temperature": 0.0
        });
        let resp = post_chat(&client, &cfg, &body).await?;
        let msg = resp["choices"][0]["message"].clone();
        let tool_calls = msg.get("tool_calls").and_then(|v| v.as_array()).cloned().unwrap_or_default();

        if tool_calls.is_empty() {
            let content = msg["content"].as_str().unwrap_or("").trim();
            println!("{content}\n\n>>> DONE");
            return Ok(());
        }

        // Append the assistant message verbatim (it carries the tool_calls the tool replies refer to),
        // then one tool-result message per call.
        let assistant: Msg = serde_json::from_value(msg).unwrap_or(Msg {
            role: "assistant".into(),
            tool_calls: Some(serde_json::Value::Array(tool_calls.clone())),
            ..Default::default()
        });
        messages.push(assistant);

        for call in &tool_calls {
            let id = call["id"].as_str().unwrap_or("").to_string();
            let name = call["function"]["name"].as_str().unwrap_or("");
            let args_str = call["function"]["arguments"].as_str().unwrap_or("{}");
            let args: serde_json::Value = serde_json::from_str(args_str).unwrap_or_else(|_| json!({}));
            let input = match name {
                "calc" => args["expr"].as_str().unwrap_or("").to_string(),
                "loom_search" => args["query"].as_str().unwrap_or("").to_string(),
                _ => String::new(),
            };
            let result = run_tool(&kb, name, &input);
            println!("  step {step}: {name}({input}) -> {result}");
            messages.push(Msg {
                role: "tool".into(),
                content: Some(result),
                tool_call_id: Some(id),
                ..Default::default()
            });
        }
    }
    println!("\n(reached MAX_STEPS={MAX_STEPS} without a plain-text answer)");
    Ok(())
}
