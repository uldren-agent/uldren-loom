//! `vindex` - derived vector-index accelerators behind the exact contract.
//!
//! Pure Rust, **no external dependency -> `wasm32`-clean** (the native HNSW graph lives in `loom-hnsw`
//! and plugs in through the [`VectorAccelerator`] trait defined here). Every accelerator narrows
//! candidates, then the returned `Hit`s carry the exact [`Metric::score`] in deterministic order
//! (score desc, id asc), applying the same [`MetaFilter`]. Candidate recall can differ from exact
//! search, so public callers need an explicit accelerator policy before this is exposed as a
//! result-changing boundary. Three pieces:
//!
//! 1. **Threshold auto-switch.** [`search_auto`] runs loom-core's exact search below
//!    [`DEFAULT_EXACT_THRESHOLD`] and an injected [`VectorAccelerator`] above it, so a browser with no
//!    accelerator simply always runs exact, while a native build switches to HNSW for large corpora.
//! 2. **Two-level / PQ.** [`PqIndex`] is a product-quantization "PQ table" coarse ranker;
//!    it ranks candidates by the cheap PQ approximation, then **re-scores only the top fraction
//!    exactly**, over our own store and independent of any HNSW library
//!    (so it is itself `wasm32`-clean and usable as the browser's above-threshold accelerator).
//! 3. **High-degree-preserving pruning.** [`prune_csr`] keeps the top-`beta%` highest-degree
//!    hub nodes at full degree and caps the rest: a deterministic CSR pass over our own adjacency.

use crate::{Hit, MetaFilter, Metric, VectorSet};
use loom_types::error::{LoomError, Result};
use loom_types::tabular::Value;
use std::cmp::Ordering;
use std::collections::BTreeMap;

/// Corpus size below which exact search is used and above which an accelerator is preferred.
pub const DEFAULT_EXACT_THRESHOLD: usize = 4096;

/// Explicit policy for using a derived vector accelerator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcceleratorPolicy {
    /// Always run the exact portable search path.
    ExactAlways,
    /// Use the accelerator only when the source set is larger than `threshold`.
    ApproximateAbove { threshold: usize },
}

impl Default for AcceleratorPolicy {
    fn default() -> Self {
        Self::ApproximateAbove {
            threshold: DEFAULT_EXACT_THRESHOLD,
        }
    }
}

/// A derived, above-threshold index (the native HNSW graph in `loom-hnsw`, or [`PqIndex`] here). Every
/// returned `Hit` carries the exact [`Metric::score`] in deterministic order, applying the same
/// [`MetaFilter`], but approximate candidate recall can differ from exact search. Defined in
/// loom-core so the engine stays wasm-clean; the heavy native impl lives in `loom-hnsw` and is
/// injected at the call site.
pub trait VectorAccelerator {
    /// Reconciled top-`k` nearest neighbours; `ef` is the candidate/beam budget (larger = better
    /// recall, slower).
    fn search(&self, query: &[f32], k: usize, filter: &MetaFilter, ef: usize) -> Vec<Hit>;
    /// Number of indexed vectors.
    fn len(&self) -> usize;
    /// Whether the index is empty.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Search with an explicit accelerator policy. The exact path is the portable contract. The
/// accelerator path returns exact-scored hits in deterministic order for the candidates it finds, but
/// candidate recall can differ.
pub fn search_with_policy(
    set: &VectorSet,
    query: &[f32],
    k: usize,
    filter: &MetaFilter,
    accel: Option<&dyn VectorAccelerator>,
    policy: AcceleratorPolicy,
    ef: usize,
) -> Result<Vec<Hit>> {
    if query.len() != set.dim() {
        return Err(LoomError::dimension_mismatch(format!(
            "query has dimension {}, set is {}",
            query.len(),
            set.dim()
        )));
    }
    match (policy, accel) {
        (AcceleratorPolicy::ApproximateAbove { threshold }, Some(a)) if set.len() > threshold => {
            Ok(a.search(query, k, filter, ef))
        }
        _ => set.search(query, k, filter),
    }
}

/// Threshold auto-switch using [`AcceleratorPolicy::ApproximateAbove`]. Kept as the compact internal
/// helper for existing callers; public surfaces should expose policy rather than making the threshold
/// implicit.
pub fn search_auto(
    set: &VectorSet,
    query: &[f32],
    k: usize,
    filter: &MetaFilter,
    accel: Option<&dyn VectorAccelerator>,
    threshold: usize,
    ef: usize,
) -> Result<Vec<Hit>> {
    search_with_policy(
        set,
        query,
        k,
        filter,
        accel,
        AcceleratorPolicy::ApproximateAbove { threshold },
        ef,
    )
}

// ---- product quantization + two-level search ----------------------------------------------------

/// A product-quantization codebook: `m` subspaces, `k` centroids each (`k <= 256`, so a code is one
/// byte per subspace). Deterministic fixed-seed k-means with lowest-index tie-breaks, so the codebook
/// is reproducible across platforms.
struct Pq {
    m: usize,
    k: usize,
    sub: usize,
    centroids: Vec<f32>, // m*k centroids laid out [subspace][centroid][dim]; len = m*k*sub
    normalize: bool,
}

impl Pq {
    fn centroid(&self, s: usize, c: usize) -> &[f32] {
        let base = (s * self.k + c) * self.sub;
        &self.centroids[base..base + self.sub]
    }

