//! Source-backed lifecycle hook registrations and PIM event envelopes.

use crate::acl::AclRight;
use crate::cbor::{self, Value};
use crate::digest::Digest;
use crate::error::{Code, LoomError, Result};
use crate::fs::FileKind;
use crate::provider::ObjectStore;
use crate::triggers::trigger_history;
use crate::vcs::Loom;
use crate::watch::WatchSelector;
use crate::workspace::{FacetKind, WorkspaceId, facet_path};
use loom_triggers::{
    MissedFirePolicy, OverlapPolicy, TriggerBinding, TriggerExecMode, TriggerFireCandidate,
    TriggerKind, TriggerOptions, TriggerStimulus, stimulus_digest,
};

const HOOK_COLLECTION: &str = "hooks";
const REGISTRATIONS_DIR: &str = "hooks/registrations";
const EVENTS_DIR: &str = "hooks/events";
const MAX_HOOK_DEPTH: u8 = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookRegistration {
    pub id: WorkspaceId,
    pub facet: FacetKind,
    pub event: String,
    pub scope: HookScope,
    pub predicate: Option<String>,
    pub program: Digest,
    pub branch: String,
    pub budget: u64,
    pub mode: TriggerExecMode,
    pub options: TriggerOptions,
    pub run_as: WorkspaceId,
    pub priority: i32,
    pub enabled: bool,
    pub required: bool,
    pub max_depth: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookScope {
    Facet,
    Principal {
        principal: String,
    },
    Collection {
        principal: String,
        collection: String,
    },
    Unit {
        principal: String,
        collection: String,
        unit: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PimEventEnvelope {
    pub workspace: WorkspaceId,
    pub facet: FacetKind,
    pub event: String,
    pub principal: String,
    pub collection: Option<String>,
    pub unit: Option<String>,
    pub commit: Option<Digest>,
    pub before: Option<Vec<u8>>,
    pub after: Option<Vec<u8>>,
    pub depth: u8,
    pub causation: Option<Digest>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookExecutionPlan {
    pub fires: Vec<TriggerFireCandidate>,
    pub refused: Vec<HookPolicyRefusal>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookPolicyRefusal {
    pub hook: WorkspaceId,
    pub reason: String,
}

impl HookRegistration {
    pub fn validate(&self) -> Result<()> {
        validate_event(&self.event)?;
        validate_scope(&self.scope)?;
        if let Some(predicate) = &self.predicate
            && predicate.trim().is_empty()
        {
            return Err(LoomError::invalid("hook predicate must not be empty"));
        }
        if self.branch.is_empty() || self.branch.contains('/') {
            return Err(LoomError::invalid("hook branch must be a ref segment"));
        }
        if self.budget == 0 {
            return Err(LoomError::invalid("hook budget must be nonzero"));
        }
        if self.max_depth == 0 || self.max_depth > MAX_HOOK_DEPTH {
            return Err(LoomError::invalid(format!(
                "hook max_depth must be between 1 and {MAX_HOOK_DEPTH}"
            )));
        }
        Ok(())
    }

    fn auth_scope(&self) -> String {
        self.scope.auth_scope()
    }
}

impl HookScope {
    pub fn matches(&self, envelope: &PimEventEnvelope) -> bool {
        match self {
            HookScope::Facet => true,
            HookScope::Principal { principal } => principal == &envelope.principal,
            HookScope::Collection {
                principal,
                collection,
            } => {
                principal == &envelope.principal
                    && envelope.collection.as_deref() == Some(collection.as_str())
            }
            HookScope::Unit {
                principal,
                collection,
                unit,
            } => {
                principal == &envelope.principal
                    && envelope.collection.as_deref() == Some(collection.as_str())
                    && envelope.unit.as_deref() == Some(unit.as_str())
            }
        }
    }

    fn auth_scope(&self) -> String {
        match self {
            HookScope::Facet => String::new(),
            HookScope::Principal { principal } => format!("{principal}/"),
            HookScope::Collection {
                principal,
                collection,
            }
            | HookScope::Unit {
                principal,
                collection,
                ..
            } => format!("{principal}/{collection}"),
        }
    }
}

impl PimEventEnvelope {
    pub fn validate(&self) -> Result<()> {
        validate_event(&self.event)?;
        validate_segment(&self.principal, "principal")?;
        if let Some(collection) = &self.collection {
            validate_segment(collection, "collection")?;
        }
        if let Some(unit) = &self.unit {
            validate_unit(unit)?;
        }
        if self.unit.is_some() && self.collection.is_none() {
            return Err(LoomError::invalid(
                "PIM event unit requires a collection or mailbox",
            ));
        }
        if self.depth > MAX_HOOK_DEPTH {
            return Err(LoomError::invalid(format!(
                "PIM event depth must be at most {MAX_HOOK_DEPTH}"
            )));
        }
        Ok(())
    }
}

pub fn hook_put<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    registration: &HookRegistration,
) -> Result<()> {
    registration.validate()?;
    loom.authorize_collection(
        ns,
        registration.facet,
        &registration.auth_scope(),
        AclRight::Admin,
    )?;
    loom.authorize_collection(ns, FacetKind::Program, HOOK_COLLECTION, AclRight::Write)?;
    loom.create_directory_reserved(ns, &facet_path(FacetKind::Program, REGISTRATIONS_DIR), true)?;
    loom.write_file_reserved(
        ns,
        &registration_path(registration.id),
        &hook_registration_to_cbor(registration)?,
        0o100644,
    )
}

pub fn hook_get<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    id: WorkspaceId,
) -> Result<HookRegistration> {
    loom.authorize_collection(ns, FacetKind::Program, HOOK_COLLECTION, AclRight::Read)?;
    let registration =
        hook_registration_from_cbor(&loom.read_file_reserved(ns, &registration_path(id))?)?;
    if registration.id != id {
        return Err(LoomError::corrupt(
            "hook registration id does not match path",
        ));
    }
    Ok(registration)
}

pub fn hook_list<S: ObjectStore>(loom: &Loom<S>, ns: WorkspaceId) -> Result<Vec<HookRegistration>> {
    loom.authorize_collection(ns, FacetKind::Program, HOOK_COLLECTION, AclRight::Read)?;
    hook_list_unchecked(loom, ns)
}

fn hook_list_unchecked<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
) -> Result<Vec<HookRegistration>> {
    let dir = facet_path(FacetKind::Program, REGISTRATIONS_DIR);
    let entries = match loom.list_directory(ns, &dir) {
        Ok(entries) => entries,
        Err(err) if err.code == Code::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err),
    };
    let mut registrations = Vec::new();
    for entry in entries {
        if entry.kind != FileKind::File || !entry.name.ends_with(".cbor") {
            continue;
        }
        registrations.push(hook_registration_from_cbor(
            &loom.read_file_reserved(ns, &format!("{dir}/{}", entry.name))?,
        )?);
    }
    sort_registrations(&mut registrations);
    Ok(registrations)
}

pub fn hook_list_matching<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    envelope: &PimEventEnvelope,
) -> Result<Vec<HookRegistration>> {
    envelope.validate()?;
    loom.authorize_collection(ns, FacetKind::Program, HOOK_COLLECTION, AclRight::Read)?;
    hook_list_matching_unchecked(loom, ns, envelope)
}

