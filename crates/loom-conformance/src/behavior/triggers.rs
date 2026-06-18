//! Behavioral conformance for the `Triggers` family: canonical binding/fire-record CBOR pinned as
//! vectors, plus an engine put/get/list/enable/remove/history round-trip over a [`MemoryStore`].

use loom_core::{
    Algo, Code, FacetKind, FireOutcome, FireRecord, Loom, MemoryStore, MissedFirePolicy,
    OverlapPolicy, Result, TriggerBinding, TriggerExecMode, TriggerKind, TriggerOptions,
    TriggerStimulus, WorkspaceId, content_address, fire_record_to_cbor, stimulus_digest,
    trigger_append_fire_record, trigger_binding_from_cbor, trigger_binding_to_cbor, trigger_enable,
    trigger_get, trigger_history, trigger_list, trigger_put, trigger_remove,
};

pub struct TriggerCanonicalVector {
    pub name: &'static str,
    pub binding: TriggerBinding,
    pub expect_canonical: &'static str,
}

/// The pinned canonical binding fixture: a gated hourly time trigger. Every binding, in every
/// language, must reproduce these exact bytes.
pub fn trigger_canonical_vectors() -> Vec<TriggerCanonicalVector> {
    vec![TriggerCanonicalVector {
        name: "gated-hourly-time-trigger",
        binding: trigger_binding_fixture(),
        expect_canonical: "8b776c6f6f6d2e747269676765722e62696e64696e672e7631782431313131313131312d313131312d313131312d313131312d313131313131313131313131836474696d656930202a202a202a202a635554437847626c616b65333a61303534323064393337343830373332353634316631303730613366343534323865346264336538666233643666653165616430353236326234633063386262782432323232323232322d323232322d323232322d323232322d323232323232323232323232646d61696e1903e86567617465648464736b6970f4006f736b69702d69662d72756e6e696e67f6f5",
    }]
}

fn trigger_binding_fixture() -> TriggerBinding {
    TriggerBinding {
        id: WorkspaceId::from_bytes([0x11; 16]),
        kind: TriggerKind::Time {
            cron: "0 * * * *".into(),
            timezone: "UTC".into(),
        },
        program: content_address(b"trigger-program"),
        target_workspace: WorkspaceId::from_bytes([0x22; 16]),
        branch: "main".into(),
        budget: 1000,
        mode: TriggerExecMode::Gated,
        options: TriggerOptions {
            missed: MissedFirePolicy::Skip,
            catch_up: false,
            jitter_ms: 0,
            overlap: OverlapPolicy::SkipIfRunning,
        },
        run_as: None,
        enabled: true,
    }
}

fn fire_record_fixture(binding: &TriggerBinding) -> Result<FireRecord> {
    let stimulus = TriggerStimulus::Time {
        fired_at_ms: 1_725_000_000_000,
    };
    let digest = stimulus_digest(Algo::Blake3, &stimulus)?;
    Ok(FireRecord {
        binding: binding.id,
        stimulus,
        stimulus_digest: digest,
        proposed: None,
        outcome: FireOutcome::Applied,
        cost: 1,
        fired_at_seq: 1,
    })
}

pub fn run_triggers_behavior() -> Result<()> {
    // Canonical binding bytes are pinned and must round-trip.
    for vector in trigger_canonical_vectors() {
        let encoded = trigger_binding_to_cbor(&vector.binding)?;
        assert_eq!(
            hex::encode(&encoded),
            vector.expect_canonical,
            "trigger binding canonical bytes mismatch for '{}'",
            vector.name
        );
        assert_eq!(
            trigger_binding_from_cbor(&encoded)?,
            vector.binding,
            "trigger binding canonical round-trip mismatch for '{}'",
            vector.name
        );
    }

    // A corrupt schema tag must be rejected.
    assert!(
        trigger_binding_from_cbor(&[0x81, 0x61, 0x78]).is_err(),
        "malformed trigger binding unexpectedly decoded"
    );

    // Engine round-trip: put/get/list/enable/remove and fire-record history.
    let mut loom = Loom::new(MemoryStore::new());
    let ns = loom.registry_mut().create(
        FacetKind::Program,
        None,
        WorkspaceId::from_bytes([0x6d; 16]),
    )?;
    let binding = trigger_binding_fixture();
    trigger_put(&mut loom, ns, &binding)?;
    assert_eq!(trigger_get(&loom, ns, binding.id)?, binding);
    assert_eq!(trigger_list(&loom, ns)?, vec![binding.clone()]);

    let disabled = trigger_enable(&mut loom, ns, binding.id, false)?;
    assert!(!disabled.enabled);
    assert!(!trigger_get(&loom, ns, binding.id)?.enabled);

    let record = fire_record_fixture(&binding)?;
    trigger_append_fire_record(&mut loom, ns, &record)?;
    let history = trigger_history(&loom, ns, binding.id, 0, 16)?;
    assert_eq!(history, vec![record.clone()]);
    // The fire record encodes and round-trips through canonical CBOR.
    assert_eq!(
        loom_core::fire_record_from_cbor(&fire_record_to_cbor(&record)?)?,
        record
    );

    assert!(trigger_remove(&mut loom, ns, binding.id)?);
    assert!(matches!(
        trigger_get(&loom, ns, binding.id),
        Err(err) if err.code == Code::NotFound
    ));
    assert!(trigger_list(&loom, ns)?.is_empty());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triggers_behavior_passes() {
        run_triggers_behavior().expect("triggers behavior must pass");
    }
}
