//! The `exec` facade: gated proposal, direct commit, and batched commit modes over one execution
//! substrate.
//!
//! [`dry_run`] forks the context's base branch, runs the metered program through the real facet-backed
//! [`StateAccess`] (files + versioned KV, every operation gated by principal ACL and the step
//! manifest's grants), commits the proposal to the fork branch, and returns a reviewable
//! [`ExecReport`]. The gated path never mutates the base branch; the caller adopts the proposal with
//! [`apply`] or discards it. Direct and batched paths commit to the context's base branch only after
//! every program step succeeds.

use std::collections::BTreeMap;

use loom_codec::Value as CborValue;
use loom_core::vcs::{Change, ChangeKind, Loom, MergeOutcome};
use loom_core::workspace::WorkspaceId;
use loom_core::{Digest, ObjectStore, PrincipalId, RoleId};

use crate::authz::ExecContext;
use crate::capability::{Capability, Grant, GrantSet, Mode, Scope};
use crate::engine::run_state;
use crate::error::ExecError;
use crate::manifest::Manifest;
use crate::state_access::StateAccess;

/// A gated execution request: run `wasm` on a fork of the context's base branch and commit the proposal
/// to `fork_branch`. The commit metadata is explicit so a dry run is deterministic and caller-controlled.
pub struct ExecRequest<'a> {
    /// Where the program runs, who runs it, and the maximum grants approved for this execution.
    pub context: ExecContext,
    /// The branch the proposal is committed to (created from the base branch tip).
    pub fork_branch: String,
    /// The program step to run.
    pub step: ExecStep<'a>,
    /// Author recorded on the proposal commit.
    pub author: String,
    /// Message recorded on the proposal commit.
    pub message: String,
    /// Timestamp (ms since the Unix epoch) recorded on the proposal commit.
    pub timestamp_ms: u64,
}

/// One program step in a direct or batched execution.
pub struct ExecStep<'a> {
    /// The content-addressed program manifest for this step.
    pub manifest: Manifest,
    /// The program body.
    pub wasm: &'a [u8],
    /// Declared read-only inputs, addressable by name through the `input_get` host call.
    pub inputs: BTreeMap<String, Vec<u8>>,
    /// The metering budget.
    pub fuel: u64,
}

/// A direct execution request: run one program against the context's base branch and commit it if the
/// full run succeeds.
pub struct DirectExecRequest<'a> {
    /// Where the program runs, who runs it, and the maximum grants approved for this execution.
    pub context: ExecContext,
    /// The program step to run.
    pub step: ExecStep<'a>,
    /// Author recorded on the commit.
    pub author: String,
    /// Message recorded on the commit.
    pub message: String,
    /// Timestamp (ms since the Unix epoch) recorded on the commit.
    pub timestamp_ms: u64,
}

/// A batched execution request: run all steps against one working state and commit once if every step
/// succeeds.
pub struct BatchExecRequest<'a> {
    /// Where the programs run, who runs them, and the maximum grants approved for this execution.
    pub context: ExecContext,
    /// Program steps executed in order.
    pub steps: Vec<ExecStep<'a>>,
    /// Author recorded on the commit.
    pub author: String,
    /// Message recorded on the commit.
    pub message: String,
    /// Timestamp (ms since the Unix epoch) recorded on the commit.
    pub timestamp_ms: u64,
}

/// The reviewable result of a gated dry run. Deterministic for a given program, base, and inputs: two
/// dry runs of the same request produce the same `after_root`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecReport {
    /// The principal the program ran as.
    pub principal: PrincipalId,
    /// The fork branch the proposal was committed to.
    pub fork_branch: String,
    /// The base commit the run started from.
    pub before: Digest,
    /// The proposed commit on the fork.
    pub after: Digest,
    /// The state root (root Tree digest) of the proposal.
    pub after_root: Digest,
    /// Path-level changes from base to proposal.
    pub changes: Vec<Change>,
    /// Fuel consumed by the program.
    pub fuel_used: u64,
    /// Ordered, bounded diagnostic log lines the program emitted through the `log` host call.
    pub logs: Vec<String>,
}

impl ExecReport {
    /// Encode the report as the stable `loom.exec.result.v1` canonical CBOR envelope.
    pub fn to_cbor(&self) -> Result<Vec<u8>, loom_codec::CodecError> {
        loom_codec::encode(&exec_result_value(ExecResultValue {
            mode: "gated",
            committed: false,
            principal: self.principal,
            branch: &self.fork_branch,
            before: self.before,
            after: self.after,
            after_root: self.after_root,
            changes: &self.changes,
            fuel_used: self.fuel_used,
            logs: &self.logs,
        }))
    }
}

/// The committed result of a direct or batched execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecCommitReport {
    /// The principal the program ran as.
    pub principal: PrincipalId,
    /// The branch that advanced.
    pub branch: String,
    /// The commit the run started from.
    pub before: Digest,
    /// The commit created by the run.
    pub after: Digest,
    /// The state root (root Tree digest) of the committed result.
    pub after_root: Digest,
    /// Path-level changes from base to committed result.
    pub changes: Vec<Change>,
    /// Total fuel consumed by all executed steps.
    pub fuel_used: u64,
    /// Ordered, bounded diagnostic log lines the programs emitted through the `log` host call.
    pub logs: Vec<String>,
}

impl ExecCommitReport {
    /// Encode the report as the stable `loom.exec.result.v1` canonical CBOR envelope.
    pub fn to_cbor(&self, mode: ExecCommitMode) -> Result<Vec<u8>, loom_codec::CodecError> {
        loom_codec::encode(&exec_result_value(ExecResultValue {
            mode: mode.as_str(),
            committed: true,
            principal: self.principal,
            branch: &self.branch,
            before: self.before,
            after: self.after,
            after_root: self.after_root,
            changes: &self.changes,
            fuel_used: self.fuel_used,
            logs: &self.logs,
        }))
    }
}

/// The committed execution mode represented by an [`ExecCommitReport`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecCommitMode {
    /// One program committed directly to the base branch.
    Direct,
    /// Several program steps committed as one base-branch commit.
    Batch,
}

impl ExecCommitMode {
    fn as_str(self) -> &'static str {
        match self {
            ExecCommitMode::Direct => "direct",
            ExecCommitMode::Batch => "batch",
        }
    }
}