fn hook_list_matching_unchecked<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    envelope: &PimEventEnvelope,
) -> Result<Vec<HookRegistration>> {
    let mut registrations = hook_list_unchecked(loom, ns)?
        .into_iter()
        .filter(|registration| {
            registration.enabled
                && registration.facet == envelope.facet
                && registration.event == envelope.event
                && registration.scope.matches(envelope)
        })
        .collect::<Vec<_>>();
    sort_registrations(&mut registrations);
    Ok(registrations)
}

pub fn hook_emit_event<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    envelope: &PimEventEnvelope,
) -> Result<Option<Digest>> {
    envelope.validate()?;
    loom.authorize_collection(
        ns,
        envelope.facet,
        &event_auth_scope(envelope),
        AclRight::Write,
    )?;
    hook_emit_event_unchecked(loom, ns, envelope)
}

pub(crate) fn hook_emit_event_unchecked<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    envelope: &PimEventEnvelope,
) -> Result<Option<Digest>> {
    envelope.validate()?;
    if !hook_enforce_event_policy_unchecked(loom, ns, envelope)? {
        return Ok(None);
    }
    let bytes = hook_event_envelope_to_cbor(envelope)?;
    let digest = Digest::hash(loom.store().digest_algo(), &bytes);
    let dir = facet_path(
        FacetKind::Program,
        &format!(
            "{EVENTS_DIR}/{}/{}",
            envelope.facet.as_str(),
            envelope.event
        ),
    );
    loom.create_directory_reserved(ns, &dir, true)?;
    loom.write_file_reserved(
        ns,
        &format!("{dir}/{}.cbor", digest.to_hex()),
        &bytes,
        0o100644,
    )?;
    Ok(Some(digest))
}

fn hook_enforce_event_policy_unchecked<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    envelope: &PimEventEnvelope,
) -> Result<bool> {
    let event_digest = hook_event_identity_digest(loom, envelope)?;
    if envelope.causation == Some(event_digest) {
        return Err(hook_denied("hook event causation forms an immediate cycle"));
    }
    let mut fireable = false;
    for registration in hook_list_matching_unchecked(loom, ns, envelope)? {
        match validate_hook_policy(loom, &registration, envelope) {
            Ok(()) => fireable = true,
            Err(err) if registration.required || is_before_event(&registration.event) => {
                return Err(err);
            }
            Err(_) => {}
        }
    }
    Ok(fireable)
}

pub fn hook_event_history<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
) -> Result<Vec<PimEventEnvelope>> {
    loom.authorize_collection(ns, FacetKind::Program, HOOK_COLLECTION, AclRight::Read)?;
    let dir = facet_path(FacetKind::Program, EVENTS_DIR);
    let mut events = Vec::new();
    collect_event_history(loom, ns, &dir, &mut events)?;
    events.sort_by(|left, right| {
        (
            left.facet,
            left.event.as_str(),
            left.principal.as_str(),
            left.collection.as_deref(),
            left.unit.as_deref(),
            left.depth,
        )
            .cmp(&(
                right.facet,
                right.event.as_str(),
                right.principal.as_str(),
                right.collection.as_deref(),
                right.unit.as_deref(),
                right.depth,
            ))
    });
    Ok(events)
}

