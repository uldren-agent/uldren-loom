//! Reusable trigger binding and fire-record contracts.

use loom_codec::Value;
use loom_types::{Algo, ChangeKind, Code, Digest, FacetKind, LoomError, Result, WorkspaceId};
use loom_watch::WatchSelector;
use std::str::FromStr;

pub const TRIGGER_BINDING_SCHEMA: &str = "loom.trigger.binding.v1";
pub const TRIGGER_FIRE_RECORD_SCHEMA: &str = "loom.trigger.fire.v2";
pub const TRIGGER_STIMULUS_SCHEMA: &str = "loom.trigger.stimulus.v1";

pub type TriggerId = WorkspaceId;
pub type PrincipalId = WorkspaceId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerBinding {
    pub id: TriggerId,
    pub kind: TriggerKind,
    pub program: Digest,
    pub target_workspace: WorkspaceId,
    pub branch: String,
    pub budget: u64,
    pub mode: TriggerExecMode,
    pub options: TriggerOptions,
    pub run_as: Option<PrincipalId>,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TriggerKind {
    Time { cron: String, timezone: String },
    Change { watch: WatchSelector },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerExecMode {
    Gated,
    Direct,
    Batched,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TriggerOptions {
    pub missed: MissedFirePolicy,
    pub catch_up: bool,
    pub jitter_ms: u32,
    pub overlap: OverlapPolicy,
}

impl Default for TriggerOptions {
    fn default() -> Self {
        Self {
            missed: MissedFirePolicy::Skip,
            catch_up: false,
            jitter_ms: 0,
            overlap: OverlapPolicy::SkipIfRunning,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MissedFirePolicy {
    Skip,
    Collapse,
    Backfill,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlapPolicy {
    SkipIfRunning,
    Allow,
    Queue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TriggerStimulus {
    Time {
        fired_at_ms: u64,
    },
    Change {
        source_cursor: String,
        commit: Digest,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FireOutcome {
    Applied,
    Proposed,
    Skipped,
    Denied,
    BudgetExceeded,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FireRecord {
    pub binding: TriggerId,
    pub stimulus: TriggerStimulus,
    pub stimulus_digest: Digest,
    pub proposed: Option<Digest>,
    pub outcome: FireOutcome,
    pub cost: u64,
    pub fired_at_seq: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerFireCandidate {
    pub binding: TriggerBinding,
    pub stimulus: TriggerStimulus,
    pub stimulus_digest: Digest,
    pub fired_at_seq: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerKeeperPlan {
    pub fires: Vec<TriggerFireCandidate>,
    pub next_wakeup_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeTriggerEvaluation {
    pub due: Vec<TriggerStimulus>,
    pub next_wakeup_ms: Option<u64>,
}

pub fn trigger_binding_to_cbor(binding: &TriggerBinding) -> Result<Vec<u8>> {
    encode_value(&binding_value(binding))
}

pub fn trigger_binding_from_cbor(bytes: &[u8]) -> Result<TriggerBinding> {
    let mut fields = take_array(decode_value(bytes, "trigger binding")?)?.into_iter();
    let schema = take_text(next_field(&mut fields, "trigger binding")?)?;
    if schema != TRIGGER_BINDING_SCHEMA {
        return Err(LoomError::corrupt("unknown trigger binding schema"));
    }
    let id = parse_id(&take_text(next_field(&mut fields, "trigger binding")?)?)?;
    let kind = take_kind(next_field(&mut fields, "trigger binding")?)?;
    let program = parse_digest(&take_text(next_field(&mut fields, "trigger binding")?)?)?;
    let target_workspace = parse_id(&take_text(next_field(&mut fields, "trigger binding")?)?)?;
    let branch = take_text(next_field(&mut fields, "trigger binding")?)?;
    let budget = take_u64(next_field(&mut fields, "trigger binding")?)?;
    let mode = take_exec_mode(next_field(&mut fields, "trigger binding")?)?;
    let options = take_options(next_field(&mut fields, "trigger binding")?)?;
    let run_as = take_optional_id(next_field(&mut fields, "trigger binding")?)?;
    let enabled = take_bool(next_field(&mut fields, "trigger binding")?)?;
    ensure_end(fields, "trigger binding")?;
    Ok(TriggerBinding {
        id,
        kind,
        program,
        target_workspace,
        branch,
        budget,
        mode,
        options,
        run_as,
        enabled,
    })
}

pub fn fire_record_to_cbor(record: &FireRecord) -> Result<Vec<u8>> {
    encode_value(&fire_record_value(record))
}

pub fn trigger_stimulus_to_cbor(stimulus: &TriggerStimulus) -> Result<Vec<u8>> {
    encode_value(&stimulus_value(stimulus))
}

pub fn fire_record_from_cbor(bytes: &[u8]) -> Result<FireRecord> {
    let mut fields = take_array(decode_value(bytes, "trigger fire record")?)?.into_iter();
    let schema = take_text(next_field(&mut fields, "trigger fire record")?)?;
    if schema != TRIGGER_FIRE_RECORD_SCHEMA {
        return Err(LoomError::corrupt("unknown trigger fire record schema"));
    }
    let binding = parse_id(&take_text(next_field(&mut fields, "trigger fire record")?)?)?;
    let stimulus = take_stimulus(next_field(&mut fields, "trigger fire record")?)?;
    let digest = parse_digest(&take_text(next_field(&mut fields, "trigger fire record")?)?)?;
    if stimulus_digest(Algo::Blake3, &stimulus)? != digest {
        return Err(LoomError::corrupt(
            "trigger fire record stimulus digest does not match stimulus",
        ));
    }
    let proposed = take_optional_digest(next_field(&mut fields, "trigger fire record")?)?;
    let outcome = take_outcome(next_field(&mut fields, "trigger fire record")?)?;
    let cost = take_u64(next_field(&mut fields, "trigger fire record")?)?;
    let fired_at_seq = take_u64(next_field(&mut fields, "trigger fire record")?)?;
    ensure_end(fields, "trigger fire record")?;
    Ok(FireRecord {
        binding,
        stimulus,
        stimulus_digest: digest,
        proposed,
        outcome,
        cost,
        fired_at_seq,
    })
}

pub fn stimulus_digest(algo: Algo, stimulus: &TriggerStimulus) -> Result<Digest> {
    Ok(Digest::hash(
        algo,
        &encode_value(&stimulus_value(stimulus))?,
    ))
}

pub fn evaluate_time_trigger(
    binding: &TriggerBinding,
    last: Option<&TriggerStimulus>,
    now_ms: u64,
    max_due: usize,
) -> Result<TimeTriggerEvaluation> {
    let TriggerKind::Time { cron, timezone } = &binding.kind else {
        return Err(LoomError::new(
            Code::InvalidArgument,
            "trigger binding is not a time trigger",
        ));
    };
    let schedule = croner::Cron::from_str(cron)
        .map_err(|err| LoomError::new(Code::InvalidArgument, format!("invalid cron: {err}")))?;
    let timezone = parse_timezone(timezone)?;
    let now = utc_ms(now_ms)?.with_timezone(&timezone);
    let start_ms = match last {
        Some(TriggerStimulus::Time { fired_at_ms }) => *fired_at_ms,
        Some(TriggerStimulus::Change { .. }) => {
            return Err(LoomError::corrupt(
                "time trigger history contains a change stimulus watermark",
            ));
        }
        None => now_ms,
    };
    let mut cursor = utc_ms(start_ms)?.with_timezone(&timezone);
    let mut due_ms = Vec::new();
    let next_wakeup_ms;
    loop {
        let next = schedule
            .find_next_occurrence(&cursor, false)
            .map_err(|err| {
                LoomError::new(
                    Code::InvalidArgument,
                    format!("cron next occurrence failed: {err}"),
                )
            })?;
        let next_ms = datetime_ms(next.with_timezone(&chrono::Utc))?;
        if next > now || due_ms.len() == max_due {
            next_wakeup_ms = Some(next_ms);
            break;
        }
        due_ms.push(next_ms);
        cursor = next;
    }
    let due_ms = if binding.options.catch_up {
        select_due_instants(due_ms, binding.options.missed)
    } else {
        due_ms.into_iter().rev().take(1).collect::<Vec<_>>()
    };
    let mut due = due_ms
        .into_iter()
        .map(|fired_at_ms| TriggerStimulus::Time { fired_at_ms })
        .collect::<Vec<_>>();
    due.sort_by_key(|stimulus| match stimulus {
        TriggerStimulus::Time { fired_at_ms } => *fired_at_ms,
        TriggerStimulus::Change { .. } => 0,
    });
    Ok(TimeTriggerEvaluation {
        due,
        next_wakeup_ms,
    })
}

fn binding_value(binding: &TriggerBinding) -> Value {
    Value::Array(vec![
        Value::Text(TRIGGER_BINDING_SCHEMA.to_string()),
        Value::Text(binding.id.to_string()),
        kind_value(&binding.kind),
        Value::Text(binding.program.to_string()),
        Value::Text(binding.target_workspace.to_string()),
        Value::Text(binding.branch.clone()),
        Value::Uint(binding.budget),
        Value::Text(exec_mode_tag(binding.mode).to_string()),
        options_value(binding.options),
        optional_id(binding.run_as),
        Value::Bool(binding.enabled),
    ])
}

fn kind_value(kind: &TriggerKind) -> Value {
    match kind {
        TriggerKind::Time { cron, timezone } => Value::Array(vec![
            Value::Text("time".to_string()),
            Value::Text(cron.clone()),
            Value::Text(timezone.clone()),
        ]),
        TriggerKind::Change { watch } => Value::Array(vec![
            Value::Text("change".to_string()),
            watch_selector_value(watch),
        ]),
    }
}

fn watch_selector_value(selector: &WatchSelector) -> Value {
    Value::Array(vec![
        Value::Text(selector.workspace.to_string()),
        Value::Text(selector.branch.clone()),
        selector
            .facet
            .map(|facet| Value::Text(facet.as_str().to_string()))
            .unwrap_or(Value::Null),
        selector
            .path_prefix
            .as_ref()
            .map(|prefix| Value::Text(prefix.clone()))
            .unwrap_or(Value::Null),
        Value::Array(
            selector
                .change_kinds
                .iter()
                .map(|kind| Value::Text(change_kind_tag(*kind).to_string()))
                .collect(),
        ),
    ])
}

fn options_value(options: TriggerOptions) -> Value {
    Value::Array(vec![
        Value::Text(missed_policy_tag(options.missed).to_string()),
        Value::Bool(options.catch_up),
        Value::Uint(options.jitter_ms as u64),
        Value::Text(overlap_policy_tag(options.overlap).to_string()),
    ])
}

fn fire_record_value(record: &FireRecord) -> Value {
    Value::Array(vec![
        Value::Text(TRIGGER_FIRE_RECORD_SCHEMA.to_string()),
        Value::Text(record.binding.to_string()),
        stimulus_value(&record.stimulus),
        Value::Text(record.stimulus_digest.to_string()),
        optional_digest(record.proposed),
        Value::Text(outcome_tag(record.outcome).to_string()),
        Value::Uint(record.cost),
        Value::Uint(record.fired_at_seq),
    ])
}

fn stimulus_value(stimulus: &TriggerStimulus) -> Value {
    match stimulus {
        TriggerStimulus::Time { fired_at_ms } => Value::Array(vec![
            Value::Text(TRIGGER_STIMULUS_SCHEMA.to_string()),
            Value::Text("time".to_string()),
            Value::Uint(*fired_at_ms),
        ]),
        TriggerStimulus::Change {
            source_cursor,
            commit,
        } => Value::Array(vec![
            Value::Text(TRIGGER_STIMULUS_SCHEMA.to_string()),
            Value::Text("change".to_string()),
            Value::Text(source_cursor.clone()),
            Value::Text(commit.to_string()),
        ]),
    }
}

fn take_kind(value: Value) -> Result<TriggerKind> {
    let mut fields = take_array(value)?.into_iter();
    let tag = take_text(next_field(&mut fields, "trigger kind")?)?;
    let kind = match tag.as_str() {
        "time" => {
            let cron = take_text(next_field(&mut fields, "trigger kind")?)?;
            let timezone = take_text(next_field(&mut fields, "trigger kind")?)?;
            TriggerKind::Time { cron, timezone }
        }
        "change" => TriggerKind::Change {
            watch: take_watch_selector(next_field(&mut fields, "trigger kind")?)?,
        },
        _ => return Err(LoomError::corrupt("unknown trigger kind")),
    };
    ensure_end(fields, "trigger kind")?;
    Ok(kind)
}

fn take_watch_selector(value: Value) -> Result<WatchSelector> {
    let mut fields = take_array(value)?.into_iter();
    let workspace = parse_id(&take_text(next_field(&mut fields, "watch selector")?)?)?;
    let branch = take_text(next_field(&mut fields, "watch selector")?)?;
    let facet = take_optional_facet(next_field(&mut fields, "watch selector")?)?;
    let path_prefix = take_optional_text(next_field(&mut fields, "watch selector")?)?;
    let change_kinds = take_array(next_field(&mut fields, "watch selector")?)?
        .into_iter()
        .map(take_change_kind)
        .collect::<Result<Vec<_>>>()?;
    ensure_end(fields, "watch selector")?;
    Ok(WatchSelector {
        workspace,
        branch,
        facet,
        path_prefix,
        change_kinds,
    })
}

fn take_options(value: Value) -> Result<TriggerOptions> {
    let mut fields = take_array(value)?.into_iter();
    let missed = take_missed_policy(next_field(&mut fields, "trigger options")?)?;
    let catch_up = take_bool(next_field(&mut fields, "trigger options")?)?;
    let jitter_ms = u32::try_from(take_u64(next_field(&mut fields, "trigger options")?)?)
        .map_err(|_| LoomError::corrupt("trigger jitter exceeds u32"))?;
    let overlap = take_overlap_policy(next_field(&mut fields, "trigger options")?)?;
    ensure_end(fields, "trigger options")?;
    Ok(TriggerOptions {
        missed,
        catch_up,
        jitter_ms,
        overlap,
    })
}

fn select_due_instants(mut due_ms: Vec<u64>, policy: MissedFirePolicy) -> Vec<u64> {
    match policy {
        MissedFirePolicy::Backfill => due_ms,
        MissedFirePolicy::Collapse | MissedFirePolicy::Skip => due_ms.pop().into_iter().collect(),
    }
}

fn parse_timezone(timezone: &str) -> Result<chrono_tz::Tz> {
    if timezone.eq_ignore_ascii_case("utc") {
        return Ok(chrono_tz::UTC);
    }
    timezone
        .parse::<chrono_tz::Tz>()
        .map_err(|_| LoomError::new(Code::InvalidArgument, "invalid trigger timezone"))
}

fn utc_ms(ms: u64) -> Result<chrono::DateTime<chrono::Utc>> {
    let ms = i64::try_from(ms)
        .map_err(|_| LoomError::new(Code::InvalidArgument, "trigger timestamp exceeds i64"))?;
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(ms)
        .ok_or_else(|| LoomError::new(Code::InvalidArgument, "invalid trigger timestamp"))
}

fn datetime_ms(datetime: chrono::DateTime<chrono::Utc>) -> Result<u64> {
    u64::try_from(datetime.timestamp_millis())
        .map_err(|_| LoomError::new(Code::InvalidArgument, "trigger timestamp is negative"))
}

fn take_stimulus(value: Value) -> Result<TriggerStimulus> {
    let mut fields = take_array(value)?.into_iter();
    let schema = take_text(next_field(&mut fields, "trigger stimulus")?)?;
    if schema != TRIGGER_STIMULUS_SCHEMA {
        return Err(LoomError::corrupt("unknown trigger stimulus schema"));
    }
    let tag = take_text(next_field(&mut fields, "trigger stimulus")?)?;
    let stimulus = match tag.as_str() {
        "time" => TriggerStimulus::Time {
            fired_at_ms: take_u64(next_field(&mut fields, "trigger stimulus")?)?,
        },
        "change" => TriggerStimulus::Change {
            source_cursor: take_text(next_field(&mut fields, "trigger stimulus")?)?,
            commit: parse_digest(&take_text(next_field(&mut fields, "trigger stimulus")?)?)?,
        },
        _ => return Err(LoomError::corrupt("unknown trigger stimulus kind")),
    };
    ensure_end(fields, "trigger stimulus")?;
    Ok(stimulus)
}

fn encode_value(value: &Value) -> Result<Vec<u8>> {
    loom_codec::encode(value)
        .map_err(|err| LoomError::corrupt(format!("trigger canonical CBOR encode failed: {err}")))
}

fn decode_value(bytes: &[u8], label: &str) -> Result<Value> {
    loom_codec::decode(bytes)
        .map_err(|err| LoomError::corrupt(format!("invalid {label} CBOR: {err}")))
}

fn parse_id(value: &str) -> Result<WorkspaceId> {
    WorkspaceId::parse(value).map_err(|_| LoomError::corrupt("invalid trigger id"))
}

fn parse_digest(value: &str) -> Result<Digest> {
    Digest::parse(value).map_err(|_| LoomError::corrupt("invalid trigger digest"))
}

fn optional_id(value: Option<WorkspaceId>) -> Value {
    value
        .map(|id| Value::Text(id.to_string()))
        .unwrap_or(Value::Null)
}

fn optional_digest(value: Option<Digest>) -> Value {
    value
        .map(|digest| Value::Text(digest.to_string()))
        .unwrap_or(Value::Null)
}

fn take_optional_id(value: Value) -> Result<Option<WorkspaceId>> {
    take_optional_text(value)?
        .map(|value| parse_id(&value))
        .transpose()
}

fn take_optional_digest(value: Value) -> Result<Option<Digest>> {
    take_optional_text(value)?
        .map(|value| parse_digest(&value))
        .transpose()
}

fn take_optional_facet(value: Value) -> Result<Option<FacetKind>> {
    take_optional_text(value)?
        .map(|facet| {
            FacetKind::parse(&facet).map_err(|_| LoomError::corrupt("invalid trigger facet"))
        })
        .transpose()
}

fn take_optional_text(value: Value) -> Result<Option<String>> {
    match value {
        Value::Null => Ok(None),
        Value::Text(value) => Ok(Some(value)),
        _ => Err(LoomError::corrupt("trigger optional text field is invalid")),
    }
}

fn take_array(value: Value) -> Result<Vec<Value>> {
    match value {
        Value::Array(value) => Ok(value),
        _ => Err(LoomError::corrupt("trigger field must be an array")),
    }
}

fn next_field(fields: &mut impl Iterator<Item = Value>, label: &str) -> Result<Value> {
    fields
        .next()
        .ok_or_else(|| LoomError::corrupt(format!("{label} is missing a field")))
}

fn ensure_end(mut fields: impl Iterator<Item = Value>, label: &str) -> Result<()> {
    if fields.next().is_some() {
        return Err(LoomError::corrupt(format!("{label} has extra fields")));
    }
    Ok(())
}

fn take_text(value: Value) -> Result<String> {
    match value {
        Value::Text(value) => Ok(value),
        _ => Err(LoomError::corrupt("trigger text field is invalid")),
    }
}

fn take_u64(value: Value) -> Result<u64> {
    match value {
        Value::Uint(value) => Ok(value),
        _ => Err(LoomError::corrupt("trigger u64 field is invalid")),
    }
}

fn take_bool(value: Value) -> Result<bool> {
    match value {
        Value::Bool(value) => Ok(value),
        _ => Err(LoomError::corrupt("trigger bool field is invalid")),
    }
}

fn take_change_kind(value: Value) -> Result<ChangeKind> {
    match take_text(value)?.as_str() {
        "added" => Ok(ChangeKind::Added),
        "modified" => Ok(ChangeKind::Modified),
        "deleted" => Ok(ChangeKind::Deleted),
        _ => Err(LoomError::corrupt("unknown trigger change kind")),
    }
}

fn take_exec_mode(value: Value) -> Result<TriggerExecMode> {
    match take_text(value)?.as_str() {
        "gated" => Ok(TriggerExecMode::Gated),
        "direct" => Ok(TriggerExecMode::Direct),
        "batched" => Ok(TriggerExecMode::Batched),
        _ => Err(LoomError::corrupt("unknown trigger exec mode")),
    }
}

fn take_missed_policy(value: Value) -> Result<MissedFirePolicy> {
    match take_text(value)?.as_str() {
        "skip" => Ok(MissedFirePolicy::Skip),
        "collapse" => Ok(MissedFirePolicy::Collapse),
        "backfill" => Ok(MissedFirePolicy::Backfill),
        _ => Err(LoomError::corrupt("unknown trigger missed-fire policy")),
    }
}

fn take_overlap_policy(value: Value) -> Result<OverlapPolicy> {
    match take_text(value)?.as_str() {
        "skip-if-running" => Ok(OverlapPolicy::SkipIfRunning),
        "allow" => Ok(OverlapPolicy::Allow),
        "queue" => Ok(OverlapPolicy::Queue),
        _ => Err(LoomError::corrupt("unknown trigger overlap policy")),
    }
}

fn take_outcome(value: Value) -> Result<FireOutcome> {
    match take_text(value)?.as_str() {
        "applied" => Ok(FireOutcome::Applied),
        "proposed" => Ok(FireOutcome::Proposed),
        "skipped" => Ok(FireOutcome::Skipped),
        "denied" => Ok(FireOutcome::Denied),
        "budget-exceeded" => Ok(FireOutcome::BudgetExceeded),
        "error" => Ok(FireOutcome::Error),
        _ => Err(LoomError::corrupt("unknown trigger fire outcome")),
    }
}

fn change_kind_tag(kind: ChangeKind) -> &'static str {
    match kind {
        ChangeKind::Added => "added",
        ChangeKind::Modified => "modified",
        ChangeKind::Deleted => "deleted",
    }
}

fn exec_mode_tag(mode: TriggerExecMode) -> &'static str {
    match mode {
        TriggerExecMode::Gated => "gated",
        TriggerExecMode::Direct => "direct",
        TriggerExecMode::Batched => "batched",
    }
}

fn missed_policy_tag(policy: MissedFirePolicy) -> &'static str {
    match policy {
        MissedFirePolicy::Skip => "skip",
        MissedFirePolicy::Collapse => "collapse",
        MissedFirePolicy::Backfill => "backfill",
    }
}

fn overlap_policy_tag(policy: OverlapPolicy) -> &'static str {
    match policy {
        OverlapPolicy::SkipIfRunning => "skip-if-running",
        OverlapPolicy::Allow => "allow",
        OverlapPolicy::Queue => "queue",
    }
}

fn outcome_tag(outcome: FireOutcome) -> &'static str {
    match outcome {
        FireOutcome::Applied => "applied",
        FireOutcome::Proposed => "proposed",
        FireOutcome::Skipped => "skipped",
        FireOutcome::Denied => "denied",
        FireOutcome::BudgetExceeded => "budget-exceeded",
        FireOutcome::Error => "error",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(seed: u8) -> WorkspaceId {
        WorkspaceId::from_bytes([seed; 16])
    }

    #[test]
    fn time_binding_round_trips() {
        let binding = TriggerBinding {
            id: id(1),
            kind: TriggerKind::Time {
                cron: "0 0 * * * *".to_string(),
                timezone: "UTC".to_string(),
            },
            program: Digest::blake3(b"program"),
            target_workspace: id(2),
            branch: "main".to_string(),
            budget: 10_000,
            mode: TriggerExecMode::Gated,
            options: TriggerOptions {
                missed: MissedFirePolicy::Collapse,
                catch_up: true,
                jitter_ms: 500,
                overlap: OverlapPolicy::Queue,
            },
            run_as: Some(id(9)),
            enabled: true,
        };

        let decoded =
            trigger_binding_from_cbor(&trigger_binding_to_cbor(&binding).unwrap()).unwrap();

        assert_eq!(decoded, binding);
    }

    #[test]
    fn change_binding_round_trips() {
        let selector = WatchSelector::new(id(2), "main")
            .unwrap()
            .with_facet(FacetKind::Files)
            .with_path_prefix("src/")
            .with_change_kind(ChangeKind::Modified);
        let binding = TriggerBinding {
            id: id(3),
            kind: TriggerKind::Change { watch: selector },
            program: Digest::blake3(b"program"),
            target_workspace: id(2),
            branch: "main".to_string(),
            budget: 1,
            mode: TriggerExecMode::Direct,
            options: TriggerOptions::default(),
            run_as: None,
            enabled: false,
        };

        let decoded =
            trigger_binding_from_cbor(&trigger_binding_to_cbor(&binding).unwrap()).unwrap();

        assert_eq!(decoded, binding);
    }

    #[test]
    fn fire_record_round_trips_and_stimulus_hash_is_stable() {
        let stimulus = TriggerStimulus::Change {
            source_cursor: "loom-watch-v1|cursor".to_string(),
            commit: Digest::blake3(b"commit"),
        };
        let digest = stimulus_digest(Algo::Blake3, &stimulus).unwrap();
        let record = FireRecord {
            binding: id(1),
            stimulus,
            stimulus_digest: digest,
            proposed: Some(Digest::blake3(b"proposal")),
            outcome: FireOutcome::Proposed,
            cost: 42,
            fired_at_seq: 7,
        };

        let decoded = fire_record_from_cbor(&fire_record_to_cbor(&record).unwrap()).unwrap();

        assert_eq!(decoded, record);
        assert_eq!(
            stimulus_digest(
                Algo::Blake3,
                &TriggerStimulus::Change {
                    source_cursor: "loom-watch-v1|cursor".to_string(),
                    commit: Digest::blake3(b"commit"),
                },
            )
            .unwrap(),
            digest
        );
    }

    #[test]
    fn time_trigger_backfills_due_instants_and_reports_next_wakeup() {
        let mut binding = TriggerBinding {
            id: id(1),
            kind: TriggerKind::Time {
                cron: "0 * * * * *".to_string(),
                timezone: "UTC".to_string(),
            },
            program: Digest::blake3(b"program"),
            target_workspace: id(2),
            branch: "main".to_string(),
            budget: 10_000,
            mode: TriggerExecMode::Gated,
            options: TriggerOptions::default(),
            run_as: None,
            enabled: true,
        };
        binding.options.catch_up = true;
        binding.options.missed = MissedFirePolicy::Backfill;

        let evaluation = evaluate_time_trigger(
            &binding,
            Some(&TriggerStimulus::Time { fired_at_ms: 0 }),
            180_000,
            10,
        )
        .unwrap();

        assert_eq!(
            evaluation.due,
            vec![
                TriggerStimulus::Time {
                    fired_at_ms: 60_000,
                },
                TriggerStimulus::Time {
                    fired_at_ms: 120_000,
                },
                TriggerStimulus::Time {
                    fired_at_ms: 180_000,
                },
            ]
        );
        assert_eq!(evaluation.next_wakeup_ms, Some(240_000));
    }

    #[test]
    fn time_trigger_collapse_keeps_latest_due_instant() {
        let mut binding = TriggerBinding {
            id: id(1),
            kind: TriggerKind::Time {
                cron: "0 * * * * *".to_string(),
                timezone: "America/New_York".to_string(),
            },
            program: Digest::blake3(b"program"),
            target_workspace: id(2),
            branch: "main".to_string(),
            budget: 10_000,
            mode: TriggerExecMode::Gated,
            options: TriggerOptions::default(),
            run_as: None,
            enabled: true,
        };
        binding.options.catch_up = true;
        binding.options.missed = MissedFirePolicy::Collapse;

        let evaluation = evaluate_time_trigger(
            &binding,
            Some(&TriggerStimulus::Time { fired_at_ms: 0 }),
            180_000,
            10,
        )
        .unwrap();

        assert_eq!(
            evaluation.due,
            vec![TriggerStimulus::Time {
                fired_at_ms: 180_000,
            }]
        );
        assert_eq!(evaluation.next_wakeup_ms, Some(240_000));
    }
}
