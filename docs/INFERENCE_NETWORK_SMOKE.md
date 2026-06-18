# Inference Network Smoke

This recipe is manual. Do not add it to ordinary CI because it reaches Hugging Face, downloads model
files, and depends on network, cache, token, and model-card state outside the repository.

## Preconditions

- Build the CLI with native Hugging Face support.
- Set `HF_HOME` to a test cache directory, or accept the standard Hugging Face cache location.
- Set `HF_TOKEN` only when testing a gated or private model. The token is used for request auth and
  must not be copied into inventory files, job files, logs, or instance settings.
- Run `loom doctor --format json` before and after the smoke so hardware, runtime support, inventory,
  jobs, and model fit are captured.

## Rerunnable Script

Use the script for the standard operator smoke path:

```sh
scripts/inference-smoke.sh
scripts/inference-smoke.sh --cli --config
```

The default runs both the embedding and LLM smoke tests. The script runs ignored network smoke tests
by exact test name, stores Cargo build outputs under `CARGO_TARGET_DIR` or
`/tmp/loom-inference-smoke-target`, and uses the standard Hugging Face cache unless `HF_HOME` is set.
`--cli` also runs CLI download, list, and doctor checks. `--config` probes the named instance and
vector workspace binding commands with create-or-update behavior so the script can be rerun.
`LOOM_SMOKE_CLI=/path/to/loom` can point the CLI section at a prebuilt binary.

## Embedding Smoke

```sh
cargo run -p uldren-loom-cli -- inference model download \
  sentence-transformers/all-MiniLM-L6-v2 \
  config.json model.safetensors tokenizer.json tokenizer_config.json special_tokens_map.json \
  --kind text-embedding \
  --runtime candle-safetensors \
  --foreground

cargo run -p uldren-loom-cli -- inference model list \
  --kind text-embedding \
  --format json

cargo run -p uldren-loom-cli -- doctor \
  --format json
```

Expected result:

- The download job reaches `installed`.
- The installed model record contains sha256 digests for every managed file.
- Doctor reports the model as blocked until a compatible local runtime is compiled, or runnable when
  the matching runtime profile is enabled and the host has enough memory.

## LLM Smoke

```sh
cargo run -p uldren-loom-cli -- inference model download \
  Qwen/Qwen2.5-0.5B-Instruct \
  config.json generation_config.json model.safetensors tokenizer.json tokenizer_config.json vocab.json merges.txt \
  --kind llm \
  --runtime candle-safetensors \
  --foreground

cargo run -p uldren-loom-cli -- inference model list \
  --kind llm \
  --format json

cargo run -p uldren-loom-cli -- doctor \
  --format json
```

Expected result:

- The download job reaches `installed`.
- The installed model record contains sha256 digests for every managed file.
- Doctor reports runtime fit using the same compatibility reasons used by `loom inference model list
  --remote`.

## Cleanup

Inspect before deleting:

```sh
cargo run -p uldren-loom-cli -- inference model remove \
  sentence-transformers/all-MiniLM-L6-v2 \
  --kind text-embedding \
  --runtime candle-safetensors \
  --dry-run

cargo run -p uldren-loom-cli -- inference model remove \
  Qwen/Qwen2.5-0.5B-Instruct \
  --kind llm \
  --runtime candle-safetensors \
  --dry-run
```

Delete only after checking the printed cache root and paths. In shared-cache deployments, another tool
may still expect those files to exist.
