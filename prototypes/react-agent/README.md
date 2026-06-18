# react-agent (prototype)

Demonstrates a **ReAct** loop (Thought -> Action -> Observation -> ... -> Final Answer) driving a local LLM
served by **LM Studio** (or any OpenAI-compatible endpoint), with **Bearer-token** auth. It is the
concept behind LEANN's `react_agent.py`, in ~200 lines of async Rust.

## What it shows

The model is told to reason one step at a time and emit `Action: tool[input]`. The host stops the
model at `Observation:`, runs the tool, feeds the real result back as `Observation: ...`, and loops
until the model emits `Final Answer:`. Two demo tools are wired in:

- `calc[A op B]` - a tiny arithmetic evaluator.
- `kb[query]` - a stand-in retrieval tool (swap in a real `vector.search` over a Loom workspace).

Adding a real tool is just another arm in `run_tool` - this is exactly where a Loom-backed
`vector.search` / `sql.query` / web-fetch tool would plug in.

## Two modes (two binaries, one shared lib + real Loom backend)

- **`react-agent`** (text ReAct): the model prints `Thought` + `Action: tool[input]`; the host parses
  it (leniently, with a corrective nudge if the format drifts), runs the tool, feeds back
  `Observation:`, loops to `Final Answer:`.
- **`react-tools`** (function-calling): the tools are sent as JSON schemas via the OpenAI
  `tools`/`tool_calls` API; the model returns structured `tool_calls`, the host runs them and replies
  with `role:"tool"` messages until the model answers in plain text. **More robust** (no text-format
  drift) and recommended for capable models.

Both share `loom_search` (a real exact search over a `loom_core::VectorSet` in a committed Loom
`vector` workspace) and `calc`.

## Run

LM Studio: load a model, enable the local server (Developer -> Start Server), note the port and API
key. Then:

```bash
export LOOM_LLM_BASE_URL="http://localhost:1234/v1"   # LM Studio default
export LOOM_LLM_TOKEN="lm-studio"                     # whatever key LM Studio shows (Bearer)
export LOOM_LLM_MODEL="qwen2.5-7b-instruct"           # the loaded model's id
cargo run -- "What is 47 * 19, and what is the Loom spec?"
```

Defaults (if the env vars are unset): base URL `http://localhost:1234/v1`, token `lm-studio`, model
`local-model`. The token is sent as `Authorization: Bearer <token>`.

## Notes

- **Builds with network** (pulls `tokio`, `reqwest`, `serde`); it is a standalone crate (empty
  `[workspace]`) so these async/HTTP deps never touch the wasm-clean core.
- It calls `localhost`, so it must run on the same machine as LM Studio (the agent's sandbox cannot
  reach your machine's LM Studio).
- `temperature: 0` and `stop: ["Observation:"]` keep the loop deterministic and force the host to
  supply tool output (the heart of ReAct). Tune `MAX_STEPS` to bound runaway loops.
- Want a different shape (streaming, tool-calling via the `tools`/function-calling API instead of
  text parsing, or a real Loom `vector.search` tool)? Say the word and I'll extend it.
