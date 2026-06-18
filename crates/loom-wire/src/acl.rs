//! Canonical wire codecs for the ACL admin facet.
//!
//! The scalar `acl_grant`/`acl_revoke` arguments cross as small fixed atoms: `effect` and each
//! `right` and `domain` are exactly one byte,
//! and each `scope`/`predicate` is a self-describing CBOR value. The one-byte `effect`/`right` tags
//! and the scope-kind tags mirror the numeric vocabulary the C ABI already uses. A full [`AclGrant`]
//! (the `acl_list` wire form) crosses as the CBOR array
//! `[subject, workspace, domain, ref_glob, scopes, rights, effect, predicate]`.

use std::collections::BTreeSet;

use loom_codec::{Value as CborValue, decode, encode};
use loom_core::{
    AclDomain, AclEffect, AclGrant, AclPredicate, AclRight, AclScope, AclScopeKind, AclSubject,
    WorkspaceId,
};
use loom_types::{Code, LoomError};

fn corrupt(err: impl core::fmt::Display) -> LoomError {
    LoomError::new(Code::CorruptObject, format!("cbor: {err}"))
}

/// The stable one-byte tag for an [`AclEffect`] (`0` Allow, `1` Deny).
pub fn acl_effect_tag(effect: AclEffect) -> u8 {
    match effect {
        AclEffect::Allow => 0,
        AclEffect::Deny => 1,
    }
}

/// Decode the one-byte `effect` wire atom.
pub fn acl_effect_from_wire(bytes: &[u8]) -> Result<AclEffect, LoomError> {
    match bytes {
        [0] => Ok(AclEffect::Allow),
        [1] => Ok(AclEffect::Deny),
        [other] => Err(LoomError::invalid(format!("unknown acl effect {other}"))),
        _ => Err(LoomError::invalid("acl effect must be exactly one byte")),
    }
}

/// The stable one-byte tag for an [`AclRight`] (the single-bit value the C ABI uses in its mask).
pub fn acl_right_tag(right: AclRight) -> u8 {
    match right {
        AclRight::Read => 0x01,
        AclRight::Write => 0x02,
        AclRight::Advance => 0x04,
        AclRight::Merge => 0x08,
        AclRight::Execute => 0x10,
        AclRight::Admin => 0x20,
    }
}

fn acl_right_from_tag(tag: u8) -> Result<AclRight, LoomError> {
    Ok(match tag {
        0x01 => AclRight::Read,
        0x02 => AclRight::Write,
        0x04 => AclRight::Advance,
        0x08 => AclRight::Merge,
        0x10 => AclRight::Execute,
        0x20 => AclRight::Admin,
        other => return Err(LoomError::invalid(format!("unknown acl right {other:#x}"))),
    })
}

/// Decode the one-byte `right` wire atom.
pub fn acl_right_from_wire(bytes: &[u8]) -> Result<AclRight, LoomError> {
    match bytes {
        [tag] => acl_right_from_tag(*tag),
        _ => Err(LoomError::invalid("acl right must be exactly one byte")),
    }
}

fn acl_scope_kind_tag(kind: AclScopeKind) -> u8 {
    match kind {
        AclScopeKind::Ref => 0,
        AclScopeKind::Collection => 1,
        AclScopeKind::Path => 2,
        AclScopeKind::Key => 3,
        AclScopeKind::Table => 4,
        AclScopeKind::Exec => 5,
    }
}

fn acl_scope_kind_from_tag(tag: u64) -> Result<AclScopeKind, LoomError> {
    Ok(match tag {
        0 => AclScopeKind::Ref,
        1 => AclScopeKind::Collection,
        2 => AclScopeKind::Path,
        3 => AclScopeKind::Key,
        4 => AclScopeKind::Table,
        5 => AclScopeKind::Exec,
        other => {
            return Err(LoomError::invalid(format!(
                "unknown acl scope kind {other}"
            )));
        }
    })
}