/// Decode and execute a `loom.exec.request.v1` canonical CBOR request, returning the
/// `loom.exec.result.v1` canonical CBOR response.
pub fn execute_cbor<S: ObjectStore>(
    loom: &mut Loom<S>,
    request: &[u8],
) -> Result<Vec<u8>, ExecError> {
    match ExecEnvelope::decode(request)?.execute(loom)? {
        ExecEnvelopeResult::Gated(report) => report
            .to_cbor()
            .map_err(|err| ExecError::Program(format!("exec result cbor: {err}"))),
        ExecEnvelopeResult::Committed { mode, report } => report
            .to_cbor(mode)
            .map_err(|err| ExecError::Program(format!("exec result cbor: {err}"))),
    }
}

struct ExecEnvelope {
    mode: ExecMode,
    context: ExecContext,
    fork_branch: Option<String>,
    steps: Vec<OwnedExecStep>,
    author: String,
    message: String,
    timestamp_ms: u64,
}

struct OwnedExecStep {
    manifest: Manifest,
    wasm: Vec<u8>,
    inputs: BTreeMap<String, Vec<u8>>,
    fuel: u64,
}

enum ExecMode {
    Gated,
    Direct,
    Batch,
}

enum ExecEnvelopeResult {
    Gated(ExecReport),
    Committed {
        mode: ExecCommitMode,
        report: ExecCommitReport,
    },
}