    fn train(
        data: &[Vec<f32>],
        dim: usize,
        m: usize,
        k: usize,
        iters: usize,
        normalize: bool,
    ) -> Self {
        let sub = dim / m;
        let k = k.clamp(1, 256);
        let mut centroids = vec![0f32; m * k * sub];
        let n = data.len();
        for s in 0..m {
            let off = s * sub;
            // Deterministic init: evenly-spaced training subvectors (or zeros when there is no data).
            for c in 0..k {
                let dst = (s * k + c) * sub;
                if n > 0 {
                    let src = (c * n / k) % n;
                    centroids[dst..dst + sub].copy_from_slice(&data[src][off..off + sub]);
                }
            }
            // Lloyd iterations.
            for _ in 0..iters {
                let mut sums = vec![0f32; k * sub];
                let mut counts = vec![0usize; k];
                for row in data {
                    let sv = &row[off..off + sub];
                    let c = nearest(
                        sv,
                        |c| {
                            let b = (s * k + c) * sub;
                            &centroids[b..b + sub]
                        },
                        k,
                    );
                    for (acc, &x) in sums[c * sub..c * sub + sub].iter_mut().zip(sv) {
                        *acc += x;
                    }
                    counts[c] += 1;
                }
                for c in 0..k {
                    if counts[c] > 0 {
                        let dst = (s * k + c) * sub;
                        for d in 0..sub {
                            centroids[dst + d] = sums[c * sub + d] / counts[c] as f32;
                        }
                    }
                    // Empty cluster keeps its previous centroid (deterministic, no re-seed).
                }
            }
        }
        Self {
            m,
            k,
            sub,
            centroids,
            normalize,
        }
    }

    fn encode(&self, v: &[f32]) -> Vec<u8> {
        let v = self.prep(v);
        let mut code = vec![0u8; self.m];
        for s in 0..self.m {
            let sv = &v[s * self.sub..s * self.sub + self.sub];
            code[s] = nearest(sv, |c| self.centroid(s, c), self.k) as u8;
        }
        code
    }

    /// Lookup table of squared distances from the query's subvectors to every centroid (m*k).
    fn lut(&self, query: &[f32]) -> Vec<f32> {
        let q = self.prep(query);
        let mut lut = vec![0f32; self.m * self.k];
        for s in 0..self.m {
            let qs = &q[s * self.sub..s * self.sub + self.sub];
            for c in 0..self.k {
                lut[s * self.k + c] = sqdist(qs, self.centroid(s, c));
            }
        }
        lut
    }

    fn approx(&self, code: &[u8], lut: &[f32]) -> f32 {
        (0..self.m)
            .map(|s| lut[s * self.k + code[s] as usize])
            .sum()
    }

