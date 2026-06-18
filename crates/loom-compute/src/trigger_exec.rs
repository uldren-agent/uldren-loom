use std::collections::{BTreeMap, BTreeSet};

use loom_core::vcs::Loom;
use loom_core::workspace::WorkspaceId;
use loom_core::{
    Digest, FireOutcome, FireRecord, ObjectStore, OverlapPolicy, PrincipalId, TriggerExecMode,
    TriggerFireCandidate, TriggerId, trigger_append_fire_record, trigger_stimulus_to_cbor,
};

use crate::authz::run_as_context;
use crate::error::ExecError;
use crate::facade::{
    BatchExecRequest, DirectExecRequest, ExecCommitMode, ExecCommitReport, ExecReport, ExecRequest,
    ExecStep, batch, direct, dry_run,
};
use crate::manifest::Manifest;

pub const TRIGGER_INPUT_ID: &str = "trigger.id";
pub const TRIGGER_INPUT_STIMULUS: &str = "trigger.stimulus";
pub const TRIGGER_INPUT_STIMULUS_DIGEST: &str = "trigger.stimulus_digest";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTriggerProgram {
    pub manifest: Manifest,
    pub wasm: Vec<u8>,
    pub inputs: BTreeMap<String, Vec<u8>>,
}

pub trait TriggerProgramResolver {
    fn resolve_trigger_program(&self, program: Digest)
    -> Result<ResolvedTriggerProgram, ExecError>;
}

