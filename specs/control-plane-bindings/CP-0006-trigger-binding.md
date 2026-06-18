# CP-0006 - `trigger` Binding

**Series:** Control-plane bindings (normative-track sub-series; Draft)
**Version:** 0.1.0-draft. **Status:** Draft. **Last updated:** 2026-06-25
**Reads first:** [`CP-0000-index.md`](./CP-0000-index.md), [`CP-0002-exec-binding.md`](./CP-0002-exec-binding.md),
[`CP-0003-watch-binding.md`](./CP-0003-watch-binding.md),
[`../facet-bindings/P9-0002-projection-conventions.md`](../facet-bindings/P9-0002-projection-conventions.md),
facade spec **0029** and ADR-0006.

`trigger` is the target control-plane surface for reactive automation. Shared keeper logic evaluates
stored bindings; hosts drive wakeups and execution capacity.

## 1. Current Source Boundary

Current source does not implement a public `trigger` facade, trigger ABI, binding wrapper, or hosted
projection.

Source-backed pieces are:

- stable `Code::TriggerNotFound` and `Code::TriggerDenied`;
- reusable trigger binding, stimulus, fire-record, missed-fire, overlap, and croner-backed time
  evaluation contracts in `crates/loom-triggers`;
- reserved trigger binding storage, fire-log append/history, and due-fire planning in
  `loom_core::triggers`;
- source-backed `watch` model and file-domain materialization in 0030;
- run-as-aware trigger execution, canonical stimulus inputs, and overlap handling in
  `crates/loom-compute`;
- executable `pim-trigger` conformance proving direct trigger execution, skipped fire records, and
  queued overlap candidates.

## 2. Target Facade Surface

Target public shape:

```text
create(binding) -> Uuid
update(id, binding)
enable(id, on: bool)
remove(id)
list(filter?) -> List<Binding>
reassign(id, run_as)
history(id, from_seq?) -> List<FireRecord>
fire_now(id) -> ExecResult
```

`Binding` describes either a cron schedule or a `watch` change selector, a target program, a target
workspace, a budget, and a `run_as` principal. `reassign` is admin-gated and audited.

## 3. Target REST

Target root `/v1/looms/{loom_id}/workspaces/{workspace_id}/triggers`:

| Method | HTTP |
| --- | --- |
| `create` | `POST /triggers` |
| `list` | `GET /triggers?kind=...&target_ns=...` |
| `update` | `PUT /triggers/{id}` |
| `remove` | `DELETE /triggers/{id}` |
| `enable` | `POST /triggers/{id}:enable` |
| `reassign` | `POST /triggers/{id}:reassign` |
| `history` | `GET /triggers/{id}/history?from_seq=...` |
| `fire_now` | `POST /triggers/{id}:fireNow` |

## 4. Target JSON-RPC and gRPC

Target JSON-RPC methods: `trigger.create`, `trigger.update`, `trigger.enable`, `trigger.remove`,
`trigger.list`, `trigger.reassign`, `trigger.history`, and `trigger.fireNow`.

Target gRPC methods mirror the facade. `History` may be server-streaming. `FireNow` follows the
promoted `exec` result/log shape once `exec` is public.

## 5. Tier-1 MCP

- **Read tools:** `trigger.list` and `trigger.history`.
- **Write tools:** `trigger.create`, `trigger.update`, `trigger.enable`, `trigger.remove`,
  `trigger.reassign`, and `trigger.fireNow`.
- **Authorization:** write tools are token-gated. A created trigger's `run_as` must not exceed the
  authority of the creator unless an admin reassigns it.

## 6. Tier-2 Foreign Adapter

Cron, webhook, and automation-system adapters are target work. They must not become the system of
record for trigger bindings. Loom content remains the durable source of truth, and hosts only drive
the shared keeper.

## 7. Errors, Parity, and Concurrency

- **Errors:** `TRIGGER_NOT_FOUND` and `TRIGGER_DENIED` are source-backed stable codes. `CURSOR_INVALID`
  is source-backed for the watch side. Execution-specific failures depend on the promoted `exec`
  envelope or future stable codes.
- **Parity:** keeper evaluation is intended to be shared Loom logic. Platforms without background
  timers rely on an active host driver.
- **Concurrency:** `run_as` is resolved against live grants at fire time and fails closed. Fire
  records provide audit, watermark, and idempotency state.

## 8. Resolved Decisions

### CP-RD-T1 - `run_as` escalation guard

- **Decision.** A created trigger's `run_as` must be less than or equal to the creator's authority.
  Reassignment to a more-privileged principal requires admin authority. Every fire is audited, and the
  keeper fails closed if `run_as` is unresolved or unauthorized at fire time.
