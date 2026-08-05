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
    let set = loom_core::capability::registry()
        .with_state_overlay(
            loom_store::provided_capabilities(),
            loom_core::CapabilityOperationalState::Supported,
        )
        .with_state_overlay(
            loom_sql::provided_capabilities(),
            loom_core::CapabilityOperationalState::Supported,
        );
    #[cfg(target_arch = "wasm32")]
    let set = set.with_state_detail(
        "certificate-generate-self-signed",
        loom_core::CapabilityOperationalState::Unsupported,
        Some("profile_unsupported"),
        Some(loom_core::Code::Unsupported),
    );
    set.to_cbor()
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

#[cfg(test)]
mod wasm_manifest_tests {
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::Path;

    #[derive(Debug)]
    struct ManifestRow {
        method: String,
        supported: bool,
        gated: bool,
    }

    fn idl_has_method(idl: &str, interface: &str, method: &str) -> bool {
        let Some(interface_start) = idl.find(&format!("interface {interface} {{")) else {
            return false;
        };
        let interface_tail = &idl[interface_start..];
        let Some(interface_end) = interface_tail.find("\n}") else {
            return false;
        };
        interface_tail[..interface_end].contains(&format!(" {method}("))
    }

    fn contract_rows() -> Vec<ManifestRow> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");
        let contract = fs::read_to_string(root.join("idl/binding-targets.json"))
            .expect("binding target contract");
        let idl = fs::read_to_string(root.join("idl/loom.idl")).expect("IDL");
        let contract: serde_json::Value =
            serde_json::from_str(&contract).expect("binding target contract JSON");
        assert_eq!(contract["schema_version"], 1);
        assert_eq!(
            contract["wasm_capability_gated"]["reason"],
            "profile_unsupported"
        );
        assert_eq!(contract["wasm_capability_gated"]["code"], "UNSUPPORTED");
        assert_eq!(
            contract["native_targets"],
            serde_json::json!([
                "c_abi",
                "cpp",
                "jvm",
                "android",
                "ios",
                "react_native",
                "nodejs",
                "python"
            ])
        );
        let rows = contract["methods"]
            .as_array()
            .expect("contract methods")
            .iter()
            .map(|entry| {
                let name = entry["name"].as_str().expect("contract method name");
                let (interface, method) = name.split_once('.').expect("qualified method");
                assert!(
                    idl_has_method(&idl, interface, method),
                    "{name} must exist in idl/loom.idl"
                );
                let wasm = entry["wasm"].as_str().expect("WASM disposition");
                let supported = wasm == "supported";
                let gated = wasm == "capability_gated";
                assert!(
                    supported ^ gated,
                    "{name} must have exactly one WASM disposition"
                );
                Some(ManifestRow {
                    method: method.to_string(),
                    supported,
                    gated,
                })
                .expect("contract row")
            })
            .collect::<Vec<_>>();
        assert_eq!(rows.len(), 84);
        assert_eq!(
            rows.iter()
                .map(|row| &row.method)
                .collect::<BTreeSet<_>>()
                .len(),
            84
        );
        rows
    }

    fn source_text(path: &Path) -> String {
        let mut out = String::new();
        for entry in fs::read_dir(path).expect("source dir") {
            let entry = entry.expect("source entry");
            let path = entry.path();
            if path.is_dir() {
                out.push_str(&source_text(&path));
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push_str(&fs::read_to_string(path).expect("source file"));
                out.push('\n');
            }
        }
        out
    }

    fn exported_methods() -> BTreeSet<String> {
        let source = source_text(Path::new(env!("CARGO_MANIFEST_DIR")).join("src").as_path());
        source
            .match_indices("pub fn ")
            .filter_map(|(index, _)| {
                let rest = &source[index + "pub fn ".len()..];
                let name = rest.split_once('(')?.0.trim();
                name.chars()
                    .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
                    .then(|| name.to_string())
            })
            .collect()
    }

    #[test]
    fn wasm_manifest_disposition_matches_supported_exports_and_gated_absence() {
        let rows = contract_rows();
        let supported = rows
            .iter()
            .filter(|row| row.supported)
            .map(|row| row.method.clone())
            .collect::<BTreeSet<_>>();
        let gated = rows
            .iter()
            .filter(|row| row.gated)
            .map(|row| row.method.clone())
            .collect::<BTreeSet<_>>();
        let exports = exported_methods();
        let promoted_exports = rows
            .iter()
            .filter(|row| exports.contains(&row.method))
            .map(|row| row.method.clone())
            .collect::<BTreeSet<_>>();

        assert_eq!(supported.len(), 44);
        assert_eq!(gated.len(), 40);
        assert_eq!(promoted_exports, supported);
        assert!(gated.is_disjoint(&exports));
        for method in [
            "audit_compact",
            "store_maintenance_status",
            "store_maintenance_policy_set",
            "store_maintenance_run",
        ] {
            assert!(gated.contains(method), "{method} must remain WASM gated");
            assert!(!exports.contains(method), "{method} must not be exported");
        }
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
