//! Native HNSW accelerator for the vector facet.
//!
//! **Native-only** workspace member: `hnsw_rs` pulls `mmap-rs`/`cpu-time` (not wasm-clean), so this
//! crate has **no dependents** in the workspace (`loom-core` must stay wasm-clean) and the gate only
//! ever builds it for the host target, never the wasm binding. `loom_vector`'s exact search is the
//! cross-platform **contract**; this is the opt-in native accelerator for above-threshold corpora.
//!
//! **Exact scoring over approximate candidates.** HNSW narrows the candidate set. We then re-score
//! those candidates with the vector facet's own [`Metric::score`] and apply the same metadata
//! [`MetaFilter`], so every returned `Hit` carries an exact score in deterministic order. Candidate
//! recall can differ from exact search. HNSW's own graph is non-deterministic due to random level
//! assignment, which is acceptable because it is a derived, rebuildable accelerator, not the synced
//! source of truth.

use hnsw_rs::prelude::{DistCosine, DistDot, DistL2, Hnsw};
use loom_types::Value;
use loom_vector::{Hit, MetaFilter, Metric, VectorAccelerator, VectorSet};
use std::collections::BTreeMap;

const M: usize = 16; // neighbours per layer
const EF_CONSTRUCTION: usize = 200;
const MAX_LAYER: usize = 16;

