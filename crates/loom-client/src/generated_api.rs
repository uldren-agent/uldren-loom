//! Re-export of the generated `LoomApi` trait families from the engine-free `loom-remote-protocol`.
//! This shim keeps
//! `loom_client::generated_api::*` and `loom_client::LoomClient` stable. Regenerate the source with
//! `cargo run -p uldren-loom-remote-codegen`.
//!
//! Licensed under BUSL-1.1.

pub use loom_remote_protocol::generated_api::*;