pub fn hook_plan_event<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    envelope: &PimEventEnvelope,
) -> Result<HookExecutionPlan> {
    envelope.validate()?;
    let event_digest = hook_event_identity_digest(loom, envelope)?;
    if envelope.causation == Some(event_digest) {
        return Err(hook_denied("hook event causation forms an immediate cycle"));
    }
    let mut fires = Vec::new();
    let mut refused = Vec::new();
    for registration in hook_list_matching(loom, ns, envelope)? {
        match validate_hook_policy(loom, &registration, envelope) {
            Ok(()) => fires.push(hook_candidate(loom, ns, &registration, envelope)?),
            Err(err) if registration.required || is_before_event(&registration.event) => {
                return Err(err);
            }
            Err(err) => refused.push(HookPolicyRefusal {
                hook: registration.id,
                reason: err.message,
            }),
        }
    }
    Ok(HookExecutionPlan { fires, refused })
}

pub fn hook_remove<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    id: WorkspaceId,
) -> Result<bool> {
    let registration = match hook_get(loom, ns, id) {
        Ok(registration) => registration,
        Err(err) if err.code == Code::NotFound => return Ok(false),
        Err(err) => return Err(err),
    };
    loom.authorize_collection(
        ns,
        registration.facet,
        &registration.auth_scope(),
        AclRight::Admin,
    )?;
    loom.authorize_collection(ns, FacetKind::Program, HOOK_COLLECTION, AclRight::Write)?;
    loom.remove_file_reserved(ns, &registration_path(id))?;
    Ok(true)
}

pub fn hook_registration_to_cbor(registration: &HookRegistration) -> Result<Vec<u8>> {
    registration.validate()?;
    Ok(cbor::encode(&Value::Array(vec![
        Value::Uint(2),
        workspace_value(registration.id),
        Value::Uint(u64::from(registration.facet.stable_tag())),
        Value::Text(registration.event.clone()),
        scope_value(&registration.scope),
        option_text_value(registration.predicate.as_deref()),
        digest_text_value(&registration.program),
        Value::Text(registration.branch.clone()),
        Value::Uint(registration.budget),
        exec_mode_value(registration.mode),
        trigger_options_value(registration.options),
        workspace_value(registration.run_as),
        Value::int(i64::from(registration.priority)),
        Value::Bool(registration.enabled),
        Value::Bool(registration.required),
        Value::Uint(u64::from(registration.max_depth)),
    ])))
}

pub fn hook_registration_from_cbor(bytes: &[u8]) -> Result<HookRegistration> {
    let mut fields = cbor::Fields::new(cbor::decode_array(bytes)?);
    let version = fields.uint()?;
    match version {
        1 | 2 => {}
        _ => return Err(LoomError::corrupt("unsupported hook registration version")),
    }
    let id = workspace_from_value(fields.next_field()?)?;
    let facet = facet_from_tag(fields.uint()?)?;
    let event = fields.text()?;
    let scope = scope_from_value(fields.next_field()?)?;
    let predicate = option_text_from_value(fields.next_field()?)?;
    let program = digest_text_from_value(fields.next_field()?)?;
    let (branch, budget, mode, options) = if version == 2 {
        (
            fields.text()?,
            fields.uint()?,
            exec_mode_from_value(fields.next_field()?)?,
            trigger_options_from_value(fields.next_field()?)?,
        )
    } else {
        (
            "main".to_string(),
            10_000,
            TriggerExecMode::Gated,
            TriggerOptions::default(),
        )
    };
    let run_as = workspace_from_value(fields.next_field()?)?;
    let priority = i32::try_from(fields.int()?)
        .map_err(|_| LoomError::corrupt("hook priority out of i32 range"))?;
    let enabled = fields.bool()?;
    let required = fields.bool()?;
    let max_depth = u8::try_from(fields.uint()?)
        .map_err(|_| LoomError::corrupt("hook max_depth out of u8 range"))?;
    fields.end()?;
    let registration = HookRegistration {
        id,
        facet,
        event,
        scope,
        predicate,
        program,
        branch,
        budget,
        mode,
        options,
        run_as,
        priority,
        enabled,
        required,
        max_depth,
    };
    registration.validate()?;
    Ok(registration)
}

pub fn hook_event_envelope_to_cbor(envelope: &PimEventEnvelope) -> Result<Vec<u8>> {
    envelope.validate()?;
    Ok(cbor::encode(&Value::Array(vec![
        Value::Uint(1),
        workspace_value(envelope.workspace),
        Value::Uint(u64::from(envelope.facet.stable_tag())),
        Value::Text(envelope.event.clone()),
        Value::Text(envelope.principal.clone()),
        option_text_value(envelope.collection.as_deref()),
        option_text_value(envelope.unit.as_deref()),
        option_digest_text_value(envelope.commit.as_ref()),
        option_bytes_value(envelope.before.as_deref()),
        option_bytes_value(envelope.after.as_deref()),
        Value::Uint(u64::from(envelope.depth)),
        option_digest_text_value(envelope.causation.as_ref()),
    ])))
}

