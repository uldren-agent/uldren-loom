//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

//! The 0015 workflow layer: deterministic derivation orchestration, graduated from
//! `prototypes/loom-compute/src/workflow.rs`. Behind the `workflows` feature (which enables
//! `derivations`).
//!
//! A [`Trigger`] binds a watched KV prefix to a [`Derivation`]. After a transition, the
//! [`TriggerEngine`] recomputes the derived views whose watched scope changed, reusing a cached result
//! when the input digest is unchanged, and cascades to a fixpoint so a derived view that feeds another
//! trigger's scope settles. Everything here is deterministic: triggers fire in registration order, the
//! cache is keyed by a content address over the canonical input bytes, and recomputation is the pure
//! [`Derivation::recompute`]. The determinism audit the promotion requires holds - no clock, randomness,
//! or ambient state.
//!
//! Only the deterministic recompute-and-cascade is graduated here. *Reactive* firing - deciding when a
//! transition happened and scheduling the pass - is the change-feed/trigger substrate owned by spec
//! `0029`, not this queue, so it is deliberately left out.

use std::collections::BTreeMap;

use loom_core::content_address;

use crate::capability::Capability;
use crate::derivation::{Derivation, KvView};

/// A trigger: a watched facet plus a derivation whose `source_prefix` scopes the watch.
#[derive(Clone, Debug)]
pub struct Trigger {
    pub watch: Capability,
    pub derivation: Derivation,
}

impl Trigger {
    /// A KV-watching trigger (the facet the graduated derivations read).
    pub fn kv(derivation: Derivation) -> Self {
        Self {
            watch: Capability::Kv,
            derivation,
        }
    }
}

/// What one trigger did during an `on_change` pass.
#[derive(Clone, Debug)]
pub struct FireReport {
    pub derivation_id: String,
    /// Whether the watched scope changed (and so the trigger ran).
    pub fired: bool,
    /// Whether the derived result was served from cache rather than recomputed.
    pub cache_hit: bool,
    pub derived_key: String,
    pub derived_value: Vec<u8>,
}

/// Why a cascade could not be completed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkflowError {
    /// The cascade did not converge within the firing budget (a likely cycle).
    Budget { budget: u64 },
}

/// The trigger registry and derivation cache. After a transition, [`TriggerEngine::on_change`]
/// recomputes the derived views whose inputs changed, reusing cached results when the input digest is
/// unchanged.
#[derive(Default)]
pub struct TriggerEngine {
    triggers: Vec<Trigger>,
    cache: BTreeMap<(String, String), (String, Vec<u8>)>,
    /// Observability: how often a derivation actually recomputed vs hit cache.
    pub recompute_count: u64,
    pub cache_hits: u64,
}

impl TriggerEngine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, trigger: Trigger) {
        self.triggers.push(trigger);
    }

    /// Apply every trigger after a transition `before -> after`. For each trigger whose watched KV
    /// scope changed, recompute (or reuse a cached) derived entry and write it into `after`. Returns one
    /// report per trigger, in registration order.
    pub fn on_change(&mut self, before: &KvView, after: &mut KvView) -> Vec<FireReport> {
        let triggers = self.triggers.clone();
        let mut reports = Vec::new();
        for trigger in &triggers {
            let prefix = trigger.derivation.source_prefix();
            if !kv_scope_changed(before, after, prefix) {
                reports.push(FireReport {
                    derivation_id: trigger.derivation.id(),
                    fired: false,
                    cache_hit: false,
                    derived_key: trigger.derivation.into_key().to_string(),
                    derived_value: Vec::new(),
                });
                continue;
            }

            let key = (
                trigger.derivation.id(),
                content_address(&trigger.derivation.input_bytes(after)).to_hex(),
            );
            let (derived_key, derived_value, cache_hit) = match self.cache.get(&key) {
                Some((dk, dv)) => {
                    self.cache_hits += 1;
                    (dk.clone(), dv.clone(), true)
                }
                None => {
                    let (dk, dv) = trigger.derivation.recompute(after);
                    self.recompute_count += 1;
                    self.cache.insert(key, (dk.clone(), dv.clone()));
                    (dk, dv, false)
                }
            };

            after.insert(derived_key.clone(), derived_value.clone());
            reports.push(FireReport {
                derivation_id: trigger.derivation.id(),
                fired: true,
                cache_hit,
                derived_key,
                derived_value,
            });
        }
        reports
    }

    /// Run triggers repeatedly until a full pass changes nothing (a fixpoint), so a derived view that
    /// feeds another trigger's watched scope cascades. Bounded by `budget` (total trigger firings): a
    /// cascade that does not converge aborts with [`WorkflowError::Budget`] rather than looping forever.
    /// Re-fires are cheap because unchanged inputs hit the derivation cache.
    pub fn on_change_to_fixpoint(
        &mut self,
        before: &KvView,
        after: &mut KvView,
        budget: u64,
    ) -> Result<Vec<FireReport>, WorkflowError> {
        let mut all = Vec::new();
        let mut spent = 0u64;
        loop {
            let snapshot = after.clone();
            let round = self.on_change(before, after);
            spent += round.iter().filter(|r| r.fired).count() as u64;
            all.extend(round);
            if *after == snapshot {
                // A full pass changed nothing: fixpoint reached.
                return Ok(all);
            }
            if spent > budget {
                return Err(WorkflowError::Budget { budget });
            }
        }
    }
}

