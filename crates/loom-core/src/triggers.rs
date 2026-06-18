//! Engine storage facade for trigger bindings.

use crate::AclRight;
use crate::error::{Code, LoomError, Result};
use crate::fs::FileKind;
use crate::provider::ObjectStore;
use crate::vcs::Loom;
use crate::workspace::{FacetKind, WorkspaceId, facet_path};
use loom_triggers::{
    FireRecord, TriggerBinding, TriggerFireCandidate, TriggerId, TriggerKeeperPlan, TriggerKind,
    TriggerStimulus, evaluate_time_trigger, fire_record_from_cbor, fire_record_to_cbor,
    stimulus_digest, trigger_binding_from_cbor, trigger_binding_to_cbor,
};
use loom_watch::WatchCursor;
use std::collections::BTreeSet;

const TRIGGER_COLLECTION: &str = "triggers";
const BINDINGS_DIR: &str = "triggers/bindings";
const FIRE_LOG_DIR: &str = "triggers/fire-log";

pub fn trigger_put<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    binding: &TriggerBinding,
) -> Result<()> {
    loom.authorize_collection(ns, FacetKind::Program, TRIGGER_COLLECTION, AclRight::Write)?;
    put_binding_unchecked(loom, ns, binding)
}

pub fn trigger_get<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    id: TriggerId,
) -> Result<TriggerBinding> {
    loom.authorize_collection(ns, FacetKind::Program, TRIGGER_COLLECTION, AclRight::Read)?;
    let binding = trigger_binding_from_cbor(&loom.read_file_reserved(ns, &binding_path(id))?)?;
    if binding.id != id {
        return Err(LoomError::corrupt("trigger binding id does not match path"));
    }
    Ok(binding)
}

pub fn trigger_list<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
) -> Result<Vec<TriggerBinding>> {
    loom.authorize_collection(ns, FacetKind::Program, TRIGGER_COLLECTION, AclRight::Read)?;
    let dir = facet_path(FacetKind::Program, BINDINGS_DIR);
    let entries = match loom.list_directory(ns, &dir) {
        Ok(entries) => entries,
        Err(err) if err.code == Code::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err),
    };
    let mut bindings = Vec::new();
    for entry in entries {
        if entry.kind != FileKind::File || !entry.name.ends_with(".cbor") {
            continue;
        }
        let path = format!("{dir}/{}", entry.name);
        bindings.push(trigger_binding_from_cbor(
            &loom.read_file_reserved(ns, &path)?,
        )?);
    }
    bindings.sort_by_key(|binding| binding.id);
    Ok(bindings)
}

pub fn trigger_enable<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    id: TriggerId,
    enabled: bool,
) -> Result<TriggerBinding> {
    loom.authorize_collection(ns, FacetKind::Program, TRIGGER_COLLECTION, AclRight::Write)?;
    let mut binding = trigger_get_unchecked(loom, ns, id)?;
    binding.enabled = enabled;
    put_binding_unchecked(loom, ns, &binding)?;
    Ok(binding)
}

pub fn trigger_remove<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    id: TriggerId,
) -> Result<bool> {
    loom.authorize_collection(ns, FacetKind::Program, TRIGGER_COLLECTION, AclRight::Write)?;
    match loom.read_file_reserved(ns, &binding_path(id)) {
        Ok(_) => {
            loom.remove_file_reserved(ns, &binding_path(id))?;
            Ok(true)
        }
        Err(err) if err.code == Code::NotFound => Ok(false),
        Err(err) => Err(err),
    }
}

pub fn trigger_history<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    id: TriggerId,
    from_seq: u64,
    limit: usize,
) -> Result<Vec<FireRecord>> {
    loom.authorize_collection(ns, FacetKind::Program, TRIGGER_COLLECTION, AclRight::Read)?;
    trigger_history_unchecked(loom, ns, id, from_seq, limit)
}

pub fn trigger_history_system<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    id: TriggerId,
    from_seq: u64,
    limit: usize,
) -> Result<Vec<FireRecord>> {
    trigger_history_unchecked(loom, ns, id, from_seq, limit)
}