pub fn hook_event_envelope_from_cbor(bytes: &[u8]) -> Result<PimEventEnvelope> {
    let mut fields = cbor::Fields::new(cbor::decode_array(bytes)?);
    match fields.uint()? {
        1 => {}
        _ => return Err(LoomError::corrupt("unsupported PIM event envelope version")),
    }
    let envelope = PimEventEnvelope {
        workspace: workspace_from_value(fields.next_field()?)?,
        facet: facet_from_tag(fields.uint()?)?,
        event: fields.text()?,
        principal: fields.text()?,
        collection: option_text_from_value(fields.next_field()?)?,
        unit: option_text_from_value(fields.next_field()?)?,
        commit: option_digest_text_from_value(fields.next_field()?)?,
        before: option_bytes_from_value(fields.next_field()?)?,
        after: option_bytes_from_value(fields.next_field()?)?,
        depth: u8::try_from(fields.uint()?)
            .map_err(|_| LoomError::corrupt("PIM event depth out of u8 range"))?,
        causation: option_digest_text_from_value(fields.next_field()?)?,
    };
    fields.end()?;
    envelope.validate()?;
    Ok(envelope)
}

fn registration_path(id: WorkspaceId) -> String {
    facet_path(
        FacetKind::Program,
        &format!("{REGISTRATIONS_DIR}/{id}.cbor"),
    )
}

fn sort_registrations(registrations: &mut [HookRegistration]) {
    registrations.sort_by_key(|registration| (registration.priority, registration.id));
}

fn hook_event_digest<S: ObjectStore>(
    loom: &Loom<S>,
    envelope: &PimEventEnvelope,
) -> Result<Digest> {
    Ok(Digest::hash(
        loom.store().digest_algo(),
        &hook_event_envelope_to_cbor(envelope)?,
    ))
}

fn hook_event_identity_digest<S: ObjectStore>(
    loom: &Loom<S>,
    envelope: &PimEventEnvelope,
) -> Result<Digest> {
    let mut identity = envelope.clone();
    identity.causation = None;
    hook_event_digest(loom, &identity)
}

fn validate_hook_policy<S: ObjectStore>(
    loom: &Loom<S>,
    registration: &HookRegistration,
    envelope: &PimEventEnvelope,
) -> Result<()> {
    if envelope.depth >= registration.max_depth {
        return Err(hook_denied("hook cascade depth exceeded"));
    }
    if let Some(identity) = loom.identity_store() {
        let principal = identity
            .principal(registration.run_as)
            .map_err(|_| hook_denied("hook run_as principal is missing"))?;
        if !principal.enabled {
            return Err(hook_denied("hook run_as principal is disabled"));
        }
    }
    Ok(())
}

fn hook_candidate<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    registration: &HookRegistration,
    envelope: &PimEventEnvelope,
) -> Result<TriggerFireCandidate> {
    let event_digest = hook_event_digest(loom, envelope)?;
    let commit = envelope.commit.unwrap_or(event_digest);
    let source_cursor = format!("pim-event:v1:{event_digest}");
    let stimulus = TriggerStimulus::Change {
        source_cursor,
        commit,
    };
    let stimulus_digest = stimulus_digest(crate::Algo::Blake3, &stimulus)?;
    let fired_at_seq = trigger_history(loom, ns, registration.id, 0, usize::MAX)?
        .into_iter()
        .map(|record| record.fired_at_seq)
        .max()
        .map_or(0, |seq| seq + 1);
    let watch = WatchSelector::new(envelope.workspace, &registration.branch)
        .map_err(|err| LoomError::invalid(format!("hook watch selector invalid: {err}")))?
        .with_facet(envelope.facet);
    Ok(TriggerFireCandidate {
        binding: TriggerBinding {
            id: registration.id,
            kind: TriggerKind::Change { watch },
            program: registration.program,
            target_workspace: envelope.workspace,
            branch: registration.branch.clone(),
            budget: registration.budget,
            mode: registration.mode,
            options: registration.options,
            run_as: Some(registration.run_as),
            enabled: registration.enabled,
        },
        stimulus,
        stimulus_digest,
        fired_at_seq,
    })
}

fn is_before_event(event: &str) -> bool {
    event.starts_with("before_")
}

fn hook_denied(message: &str) -> LoomError {
    LoomError::new(Code::TriggerDenied, message)
}

fn collect_event_history<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    dir: &str,
    events: &mut Vec<PimEventEnvelope>,
) -> Result<()> {
    let entries = match loom.list_directory(ns, dir) {
        Ok(entries) => entries,
        Err(err) if err.code == Code::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };
    for entry in entries {
        let path = format!("{dir}/{}", entry.name);
        match entry.kind {
            FileKind::File if entry.name.ends_with(".cbor") => {
                events.push(hook_event_envelope_from_cbor(
                    &loom.read_file_reserved(ns, &path)?,
                )?);
            }
            FileKind::Directory => collect_event_history(loom, ns, &path, events)?,
            _ => {}
        }
    }
    Ok(())
}

