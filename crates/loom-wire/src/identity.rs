//! Canonical wire codecs for the identity control plane.
//!
//! `identity_list` returns a full `IdentitySnapshot` (IDL `struct IdentitySnapshot`), encoded here as a
//! CBOR array `[authenticated_mode, root, authority, authority_handoffs, forced_detach, principals,
//! roles, app_credentials, external_credentials, public_keys]`. Every nested record is a CBOR array in
//! its IDL field order; ids are UUID text, enum discriminants are their stable `loom_core` tags, and
//! optional fields are the value or CBOR null. The record shapes mirror the IDL structs and the C ABI
//! `identity_list_json` field set.
//!
//! On the input side, `PrincipalKind` and `ExternalCredentialKind` cross as exactly one stable tag
//! byte, and an `ExternalCredentialSpec` crosses as `[kind_tag, label, issuer, subject,
//! material_digest]` (its `id` is minted by the caller, not carried on the wire). Unknown tags and any
//! malformed shape are `INVALID_ARGUMENT`.

use loom_codec::{Value as CborValue, decode, encode};
use loom_core::digest::Digest;
use loom_core::{
    AppCredential, ExternalCredential, ExternalCredentialKind, ExternalCredentialSpec,
    IdentityAuthorityDetach, IdentityAuthorityHandoff, IdentityAuthorityMode,
    IdentityAuthorityState, IdentityPublicKey, IdentityRole, IdentityStore, Principal,
    PrincipalKind, WorkspaceId,
};
use loom_types::{Code, LoomError};

fn encode_value(value: CborValue) -> Result<Vec<u8>, LoomError> {
    encode(&value).map_err(|err| LoomError::new(Code::CorruptObject, format!("cbor: {err}")))
}

fn uuid_text(id: WorkspaceId) -> CborValue {
    CborValue::Text(id.to_string())
}

fn opt_uuid_text(id: Option<WorkspaceId>) -> CborValue {
    match id {
        Some(id) => uuid_text(id),
        None => CborValue::Null,
    }
}

fn opt_digest_text(digest: &Option<Digest>) -> CborValue {
    match digest {
        Some(digest) => CborValue::Text(digest.to_string()),
        None => CborValue::Null,
    }
}

fn opt_string(value: &Option<String>) -> CborValue {
    match value {
        Some(value) => CborValue::Text(value.clone()),
        None => CborValue::Null,
    }
}

/// The stable wire tag for an authority mode (IDL `enum IdentityAuthorityMode`: Authority=0, Mirror=1,
/// Detached=2). Encode-only: the snapshot is an output value.
fn authority_mode_tag(mode: IdentityAuthorityMode) -> u64 {
    match mode {
        IdentityAuthorityMode::Authority => 0,
        IdentityAuthorityMode::Mirror => 1,
        IdentityAuthorityMode::Detached => 2,
    }
}

fn principal_to_value(principal: &Principal) -> CborValue {
    CborValue::Array(vec![
        uuid_text(principal.id),
        CborValue::Text(principal.handle.clone()),
        CborValue::Text(principal.name.clone()),
        CborValue::Uint(u64::from(principal.kind.stable_tag())),
        CborValue::Bool(principal.enabled),
        CborValue::Bool(principal.has_passphrase),
        CborValue::Array(
            principal
                .roles
                .iter()
                .map(|role| uuid_text(*role))
                .collect(),
        ),
    ])
}

fn role_to_value(role: &IdentityRole) -> CborValue {
    CborValue::Array(vec![
        uuid_text(role.id),
        CborValue::Text(role.name.clone()),
        CborValue::Bool(role.enabled),
    ])
}

fn app_credential_to_value(credential: &AppCredential) -> CborValue {
    CborValue::Array(vec![
        uuid_text(credential.id),
        uuid_text(credential.principal),
        CborValue::Text(credential.label.clone()),
        CborValue::Bool(credential.enabled),
    ])
}

fn external_credential_to_value(credential: &ExternalCredential) -> CborValue {
    CborValue::Array(vec![
        uuid_text(credential.id),
        uuid_text(credential.principal),
        CborValue::Uint(u64::from(credential.kind.stable_tag())),
        CborValue::Text(credential.label.clone()),
        CborValue::Text(credential.issuer.clone()),
        CborValue::Text(credential.subject.clone()),
        opt_string(&credential.material_digest),
        CborValue::Bool(credential.enabled),
    ])
}

fn public_key_to_value(key: &IdentityPublicKey) -> CborValue {
    CborValue::Array(vec![
        uuid_text(key.id),
        uuid_text(key.principal),
        CborValue::Text(key.label.clone()),
        CborValue::Text(key.algorithm.clone()),
        CborValue::Bytes(key.public_key.clone()),
        CborValue::Bool(key.enabled),
    ])
}

