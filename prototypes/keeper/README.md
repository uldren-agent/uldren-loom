# keeper-prototype (throwaway)

A vertical slice of the keeper: a host-side driver that fires a stored program on a cron schedule,
passing the firing instant as a seeded input. Detached from the workspace; safe to delete.

```
cargo run --release
```

It validates four contracts:

1. **Deterministic schedule**: the next-fire set is a pure function of `(cron expression, base
   instant)`. The base is fixed, never `now()`, because time is a seeded input to a program,
   never an ambient read.
2. **Idempotency via content addressing**: a fire is a pure function of `(program, inputs)`, so
   re-firing on the same stimulus yields an identical state root. At-least-once firing gives
   effectively-once outcomes; the keeper dedups on `(binding, stimulus)`.
3. **Loom is the keeper's durable backend**: the only state is the binding plus a per-binding
   last-fired watermark; there is no external job-queue store.
4. **Missed-fire policy**: `skip` (default), `collapse`, `backfill` over instants missed while the
   keeper was down, computed deterministically from the watermark.

Stubbed (the real engine has these; this slice does not): wasmi execution and `StateAccess` (the
"program" is a pure hash), the real `trigger` workspace and version control, and authorization of
`run_as`. The cron dialect is `croner`.