fn event_auth_scope(envelope: &PimEventEnvelope) -> String {
    match &envelope.collection {
        Some(collection) => format!("{}/{}", envelope.principal, collection),
        None => format!("{}/", envelope.principal),
    }
}

fn validate_event(event: &str) -> Result<()> {
    if event.is_empty() || event.contains('/') || event.chars().any(char::is_whitespace) {
        return Err(LoomError::invalid(format!("invalid hook event {event:?}")));
    }
    Ok(())
}

fn validate_scope(scope: &HookScope) -> Result<()> {
    match scope {
        HookScope::Facet => Ok(()),
        HookScope::Principal { principal } => validate_segment(principal, "principal"),
        HookScope::Collection {
            principal,
            collection,
        } => {
            validate_segment(principal, "principal")?;
            validate_segment(collection, "collection")
        }
        HookScope::Unit {
            principal,
            collection,
            unit,
        } => {
            validate_segment(principal, "principal")?;
            validate_segment(collection, "collection")?;
            validate_unit(unit)
        }
    }
}

fn validate_segment(seg: &str, what: &str) -> Result<()> {
    if seg.is_empty() || seg == "." || seg == ".." || seg.contains('/') || seg.starts_with('.') {
        return Err(LoomError::invalid(format!(
            "hook: invalid {what} segment {seg:?}"
        )));
    }
    Ok(())
}

fn validate_unit(unit: &str) -> Result<()> {
    if unit.is_empty() || unit.chars().any(char::is_control) {
        return Err(LoomError::invalid(format!("hook: invalid unit {unit:?}")));
    }
    Ok(())
}

fn workspace_value(id: WorkspaceId) -> Value {
    Value::Bytes(id.as_bytes().to_vec())
}

fn workspace_from_value(value: Value) -> Result<WorkspaceId> {
    let bytes = cbor::as_bytes(value)?;
    let bytes: [u8; 16] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| LoomError::corrupt("workspace id field is not 16 bytes"))?;
    Ok(WorkspaceId::from_bytes(bytes))
}

fn facet_from_tag(tag: u64) -> Result<FacetKind> {
    let tag = u8::try_from(tag).map_err(|_| LoomError::corrupt("facet tag out of range"))?;
    FacetKind::from_stable_tag(tag).ok_or_else(|| LoomError::corrupt("unknown facet tag"))
}

fn scope_value(scope: &HookScope) -> Value {
    match scope {
        HookScope::Facet => Value::Array(vec![Value::Uint(0)]),
        HookScope::Principal { principal } => Value::Array(vec![
            Value::Uint(1),
            Value::Text(principal.clone()),
            Value::Null,
            Value::Null,
        ]),
        HookScope::Collection {
            principal,
            collection,
        } => Value::Array(vec![
            Value::Uint(2),
            Value::Text(principal.clone()),
            Value::Text(collection.clone()),
            Value::Null,
        ]),
        HookScope::Unit {
            principal,
            collection,
            unit,
        } => Value::Array(vec![
            Value::Uint(3),
            Value::Text(principal.clone()),
            Value::Text(collection.clone()),
            Value::Text(unit.clone()),
        ]),
    }
}

fn scope_from_value(value: Value) -> Result<HookScope> {
    let mut fields = cbor::Fields::new(cbor::as_array(value)?);
    let scope = match fields.uint()? {
        0 => HookScope::Facet,
        1 => HookScope::Principal {
            principal: fields.text()?,
        },
        2 => HookScope::Collection {
            principal: fields.text()?,
            collection: fields.text()?,
        },
        3 => HookScope::Unit {
            principal: fields.text()?,
            collection: fields.text()?,
            unit: fields.text()?,
        },
        _ => return Err(LoomError::corrupt("unknown hook scope kind")),
    };
    for value in fields.map_or_trailing_values()? {
        if !matches!(value, Value::Null) {
            return Err(LoomError::corrupt("unexpected hook scope field"));
        }
    }
    validate_scope(&scope)?;
    Ok(scope)
}

fn option_text_value(value: Option<&str>) -> Value {
    value.map_or(Value::Null, |text| Value::Text(text.to_string()))
}

fn option_text_from_value(value: Value) -> Result<Option<String>> {
    match value {
        Value::Null => Ok(None),
        Value::Text(text) => Ok(Some(text)),
        _ => Err(LoomError::corrupt("expected optional text")),
    }
}

fn option_bytes_value(value: Option<&[u8]>) -> Value {
    value.map_or(Value::Null, |bytes| Value::Bytes(bytes.to_vec()))
}

fn option_bytes_from_value(value: Value) -> Result<Option<Vec<u8>>> {
    match value {
        Value::Null => Ok(None),
        Value::Bytes(bytes) => Ok(Some(bytes)),
        _ => Err(LoomError::corrupt("expected optional bytes")),
    }
}