// One graph per metric (the navigation distance matches the facet's metric).
enum Graph {
    Cosine(Hnsw<'static, f32, DistCosine>),
    L2(Hnsw<'static, f32, DistL2>),
    Dot(Hnsw<'static, f32, DistDot>),
}

/// A derived HNSW index over a [`VectorSet`]: the graph plus copies of the vectors/metadata so the
/// candidate set can be re-scored exactly and filtered. Rebuildable; never stored or synced.
pub struct HnswIndex {
    ids: Vec<String>,
    vectors: Vec<Vec<f32>>,
    metas: Vec<BTreeMap<String, Value>>,
    metric: Metric,
    graph: Graph,
}

impl HnswIndex {
    /// Build the index from a vector set (copies the source-of-truth vectors + metadata).
    pub fn build(set: &VectorSet) -> Self {
        let mut ids = Vec::new();
        let mut vectors = Vec::new();
        let mut metas = Vec::new();
        for (id, v, m) in set.entries() {
            ids.push(id.to_string());
            vectors.push(v.to_vec());
            metas.push(m.clone());
        }
        let n = vectors.len().max(1);
        let graph = match set.metric() {
            Metric::Cosine => {
                let g =
                    Hnsw::<f32, DistCosine>::new(M, n, MAX_LAYER, EF_CONSTRUCTION, DistCosine {});
                for (i, v) in vectors.iter().enumerate() {
                    g.insert((v.as_slice(), i));
                }
                Graph::Cosine(g)
            }
            Metric::L2 => {
                let g = Hnsw::<f32, DistL2>::new(M, n, MAX_LAYER, EF_CONSTRUCTION, DistL2 {});
                for (i, v) in vectors.iter().enumerate() {
                    g.insert((v.as_slice(), i));
                }
                Graph::L2(g)
            }
            Metric::Dot => {
                let g = Hnsw::<f32, DistDot>::new(M, n, MAX_LAYER, EF_CONSTRUCTION, DistDot {});
                for (i, v) in vectors.iter().enumerate() {
                    g.insert((v.as_slice(), i));
                }
                Graph::Dot(g)
            }
        };
        Self {
            ids,
            vectors,
            metas,
            metric: set.metric(),
            graph,
        }
    }

    /// Number of indexed vectors.
    pub fn len(&self) -> usize {
        self.ids.len()
    }
    /// Whether the index is empty.
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    /// Approximate top-`k` nearest neighbours of `query` among vectors passing `filter`. HNSW finds
    /// candidates, which are then re-scored exactly and returned in deterministic order. `ef` is the
    /// search beam.
    pub fn search(&self, query: &[f32], k: usize, filter: &MetaFilter, ef: usize) -> Vec<Hit> {
        if self.ids.is_empty() || k == 0 {
            return Vec::new();
        }
        // Over-fetch so metadata filtering still leaves ~k, and widen the beam accordingly.
        let want = k.saturating_mul(4).clamp(k, self.ids.len());
        let beam = ef.max(want);
        let neighbours = match &self.graph {
            Graph::Cosine(g) => g.search(query, want, beam),
            Graph::L2(g) => g.search(query, want, beam),
            Graph::Dot(g) => g.search(query, want, beam),
        };
        let mut hits: Vec<Hit> = neighbours
            .iter()
            .map(|nb| nb.d_id)
            .filter(|&i| filter.eval(&self.metas[i]))
            .map(|i| Hit {
                id: self.ids[i].clone(),
                score: self.metric.score(query, &self.vectors[i]),
            })
            .collect();
        hits.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.id.cmp(&b.id)));
        hits.truncate(k);
        hits
    }
}

/// The native HNSW index as loom-core's [`VectorAccelerator`]: it plugs into
/// `loom_vector::search_auto` so a corpus above the threshold uses HNSW while smaller ones and the
/// browser stay on exact search.
impl VectorAccelerator for HnswIndex {
    fn search(&self, query: &[f32], k: usize, filter: &MetaFilter, ef: usize) -> Vec<Hit> {
        HnswIndex::search(self, query, k, filter, ef)
    }
    fn len(&self) -> usize {
        HnswIndex::len(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn circle_set(n: usize) -> VectorSet {
        let mut set = VectorSet::new(2, Metric::Cosine);
        for i in 0..n {
            let a = (i as f32) * 0.11;
            set.upsert(format!("v{i:03}"), vec![a.cos(), a.sin()], BTreeMap::new())
                .unwrap();
        }
        set
    }

    #[test]
    fn hnsw_matches_exact_top1_and_returns_k() {
        let set = circle_set(200);
        let q = vec![1.0_f32, 0.0]; // closest to v000 (angle 0)
        let exact = set.search(&q, 5, &MetaFilter::All).unwrap();
        let idx = HnswIndex::build(&set);
        let ann = idx.search(&q, 5, &MetaFilter::All, 64);
        assert_eq!(ann.len(), 5);
        // The clear nearest is found, and returned scores are the exact scores in exact order.
        assert_eq!(ann[0].id, exact[0].id);
        for w in ann.windows(2) {
            assert!(w[0].score >= w[1].score, "results must be score-descending");
        }
    }

    #[test]
    fn metadata_pre_filter_applies_to_ann() {
        let mut set = VectorSet::new(2, Metric::Cosine);
        for i in 0..50 {
            let a = (i as f32) * 0.12;
            let mut m = BTreeMap::new();
            m.insert(
                "lang".to_string(),
                Value::Text(if i % 2 == 0 { "en" } else { "fr" }.into()),
            );
            set.upsert(format!("v{i:03}"), vec![a.cos(), a.sin()], m)
                .unwrap();
        }
        let idx = HnswIndex::build(&set);
        let en = MetaFilter::Eq("lang".into(), Value::Text("en".into()));
        let hits = idx.search(&[1.0, 0.0], 5, &en, 64);
        // Every returned hit is an even (English) vector.
        for h in &hits {
            let n: usize = h.id.trim_start_matches('v').parse().unwrap();
            assert_eq!(n % 2, 0, "filter must exclude French vectors");
        }
    }

    #[test]
    fn search_auto_uses_hnsw_above_threshold() {
        use loom_vector::search_auto;
        let set = circle_set(200);
        let q = vec![1.0_f32, 0.0];
        let idx = HnswIndex::build(&set);
        // threshold 0 -> the accelerator path is taken; it must reconcile to exact's top hit.
        let exact = set.search(&q, 5, &MetaFilter::All).unwrap();
        let auto = search_auto(&set, &q, 5, &MetaFilter::All, Some(&idx), 0, 64).unwrap();
        assert_eq!(auto.len(), 5);
        assert_eq!(auto[0].id, exact[0].id);
        // threshold above the corpus size -> exact path, identical to a direct exact search.
        let auto_exact =
            search_auto(&set, &q, 5, &MetaFilter::All, Some(&idx), 10_000, 64).unwrap();
        assert_eq!(auto_exact, exact);
    }
}
