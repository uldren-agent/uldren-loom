//! THROWAWAY keeper slice (events & triggers). A host-side driver that fires a stored
//! program on a cron schedule, passing the firing instant as a seeded input.
//!
//! What this demonstrates, end to end:
//! - **Determinism of the schedule**: the next-fire set is a pure function of
//!   `(cron expression, base instant)`. The base is fixed, never `now()`, because time is a seeded
//!   input to a Loom program, never an ambient read.
//! - **Idempotency via content addressing**: a fire is a pure function of
//!   `(program, inputs)`, so re-firing on the same stimulus yields an identical state root. The keeper
//!   dedups on `(binding, stimulus)` rather than building a delivery protocol.
//! - **Keeper state lives in Loom**: the only state is the
//!   trigger binding plus a per-binding last-fired watermark; there is no external job-queue store.
//! - **Missed-fire policy**: skip (default), collapse, or backfill instants missed while the
//!   keeper was down, computed deterministically from the watermark.
//!
//! Stubbed (the real engine has these; this slice does not): wasmi execution and `StateAccess` (the
//! "program" here is a pure hash), the real `trigger` workspace and version control, and authorization
//! of `run_as`. The point is to validate the keeper's timing and idempotency contracts.

use chrono::{DateTime, Duration, TimeZone, Utc};
use croner::Cron;
use std::str::FromStr;

/// A content address (blake3 over canonical bytes), standing in for a Loom Digest. Short
/// hex for display only.
fn digest(parts: &[&[u8]]) -> String {
    let mut h = blake3::Hasher::new();
    for p in parts {
        h.update(p);
    }
    hex::encode(&h.finalize().as_bytes()[..8])
}

/// The stimulus for a time trigger is the digest of the fired timestamp: the keeper captures
/// the instant, encodes it canonically, and hands its digest to the program as a seeded input.
fn stimulus_for(ts: DateTime<Utc>) -> String {
    digest(&[ts.to_rfc3339().as_bytes()])
}

/// The deterministic "program": a new state root is a pure function of `(program, prior root,
/// stimulus)`. In real Loom this runs in wasmi through `StateAccess` against a forked branch;
/// here it is a pure hash so the slice can show determinism and idempotency without the engine.
fn run_program(program: &str, prior_root: &str, stimulus: &str) -> String {
    digest(&[program.as_bytes(), prior_root.as_bytes(), stimulus.as_bytes()])
}

#[derive(Clone, Copy)]
enum Missed {
    Skip,
    Collapse,
    Backfill,
}

impl Missed {
    fn name(self) -> &'static str {
        match self {
            Missed::Skip => "skip",
            Missed::Collapse => "collapse",
            Missed::Backfill => "backfill",
        }
    }
}

/// A binding in the reserved `trigger` workspace, simplified to the time-trigger fields.
struct Binding {
    id: &'static str,
    cron: &'static str,
    program: &'static str, // a program manifest digest; here just a label
    missed: Missed,
}

/// The next `n` fire instants strictly after `base`. Pure function of `(cron, base)`.
fn next_fires(cron: &str, base: DateTime<Utc>, n: usize) -> Vec<DateTime<Utc>> {
    let c = Cron::from_str(cron).expect("parse cron");
    let mut out = Vec::with_capacity(n);
    let mut cur = base;
    for _ in 0..n {
        let nxt = c.find_next_occurrence(&cur, false).expect("next occurrence");
        out.push(nxt);
        cur = nxt;
    }
    out
}

/// One entry in the event spine: an append-only audit + watermark + idempotency record.
struct FireRecord {
    seq: u64,
    binding: String,
    stimulus: String,
    state_root: String,
}

fn main() {
    // Fixed base so the whole run is reproducible. 2023-11-14T22:13:20Z.
    let base = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
    let genesis_root = "state:genesis";

    let binding = Binding {
        id: "nightly-consolidate",
        cron: "0 3 * * *", // 03:00 every day (the memory-maintenance Agent OS program)
        program: "prog:consolidate",
        missed: Missed::Skip,
    };

    println!("== keeper slice for 0029 ==");
    println!(
        "binding '{}': cron='{}' program={} missed={}",
        binding.id,
        binding.cron,
        binding.program,
        binding.missed.name()
    );

    // (1) Deterministic schedule: next fires are a pure function of (cron, base).
    let fires = next_fires(binding.cron, base, 3);
    println!("\n[1] next 3 fires after {}:", base.to_rfc3339());
    for f in &fires {
        println!("    {}  (stimulus {})", f.to_rfc3339(), stimulus_for(*f));
    }
    // Recompute to show purity: same inputs, same set.
    assert_eq!(fires, next_fires(binding.cron, base, 3), "schedule must be deterministic");

    // (2) Fire each, building the event log. Each fire chains the state root, but every fire is a pure
    // function of (program, prior root, stimulus).
    let mut log: Vec<FireRecord> = Vec::new();
    let mut root = genesis_root.to_string();
    println!("\n[2] firing, appending to the event spine:");
    for (i, ts) in fires.iter().enumerate() {
        let stim = stimulus_for(*ts);
        let new_root = run_program(binding.program, &root, &stim);
        log.push(FireRecord {
            seq: i as u64,
            binding: binding.id.to_string(),
            stimulus: stim,
            state_root: new_root.clone(),
        });
        root = new_root;
    }
    for r in &log {
        println!(
            "    seq={} binding={} stimulus={} -> state_root={}",
            r.seq, r.binding, r.stimulus, r.state_root
        );
    }

    // (3) Idempotency: re-firing the FIRST stimulus from the same prior root yields the SAME state
    // root (cacheable on (program, inputs)). At-least-once firing -> effectively-once outcome.
    let replay = run_program(binding.program, genesis_root, &stimulus_for(fires[0]));
    let original = &log[0].state_root;
    println!(
        "\n[3] idempotency: original={} replay={} equal={}",
        original,
        replay,
        &replay == original
    );
    assert_eq!(&replay, original, "re-firing the same stimulus must yield the same state root");

    // (4) Missed-fire policy: the keeper was down; recompute the instants missed since the watermark,
    // deterministically, and apply each policy. 'now' is 2 days 5 hours after base.
    let watermark = base;
    let now = base + Duration::days(2) + Duration::hours(5);
    let missed: Vec<_> = next_fires(binding.cron, watermark, 16)
        .into_iter()
        .filter(|t| *t <= now)
        .collect();
    println!(
        "\n[4] downtime {} -> {}: {} instants missed. Per policy:",
        watermark.to_rfc3339(),
        now.to_rfc3339(),
        missed.len()
    );
    for policy in [Missed::Skip, Missed::Collapse, Missed::Backfill] {
        let fired: Vec<DateTime<Utc>> = match policy {
            Missed::Skip => Vec::new(),                       // drop missed; wait for the next future instant
            Missed::Collapse => missed.last().cloned().into_iter().collect(), // one catch-up
            Missed::Backfill => missed.clone(),               // one fire per missed instant
        };
        println!(
            "    {:9} fires {} {}",
            policy.name(),
            fired.len(),
            if policy.name() == binding.missed.name() { "(this binding's setting)" } else { "" }
        );
    }

    println!("\nall keeper invariants held.");
}