    fn prep(&self, v: &[f32]) -> Vec<f32> {
        let mut v = v.to_vec();
        if self.normalize {
            l2_normalize(&mut v);
        }
        v
    }
}

/// A two-level PQ index over a [`VectorSet`]: the PQ codebook + per-vector codes, plus
/// copies of the vectors/metadata so the shortlist can be re-scored exactly and filtered. Derived and
/// rebuildable, never stored or synced. Pure Rust, so it is the `wasm32`-clean accelerator option.
pub struct PqIndex {
    pq: Pq,
    ids: Vec<String>,
    codes: Vec<Vec<u8>>,
    vectors: Vec<Vec<f32>>,
    metas: Vec<BTreeMap<String, Value>>,
    metric: Metric,
}

impl PqIndex {
    /// Build a PQ index over `set` with `m` subspaces (must divide the set's dimension), `k` centroids
    /// (clamped to 256), and `iters` k-means iterations. `DIMENSION_MISMATCH` if `m` does not divide the
    /// dimension or `m == 0`.
    pub fn build(set: &VectorSet, m: usize, k: usize, iters: usize) -> Result<Self> {
        let dim = set.dim();
        if m == 0 || dim == 0 || !dim.is_multiple_of(m) {
            return Err(LoomError::dimension_mismatch(format!(
                "PQ subspaces m={m} must divide dimension {dim} (and be nonzero)"
            )));
        }
        let normalize = set.metric() == Metric::Cosine;
        let mut ids = Vec::new();
        let mut vectors = Vec::new();
        let mut metas = Vec::new();
        for (id, v, meta) in set.entries() {
            ids.push(id.to_string());
            vectors.push(v.to_vec());
            metas.push(meta.clone());
        }
        let prepped: Vec<Vec<f32>> = vectors
            .iter()
            .map(|v| {
                let mut v = v.clone();
                if normalize {
                    l2_normalize(&mut v);
                }
                v
            })
            .collect();
        let pq = Pq::train(&prepped, dim, m, k, iters, normalize);
        let codes = vectors.iter().map(|v| pq.encode(v)).collect();
        Ok(Self {
            pq,
            ids,
            codes,
            vectors,
            metas,
            metric: set.metric(),
        })
    }

    /// Number of indexed vectors.
    pub fn len(&self) -> usize {
        self.ids.len()
    }
    /// Whether the index is empty.
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    /// Two-level search: rank all filtered candidates by the cheap PQ approximation, take the top
    /// `shortlist` (clamped to `[k, n]`), then **re-score those exactly** and return the top-`k` in exact
    /// order (score desc, id asc). For a separable query this returns the same top-k as exact; under PQ
    /// approximation only candidate recall can differ, never a returned hit's score.
    pub fn search(
        &self,
        query: &[f32],
        k: usize,
        filter: &MetaFilter,
        shortlist: usize,
    ) -> Vec<Hit> {
        if self.ids.is_empty() || k == 0 {
            return Vec::new();
        }
        let lut = self.pq.lut(query);
        // Coarse rank: (approx distance asc, id asc) over filtered candidates.
        let mut cand: Vec<(f32, usize)> = (0..self.ids.len())
            .filter(|&i| filter.eval(&self.metas[i]))
            .map(|i| (self.pq.approx(&self.codes[i], &lut), i))
            .collect();
        cand.sort_by(|a, b| match a.0.total_cmp(&b.0) {
            Ordering::Equal => self.ids[a.1].cmp(&self.ids[b.1]),
            other => other,
        });
        let take = shortlist.max(k).min(cand.len());
        // Exact re-score of the shortlist (the "recompute only the top fraction" step).
        let mut hits: Vec<Hit> = cand[..take]
            .iter()
            .map(|&(_, i)| Hit {
                id: self.ids[i].clone(),
                score: self.metric.score(query, &self.vectors[i]),
            })
            .collect();
        hits.sort_by(|a, b| match b.score.total_cmp(&a.score) {
            Ordering::Equal => a.id.cmp(&b.id),
            other => other,
        });
        hits.truncate(k);
        hits
    }
}

impl VectorAccelerator for PqIndex {
    fn search(&self, query: &[f32], k: usize, filter: &MetaFilter, ef: usize) -> Vec<Hit> {
        // `ef` is the shortlist size; over-fetch a few k by default if the caller passes something tiny.
        PqIndex::search(self, query, k, filter, ef.max(k.saturating_mul(4)))
    }
    fn len(&self) -> usize {
        PqIndex::len(self)
    }
}

// ---- high-degree-preserving pruning -------------------------------------------------------------

/// A compressed-sparse-row adjacency: `offsets` (len `nodes + 1`) into `neighbors`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Csr {
    /// `offsets[i]..offsets[i+1]` is node `i`'s slice of `neighbors`.
    pub offsets: Vec<usize>,
    /// Flattened neighbour ids.
    pub neighbors: Vec<u32>,
}

impl Csr {
    /// Number of nodes.
    pub fn nodes(&self) -> usize {
        self.offsets.len().saturating_sub(1)
    }
    /// Node `i`'s neighbours.
    pub fn neighbors(&self, i: usize) -> &[u32] {
        &self.neighbors[self.offsets[i]..self.offsets[i + 1]]
    }
    /// Node `i`'s out-degree after pruning.
    pub fn degree(&self, i: usize) -> usize {
        self.offsets[i + 1] - self.offsets[i]
    }
}