fn acl_scope_to_value(scope: &AclScope) -> CborValue {
    match scope {
        AclScope::All => CborValue::Array(Vec::new()),
        AclScope::Prefix { kind, prefix } => CborValue::Array(vec![
            CborValue::Uint(u64::from(acl_scope_kind_tag(*kind))),
            CborValue::Bytes(prefix.clone()),
        ]),
    }
}

fn acl_scope_from_value(value: &CborValue) -> Result<AclScope, LoomError> {
    let CborValue::Array(items) = value else {
        return Err(LoomError::invalid("acl scope must be a cbor array"));
    };
    match items.as_slice() {
        [] => Ok(AclScope::All),
        [CborValue::Uint(kind), CborValue::Bytes(prefix)] => Ok(AclScope::Prefix {
            kind: acl_scope_kind_from_tag(*kind)?,
            prefix: prefix.clone(),
        }),
        _ => Err(LoomError::invalid("acl scope must be [] or [kind, prefix]")),
    }
}

/// Decode one `scope` wire atom (a self-describing CBOR value).
pub fn acl_scope_from_wire(bytes: &[u8]) -> Result<AclScope, LoomError> {
    let value = decode(bytes).map_err(|err| LoomError::invalid(format!("acl scope: {err}")))?;
    acl_scope_from_value(&value)
}

/// Decode the optional list of `scope` atoms. An absent or empty list matches all resources, mirroring
/// the C ABI (a zero scope count means `AclScope::All`).
pub fn acl_scopes_from_wire(scopes: Option<&[Vec<u8>]>) -> Result<Vec<AclScope>, LoomError> {
    match scopes {
        None | Some([]) => Ok(vec![AclScope::All]),
        Some(items) => items
            .iter()
            .map(|scope| acl_scope_from_wire(scope))
            .collect(),
    }
}

/// Decode the optional list of one-byte `right` atoms into a right set. An empty set is left to
/// `loom_core` to reject (`acl grant must include at least one right`).
pub fn acl_rights_from_wire(rights: Option<&[Vec<u8>]>) -> Result<BTreeSet<AclRight>, LoomError> {
    let mut out = BTreeSet::new();
    for right in rights.unwrap_or(&[]) {
        out.insert(acl_right_from_wire(right)?);
    }
    Ok(out)
}

/// Decode one `predicate` wire atom, the CBOR array `[language, expression]`.
pub fn acl_predicate_from_wire(bytes: &[u8]) -> Result<AclPredicate, LoomError> {
    let value = decode(bytes).map_err(|err| LoomError::invalid(format!("acl predicate: {err}")))?;
    let CborValue::Array(items) = value else {
        return Err(LoomError::invalid("acl predicate must be a cbor array"));
    };
    let [CborValue::Text(language), CborValue::Text(expression)] = items.as_slice() else {
        return Err(LoomError::invalid(
            "acl predicate must be [language, expression]",
        ));
    };
    Ok(AclPredicate {
        language: language.clone(),
        expression: expression.clone(),
    })
}

/// Parse a `subject` string into an [`AclSubject`]: `*`/`everyone`, `role:<uuid>`, or a bare principal
/// UUID (mirrors the C ABI subject grammar).
pub fn acl_subject_from_wire(subject: &str) -> Result<AclSubject, LoomError> {
    match subject {
        "*" | "everyone" => Ok(AclSubject::Everyone),
        role if role.starts_with("role:") => Ok(AclSubject::Role(WorkspaceId::parse(&role[5..])?)),
        other => Ok(AclSubject::Principal(WorkspaceId::parse(other)?)),
    }
}

