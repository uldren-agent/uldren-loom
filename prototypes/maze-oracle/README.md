# maze-oracle

A throwaway prototype for measuring an LLM oracle guiding a rat through one deterministic maze.

The runner compares six scenarios for each maze size:

- full view, exit coordinate known
- 50 percent local view, exit coordinate known
- 15 percent local view, exit coordinate known
- full view, exit coordinate hidden
- 50 percent local view, exit coordinate hidden
- 15 percent local view, exit coordinate hidden

The maze entrance is always the bottom left opening and the exit is always the top right opening. For
one size and seed, every scenario uses the same maze. Sizes `2` through `6` use a fully open grid for
debugging prompt and direction handling. Sizes `7` and above use generated mazes and must be odd.

## Run with a deterministic oracle

```
cargo run -- --oracle deterministic
```

## Run with LM Studio

The prototype uses the same environment names as `../react-agent/run.sh`.

```
export LOOM_LLM_BASE_URL="http://localhost:1234"
export LOOM_LLM_TOKEN="sk-lm-cL6kSL9o:OwhBkvVACocQnZbNRJqf"
export LOOM_LLM_MODEL="google/gemma-4-31b-qat"
export LOOM_LLM_API="rest-v1"
cargo run -- --oracle lm-studio --analysis
```

`LOOM_LLM_API` accepts `rest-v1`, `rest-v0`, or `openai`. The default is `rest-v1`, which calls
LM Studio's native `POST /api/v1/chat` endpoint and sends `reasoning: "off"`. If you are using the
OpenAI-compatible server, set `LOOM_LLM_API=openai`.

Useful flags:

```
--execution permissive
--execution guarded
--size 11
--sizes 7,11,21
--seed 7
--path-limit 24
--max-calls 80
--max-steps 5000
--retries 1
--analysis
--transcripts-dir transcripts
```

The default sizes are `7,11,21`. Larger runs such as `--size 41` are useful stress tests, but local
models can spend a long time producing or validating paths over the full ASCII grid. `--path-limit`
keeps each oracle response short and forces the rat to replan.

Execution modes:

- `permissive` follows every legal oracle move.
- `guarded` rejects moves that do not reduce the shortest-path distance to the exit, records the
  rejection, applies one deterministic fallback move, and asks the oracle again from the new state.

Each scenario writes a JSONL transcript and prints a summary row with oracle calls, steps, mistakes,
optimal path length, and efficiency.