fn trigger_history_unchecked<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    id: TriggerId,
    from_seq: u64,
    limit: usize,
) -> Result<Vec<FireRecord>> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let dir = fire_log_dir(id);
    let entries = match loom.list_directory(ns, &dir) {
        Ok(entries) => entries,
        Err(err) if err.code == Code::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err),
    };
    let mut records = Vec::new();
    for entry in entries {
        if records.len() == limit || entry.kind != FileKind::File || !entry.name.ends_with(".cbor")
        {
            continue;
        }
        let Some(seq) = entry
            .name
            .strip_suffix(".cbor")
            .and_then(|name| name.parse::<u64>().ok())
        else {
            continue;
        };
        if seq < from_seq {
            continue;
        }
        let path = format!("{dir}/{}", entry.name);
        records.push(fire_record_from_cbor(&loom.read_file_reserved(ns, &path)?)?);
    }
    records.sort_by_key(|record| record.fired_at_seq);
    Ok(records)
}

pub fn trigger_append_fire_record<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    record: &FireRecord,
) -> Result<()> {
    loom.authorize_collection(ns, FacetKind::Program, TRIGGER_COLLECTION, AclRight::Write)?;
    trigger_append_fire_record_unchecked(loom, ns, record)
}

pub fn trigger_append_fire_record_system<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    record: &FireRecord,
) -> Result<()> {
    trigger_append_fire_record_unchecked(loom, ns, record)
}

fn trigger_append_fire_record_unchecked<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    record: &FireRecord,
) -> Result<()> {
    let dir = fire_log_dir(record.binding);
    let path = fire_record_path(record.binding, record.fired_at_seq);
    if loom.read_file_reserved(ns, &path).is_ok() {
        return Err(LoomError::new(
            Code::AlreadyExists,
            "trigger fire record sequence already exists",
        ));
    }
    loom.create_directory_reserved(ns, &dir, true)?;
    loom.write_file_reserved(ns, &path, &fire_record_to_cbor(record)?, 0o100644)
}

pub fn trigger_keeper_due<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    now_ms: u64,
    max_fires: usize,
) -> Result<TriggerKeeperPlan> {
    if max_fires == 0 {
        return Ok(TriggerKeeperPlan {
            fires: Vec::new(),
            next_wakeup_ms: None,
        });
    }
    let mut fires = Vec::new();
    let mut next_wakeup_ms = None;
    for binding in trigger_list(loom, ns)? {
        if !binding.enabled || fires.len() == max_fires {
            continue;
        }
        let history = trigger_history(loom, ns, binding.id, 0, usize::MAX)?;
        let recorded = history
            .iter()
            .map(|record| record.stimulus_digest)
            .collect::<BTreeSet<_>>();
        let mut next_seq = history
            .iter()
            .map(|record| record.fired_at_seq)
            .max()
            .map_or(0, |seq| seq + 1);
        match &binding.kind {
            TriggerKind::Time { .. } => {
                let last = latest_time_stimulus(&history);
                let evaluation =
                    evaluate_time_trigger(&binding, last, now_ms, max_fires - fires.len())?;
                next_wakeup_ms = min_wakeup(next_wakeup_ms, evaluation.next_wakeup_ms);
                for stimulus in evaluation.due {
                    let digest = stimulus_digest(crate::Algo::Blake3, &stimulus)?;
                    if recorded.contains(&digest) {
                        continue;
                    }
                    fires.push(TriggerFireCandidate {
                        binding: binding.clone(),
                        stimulus,
                        stimulus_digest: digest,
                        fired_at_seq: next_seq,
                    });
                    next_seq += 1;
                }
            }
            TriggerKind::Change { watch } => {
                let cursor = change_trigger_cursor(loom, &binding, &history)?;
                let remaining = max_fires - fires.len();
                let batch = loom.watch_poll(&cursor, remaining)?;
                for event in batch.events {
                    let source_cursor =
                        WatchCursor::from_selector(watch, Some(event.commit)).encode();
                    let stimulus = TriggerStimulus::Change {
                        source_cursor,
                        commit: event.commit,
                    };
                    let digest = stimulus_digest(crate::Algo::Blake3, &stimulus)?;
                    if recorded.contains(&digest) {
                        continue;
                    }
                    fires.push(TriggerFireCandidate {
                        binding: binding.clone(),
                        stimulus,
                        stimulus_digest: digest,
                        fired_at_seq: next_seq,
                    });
                    next_seq += 1;
                }
            }
        }
    }
    Ok(TriggerKeeperPlan {
        fires,
        next_wakeup_ms,
    })
}