/// High-degree-preserving pruning: keep the top `beta_percent` highest-degree **hub** nodes
/// at up to `m_full` neighbours and cap every other node at `m_cap`, keeping hubs dense while pruning
/// the long tail to shrink graph metadata. Deterministic: hubs are chosen by degree desc then id asc;
/// the kept neighbours of each node are its lowest ids (dedup'd, sorted). Pure CSR pass, no external
/// dependency, identical on native and `wasm32`.
pub fn prune_csr(adjacency: &[Vec<u32>], beta_percent: u32, m_full: usize, m_cap: usize) -> Csr {
    let n = adjacency.len();
    let mut by_degree: Vec<usize> = (0..n).collect();
    by_degree.sort_by(|&a, &b| adjacency[b].len().cmp(&adjacency[a].len()).then(a.cmp(&b)));
    let hub_count = ((n as u64 * beta_percent.min(100) as u64) / 100) as usize;
    let mut is_hub = vec![false; n];
    for &node in by_degree.iter().take(hub_count) {
        is_hub[node] = true;
    }
    let mut offsets = Vec::with_capacity(n + 1);
    let mut neighbors = Vec::new();
    offsets.push(0);
    for (node, nbrs) in adjacency.iter().enumerate() {
        let cap = if is_hub[node] { m_full } else { m_cap };
        let mut nb = nbrs.clone();
        nb.sort_unstable();
        nb.dedup();
        nb.truncate(cap);
        neighbors.extend_from_slice(&nb);
        offsets.push(neighbors.len());
    }
    Csr { offsets, neighbors }
}

// ---- shared helpers -----------------------------------------------------------------------------

fn sqdist(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| (x - y) * (x - y)).sum()
}

/// Index of the nearest centroid to `sv` (min squared distance; lowest index on ties).
fn nearest<'a>(sv: &[f32], centroid: impl Fn(usize) -> &'a [f32], k: usize) -> usize {
    let mut best = 0usize;
    let mut best_d = f32::INFINITY;
    for c in 0..k {
        let d = sqdist(sv, centroid(c));
        if d < best_d {
            best_d = d;
            best = c;
        }
    }
    best
}