fn digest_text_value(digest: &Digest) -> Value {
    Value::Text(digest.to_string())
}

fn digest_text_from_value(value: Value) -> Result<Digest> {
    Digest::parse(&cbor::as_text(value)?)
}

fn option_digest_text_value(value: Option<&Digest>) -> Value {
    value.map_or(Value::Null, digest_text_value)
}

fn option_digest_text_from_value(value: Value) -> Result<Option<Digest>> {
    match value {
        Value::Null => Ok(None),
        Value::Text(text) => Digest::parse(&text).map(Some),
        _ => Err(LoomError::corrupt("expected optional digest text")),
    }
}

fn exec_mode_value(mode: TriggerExecMode) -> Value {
    Value::Text(
        match mode {
            TriggerExecMode::Gated => "gated",
            TriggerExecMode::Direct => "direct",
            TriggerExecMode::Batched => "batched",
        }
        .to_string(),
    )
}

fn exec_mode_from_value(value: Value) -> Result<TriggerExecMode> {
    Ok(match cbor::as_text(value)?.as_str() {
        "gated" => TriggerExecMode::Gated,
        "direct" => TriggerExecMode::Direct,
        "batched" => TriggerExecMode::Batched,
        _ => return Err(LoomError::corrupt("unknown hook exec mode")),
    })
}

fn trigger_options_value(options: TriggerOptions) -> Value {
    Value::Array(vec![
        missed_policy_value(options.missed),
        Value::Bool(options.catch_up),
        Value::Uint(u64::from(options.jitter_ms)),
        overlap_policy_value(options.overlap),
    ])
}

fn trigger_options_from_value(value: Value) -> Result<TriggerOptions> {
    let mut fields = cbor::Fields::new(cbor::as_array(value)?);
    let missed = missed_policy_from_value(fields.next_field()?)?;
    let catch_up = fields.bool()?;
    let jitter_ms = u32::try_from(fields.uint()?)
        .map_err(|_| LoomError::corrupt("hook jitter_ms out of u32 range"))?;
    let overlap = overlap_policy_from_value(fields.next_field()?)?;
    fields.end()?;
    Ok(TriggerOptions {
        missed,
        catch_up,
        jitter_ms,
        overlap,
    })
}

fn missed_policy_value(policy: MissedFirePolicy) -> Value {
    Value::Text(
        match policy {
            MissedFirePolicy::Skip => "skip",
            MissedFirePolicy::Collapse => "collapse",
            MissedFirePolicy::Backfill => "backfill",
        }
        .to_string(),
    )
}

fn missed_policy_from_value(value: Value) -> Result<MissedFirePolicy> {
    Ok(match cbor::as_text(value)?.as_str() {
        "skip" => MissedFirePolicy::Skip,
        "collapse" => MissedFirePolicy::Collapse,
        "backfill" => MissedFirePolicy::Backfill,
        _ => return Err(LoomError::corrupt("unknown hook missed-fire policy")),
    })
}

fn overlap_policy_value(policy: OverlapPolicy) -> Value {
    Value::Text(
        match policy {
            OverlapPolicy::SkipIfRunning => "skip-if-running",
            OverlapPolicy::Allow => "allow",
            OverlapPolicy::Queue => "queue",
        }
        .to_string(),
    )
}

fn overlap_policy_from_value(value: Value) -> Result<OverlapPolicy> {
    Ok(match cbor::as_text(value)?.as_str() {
        "skip-if-running" => OverlapPolicy::SkipIfRunning,
        "allow" => OverlapPolicy::Allow,
        "queue" => OverlapPolicy::Queue,
        _ => return Err(LoomError::corrupt("unknown hook overlap policy")),
    })
}

trait ScopeFields {
    fn map_or_trailing_values(self) -> Result<Vec<Value>>;
}