fn authority_state_to_value(state: &IdentityAuthorityState) -> CborValue {
    CborValue::Array(vec![
        CborValue::Uint(authority_mode_tag(state.mode)),
        uuid_text(state.authority),
        CborValue::Uint(state.generation),
        opt_digest_text(&state.head),
    ])
}

fn authority_handoff_to_value(handoff: &IdentityAuthorityHandoff) -> CborValue {
    CborValue::Array(vec![
        uuid_text(handoff.from),
        uuid_text(handoff.to),
        CborValue::Uint(handoff.generation),
        opt_digest_text(&handoff.head),
        CborValue::Bytes(handoff.signed_record.clone()),
    ])
}

fn authority_detach_to_value(detach: &IdentityAuthorityDetach) -> CborValue {
    CborValue::Array(vec![
        uuid_text(detach.previous_authority),
        uuid_text(detach.new_authority),
        CborValue::Uint(detach.generation),
        CborValue::Text(detach.reason.clone()),
    ])
}

/// Encode a `PrincipalRecord` `[id, handle, name, kind_tag, enabled, has_passphrase, roles]`.
pub fn principal_record_to_cbor(principal: &Principal) -> Result<Vec<u8>, LoomError> {
    encode_value(principal_to_value(principal))
}

/// Encode a `RoleRecord` `[id, name, enabled]`.
pub fn role_record_to_cbor(role: &IdentityRole) -> Result<Vec<u8>, LoomError> {
    encode_value(role_to_value(role))
}

/// Encode an `AppCredentialRecord` `[id, principal, label, enabled]`.
pub fn app_credential_record_to_cbor(credential: &AppCredential) -> Result<Vec<u8>, LoomError> {
    encode_value(app_credential_to_value(credential))
}

/// Encode an `ExternalCredentialRecord`
/// `[id, principal, kind_tag, label, issuer, subject, material_digest, enabled]`.
pub fn external_credential_record_to_cbor(
    credential: &ExternalCredential,
) -> Result<Vec<u8>, LoomError> {
    encode_value(external_credential_to_value(credential))
}

/// Encode an `IdentityPublicKeyRecord` `[id, principal, label, algorithm, public_key, enabled]`.
pub fn public_key_record_to_cbor(key: &IdentityPublicKey) -> Result<Vec<u8>, LoomError> {
    encode_value(public_key_to_value(key))
}

/// Encode a full [`IdentityStore`] as the canonical `IdentitySnapshot` CBOR array.
pub fn identity_snapshot_to_cbor(store: &IdentityStore) -> Result<Vec<u8>, LoomError> {
    let forced_detach = match store.forced_detach() {
        Some(detach) => authority_detach_to_value(detach),
        None => CborValue::Null,
    };
    encode_value(CborValue::Array(vec![
        CborValue::Bool(store.authenticated_mode()),
        opt_uuid_text(store.root_principal()),
        authority_state_to_value(store.authority_state()),
        CborValue::Array(
            store
                .authority_handoffs()
                .map(authority_handoff_to_value)
                .collect(),
        ),
        forced_detach,
        CborValue::Array(store.principals().map(principal_to_value).collect()),
        CborValue::Array(store.roles().map(role_to_value).collect()),
        CborValue::Array(
            store
                .app_credentials()
                .map(app_credential_to_value)
                .collect(),
        ),
        CborValue::Array(
            store
                .external_credentials()
                .map(external_credential_to_value)
                .collect(),
        ),
        CborValue::Array(store.public_keys().map(public_key_to_value).collect()),
    ]))
}

/// Decode the one-byte `PrincipalKind` wire atom.
pub fn principal_kind_from_wire(bytes: &[u8]) -> Result<PrincipalKind, LoomError> {
    match bytes {
        [tag] => PrincipalKind::from_stable_tag(*tag)
            .ok_or_else(|| LoomError::invalid(format!("unknown principal kind {tag}"))),
        _ => Err(LoomError::invalid(
            "principal kind must be exactly one byte",
        )),
    }
}

fn take_text(value: &CborValue, field: &str) -> Result<String, LoomError> {
    match value {
        CborValue::Text(text) => Ok(text.clone()),
        _ => Err(LoomError::invalid(format!(
            "external credential {field} must be text"
        ))),
    }
}

/// Decode an `ExternalCredentialSpec` wire blob `[kind_tag, label, issuer, subject, material_digest]`,
/// binding the caller-minted `id`.
/// Encode an [`ExternalCredentialSpec`] to its wire form `[kind_tag, label, issuer, subject,
/// material_digest]` for `identity_create_external_credential`. The `id` is minted server-side and is not
/// carried on the wire.
pub fn external_credential_spec_to_wire(
    spec: &ExternalCredentialSpec,
) -> Result<Vec<u8>, LoomError> {
    encode_value(CborValue::Array(vec![
        CborValue::Uint(u64::from(spec.kind.stable_tag())),
        CborValue::Text(spec.label.clone()),
        CborValue::Text(spec.issuer.clone()),
        CborValue::Text(spec.subject.clone()),
        opt_string(&spec.material_digest),
    ]))
}