fn trigger_get_unchecked<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    id: TriggerId,
) -> Result<TriggerBinding> {
    let binding = trigger_binding_from_cbor(&loom.read_file_reserved(ns, &binding_path(id))?)?;
    if binding.id != id {
        return Err(LoomError::corrupt("trigger binding id does not match path"));
    }
    Ok(binding)
}

fn put_binding_unchecked<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    binding: &TriggerBinding,
) -> Result<()> {
    loom.create_directory_reserved(ns, &facet_path(FacetKind::Program, BINDINGS_DIR), true)?;
    loom.write_file_reserved(
        ns,
        &binding_path(binding.id),
        &trigger_binding_to_cbor(binding)?,
        0o100644,
    )
}

fn binding_path(id: TriggerId) -> String {
    facet_path(FacetKind::Program, &format!("{BINDINGS_DIR}/{id}.cbor"))
}

fn fire_log_dir(id: TriggerId) -> String {
    facet_path(FacetKind::Program, &format!("{FIRE_LOG_DIR}/{id}"))
}

fn fire_record_path(id: TriggerId, seq: u64) -> String {
    facet_path(
        FacetKind::Program,
        &format!("{FIRE_LOG_DIR}/{id}/{seq:020}.cbor"),
    )
}

fn change_trigger_cursor<S: ObjectStore>(
    loom: &Loom<S>,
    binding: &TriggerBinding,
    history: &[FireRecord],
) -> Result<WatchCursor> {
    let TriggerKind::Change { watch } = &binding.kind else {
        return Err(LoomError::new(
            Code::Unsupported,
            "time trigger scheduling is not part of the change trigger keeper path",
        ));
    };
    for record in history.iter().rev() {
        if let TriggerStimulus::Change { source_cursor, .. } = &record.stimulus {
            return WatchCursor::decode(source_cursor);
        }
    }
    if binding.options.catch_up {
        Ok(WatchCursor::from_selector(watch, None))
    } else {
        loom.watch_subscribe(watch, None)
    }
}

fn latest_time_stimulus(history: &[FireRecord]) -> Option<&TriggerStimulus> {
    history.iter().rev().find_map(|record| {
        matches!(record.stimulus, TriggerStimulus::Time { .. }).then_some(&record.stimulus)
    })
}

