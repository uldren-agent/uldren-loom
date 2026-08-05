//! Locator-aware Loom client APIs.
//!
//! The `LoomApi` trait families contain one trait per IDL interface plus the `LoomClient` supertrait.
//!
//! Licensed under BUSL-1.1.

pub mod generated_api;
pub mod identity_authority_policy;
#[cfg(feature = "local-client")]
pub mod local;
#[cfg(feature = "local-client")]
pub mod locks;
pub mod result_view;
pub mod security_admin;
pub mod serve_config;
#[cfg(feature = "local-client")]
mod service;
pub mod types;

#[cfg(feature = "local-client")]
pub use generated_api::LoomClient;
pub use identity_authority_policy::IdentityAuthorityPolicyService;
#[cfg(feature = "local-client")]
pub use local::LocalLoomClient;
#[cfg(feature = "local-client")]
pub use locks::{InProcessLocksAuthority, LocksAuthority, LocksPersistence};
pub use result_view::LocalResultView;
pub use security_admin::SecurityAdminService;
