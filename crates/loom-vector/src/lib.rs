//! The vector facet substrate: a pure-Rust, `wasm32`-clean, deterministic **exact** vector
//! store: a fixed dimension + metric per set, id-keyed vectors with metadata, an exact top-k search
//! with a metadata **pre-filter**, and a canonical encoding so a set versions/syncs like any other
//! Loom state.
//!
//! Exact search is the cross-platform **contract**: identical results on native and `wasm32`, recall
//! 1.0, no index to build. Accelerators sit behind separate Rust helper APIs and re-score returned
//! candidates exactly, but approximate candidate recall is not part of this facade. Derived ANN indexes
//! are never stored here, so nothing in this module needs a non-portable dependency.

use loom_types::error::{LoomError, Result};
use loom_types::tabular::Value;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

/// The distance/similarity metric of a vector set, fixed at creation. Scores are
/// normalized so that **higher is more similar** for every metric, giving one sort order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Metric {
    /// Cosine similarity in [-1, 1] (dot of L2-normalized vectors).
    Cosine,
    /// Negative squared Euclidean distance (higher = closer).
    L2,
    /// Dot product (maximum inner product).
    Dot,
}

impl Metric {
    pub fn tag(self) -> u8 {
        match self {
            Metric::Cosine => 1,
            Metric::L2 => 2,
            Metric::Dot => 3,
        }
    }
    pub fn from_tag(b: u8) -> Result<Self> {
        Ok(match b {
            1 => Metric::Cosine,
            2 => Metric::L2,
            3 => Metric::Dot,
            other => return Err(LoomError::corrupt(format!("unknown metric tag {other:#x}"))),
        })
    }
    /// Similarity of `a` to `b` under this metric; higher = more similar. Public so a derived
    /// accelerator (e.g. an HNSW index) can re-score candidates against the exact contract.
    pub fn score(self, a: &[f32], b: &[f32]) -> f32 {
        let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
        match self {
            Metric::Dot => dot,
            Metric::Cosine => {
                let na = a.iter().map(|x| x * x).sum::<f32>().sqrt();
                let nb = b.iter().map(|x| x * x).sum::<f32>().sqrt();
                if na == 0.0 || nb == 0.0 {
                    0.0
                } else {
                    dot / (na * nb)
                }
            }
            Metric::L2 => -a.iter().zip(b).map(|(x, y)| (x - y) * (x - y)).sum::<f32>(),
        }
    }
}

/// A metadata pre-filter: equality predicates ANDed, evaluated *before* scoring so a
/// filtered search never returns fewer than `k` for the wrong reason.
#[derive(Debug, Clone)]
pub enum MetaFilter {
    /// Matches every vector.
    All,
    /// `metadata[key] == value`.
    Eq(String, Value),
    /// `metadata[key] != value`, including missing keys.
    Ne(String, Value),
    /// `metadata[key] < value`.
    Lt(String, Value),
    /// `metadata[key] <= value`.
    Le(String, Value),
    /// `metadata[key] > value`.
    Gt(String, Value),
    /// `metadata[key] >= value`.
    Ge(String, Value),
    /// `metadata[key]` is one of the supplied values.
    In(String, Vec<Value>),
    /// `metadata[key]` exists.
    Exists(String),
    /// Both must hold.
    And(Box<MetaFilter>, Box<MetaFilter>),
    /// At least one must hold.
    Or(Box<MetaFilter>, Box<MetaFilter>),
    /// The inner predicate must not hold.
    Not(Box<MetaFilter>),
}

impl MetaFilter {
    /// Whether `meta` satisfies this filter. Public so a derived accelerator can apply the same
    /// pre-filter as exact search.
    pub fn eval(&self, meta: &BTreeMap<String, Value>) -> bool {
        match self {
            MetaFilter::All => true,
            MetaFilter::And(a, b) => a.eval(meta) && b.eval(meta),
            MetaFilter::Or(a, b) => a.eval(meta) || b.eval(meta),
            MetaFilter::Not(inner) => !inner.eval(meta),
            MetaFilter::Eq(k, v) => meta.get(k).map(|m| m == v).unwrap_or(false),
            MetaFilter::Ne(k, v) => meta.get(k).map(|m| m != v).unwrap_or(true),
            MetaFilter::Lt(k, v) => meta_value_cmp(meta, k, v)
                .map(|ord| ord == Ordering::Less)
                .unwrap_or(false),
            MetaFilter::Le(k, v) => meta_value_cmp(meta, k, v)
                .map(|ord| ord != Ordering::Greater)
                .unwrap_or(false),
            MetaFilter::Gt(k, v) => meta_value_cmp(meta, k, v)
                .map(|ord| ord == Ordering::Greater)
                .unwrap_or(false),
            MetaFilter::Ge(k, v) => meta_value_cmp(meta, k, v)
                .map(|ord| ord != Ordering::Less)
                .unwrap_or(false),
            MetaFilter::In(k, values) => meta
                .get(k)
                .map(|m| values.iter().any(|value| m == value))
                .unwrap_or(false),
            MetaFilter::Exists(k) => meta.contains_key(k),
        }
    }
}