impl ExecEnvelope {
    fn decode(bytes: &[u8]) -> Result<Self, ExecError> {
        let value = loom_codec::decode(bytes)
            .map_err(|err| ExecError::Program(format!("exec request cbor: {err}")))?;
        let map = CborMap::new(value)?;
        map.expect_text("schema", "loom.exec.request.v1")?;
        let mode = ExecMode::decode(map.text("mode")?.as_str())?;
        let workspace = id16(map.bytes("workspace")?, "workspace")?;
        let principal = id16(map.bytes("principal")?, "principal")?;
        let roles = map
            .array("roles")?
            .into_iter()
            .map(|v| match v {
                CborValue::Bytes(bytes) => id16(bytes, "role").map(RoleId::from_bytes),
                _ => Err(ExecError::Program(
                    "exec request role must be bytes".to_string(),
                )),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let grants = decode_grants(CborValue::Array(map.array("grants")?))?;
        if !grants.is_grantable() {
            return Err(ExecError::Denied(
                "exec context grants include a non-grantable facet".to_string(),
            ));
        }
        let context = ExecContext {
            workspace: WorkspaceId::from_bytes(workspace),
            principal: PrincipalId::from_bytes(principal),
            roles,
            authenticated: map.bool("authenticated")?,
            base_branch: map.text("base_branch")?,
            grants,
        };
        let fork_branch = map.opt_text("fork_branch")?;
        let steps = map
            .array("steps")?
            .into_iter()
            .map(OwnedExecStep::decode)
            .collect::<Result<Vec<_>, _>>()?;
        let envelope = Self {
            mode,
            context,
            fork_branch,
            steps,
            author: map.text("author")?,
            message: map.text("message")?,
            timestamp_ms: map.uint("timestamp_ms")?,
        };
        envelope.validate_shape()?;
        Ok(envelope)
    }

    fn validate_shape(&self) -> Result<(), ExecError> {
        match self.mode {
            ExecMode::Gated => {
                if self.fork_branch.as_deref().unwrap_or("").is_empty() {
                    return Err(ExecError::Program(
                        "gated exec request requires fork_branch".to_string(),
                    ));
                }
                if self.steps.len() != 1 {
                    return Err(ExecError::Program(
                        "gated exec request requires exactly one step".to_string(),
                    ));
                }
            }
            ExecMode::Direct => {
                if self.fork_branch.is_some() {
                    return Err(ExecError::Program(
                        "direct exec request must not include fork_branch".to_string(),
                    ));
                }
                if self.steps.len() != 1 {
                    return Err(ExecError::Program(
                        "direct exec request requires exactly one step".to_string(),
                    ));
                }
            }
            ExecMode::Batch => {
                if self.fork_branch.is_some() {
                    return Err(ExecError::Program(
                        "batch exec request must not include fork_branch".to_string(),
                    ));
                }
                if self.steps.is_empty() {
                    return Err(ExecError::Program(
                        "batch exec request requires at least one step".to_string(),
                    ));
                }
            }
        }
        Ok(())
    }

    fn execute<S: ObjectStore>(self, loom: &mut Loom<S>) -> Result<ExecEnvelopeResult, ExecError> {
        match self.mode {
            ExecMode::Gated => {
                let mut steps = self.steps;
                let step_owned = steps.remove(0);
                let step = step_owned.borrowed();
                let report = dry_run(
                    loom,
                    ExecRequest {
                        context: self.context,
                        fork_branch: self.fork_branch.unwrap_or_default(),
                        step,
                        author: self.author,
                        message: self.message,
                        timestamp_ms: self.timestamp_ms,
                    },
                )?;
                Ok(ExecEnvelopeResult::Gated(report))
            }
            ExecMode::Direct => {
                let mut steps = self.steps;
                let step_owned = steps.remove(0);
                let step = step_owned.borrowed();
                let report = direct(
                    loom,
                    DirectExecRequest {
                        context: self.context,
                        step,
                        author: self.author,
                        message: self.message,
                        timestamp_ms: self.timestamp_ms,
                    },
                )?;
                Ok(ExecEnvelopeResult::Committed {
                    mode: ExecCommitMode::Direct,
                    report,
                })
            }
            ExecMode::Batch => {
                let steps = self
                    .steps
                    .iter()
                    .map(OwnedExecStep::borrowed)
                    .collect::<Vec<_>>();
                let report = batch(
                    loom,
                    BatchExecRequest {
                        context: self.context,
                        steps,
                        author: self.author,
                        message: self.message,
                        timestamp_ms: self.timestamp_ms,
                    },
                )?;
                Ok(ExecEnvelopeResult::Committed {
                    mode: ExecCommitMode::Batch,
                    report,
                })
            }
        }
    }
}

impl ExecMode {
    fn decode(value: &str) -> Result<Self, ExecError> {
        match value {
            "gated" => Ok(Self::Gated),
            "direct" => Ok(Self::Direct),
            "batch" => Ok(Self::Batch),
            other => Err(ExecError::Program(format!(
                "unknown exec request mode {other:?}"
            ))),
        }
    }
}

impl OwnedExecStep {
    fn decode(value: CborValue) -> Result<Self, ExecError> {
        let map = CborMap::new(value)?;
        let manifest_bytes = map.bytes("manifest")?;
        let manifest = Manifest::decode(&manifest_bytes)
            .ok_or_else(|| ExecError::Program("invalid exec step manifest".to_string()))?;
        let inputs = map
            .map("inputs")?
            .into_iter()
            .map(|(k, v)| match (k, v) {
                (CborValue::Text(key), CborValue::Bytes(value)) => Ok((key, value)),
                _ => Err(ExecError::Program(
                    "exec step inputs must be text to bytes".to_string(),
                )),
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        Ok(Self {
            manifest,
            wasm: map.bytes("wasm")?,
            inputs,
            fuel: map.uint("fuel")?,
        })
    }

    fn borrowed(&self) -> ExecStep<'_> {
        ExecStep {
            manifest: self.manifest.clone(),
            wasm: &self.wasm,
            inputs: self.inputs.clone(),
            fuel: self.fuel,
        }
    }
}

struct CborMap {
    entries: Vec<(CborValue, CborValue)>,
}

impl CborMap {
    fn new(value: CborValue) -> Result<Self, ExecError> {
        match value {
            CborValue::Map(entries) => Ok(Self { entries }),
            _ => Err(ExecError::Program("exec request must be a map".to_string())),
        }
    }

    fn get(&self, key: &str) -> Result<&CborValue, ExecError> {
        self.entries
            .iter()
            .find_map(|(k, v)| match k {
                CborValue::Text(found) if found == key => Some(v),
                _ => None,
            })
            .ok_or_else(|| ExecError::Program(format!("exec request missing field {key:?}")))
    }

    fn expect_text(&self, key: &str, expected: &str) -> Result<(), ExecError> {
        let found = self.text(key)?;
        if found == expected {
            Ok(())
        } else {
            Err(ExecError::Program(format!(
                "exec request field {key:?} must be {expected:?}"
            )))
        }
    }

    fn text(&self, key: &str) -> Result<String, ExecError> {
        match self.get(key)? {
            CborValue::Text(value) => Ok(value.clone()),
            _ => Err(ExecError::Program(format!(
                "exec request field {key:?} must be text"
            ))),
        }
    }

    fn opt_text(&self, key: &str) -> Result<Option<String>, ExecError> {
        match self.get(key) {
            Ok(CborValue::Text(value)) => Ok(Some(value.clone())),
            Ok(CborValue::Null) => Ok(None),
            Ok(_) => Err(ExecError::Program(format!(
                "exec request field {key:?} must be text or null"
            ))),
            Err(_) => Ok(None),
        }
    }

    fn bytes(&self, key: &str) -> Result<Vec<u8>, ExecError> {
        match self.get(key)? {
            CborValue::Bytes(value) => Ok(value.clone()),
            _ => Err(ExecError::Program(format!(
                "exec request field {key:?} must be bytes"
            ))),
        }
    }

    fn bool(&self, key: &str) -> Result<bool, ExecError> {
        match self.get(key)? {
            CborValue::Bool(value) => Ok(*value),
            _ => Err(ExecError::Program(format!(
                "exec request field {key:?} must be bool"
            ))),
        }
    }

    fn uint(&self, key: &str) -> Result<u64, ExecError> {
        match self.get(key)? {
            CborValue::Uint(value) => Ok(*value),
            _ => Err(ExecError::Program(format!(
                "exec request field {key:?} must be uint"
            ))),
        }
    }

    fn array(&self, key: &str) -> Result<Vec<CborValue>, ExecError> {
        match self.get(key)? {
            CborValue::Array(value) => Ok(value.clone()),
            _ => Err(ExecError::Program(format!(
                "exec request field {key:?} must be array"
            ))),
        }
    }

    fn map(&self, key: &str) -> Result<Vec<(CborValue, CborValue)>, ExecError> {
        match self.get(key)? {
            CborValue::Map(value) => Ok(value.clone()),
            _ => Err(ExecError::Program(format!(
                "exec request field {key:?} must be map"
            ))),
        }
    }
}

fn id16(bytes: Vec<u8>, field: &str) -> Result<[u8; 16], ExecError> {
    bytes
        .as_slice()
        .try_into()
        .map_err(|_| ExecError::Program(format!("exec request field {field:?} must be 16 bytes")))
}

fn decode_grants(value: CborValue) -> Result<GrantSet, ExecError> {
    let CborValue::Array(items) = value else {
        return Err(ExecError::Program(
            "exec request grants must be an array".to_string(),
        ));
    };
    let grants = items
        .into_iter()
        .map(decode_grant)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(GrantSet::new(grants))
}

fn decode_grant(value: CborValue) -> Result<Grant, ExecError> {
    let CborValue::Array(items) = value else {
        return Err(ExecError::Program(
            "exec request grant must be an array".to_string(),
        ));
    };
    let mut fields = items.into_iter();
    let facet = match fields.next() {
        Some(CborValue::Uint(tag)) => {
            Capability::from_stable_tag(u8::try_from(tag).map_err(|_| {
                ExecError::Program("exec request grant facet tag out of range".to_string())
            })?)
            .ok_or_else(|| ExecError::Program("exec request grant facet tag unknown".to_string()))?
        }
        _ => {
            return Err(ExecError::Program(
                "exec request grant facet must be uint".to_string(),
            ));
        }
    };
    let mode = match fields.next() {
        Some(CborValue::Uint(tag)) => Mode::from_u8(u8::try_from(tag).map_err(|_| {
            ExecError::Program("exec request grant mode tag out of range".to_string())
        })?)
        .ok_or_else(|| ExecError::Program("exec request grant mode tag unknown".to_string()))?,
        _ => {
            return Err(ExecError::Program(
                "exec request grant mode must be uint".to_string(),
            ));
        }
    };
    let scopes = match fields.next() {
        Some(CborValue::Array(scopes)) => scopes
            .into_iter()
            .map(decode_scope)
            .collect::<Result<Vec<_>, _>>()?,
        _ => {
            return Err(ExecError::Program(
                "exec request grant scopes must be an array".to_string(),
            ));
        }
    };
    if fields.next().is_some() {
        return Err(ExecError::Program(
            "exec request grant has trailing fields".to_string(),
        ));
    }
    if scopes.is_empty() {
        return Err(ExecError::Program(
            "exec request grant scopes must not be empty".to_string(),
        ));
    }
    Ok(Grant {
        facet,
        mode,
        scopes,
    })
}

fn decode_scope(value: CborValue) -> Result<Scope, ExecError> {
    let CborValue::Array(items) = value else {
        return Err(ExecError::Program(
            "exec request grant scope must be an array".to_string(),
        ));
    };
    let mut fields = items.into_iter();
    let scope = match fields.next() {
        Some(CborValue::Uint(0)) => Scope::All,
        Some(CborValue::Uint(1)) => match fields.next() {
            Some(CborValue::Text(prefix)) => Scope::Prefix(prefix),
            _ => {
                return Err(ExecError::Program(
                    "exec request prefix scope must include text".to_string(),
                ));
            }
        },
        Some(CborValue::Uint(_)) => {
            return Err(ExecError::Program(
                "exec request scope tag unknown".to_string(),
            ));
        }
        _ => {
            return Err(ExecError::Program(
                "exec request scope tag must be uint".to_string(),
            ));
        }
    };
    if fields.next().is_some() {
        return Err(ExecError::Program(
            "exec request scope has trailing fields".to_string(),
        ));
    }
    Ok(scope)
}

/// Dry-run a program on a fork of the context's base branch and return a reviewable proposal. The base
/// branch is untouched. A denied operation, malformed ABI input, or out-of-fuel program returns an
/// [`ExecError`] and no proposal is adopted.
/// Evaluate the manifest's guards for `phase` against the workspace's current KV state, failing closed.
/// The guard view is grant-scoped: every KV collection is enumerated, and only entries the manifest's
/// `Kv` read grant permits (checked against the canonical PEP target) enter the context, keyed by the
/// guard authoring form. A guard that is false, errors, or reaches ungranted state denies the run.
#[cfg(feature = "guards")]
fn check_guards<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    grants: &crate::capability::GrantSet,
    guards: &[crate::manifest::ManifestGuard],
    inputs: &crate::guard::StateView,
    phase: crate::manifest::GuardPhase,
) -> Result<(), ExecError> {
    if !guards.iter().any(|g| g.phase == phase) {
        return Ok(());
    }
    let mut view = crate::guard::StateView::new();
    for collection in loom.list_collections(ns, loom_core::FacetKind::Kv) {
        let map = loom_core::kv_list(loom, ns, &collection)?;
        let entries: Vec<(loom_core::tabular::Value, Vec<u8>)> =
            map.iter().map(|(k, v)| (k.clone(), v.to_vec())).collect();
        view.extend(crate::guard::guard_view_from_collection(
            grants,
            &collection,
            &entries,
        ));
    }
    for guard in guards.iter().filter(|g| g.phase == phase) {
        match crate::guard::evaluate_view(&guard.expr, &view, inputs, false) {
            Ok(true) => {}
            Ok(false) => {
                return Err(ExecError::Denied(format!(
                    "guard predicate failed: {}",
                    guard.expr
                )));
            }
            Err(e) => {
                return Err(ExecError::Denied(format!("guard evaluation error: {e:?}")));
            }
        }
    }
    Ok(())
}

pub fn dry_run<S: ObjectStore>(
    loom: &mut Loom<S>,
    req: ExecRequest<'_>,
) -> Result<ExecReport, ExecError> {
    let ExecRequest {
        context,
        fork_branch,
        step,
        author,
        message,
        timestamp_ms,
    } = req;
    let ns = context.workspace;
    let principal = context.principal;
    let base = context.base_branch.clone();

    validate_step(&context, &step)?;

    // Guards fold into manifest identity; enforce them here. Fail closed when a program declares guards
    // but this build cannot evaluate them (the `guards` feature is off).
    let guards = step.manifest.guards.clone();
    #[cfg(feature = "guards")]
    let guard_inputs: crate::guard::StateView = step.inputs.clone();
    // Guard visibility equals the program's read authority: scope the view by the step's manifest
    // grants, not the (broader) context upper bound.
    #[cfg(feature = "guards")]
    let guard_grants = step.manifest.grants.clone();
    #[cfg(not(feature = "guards"))]
    if !guards.is_empty() {
        return Err(ExecError::Denied(
            "program declares guards but the `guards` feature is not enabled".to_string(),
        ));
    }

    loom.registry().supports_branching(ns)?;
    let before = loom
        .registry()
        .branch_tip(ns, &base)?
        .ok_or_else(|| ExecError::Program(format!("base branch {base:?} has no commits")))?;

    // Fork the base branch and run the program against the fork's working state.
    loom.checkout_branch(ns, &base)?;
    // Precondition guards evaluate against the base state, before the program runs.
    #[cfg(feature = "guards")]
    check_guards(
        loom,
        ns,
        &guard_grants,
        &guards,
        &guard_inputs,
        crate::manifest::GuardPhase::Pre,
    )?;
    loom.branch(ns, &fork_branch)?;
    loom.checkout_branch(ns, &fork_branch)?;

    let state = StateAccess::new(loom, step_context(&context, &step));
    let (mut state, outcome) = match run_state(step.wasm, state, step.fuel, step.inputs) {
        Ok(result) => result,
        Err(err) => {
            discard_fork(loom, ns, &base, &fork_branch);
            return Err(err);
        }
    };
    let after = match state.commit(&author, &message, timestamp_ms) {
        Ok(after) => after,
        Err(err) => {
            drop(state);
            discard_fork(loom, ns, &base, &fork_branch);
            return Err(err);
        }
    };
    drop(state);

    // Postcondition / invariant guards evaluate against the proposed state, before it is accepted.
    #[cfg(feature = "guards")]
    if let Err(err) = check_guards(
        loom,
        ns,
        &guard_grants,
        &guards,
        &guard_inputs,
        crate::manifest::GuardPhase::Post,
    ) {
        discard_fork(loom, ns, &base, &fork_branch);
        return Err(err);
    }

    let after_root = loom.commit_tree(after)?;
    let changes = loom.diff(ns, before, after)?;

    Ok(ExecReport {
        principal,
        fork_branch,
        before,
        after,
        after_root,
        changes,
        fuel_used: outcome.fuel_used,
        logs: outcome.logs,
    })
}

fn discard_fork<S: ObjectStore>(loom: &mut Loom<S>, ns: WorkspaceId, base: &str, fork: &str) {
    let _ = loom.checkout_branch(ns, base);
    let _ = loom.branch_delete(ns, fork);
}

/// Run one program directly against the context's base branch and commit it if the full run succeeds.
pub fn direct<S: ObjectStore>(
    loom: &mut Loom<S>,
    req: DirectExecRequest<'_>,
) -> Result<ExecCommitReport, ExecError> {
    run_steps_on_base(
        loom,
        req.context,
        vec![req.step],
        &req.author,
        &req.message,
        req.timestamp_ms,
    )
}

/// Run several programs against one working state and commit once if every step succeeds.
pub fn batch<S: ObjectStore>(
    loom: &mut Loom<S>,
    req: BatchExecRequest<'_>,
) -> Result<ExecCommitReport, ExecError> {
    run_steps_on_base(
        loom,
        req.context,
        req.steps,
        &req.author,
        &req.message,
        req.timestamp_ms,
    )
}

/// Adopt a reviewed proposal by merging `fork_branch` into `base_branch`. Returns the merge outcome
/// (fast-forward, merge commit, up-to-date, or conflicts). The base branch advances only here.
pub fn apply<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    base_branch: &str,
    fork_branch: &str,
    author: &str,
    timestamp_ms: u64,
) -> Result<MergeOutcome, ExecError> {
    loom.checkout_branch(ns, base_branch)?;
    Ok(loom.merge(ns, fork_branch, author, timestamp_ms)?)
}

fn run_steps_on_base<S: ObjectStore>(
    loom: &mut Loom<S>,
    context: ExecContext,
    steps: Vec<ExecStep<'_>>,
    author: &str,
    message: &str,
    timestamp_ms: u64,
) -> Result<ExecCommitReport, ExecError> {
    if steps.is_empty() {
        return Err(ExecError::Program(
            "execution batch must include at least one step".to_string(),
        ));
    }

    let ns = context.workspace;
    let principal = context.principal;
    let branch = context.base_branch.clone();
    let before = loom
        .registry()
        .branch_tip(ns, &branch)?
        .ok_or_else(|| ExecError::Program(format!("base branch {branch:?} has no commits")))?;
    loom.checkout_branch(ns, &branch)?;

    let mut fuel_used = 0u64;
    let mut logs = Vec::new();
    for step in steps {
        validate_step(&context, &step)?;

        let guards = step.manifest.guards.clone();
        #[cfg(feature = "guards")]
        let guard_inputs: crate::guard::StateView = step.inputs.clone();
        #[cfg(feature = "guards")]
        let guard_grants = step.manifest.grants.clone();
        #[cfg(not(feature = "guards"))]
        if !guards.is_empty() {
            rollback_base(loom, ns, &branch, before);
            return Err(ExecError::Denied(
                "program declares guards but the `guards` feature is not enabled".to_string(),
            ));
        }

        // Precondition guards evaluate against the working state before this step runs.
        #[cfg(feature = "guards")]
        if let Err(e) = check_guards(
            loom,
            ns,
            &guard_grants,
            &guards,
            &guard_inputs,
            crate::manifest::GuardPhase::Pre,
        ) {
            rollback_base(loom, ns, &branch, before);
            return Err(e);
        }

        let state = StateAccess::new(loom, step_context(&context, &step));
        match run_state(step.wasm, state, step.fuel, step.inputs) {
            Ok((state, outcome)) => {
                drop(state);
                fuel_used = fuel_used.saturating_add(outcome.fuel_used);
                logs.extend(outcome.logs);
            }
            Err(err) => {
                rollback_base(loom, ns, &branch, before);
                return Err(err);
            }
        }

        // Postcondition guards evaluate against the working state after this step runs.
        #[cfg(feature = "guards")]
        if let Err(e) = check_guards(
            loom,
            ns,
            &guard_grants,
            &guards,
            &guard_inputs,
            crate::manifest::GuardPhase::Post,
        ) {
            rollback_base(loom, ns, &branch, before);
            return Err(e);
        }
    }

    let after = match loom.commit(ns, author, message, timestamp_ms) {
        Ok(commit) => commit,
        Err(err) => {
            rollback_base(loom, ns, &branch, before);
            return Err(err.into());
        }
    };
    let after_root = loom.commit_tree(after)?;
    let changes = loom.diff(ns, before, after)?;
    Ok(ExecCommitReport {
        principal,
        branch,
        before,
        after,
        after_root,
        changes,
        fuel_used,
        logs,
    })
}

fn validate_step(context: &ExecContext, step: &ExecStep<'_>) -> Result<(), ExecError> {
    if step.manifest.engine != "wasm"
        || step.manifest.abi_version != 1
        || step.manifest.entry != "run"
    {
        return Err(ExecError::Program(
            "exec step manifest must target wasm abi v1 entry run".to_string(),
        ));
    }
    let body = loom_core::Object::Blob(step.wasm.to_vec()).digest();
    if step.manifest.body != body {
        return Err(ExecError::Program(
            "exec step wasm body does not match manifest digest".to_string(),
        ));
    }
    if !step.manifest.grants.is_grantable() {
        return Err(ExecError::Denied(
            "manifest declares a non-grantable facet (Vcs or Program)".to_string(),
        ));
    }
    if !context.grants.covers(&step.manifest.grants) {
        return Err(ExecError::Denied(
            "exec context grants do not cover step manifest grants".to_string(),
        ));
    }
    Ok(())
}

fn step_context(context: &ExecContext, step: &ExecStep<'_>) -> ExecContext {
    let mut next = context.clone();
    next.grants = step.manifest.grants.clone();
    next
}

fn rollback_base<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    branch: &str,
    before: Digest,
) {
    let _ = loom.checkout_branch(ns, branch);
    let _ = loom.checkout_commit(ns, before);
}

