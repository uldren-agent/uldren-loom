//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

//! The 0015 derivation layer: deterministic derived views over a watched KV scope. Behind the
//! `derivations` feature.
//!
//! Two of the three derivations are plain deterministic folds over the granted KV entries under a
//! prefix (count and integer sum) and need no engine. The third, reachable-count, is recursive - a
//! transitive closure over directed edges - which a scalar fold cannot express, so it is computed with
//! Datalog via the `crepe` proc-macro. `crepe` compiles the rules to inline Rust at build time (its
//! `syn`/`quote` build deps never ship to the wasm target), so the derivation layer adds no runtime
//! engine dependency. Determinism is guaranteed by construction: inputs are read from a sorted
//! [`BTreeMap`] view, nodes are interned in sorted order, and the reachable set is collected into a
//! [`BTreeSet`] before counting, so the output never depends on `crepe`'s internal hash order.
//!
//! The caller supplies the KV view. Live facet access and trigger scheduling stay outside this module
//! so recomputation remains pure and deterministically testable.

use std::collections::{BTreeMap, BTreeSet};

/// A capability-scoped, read-only view of KV entries (key -> raw value bytes). A [`BTreeMap`] so
/// iteration is in sorted key order and every derivation is deterministic.
pub type KvView = BTreeMap<String, Vec<u8>>;

/// A deterministic derivation over a watched KV prefix, producing one derived KV entry. [`Derivation::id`]
/// is its stable identity (its "program digest").
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Derivation {
    /// Count the KV keys under `source_prefix`; write the decimal count into `into_key`.
    CountUnderPrefix {
        source_prefix: String,
        into_key: String,
    },
    /// Sum the base-10 integer values of the KV entries under `source_prefix` (non-integers count as 0,
    /// deterministically); write the sum into `into_key`.
    SumUnderPrefix {
        source_prefix: String,
        into_key: String,
    },
    /// Recursive reachability via Datalog (`crepe`): read directed edges from the KV entries under
    /// `edges_prefix` (each value is `"src,dst"`), compute the transitive closure, and write the count
    /// of distinct nodes reachable from `from` into `into_key`.
    ReachableCount {
        edges_prefix: String,
        from: String,
        into_key: String,
    },
}

impl Derivation {
    /// Stable identity, standing for the program's content address.
    pub fn id(&self) -> String {
        match self {
            Derivation::CountUnderPrefix {
                source_prefix,
                into_key,
            } => format!("count-under:{source_prefix}->{into_key}"),
            Derivation::SumUnderPrefix {
                source_prefix,
                into_key,
            } => format!("sum-under:{source_prefix}->{into_key}"),
            Derivation::ReachableCount {
                edges_prefix,
                from,
                into_key,
            } => format!("reach-count:{edges_prefix}:{from}->{into_key}"),
        }
    }

    /// The watched KV prefix this derivation reads.
    pub fn source_prefix(&self) -> &str {
        match self {
            Derivation::CountUnderPrefix { source_prefix, .. }
            | Derivation::SumUnderPrefix { source_prefix, .. } => source_prefix,
            Derivation::ReachableCount { edges_prefix, .. } => edges_prefix,
        }
    }

    /// The derived KV key this derivation writes.
    pub fn into_key(&self) -> &str {
        match self {
            Derivation::CountUnderPrefix { into_key, .. }
            | Derivation::SumUnderPrefix { into_key, .. }
            | Derivation::ReachableCount { into_key, .. } => into_key,
        }
    }

    /// The canonical byte encoding of exactly the inputs this derivation reads: the KV entries under
    /// `source_prefix`, in sorted key order, each key- and value-length-prefixed. Equal inputs yield
    /// equal bytes, so a content address over this is a stable cache key for the derived result.
    pub fn input_bytes(&self, kv: &KvView) -> Vec<u8> {
        let prefix = self.source_prefix();
        let mut bytes = Vec::new();
        for (k, v) in kv.iter().filter(|(k, _)| k.starts_with(prefix)) {
            bytes.extend_from_slice(&(k.len() as u32).to_le_bytes());
            bytes.extend_from_slice(k.as_bytes());
            bytes.extend_from_slice(&(v.len() as u32).to_le_bytes());
            bytes.extend_from_slice(v);
        }
        bytes
    }