fn meta_value_cmp(meta: &BTreeMap<String, Value>, key: &str, value: &Value) -> Option<Ordering> {
    let actual = meta.get(key)?;
    if std::mem::discriminant(actual) == std::mem::discriminant(value) {
        Some(actual.cmp(value))
    } else {
        None
    }
}

/// One search result, highest score first.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    /// The matched vector's id.
    pub id: String,
    /// Similarity score (metric-dependent; higher = more similar).
    pub score: f32,
}

/// A vector's stored payload: its `f32` components and its metadata.
pub type VectorEntry = (Vec<f32>, BTreeMap<String, Value>);

/// A versioned vector set: id-keyed vectors + metadata, with a fixed dimension and metric.
#[derive(Debug, Clone)]
pub struct VectorSet {
    dim: usize,
    metric: Metric,
    entries: BTreeMap<String, VectorEntry>,
    metadata_indexes: BTreeSet<String>,
}

impl VectorSet {
    /// An empty set with a fixed, immutable `dim` and `metric`.
    pub fn new(dim: usize, metric: Metric) -> Self {
        Self {
            dim,
            metric,
            entries: BTreeMap::new(),
            metadata_indexes: BTreeSet::new(),
        }
    }

    /// The fixed dimension.
    pub fn dim(&self) -> usize {
        self.dim
    }
    /// The fixed metric.
    pub fn metric(&self) -> Metric {
        self.metric
    }
    /// Number of vectors.
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    /// Whether the set is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Insert or replace the vector + metadata at `id`. Rejects a wrong-dimension vector with
    /// `DIMENSION_MISMATCH`.
    pub fn upsert(
        &mut self,
        id: impl Into<String>,
        vector: Vec<f32>,
        metadata: BTreeMap<String, Value>,
    ) -> Result<()> {
        if vector.len() != self.dim {
            return Err(LoomError::dimension_mismatch(format!(
                "vector has dimension {}, set is {}",
                vector.len(),
                self.dim
            )));
        }
        self.entries.insert(id.into(), (vector, metadata));
        Ok(())
    }

    /// The vector + metadata at `id`.
    pub fn get(&self, id: &str) -> Option<&VectorEntry> {
        self.entries.get(id)
    }

    /// Iterate `(id, vector, metadata)` in id order. For building a derived index (e.g. HNSW) over
    /// the source-of-truth vectors.
    pub fn entries(&self) -> impl Iterator<Item = (&str, &[f32], &BTreeMap<String, Value>)> {
        self.entries
            .iter()
            .map(|(id, (v, m))| (id.as_str(), v.as_slice(), m))
    }

    /// Vector ids in ascending order.
    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }

    /// Metadata keys with maintained equality indexes.
    pub fn metadata_indexes(&self) -> impl Iterator<Item = &str> {
        self.metadata_indexes.iter().map(String::as_str)
    }

    /// Declare a maintained equality index for one metadata key.
    pub fn add_metadata_index(&mut self, key: impl Into<String>) -> bool {
        self.metadata_indexes.insert(key.into())
    }

    /// Drop a maintained equality index for one metadata key.
    pub fn remove_metadata_index(&mut self, key: &str) -> bool {
        self.metadata_indexes.remove(key)
    }

    /// Remove `id`; returns whether it was present.
    pub fn remove(&mut self, id: &str) -> bool {
        self.entries.remove(id).is_some()
    }

    /// Exact top-`k` nearest neighbours of `query` among vectors passing `filter`. Deterministic
    /// order: score descending, ties broken by ascending id. `DIMENSION_MISMATCH` if the query width
    /// is wrong.
    pub fn search(&self, query: &[f32], k: usize, filter: &MetaFilter) -> Result<Vec<Hit>> {
        if query.len() != self.dim {
            return Err(LoomError::dimension_mismatch(format!(
                "query has dimension {}, set is {}",
                query.len(),
                self.dim
            )));
        }
        let hits: Vec<Hit> = self
            .entries
            .iter()
            .filter(|(_, (_, meta))| filter.eval(meta))
            .map(|(id, (v, _))| Hit {
                id: id.clone(),
                score: self.metric.score(query, v),
            })
            .collect();
        Ok(sort_hits(hits, k))
    }
}