struct ExecResultValue<'a> {
    mode: &'a str,
    committed: bool,
    principal: PrincipalId,
    branch: &'a str,
    before: Digest,
    after: Digest,
    after_root: Digest,
    changes: &'a [Change],
    fuel_used: u64,
    logs: &'a [String],
}

fn exec_result_value(result: ExecResultValue<'_>) -> CborValue {
    CborValue::Map(vec![
        text_pair("schema", CborValue::Text("loom.exec.result.v1".to_string())),
        text_pair("mode", CborValue::Text(result.mode.to_string())),
        text_pair("committed", CborValue::Bool(result.committed)),
        text_pair(
            "principal",
            CborValue::Bytes(result.principal.as_bytes().to_vec()),
        ),
        text_pair("branch", CborValue::Text(result.branch.to_string())),
        text_pair("before", digest_value(result.before)),
        text_pair("after", digest_value(result.after)),
        text_pair("after_root", digest_value(result.after_root)),
        text_pair(
            "changes",
            CborValue::Array(result.changes.iter().map(change_value).collect()),
        ),
        text_pair("fuel_used", CborValue::Uint(result.fuel_used)),
        text_pair(
            "logs",
            CborValue::Array(result.logs.iter().cloned().map(CborValue::Text).collect()),
        ),
    ])
}