pub fn external_credential_spec_from_wire(
    bytes: &[u8],
    id: WorkspaceId,
) -> Result<ExternalCredentialSpec, LoomError> {
    let value = decode(bytes)
        .map_err(|err| LoomError::invalid(format!("external credential spec: {err}")))?;
    let CborValue::Array(items) = value else {
        return Err(LoomError::invalid(
            "external credential spec must be a cbor array",
        ));
    };
    let [kind, label, issuer, subject, material_digest] = items.as_slice() else {
        return Err(LoomError::invalid(
            "external credential spec must be [kind, label, issuer, subject, material_digest]",
        ));
    };
    let CborValue::Uint(kind) = kind else {
        return Err(LoomError::invalid(
            "external credential kind must be a tag uint",
        ));
    };
    let kind = u8::try_from(*kind)
        .ok()
        .and_then(ExternalCredentialKind::from_stable_tag)
        .ok_or_else(|| LoomError::invalid("unknown external credential kind tag"))?;
    let material_digest = match material_digest {
        CborValue::Null => None,
        CborValue::Text(text) => Some(text.clone()),
        _ => {
            return Err(LoomError::invalid(
                "external credential material_digest must be text or null",
            ));
        }
    };
    Ok(ExternalCredentialSpec {
        id,
        kind,
        label: take_text(label, "label")?,
        issuer: take_text(issuer, "issuer")?,
        subject: take_text(subject, "subject")?,
        material_digest,
    })
}

/// A decoded [`IdentitySnapshot`] carrying the public identity view (the field set the CLI `identity
/// list` output renders). Every field mirrors the canonical snapshot record shapes; secrets (passphrase
/// material, credential secrets) are not part of the snapshot and so are absent here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentitySnapshotView {
    pub authenticated_mode: bool,
    pub root: Option<WorkspaceId>,
    pub authority: IdentityAuthorityState,
    pub authority_handoffs: Vec<IdentityAuthorityHandoff>,
    pub forced_detach: Option<IdentityAuthorityDetach>,
    pub principals: Vec<Principal>,
    pub roles: Vec<IdentityRole>,
    pub app_credentials: Vec<AppCredential>,
    pub external_credentials: Vec<ExternalCredential>,
    pub public_keys: Vec<IdentityPublicKey>,
}

fn as_array<'a>(value: &'a CborValue, field: &str) -> Result<&'a [CborValue], LoomError> {
    match value {
        CborValue::Array(items) => Ok(items.as_slice()),
        _ => Err(LoomError::invalid(format!("{field} must be a cbor array"))),
    }
}

fn as_text(value: &CborValue, field: &str) -> Result<String, LoomError> {
    match value {
        CborValue::Text(text) => Ok(text.clone()),
        _ => Err(LoomError::invalid(format!("{field} must be text"))),
    }
}

fn as_bool(value: &CborValue, field: &str) -> Result<bool, LoomError> {
    match value {
        CborValue::Bool(flag) => Ok(*flag),
        _ => Err(LoomError::invalid(format!("{field} must be a bool"))),
    }
}

fn as_uint(value: &CborValue, field: &str) -> Result<u64, LoomError> {
    match value {
        CborValue::Uint(num) => Ok(*num),
        _ => Err(LoomError::invalid(format!("{field} must be a uint"))),
    }
}

fn as_bytes(value: &CborValue, field: &str) -> Result<Vec<u8>, LoomError> {
    match value {
        CborValue::Bytes(bytes) => Ok(bytes.clone()),
        _ => Err(LoomError::invalid(format!("{field} must be bytes"))),
    }
}

fn as_uuid(value: &CborValue, field: &str) -> Result<WorkspaceId, LoomError> {
    WorkspaceId::parse(&as_text(value, field)?)
        .map_err(|err| LoomError::invalid(format!("{field}: {err}")))
}

fn as_opt_uuid(value: &CborValue, field: &str) -> Result<Option<WorkspaceId>, LoomError> {
    match value {
        CborValue::Null => Ok(None),
        _ => Ok(Some(as_uuid(value, field)?)),
    }
}

fn as_opt_digest(value: &CborValue, field: &str) -> Result<Option<Digest>, LoomError> {
    match value {
        CborValue::Null => Ok(None),
        _ => Digest::parse(&as_text(value, field)?)
            .map(Some)
            .map_err(|err| LoomError::invalid(format!("{field}: {err}"))),
    }
}

