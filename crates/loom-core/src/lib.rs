//! `loom-core` - the Uldren Loom engine.
//!
//! Implements the content-addressed object model and the low-level provider contract that the
//! command-line tool, the C ABI, and every language binding build upon.
//!
//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). © Uldren Technologies LLC.

pub mod digest;
pub mod error;
pub mod object;
pub mod provider;

pub use digest::{Algo, Digest};
pub use error::{Code, LoomError, Result};
pub use object::{Object, ObjectType};
pub use provider::ObjectStore;
pub use provider::memory::MemoryStore;

/// The crate version (from Cargo).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