fn digest_value(digest: Digest) -> CborValue {
    CborValue::Map(vec![
        text_pair("algo", CborValue::Text(digest.algo().as_str().to_string())),
        text_pair("bytes", CborValue::Bytes(digest.bytes().to_vec())),
    ])
}

fn change_value(change: &Change) -> CborValue {
    CborValue::Map(vec![
        text_pair("path", CborValue::Text(change.path.clone())),
        text_pair(
            "kind",
            CborValue::Text(change_kind(change.kind).to_string()),
        ),
    ])
}

fn change_kind(kind: ChangeKind) -> &'static str {
    match kind {
        ChangeKind::Added => "added",
        ChangeKind::Modified => "modified",
        ChangeKind::Deleted => "deleted",
    }
}

fn text_pair(key: &str, value: CborValue) -> (CborValue, CborValue) {
    (CborValue::Text(key.to_string()), value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::{Capability, Grant, GrantSet, Mode, Scope};
    use loom_codec::Value as CborValue;
    use loom_core::tabular::Value;
    use loom_core::{AclRight, AclSubject, FacetKind, MemoryStore, key_to_cbor, kv_get};

    fn nid(seed: u8) -> WorkspaceId {
        WorkspaceId::from_bytes([seed; 16])
    }

    fn pid(seed: u8) -> PrincipalId {
        PrincipalId::from_bytes([seed; 16])
    }

    // A workspace with one base commit and a principal allowed to Execute in it.
    fn seeded(seed: u8) -> (Loom<MemoryStore>, WorkspaceId, Digest) {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Files, None, nid(seed))
            .unwrap();
        loom.acl_store_mut()
            .allow(
                AclSubject::Principal(pid(9)),
                Some(ns),
                None,
                [AclRight::Execute],
            )
            .unwrap();
        loom.write_file(ns, "/seed", b"s", 0o100644).unwrap();
        let base = loom.commit(ns, "nas", "base", 1).unwrap();
        (loom, ns, base)
    }

    fn context(ns: WorkspaceId) -> ExecContext {
        ExecContext {
            workspace: ns,
            principal: pid(9),
            roles: Vec::new(),
            authenticated: true,
            base_branch: "main".to_string(),
            grants: GrantSet::new(vec![
                Grant {
                    facet: Capability::Files,
                    mode: Mode::ReadWrite,
                    scopes: vec![Scope::All],
                },
                Grant {
                    facet: Capability::Kv,
                    mode: Mode::ReadWrite,
                    scopes: vec![Scope::All],
                },
            ]),
        }
    }

    fn files_only_context(ns: WorkspaceId) -> ExecContext {
        ExecContext {
            workspace: ns,
            principal: pid(9),
            roles: Vec::new(),
            authenticated: true,
            base_branch: "main".to_string(),
            grants: GrantSet::new(vec![Grant {
                facet: Capability::Files,
                mode: Mode::ReadWrite,
                scopes: vec![Scope::All],
            }]),
        }
    }

    // Fetches a typed key from input `nk`, writes /out and cache/<key>=v through the host ABI.
    fn program() -> Vec<u8> {
        wat::parse_str(
            r#"(module
                 (import "env" "input_get" (func $in (param i32 i32 i32 i32) (result i32)))
                 (import "env" "kv_put" (func $put (param i32 i32 i32 i32 i32 i32)))
                 (import "env" "file_write" (func $fw (param i32 i32 i32 i32)))
                 (import "env" "log" (func $log (param i32 i32)))
                 (memory (export "memory") 1)
                 (data (i32.const 0) "cache")
                 (data (i32.const 16) "nk")
                 (data (i32.const 32) "v")
                 (data (i32.const 48) "/out")
                 (data (i32.const 64) "done")
                 (func (export "run") (local $l i32)
                   (call $fw (i32.const 48)(i32.const 4)(i32.const 32)(i32.const 1))
                   (local.set $l (call $in (i32.const 16)(i32.const 2)(i32.const 200)(i32.const 64)))
                   (call $put (i32.const 0)(i32.const 5)(i32.const 200)(local.get $l)(i32.const 32)(i32.const 1))
                   (call $log (i32.const 64)(i32.const 4))))"#,
        )
        .expect("assemble program")
    }

    fn second_program() -> Vec<u8> {
        wat::parse_str(
            r#"(module
                 (import "env" "file_write" (func $fw (param i32 i32 i32 i32)))
                 (import "env" "log" (func $log (param i32 i32)))
                 (memory (export "memory") 1)
                 (data (i32.const 0) "/second")
                 (data (i32.const 16) "w")
                 (data (i32.const 32) "two")
                 (func (export "run")
                   (call $fw (i32.const 0)(i32.const 7)(i32.const 16)(i32.const 1))
                   (call $log (i32.const 32)(i32.const 3))))"#,
        )
        .expect("assemble second program")
    }

    fn request<'a>(ns: WorkspaceId, wasm: &'a [u8], key: &Value) -> ExecRequest<'a> {
        ExecRequest {
            context: context(ns),
            fork_branch: "proposed".to_string(),
            step: step(wasm, key),
            author: "program".to_string(),
            message: "gated".to_string(),
            timestamp_ms: 2,
        }
    }

    fn all_grants() -> GrantSet {
        GrantSet::new(vec![
            Grant {
                facet: Capability::Files,
                mode: Mode::ReadWrite,
                scopes: vec![Scope::All],
            },
            Grant {
                facet: Capability::Kv,
                mode: Mode::ReadWrite,
                scopes: vec![Scope::All],
            },
        ])
    }

    fn files_grants() -> GrantSet {
        GrantSet::new(vec![Grant {
            facet: Capability::Files,
            mode: Mode::ReadWrite,
            scopes: vec![Scope::All],
        }])
    }

    fn manifest(wasm: &[u8], grants: GrantSet) -> Manifest {
        Manifest::for_wasm("test", wasm, grants)
    }

    fn step<'a>(wasm: &'a [u8], key: &Value) -> ExecStep<'a> {
        ExecStep {
            manifest: manifest(wasm, all_grants()),
            wasm,
            inputs: BTreeMap::from([("nk".to_string(), key_to_cbor(key))]),
            fuel: 1_000_000,
        }
    }

    fn map_get<'a>(value: &'a CborValue, key: &str) -> &'a CborValue {
        let CborValue::Map(entries) = value else {
            panic!("expected map")
        };
        entries
            .iter()
            .find_map(|(k, v)| match k {
                CborValue::Text(found) if found == key => Some(v),
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing key {key}"))
    }

    fn grant_cbor(grant: &Grant) -> CborValue {
        CborValue::Array(vec![
            CborValue::Uint(u64::from(grant.facet.stable_tag())),
            CborValue::Uint(u64::from(grant.mode.as_u8())),
            CborValue::Array(
                grant
                    .scopes
                    .iter()
                    .map(|scope| match scope {
                        Scope::All => CborValue::Array(vec![CborValue::Uint(0)]),
                        Scope::Prefix(prefix) => CborValue::Array(vec![
                            CborValue::Uint(1),
                            CborValue::Text(prefix.clone()),
                        ]),
                    })
                    .collect(),
            ),
        ])
    }

    fn grants_cbor(grants: &GrantSet) -> CborValue {
        CborValue::Array(grants.grants.iter().map(grant_cbor).collect())
    }

    struct RequestCbor<'a> {
        mode: &'a str,
        ns: WorkspaceId,
        context_grants: GrantSet,
        fork_branch: Option<&'a str>,
        steps: Vec<ExecStep<'a>>,
        message: &'a str,
        timestamp_ms: u64,
    }

    fn request_cbor(req: RequestCbor<'_>) -> Vec<u8> {
        let steps = req
            .steps
            .into_iter()
            .map(|step| {
                CborValue::Map(vec![
                    text_pair("manifest", CborValue::Bytes(step.manifest.encode())),
                    text_pair("wasm", CborValue::Bytes(step.wasm.to_vec())),
                    text_pair(
                        "inputs",
                        CborValue::Map(
                            step.inputs
                                .into_iter()
                                .map(|(k, v)| (CborValue::Text(k), CborValue::Bytes(v)))
                                .collect(),
                        ),
                    ),
                    text_pair("fuel", CborValue::Uint(step.fuel)),
                ])
            })
            .collect();
        loom_codec::encode(&CborValue::Map(vec![
            text_pair(
                "schema",
                CborValue::Text("loom.exec.request.v1".to_string()),
            ),
            text_pair("mode", CborValue::Text(req.mode.to_string())),
            text_pair("workspace", CborValue::Bytes(req.ns.as_bytes().to_vec())),
            text_pair("principal", CborValue::Bytes(pid(9).as_bytes().to_vec())),
            text_pair("roles", CborValue::Array(Vec::new())),
            text_pair("authenticated", CborValue::Bool(true)),
            text_pair("base_branch", CborValue::Text("main".to_string())),
            text_pair("grants", grants_cbor(&req.context_grants)),
            text_pair(
                "fork_branch",
                req.fork_branch
                    .map_or(CborValue::Null, |s| CborValue::Text(s.to_string())),
            ),
            text_pair("steps", CborValue::Array(steps)),
            text_pair("author", CborValue::Text("program".to_string())),
            text_pair("message", CborValue::Text(req.message.to_string())),
            text_pair("timestamp_ms", CborValue::Uint(req.timestamp_ms)),
        ]))
        .unwrap()
    }

    #[test]
    fn dry_run_proposes_without_touching_base_then_apply_adopts() {
        let (mut loom, ns, base) = seeded(1);
        let key = Value::Text("k".to_string());
        let wasm = program();
        let report = dry_run(&mut loom, request(ns, &wasm, &key)).unwrap();

        assert_eq!(report.before, base);
        assert_ne!(report.after, base);
        assert!(report.fuel_used > 0);
        assert_eq!(report.logs, vec!["done".to_string()]);
        assert!(report.changes.iter().any(|c| c.path == "out"));
        // The base branch is untouched until an explicit apply.
        assert_eq!(loom.registry().branch_tip(ns, "main").unwrap(), Some(base));

        let outcome = apply(&mut loom, ns, "main", "proposed", "nas", 3).unwrap();
        assert!(matches!(
            outcome,
            MergeOutcome::FastForward(_) | MergeOutcome::Merged(_)
        ));
        // The adopted state carries the program's file and KV writes.
        assert_eq!(loom.read_file(ns, "/out").unwrap(), b"v");
        assert_eq!(
            kv_get(&loom, ns, "cache", &key).unwrap(),
            Some(b"v".to_vec())
        );

        let envelope = loom_codec::decode(&report.to_cbor().unwrap()).unwrap();
        assert_eq!(
            map_get(&envelope, "schema"),
            &CborValue::Text("loom.exec.result.v1".to_string())
        );
        assert_eq!(
            map_get(&envelope, "mode"),
            &CborValue::Text("gated".to_string())
        );
        assert_eq!(map_get(&envelope, "committed"), &CborValue::Bool(false));
        assert_eq!(
            map_get(&envelope, "branch"),
            &CborValue::Text("proposed".to_string())
        );
    }

    #[test]
    fn dry_run_requires_base_commits() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Files, None, nid(2))
            .unwrap();
        let key = Value::Text("k".to_string());
        let wasm = program();
        // No base commit exists, so the dry run cannot fork; it fails rather than proposing.
        assert!(dry_run(&mut loom, request(ns, &wasm, &key)).is_err());
    }

    #[test]
    fn direct_commits_to_base_after_success() {
        let (mut loom, ns, base) = seeded(3);
        let key = Value::Text("k".to_string());
        let wasm = program();
        let report = direct(
            &mut loom,
            DirectExecRequest {
                context: context(ns),
                step: step(&wasm, &key),
                author: "program".to_string(),
                message: "direct".to_string(),
                timestamp_ms: 4,
            },
        )
        .unwrap();

        assert_eq!(report.branch, "main");
        assert_eq!(report.before, base);
        assert_eq!(
            loom.registry().branch_tip(ns, "main").unwrap(),
            Some(report.after)
        );
        assert_eq!(loom.read_file(ns, "/out").unwrap(), b"v");
        assert_eq!(
            kv_get(&loom, ns, "cache", &key).unwrap(),
            Some(b"v".to_vec())
        );

        let envelope =
            loom_codec::decode(&report.to_cbor(ExecCommitMode::Direct).unwrap()).unwrap();
        assert_eq!(
            map_get(&envelope, "mode"),
            &CborValue::Text("direct".to_string())
        );
        assert_eq!(map_get(&envelope, "committed"), &CborValue::Bool(true));
    }

    #[test]
    fn batch_commits_all_steps_once() {
        let (mut loom, ns, base) = seeded(4);
        let key = Value::Text("k".to_string());
        let first = program();
        let second = second_program();
        let report = batch(
            &mut loom,
            BatchExecRequest {
                context: context(ns),
                steps: vec![
                    step(&first, &key),
                    ExecStep {
                        manifest: manifest(&second, files_grants()),
                        wasm: &second,
                        inputs: BTreeMap::new(),
                        fuel: 1_000_000,
                    },
                ],
                author: "program".to_string(),
                message: "batch".to_string(),
                timestamp_ms: 5,
            },
        )
        .unwrap();

        assert_eq!(report.before, base);
        assert_eq!(
            loom.registry().branch_tip(ns, "main").unwrap(),
            Some(report.after)
        );
        assert_eq!(loom.read_file(ns, "/out").unwrap(), b"v");
        assert_eq!(loom.read_file(ns, "/second").unwrap(), b"w");
        assert_eq!(report.logs, vec!["done".to_string(), "two".to_string()]);

        let envelope = loom_codec::decode(&report.to_cbor(ExecCommitMode::Batch).unwrap()).unwrap();
        assert_eq!(
            map_get(&envelope, "mode"),
            &CborValue::Text("batch".to_string())
        );
        assert_eq!(map_get(&envelope, "committed"), &CborValue::Bool(true));
    }

    #[test]
    fn direct_denial_rolls_back_prior_guest_writes() {
        let (mut loom, ns, base) = seeded(5);
        let key = Value::Text("k".to_string());
        let wasm = program();
        let err = direct(
            &mut loom,
            DirectExecRequest {
                context: context(ns),
                step: ExecStep {
                    manifest: manifest(&wasm, files_grants()),
                    wasm: &wasm,
                    inputs: BTreeMap::from([("nk".to_string(), key_to_cbor(&key))]),
                    fuel: 1_000_000,
                },
                author: "program".to_string(),
                message: "direct denied".to_string(),
                timestamp_ms: 6,
            },
        )
        .unwrap_err();

        assert_eq!(err.code(), loom_core::Code::PermissionDenied);
        assert_eq!(loom.registry().branch_tip(ns, "main").unwrap(), Some(base));
        assert!(loom.read_file(ns, "/out").is_err());
        assert_eq!(kv_get(&loom, ns, "cache", &key).unwrap(), None);
    }

    #[test]
    fn step_grants_must_fit_context_upper_bound() {
        let (mut loom, ns, base) = seeded(6);
        let key = Value::Text("k".to_string());
        let wasm = program();
        let err = direct(
            &mut loom,
            DirectExecRequest {
                context: files_only_context(ns),
                step: step(&wasm, &key),
                author: "program".to_string(),
                message: "direct denied".to_string(),
                timestamp_ms: 6,
            },
        )
        .unwrap_err();

        assert!(matches!(err, ExecError::Denied(_)));
        assert_eq!(loom.registry().branch_tip(ns, "main").unwrap(), Some(base));
        assert!(loom.read_file(ns, "/out").is_err());
        assert_eq!(kv_get(&loom, ns, "cache", &key).unwrap(), None);
    }

    #[test]
    fn execute_cbor_direct_uses_public_request_envelope() {
        let (mut loom, ns, base) = seeded(7);
        let key = Value::Text("k".to_string());
        let wasm = program();
        let response = execute_cbor(
            &mut loom,
            &request_cbor(RequestCbor {
                mode: "direct",
                ns,
                context_grants: all_grants(),
                fork_branch: None,
                steps: vec![step(&wasm, &key)],
                message: "direct envelope",
                timestamp_ms: 7,
            }),
        )
        .unwrap();

        let envelope = loom_codec::decode(&response).unwrap();
        assert_eq!(
            map_get(&envelope, "schema"),
            &CborValue::Text("loom.exec.result.v1".to_string())
        );
        assert_eq!(
            map_get(&envelope, "mode"),
            &CborValue::Text("direct".to_string())
        );
        assert_eq!(map_get(&envelope, "committed"), &CborValue::Bool(true));
        assert_ne!(loom.registry().branch_tip(ns, "main").unwrap(), Some(base));
        assert_eq!(loom.read_file(ns, "/out").unwrap(), b"v");
        assert_eq!(
            kv_get(&loom, ns, "cache", &key).unwrap(),
            Some(b"v".to_vec())
        );
    }

    #[test]
    fn execute_cbor_batch_runs_each_step_manifest() {
        let (mut loom, ns, _) = seeded(8);
        let key = Value::Text("k".to_string());
        let first = program();
        let second = second_program();
        let response = execute_cbor(
            &mut loom,
            &request_cbor(RequestCbor {
                mode: "batch",
                ns,
                context_grants: all_grants(),
                fork_branch: None,
                steps: vec![
                    step(&first, &key),
                    ExecStep {
                        manifest: manifest(&second, files_grants()),
                        wasm: &second,
                        inputs: BTreeMap::new(),
                        fuel: 1_000_000,
                    },
                ],
                message: "batch envelope",
                timestamp_ms: 8,
            }),
        )
        .unwrap();

        let envelope = loom_codec::decode(&response).unwrap();
        assert_eq!(
            map_get(&envelope, "mode"),
            &CborValue::Text("batch".to_string())
        );
        assert_eq!(loom.read_file(ns, "/out").unwrap(), b"v");
        assert_eq!(loom.read_file(ns, "/second").unwrap(), b"w");
    }

    #[test]
    fn execute_cbor_rejects_manifest_body_mismatch() {
        let (mut loom, ns, base) = seeded(9);
        let key = Value::Text("k".to_string());
        let wasm = program();
        let different = second_program();
        let request = request_cbor(RequestCbor {
            mode: "direct",
            ns,
            context_grants: all_grants(),
            fork_branch: None,
            steps: vec![ExecStep {
                manifest: manifest(&different, files_grants()),
                wasm: &wasm,
                inputs: BTreeMap::from([("nk".to_string(), key_to_cbor(&key))]),
                fuel: 1_000_000,
            }],
            message: "bad body",
            timestamp_ms: 9,
        });
        let err = execute_cbor(&mut loom, &request).unwrap_err();

        assert!(matches!(err, ExecError::Program(_)));
        assert_eq!(loom.registry().branch_tip(ns, "main").unwrap(), Some(base));
        assert!(loom.read_file(ns, "/out").is_err());
    }
}