fn min_wakeup(left: Option<u64>, right: Option<u64>) -> Option<u64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acl::{AclRight, AclSubject};
    use crate::identity::{IdentityStore, PrincipalKind, ROLE_SERVICE_ID};
    use crate::provider::memory::MemoryStore;
    use crate::vcs::Loom;
    use loom_triggers::{
        FireOutcome, FireRecord, TriggerExecMode, TriggerKind, TriggerOptions, TriggerStimulus,
        stimulus_digest,
    };

    fn id(seed: u8) -> WorkspaceId {
        WorkspaceId::from_bytes([seed; 16])
    }

    fn binding(seed: u8, enabled: bool) -> TriggerBinding {
        TriggerBinding {
            id: id(seed),
            kind: TriggerKind::Time {
                cron: "0 0 * * * *".to_string(),
                timezone: "UTC".to_string(),
            },
            program: crate::Digest::blake3(format!("program-{seed}").as_bytes()),
            target_workspace: id(100),
            branch: "main".to_string(),
            budget: 10_000,
            mode: TriggerExecMode::Gated,
            options: TriggerOptions::default(),
            run_as: Some(id(9)),
            enabled,
        }
    }

    fn program_ns(loom: &mut Loom<MemoryStore>) -> WorkspaceId {
        loom.registry_mut()
            .create(FacetKind::Program, Some("program"), id(7))
            .unwrap()
    }

    fn files_ns(loom: &mut Loom<MemoryStore>) -> (WorkspaceId, crate::Digest, crate::Digest) {
        let ns = loom
            .registry_mut()
            .create(FacetKind::Files, Some("files"), id(8))
            .unwrap();
        loom.write_file(ns, "a.txt", b"one", 0o100644).unwrap();
        let c0 = loom.commit(ns, "test", "first", 1).unwrap();
        loom.write_file(ns, "a.txt", b"two", 0o100644).unwrap();
        let c1 = loom.commit(ns, "test", "second", 2).unwrap();
        (ns, c0, c1)
    }

    #[test]
    fn trigger_bindings_store_list_enable_remove_and_version() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = program_ns(&mut loom);
        trigger_put(&mut loom, ns, &binding(2, true)).unwrap();
        trigger_put(&mut loom, ns, &binding(1, false)).unwrap();

        assert_eq!(
            trigger_list(&loom, ns)
                .unwrap()
                .into_iter()
                .map(|binding| binding.id)
                .collect::<Vec<_>>(),
            vec![id(1), id(2)]
        );
        assert!(!trigger_get(&loom, ns, id(1)).unwrap().enabled);
        assert!(trigger_enable(&mut loom, ns, id(1), true).unwrap().enabled);

        let c1 = loom.commit(ns, "trigger", "bindings", 1).unwrap();
        assert!(trigger_remove(&mut loom, ns, id(2)).unwrap());
        assert_eq!(trigger_list(&loom, ns).unwrap().len(), 1);
        loom.checkout_commit(ns, c1).unwrap();
        assert_eq!(trigger_list(&loom, ns).unwrap().len(), 2);
    }

    #[test]
    fn absent_trigger_history_is_empty() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = program_ns(&mut loom);

        assert!(trigger_history(&loom, ns, id(1), 0, 10).unwrap().is_empty());
        assert!(trigger_history(&loom, ns, id(1), 0, 0).unwrap().is_empty());
    }

    #[test]
    fn fire_records_append_and_history_reads_from_sequence() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = program_ns(&mut loom);
        let binding_id = id(1);
        for seq in 0..3 {
            let stimulus = TriggerStimulus::Time {
                fired_at_ms: 1_000 + seq,
            };
            let digest = stimulus_digest(crate::Algo::Blake3, &stimulus).unwrap();
            trigger_append_fire_record(
                &mut loom,
                ns,
                &FireRecord {
                    binding: binding_id,
                    stimulus,
                    stimulus_digest: digest,
                    proposed: None,
                    outcome: FireOutcome::Applied,
                    cost: seq,
                    fired_at_seq: seq,
                },
            )
            .unwrap();
        }

        let history = trigger_history(&loom, ns, binding_id, 1, 10).unwrap();

        assert_eq!(
            history
                .iter()
                .map(|record| record.fired_at_seq)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(
            trigger_append_fire_record(
                &mut loom,
                ns,
                &FireRecord {
                    binding: binding_id,
                    stimulus: history[0].stimulus.clone(),
                    stimulus_digest: history[0].stimulus_digest,
                    proposed: None,
                    outcome: FireOutcome::Applied,
                    cost: 0,
                    fired_at_seq: 1,
                },
            )
            .unwrap_err()
            .code,
            Code::AlreadyExists
        );
    }

    #[test]
    fn keeper_due_emits_change_candidates_from_watch_history() {
        let mut loom = Loom::new(MemoryStore::new());
        let program_ns = program_ns(&mut loom);
        let (files_ns, c0, c1) = files_ns(&mut loom);
        let mut binding = binding(1, true);
        binding.kind = TriggerKind::Change {
            watch: crate::WatchSelector::new(files_ns, "main")
                .unwrap()
                .with_facet(FacetKind::Files),
        };
        binding.options.catch_up = true;
        trigger_put(&mut loom, program_ns, &binding).unwrap();

        let plan = trigger_keeper_due(&loom, program_ns, 0, 10).unwrap();
        let candidates = plan.fires;

        assert_eq!(candidates.len(), 2);
        assert_eq!(plan.next_wakeup_ms, None);
        assert_eq!(candidates[0].binding.id, binding.id);
        assert_eq!(candidates[0].fired_at_seq, 0);
        assert_eq!(candidates[1].fired_at_seq, 1);
        assert_eq!(
            candidates
                .iter()
                .map(|candidate| match candidate.stimulus {
                    TriggerStimulus::Change { commit, .. } => commit,
                    TriggerStimulus::Time { .. } => panic!("expected change stimulus"),
                })
                .collect::<Vec<_>>(),
            vec![c0, c1]
        );
        assert_eq!(
            candidates[0].stimulus_digest,
            stimulus_digest(crate::Algo::Blake3, &candidates[0].stimulus).unwrap()
        );
    }

    #[test]
    fn keeper_due_resumes_after_recorded_change_cursor() {
        let mut loom = Loom::new(MemoryStore::new());
        let program_ns = program_ns(&mut loom);
        let (files_ns, _, c1) = files_ns(&mut loom);
        let mut binding = binding(1, true);
        binding.kind = TriggerKind::Change {
            watch: crate::WatchSelector::new(files_ns, "main")
                .unwrap()
                .with_facet(FacetKind::Files),
        };
        binding.options.catch_up = true;
        trigger_put(&mut loom, program_ns, &binding).unwrap();
        let first = trigger_keeper_due(&loom, program_ns, 0, 10).unwrap().fires;
        trigger_append_fire_record(
            &mut loom,
            program_ns,
            &FireRecord {
                binding: binding.id,
                stimulus: first[0].stimulus.clone(),
                stimulus_digest: first[0].stimulus_digest,
                proposed: None,
                outcome: FireOutcome::Applied,
                cost: 0,
                fired_at_seq: first[0].fired_at_seq,
            },
        )
        .unwrap();

        let candidates = trigger_keeper_due(&loom, program_ns, 0, 10).unwrap().fires;

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].fired_at_seq, 1);
        assert_eq!(
            match candidates[0].stimulus {
                TriggerStimulus::Change { commit, .. } => commit,
                TriggerStimulus::Time { .. } => panic!("expected change stimulus"),
            },
            c1
        );
    }

    #[test]
    fn keeper_due_emits_time_candidates_and_next_wakeup() {
        let mut loom = Loom::new(MemoryStore::new());
        let program_ns = program_ns(&mut loom);
        let mut binding = binding(1, true);
        binding.kind = TriggerKind::Time {
            cron: "0 * * * * *".to_string(),
            timezone: "UTC".to_string(),
        };
        binding.options.catch_up = true;
        binding.options.missed = loom_triggers::MissedFirePolicy::Backfill;
        trigger_put(&mut loom, program_ns, &binding).unwrap();
        let first_stimulus = TriggerStimulus::Time { fired_at_ms: 0 };
        trigger_append_fire_record(
            &mut loom,
            program_ns,
            &FireRecord {
                binding: binding.id,
                stimulus: first_stimulus.clone(),
                stimulus_digest: stimulus_digest(crate::Algo::Blake3, &first_stimulus).unwrap(),
                proposed: None,
                outcome: FireOutcome::Applied,
                cost: 0,
                fired_at_seq: 0,
            },
        )
        .unwrap();

        let plan = trigger_keeper_due(&loom, program_ns, 180_000, 10).unwrap();

        assert_eq!(plan.next_wakeup_ms, Some(240_000));
        assert_eq!(
            plan.fires
                .iter()
                .map(|candidate| candidate.fired_at_seq)
                .collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        assert_eq!(
            plan.fires
                .iter()
                .map(|candidate| match candidate.stimulus {
                    TriggerStimulus::Time { fired_at_ms } => fired_at_ms,
                    TriggerStimulus::Change { .. } => panic!("expected time stimulus"),
                })
                .collect::<Vec<_>>(),
            vec![60_000, 120_000, 180_000]
        );
    }

    #[test]
    fn authenticated_trigger_binding_operations_are_acl_checked() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = program_ns(&mut loom);
        let root = id(1);
        let service = id(2);
        let mut identity = IdentityStore::new(root);
        identity.set_passphrase(root, "root", b"12345678").unwrap();
        identity
            .add_principal(service, "svc", PrincipalKind::Service)
            .unwrap();
        identity.assign_role(service, ROLE_SERVICE_ID).unwrap();
        let session = identity
            .authenticate_passphrase(root, "root", "session")
            .unwrap();
        loom.set_identity_store(identity);
        loom.set_session(session.id);

        assert_eq!(
            trigger_put(&mut loom, ns, &binding(1, true))
                .unwrap_err()
                .code,
            Code::PermissionDenied
        );
        loom.acl_store_mut()
            .allow(
                AclSubject::Principal(root),
                Some(ns),
                Some(FacetKind::Program),
                [AclRight::Read, AclRight::Write],
            )
            .unwrap();
        trigger_put(&mut loom, ns, &binding(1, true)).unwrap();
        assert!(trigger_get(&loom, ns, id(1)).is_ok());
    }
}
