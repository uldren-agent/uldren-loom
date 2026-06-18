//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

//! The 0015 statechart layer: a small, deterministic finite-state-machine interpreter. Behind the
//! `statecharts` feature, which enables `guards` because transitions may carry a CEL guard.
//!
//! A machine's current state is persisted under a KV key, so a lifecycle is content-addressed like any
//! other state and travels with branch/merge. Firing an event selects the first transition (in declared
//! order) whose `from` state and `event` match and whose optional guard holds; the guard is the L1
//! read-only CEL predicate from [`crate::guard`], evaluated against the capability-scoped view. Taking a
//! transition persists the new state and runs an optional action. Selection is deterministic (declared
//! order, no clock or randomness), so a lifecycle replays identically - the determinism audit the
//! promotion requires: nothing here reads ambient state.
//!
//! The interpreter operates on an explicit [`StateView`] (the machine's KV state); binding that to the
//! live branch through `StateAccess` is the facade's job, kept separate so this layer stays pure and
//! deterministically testable.

use crate::capability::GrantSet;
use crate::guard::{self, StateView};

/// One transition: from `from`, on `event`, if the optional CEL `guard` holds, move to `to` and
/// optionally write `action` (a KV key/value) as a side effect.
#[derive(Clone, Debug)]
pub struct Transition {
    pub from: String,
    pub event: String,
    pub guard: Option<String>,
    pub to: String,
    pub action: Option<(String, Vec<u8>)>,
}

/// A statechart: where the current state is persisted (`state_key` in KV), the initial state, and the
/// ordered transitions.
#[derive(Clone, Debug)]
pub struct Machine {
    pub state_key: String,
    pub initial: String,
    pub transitions: Vec<Transition>,
}

/// The transition that fired.
#[derive(Clone, Debug)]
pub struct Step {
    pub from: String,
    pub to: String,
    pub event: String,
}

/// Why firing an event did not produce a transition.
#[derive(Clone, Debug)]
pub enum StepError {
    /// No transition matched the (current state, event) once guards were applied.
    NoTransition { state: String, event: String },
    /// A transition's CEL guard failed to compile or evaluate.
    Guard(guard::GuardError),
}

impl Machine {
    /// The current state read from the view, or the initial state if it is unset.
    pub fn current(&self, kv: &StateView) -> String {
        kv.get(&self.state_key)
            .map(|v| String::from_utf8_lossy(v).into_owned())
            .unwrap_or_else(|| self.initial.clone())
    }

    /// Fire `event`: take the first transition from the current state whose event matches and whose
    /// guard holds, persisting the new state and running its action. A guard that evaluates to false is
    /// skipped (the next matching transition is tried), so the same event can branch on a guard.
    pub fn fire(
        &self,
        kv: &mut StateView,
        event: &str,
        inputs: &StateView,
        grants: &GrantSet,
        ledger_verified: bool,
    ) -> Result<Step, StepError> {
        let current = self.current(kv);
        for t in &self.transitions {
            if t.from != current || t.event != event {
                continue;
            }
            if let Some(expr) = &t.guard {
                match guard::evaluate(expr, kv, inputs, grants, ledger_verified) {
                    Ok(true) => {}
                    Ok(false) => continue, // guard false: try the next matching transition
                    Err(e) => return Err(StepError::Guard(e)),
                }
            }
            kv.insert(self.state_key.clone(), t.to.clone().into_bytes());
            if let Some((key, value)) = &t.action {
                kv.insert(key.clone(), value.clone());
            }
            return Ok(Step {
                from: current,
                to: t.to.clone(),
                event: event.to_string(),
            });
        }
        Err(StepError::NoTransition {
            state: current,
            event: event.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::{Capability, Grant, Mode, Scope};

    fn doc_machine() -> Machine {
        Machine {
            state_key: "doc:state".into(),
            initial: "draft".into(),
            transitions: vec![
                Transition {
                    from: "draft".into(),
                    event: "submit".into(),
                    guard: None,
                    to: "review".into(),
                    action: None,
                },
                Transition {
                    from: "review".into(),
                    event: "approve".into(),
                    guard: Some(r#"kv.reviewer == "alice""#.into()),
                    to: "published".into(),
                    action: Some(("published_by".into(), b"alice".to_vec())),
                },
            ],
        }
    }

    fn read_grants() -> GrantSet {
        GrantSet::new(vec![Grant {
            facet: Capability::Kv,
            mode: Mode::Read,
            scopes: vec![Scope::All],
        }])
    }

    #[test]
    fn lifecycle_transitions_and_persists() {
        let m = doc_machine();
        let mut kv = StateView::new();
        kv.insert("reviewer".into(), b"alice".to_vec());
        assert_eq!(m.current(&kv), "draft");

        let s1 = m
            .fire(&mut kv, "submit", &StateView::new(), &read_grants(), false)
            .unwrap();
        assert_eq!((s1.from.as_str(), s1.to.as_str()), ("draft", "review"));
        assert_eq!(m.current(&kv), "review");

        let s2 = m
            .fire(&mut kv, "approve", &StateView::new(), &read_grants(), false)
            .unwrap();
        assert_eq!(s2.to, "published");
        assert_eq!(m.current(&kv), "published");
        assert_eq!(
            kv.get("published_by").map(|v| v.as_slice()),
            Some(b"alice".as_slice())
        );
    }

    #[test]
    fn guard_gates_the_transition() {
        let m = doc_machine();
        let mut kv = StateView::new();
        kv.insert("reviewer".into(), b"bob".to_vec()); // the approve guard expects alice
        m.fire(&mut kv, "submit", &StateView::new(), &read_grants(), false)
            .unwrap(); // draft -> review
        // approve's guard (kv.reviewer == "alice") is false and nothing else matches -> no transition.
        let err = m
            .fire(&mut kv, "approve", &StateView::new(), &read_grants(), false)
            .unwrap_err();
        assert!(matches!(err, StepError::NoTransition { .. }));
        assert_eq!(m.current(&kv), "review"); // state unchanged
    }

    #[test]
    fn unknown_event_is_no_transition() {
        let m = doc_machine();
        let mut kv = StateView::new();
        let err = m
            .fire(&mut kv, "delete", &StateView::new(), &read_grants(), false)
            .unwrap_err();
        assert!(matches!(err, StepError::NoTransition { .. }));
    }
}