/// Assemble an [`AclGrant`] from the wire-typed `acl_grant`/`acl_revoke` arguments. `workspace` is the
/// already-resolved scoping workspace (or `None` for a store-global grant).
#[allow(clippy::too_many_arguments)]
pub fn acl_grant_from_wire(
    effect: &[u8],
    subject: &str,
    workspace: Option<WorkspaceId>,
    domain: Option<&[u8]>,
    ref_glob: Option<String>,
    scopes: Option<&[Vec<u8>]>,
    rights: Option<&[Vec<u8>]>,
    predicate: Option<&[u8]>,
) -> Result<AclGrant, LoomError> {
    Ok(AclGrant {
        subject: acl_subject_from_wire(subject)?,
        workspace,
        domain: match domain {
            Some([tag]) => Some(
                AclDomain::from_stable_tag(*tag)
                    .ok_or_else(|| LoomError::invalid(format!("unknown ACL domain tag {tag}")))?,
            ),
            Some(_) => return Err(LoomError::invalid("ACL domain must be exactly one byte")),
            None => None,
        },
        ref_glob: ref_glob.filter(|value| !value.is_empty()),
        scopes: acl_scopes_from_wire(scopes)?,
        rights: acl_rights_from_wire(rights)?,
        effect: acl_effect_from_wire(effect)?,
        predicate: match predicate {
            Some(bytes) => Some(acl_predicate_from_wire(bytes)?),
            None => None,
        },
    })
}

/// Encode an [`AclEffect`] as its one-byte `effect` wire atom (inverse of [`acl_effect_from_wire`]).
pub fn acl_effect_to_wire(effect: AclEffect) -> Vec<u8> {
    vec![acl_effect_tag(effect)]
}

/// Encode an [`AclRight`] as its one-byte `right` wire atom (inverse of [`acl_right_from_wire`]).
pub fn acl_right_to_wire(right: AclRight) -> Vec<u8> {
    vec![acl_right_tag(right)]
}

/// Encode one `scope` atom as a self-describing CBOR value (inverse of [`acl_scope_from_wire`]).
pub fn acl_scope_to_wire(scope: &AclScope) -> Result<Vec<u8>, LoomError> {
    encode(&acl_scope_to_value(scope)).map_err(corrupt)
}

/// Encode one `predicate` atom as the CBOR array `[language, expression]` (inverse of
/// [`acl_predicate_from_wire`]).
pub fn acl_predicate_to_wire(predicate: &AclPredicate) -> Result<Vec<u8>, LoomError> {
    encode(&CborValue::Array(vec![
        CborValue::Text(predicate.language.clone()),
        CborValue::Text(predicate.expression.clone()),
    ]))
    .map_err(corrupt)
}

