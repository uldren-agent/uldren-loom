//! Uldren Loom compute layer.

pub mod authz;
pub mod capability;
#[cfg(feature = "derivations")]
pub mod derivation;
pub mod engine;
pub mod error;
pub mod facade;
pub mod gate;
#[cfg(feature = "guards")]
pub mod guard;
pub mod manifest;
pub mod program_lifecycle;
pub mod state_access;
#[cfg(feature = "statecharts")]
pub mod statechart;
pub mod template_engine;
pub mod trigger_exec;
#[cfg(feature = "workflows")]
pub mod workflow;

pub use authz::{ExecContext, run_as_context};
pub use capability::{
    Capability, Grant, GrantSet, Mode, Scope, grantable_facets, is_program_grantable,
};
pub use engine::{FileSet, RunOutcome, RunResult, run_state};
pub use error::ExecError;
pub use facade::{
    BatchExecRequest, DirectExecRequest, ExecCommitMode, ExecCommitReport, ExecReport, ExecRequest,
    ExecStep, apply, batch, direct, dry_run, execute_cbor,
};
pub use gate::{RunReport, run_on_branch};
#[cfg(feature = "guards")]
pub use guard::{CelAclPredicateEvaluator, evaluate_acl_predicate};
pub use manifest::Manifest;
pub use program_lifecycle::{
    ProgramBody, StoredProgram, program_get, program_inspect, program_list, program_put,
    program_put_cel, program_put_template, program_put_wasm, program_remove,
};
pub use state_access::StateAccess;
pub use template_engine::{TemplateExecution, render_template_program};
pub use trigger_exec::{
    ResolvedTriggerProgram, TRIGGER_INPUT_ID, TRIGGER_INPUT_STIMULUS,
    TRIGGER_INPUT_STIMULUS_DIGEST, TriggerExecution, TriggerExecutionState, TriggerFireDisposition,
    TriggerFireReport, TriggerProgramResolver, fire_trigger_candidate,
    fire_trigger_candidate_with_state,
};
