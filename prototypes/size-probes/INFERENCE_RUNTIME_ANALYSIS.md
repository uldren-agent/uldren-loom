# Inference Runtime Size Probe Analysis

This analysis uses `inference_http_baseline` as the control feature. The baseline links Tokio,
Reqwest, and the Loom rustls profile so candidate rows measure marginal size over the shared
HTTP/TLS stack.

Command:

```sh
bash prototypes/size-probes/compare-inference-runtime-size.sh
```

Baseline:

| Feature | Size | Dependencies |
| --- | ---: | ---: |
| `inference_http_baseline` | 2,952,720 bytes, 2.82 MiB | 120 |

Measured candidates:

| Candidate | Probe feature set | Size | Delta vs baseline | Dependencies | Result |
| --- | --- | ---: | ---: | ---: | --- |
| `genai` | `inference_http_baseline,genai_rustls` | 4,548,512 bytes, 4.34 MiB | +1,595,792 bytes, +1.52 MiB | 195 | Builds |
| `ollama-rs` | `inference_http_baseline,ollama_rs_rustls` | 2,952,856 bytes, 2.82 MiB | +136 bytes, +0.1 KiB | 138 | Builds |
| `ollama-rs` with streaming | `inference_http_baseline,ollama_rs_stream` | 2,987,488 bytes, 2.85 MiB | +34,768 bytes, +34.0 KiB | 146 | Builds |
| `llmfit-core` | `inference_http_baseline,llmfit_core` | 3,037,280 bytes, 2.90 MiB | +84,560 bytes, +82.6 KiB | 155 | Builds |
| `llama-cpp-2` common | `inference_http_baseline,llama_cpp_2_common` | 4,612,208 bytes, 4.40 MiB | +1,659,488 bytes, +1.58 MiB | 130 | Builds |
| `llama-cpp-2` Metal | `inference_http_baseline,llama_cpp_2_metal` | 4,612,128 bytes, 4.40 MiB | +1,659,408 bytes, +1.58 MiB | 130 | Builds |
| `mistralrs` default | `inference_http_baseline,mistralrs_default` | 3,176,584 bytes, 3.03 MiB | +223,864 bytes, +218.6 KiB | 632 | Builds |
| `apple-mlx` | `inference_http_baseline,apple_mlx` | n/a | n/a | n/a | Fails without MLX C package |

## Candidate Notes

`ollama-rs` is the smallest HTTP client candidate over the baseline. With `default-features = false`
and the `rustls` feature, it reuses the baseline `reqwest 0.12.28` dependency line. The streaming
feature adds `tokio-stream` surface and measured at about +34 KiB over the baseline.

`genai` is a broad hosted-provider client for OpenAI-style and multi-provider chat APIs. It adds
about +1.52 MiB over the baseline and brings `reqwest 0.13.4` in addition to the baseline
`reqwest 0.12.28`. That duplicate HTTP client line is a meaningful enterprise dependency cost.

`llama-cpp-2` is the practical Rust binding candidate for local GGUF and llama.cpp-backed inference.
The common and Metal probes measured nearly identical binary size in this environment, both about
+1.58 MiB over the baseline. The crate supports CPU, Metal, CUDA, Vulkan, and other native backend
features through `llama-cpp-sys-2`.

`mistralrs` is a full local inference runtime built on Candle. The linked binary delta is small in
this probe because the probe constructs model and request builders but does not load a model. The
dependency graph is the important signal: 632 normal dependencies, including Candle, tokenizers,
HF Hub, image/audio stacks, MCP support, `sysinfo`, `reqwest 0.12.28`, and `reqwest 0.13.4`.

`apple-mlx` is a direct MLX C API binding. It failed to build because CMake could not find the MLX C
package configuration. This is a system prerequisite rather than a Rust-only dependency. It should
not be treated as a portable default runtime unless Loom owns MLX installation and doctor checks.

## MLX Native Bundle Footprint

Measured on 2026-07-06 from the refreshable `mlx-c` source checkout at `fba4470`.

Command inputs:

```sh
scripts/inference-mlx-runtime.sh clean-build
scripts/inference-mlx-smoke.sh
du -sk crates/loom-inference/native/mlx/aarch64-apple-darwin target/inference/mlx/install target/inference/mlx/smoke
```

Bundle sizes:

