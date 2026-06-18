//! Locator-aware Loom client APIs.
//!
//! The `LoomApi` trait families contain one trait per IDL interface plus the `LoomClient` supertrait.
//!
//! Licensed under BUSL-1.1.

pub mod generated_api;
pub mod local;
pub mod result_view;
mod service;
pub mod types;

pub use generated_api::LoomClient;
pub use local::LocalLoomClient;
pub use result_view::LocalResultView;