fn as_opt_text(value: &CborValue, field: &str) -> Result<Option<String>, LoomError> {
    match value {
        CborValue::Null => Ok(None),
        _ => Ok(Some(as_text(value, field)?)),
    }
}

/// Decode the stable authority-mode tag (Authority=0, Mirror=1, Detached=2) written by
/// [`authority_mode_tag`].
fn authority_mode_from_tag(tag: u64) -> Result<IdentityAuthorityMode, LoomError> {
    match tag {
        0 => Ok(IdentityAuthorityMode::Authority),
        1 => Ok(IdentityAuthorityMode::Mirror),
        2 => Ok(IdentityAuthorityMode::Detached),
        other => Err(LoomError::invalid(format!(
            "unknown authority mode tag {other}"
        ))),
    }
}

fn principal_kind_from_tag(tag: u64) -> Result<PrincipalKind, LoomError> {
    u8::try_from(tag)
        .ok()
        .and_then(PrincipalKind::from_stable_tag)
        .ok_or_else(|| LoomError::invalid(format!("unknown principal kind tag {tag}")))
}

fn external_credential_kind_from_tag(tag: u64) -> Result<ExternalCredentialKind, LoomError> {
    u8::try_from(tag)
        .ok()
        .and_then(ExternalCredentialKind::from_stable_tag)
        .ok_or_else(|| LoomError::invalid(format!("unknown external credential kind tag {tag}")))
}

fn authority_state_from_value(value: &CborValue) -> Result<IdentityAuthorityState, LoomError> {
    let [mode, authority, generation, head] = as_array(value, "authority")? else {
        return Err(LoomError::invalid(
            "authority must be [mode, authority, generation, head]",
        ));
    };
    Ok(IdentityAuthorityState {
        mode: authority_mode_from_tag(as_uint(mode, "authority mode")?)?,
        authority: as_uuid(authority, "authority principal")?,
        generation: as_uint(generation, "authority generation")?,
        head: as_opt_digest(head, "authority head")?,
    })
}

fn authority_handoff_from_value(value: &CborValue) -> Result<IdentityAuthorityHandoff, LoomError> {
    let [from, to, generation, head, signed_record] = as_array(value, "authority handoff")? else {
        return Err(LoomError::invalid(
            "authority handoff must be [from, to, generation, head, signed_record]",
        ));
    };
    Ok(IdentityAuthorityHandoff {
        from: as_uuid(from, "handoff from")?,
        to: as_uuid(to, "handoff to")?,
        generation: as_uint(generation, "handoff generation")?,
        head: as_opt_digest(head, "handoff head")?,
        signed_record: as_bytes(signed_record, "handoff signed_record")?,
    })
}

fn authority_detach_from_value(value: &CborValue) -> Result<IdentityAuthorityDetach, LoomError> {
    let [previous, new, generation, reason] = as_array(value, "forced detach")? else {
        return Err(LoomError::invalid(
            "forced detach must be [previous_authority, new_authority, generation, reason]",
        ));
    };
    Ok(IdentityAuthorityDetach {
        previous_authority: as_uuid(previous, "detach previous_authority")?,
        new_authority: as_uuid(new, "detach new_authority")?,
        generation: as_uint(generation, "detach generation")?,
        reason: as_text(reason, "detach reason")?,
    })
}

fn principal_from_value(value: &CborValue) -> Result<Principal, LoomError> {
    let [id, handle, name, kind, enabled, has_passphrase, roles] =
        as_array(value, "principal record")?
    else {
        return Err(LoomError::invalid(
            "principal record must be [id, handle, name, kind, enabled, has_passphrase, roles]",
        ));
    };
    let roles = as_array(roles, "principal roles")?
        .iter()
        .map(|role| as_uuid(role, "principal role"))
        .collect::<Result<_, _>>()?;
    Ok(Principal {
        id: as_uuid(id, "principal id")?,
        handle: as_text(handle, "principal handle")?,
        name: as_text(name, "principal name")?,
        kind: principal_kind_from_tag(as_uint(kind, "principal kind")?)?,
        enabled: as_bool(enabled, "principal enabled")?,
        has_passphrase: as_bool(has_passphrase, "principal has_passphrase")?,
        roles,
    })
}

fn role_from_value(value: &CborValue) -> Result<IdentityRole, LoomError> {
    let [id, name, enabled] = as_array(value, "role record")? else {
        return Err(LoomError::invalid(
            "role record must be [id, name, enabled]",
        ));
    };
    Ok(IdentityRole {
        id: as_uuid(id, "role id")?,
        name: as_text(name, "role name")?,
        enabled: as_bool(enabled, "role enabled")?,
    })
}