impl<F> TriggerProgramResolver for F
where
    F: Fn(Digest) -> Result<ResolvedTriggerProgram, ExecError>,
{
    fn resolve_trigger_program(
        &self,
        program: Digest,
    ) -> Result<ResolvedTriggerProgram, ExecError> {
        self(program)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TriggerExecution {
    Gated(ExecReport),
    Committed {
        mode: ExecCommitMode,
        report: ExecCommitReport,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerFireReport {
    pub record: FireRecord,
    pub execution: Option<TriggerExecution>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TriggerExecutionState {
    pub running: BTreeSet<TriggerId>,
}

impl TriggerExecutionState {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn with_running(running: impl IntoIterator<Item = TriggerId>) -> Self {
        Self {
            running: running.into_iter().collect(),
        }
    }

    fn is_running(&self, id: TriggerId) -> bool {
        self.running.contains(&id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TriggerFireDisposition {
    Fired(TriggerFireReport),
    Skipped(TriggerFireReport),
    Queued(TriggerFireCandidate),
}

pub fn fire_trigger_candidate<S, R>(
    loom: &mut Loom<S>,
    program_workspace: WorkspaceId,
    candidate: TriggerFireCandidate,
    resolver: &R,
    timestamp_ms: u64,
) -> Result<TriggerFireReport, ExecError>
where
    S: ObjectStore,
    R: TriggerProgramResolver,
{
    match fire_trigger_candidate_with_state(
        loom,
        program_workspace,
        candidate,
        resolver,
        timestamp_ms,
        &TriggerExecutionState::empty(),
    )? {
        TriggerFireDisposition::Fired(report) | TriggerFireDisposition::Skipped(report) => {
            Ok(report)
        }
        TriggerFireDisposition::Queued(_) => Err(ExecError::Program(
            "empty trigger execution state cannot queue a candidate".to_string(),
        )),
    }
}

pub fn fire_trigger_candidate_with_state<S, R>(
    loom: &mut Loom<S>,
    program_workspace: WorkspaceId,
    candidate: TriggerFireCandidate,
    resolver: &R,
    timestamp_ms: u64,
    state: &TriggerExecutionState,
) -> Result<TriggerFireDisposition, ExecError>
where
    S: ObjectStore,
    R: TriggerProgramResolver,
{
    if state.is_running(candidate.binding.id) {
        match candidate.binding.options.overlap {
            OverlapPolicy::SkipIfRunning => {
                let report = skipped_report(&candidate);
                trigger_append_fire_record(loom, program_workspace, &report.record)?;
                return Ok(TriggerFireDisposition::Skipped(report));
            }
            OverlapPolicy::Queue => return Ok(TriggerFireDisposition::Queued(candidate)),
            OverlapPolicy::Allow => {}
        }
    }
    let execution = execute_trigger_candidate(loom, &candidate, resolver, timestamp_ms);
    let report = match execution {
        Ok(execution) => success_report(&candidate, execution),
        Err(err) => failure_report(&candidate, err),
    };
    trigger_append_fire_record(loom, program_workspace, &report.record)?;
    Ok(TriggerFireDisposition::Fired(report))
}

fn execute_trigger_candidate<S, R>(
    loom: &mut Loom<S>,
    candidate: &TriggerFireCandidate,
    resolver: &R,
    timestamp_ms: u64,
) -> Result<TriggerExecution, ExecError>
where
    S: ObjectStore,
    R: TriggerProgramResolver,
{
    let program = resolver.resolve_trigger_program(candidate.binding.program)?;
    let principal = trigger_principal(loom, candidate)?;
    let context = run_as_context(
        loom,
        candidate.binding.target_workspace,
        principal,
        candidate.binding.branch.clone(),
        program.manifest.grants.clone(),
    )?;
    let inputs = trigger_inputs(candidate, program.inputs)?;
    let author = format!("trigger:{}", candidate.binding.id);
    let message = format!(
        "trigger fire {} seq {}",
        candidate.binding.id, candidate.fired_at_seq
    );
    let step = ExecStep {
        manifest: program.manifest,
        wasm: &program.wasm,
        inputs,
        fuel: candidate.binding.budget,
    };
    match candidate.binding.mode {
        TriggerExecMode::Gated => Ok(TriggerExecution::Gated(dry_run(
            loom,
            ExecRequest {
                context,
                fork_branch: trigger_fork_branch(candidate),
                step,
                author,
                message,
                timestamp_ms,
            },
        )?)),
        TriggerExecMode::Direct => Ok(TriggerExecution::Committed {
            mode: ExecCommitMode::Direct,
            report: direct(
                loom,
                DirectExecRequest {
                    context,
                    step,
                    author,
                    message,
                    timestamp_ms,
                },
            )?,
        }),
        TriggerExecMode::Batched => Ok(TriggerExecution::Committed {
            mode: ExecCommitMode::Batch,
            report: batch(
                loom,
                BatchExecRequest {
                    context,
                    steps: vec![step],
                    author,
                    message,
                    timestamp_ms,
                },
            )?,
        }),
    }
}

fn trigger_principal<S: ObjectStore>(
    loom: &Loom<S>,
    candidate: &TriggerFireCandidate,
) -> Result<PrincipalId, ExecError> {
    if let Some(principal) = candidate.binding.run_as {
        return Ok(principal);
    }
    loom.effective_principal()?
        .ok_or_else(|| ExecError::Denied("trigger binding has no run_as principal".to_string()))
}

fn trigger_inputs(
    candidate: &TriggerFireCandidate,
    mut inputs: BTreeMap<String, Vec<u8>>,
) -> Result<BTreeMap<String, Vec<u8>>, ExecError> {
    let stimulus = trigger_stimulus_to_cbor(&candidate.stimulus)?;
    inputs.insert(
        TRIGGER_INPUT_ID.to_string(),
        candidate.binding.id.to_string().into_bytes(),
    );
    inputs.insert(TRIGGER_INPUT_STIMULUS.to_string(), stimulus);
    inputs.insert(
        TRIGGER_INPUT_STIMULUS_DIGEST.to_string(),
        candidate.stimulus_digest.to_string().into_bytes(),
    );
    Ok(inputs)
}

fn trigger_fork_branch(candidate: &TriggerFireCandidate) -> String {
    format!(
        "trigger/{}/{}",
        candidate.binding.id, candidate.fired_at_seq
    )
}

fn success_report(
    candidate: &TriggerFireCandidate,
    execution: TriggerExecution,
) -> TriggerFireReport {
    let (outcome, proposed, cost) = match &execution {
        TriggerExecution::Gated(report) => {
            (FireOutcome::Proposed, Some(report.after), report.fuel_used)
        }
        TriggerExecution::Committed { report, .. } => {
            (FireOutcome::Applied, Some(report.after), report.fuel_used)
        }
    };
    TriggerFireReport {
        record: fire_record(candidate, proposed, outcome, cost),
        execution: Some(execution),
        error: None,
    }
}

fn failure_report(candidate: &TriggerFireCandidate, err: ExecError) -> TriggerFireReport {
    let outcome = match err.code() {
        loom_core::Code::PermissionDenied | loom_core::Code::TriggerDenied => FireOutcome::Denied,
        loom_core::Code::ResourceExhausted => FireOutcome::BudgetExceeded,
        _ => FireOutcome::Error,
    };
    TriggerFireReport {
        record: fire_record(candidate, None, outcome, 0),
        execution: None,
        error: Some(err.to_string()),
    }
}

fn skipped_report(candidate: &TriggerFireCandidate) -> TriggerFireReport {
    TriggerFireReport {
        record: fire_record(candidate, None, FireOutcome::Skipped, 0),
        execution: None,
        error: None,
    }
}

fn fire_record(
    candidate: &TriggerFireCandidate,
    proposed: Option<Digest>,
    outcome: FireOutcome,
    cost: u64,
) -> FireRecord {
    FireRecord {
        binding: candidate.binding.id,
        stimulus: candidate.stimulus.clone(),
        stimulus_digest: candidate.stimulus_digest,
        proposed,
        outcome,
        cost,
        fired_at_seq: candidate.fired_at_seq,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_core::calendar::{CalendarEntry, CollectionMeta, Component};
    use loom_core::{
        Algo, FacetKind, MemoryStore, OverlapPolicy, TriggerBinding, TriggerKind, TriggerOptions,
        stimulus_digest, vcs::Loom,
    };

    use crate::{Capability, Grant, GrantSet, Mode, Scope};

    fn id(seed: u8) -> WorkspaceId {
        WorkspaceId::from_bytes([seed; 16])
    }

    fn pid(seed: u8) -> PrincipalId {
        PrincipalId::from_bytes([seed; 16])
    }

    fn grants() -> GrantSet {
        GrantSet::new(vec![Grant {
            facet: Capability::Calendar,
            mode: Mode::ReadWrite,
            scopes: vec![Scope::All],
        }])
    }

    fn calendar_trigger_program() -> Vec<u8> {
        wat::parse_str(
            r#"(module
                 (import "env" "input_get" (func $in (param i32 i32 i32 i32) (result i32)))
                 (import "env" "calendar_create_collection" (func $cal_create (param i32 i32 i32 i32 i32 i32)))
                 (import "env" "calendar_put_entry" (func $cal_put (param i32 i32 i32 i32 i32 i32 i32 i32) (result i32)))
                 (memory (export "memory") 1)
                 (data (i32.const 0) "alice")
                 (data (i32.const 16) "work")
                 (data (i32.const 32) "cal_meta")
                 (data (i32.const 48) "cal_entry")
                 (func (export "run") (local $n i32)
                   (local.set $n (call $in (i32.const 32)(i32.const 8)(i32.const 1000)(i32.const 256)))
                   (call $cal_create (i32.const 0)(i32.const 5)(i32.const 16)(i32.const 4)(i32.const 1000)(local.get $n))
                   (local.set $n (call $in (i32.const 48)(i32.const 9)(i32.const 1400)(i32.const 512)))
                   (drop (call $cal_put (i32.const 0)(i32.const 5)(i32.const 16)(i32.const 4)(i32.const 1400)(local.get $n)(i32.const 1900)(i32.const 32)))))"#,
        )
        .expect("assemble calendar trigger program")
    }

    fn time_candidate(overlap: OverlapPolicy) -> TriggerFireCandidate {
        let stimulus = loom_core::TriggerStimulus::Time {
            fired_at_ms: 60_000,
        };
        TriggerFireCandidate {
            binding: TriggerBinding {
                id: id(3),
                kind: TriggerKind::Time {
                    cron: "0 * * * * *".to_string(),
                    timezone: "UTC".to_string(),
                },
                program: Digest::blake3(b"program"),
                target_workspace: id(2),
                branch: "main".to_string(),
                budget: 2_000_000,
                mode: TriggerExecMode::Direct,
                options: TriggerOptions {
                    overlap,
                    ..TriggerOptions::default()
                },
                run_as: Some(pid(9)),
                enabled: true,
            },
            stimulus_digest: stimulus_digest(Algo::Blake3, &stimulus).unwrap(),
            stimulus,
            fired_at_seq: 0,
        }
    }

    #[test]
    fn trigger_fire_runs_pim_program_and_records_history() {
        let mut loom = Loom::new(MemoryStore::default());
        let program_ns = id(1);
        let target_ns = id(2);
        loom.registry_mut()
            .create(FacetKind::Program, Some("programs"), program_ns)
            .unwrap();
        loom.registry_mut()
            .create(FacetKind::Calendar, Some("calendar"), target_ns)
            .unwrap();
        loom.commit(program_ns, "system", "init programs", 0)
            .unwrap();
        loom.commit(target_ns, "system", "init calendar", 0)
            .unwrap();

        let wasm = calendar_trigger_program();
        let manifest = Manifest::for_wasm("calendar-hook", &wasm, grants());
        let program_digest = manifest.store(loom.store_mut()).unwrap();
        let candidate = TriggerFireCandidate {
            binding: TriggerBinding {
                id: id(3),
                kind: TriggerKind::Time {
                    cron: "0 * * * * *".to_string(),
                    timezone: "UTC".to_string(),
                },
                program: program_digest,
                target_workspace: target_ns,
                branch: "main".to_string(),
                budget: 2_000_000,
                mode: TriggerExecMode::Direct,
                options: TriggerOptions::default(),
                run_as: Some(pid(9)),
                enabled: true,
            },
            stimulus: loom_core::TriggerStimulus::Time {
                fired_at_ms: 60_000,
            },
            stimulus_digest: stimulus_digest(
                Algo::Blake3,
                &loom_core::TriggerStimulus::Time {
                    fired_at_ms: 60_000,
                },
            )
            .unwrap(),
            fired_at_seq: 0,
        };
        let inputs = BTreeMap::from([
            (
                "cal_meta".to_string(),
                CollectionMeta {
                    display_name: "Work".to_string(),
                    component_set: vec![Component::Event],
                }
                .encode(),
            ),
            (
                "cal_entry".to_string(),
                CalendarEntry::event("u1", "Standup", "20240101T090000").encode(),
            ),
        ]);
        let resolver = |digest| {
            assert_eq!(digest, program_digest);
            Ok(ResolvedTriggerProgram {
                manifest: manifest.clone(),
                wasm: wasm.clone(),
                inputs: inputs.clone(),
            })
        };

        let report =
            fire_trigger_candidate(&mut loom, program_ns, candidate, &resolver, 60_001).unwrap();
        assert_eq!(report.record.outcome, FireOutcome::Applied);
        assert!(report.record.cost > 0);
        assert!(report.record.proposed.is_some());
        assert!(report.error.is_none());
        assert_eq!(
            loom_core::calendar::get_entry(&loom, target_ns, "alice", "work", "u1")
                .unwrap()
                .unwrap()
                .summary,
            "Standup"
        );
        let history = loom_core::trigger_history(&loom, program_ns, id(3), 0, 10).unwrap();
        assert_eq!(history, vec![report.record]);
    }

    #[test]
    fn running_skip_if_running_records_skipped_without_resolving_program() {
        let mut loom = Loom::new(MemoryStore::default());
        let program_ns = id(1);
        loom.registry_mut()
            .create(FacetKind::Program, Some("programs"), program_ns)
            .unwrap();
        let candidate = time_candidate(OverlapPolicy::SkipIfRunning);
        let resolver = |_| panic!("skip-if-running must not resolve the program");

        let disposition = fire_trigger_candidate_with_state(
            &mut loom,
            program_ns,
            candidate.clone(),
            &resolver,
            60_001,
            &TriggerExecutionState::with_running([candidate.binding.id]),
        )
        .unwrap();

        let TriggerFireDisposition::Skipped(report) = disposition else {
            panic!("expected skipped disposition");
        };
        assert_eq!(report.record.outcome, FireOutcome::Skipped);
        assert!(report.execution.is_none());
        assert!(report.error.is_none());
        assert_eq!(
            loom_core::trigger_history(&loom, program_ns, candidate.binding.id, 0, 10).unwrap(),
            vec![report.record]
        );
    }

    #[test]
    fn running_queue_returns_candidate_without_fire_record() {
        let mut loom = Loom::new(MemoryStore::default());
        let program_ns = id(1);
        loom.registry_mut()
            .create(FacetKind::Program, Some("programs"), program_ns)
            .unwrap();
        let candidate = time_candidate(OverlapPolicy::Queue);
        let resolver = |_| panic!("queue overlap must not resolve the program");

        let disposition = fire_trigger_candidate_with_state(
            &mut loom,
            program_ns,
            candidate.clone(),
            &resolver,
            60_001,
            &TriggerExecutionState::with_running([candidate.binding.id]),
        )
        .unwrap();

        assert_eq!(
            disposition,
            TriggerFireDisposition::Queued(candidate.clone())
        );
        assert!(
            loom_core::trigger_history(&loom, program_ns, candidate.binding.id, 0, 10)
                .unwrap()
                .is_empty()
        );
    }
}
