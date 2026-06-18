//! Canonical wire codec for the pull-watch selector.
//!
//! A [`WatchSelector`] crosses as the CBOR array `[workspace, branch, facet, path_prefix,
//! change_kinds]`:
//!   - `workspace` is the UUID text. The source `WatchSelector.workspace` is a `WorkspaceId`, not a
//!     name-or-id `WsSelector` (`loom-watch/src/lib.rs`: field `pub workspace: WorkspaceId` and the
//!     `WatchSelector::new(workspace: WorkspaceId, ...)` constructor), so the wire form carries the
//!     stable id as text.
//!   - `branch` is text.
//!   - `facet` is the stable `FacetKind` tag (uint) or null.
//!   - `path_prefix` is text or null.
//!   - `change_kinds` is an array of stable `ChangeKind` tags (uint).
//!
//! Decoding rebuilds the selector through the `WatchSelector::new`/`with_*` constructors so branch
//! validation and change-kind canonicalization stay in `loom_core`. Unknown facet/change tags and any
//! malformed shape are `INVALID_ARGUMENT`.

use loom_codec::{Value as CborValue, decode, encode};
use loom_core::{ChangeKind, FacetKind, WatchSelector, WorkspaceId};
use loom_types::{Code, LoomError};

/// Encode a [`WatchSelector`] as the canonical CBOR array `[workspace, branch, facet, path_prefix,
/// change_kinds]`.
pub fn watch_selector_to_cbor(selector: &WatchSelector) -> Result<Vec<u8>, LoomError> {
    let facet = match selector.facet {
        Some(facet) => CborValue::Uint(u64::from(facet.stable_tag())),
        None => CborValue::Null,
    };
    let path_prefix = match &selector.path_prefix {
        Some(prefix) => CborValue::Text(prefix.clone()),
        None => CborValue::Null,
    };
    let change_kinds = CborValue::Array(
        selector
            .change_kinds
            .iter()
            .map(|kind| CborValue::Uint(u64::from(kind.stable_tag())))
            .collect(),
    );
    encode(&CborValue::Array(vec![
        CborValue::Text(selector.workspace.to_string()),
        CborValue::Text(selector.branch.clone()),
        facet,
        path_prefix,
        change_kinds,
    ]))
    .map_err(|err| LoomError::new(Code::CorruptObject, format!("cbor: {err}")))
}