    /// Recompute the derived `(key, value)` from the watched scope. Deterministic.
    pub fn recompute(&self, kv: &KvView) -> (String, Vec<u8>) {
        let prefix = self.source_prefix();
        let value = match self {
            Derivation::CountUnderPrefix { .. } => kv
                .keys()
                .filter(|k| k.starts_with(prefix))
                .count()
                .to_string(),
            Derivation::SumUnderPrefix { .. } => kv
                .iter()
                .filter(|(k, _)| k.starts_with(prefix))
                .map(|(_, v)| parse_i64(v))
                // Saturating so a pathological input can never panic on overflow (determinism).
                .fold(0i64, i64::saturating_add)
                .to_string(),
            Derivation::ReachableCount { from, .. } => {
                let edges: Vec<(String, String)> = kv
                    .iter()
                    .filter(|(k, _)| k.starts_with(prefix))
                    .filter_map(|(_, v)| parse_edge(v))
                    .collect();
                reachable_count(&edges, from).to_string()
            }
        };
        (self.into_key().to_string(), value.into_bytes())
    }
}

/// Parse a KV value as a trimmed base-10 integer; anything else contributes 0 (deterministic).
fn parse_i64(v: &[u8]) -> i64 {
    std::str::from_utf8(v)
        .ok()
        .and_then(|s| s.trim().parse::<i64>().ok())
        .unwrap_or(0)
}

/// Parse a KV value `"src,dst"` into a directed edge; malformed values are skipped.
fn parse_edge(v: &[u8]) -> Option<(String, String)> {
    let s = std::str::from_utf8(v).ok()?;
    let (a, b) = s.split_once(',')?;
    Some((a.to_string(), b.to_string()))
}

/// Count the distinct nodes reachable from `from` in the transitive closure of `edges`, via Datalog.
/// Node labels are interned to `u32` in sorted order so the computation is deterministic, and the
/// result is collected into a [`BTreeSet`] so it never depends on the solver's internal hash order.
fn reachable_count(edges: &[(String, String)], from: &str) -> usize {
    let mut nodes: Vec<&str> = edges
        .iter()
        .flat_map(|(a, b)| [a.as_str(), b.as_str()])
        .collect();
    nodes.sort_unstable();
    nodes.dedup();
    let id = |s: &str| nodes.binary_search(&s).ok().map(|i| i as u32);
    let Some(from_id) = id(from) else {
        return 0;
    };

    use crepe::crepe;
    crepe! {
        @input
        struct Edge(u32, u32);
        @output
        struct Reach(u32, u32);
        Reach(x, y) <- Edge(x, y);
        Reach(x, z) <- Edge(x, y), Reach(y, z);
    }

    let mut runtime = Crepe::new();
    runtime.extend(edges.iter().filter_map(|(a, b)| Some(Edge(id(a)?, id(b)?))));
    let (reach,) = runtime.run();
    reach
        .into_iter()
        .filter(|Reach(x, _)| *x == from_id)
        .map(|Reach(_, y)| y)
        .collect::<BTreeSet<u32>>()
        .len()
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
    fn count_under_prefix_counts_only_the_prefix() {
        let m = kv(&[("item:1", b"a"), ("item:2", b"b"), ("other", b"c")]);
        let d = Derivation::CountUnderPrefix {
            source_prefix: "item:".into(),
            into_key: "n".into(),
        };
        assert_eq!(d.recompute(&m), ("n".to_string(), b"2".to_vec()));
    }

    #[test]
    fn sum_under_prefix_treats_non_integers_as_zero() {
        let m = kv(&[("v:1", b"10"), ("v:2", b"5"), ("v:3", b"x")]);
        let d = Derivation::SumUnderPrefix {
            source_prefix: "v:".into(),
            into_key: "s".into(),
        };
        assert_eq!(d.recompute(&m), ("s".to_string(), b"15".to_vec()));
    }

    #[test]
    fn reachable_count_is_the_transitive_closure() {
        // a -> b -> c, plus a disjoint x -> y. Reachable from a is {b, c} = 2.
        let m = kv(&[("e:1", b"a,b"), ("e:2", b"b,c"), ("e:3", b"x,y")]);
        let d = Derivation::ReachableCount {
            edges_prefix: "e:".into(),
            from: "a".into(),
            into_key: "r".into(),
        };
        assert_eq!(d.recompute(&m), ("r".to_string(), b"2".to_vec()));
    }

    #[test]
    fn reachable_count_of_an_absent_node_is_zero() {
        let m = kv(&[("e:1", b"a,b")]);
        let d = Derivation::ReachableCount {
            edges_prefix: "e:".into(),
            from: "z".into(),
            into_key: "r".into(),
        };
        assert_eq!(d.recompute(&m), ("r".to_string(), b"0".to_vec()));
    }
}