fn app_credential_from_value(value: &CborValue) -> Result<AppCredential, LoomError> {
    let [id, principal, label, enabled] = as_array(value, "app credential record")? else {
        return Err(LoomError::invalid(
            "app credential record must be [id, principal, label, enabled]",
        ));
    };
    Ok(AppCredential {
        id: as_uuid(id, "app credential id")?,
        principal: as_uuid(principal, "app credential principal")?,
        label: as_text(label, "app credential label")?,
        enabled: as_bool(enabled, "app credential enabled")?,
    })
}

fn external_credential_from_value(value: &CborValue) -> Result<ExternalCredential, LoomError> {
    let [
        id,
        principal,
        kind,
        label,
        issuer,
        subject,
        material_digest,
        enabled,
    ] = as_array(value, "external credential record")?
    else {
        return Err(LoomError::invalid(
            "external credential record must be [id, principal, kind, label, issuer, subject, \
             material_digest, enabled]",
        ));
    };
    Ok(ExternalCredential {
        id: as_uuid(id, "external credential id")?,
        principal: as_uuid(principal, "external credential principal")?,
        kind: external_credential_kind_from_tag(as_uint(kind, "external credential kind")?)?,
        label: as_text(label, "external credential label")?,
        issuer: as_text(issuer, "external credential issuer")?,
        subject: as_text(subject, "external credential subject")?,
        material_digest: as_opt_text(material_digest, "external credential material_digest")?,
        enabled: as_bool(enabled, "external credential enabled")?,
    })
}

fn public_key_from_value(value: &CborValue) -> Result<IdentityPublicKey, LoomError> {
    let [id, principal, label, algorithm, public_key, enabled] =
        as_array(value, "public key record")?
    else {
        return Err(LoomError::invalid(
            "public key record must be [id, principal, label, algorithm, public_key, enabled]",
        ));
    };
    Ok(IdentityPublicKey {
        id: as_uuid(id, "public key id")?,
        principal: as_uuid(principal, "public key principal")?,
        label: as_text(label, "public key label")?,
        algorithm: as_text(algorithm, "public key algorithm")?,
        public_key: as_bytes(public_key, "public key material")?,
        enabled: as_bool(enabled, "public key enabled")?,
    })
}

fn decode_list<T>(
    value: &CborValue,
    field: &str,
    decode_item: impl Fn(&CborValue) -> Result<T, LoomError>,
) -> Result<Vec<T>, LoomError> {
    as_array(value, field)?.iter().map(decode_item).collect()
}

/// Decode a full [`IdentitySnapshotView`] from the canonical `IdentitySnapshot` CBOR array produced by
/// [`identity_snapshot_to_cbor`] (its inverse for the public view).
pub fn identity_snapshot_from_cbor(bytes: &[u8]) -> Result<IdentitySnapshotView, LoomError> {
    let value =
        decode(bytes).map_err(|err| LoomError::invalid(format!("identity snapshot: {err}")))?;
    let [
        authenticated_mode,
        root,
        authority,
        authority_handoffs,
        forced_detach,
        principals,
        roles,
        app_credentials,
        external_credentials,
        public_keys,
    ] = as_array(&value, "identity snapshot")?
    else {
        return Err(LoomError::invalid(
            "identity snapshot must be a 10-element cbor array",
        ));
    };
    Ok(IdentitySnapshotView {
        authenticated_mode: as_bool(authenticated_mode, "authenticated_mode")?,
        root: as_opt_uuid(root, "root")?,
        authority: authority_state_from_value(authority)?,
        authority_handoffs: decode_list(
            authority_handoffs,
            "authority_handoffs",
            authority_handoff_from_value,
        )?,
        forced_detach: match forced_detach {
            CborValue::Null => None,
            other => Some(authority_detach_from_value(other)?),
        },
        principals: decode_list(principals, "principals", principal_from_value)?,
        roles: decode_list(roles, "roles", role_from_value)?,
        app_credentials: decode_list(
            app_credentials,
            "app_credentials",
            app_credential_from_value,
        )?,
        external_credentials: decode_list(
            external_credentials,
            "external_credentials",
            external_credential_from_value,
        )?,
        public_keys: decode_list(public_keys, "public_keys", public_key_from_value)?,
    })
}

/// A decoded audited identity mutation result: the audit sequence assigned to the mutation, the minted
/// id when the mutation creates one (external credential or public key), and the echoed audit action and
/// redacted target. Carries no secret material (credential secrets and public-key bytes never enter the
/// audit record).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityAuditResult {
    pub audit_seq: u64,
    pub id: Option<WorkspaceId>,
    pub action: String,
    pub target: Option<String>,
}