fn l2_normalize(v: &mut [f32]) {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn circle(n: usize, metric: Metric) -> VectorSet {
        let mut s = VectorSet::new(4, metric);
        for i in 0..n {
            let a = (i as f32) * 0.013;
            // A 4-d vector on a smooth curve so PQ subspaces (2x2) have structure to quantize.
            s.upsert(
                format!("v{i:05}"),
                vec![a.cos(), a.sin(), (2.0 * a).cos(), (2.0 * a).sin()],
                BTreeMap::new(),
            )
            .unwrap();
        }
        s
    }

    #[test]
    fn threshold_auto_switch_picks_exact_then_accelerator() {
        let set = circle(50, Metric::Cosine);
        let q = vec![1.0, 0.0, 1.0, 0.0];
        // Below threshold (or no accel): exact.
        let exact = search_auto(
            &set,
            &q,
            5,
            &MetaFilter::All,
            None,
            DEFAULT_EXACT_THRESHOLD,
            64,
        )
        .unwrap();
        assert_eq!(exact, set.search(&q, 5, &MetaFilter::All).unwrap());
        // With an accelerator and threshold 0, the accelerator path is taken; returned hits still carry
        // exact scores in descending order.
        let idx = PqIndex::build(&set, 2, 16, 8).unwrap();
        let via_accel = search_auto(&set, &q, 5, &MetaFilter::All, Some(&idx), 0, 64).unwrap();
        assert_eq!(via_accel.len(), 5);
        for w in via_accel.windows(2) {
            assert!(
                w[0].score >= w[1].score,
                "accelerator must return descending exact scores"
            );
        }
    }

    #[test]
    fn explicit_policy_can_force_exact_or_allow_acceleration() {
        let set = circle(50, Metric::Cosine);
        let q = vec![1.0, 0.0, 1.0, 0.0];
        let exact = set.search(&q, 5, &MetaFilter::All).unwrap();
        let idx = PqIndex::build(&set, 2, 16, 8).unwrap();

        let forced_exact = search_with_policy(
            &set,
            &q,
            5,
            &MetaFilter::All,
            Some(&idx),
            AcceleratorPolicy::ExactAlways,
            64,
        )
        .unwrap();
        assert_eq!(forced_exact, exact);

        let via_policy = search_with_policy(
            &set,
            &q,
            5,
            &MetaFilter::All,
            Some(&idx),
            AcceleratorPolicy::ApproximateAbove { threshold: 0 },
            64,
        )
        .unwrap();
        assert_eq!(via_policy.len(), 5);
        for w in via_policy.windows(2) {
            assert!(w[0].score >= w[1].score);
        }
    }

    #[test]
    fn explicit_policy_rejects_wrong_width_before_acceleration() {
        let set = circle(50, Metric::Cosine);
        let idx = PqIndex::build(&set, 2, 16, 8).unwrap();
        let err = search_with_policy(
            &set,
            &[1.0, 0.0],
            5,
            &MetaFilter::All,
            Some(&idx),
            AcceleratorPolicy::ApproximateAbove { threshold: 0 },
            64,
        )
        .unwrap_err();
        assert_eq!(err.code, loom_types::Code::DimensionMismatch);
    }

    #[test]
    fn two_level_pq_matches_exact_top1_for_separable_query() {
        let set = circle(200, Metric::Cosine);
        let q = vec![1.0, 0.0, 1.0, 0.0]; // closest to v00000 (angle 0)
        let exact = set.search(&q, 5, &MetaFilter::All).unwrap();
        let idx = PqIndex::build(&set, 2, 32, 10).unwrap();
        let approx = idx.search(&q, 5, &MetaFilter::All, 40);
        assert_eq!(approx.len(), 5);
        assert_eq!(
            approx[0].id, exact[0].id,
            "two-level must find the clear nearest"
        );
        // Returned scores are exact (re-scored) and descending.
        for w in approx.windows(2) {
            assert!(w[0].score >= w[1].score);
        }
        // The exact score of the top hit equals the set's exact score for that id.
        let v = set.get(&approx[0].id).unwrap().0.clone();
        assert!((approx[0].score - Metric::Cosine.score(&q, &v)).abs() < 1e-6);
    }

    #[test]
    fn two_level_pq_applies_metadata_filter() {
        let mut set = VectorSet::new(4, Metric::Cosine);
        for i in 0..60 {
            let a = (i as f32) * 0.05;
            let mut m = BTreeMap::new();
            m.insert(
                "lang".into(),
                Value::Text(if i % 2 == 0 { "en" } else { "fr" }.into()),
            );
            set.upsert(format!("v{i:03}"), vec![a.cos(), a.sin(), 0.0, 0.0], m)
                .unwrap();
        }
        let idx = PqIndex::build(&set, 2, 16, 8).unwrap();
        let en = MetaFilter::Eq("lang".into(), Value::Text("en".into()));
        let hits = idx.search(&[1.0, 0.0, 0.0, 0.0], 5, &en, 40);
        for h in &hits {
            let n: usize = h.id.trim_start_matches('v').parse().unwrap();
            assert_eq!(n % 2, 0, "filter must exclude French vectors");
        }
    }

    #[test]
    fn pq_build_rejects_bad_subspace_count() {
        let set = circle(4, Metric::L2);
        assert!(PqIndex::build(&set, 3, 16, 4).is_err()); // 3 does not divide 4
        assert!(PqIndex::build(&set, 0, 16, 4).is_err());
    }

    #[test]
    fn prune_keeps_hubs_dense_and_caps_the_tail() {
        // node 0 is a hub (degree 8); nodes 1..=8 have degree 1 each.
        let mut adj: Vec<Vec<u32>> = vec![vec![1, 2, 3, 4, 5, 6, 7, 8]];
        for _ in 1..=8 {
            adj.push(vec![0]);
        }
        // top 20% of 9 nodes = 1 hub (node 0). hub keeps up to 4; others capped at 1.
        let csr = prune_csr(&adj, 20, 4, 1);
        assert_eq!(csr.nodes(), 9);
        assert_eq!(csr.degree(0), 4, "hub kept at m_full");
        assert_eq!(
            csr.neighbors(0),
            &[1, 2, 3, 4],
            "kept lowest ids deterministically"
        );
        for i in 1..=8 {
            assert!(csr.degree(i) <= 1, "tail capped at m_cap");
        }
        // Deterministic: same input -> same output.
        assert_eq!(prune_csr(&adj, 20, 4, 1), csr);
    }
}
