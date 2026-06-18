# AI Assistant / Agent Design Patterns — A Builder's Catalogue (2026)

**Context:** patterns and techniques for building a new **desktop app** that calls an LLM via the **OpenAI API**. Code examples are **TypeScript/Node** (OpenAI Node SDK + the `@openai/agents` SDK + Zod).

**How to read this:** patterns are grouped from "control flow" → "context" → "orchestration" → "reliability/safety/cost" → "desktop-specific". Each pattern has *what it is*, *when to use it*, and (where useful) a code sketch. You don't need all of them — see the [decision guide](#13-decision-guide-pick-the-smallest-thing-that-works) and [starter architecture](#14-recommended-starter-architecture-for-a-new-desktop-app) at the end.

> **The single most important principle** (Anthropic, *Building Effective Agents*): **start with the simplest architecture that could plausibly work.** A single LLM call with good retrieval and tools beats an elaborate multi-agent system for most tasks. Add complexity only when it measurably improves outcomes.

---

## Table of contents

1. [Foundations: the OpenAI primitives you build on](#1-foundations-the-openai-primitives-you-build-on)
2. [Core reasoning / control-flow patterns](#2-core-reasoning--control-flow-patterns)
3. [Workflow composition patterns (Anthropic's five)](#3-workflow-composition-patterns-anthropics-five)
4. [Tool use & function-calling patterns](#4-tool-use--function-calling-patterns)
5. [Context & memory engineering](#5-context--memory-engineering)
6. [Retrieval (RAG) patterns](#6-retrieval-rag-patterns)
7. [Multi-agent orchestration](#7-multi-agent-orchestration)
8. [Reliability, evals & observability](#8-reliability-evals--observability)
9. [Safety & security](#9-safety--security)
10. [Cost & latency](#10-cost--latency)
11. [Desktop-app-specific architecture](#11-desktop-app-specific-architecture)
12. [Anti-patterns to avoid](#12-anti-patterns-to-avoid)
13. [Decision guide](#13-decision-guide-pick-the-smallest-thing-that-works)
14. [Recommended starter architecture](#14-recommended-starter-architecture-for-a-new-desktop-app)
15. [Sources](#15-sources)

---

## 1. Foundations: the OpenAI primitives you build on

Before patterns, know the three API surfaces. Picking the right base layer saves you from re-implementing agent plumbing.

| Surface | What it is | Use when |
|---|---|---|
| **Chat Completions** | The classic `messages` → `message` call. Stateless, you manage history. | Simple single-turn or you want full control over the loop. |
| **Responses API** | Agentic-by-default successor. One request can call multiple **hosted tools** (web search, file search, code interpreter, computer use, image gen, remote MCP) plus your custom functions, and chains tool calls server-side. Stores state via `previous_response_id`. | Most new apps — less plumbing, server-managed tool loop. |
| **Agents SDK (`@openai/agents`)** | A thin orchestration framework over the above: `Agent`, `tool()`, `handoff`, guardrails, sessions, tracing, human-in-the-loop, realtime/voice. Tools get **Zod-powered schema generation** automatically. | You want multi-agent, handoffs, guardrails, and tracing without writing the loop yourself. |

**Models (mid-2026 landscape):** `gpt-5.1` is the flagship for coding/agentic tasks with **configurable reasoning** (`reasoning.effort`: `none` | `low` | `medium` | `high`). `gpt-5.5` / `gpt-5.5 Pro` are the newest general-reasoning models. Smaller variants (e.g. `gpt-5.4 mini` / `nano`) trade capability for latency and cost. **Pattern:** don't hardcode one model — route by task (see [§10](#10-cost--latency)).

```ts
import OpenAI from "openai";
const client = new OpenAI(); // reads OPENAI_API_KEY from env

// Responses API — the agentic default
const res = await client.responses.create({
  model: "gpt-5.1",
  reasoning: { effort: "low" },
  input: "Summarize today's standup notes.",
});
console.log(res.output_text);
```

---

## 2. Core reasoning / control-flow patterns

These govern *how a single agent thinks and acts*. They form a ladder of increasing capability and cost.

### 2.1 The Agent Loop / ReAct (Reason + Act)

**What:** the foundational pattern. The model interleaves **Thought → Action (tool call) → Observation (tool result)** in a loop until it produces a final answer. It is the safe default for any tool-using agent.

**When:** essentially every interactive assistant. Start here.

```ts
import OpenAI from "openai";
const client = new OpenAI();

const tools = [{
  type: "function" as const,
  name: "search_files",
  description: "Full-text search the user's local notes.",
  strict: true,
  parameters: {
    type: "object",
    properties: { query: { type: "string" } },
    required: ["query"],
    additionalProperties: false,
  },
}];

async function runAgentLoop(userInput: string) {
  let input: any[] = [{ role: "user", content: userInput }];
  for (let step = 0; step < 10; step++) {           // ALWAYS cap the loop
    const res = await client.responses.create({ model: "gpt-5.1", tools, input });
    const calls = res.output.filter((o: any) => o.type === "function_call");
    if (calls.length === 0) return res.output_text;  // model is done
    input = input.concat(res.output);
    for (const call of calls) {                      // execute + feed back
      const args = JSON.parse(call.arguments);
      const result = await dispatchTool(call.name, args);
      input.push({ type: "function_call_output", call_id: call.call_id,
                   output: JSON.stringify(result) });
    }
  }
  throw new Error("Agent exceeded step budget"); // loop guard — see §8
}
```

**Key implementation notes:** always bound the loop (`max steps`), feed tool *errors* back as observations (the model can recover), and stream tokens to the UI as they arrive.

### 2.2 Reflection / Self-Critique

**What:** after producing a draft, the model critiques its own output against criteria, then revises. One generate step + one critique-and-revise step.

**When:** writing, code, or any output with a definable quality bar. Cheap quality boost.

```ts
const draft = await complete(`Write the function:\n${spec}`);
const critique = await complete(
  `Review this code for bugs, edge cases, and the spec. List concrete issues:\n${draft}`);
const final = await complete(
  `Revise the code to fix every issue.\nCode:\n${draft}\nIssues:\n${critique}`);
```

### 2.3 Reflexion

**What:** Reflection extended into a *loop with memory*. The agent acts, evaluates against a success signal, writes a **verbal self-critique** into memory, and retries — learning from prior attempts within the session.

**When:** tasks with a clear automatic success signal (tests pass, schema validates, tool returns OK). The practical rule: **start with ReAct, add Reflexion once you have a signal to evaluate against.**

### 2.4 Plan-and-Execute (Plan-then-Execute)

**What:** explicitly decouple **planning** from **execution**. One LLM (or call) produces a full multi-step plan; a separate executor carries out each step. Reduces drift on long-horizon tasks and makes the plan inspectable/approvable.

**When:** multi-step goals where ReAct tends to wander; when you want a human to approve the plan before execution; when steps are parallelizable.

```ts
const Plan = z.object({ steps: z.array(z.object({
  id: z.number(), action: z.string(), tool: z.string().nullable() })) });
const plan = await parseStructured(`Make a step-by-step plan for: ${goal}`, Plan);
// optional: show plan to user for approval (HITL, §9)
for (const step of plan.steps) await executeStep(step);
```

> **Security note:** Plan-then-Execute is also a hardening pattern — a fixed plan limits how far an injected instruction can redirect the agent mid-run. See [§9](#9-safety--security).

### 2.5 Search-based reasoning: Tree of Thoughts (ToT), LATS, Graph of Thoughts

**What:** instead of one linear chain, explore *multiple* reasoning branches and select the best. **ToT** branches and prunes; **LATS** (Language Agent Tree Search) adds MCTS-style search over actions with backtracking; **Graph of Thoughts** generalizes to a DAG.

**When:** hard problems where a wrong early step is fatal and you can afford many model calls (planning, math, complex code). **Expensive** — use sparingly and only when single-pass quality is insufficient.

### 2.6 Self-Discover

**What:** the model first *composes its own reasoning structure* (selects/combines reasoning modules) for the task class, then applies it. Cheaper than full tree search, better than fixed CoT for novel task types.

---

## 3. Workflow composition patterns (Anthropic's five)

Anthropic's *Building Effective Agents* draws a useful line: **workflows** orchestrate LLM calls along predefined paths (predictable, debuggable); **agents** decide their own path (flexible, harder to control). These five workflow patterns sit on a complexity ladder — chaining/routing are easy; orchestrator and evaluator are powerful but costly.

### 3.1 Prompt Chaining
Each call processes the previous call's output (e.g. outline → draft → polish). Use when a task cleanly decomposes into fixed sequential steps. Add a **gate** between steps to validate before continuing.

### 3.2 Routing
A classifier LLM (or cheap model) labels the input and dispatches it to a specialized prompt/model/tool. Use for distinct input categories (support tiers, "code vs chat vs search"). Keeps each prompt focused and lets you send easy queries to cheap models.

```ts
const Route = z.object({ category: z.enum(["billing","technical","general"]),
                         complexity: z.enum(["simple","complex"]) });
const r = await parseStructured(query, Route);
const model = r.complexity === "simple" ? "gpt-5.4-mini" : "gpt-5.1";
return handlers[r.category](query, model);
```

### 3.3 Parallelization
Run independent subtasks (or multiple votes on the same task) concurrently, then aggregate. Two flavors: **sectioning** (split work) and **voting** (sample N, take consensus). Use when speed matters or independent perspectives improve quality.

### 3.4 Orchestrator–Workers
A central LLM dynamically decomposes a task and delegates subtasks to worker LLMs, then synthesizes. Unlike parallelization, the subtasks aren't known up front. Use for tasks of unpredictable shape (e.g. "refactor across these files").

### 3.5 Evaluator–Optimizer
One LLM generates, another evaluates and gives feedback, looping until the evaluator passes. Use when you have clear evaluation criteria and iteration demonstrably helps (translation nuance, complex search). This is the workflow form of Reflexion.

### 3.6 Deterministic / zero-LLM orchestration
Not every branch needs a model. Use plain code (`if/switch`, state machines, queues) for control flow and reserve LLM calls for the genuinely fuzzy steps. **Cheaper, faster, fully testable.** A surprising amount of "agent" logic should be ordinary code.

---

## 4. Tool use & function-calling patterns

### 4.1 Strict structured outputs (Schema-First / Zod-first)
**The production default.** Constrain model output to a JSON Schema so every field is present and correctly typed — the model *cannot* emit non-conforming output. Define the schema in **Zod** (TS) once and derive both the API format and your runtime type. JSON mode (`json_object`) is legacy: it guarantees valid JSON syntax but **not** schema adherence.

```ts
import { z } from "zod";
import { zodTextFormat } from "openai/helpers/zod";

const Invoice = z.object({
  vendor: z.string(),
  total: z.number(),
  due_date: z.string(),
  line_items: z.array(z.object({ desc: z.string(), amount: z.number() })),
});

const res = await client.responses.parse({
  model: "gpt-5.1",
  input: [{ role: "user", content: rawText }],
  text: { format: zodTextFormat(Invoice, "invoice") },
});
const invoice = res.output_parsed; // fully typed, schema-guaranteed
```

Strict-mode rules to remember: `additionalProperties: false`, every property listed in `required`, and model optional fields with a nullable type rather than omitting them. Always still handle two cases: a **refusal** and a **truncated** (length-capped) response.

### 4.2 Function/tool calling with the Agents SDK
The SDK turns any TS function into a tool with auto-generated, Zod-validated schemas — far less boilerplate than hand-writing JSON Schema.

```ts
import { Agent, run, tool } from "@openai/agents";
import { z } from "zod";

const getWeather = tool({
  name: "get_weather",
  description: "Current weather for a city.",
  parameters: z.object({ city: z.string() }),
  execute: async ({ city }) => fetchWeather(city),
});

const agent = new Agent({
  name: "Assistant",
  instructions: "You are a concise desktop assistant.",
  tools: [getWeather],
});

const result = await run(agent, "What's the weather in Lisbon?");
console.log(result.finalOutput);
```

### 4.3 Tool design for LLMs (LLM-friendly APIs)
Tools are a UX surface *for the model*. Design accordingly: small focused tools over giant multiplexers; descriptive names and parameter docs; return **structured, terse** results (not raw HTML/giant blobs); and make **error messages instructive** ("file not found; did you mean X?") so the model self-corrects on the next loop step. Treat tool ergonomics as seriously as your prompts.

### 4.4 Parallel tool calls
Let the model request several independent tool calls in one turn and execute them concurrently. Big latency win for fan-out reads (search + calendar + files). Note: OpenAI strict structured *function* outputs and parallel calls have an interaction — test the combination.

### 4.5 Progressive tool discovery / tool-search lazy loading
Don't stuff 80 tool definitions into every prompt (it bloats context and confuses selection). Instead expose a **`search_tools`** meta-tool, or load tool subsets by phase/route. The agent discovers the tools it needs on demand. Critical once you exceed ~10–20 tools.

### 4.6 Code mode / Code-over-API
Instead of many granular tool calls, give the model a sandboxed **code execution** tool and let it write code that calls your APIs. Often fewer round-trips and more expressive than chaining dozens of discrete tool calls. (OpenAI's hosted code interpreter, or your own sandbox.)

### 4.7 MCP (Model Context Protocol)
The emerging standard for exposing tools/data to agents. The Responses API and Agents SDK can connect to local or remote **MCP servers**, so you integrate a capability once and reuse it across apps. Good for a desktop app that wants a pluggable tool ecosystem. (Mind the security implications — [§9](#9-safety--security).)

### 4.8 Unified tool gateway / policy-gated tool proxy
Route every tool call through one internal gateway that handles auth, rate limits, logging, retries, and **policy checks** (allow/deny, argument validation). Centralizes governance and is where you enforce human approval and the lethal-trifecta defenses.

---

## 5. Context & memory engineering

> *Prompt engineering asks "how to ask." **Context engineering** asks "what does the agent know, see, and remember at the moment of action?"* The model has a finite attention budget; every token competes. As context grows, precision drops. This discipline is now as important as prompting.

The canonical framework is four strategies: **Write, Select, Compress, Isolate.**

### 5.1 Write — externalize state
Persist working state *outside* the context window: a **scratchpad** of notes, a **todo list** the agent maintains, or **filesystem-based state**. Lets the agent track progress and survive across steps without re-reading everything.

### 5.2 Select — pull in only what's relevant
Dynamically retrieve the right context per step (semantic search, relevance scoring, metadata filters) instead of dumping everything. This is RAG applied to the agent's own memory/history. See [§6](#6-retrieval-rag-patterns).

### 5.3 Compress — compaction & summarization
When the transcript would overflow, **compact** it: summarize older turns, drop redundant tool output, keep the load-bearing facts. This is what lets a long-running agent keep going. The art is compressing *without* dropping the one detail that matters three steps later — protect IDs, decisions, and constraints.

```ts
function maybeCompact(history: Msg[], budgetTokens: number): Msg[] {
  if (estimateTokens(history) < budgetTokens) return history;
  const recent = history.slice(-6);                  // keep recent turns verbatim
  const summary = summarizeWithCheapModel(history.slice(0, -6)); // gpt-5.4-mini
  return [{ role: "system", content: `Summary so far:\n${summary}` }, ...recent];
}
```

### 5.4 Isolate — separate windows per subtask
Give each subtask its own clean context via separate agent processes/sub-agents. A supervisor decomposes the job and hands each worker a narrow window scoped to its piece, so noise from one task doesn't pollute another. (Connects to multi-agent, [§7](#7-multi-agent-orchestration).)

### 5.5 Prompt caching via stable prefix
**Put static content first** (system prompt, instructions, tool defs, few-shot examples), variable/user content **last**. OpenAI caches exact prefixes ≥1024 tokens automatically — up to ~80% lower latency and ~90% lower input cost on hits, no code change. Set a `prompt_cache_key` to improve hit routing. This single layout choice is one of the highest-ROI patterns for a chatty desktop app.

### 5.6 Memory types: episodic & semantic
- **Episodic memory:** retrieve relevant past interactions/events and inject them ("last time you preferred X").
- **Semantic memory:** durable facts about the user/project in a store, surfaced on demand.
- **Memory synthesis:** periodically distill execution logs into reusable memories/skills.
For a desktop app, store these **locally** (SQLite + a vector index) for privacy and offline use.

---

## 6. Retrieval (RAG) patterns

RAG in 2026 is a *spectrum*, not one technique. Match pipeline complexity to query complexity.

### 6.1 Hybrid search + reranking (ship this first)
Combine **dense** (embeddings) and **sparse/keyword** (BM25) retrieval so you never miss exact-term matches, then run a **cross-encoder reranker** to float the best chunks to the top. If retrieval is "missing the obvious," you usually don't need an agent — you need hybrid + rerank. Compress/trim chunks to fit the budget.

### 6.2 Adaptive routing
Classify query complexity and route: simple lookups → single retrieval pass; hard/multi-hop → the full agentic or graph pipeline. Keeps cost low for the common case.

### 6.3 Agentic RAG
Replace the linear "retrieve → generate" pipeline with an agent that can **plan, retrieve, evaluate sufficiency, and re-retrieve in a loop** — and decompose multi-part questions. Use when single-shot retrieval quality is inadequate and queries are genuinely complex. Costs more per query; reserve for when it earns its keep.

### 6.4 Graph / schema-guided retrieval
For multi-hop reasoning over connected data, retrieve along a knowledge graph or schema rather than flat chunks. Higher build cost, better for relationship-heavy questions.

---

## 7. Multi-agent orchestration

Reach for this only after a single agent with good tools/context is proven insufficient. The benefit is **context isolation** and specialization; the cost is latency, token spend, and debugging difficulty.

### 7.1 Supervisor (the 2026 default)
A coordinator agent owns the conversation and delegates to specialized sub-agents (e.g. researcher + coder + reviewer), synthesizing their results. Claude Code subagents, LangGraph Supervisor, and the OpenAI Agents SDK all converge here. Start with this before fancier topologies.

### 7.2 Handoffs / Swarm
Agents transfer control to each other; a handoff is literally "a function that returns another agent." Lightweight and good for routing between specialists (triage → billing → technical). The OpenAI Agents SDK models this explicitly.

```ts
import { Agent, run } from "@openai/agents";

const billing = new Agent({ name: "Billing", instructions: "Handle billing." });
const technical = new Agent({ name: "Technical", instructions: "Handle tech issues." });

const triage = new Agent({
  name: "Triage",
  instructions: "Route the user to the right specialist.",
  handoffs: [billing, technical],
});

const result = await run(triage, "My invoice is wrong and the app won't sync.");
```

### 7.3 Hierarchical (supervisors of supervisors)
For genuinely large workflows: a top supervisor coordinates mid-level supervisors that each own a domain. Powerful, operationally heavy — use only when complexity demands it.

### 7.4 Debate / opponent-processor
Two or more agents argue opposing positions; a judge synthesizes. Improves correctness/robustness on contestable questions at the cost of extra calls.

### 7.5 Map-Reduce / parallel exploration
Fan a task out across many workers (map), then combine (reduce). Great for "summarize 200 documents" or parallel search.

---

## 8. Reliability, evals & observability

Agents fail in ways model benchmarks don't capture. Most incidents are **tool-call failures, context truncation, and runaway loops** — not raw model errors. Build the safety net from day one.

### 8.1 The eval ladder
Three layers, increasing realism: **unit evals** on discrete steps (does this tool/parse work?); **LLM-as-judge regression suites** for subjective quality (graded against criteria); **production trace sampling** to catch real-world drift.

### 8.2 Eval-driven development loop
- **Incident-to-eval:** every production failure becomes a new eval case, so the suite grows from real behavior and regressions are caught automatically.
- **CI gating:** wire LLM-as-judge evals into CI and **block deploys when scores regress.**
- **Workflow evals with mocked tools:** test agent logic deterministically by mocking tool responses, so evals are fast and don't hit live systems.

### 8.3 Tracing & observability
Capture every step: prompts, tool calls + args + results, token counts, latency, cost, and final output. Use **OpenTelemetry GenAI semantic conventions** to stay vendor-neutral. The Agents SDK has built-in tracing; platforms like Langfuse (OSS baseline), LangSmith, and Braintrust add eval/observability tooling.

### 8.4 Runtime guardrails on the loop
- **Loop/step budget** and **wall-clock timeout** (kill runaway agents).
- **Agent circuit breaker:** trip and stop after repeated failures/cost thresholds.
- **Schema-validation retry:** on a bad/parse-failed tool arg, return the validation error to the model and let it retry (with a cap).
- **Failover-aware model fallback:** on provider error/rate limit, fall back to an alternate model/provider.

```ts
async function withGuards<T>(fn: () => Promise<T>, opts = { retries: 2, timeoutMs: 60000 }) {
  for (let i = 0; i <= opts.retries; i++) {
    try {
      return await Promise.race([
        fn(),
        new Promise<never>((_, rej) => setTimeout(() => rej(new Error("timeout")), opts.timeoutMs)),
      ]);
    } catch (e) {
      if (i === opts.retries) throw e;
      await new Promise(r => setTimeout(r, 2 ** i * 500)); // exponential backoff
    }
  }
  throw new Error("unreachable");
}
```

---

## 9. Safety & security

Especially important for a **desktop app** that can touch local files, the user's data, and the network at once.

### 9.1 The Lethal Trifecta (read this before shipping tools)
Coined by Simon Willison: an agent becomes a near-guaranteed exfiltration risk when it combines **(1) access to private data + (2) exposure to untrusted content + (3) the ability to communicate externally.** Because LLMs can't reliably separate trusted instructions from untrusted data (both are just tokens), malicious instructions hidden in fetched content can hijack the agent. **Mitigation:** break the trifecta — if the agent reads untrusted web/email content, restrict its outbound/exfiltration channels and its access to secrets; don't grant all three at once.

### 9.2 Human-in-the-loop (HITL) approval
Pause for human approval at defined high-risk decision points (send email, delete files, spend money, run shell). The Agents SDK supports pausing a tool call, awaiting approval/rejection, and **resuming from the same state**. Gate by risk so you don't bottleneck on low-stakes actions — auto-approve reads, confirm writes/destructive ops.

```ts
const deleteFile = tool({
  name: "delete_file",
  parameters: z.object({ path: z.string() }),
  needsApproval: true, // SDK pauses; your UI confirms before execution
  execute: async ({ path }) => fs.rm(path),
});
```

### 9.3 Dual-LLM / quarantined-content pattern
Use a **privileged** LLM that never sees raw untrusted text and an **isolated** LLM that processes untrusted content but has no tool access. The privileged one orchestrates via symbolic references, so injected instructions in the untrusted data can't trigger privileged actions.

### 9.4 Sandboxing, egress lockdown, policy-gated proxy
Run tool execution (shell, code, file ops) in a sandbox with least privilege. **Egress lockdown:** restrict outbound network so a compromised agent can't phone home. Route tools through a **policy-gated proxy** that validates arguments and enforces allow/deny lists. **PII tokenization:** replace sensitive values with tokens before they hit the model, detokenize after.

### 9.5 Prompt-injection hygiene
Treat all retrieved/tool content as untrusted. Keep system instructions separate and high-priority, label external content clearly, and prefer **Plan-then-Execute** (a fixed plan limits how far an injection can redirect a run).

---

## 10. Cost & latency

### 10.1 Model routing / right-sizing
Send easy turns to a small/cheap model and hard turns to the flagship; use `reasoning.effort: "none"|"low"` for quick tasks and `"high"` only when needed. Implement **budget-aware routing with hard cost caps** so a session can't blow the budget.

### 10.2 Prompt caching (highest ROI)
See [§5.5](#55-prompt-caching-via-stable-prefix). Stable prefix + `prompt_cache_key` → big automatic savings on every repeated system/tool preamble.

### 10.3 Streaming
Stream tokens to the UI (`stream: true`) for perceived latency — the user sees output immediately. Essential UX for a desktop assistant; also enables early cancellation.

### 10.4 Batching (Batch API)
For non-interactive bulk work (embedding a corpus, nightly summarization), use the **Batch API** for much higher throughput at lower per-item cost. Don't use it on the interactive hot path.

### 10.5 Semantic / response caching
Cache responses to semantically similar requests (L1 in-memory + L2 persistent). Cuts cost and latency for FAQ-like traffic. Be careful to scope caches per user/context to avoid leaking.

---

## 11. Desktop-app-specific architecture

This is where a desktop assistant differs most from a server-side agent.

### 11.1 Never embed your API key in the client (the #1 rule)
A key shipped in the app binary or WebView can be extracted and abused. Two acceptable models:
- **BYO-key (local):** the *user* supplies their own OpenAI key; store it in the **OS keychain/credential store**, never in plaintext config. Requests go straight from the user's machine with their key. Good for power-user/dev tools.
- **Backend proxy:** route requests through *your* server, which holds the key, authenticates the user, and enforces rate/cost limits. Good for consumer apps and when you want central control. The desktop app never sees the provider key.

### 11.2 Framework: Tauri vs Electron
- **Tauri (Rust + system WebView):** smaller binaries, lower memory, sandboxed; system APIs are exposed only through a vetted Rust bridge with an explicit permission model — smaller attack surface. There's a first-class secure-storage/keychain story.
- **Electron (bundled Chromium):** richer ecosystem and familiarity, heavier footprint. Use the **main process** as your trusted boundary (hold secrets there, never in the renderer), and `contextIsolation`.

Either way: **secrets and provider calls live in the trusted process (Rust core / Electron main), never in the renderer/WebView.**

### 11.3 Local loopback proxy for streaming
A common Tauri/Electron pattern: run a **loopback-only** HTTP server (e.g. `127.0.0.1`, never bound to the network) in the trusted process. The WebView calls localhost; the Rust/Node core injects the key from the keychain and proxies to OpenAI. **Stream properly** — forward SSE/events token-by-token rather than buffering the whole response (a frequent bug in naive proxies).

### 11.4 Streaming UI + cancellation
Wire streamed deltas into the UI and support **abort** (user hits stop) via `AbortController`, cancelling the in-flight request to stop token spend.

```ts
const controller = new AbortController();
const stream = await client.responses.create(
  { model: "gpt-5.1", input, stream: true },
  { signal: controller.signal },
);
for await (const event of stream) {
  if (event.type === "response.output_text.delta") appendToUI(event.delta);
}
// stopButton.onclick = () => controller.abort();
```

### 11.5 Local persistence & memory
Store threads, settings, and memory locally (SQLite; a local vector index for semantic memory/RAG). Benefits: privacy, offline history, instant startup. Encrypt sensitive stores.

### 11.6 Offline / local-model fallback
Optionally bundle or detect a local model (e.g. via llama.cpp/Ollama) for offline use or privacy-sensitive tasks, and fall back to the OpenAI API when online or when quality demands it. Abstract the model behind one interface so the rest of the app is provider-agnostic.

### 11.7 Background / async agents
Long-running tasks (indexing, research) should run as **background jobs** in the trusted process with progress events to the UI, not block the chat thread. Provide a **seamless background-to-foreground handoff** so the user can check in.

---

## 12. Anti-patterns to avoid

- **Multi-agent everything.** Reaching for a 5-agent system when one well-equipped agent would do — you pay latency, cost, and debugging tax for no quality gain.
- **No loop guard.** Letting the agent loop without a step/time/cost cap → runaway spend.
- **API key in the renderer/binary.** Covered above; this is the cardinal desktop sin.
- **JSON mode instead of strict structured outputs.** You'll fight schema drift forever.
- **Dumping all tools/context every call.** Bloats tokens, hurts tool selection, breaks caching.
- **Granting the lethal trifecta by default.** Private data + untrusted content + outbound network with no isolation.
- **No evals.** "It looked fine in the demo" is not a quality bar; without evals you can't tell if a prompt/model change regressed.
- **Buffering streams.** Kills the perceived-latency advantage of a desktop app.

---

## 13. Decision guide: pick the smallest thing that works

| If you need to… | Use |
|---|---|
| Answer with tools, interactively | **ReAct agent loop** ([2.1](#21-the-agent-loop--react-reason--act)) |
| Improve output quality cheaply | **Reflection** → **Reflexion** when you have a signal ([2.2](#22-reflection--self-critique)/[2.3](#23-reflexion)) |
| Handle a long, multi-step goal | **Plan-and-Execute** ([2.4](#24-plan-and-execute-plan-then-execute)) |
| Send inputs to specialized handlers | **Routing** ([3.2](#32-routing)) / **Handoffs** ([7.2](#72-handoffs--swarm)) |
| Speed up independent subtasks | **Parallelization** ([3.3](#33-parallelization)) / **Map-Reduce** ([7.5](#75-map-reduce--parallel-exploration)) |
| Decompose unpredictable work | **Orchestrator–Workers** ([3.4](#34-orchestratorworkers)) / **Supervisor** ([7.1](#71-supervisor-the-2026-default)) |
| Iterate to a quality bar | **Evaluator–Optimizer** ([3.5](#35-evaluatoroptimizer)) |
| Get reliable typed data out | **Strict structured outputs (Zod-first)** ([4.1](#41-strict-structured-outputs-schema-first--zod-first)) |
| Fix bad retrieval | **Hybrid search + reranker** ([6.1](#61-hybrid-search--reranking-ship-this-first)) |
| Keep long sessions coherent | **Compaction + scratchpad + prompt caching** ([5](#5-context--memory-engineering)) |
| Cut cost/latency | **Model routing + prompt caching + streaming** ([10](#10-cost--latency)) |
| Allow risky actions safely | **HITL approval + sandboxing** ([9.2](#92-human-in-the-loop-hitl-approval)/[9.4](#94-sandboxing-egress-lockdown-policy-gated-proxy)) |

---

## 14. Recommended starter architecture for a new desktop app

A pragmatic stack that uses the smallest set of patterns and grows cleanly:

1. **Shell:** Tauri (or Electron) with secrets + all OpenAI calls in the **trusted core**; renderer talks to a **loopback proxy** ([11.1](#111-never-embed-your-api-key-in-the-client-the-1-rule)–[11.3](#113-local-loopback-proxy-for-streaming)).
2. **API layer:** OpenAI **Responses API** or **Agents SDK**; abstract behind one interface so model/provider is swappable ([1](#1-foundations-the-openai-primitives-you-build-on)).
3. **Core loop:** a single **ReAct agent** with a bounded loop, **streaming** to the UI, and **abort** support ([2.1](#21-the-agent-loop--react-reason--act), [11.4](#114-streaming-ui--cancellation)).
4. **Tools:** a handful of well-described **Zod-typed function tools** behind a **policy-gated gateway**; **HITL approval** on destructive ones ([4](#4-tool-use--function-calling-patterns), [9.2](#92-human-in-the-loop-hitl-approval)).
5. **Outputs:** **strict structured outputs** wherever you parse model output ([4.1](#41-strict-structured-outputs-schema-first--zod-first)).
6. **Context:** stable cached prefix; **compaction** for long chats; local **SQLite + vector index** for memory/RAG ([5](#5-context--memory-engineering), [6.1](#61-hybrid-search--reranking-ship-this-first)).
7. **Cost:** **model routing** (cheap for easy turns) + **prompt caching** ([10](#10-cost--latency)).
8. **Reliability:** loop/timeout/cost guards, **tracing**, and a small **eval suite** in CI from day one ([8](#8-reliability-evals--observability)).
9. **Security:** respect the **lethal trifecta**; sandbox tool execution; never ship the key in the client ([9](#9-safety--security)).

Then add multi-agent, agentic RAG, or tree search **only** when evals show a single-agent baseline isn't enough.

---

## 15. Sources

Agent reasoning patterns (ReAct, Plan-and-Execute, Reflexion, ToT):
- [Agent Design Patterns: ReAct, Reflexion, Plan-and-Execute — Inductivee](https://inductivee.com/blog/autonomous-agent-design-patterns)
- [5 AI Agent Design Patterns to Master by 2026 — n1n.ai](https://explore.n1n.ai/blog/5-ai-agent-design-patterns-master-2026-2026-03-21)
- [Agentic Reasoning Patterns: ReAct, Reflexion & ToT (2026) — ServicesGround](https://servicesground.com/blog/agentic-reasoning-patterns/)
- [LLM Agent Architectures in 2026 — Future AGI](https://futureagi.com/blog/llm-agent-architectures-core-components/)
- [The Definitive Guide to Agentic Design Patterns in 2026 — SitePoint](https://www.sitepoint.com/the-definitive-guide-to-agentic-design-patterns-in-2026/)
- [Architecting Resilient LLM Agents: Secure Plan-then-Execute (arXiv)](https://arxiv.org/pdf/2509.08646)

Workflow patterns (Anthropic's five):
- [Building Effective AI Agents — Anthropic](https://www.anthropic.com/research/building-effective-agents)
- [Anthropic thinks you should build agents like this — aihero.dev](https://www.aihero.dev/building-effective-agents)

Pattern catalogues (GitHub):
- [nibzard/awesome-agentic-patterns](https://github.com/nibzard/awesome-agentic-patterns) ([website](https://agentic-patterns.com))
- [gtzheng/Awesome-Agentic-System-Design](https://github.com/gtzheng/Awesome-Agentic-System-Design)
- [ai-enhanced-engineer/agentic-design-patterns](https://github.com/ai-enhanced-engineer/agentic-design-patterns)
- [VoltAgent/awesome-ai-agent-papers (2026)](https://github.com/VoltAgent/awesome-ai-agent-papers)
- [promptadvisers/agentic-design-patterns-docs (21 patterns)](https://github.com/promptadvisers/agentic-design-patterns-docs)

Context engineering & memory:
- [Context Engineering for Agents — LangChain](https://www.langchain.com/blog/context-engineering-for-agents)
- [Context Engineering: Agent Reliability Playbook 2026 — DigitalApplied](https://www.digitalapplied.com/blog/context-engineering-agent-reliability-playbook-2026)
- [Context Engineering Guide — Mem0](https://mem0.ai/blog/context-engineering-ai-agents-guide)
- [Active Context Compression (arXiv 2601.07190)](https://arxiv.org/abs/2601.07190)

OpenAI APIs (function calling, structured outputs, Responses, Agents SDK, models):
- [Function calling — OpenAI](https://developers.openai.com/api/docs/guides/function-calling)
- [Structured model outputs — OpenAI](https://developers.openai.com/api/docs/guides/structured-outputs)
- [Migrate to the Responses API — OpenAI](https://platform.openai.com/docs/guides/migrate-to-responses)
- [Agents SDK guide — OpenAI](https://developers.openai.com/api/docs/guides/agents)
- [OpenAI Agents SDK (JS/TS) docs](https://openai.github.io/openai-agents-js/) · [GitHub](https://github.com/openai/openai-agents-js) · [Tools guide](https://openai.github.io/openai-agents-js/guides/tools/)
- [GPT-5.1 model — OpenAI](https://developers.openai.com/api/docs/models/gpt-5.1) · [GPT-5.5](https://developers.openai.com/api/docs/models/gpt-5.5) · [Models](https://developers.openai.com/api/docs/models)
- [Prompt caching — OpenAI](https://developers.openai.com/api/docs/guides/prompt-caching) · [Latency optimization](https://developers.openai.com/api/docs/guides/latency-optimization)

Multi-agent orchestration:
- [Multi-Agent Orchestration: 5 Patterns That Work in 2026 — DigitalApplied](https://www.digitalapplied.com/blog/multi-agent-orchestration-5-patterns-that-work)
- [OpenAI Swarm guide (2026) — Morph](https://www.morphllm.com/openai-swarm)
- [Multi-Agent Orchestration Frameworks 2026 — Presenc AI](https://presenc.ai/research/multi-agent-orchestration-frameworks-2026)

RAG:
- [How to Build RAG Systems in 2026: 8 Architecture Patterns — AIThinkerLab](https://aithinkerlab.com/build-rag-systems-2026-architecture-patterns/)
- [Agentic RAG Developer Guide (2026) — Future AGI](https://futureagi.com/blog/agentic-rag-systems-2025/)
- [Hybrid search and reranking — Ubuntu](https://ubuntu.com/blog/hybrid-search-and-reranking-a-deeper-look-at-rag)

Evals, observability, guardrails:
- [Agent Observability 2026: Evals, Traces, Cost — DigitalApplied](https://www.digitalapplied.com/blog/agent-observability-2026-evals-traces-cost-guide)
- [Agent observability: complete guide 2026 — Braintrust](https://www.braintrust.dev/articles/agent-observability-complete-guide-2026)
- [LLM Guardrails: Production Safety Layers Reference 2026 — DigitalApplied](https://www.digitalapplied.com/blog/llm-guardrails-production-safety-layers-reference-2026)
- [AI Agents in 2026: Tools, Memory, Evals, Guardrails — Andrii Furmanets](https://andriifurmanets.com/blogs/ai-agents-2026-practical-architecture-tools-memory-evals-guardrails)

Security (prompt injection / lethal trifecta):
- [The lethal trifecta for AI agents — Simon Willison](https://simonwillison.net/2025/Jun/16/the-lethal-trifecta/)
- [AI Security in 2026: Prompt Injection & the Lethal Trifecta — Airia](https://airia.com/ai-security-in-2026-prompt-injection-the-lethal-trifecta-and-how-to-defend/)
- [Human-in-the-Loop AI Agents: Approvals, Escalation, Safe Autonomy (2026) — Medium](https://medium.com/@arvisionlab/human-in-the-loop-ai-agents-how-to-add-approvals-escalation-and-safe-autonomy-in-production-0a21e359781c)

Desktop app architecture & key safety:
- [Best Practices for API Key Safety — OpenAI Help Center](https://help.openai.com/en/articles/5112595-best-practices-for-api-key-safety)
- [How to Store API Keys for AI Agents Securely — DEV](https://dev.to/the_seventeen/how-to-store-api-keys-for-ai-agents-securely-11kg)
- [Electron or Tauri for Modern Desktop Apps? — SoftwareLogic](https://softwarelogic.co/en/blog/how-to-choose-electron-or-tauri-for-modern-desktop-apps)
- [Building a Private AI Desktop App with Rust, Tauri, llama.cpp — n1n.ai](https://explore.n1n.ai/blog/building-private-ai-desktop-app-rust-tauri-llamacpp-2026-06-09)
- [How I Built a Desktop AI App with Tauri v2 + React 19 (2026) — DEV](https://dev.to/purpledoubled/how-i-built-a-desktop-ai-app-with-tauri-v2-react-19-in-2026-1g47)
- [Building an Electron Chat App with the OpenAI Agents SDK — Medium](https://medium.com/@dorangao/building-an-electron-based-chat-app-with-the-openai-agents-sdk-step-by-step-guide-045b15fa0a1b)

> **Note on currency:** model names/versions and SDK method signatures move fast. Verify exact identifiers (`gpt-5.1`/`gpt-5.5`, helper names like `zodTextFormat`) against the live OpenAI docs before shipping.