/// Encode an [`IdentityAuditResult`] as the canonical CBOR array `[audit_seq, id, action, target]`, with
/// `id`/`target` as text or CBOR null.
pub fn identity_audit_result_to_cbor(result: &IdentityAuditResult) -> Result<Vec<u8>, LoomError> {
    encode_value(CborValue::Array(vec![
        CborValue::Uint(result.audit_seq),
        opt_uuid_text(result.id),
        CborValue::Text(result.action.clone()),
        opt_string(&result.target),
    ]))
}

/// Decode an [`IdentityAuditResult`] from the canonical CBOR array produced by
/// [`identity_audit_result_to_cbor`].
pub fn identity_audit_result_from_cbor(bytes: &[u8]) -> Result<IdentityAuditResult, LoomError> {
    let value =
        decode(bytes).map_err(|err| LoomError::invalid(format!("identity audit result: {err}")))?;
    let [audit_seq, id, action, target] = as_array(&value, "identity audit result")? else {
        return Err(LoomError::invalid(
            "identity audit result must be a 4-element cbor array",
        ));
    };
    Ok(IdentityAuditResult {
        audit_seq: as_uint(audit_seq, "audit_seq")?,
        id: as_opt_uuid(id, "id")?,
        action: as_text(action, "action")?,
        target: match target {
            CborValue::Null => None,
            other => Some(as_text(other, "target")?),
        },
    })
}

/// The one-time result of an app-credential create: the audit sequence, the stored (secret-free) record
/// fields, and the plaintext bearer token delivered exactly once. This is a dedicated secret-bearing type
/// kept separate from [`IdentityAuditResult`]; the token is never echoed by any read/list/audit/revoke
/// path and is never persisted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppCredentialCreateResult {
    pub audit_seq: u64,
    pub id: WorkspaceId,
    pub principal: WorkspaceId,
    pub label: String,
    pub enabled: bool,
    pub secret_token: String,
}

/// Encode an [`AppCredentialCreateResult`] as the canonical CBOR array
/// `[audit_seq, id, principal, label, enabled, secret_token]`.
pub fn app_credential_create_result_to_cbor(
    result: &AppCredentialCreateResult,
) -> Result<Vec<u8>, LoomError> {
    encode_value(CborValue::Array(vec![
        CborValue::Uint(result.audit_seq),
        uuid_text(result.id),
        uuid_text(result.principal),
        CborValue::Text(result.label.clone()),
        CborValue::Bool(result.enabled),
        CborValue::Text(result.secret_token.clone()),
    ]))
}