impl ScopeFields for cbor::Fields {
    fn map_or_trailing_values(mut self) -> Result<Vec<Value>> {
        let mut values = Vec::new();
        while let Ok(value) = self.next_field() {
            values.push(value);
        }
        Ok(values)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::{self, CalendarEntry, CollectionMeta, Component};
    use crate::contacts::{self, BookMeta, ContactEntry};
    use crate::mail::{self, MailboxMeta};
    use crate::provider::memory::MemoryStore;

    const RAW: &[u8] = b"From: a@example.com\r\nTo: b@example.com\r\nSubject: hello\r\nMessage-ID: <m1@example.com>\r\nDate: Tue, 01 Jan 2030 00:00:00 +0000\r\n\r\nBody";

    fn id(seed: u8) -> WorkspaceId {
        WorkspaceId::from_bytes([seed; 16])
    }

    fn registration(seed: u8, priority: i32, enabled: bool) -> HookRegistration {
        HookRegistration {
            id: id(seed),
            facet: FacetKind::Mail,
            event: "on_message_ingested".to_string(),
            scope: HookScope::Collection {
                principal: "user@example.com".to_string(),
                collection: "inbox".to_string(),
            },
            predicate: Some("headers.subject.contains('invoice')".to_string()),
            program: Digest::hash(crate::Algo::Sha256, format!("program-{seed}").as_bytes()),
            branch: "main".to_string(),
            budget: 10_000,
            mode: TriggerExecMode::Gated,
            options: TriggerOptions::default(),
            run_as: id(90),
            priority,
            enabled,
            required: false,
            max_depth: 4,
        }
    }

    fn domain_registration(
        seed: u8,
        facet: FacetKind,
        event: &str,
        collection: &str,
    ) -> HookRegistration {
        HookRegistration {
            id: id(seed),
            facet,
            event: event.to_string(),
            scope: HookScope::Collection {
                principal: "alice".to_string(),
                collection: collection.to_string(),
            },
            predicate: None,
            program: Digest::blake3(format!("program-{seed}").as_bytes()),
            branch: "main".to_string(),
            budget: 10_000,
            mode: TriggerExecMode::Gated,
            options: TriggerOptions::default(),
            run_as: id(90),
            priority: 0,
            enabled: true,
            required: false,
            max_depth: 4,
        }
    }

    #[test]
    fn hook_registration_round_trips_canonical_cbor() {
        let hook = registration(1, -10, true);
        let bytes = hook_registration_to_cbor(&hook).unwrap();
        let decoded = hook_registration_from_cbor(&bytes).unwrap();

        assert_eq!(decoded, hook);
        assert_eq!(hook_registration_to_cbor(&decoded).unwrap(), bytes);
    }

    #[test]
    fn pim_event_envelope_round_trips_canonical_cbor() {
        let envelope = PimEventEnvelope {
            workspace: id(7),
            facet: FacetKind::Calendar,
            event: "on_event_added".to_string(),
            principal: "user@example.com".to_string(),
            collection: Some("work".to_string()),
            unit: Some("event-1".to_string()),
            commit: Some(Digest::blake3(b"commit")),
            before: None,
            after: Some(b"calendar-record".to_vec()),
            depth: 1,
            causation: Some(Digest::hash(crate::Algo::Sha256, b"parent-event")),
        };

        let bytes = hook_event_envelope_to_cbor(&envelope).unwrap();
        let decoded = hook_event_envelope_from_cbor(&bytes).unwrap();

        assert_eq!(decoded, envelope);
        assert_eq!(hook_event_envelope_to_cbor(&decoded).unwrap(), bytes);
    }

    #[test]
    fn hook_storage_lists_by_priority_and_matches_scope() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Program, Some("program"), id(1))
            .unwrap();
        loom.registry_mut().add_facet(ns, FacetKind::Mail).unwrap();
        hook_put(&mut loom, ns, &registration(3, 20, true)).unwrap();
        hook_put(&mut loom, ns, &registration(2, 10, false)).unwrap();
        hook_put(&mut loom, ns, &registration(4, 10, true)).unwrap();

        let listed = hook_list(&loom, ns).unwrap();

        assert_eq!(
            listed
                .iter()
                .map(|registration| registration.id)
                .collect::<Vec<_>>(),
            vec![id(2), id(4), id(3)]
        );

        let matched = hook_list_matching(
            &loom,
            ns,
            &PimEventEnvelope {
                workspace: ns,
                facet: FacetKind::Mail,
                event: "on_message_ingested".to_string(),
                principal: "user@example.com".to_string(),
                collection: Some("inbox".to_string()),
                unit: Some("message-1".to_string()),
                commit: Some(Digest::blake3(b"commit")),
                before: None,
                after: Some(b"mail-record".to_vec()),
                depth: 0,
                causation: None,
            },
        )
        .unwrap();

        assert_eq!(
            matched
                .iter()
                .map(|registration| registration.id)
                .collect::<Vec<_>>(),
            vec![id(4), id(3)]
        );