/// Decode a full [`AclGrant`] from its canonical `acl_list` wire record (the inverse of
/// [`acl_grant_to_cbor`]: the CBOR array `[subject, workspace, domain, ref_glob, scopes, rights, effect,
/// predicate]`).
pub fn acl_grant_from_cbor(bytes: &[u8]) -> Result<AclGrant, LoomError> {
    let value = decode(bytes).map_err(corrupt)?;
    let CborValue::Array(items) = value else {
        return Err(LoomError::invalid("acl grant record must be a cbor array"));
    };
    let [
        subject,
        workspace,
        domain,
        ref_glob,
        scopes,
        rights,
        effect,
        predicate,
    ] = items.as_slice()
    else {
        return Err(LoomError::invalid(
            "acl grant record must have 8 fields [subject, workspace, domain, ref_glob, scopes, rights, effect, predicate]",
        ));
    };
    let subject = match subject {
        CborValue::Array(fields) => match fields.as_slice() {
            [CborValue::Uint(0), _] => AclSubject::Everyone,
            [CborValue::Uint(1), CborValue::Text(id)] => {
                AclSubject::Principal(WorkspaceId::parse(id)?)
            }
            [CborValue::Uint(2), CborValue::Text(id)] => AclSubject::Role(WorkspaceId::parse(id)?),
            _ => return Err(LoomError::invalid("acl grant record has a bad subject")),
        },
        _ => return Err(LoomError::invalid("acl grant subject must be a cbor array")),
    };
    let workspace = match workspace {
        CborValue::Null => None,
        CborValue::Text(id) => Some(WorkspaceId::parse(id)?),
        _ => {
            return Err(LoomError::invalid(
                "acl grant workspace must be text or null",
            ));
        }
    };
    let domain = match domain {
        CborValue::Null => None,
        CborValue::Uint(tag) => {
            let tag = u8::try_from(*tag)
                .map_err(|_| LoomError::invalid("acl grant domain tag out of range"))?;
            Some(
                AclDomain::from_stable_tag(tag)
                    .ok_or_else(|| LoomError::invalid(format!("unknown ACL domain tag {tag}")))?,
            )
        }
        _ => {
            return Err(LoomError::invalid(
                "acl grant domain must be a uint or null",
            ));
        }
    };
    let ref_glob = match ref_glob {
        CborValue::Null => None,
        CborValue::Text(glob) => Some(glob.clone()).filter(|value| !value.is_empty()),
        _ => {
            return Err(LoomError::invalid(
                "acl grant ref_glob must be text or null",
            ));
        }
    };
    let scopes = match scopes {
        CborValue::Array(items) => items
            .iter()
            .map(acl_scope_from_value)
            .collect::<Result<Vec<_>, _>>()?,
        _ => return Err(LoomError::invalid("acl grant scopes must be a cbor array")),
    };
    let rights = match rights {
        CborValue::Array(items) => {
            let mut out = BTreeSet::new();
            for right in items {
                let CborValue::Uint(tag) = right else {
                    return Err(LoomError::invalid("acl grant right must be a uint"));
                };
                let tag = u8::try_from(*tag)
                    .map_err(|_| LoomError::invalid("acl grant right tag out of range"))?;
                out.insert(acl_right_from_tag(tag)?);
            }
            out
        }
        _ => return Err(LoomError::invalid("acl grant rights must be a cbor array")),
    };
    let effect = match effect {
        CborValue::Uint(0) => AclEffect::Allow,
        CborValue::Uint(1) => AclEffect::Deny,
        _ => return Err(LoomError::invalid("acl grant effect must be 0 or 1")),
    };
    let predicate = match predicate {
        CborValue::Null => None,
        CborValue::Array(fields) => match fields.as_slice() {
            [CborValue::Text(language), CborValue::Text(expression)] => Some(AclPredicate {
                language: language.clone(),
                expression: expression.clone(),
            }),
            _ => {
                return Err(LoomError::invalid(
                    "acl grant predicate must be [language, expression]",
                ));
            }
        },
        _ => {
            return Err(LoomError::invalid(
                "acl grant predicate must be an array or null",
            ));
        }
    };
    Ok(AclGrant {
        subject,
        workspace,
        domain,
        ref_glob,
        scopes,
        rights,
        effect,
        predicate,
    })
}