/// Decode an [`AppCredentialCreateResult`] from the canonical CBOR array produced by
/// [`app_credential_create_result_to_cbor`].
pub fn app_credential_create_result_from_cbor(
    bytes: &[u8],
) -> Result<AppCredentialCreateResult, LoomError> {
    let value = decode(bytes)
        .map_err(|err| LoomError::invalid(format!("app credential create result: {err}")))?;
    let [audit_seq, id, principal, label, enabled, secret_token] =
        as_array(&value, "app credential create result")?
    else {
        return Err(LoomError::invalid(
            "app credential create result must be a 6-element cbor array",
        ));
    };
    Ok(AppCredentialCreateResult {
        audit_seq: as_uint(audit_seq, "audit_seq")?,
        id: as_uuid(id, "id")?,
        principal: as_uuid(principal, "principal")?,
        label: as_text(label, "label")?,
        enabled: as_bool(enabled, "enabled")?,
        secret_token: as_text(secret_token, "secret_token")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(seed: u8) -> WorkspaceId {
        WorkspaceId::v4_from_bytes([seed; 16])
    }

    #[test]
    fn app_credential_create_result_round_trips() {
        let result = AppCredentialCreateResult {
            audit_seq: 3,
            id: id(0x31),
            principal: id(0x07),
            label: "ci-runner".to_string(),
            enabled: true,
            secret_token: "loom_app_00000000-0000-4000-8000-000000000031_deadbeef".to_string(),
        };
        let bytes = app_credential_create_result_to_cbor(&result).unwrap();
        assert_eq!(
            app_credential_create_result_from_cbor(&bytes).unwrap(),
            result
        );
        assert_eq!(
            app_credential_create_result_from_cbor(b"\x00")
                .unwrap_err()
                .code,
            Code::InvalidArgument
        );
    }

    #[test]
    fn external_credential_spec_wire_round_trips() {
        let spec = ExternalCredentialSpec {
            id: id(0xc1),
            kind: ExternalCredentialKind::OidcSubject,
            label: "ci".to_string(),
            issuer: "https://issuer".to_string(),
            subject: "svc-bot".to_string(),
            material_digest: Some("blake3:abcd".to_string()),
        };
        let bytes = external_credential_spec_to_wire(&spec).unwrap();
        assert_eq!(
            external_credential_spec_from_wire(&bytes, spec.id).unwrap(),
            spec
        );
    }

    #[test]
    fn identity_audit_result_round_trips() {
        for result in [
            IdentityAuditResult {
                audit_seq: 7,
                id: Some(id(0xa1)),
                action: "identity.create_external_credential".to_string(),
                target: Some(id(0xa1).to_string()),
            },
            IdentityAuditResult {
                audit_seq: 0,
                id: None,
                action: "identity.revoke_public_key".to_string(),
                target: None,
            },
        ] {
            let bytes = identity_audit_result_to_cbor(&result).unwrap();
            assert_eq!(identity_audit_result_from_cbor(&bytes).unwrap(), result);
        }
        assert_eq!(
            identity_audit_result_from_cbor(b"\x00").unwrap_err().code,
            Code::InvalidArgument
        );
    }

    #[test]
    fn principal_kind_atom_round_trips() {
        for kind in [
            PrincipalKind::Root,
            PrincipalKind::User,
            PrincipalKind::Service,
        ] {
            assert_eq!(
                principal_kind_from_wire(&[kind.stable_tag()]).unwrap(),
                kind
            );
        }
        assert_eq!(
            principal_kind_from_wire(&[9]).unwrap_err().code,
            Code::InvalidArgument
        );
        assert_eq!(
            principal_kind_from_wire(&[]).unwrap_err().code,
            Code::InvalidArgument
        );
        assert_eq!(
            principal_kind_from_wire(&[0, 1]).unwrap_err().code,
            Code::InvalidArgument
        );
    }

    #[test]
    fn external_credential_kind_tags_are_stable() {
        assert_eq!(ExternalCredentialKind::PublicKey.stable_tag(), 0);
        assert_eq!(ExternalCredentialKind::MtlsCertificate.stable_tag(), 1);
        assert_eq!(ExternalCredentialKind::Passkey.stable_tag(), 2);
        assert_eq!(ExternalCredentialKind::OidcSubject.stable_tag(), 3);
        assert_eq!(ExternalCredentialKind::SamlSubject.stable_tag(), 4);
        assert_eq!(ExternalCredentialKind::from_stable_tag(5), None);
    }

    #[test]
    fn external_credential_spec_decodes_with_minted_id() {
        let bytes = encode(&CborValue::Array(vec![
            CborValue::Uint(u64::from(ExternalCredentialKind::OidcSubject.stable_tag())),
            CborValue::Text("label".to_string()),
            CborValue::Text("issuer".to_string()),
            CborValue::Text("subject".to_string()),
            CborValue::Null,
        ]))
        .unwrap();
        let spec = external_credential_spec_from_wire(&bytes, id(7)).unwrap();
        assert_eq!(spec.id, id(7));
        assert_eq!(spec.kind, ExternalCredentialKind::OidcSubject);
        assert_eq!(spec.label, "label");
        assert_eq!(spec.material_digest, None);
    }

    #[test]
    fn external_credential_spec_rejects_unknown_kind_and_bad_shape() {
        let bad_kind = encode(&CborValue::Array(vec![
            CborValue::Uint(99),
            CborValue::Text("l".to_string()),
            CborValue::Text("i".to_string()),
            CborValue::Text("s".to_string()),
            CborValue::Null,
        ]))
        .unwrap();
        assert_eq!(
            external_credential_spec_from_wire(&bad_kind, id(1))
                .unwrap_err()
                .code,
            Code::InvalidArgument
        );
        let short = encode(&CborValue::Array(vec![CborValue::Uint(0)])).unwrap();
        assert_eq!(
            external_credential_spec_from_wire(&short, id(1))
                .unwrap_err()
                .code,
            Code::InvalidArgument
        );
        assert_eq!(
            external_credential_spec_from_wire(&[0xff, 0xff], id(1))
                .unwrap_err()
                .code,
            Code::InvalidArgument
        );
    }

    #[test]
    fn principal_record_shape() {
        let store = IdentityStore::new(id(1));
        let root = store.principals().next().unwrap();
        let CborValue::Array(items) =
            loom_codec::decode(&principal_record_to_cbor(root).unwrap()).unwrap()
        else {
            panic!("expected array");
        };
        // [id, handle, name, kind_tag, enabled, has_passphrase, roles]
        assert_eq!(items.len(), 7);
        assert_eq!(items[0], CborValue::Text(id(1).to_string()));
        assert_eq!(items[1], CborValue::Text(root.handle.clone())); // handle
        assert_eq!(items[2], CborValue::Text(root.name.clone())); // name
        assert_eq!(
            items[3],
            CborValue::Uint(u64::from(PrincipalKind::Root.stable_tag()))
        );
        assert_eq!(items[4], CborValue::Bool(true)); // enabled
        assert_eq!(items[5], CborValue::Bool(false)); // has_passphrase
    }

    #[test]
    fn snapshot_round_trips_principal_handle() {
        let mut store = IdentityStore::new(id(1));
        store
            .add_principal_with_handle(id(2), "alice-handle", "Alice", PrincipalKind::User)
            .unwrap();
        let view =
            identity_snapshot_from_cbor(&identity_snapshot_to_cbor(&store).unwrap()).unwrap();
        // The decoded view carries every principal's handle (the field the wire form used to drop).
        let decoded: Vec<_> = view.principals.iter().collect();
        let expected: Vec<_> = store.principals().collect();
        assert_eq!(decoded.len(), expected.len());
        for (got, want) in decoded.iter().zip(expected.iter()) {
            assert_eq!(&got.id, &want.id);
            assert_eq!(&got.handle, &want.handle);
            assert_eq!(&got.name, &want.name);
            assert_eq!(got.kind, want.kind);
            assert_eq!(got.enabled, want.enabled);
            assert_eq!(got.has_passphrase, want.has_passphrase);
            assert_eq!(&got.roles, &want.roles);
        }
        let alice = view
            .principals
            .iter()
            .find(|p| p.id == id(2))
            .expect("alice present");
        assert_eq!(alice.handle, "alice-handle");
        assert_eq!(alice.name, "Alice");
    }

    #[test]
    fn snapshot_round_trips_authority_and_root() {
        let store = IdentityStore::new(id(1));
        let view =
            identity_snapshot_from_cbor(&identity_snapshot_to_cbor(&store).unwrap()).unwrap();
        assert_eq!(view.root, store.root_principal());
        assert_eq!(view.authority.authority, store.authority_state().authority);
        assert_eq!(view.authority.mode, store.authority_state().mode);
        assert_eq!(view.authenticated_mode, store.authenticated_mode());
    }

    #[test]
    fn role_record_shape() {
        let store = IdentityStore::new(id(1));
        let role = store.roles().next().unwrap();
        let CborValue::Array(items) =
            loom_codec::decode(&role_record_to_cbor(role).unwrap()).unwrap()
        else {
            panic!("expected array");
        };
        assert_eq!(items.len(), 3);
        assert!(matches!(items[0], CborValue::Text(_)));
        assert!(matches!(items[2], CborValue::Bool(_)));
    }

    #[test]
    fn external_credential_record_shape() {
        let mut store = IdentityStore::new(id(1));
        store
            .create_external_credential(
                id(1),
                ExternalCredentialSpec {
                    id: id(2),
                    kind: ExternalCredentialKind::MtlsCertificate,
                    label: "cert".to_string(),
                    issuer: "ca".to_string(),
                    subject: "cn".to_string(),
                    material_digest: None,
                },
            )
            .unwrap();
        let credential = store.external_credentials().next().unwrap();
        let CborValue::Array(items) =
            loom_codec::decode(&external_credential_record_to_cbor(credential).unwrap()).unwrap()
        else {
            panic!("expected array");
        };
        assert_eq!(items.len(), 8);
        assert_eq!(items[0], CborValue::Text(id(2).to_string()));
        assert_eq!(items[1], CborValue::Text(id(1).to_string()));
        assert_eq!(
            items[2],
            CborValue::Uint(u64::from(
                ExternalCredentialKind::MtlsCertificate.stable_tag()
            ))
        );
        assert_eq!(items[6], CborValue::Null); // material_digest
        assert_eq!(items[7], CborValue::Bool(true)); // enabled
    }

    #[test]
    fn full_snapshot_shape() {
        let mut store = IdentityStore::new(id(1));
        store
            .add_principal(id(2), "user", PrincipalKind::User)
            .unwrap();
        store
            .set_passphrase(id(2), "s3cret", b"salt-bytes")
            .unwrap();
        let CborValue::Array(items) =
            loom_codec::decode(&identity_snapshot_to_cbor(&store).unwrap()).unwrap()
        else {
            panic!("expected array");
        };
        assert_eq!(items.len(), 10);
        assert_eq!(items[0], CborValue::Bool(true)); // authenticated_mode (>1 principal)
        assert_eq!(items[1], CborValue::Text(id(1).to_string())); // root
        // authority record: [mode_tag, authority, generation, head]
        let CborValue::Array(authority) = &items[2] else {
            panic!("authority must be an array");
        };
        assert_eq!(authority.len(), 4);
        assert_eq!(authority[0], CborValue::Uint(0)); // Authority mode
        assert_eq!(authority[1], CborValue::Text(id(1).to_string()));
        // principals list has 2 entries.
        let CborValue::Array(principals) = &items[5] else {
            panic!("principals must be an array");
        };
        assert_eq!(principals.len(), 2);
    }
}
