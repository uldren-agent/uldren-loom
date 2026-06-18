//! Re-export of the `LoomApi` support types from the engine-free `loom-remote-protocol`
//! so both `LocalLoomClient` (this crate) and `RemoteLoomClient` implement the same generated traits over
//! the same types. This shim keeps `loom_client::types::*` (and `crate::types::*`) stable.
//!
//! Licensed under BUSL-1.1.

pub use loom_remote_protocol::api_types::*;