/// Encode a full [`AclGrant`] as the canonical CBOR array
/// `[subject, workspace, domain, ref_glob, scopes, rights, effect, predicate]` (the `acl_list` wire
/// form).
pub fn acl_grant_to_cbor(grant: &AclGrant) -> Result<Vec<u8>, LoomError> {
    let subject = match grant.subject {
        AclSubject::Everyone => {
            CborValue::Array(vec![CborValue::Uint(0), CborValue::Text(String::new())])
        }
        AclSubject::Principal(id) => {
            CborValue::Array(vec![CborValue::Uint(1), CborValue::Text(id.to_string())])
        }
        AclSubject::Role(id) => {
            CborValue::Array(vec![CborValue::Uint(2), CborValue::Text(id.to_string())])
        }
    };
    let workspace = match grant.workspace {
        Some(ws) => CborValue::Text(ws.to_string()),
        None => CborValue::Null,
    };
    let domain = match grant.domain {
        Some(domain) => CborValue::Uint(u64::from(domain.stable_tag())),
        None => CborValue::Null,
    };
    let ref_glob = match &grant.ref_glob {
        Some(glob) => CborValue::Text(glob.clone()),
        None => CborValue::Null,
    };
    let scopes = CborValue::Array(grant.scopes.iter().map(acl_scope_to_value).collect());
    let rights = CborValue::Array(
        grant
            .rights
            .iter()
            .map(|right| CborValue::Uint(u64::from(acl_right_tag(*right))))
            .collect(),
    );
    let predicate = match &grant.predicate {
        Some(predicate) => CborValue::Array(vec![
            CborValue::Text(predicate.language.clone()),
            CborValue::Text(predicate.expression.clone()),
        ]),
        None => CborValue::Null,
    };
    encode(&CborValue::Array(vec![
        subject,
        workspace,
        domain,
        ref_glob,
        scopes,
        rights,
        CborValue::Uint(u64::from(acl_effect_tag(grant.effect))),
        predicate,
    ]))
    .map_err(corrupt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effect_atoms_round_trip() {
        for effect in [AclEffect::Allow, AclEffect::Deny] {
            assert_eq!(
                acl_effect_from_wire(&[acl_effect_tag(effect)]).unwrap(),
                effect
            );
        }
        assert_eq!(
            acl_effect_from_wire(&[2]).unwrap_err().code,
            Code::InvalidArgument
        );
        assert_eq!(
            acl_effect_from_wire(&[]).unwrap_err().code,
            Code::InvalidArgument
        );
    }

    #[test]
    fn right_atoms_round_trip() {
        for right in [
            AclRight::Read,
            AclRight::Write,
            AclRight::Advance,
            AclRight::Merge,
            AclRight::Execute,
            AclRight::Admin,
        ] {
            assert_eq!(acl_right_from_wire(&[acl_right_tag(right)]).unwrap(), right);
        }
        assert_eq!(
            acl_right_from_wire(&[0x40]).unwrap_err().code,
            Code::InvalidArgument
        );
    }

    #[test]
    fn scope_all_and_prefix_round_trip() {
        let all = decode(&encode(&acl_scope_to_value(&AclScope::All)).unwrap()).unwrap();
        assert_eq!(acl_scope_from_value(&all).unwrap(), AclScope::All);

        let prefix = AclScope::Prefix {
            kind: AclScopeKind::Path,
            prefix: b"docs/".to_vec(),
        };
        let bytes = encode(&acl_scope_to_value(&prefix)).unwrap();
        assert_eq!(acl_scope_from_wire(&bytes).unwrap(), prefix);
    }

    #[test]
    fn absent_scopes_match_all() {
        assert_eq!(acl_scopes_from_wire(None).unwrap(), vec![AclScope::All]);
        assert_eq!(
            acl_scopes_from_wire(Some(&[])).unwrap(),
            vec![AclScope::All]
        );
    }

    #[test]
    fn subject_grammar() {
        assert_eq!(acl_subject_from_wire("*").unwrap(), AclSubject::Everyone);
        assert_eq!(
            acl_subject_from_wire("everyone").unwrap(),
            AclSubject::Everyone
        );
        let uuid = WorkspaceId::v4_from_bytes([3u8; 16]).to_string();
        assert_eq!(
            acl_subject_from_wire(&uuid).unwrap(),
            AclSubject::Principal(WorkspaceId::parse(&uuid).unwrap())
        );
        assert_eq!(
            acl_subject_from_wire(&format!("role:{uuid}")).unwrap(),
            AclSubject::Role(WorkspaceId::parse(&uuid).unwrap())
        );
    }

    #[test]
    fn grant_to_cbor_round_trips_shape() {
        let grant = AclGrant {
            subject: AclSubject::Everyone,
            workspace: None,
            domain: Some(loom_core::FacetKind::Files.into()),
            ref_glob: Some("refs/heads/*".to_string()),
            scopes: vec![AclScope::All],
            rights: BTreeSet::from([AclRight::Read, AclRight::Write]),
            effect: AclEffect::Allow,
            predicate: None,
        };
        let CborValue::Array(items) = decode(&acl_grant_to_cbor(&grant).unwrap()).unwrap() else {
            panic!("expected array");
        };
        assert_eq!(items.len(), 8);
        // subject: [0, ""]
        assert_eq!(
            items[0],
            CborValue::Array(vec![CborValue::Uint(0), CborValue::Text(String::new())])
        );
        assert_eq!(items[1], CborValue::Null); // workspace
        assert_eq!(
            items[2],
            CborValue::Uint(u64::from(loom_core::FacetKind::Files.stable_tag()))
        );
        assert_eq!(items[3], CborValue::Text("refs/heads/*".to_string()));
        assert_eq!(
            items[4],
            CborValue::Array(vec![CborValue::Array(Vec::new())])
        );
        assert_eq!(
            items[5],
            CborValue::Array(vec![
                CborValue::Uint(u64::from(acl_right_tag(AclRight::Read))),
                CborValue::Uint(u64::from(acl_right_tag(AclRight::Write))),
            ])
        );
        assert_eq!(items[6], CborValue::Uint(0)); // effect Allow
        assert_eq!(items[7], CborValue::Null); // predicate
    }

    #[test]
    fn grant_cbor_round_trips_through_from_cbor() {
        for predicate in [
            None,
            Some(AclPredicate {
                language: "cel".to_string(),
                expression: "subject == 'x'".to_string(),
            }),
        ] {
            let grant = AclGrant {
                subject: AclSubject::Role(WorkspaceId::v4_from_bytes([7u8; 16])),
                workspace: Some(WorkspaceId::v4_from_bytes([9u8; 16])),
                domain: Some(loom_core::FacetKind::Kv.into()),
                ref_glob: Some("refs/heads/*".to_string()),
                scopes: vec![
                    AclScope::All,
                    AclScope::Prefix {
                        kind: AclScopeKind::Path,
                        prefix: b"docs/".to_vec(),
                    },
                ],
                rights: BTreeSet::from([AclRight::Read, AclRight::Admin]),
                effect: AclEffect::Deny,
                predicate,
            };
            let encoded = acl_grant_to_cbor(&grant).unwrap();
            assert_eq!(acl_grant_from_cbor(&encoded).unwrap(), grant);
        }
    }

    #[test]
    fn arg_atoms_encode_inverse_of_decode() {
        assert_eq!(
            acl_effect_from_wire(&acl_effect_to_wire(AclEffect::Deny)).unwrap(),
            AclEffect::Deny
        );
        assert_eq!(
            acl_right_from_wire(&acl_right_to_wire(AclRight::Merge)).unwrap(),
            AclRight::Merge
        );
        let scope = AclScope::Prefix {
            kind: AclScopeKind::Key,
            prefix: b"k/".to_vec(),
        };
        assert_eq!(
            acl_scope_from_wire(&acl_scope_to_wire(&scope).unwrap()).unwrap(),
            scope
        );
        let predicate = AclPredicate {
            language: "cel".to_string(),
            expression: "true".to_string(),
        };
        assert_eq!(
            acl_predicate_from_wire(&acl_predicate_to_wire(&predicate).unwrap()).unwrap(),
            predicate
        );
    }

    #[test]
    fn grant_from_wire_assembles_typed_grant() {
        let grant = acl_grant_from_wire(
            &[acl_effect_tag(AclEffect::Deny)],
            "everyone",
            None,
            Some(&[loom_core::FacetKind::Kv.stable_tag()]),
            Some("k/*".to_string()),
            None,
            Some(&[vec![acl_right_tag(AclRight::Admin)]]),
            None,
        )
        .unwrap();
        assert_eq!(grant.effect, AclEffect::Deny);
        assert_eq!(grant.subject, AclSubject::Everyone);
        assert_eq!(grant.domain, Some(loom_core::AclDomain::Kv));
        assert_eq!(grant.ref_glob.as_deref(), Some("k/*"));
        assert_eq!(grant.scopes, vec![AclScope::All]);
        assert_eq!(grant.rights, BTreeSet::from([AclRight::Admin]));
    }
}