| Artifact | Size | Notes |
| --- | ---: | --- |
| Staged runtime bundle | 176,736 KiB, 172.59 MiB | `crates/loom-inference/native/mlx/aarch64-apple-darwin`; ignored generated artifact. |
| Build install prefix | 180,916 KiB, 176.68 MiB | `target/inference/mlx/install`; source for staging. |
| Source checkout | 9,016 KiB, 8.80 MiB | `target/inference/mlx/src/mlx-c`; managed clone. |
| CMake build tree | 390,092 KiB, 380.95 MiB | `target/inference/mlx/build`; disposable clean-build output. |
| Manual smoke binary | 35,184 bytes, 34.36 KiB | Links to staged `libmlxc`, `libmlx`, and `libjaccl` through `@rpath`. |

Staged bundle contents:

| File | Size | Role |
| --- | ---: | --- |
| `mlx.metallib` | 157,748,008 bytes, 150.44 MiB | Metal shader library. Dominates the bundle. |
| `libmlx.dylib` | 21,512,976 bytes, 20.52 MiB | Upstream MLX runtime. |
| `libjaccl.dylib` | 861,888 bytes, 841.69 KiB | Local runtime dependency required by `libmlx.dylib`. |
| `libmlxc.dylib` | 836,464 bytes, 816.86 KiB | Upstream MLX C API. |
| `manifest.txt` | 2,961 bytes, 2.89 KiB | Staged linkage manifest. |
| `bundle.env` | 265 bytes | Runtime path helper. |

The staged raw MLX C bundle is not a default Loom dependency and is not checked into git. It is a
large optional Apple artifact that must stay refreshable and diagnosable. The future
`libloom_mlx_adapter.dylib` will add size on top of this bundle, but the current lower bound is
already about 173 MiB because of `mlx.metallib`.

The manual smoke loaded the staged libraries and reported MLX version `0.31.2` in this Codex
session, but returned exit code 77 before the array operation because MLX could not access a Metal
device from the headless or sandboxed host session.

`llmfit-core` is not an inference runtime. It is useful as a hardware and model-fit analysis
candidate. It adds about +83 KiB over the baseline in this probe, but Loom already chose `sysinfo`
for the long-term doctor substrate.

The high-level `llama_cpp` crate was not kept in this probe manifest because it conflicts with
`llama-cpp-2`: both native sys crates declare the same native `links = "llama"` target. Measuring both
requires isolated probe manifests.

## Design Inputs

Remote model download remains separate from inference runtime selection. `hf-hub` is still the
enterprise choice for Hugging Face cache semantics, but its async profile brings Reqwest indirectly.
That dependency is now part of the baseline when evaluating inference runtime choices.

The runtime API must expose resolved settings, not hidden defaults. The probed runtimes expose
settings such as temperature, top-k, top-p, min-p, maximum generated tokens, stop tokens, penalties,
keep-alive, model loading profile, quantization, device placement, context behavior, and streaming.

The CLI should distinguish model artifacts from usable configured instances. Artifact management
answers "what is downloaded or available"; instance management answers "what named configuration can
facets use".

## Accepted Runtime Split

Loom owns the public instance and settings schema. Runtime and HTTP crates are implementation
details behind Loom adapters.

The accepted split is:

| Area | Library choice | Reason |
| --- | --- | --- |
| Hugging Face downloads | `hf-hub` with Tokio and rustls | Standard cache semantics, revision handling, and Hub-specific behavior. |
| Hosted and multi-provider APIs | `genai` | Broad OpenAI, Anthropic, Gemini, Ollama-compatible, Bedrock, Vertex, OpenRouter, and related API shape coverage. |
| Ollama local daemon | `ollama-rs` | Small measured footprint, MIT license, baseline Reqwest reuse, Ollama-native settings, and local model lifecycle endpoints. |
| Native GGUF | Candle GGUF first, optional `llama-cpp-2` | Candle keeps the Rust-first path; `llama-cpp-2` covers broader GGUF/runtime compatibility when needed. |
| Apple MLX | optional MLX profile | Apple-only runtime with MLX C system prerequisite. |

`genai` does not replace `ollama-rs` for Ollama. `genai` normalizes provider calls and intentionally
maps only a portable subset of options into Ollama. `ollama-rs` exposes Ollama-native options such as
`mirostat`, `num_ctx`, `num_gpu`, `num_thread`, `repeat_last_n`, `repeat_penalty`, `temperature`,
`seed`, `stop`, `tfs_z`, `num_predict`, `top_k`, `top_p`, `min_p`, plus an extra settings map.

Decision Points: none in this measurement artifact.
