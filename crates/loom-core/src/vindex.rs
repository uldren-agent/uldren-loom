//! Derived vector-index accelerators behind the exact vector contract.

pub use loom_vector::{
    AcceleratorPolicy, Csr, DEFAULT_EXACT_THRESHOLD, PqIndex, VectorAccelerator, prune_csr,
    search_auto, search_with_policy,
};