        assert!(hook_remove(&mut loom, ns, id(4)).unwrap());
        assert_eq!(hook_list(&loom, ns).unwrap().len(), 2);
    }

    #[test]
    fn pim_facets_emit_registered_lifecycle_events() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Program, Some("program"), id(1))
            .unwrap();
        for facet in [
            FacetKind::Calendar,
            FacetKind::Contacts,
            FacetKind::Mail,
            FacetKind::Cas,
        ] {
            loom.registry_mut().add_facet(ns, facet).unwrap();
        }
        for registration in [
            domain_registration(10, FacetKind::Calendar, "on_event_added", "work"),
            domain_registration(11, FacetKind::Contacts, "on_contact_added", "people"),
            domain_registration(12, FacetKind::Mail, "on_message_ingested", "inbox"),
            domain_registration(13, FacetKind::Mail, "on_flags_changed", "inbox"),
            domain_registration(14, FacetKind::Mail, "on_moved", "archive"),
        ] {
            hook_put(&mut loom, ns, &registration).unwrap();
        }

        calendar::create_collection(
            &mut loom,
            ns,
            "alice",
            "work",
            &CollectionMeta {
                display_name: "Work".to_string(),
                component_set: vec![Component::Event],
            },
        )
        .unwrap();
        calendar::put_entry(
            &mut loom,
            ns,
            "alice",
            "work",
            &CalendarEntry::event("event/1", "Planning", "20300101T090000Z"),
        )
        .unwrap();
        contacts::create_book(
            &mut loom,
            ns,
            "alice",
            "people",
            &BookMeta {
                display_name: "People".to_string(),
            },
        )
        .unwrap();
        contacts::put_entry(
            &mut loom,
            ns,
            "alice",
            "people",
            &ContactEntry::new("contact/1", "Ada Example"),
        )
        .unwrap();
        mail::create_mailbox(
            &mut loom,
            ns,
            "alice",
            "inbox",
            &MailboxMeta {
                display_name: "Inbox".to_string(),
            },
        )
        .unwrap();
        mail::create_mailbox(
            &mut loom,
            ns,
            "alice",
            "archive",
            &MailboxMeta {
                display_name: "Archive".to_string(),
            },
        )
        .unwrap();
        mail::ingest_message(&mut loom, ns, "alice", "inbox", "m1", RAW).unwrap();
        mail::set_flags(&mut loom, ns, "alice", "inbox", "m1", &["seen".to_string()]).unwrap();
        assert!(
            mail::move_message(&mut loom, ns, "alice", "inbox", "m1", "archive", "m2").unwrap()
        );

        let history = hook_event_history(&loom, ns).unwrap();
        let events = history
            .iter()
            .map(|event| {
                (
                    event.facet,
                    event.event.as_str(),
                    event.collection.as_deref(),
                    event.unit.as_deref(),
                    event.before.is_some(),
                    event.after.is_some(),
                )
            })
            .collect::<Vec<_>>();

        assert!(events.contains(&(
            FacetKind::Calendar,
            "on_event_added",
            Some("work"),
            Some("event/1"),
            false,
            true
        )));
        assert!(events.contains(&(
            FacetKind::Contacts,
            "on_contact_added",
            Some("people"),
            Some("contact/1"),
            false,
            true
        )));
        assert!(events.contains(&(
            FacetKind::Mail,
            "on_message_ingested",
            Some("inbox"),
            Some("m1"),
            false,
            true
        )));
        assert!(events.contains(&(
            FacetKind::Mail,
            "on_flags_changed",
            Some("inbox"),
            Some("m1"),
            true,
            true
        )));
        assert!(events.contains(&(
            FacetKind::Mail,
            "on_moved",
            Some("archive"),
            Some("m2"),
            true,
            true
        )));
    }

    #[test]
    fn hook_policy_plan_orders_candidates_and_refuses_depth_or_loop() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Program, Some("program"), id(1))
            .unwrap();
        loom.registry_mut().add_facet(ns, FacetKind::Mail).unwrap();
        hook_put(&mut loom, ns, &registration(3, 20, true)).unwrap();
        hook_put(&mut loom, ns, &registration(2, 10, true)).unwrap();
        let envelope = PimEventEnvelope {
            workspace: ns,
            facet: FacetKind::Mail,
            event: "on_message_ingested".to_string(),
            principal: "user@example.com".to_string(),
            collection: Some("inbox".to_string()),
            unit: Some("message-1".to_string()),
            commit: Some(Digest::blake3(b"commit")),
            before: None,
            after: Some(b"mail-record".to_vec()),
            depth: 0,
            causation: None,
        };

        let plan = hook_plan_event(&loom, ns, &envelope).unwrap();

        assert!(plan.refused.is_empty());
        assert_eq!(
            plan.fires
                .iter()
                .map(|candidate| candidate.binding.id)
                .collect::<Vec<_>>(),
            vec![id(2), id(3)]
        );
        assert_eq!(plan.fires[0].binding.run_as, Some(id(90)));
        assert_eq!(plan.fires[0].binding.budget, 10_000);
        assert_eq!(plan.fires[0].binding.mode, TriggerExecMode::Gated);
        assert_eq!(plan.fires[0].fired_at_seq, 0);

        let mut shallow = registration(4, 0, true);
        shallow.max_depth = 1;
        shallow.required = false;
        hook_put(&mut loom, ns, &shallow).unwrap();
        let mut nested = envelope.clone();
        nested.depth = 1;
        let plan = hook_plan_event(&loom, ns, &nested).unwrap();

        assert!(plan.refused.iter().any(|refusal| refusal.hook == id(4)));

        let mut required = registration(5, 0, true);
        required.event = "before_create".to_string();
        required.max_depth = 1;
        required.required = true;
        hook_put(&mut loom, ns, &required).unwrap();
        let mut before = nested.clone();
        before.event = "before_create".to_string();
        let err = hook_plan_event(&loom, ns, &before).unwrap_err();
        assert_eq!(err.code, Code::TriggerDenied);
        let err = hook_emit_event_unchecked(&mut loom, ns, &before).unwrap_err();
        assert_eq!(err.code, Code::TriggerDenied);

        let mut cyclic = envelope.clone();
        cyclic.causation = Some(hook_event_identity_digest(&loom, &cyclic).unwrap());
        let err = hook_plan_event(&loom, ns, &cyclic).unwrap_err();
        assert_eq!(err.code, Code::TriggerDenied);
    }
}