/// Decode a `WatchSelector` wire blob, rebuilding it through the `loom_core` constructors.
pub fn watch_selector_from_wire(bytes: &[u8]) -> Result<WatchSelector, LoomError> {
    let value =
        decode(bytes).map_err(|err| LoomError::invalid(format!("watch selector: {err}")))?;
    let CborValue::Array(items) = value else {
        return Err(LoomError::invalid("watch selector must be a cbor array"));
    };
    let [workspace, branch, facet, path_prefix, change_kinds] = items.as_slice() else {
        return Err(LoomError::invalid(
            "watch selector must be [workspace, branch, facet, path_prefix, change_kinds]",
        ));
    };
    let CborValue::Text(workspace) = workspace else {
        return Err(LoomError::invalid("watch selector workspace must be text"));
    };
    let CborValue::Text(branch) = branch else {
        return Err(LoomError::invalid("watch selector branch must be text"));
    };
    let mut selector = WatchSelector::new(WorkspaceId::parse(workspace)?, branch.clone())?;
    match facet {
        CborValue::Null => {}
        CborValue::Uint(tag) => {
            let facet = u8::try_from(*tag)
                .ok()
                .and_then(FacetKind::from_stable_tag)
                .ok_or_else(|| LoomError::invalid("unknown watch selector facet tag"))?;
            selector = selector.with_facet(facet);
        }
        _ => {
            return Err(LoomError::invalid(
                "watch selector facet must be a tag uint or null",
            ));
        }
    }
    match path_prefix {
        CborValue::Null => {}
        CborValue::Text(prefix) => selector = selector.with_path_prefix(prefix.clone()),
        _ => {
            return Err(LoomError::invalid(
                "watch selector path_prefix must be text or null",
            ));
        }
    }
    let CborValue::Array(kinds) = change_kinds else {
        return Err(LoomError::invalid(
            "watch selector change_kinds must be an array",
        ));
    };
    for kind in kinds {
        let CborValue::Uint(tag) = kind else {
            return Err(LoomError::invalid(
                "watch selector change kind must be a tag uint",
            ));
        };
        let kind = u8::try_from(*tag)
            .ok()
            .and_then(ChangeKind::from_stable_tag)
            .ok_or_else(|| LoomError::invalid("unknown watch selector change kind tag"))?;
        selector = selector.with_change_kind(kind);
    }
    Ok(selector)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ws() -> WorkspaceId {
        WorkspaceId::v4_from_bytes([9u8; 16])
    }

    #[test]
    fn selector_round_trips_with_all_fields() {
        let selector = WatchSelector::new(ws(), "main")
            .unwrap()
            .with_facet(FacetKind::Files)
            .with_path_prefix("docs/")
            .with_change_kind(ChangeKind::Added)
            .with_change_kind(ChangeKind::Deleted);
        let decoded =
            watch_selector_from_wire(&watch_selector_to_cbor(&selector).unwrap()).unwrap();
        assert_eq!(decoded, selector);
    }

    #[test]
    fn selector_round_trips_with_optional_fields_absent() {
        let selector = WatchSelector::new(ws(), "trunk").unwrap();
        let bytes = watch_selector_to_cbor(&selector).unwrap();
        let CborValue::Array(items) = loom_codec::decode(&bytes).unwrap() else {
            panic!("expected array");
        };
        assert_eq!(items.len(), 5);
        assert_eq!(items[2], CborValue::Null); // facet
        assert_eq!(items[3], CborValue::Null); // path_prefix
        assert_eq!(items[4], CborValue::Array(Vec::new())); // change_kinds
        assert_eq!(watch_selector_from_wire(&bytes).unwrap(), selector);
    }

    #[test]
    fn facet_tag_is_the_stable_facet_tag() {
        let selector = WatchSelector::new(ws(), "main")
            .unwrap()
            .with_facet(FacetKind::Kv);
        let CborValue::Array(items) =
            loom_codec::decode(&watch_selector_to_cbor(&selector).unwrap()).unwrap()
        else {
            panic!("expected array");
        };
        assert_eq!(
            items[2],
            CborValue::Uint(u64::from(FacetKind::Kv.stable_tag()))
        );
    }

    #[test]
    fn unknown_facet_tag_is_invalid_argument() {
        let bytes = encode(&CborValue::Array(vec![
            CborValue::Text(ws().to_string()),
            CborValue::Text("main".to_string()),
            CborValue::Uint(250),
            CborValue::Null,
            CborValue::Array(Vec::new()),
        ]))
        .unwrap();
        assert_eq!(
            watch_selector_from_wire(&bytes).unwrap_err().code,
            Code::InvalidArgument
        );
    }

    #[test]
    fn unknown_change_kind_tag_is_invalid_argument() {
        let bytes = encode(&CborValue::Array(vec![
            CborValue::Text(ws().to_string()),
            CborValue::Text("main".to_string()),
            CborValue::Null,
            CborValue::Null,
            CborValue::Array(vec![CborValue::Uint(9)]),
        ]))
        .unwrap();
        assert_eq!(
            watch_selector_from_wire(&bytes).unwrap_err().code,
            Code::InvalidArgument
        );
    }

    #[test]
    fn malformed_shape_is_invalid_argument() {
        // Not an array.
        let bad = encode(&CborValue::Text("nope".to_string())).unwrap();
        assert_eq!(
            watch_selector_from_wire(&bad).unwrap_err().code,
            Code::InvalidArgument
        );
        // Wrong element count.
        let short = encode(&CborValue::Array(vec![CborValue::Text("x".to_string())])).unwrap();
        assert_eq!(
            watch_selector_from_wire(&short).unwrap_err().code,
            Code::InvalidArgument
        );
        // Undecodable bytes.
        assert_eq!(
            watch_selector_from_wire(&[0xff, 0xff]).unwrap_err().code,
            Code::InvalidArgument
        );
    }
}