pub fn sort_hits(mut hits: Vec<Hit>, k: usize) -> Vec<Hit> {
    hits.sort_by(|a, b| match b.score.total_cmp(&a.score) {
        Ordering::Equal => a.id.cmp(&b.id),
        other => other,
    });
    hits.truncate(k);
    hits
}

mod vindex;
pub use vindex::{
    AcceleratorPolicy, Csr, DEFAULT_EXACT_THRESHOLD, PqIndex, VectorAccelerator, prune_csr,
    search_auto, search_with_policy,
};

#[cfg(test)]
mod tests {
    use super::*;
    use loom_types::Code;

    fn set() -> VectorSet {
        let mut set = VectorSet::new(2, Metric::Cosine);
        let mut en = BTreeMap::new();
        en.insert("lang".to_string(), Value::Text("en".into()));
        set.upsert("a", vec![1.0, 0.0], en.clone()).unwrap();
        set.upsert("b", vec![0.0, 1.0], en).unwrap();
        let mut fr = BTreeMap::new();
        fr.insert("lang".to_string(), Value::Text("fr".into()));
        set.upsert("c", vec![0.9, 0.1], fr).unwrap();
        set
    }

    #[test]
    fn dimension_mismatch_rejected() {
        let mut set = VectorSet::new(3, Metric::L2);
        assert_eq!(
            set.upsert("x", vec![1.0, 2.0], BTreeMap::new())
                .unwrap_err()
                .code,
            Code::DimensionMismatch
        );
        assert_eq!(
            set.search(&[1.0, 2.0], 1, &MetaFilter::All)
                .unwrap_err()
                .code,
            Code::DimensionMismatch
        );
    }

    #[test]
    fn exact_topk_is_ordered_and_deterministic() {
        let hits = set().search(&[1.0, 0.0], 3, &MetaFilter::All).unwrap();
        assert_eq!(
            hits.iter().map(|hit| hit.id.as_str()).collect::<Vec<_>>(),
            ["a", "c", "b"]
        );
        assert!(hits[0].score > hits[1].score && hits[1].score > hits[2].score);
    }

    #[test]
    fn metadata_predicates_cover_compatibility_filters() {
        let mut set = VectorSet::new(2, Metric::Cosine);
        let mut alpha = BTreeMap::new();
        alpha.insert("lang".to_string(), Value::Text("en".into()));
        alpha.insert("score".to_string(), Value::Int(10));
        alpha.insert("tenant".to_string(), Value::Text("a".into()));
        set.upsert("alpha", vec![1.0, 0.0], alpha).unwrap();

        let mut beta = BTreeMap::new();
        beta.insert("lang".to_string(), Value::Text("fr".into()));
        beta.insert("score".to_string(), Value::Int(3));
        beta.insert("tenant".to_string(), Value::Text("b".into()));
        set.upsert("beta", vec![0.9, 0.1], beta).unwrap();

        let mut gamma = BTreeMap::new();
        gamma.insert("score".to_string(), Value::Int(8));
        set.upsert("gamma", vec![0.0, 1.0], gamma).unwrap();

        let filter = MetaFilter::And(
            Box::new(MetaFilter::Or(
                Box::new(MetaFilter::In(
                    "tenant".into(),
                    vec![Value::Text("a".into()), Value::Text("c".into())],
                )),
                Box::new(MetaFilter::Not(Box::new(MetaFilter::Exists("lang".into())))),
            )),
            Box::new(MetaFilter::Ge("score".into(), Value::Int(8))),
        );
        let hits = set.search(&[1.0, 0.0], 10, &filter).unwrap();
        assert_eq!(
            hits.iter().map(|hit| hit.id.as_str()).collect::<Vec<_>>(),
            ["alpha", "gamma"]
        );

        let filter = MetaFilter::And(
            Box::new(MetaFilter::Ne("lang".into(), Value::Text("fr".into()))),
            Box::new(MetaFilter::Lt("score".into(), Value::Int(9))),
        );
        let hits = set.search(&[1.0, 0.0], 10, &filter).unwrap();
        assert_eq!(
            hits.iter().map(|hit| hit.id.as_str()).collect::<Vec<_>>(),
            ["gamma"]
        );
    }
}
