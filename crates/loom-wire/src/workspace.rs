//! Canonical wire codecs for the workspace admin facet. A [`FacetKind`] crosses as exactly one byte -
//! its stable `loom_core` tag. A [`WorkspaceInfo`] crosses as the CBOR array `[id, name, facets, head]`
//! where `id` is the UUID text, `facets` is an array of the per-facet stable tags, and `head` is the
//! tip-commit digest text (`algo:hex`) or null.

use loom_codec::{Value as CborValue, encode};
use loom_core::{FacetKind, WorkspaceInfo};
use loom_types::{Code, LoomError};

/// The stable one-byte wire tag for a [`FacetKind`] (the `loom_core` durable facet tag).
pub fn facet_tag(facet: FacetKind) -> u8 {
    facet.stable_tag()
}

/// Decode a one-byte `facet` wire atom into a [`FacetKind`]. An empty or multi-byte buffer, or an
/// unknown tag, is `INVALID_ARGUMENT`.
pub fn facet_from_wire(bytes: &[u8]) -> Result<FacetKind, LoomError> {
    match bytes {
        [tag] => FacetKind::from_stable_tag(*tag)
            .ok_or_else(|| LoomError::invalid(format!("unknown workspace facet tag {tag}"))),
        _ => Err(LoomError::invalid(
            "workspace facet must be exactly one byte",
        )),
    }
}

/// Encode a [`WorkspaceInfo`] as the canonical CBOR array `[id, name, [facet_tag...], head]`.
pub fn workspace_info_to_cbor(info: &WorkspaceInfo) -> Result<Vec<u8>, LoomError> {
    let facets = info
        .facets
        .iter()
        .map(|facet| CborValue::Uint(u64::from(facet.stable_tag())))
        .collect::<Vec<_>>();
    let head = match &info.head {
        Some(head) => CborValue::Text(head.to_string()),
        None => CborValue::Null,
    };
    encode(&CborValue::Array(vec![
        CborValue::Text(info.id.to_string()),
        CborValue::Text(info.name.clone()),
        CborValue::Array(facets),
        head,
    ]))
    .map_err(|err| LoomError::new(Code::CorruptObject, format!("cbor: {err}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_core::WorkspaceId;
    use loom_core::digest::{Algo, Digest};

    #[test]
    fn facet_tags_round_trip() {
        for facet in FacetKind::ALL {
            assert_eq!(facet_from_wire(&[facet_tag(facet)]).unwrap(), facet);
        }
    }

    #[test]
    fn unknown_facet_tag_is_invalid_argument() {
        assert_eq!(
            facet_from_wire(&[200]).unwrap_err().code,
            Code::InvalidArgument
        );
    }

    #[test]
    fn empty_or_multi_byte_facet_is_invalid_argument() {
        assert_eq!(
            facet_from_wire(&[]).unwrap_err().code,
            Code::InvalidArgument
        );
        assert_eq!(
            facet_from_wire(&[0, 1]).unwrap_err().code,
            Code::InvalidArgument
        );
    }

    #[test]
    fn workspace_info_encodes_as_id_name_facets_head() {
        let head = Digest::hash(Algo::Blake3, b"tip");
        let info = WorkspaceInfo {
            id: WorkspaceId::v4_from_bytes([7u8; 16]),
            name: "main".to_string(),
            facets: vec![FacetKind::Files, FacetKind::Vcs],
            head: Some(head),
        };
        let CborValue::Array(items) =
            loom_codec::decode(&workspace_info_to_cbor(&info).unwrap()).unwrap()
        else {
            panic!("expected array");
        };
        assert_eq!(items.len(), 4);
        assert_eq!(items[0], CborValue::Text(info.id.to_string()));
        assert_eq!(items[1], CborValue::Text("main".to_string()));
        assert_eq!(
            items[2],
            CborValue::Array(vec![
                CborValue::Uint(u64::from(FacetKind::Files.stable_tag())),
                CborValue::Uint(u64::from(FacetKind::Vcs.stable_tag())),
            ])
        );
        assert_eq!(items[3], CborValue::Text(head.to_string()));
    }

    #[test]
    fn workspace_info_head_none_encodes_as_null() {
        let info = WorkspaceInfo {
            id: WorkspaceId::v4_from_bytes([1u8; 16]),
            name: "empty".to_string(),
            facets: Vec::new(),
            head: None,
        };
        let CborValue::Array(items) =
            loom_codec::decode(&workspace_info_to_cbor(&info).unwrap()).unwrap()
        else {
            panic!("expected array");
        };
        assert_eq!(items[3], CborValue::Null);
        assert_eq!(items[2], CborValue::Array(Vec::new()));
    }
}