/// Whether the KV entries under `prefix` differ between `before` and `after`.
fn kv_scope_changed(before: &KvView, after: &KvView, prefix: &str) -> bool {
    let under = |b: &KvView| -> BTreeMap<String, Vec<u8>> {
        b.iter()
            .filter(|(k, _)| k.starts_with(prefix))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    };
    under(before) != under(after)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kv(pairs: &[(&str, &[u8])]) -> KvView {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), v.to_vec()))
            .collect()
    }

    #[test]
    fn trigger_fires_on_change_and_materializes_view() {
        let mut engine = TriggerEngine::new();
        engine.register(Trigger::kv(Derivation::CountUnderPrefix {
            source_prefix: "item:".into(),
            into_key: "derived:item_count".into(),
        }));

        let before = KvView::new();
        let mut after = kv(&[("item:1", b"a"), ("item:2", b"b")]);
        let reports = engine.on_change(&before, &mut after);

        assert_eq!(reports.len(), 1);
        assert!(reports[0].fired && !reports[0].cache_hit);
        assert_eq!(
            after.get("derived:item_count").map(|v| v.as_slice()),
            Some(b"2".as_slice())
        );
        assert_eq!(engine.recompute_count, 1);
    }

    #[test]
    fn identical_inputs_hit_cache_and_skip_recompute() {
        let mut engine = TriggerEngine::new();
        engine.register(Trigger::kv(Derivation::CountUnderPrefix {
            source_prefix: "item:".into(),
            into_key: "derived:item_count".into(),
        }));
        // First pass recomputes; a second pass over the same inputs (after clearing the derived key so
        // the scope "changes") reuses the cache keyed by the identical input digest.
        let before = KvView::new();
        let mut after = kv(&[("item:1", b"a"), ("item:2", b"b")]);
        engine.on_change(&before, &mut after);
        after.remove("derived:item_count");
        let reports = engine.on_change(&KvView::new(), &mut after);
        assert!(reports[0].fired && reports[0].cache_hit);
        assert_eq!(engine.recompute_count, 1);
        assert_eq!(engine.cache_hits, 1);
    }

    #[test]
    fn cascade_runs_to_fixpoint() {
        // count under item: -> derived:count; a second trigger counts derived: entries -> derived2:n.
        let mut engine = TriggerEngine::new();
        engine.register(Trigger::kv(Derivation::CountUnderPrefix {
            source_prefix: "item:".into(),
            into_key: "derived:count".into(),
        }));
        engine.register(Trigger::kv(Derivation::CountUnderPrefix {
            source_prefix: "derived:".into(),
            into_key: "derived2:n".into(),
        }));
        let before = KvView::new();
        let mut after = kv(&[("item:1", b"a")]);
        let reports = engine
            .on_change_to_fixpoint(&before, &mut after, 16)
            .unwrap();
        assert!(!reports.is_empty());
        assert_eq!(
            after.get("derived:count").map(|v| v.as_slice()),
            Some(b"1".as_slice())
        );
        assert_eq!(
            after.get("derived2:n").map(|v| v.as_slice()),
            Some(b"1".as_slice())
        );
    }
}
