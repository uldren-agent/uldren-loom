//! WASM binding for Uldren Loom via wasm-bindgen. Published as `@uldrenai/loom-wasm`.
//!
//! The browser / JS-runtime path.
//! Licensed under BUSL-1.1 (see the repo `LICENSE`). (c) Uldren Technologies LLC.

use loom_core::Object;
use wasm_bindgen::prelude::*;

/// The library version.
#[wasm_bindgen]
pub fn version() -> String {
    loom_core::VERSION.to_string()
}

/// Compute the Blob content address (`"algo:hex"`) of the given bytes.
#[wasm_bindgen]
pub fn blob_digest(data: &[u8]) -> String {
    Object::Blob(data.to_vec()).digest().to_string()
}

/// The build capability report (0010 section 5) as canonical CBOR: a `CapabilitySet` map with
/// `schema_version` and `records`. Build-aware: this build links loom-store and loom-sql, so their
/// owned capabilities are reported with operational state `supported`. Mirrors the C ABI
/// `loom_capabilities`.
#[wasm_bindgen]
pub fn capabilities() -> Vec<u8> {
    loom_core::capability::registry()
        .with_state_overlay(
            loom_store::provided_capabilities(),
            loom_core::CapabilityOperationalState::Supported,
        )
        .with_state_overlay(
            loom_sql::provided_capabilities(),
            loom_core::CapabilityOperationalState::Supported,
        )
        .to_cbor()
}

/// The linked WASM runtime profile as canonical CBOR.
#[wasm_bindgen]
pub fn runtime_profile() -> Vec<u8> {
    loom_core::runtime_profile().to_cbor()
}

#[wasm_bindgen]
pub fn studio_surface_catalog_json(
    workspace: &str,
    set: Option<String>,
) -> Result<String, JsError> {
    loom_substrate::surfaces::surface_catalog_json(workspace, set.as_deref().unwrap_or("all"))
        .map_err(|e| JsError::new(&e.to_string()))
}

/// The pinned conformance commit address (computed natively, 64-bit; `loom_sql::CONFORMANCE_COMMIT`).
/// The in-browser conformance check recomputes the same vector live and asserts equality - so any
/// 32-bit-wasm vs 64-bit-native canonical-encoding drift shows up as a mismatch.
#[wasm_bindgen]
pub fn conformance_expected() -> String {
    loom_sql::CONFORMANCE_COMMIT.to_string()
}

/// Run the deterministic SQL conformance vector over an in-memory `FileStore` on THIS target and return
/// the resulting commit address. On wasm32 it must equal [`conformance_expected`].
#[wasm_bindgen]
pub fn conformance_digest() -> Result<String, JsError> {
    let store =
        loom_store::FileStore::with_backing(Box::new(loom_store::MemoryBacking::new()), true)
            .map_err(|e| JsError::new(&e.to_string()))?;
    loom_sql::conformance_commit_digest(store).map_err(|e| JsError::new(&e.to_string()))
}

/// Classify an OPFS locator for the browser binding's local-vs-remote split. The browser binding opens
/// plain OPFS names locally and rejects remote locators unless the build enables the remote feature.
/// Alias TOML is not consulted because the browser binding has no filesystem config surface.
#[cfg(any(target_arch = "wasm32", test))]
pub(crate) fn reject_remote_locator(locator: &str) -> Result<(), String> {
    if locator.starts_with("https://") || locator.starts_with("http://") {
        #[cfg(not(feature = "remote"))]
        {
            return Err(
                "remote Loom locators require the remote feature in this binding".to_string(),
            );
        }
        #[cfg(feature = "remote")]
        {
            return Err(
                "remote Loom locators are not yet wired in this binding (constructor surface only)"
                    .to_string(),
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod locator_tests {
    use super::reject_remote_locator;

    #[test]
    fn local_opfs_names_pass_through() {
        assert!(reject_remote_locator("app.loom").is_ok());
        assert!(reject_remote_locator("workspace/app").is_ok());
    }

    #[test]
    fn remote_url_is_rejected_without_remote_feature() {
        let err = reject_remote_locator("https://loom.example.com/prod").unwrap_err();
        assert!(err.contains("remote feature"), "unexpected error: {err}");
        assert!(reject_remote_locator("http://loom.example.com/prod").is_err());
    }
}

// ---------------------------------------------------------------------------------------------------
// OPFS-backed SQL session. wasm32 only - it depends on the browser OPFS sync-access-handle
// API, which exists only inside a Web Worker. This whole module is excluded on native targets, so the
// native `cargo check` of this crate compiles only the helpers above; the code below is verified by
// `wasm-pack build --target web`.
//
// NOTE (not verifiable without a wasm toolchain): the exact `web-sys` setter/method spellings can vary
// by `web-sys` patch version (e.g. `FileSystemGetFileOptions::set_create` vs `.create`), and the
// loom-store/loom-sql/gluesql tree must build for `wasm32-unknown-unknown`.
// ---------------------------------------------------------------------------------------------------
#[cfg(target_arch = "wasm32")]
mod opfs_sql;
