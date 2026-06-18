//! Local principal registry and session authentication.

use std::collections::{BTreeMap, BTreeSet};

use sha1::{Digest as Sha1Digest, Sha1};
use zeroize::Zeroizing;

use crate::{
    Algo, Code, Digest, LoomError, Result, WorkspaceId,
    cbor::{self, Value},
};

const KEY_LEN: usize = 32;
const MYSQL_NATIVE_PASSWORD_HASH_LEN: usize = 20;
const ARGON2_M_KIB: u32 = 65_536;
const ARGON2_T: u32 = 3;
const ARGON2_P: u32 = 4;
pub const IDENTITY_AUTHORITY_HANDOFF_RECORD_TYPE: &str = "loom.identity.authority_handoff.v1";
pub const IDENTITY_AUTHORITY_HANDOFF_PAYLOAD_TYPE: &str =
    "loom.identity.authority_handoff.payload.v1";
pub const IDENTITY_PRINCIPAL_SIGNED_PAYLOAD_TYPE: &str =
    "loom.identity.principal_signed_payload.v1";
pub const IDENTITY_AUTHORITY_WITNESS_RECORD_TYPE: &str = "loom.identity.authority_witness.v1";
pub const IDENTITY_AUTHORITY_HANDOFF_ALG_ES256: &str = "ES256";
pub const IDENTITY_SIGNATURE_SUITE_ED25519: &str = "Ed25519";
pub const IDENTITY_MAX_PUBLIC_KEY_LEN: usize = 4096;

pub type PrincipalId = WorkspaceId;
pub type RoleId = WorkspaceId;

pub const ROLE_ADMIN_ID: RoleId = WorkspaceId::from_bytes([
    0x6c, 0x6f, 0x6f, 0x6d, 0, 0, 0x40, 0, 0x80, 0, 0, 0, 0, 0, 0, 1,
]);
pub const ROLE_READER_ID: RoleId = WorkspaceId::from_bytes([
    0x6c, 0x6f, 0x6f, 0x6d, 0, 0, 0x40, 0, 0x80, 0, 0, 0, 0, 0, 0, 2,
]);
pub const ROLE_WRITER_ID: RoleId = WorkspaceId::from_bytes([
    0x6c, 0x6f, 0x6f, 0x6d, 0, 0, 0x40, 0, 0x80, 0, 0, 0, 0, 0, 0, 3,
]);
pub const ROLE_OPERATOR_ID: RoleId = WorkspaceId::from_bytes([
    0x6c, 0x6f, 0x6f, 0x6d, 0, 0, 0x40, 0, 0x80, 0, 0, 0, 0, 0, 0, 4,
]);
pub const ROLE_SERVICE_ID: RoleId = WorkspaceId::from_bytes([
    0x6c, 0x6f, 0x6f, 0x6d, 0, 0, 0x40, 0, 0x80, 0, 0, 0, 0, 0, 0, 5,
]);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrincipalKind {
    Root,
    User,
    Service,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Principal {
    pub id: PrincipalId,
    pub handle: String,
    pub name: String,
    pub kind: PrincipalKind,
    pub enabled: bool,
    pub has_passphrase: bool,
    pub roles: BTreeSet<RoleId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppCredential {
    pub id: WorkspaceId,
    pub principal: PrincipalId,
    pub label: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ExternalCredentialKind {
    PublicKey,
    MtlsCertificate,
    Passkey,
    OidcSubject,
    SamlSubject,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalCredential {
    pub id: WorkspaceId,
    pub principal: PrincipalId,
    pub kind: ExternalCredentialKind,
    pub label: String,
    pub issuer: String,
    pub subject: String,
    pub material_digest: Option<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalCredentialChallenge {
    pub id: WorkspaceId,
    pub credential: WorkspaceId,
    pub nonce: Vec<u8>,
    pub issued_at_ms: u64,
    pub expires_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalCredentialSpec {
    pub id: WorkspaceId,
    pub kind: ExternalCredentialKind,
    pub label: String,
    pub issuer: String,
    pub subject: String,
    pub material_digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityPublicKey {
    pub id: WorkspaceId,
    pub principal: PrincipalId,
    pub label: String,
    pub algorithm: String,
    pub public_key: Vec<u8>,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityPublicKeySpec {
    pub id: WorkspaceId,
    pub label: String,
    pub algorithm: String,
    pub public_key: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerifiedExternalCredentialAuth<'a> {
    pub kind: ExternalCredentialKind,
    pub issuer: &'a str,
    pub subject: &'a str,
    pub material_digest: Option<&'a str>,
    pub challenge_id: Option<WorkspaceId>,
    pub now_ms: u64,
    pub session_id: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityRole {
    pub id: RoleId,
    pub name: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub id: String,
    pub principal: PrincipalId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentityAuthorityMode {
    Authority,
    Mirror,
    Detached,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityAuthorityState {
    pub mode: IdentityAuthorityMode,
    pub authority: PrincipalId,
    pub generation: u64,
    pub head: Option<Digest>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityAuthorityHandoff {
    pub from: PrincipalId,
    pub to: PrincipalId,
    pub generation: u64,
    pub head: Option<Digest>,
    pub signed_record: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityAuthorityDetach {
    pub previous_authority: PrincipalId,
    pub new_authority: PrincipalId,
    pub generation: u64,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityAuthorityWitness {
    pub authority: PrincipalId,
    pub mode: IdentityAuthorityMode,
    pub generation: u64,
    pub head: Option<Digest>,
    pub snapshot_digest: Digest,
    pub latest_handoff_digest: Option<Digest>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityAuthoritySyncReport {
    pub from_generation: u64,
    pub to_generation: u64,
    pub applied: bool,
    pub witness: IdentityAuthorityWitness,
}

impl IdentityAuthorityWitness {
    pub fn encode(&self) -> Vec<u8> {
        cbor::encode(&Value::Array(vec![
            Value::Text(IDENTITY_AUTHORITY_WITNESS_RECORD_TYPE.to_string()),
            Value::Bytes(self.authority.as_bytes().to_vec()),
            Value::Uint(u64::from(authority_mode_to_u8(self.mode))),
            Value::Uint(self.generation),
            optional_digest_value(self.head),
            digest_value(self.snapshot_digest),
            self.latest_handoff_digest.map_or(Value::Null, digest_value),
        ]))
    }

    pub fn digest(&self, algo: Algo) -> Digest {
        Digest::hash(algo, &self.encode())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PassphraseVerifier {
    salt: Vec<u8>,
    hash: [u8; KEY_LEN],
    mysql_native_password_hash: Option<[u8; MYSQL_NATIVE_PASSWORD_HASH_LEN]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AppCredentialVerifier {
    credential: AppCredential,
    salt: Vec<u8>,
    hash: [u8; KEY_LEN],
    mysql_native_password_hash: Option<[u8; MYSQL_NATIVE_PASSWORD_HASH_LEN]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExternalCredentialVerifier {
    credential: ExternalCredential,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExternalCredentialChallengeEntry {
    challenge: ExternalCredentialChallenge,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityStore {
    root: Option<PrincipalId>,
    authority: IdentityAuthorityState,
    handoffs: BTreeMap<u64, IdentityAuthorityHandoff>,
    detach: Option<IdentityAuthorityDetach>,
    principals: BTreeMap<PrincipalId, Principal>,
    handles: BTreeMap<String, PrincipalId>,
    roles: BTreeMap<RoleId, IdentityRole>,
    passphrases: BTreeMap<PrincipalId, PassphraseVerifier>,
    app_credentials: BTreeMap<WorkspaceId, AppCredentialVerifier>,
    external_credentials: BTreeMap<WorkspaceId, ExternalCredentialVerifier>,
    external_challenges: BTreeMap<WorkspaceId, ExternalCredentialChallengeEntry>,
    public_keys: BTreeMap<WorkspaceId, IdentityPublicKey>,
    sessions: BTreeMap<String, PrincipalId>,
}

impl IdentityStore {
    pub fn new(root: PrincipalId) -> Self {
        let roles = built_in_roles();
        let mut principal_roles = BTreeSet::new();
        principal_roles.insert(ROLE_ADMIN_ID);
        let principal = Principal {
            id: root,
            handle: "root".to_string(),
            name: "root".to_string(),
            kind: PrincipalKind::Root,
            enabled: true,
            has_passphrase: false,
            roles: principal_roles,
        };
        Self {
            root: Some(root),
            authority: IdentityAuthorityState {
                mode: IdentityAuthorityMode::Authority,
                authority: root,
                generation: 0,
                head: None,
            },
            handoffs: BTreeMap::new(),
            detach: None,
            principals: BTreeMap::from([(root, principal)]),
            handles: BTreeMap::from([("root".to_string(), root)]),
            roles,
            passphrases: BTreeMap::new(),
            app_credentials: BTreeMap::new(),
            external_credentials: BTreeMap::new(),
            external_challenges: BTreeMap::new(),
            public_keys: BTreeMap::new(),
            sessions: BTreeMap::new(),
        }
    }

    pub fn authenticated_mode(&self) -> bool {
        self.passphrases.values().next().is_some()
            || self.app_credentials.values().next().is_some()
            || self.external_credentials.values().next().is_some()
            || self.principals.len() > 1
    }

    pub fn root_principal(&self) -> Option<PrincipalId> {
        self.root
    }

    pub fn authority_state(&self) -> &IdentityAuthorityState {
        &self.authority
    }

    pub fn authority_handoffs(&self) -> impl Iterator<Item = &IdentityAuthorityHandoff> {
        self.handoffs.values()
    }

    pub fn forced_detach(&self) -> Option<&IdentityAuthorityDetach> {
        self.detach.as_ref()
    }

    pub fn authority_witness(&self, algo: Algo) -> IdentityAuthorityWitness {
        IdentityAuthorityWitness {
            authority: self.authority.authority,
            mode: self.authority.mode,
            generation: self.authority.generation,
            head: self.authority.head,
            snapshot_digest: Digest::hash(algo, &self.encode()),
            latest_handoff_digest: self
                .handoffs
                .values()
                .next_back()
                .map(|handoff| Digest::hash(algo, &handoff.signed_record)),
        }
    }

    pub fn public_keys(&self) -> impl Iterator<Item = &IdentityPublicKey> {
        self.public_keys.values()
    }

    pub fn replicate_authority_from(
        &mut self,
        source: &IdentityStore,
        algo: Algo,
        become_authority: bool,
    ) -> Result<IdentityAuthoritySyncReport> {
        let from_generation = self.authority.generation;
        let to_generation = source.authority.generation;
        if to_generation < from_generation {
            return Err(LoomError::new(
                Code::Conflict,
                "authority source generation is behind destination",
            ));
        }
        if to_generation == from_generation {
            let mut normalized_source = source.clone();
            normalized_source.authority.mode = self.authority.mode;
            let source_witness = normalized_source.authority_witness(algo);
            if self.authority_witness(algo).snapshot_digest != source_witness.snapshot_digest {
                return Err(LoomError::new(
                    Code::Conflict,
                    "authority snapshots diverge at the same generation",
                ));
            }
            return Ok(IdentityAuthoritySyncReport {
                from_generation,
                to_generation,
                applied: false,
                witness: source_witness,
            });
        }
        if self.detach.is_some() || source.authority.mode == IdentityAuthorityMode::Detached {
            return Err(LoomError::new(
                Code::Conflict,
                "detached authority state requires explicit reconciliation",
            ));
        }
        let mut expected_authority = self.authority.authority;
        let mut expected_generation = from_generation;
        let mut advanced = false;
        for handoff in source
            .handoffs
            .values()
            .filter(|handoff| handoff.generation > from_generation)
        {
            if handoff.from != expected_authority {
                return Err(LoomError::new(
                    Code::Conflict,
                    "authority handoff chain does not start from destination authority",
                ));
            }
            if handoff.generation <= expected_generation {
                return Err(LoomError::new(
                    Code::Conflict,
                    "authority handoff chain is not strictly increasing",
                ));
            }
            source.verify_authority_handoff_signature(handoff)?;
            expected_authority = handoff.to;
            expected_generation = handoff.generation;
            advanced = true;
        }
        if !advanced
            || expected_generation != to_generation
            || expected_authority != source.authority.authority
        {
            return Err(LoomError::new(
                Code::Conflict,
                "authority source is not a signed fast-forward of destination",
            ));
        }
        let mut replicated = source.clone();
        replicated.authority.mode = if become_authority {
            IdentityAuthorityMode::Authority
        } else {
            IdentityAuthorityMode::Mirror
        };
        let witness = replicated.authority_witness(algo);
        *self = replicated;
        Ok(IdentityAuthoritySyncReport {
            from_generation,
            to_generation,
            applied: true,
            witness,
        })
    }

    pub fn apply_authority_handoff(
        &mut self,
        handoff: IdentityAuthorityHandoff,
        become_authority: bool,
    ) -> Result<()> {
        self.apply_authority_handoff_inner(handoff, become_authority, true)
    }

    pub fn apply_verified_authority_handoff(
        &mut self,
        handoff: IdentityAuthorityHandoff,
        become_authority: bool,
    ) -> Result<()> {
        self.apply_authority_handoff(handoff, become_authority)
    }

    fn apply_authority_handoff_inner(
        &mut self,
        handoff: IdentityAuthorityHandoff,
        become_authority: bool,
        verify_signature: bool,
    ) -> Result<()> {
        if handoff.from != self.authority.authority {
            return Err(LoomError::new(
                Code::Conflict,
                "authority handoff does not start from current authority",
            ));
        }
        if handoff.generation <= self.authority.generation {
            return Err(LoomError::new(
                Code::Conflict,
                "authority handoff generation must advance",
            ));
        }
        if handoff.signed_record.is_empty() {
            return Err(LoomError::invalid(
                "authority handoff signature must not be empty",
            ));
        }
        validate_authority_handoff_record(&handoff).map_err(|_| {
            LoomError::invalid("authority handoff record must match the canonical handoff payload")
        })?;
        if verify_signature {
            self.verify_authority_handoff_signature(&handoff)?;
        }
        self.authority = IdentityAuthorityState {
            mode: if become_authority {
                IdentityAuthorityMode::Authority
            } else {
                IdentityAuthorityMode::Mirror
            },
            authority: handoff.to,
            generation: handoff.generation,
            head: handoff.head,
        };
        self.detach = None;
        self.handoffs.insert(handoff.generation, handoff);
        Ok(())
    }

    pub fn add_public_key(
        &mut self,
        principal: PrincipalId,
        spec: IdentityPublicKeySpec,
    ) -> Result<IdentityPublicKey> {
        if self.public_keys.contains_key(&spec.id) {
            return Err(LoomError::new(
                Code::AlreadyExists,
                "identity public key exists",
            ));
        }
        let p = self.principal(principal)?;
        if !p.enabled {
            return Err(LoomError::new(Code::PermissionDenied, "principal disabled"));
        }
        let label = validate_external_credential_text("identity public key label", spec.label)?;
        validate_public_key_algorithm(&spec.algorithm)?;
        validate_public_key_material(&spec.algorithm, &spec.public_key)?;
        let key = IdentityPublicKey {
            id: spec.id,
            principal,
            label,
            algorithm: spec.algorithm,
            public_key: spec.public_key,
            enabled: true,
        };
        self.public_keys.insert(key.id, key.clone());
        Ok(key)
    }

    pub fn revoke_public_key(&mut self, id: WorkspaceId) -> Result<IdentityPublicKey> {
        self.public_keys
            .remove(&id)
            .ok_or_else(|| LoomError::new(Code::NotFound, "identity public key not found"))
    }

    pub fn verify_authority_handoff_signature(
        &self,
        handoff: &IdentityAuthorityHandoff,
    ) -> Result<()> {
        let record = parse_authority_handoff_record(&handoff.signed_record).map_err(|_| {
            LoomError::invalid("authority handoff record must match the canonical handoff payload")
        })?;
        let expected = identity_authority_handoff_payload(
            handoff.from,
            handoff.to,
            handoff.generation,
            handoff.head,
        );
        if record.payload != expected {
            return Err(LoomError::invalid(
                "authority handoff record must match the canonical handoff payload",
            ));
        }
        let key_id = key_id_from_header(&record.key_id)?;
        let key = self
            .public_keys
            .get(&key_id)
            .ok_or_else(|| LoomError::new(Code::NotFound, "identity public key not found"))?;
        if !key.enabled {
            return Err(LoomError::new(
                Code::PermissionDenied,
                "identity public key disabled",
            ));
        }
        if key.principal != handoff.from {
            return Err(LoomError::new(
                Code::PermissionDenied,
                "authority handoff key does not belong to current authority",
            ));
        }
        if key.algorithm != record.algorithm {
            return Err(LoomError::invalid(
                "authority handoff algorithm does not match identity public key",
            ));
        }
        verify_identity_signature(
            &record.algorithm,
            &key.public_key,
            &record.payload,
            &record.signature,
        )
    }

    pub fn verify_principal_signature(
        &self,
        principal: PrincipalId,
        key_id: WorkspaceId,
        suite: &str,
        purpose: &str,
        payload: &[u8],
        signature: &[u8],
    ) -> Result<()> {
        validate_signature_suite(suite)?;
        let key = self
            .public_keys
            .get(&key_id)
            .ok_or_else(|| LoomError::new(Code::NotFound, "identity public key not found"))?;
        if !key.enabled {
            return Err(LoomError::new(
                Code::PermissionDenied,
                "identity public key disabled",
            ));
        }
        if key.principal != principal {
            return Err(LoomError::new(
                Code::PermissionDenied,
                "identity public key does not belong to principal",
            ));
        }
        if key.algorithm != suite {
            return Err(LoomError::invalid(
                "identity signature suite does not match public key",
            ));
        }
        let signed_payload =
            principal_signature_payload(principal, key_id, suite, purpose, payload)?;
        verify_identity_signature(suite, &key.public_key, &signed_payload, signature)
    }

    pub fn force_detach_authority(
        &mut self,
        new_authority: PrincipalId,
        generation: u64,
        reason: impl Into<String>,
    ) -> Result<IdentityAuthorityDetach> {
        if generation <= self.authority.generation {
            return Err(LoomError::new(
                Code::Conflict,
                "forced detach generation must advance",
            ));
        }
        let reason = reason.into();
        if reason.is_empty() {
            return Err(LoomError::invalid("forced detach reason must not be empty"));
        }
        let detach = IdentityAuthorityDetach {
            previous_authority: self.authority.authority,
            new_authority,
            generation,
            reason,
        };
        self.authority = IdentityAuthorityState {
            mode: IdentityAuthorityMode::Detached,
            authority: new_authority,
            generation,
            head: None,
        };
        self.detach = Some(detach.clone());
        Ok(detach)
    }

    pub fn principals(&self) -> impl Iterator<Item = &Principal> {
        self.principals.values()
    }

    pub fn roles(&self) -> impl Iterator<Item = &IdentityRole> {
        self.roles.values()
    }

    pub fn app_credentials(&self) -> impl Iterator<Item = &AppCredential> {
        self.app_credentials.values().map(|entry| &entry.credential)
    }

    pub fn external_credentials(&self) -> impl Iterator<Item = &ExternalCredential> {
        self.external_credentials
            .values()
            .map(|entry| &entry.credential)
    }

    pub fn external_challenges(&self) -> impl Iterator<Item = &ExternalCredentialChallenge> {
        self.external_challenges
            .values()
            .map(|entry| &entry.challenge)
    }

    pub fn principal(&self, id: PrincipalId) -> Result<&Principal> {
        self.principals
            .get(&id)
            .ok_or_else(|| LoomError::new(Code::NotFound, "principal not found"))
    }

    pub fn role(&self, id: RoleId) -> Result<&IdentityRole> {
        self.roles
            .get(&id)
            .ok_or_else(|| LoomError::new(Code::NotFound, "role not found"))
    }

    pub fn add_principal(
        &mut self,
        id: PrincipalId,
        name: impl Into<String>,
        kind: PrincipalKind,
    ) -> Result<()> {
        let name = name.into();
        let handle = principal_handle_from_display_name(&name)?;
        self.add_principal_with_handle(id, handle, name, kind)
    }

    pub fn add_principal_with_handle(
        &mut self,
        id: PrincipalId,
        handle: impl Into<String>,
        name: impl Into<String>,
        kind: PrincipalKind,
    ) -> Result<()> {
        if self.principals.contains_key(&id) {
            return Err(LoomError::new(Code::AlreadyExists, "principal exists"));
        }
        let handle = normalize_principal_handle(&handle.into())?;
        if self.handles.contains_key(&handle) {
            return Err(LoomError::new(
                Code::AlreadyExists,
                "principal handle is reserved",
            ));
        }
        let name = name.into();
        if name.is_empty() {
            return Err(LoomError::invalid("principal name must not be empty"));
        }
        self.principals.insert(
            id,
            Principal {
                id,
                handle: handle.clone(),
                name,
                kind,
                enabled: true,
                has_passphrase: false,
                roles: BTreeSet::new(),
            },
        );
        self.handles.insert(handle, id);
        Ok(())
    }

    pub fn resolve_handle(&self, handle: &str) -> Result<Option<PrincipalId>> {
        let handle = normalize_principal_handle(handle)?;
        Ok(self
            .handles
            .get(&handle)
            .copied()
            .filter(|principal| self.principals.contains_key(principal)))
    }

    pub fn rename_principal_handle(
        &mut self,
        principal: PrincipalId,
        handle: impl Into<String>,
    ) -> Result<()> {
        let handle = normalize_principal_handle(&handle.into())?;
        if let Some(existing) = self.handles.get(&handle)
            && *existing != principal
        {
            return Err(LoomError::new(
                Code::AlreadyExists,
                "principal handle is reserved",
            ));
        }
        let record = self
            .principals
            .get_mut(&principal)
            .ok_or_else(|| LoomError::new(Code::NotFound, "principal not found"))?;
        record.handle = handle.clone();
        self.handles.insert(handle, principal);
        Ok(())
    }

    pub fn assign_role(&mut self, principal: PrincipalId, role: RoleId) -> Result<()> {
        let r = self.role(role)?;
        if !r.enabled {
            return Err(LoomError::new(Code::PermissionDenied, "role disabled"));
        }
        let p = self
            .principals
            .get_mut(&principal)
            .ok_or_else(|| LoomError::new(Code::NotFound, "principal not found"))?;
        if !p.roles.insert(role) {
            return Err(LoomError::new(
                Code::AlreadyExists,
                "principal already has role",
            ));
        }
        Ok(())
    }

    pub fn revoke_role(&mut self, principal: PrincipalId, role: RoleId) -> Result<bool> {
        let removed = {
            let p = self
                .principals
                .get_mut(&principal)
                .ok_or_else(|| LoomError::new(Code::NotFound, "principal not found"))?;
            p.roles.remove(&role)
        };
        if removed && self.authenticated_mode() && !self.has_recovery_principal() {
            let p = self
                .principals
                .get_mut(&principal)
                .ok_or_else(|| LoomError::new(Code::NotFound, "principal not found"))?;
            p.roles.insert(role);
            return Err(LoomError::new(
                Code::IdentityNoRootCredential,
                "revoking role would leave no recovery identity",
            ));
        }
        Ok(removed)
    }

    pub fn effective_roles(&self, principal: PrincipalId) -> Result<BTreeSet<RoleId>> {
        let p = self.principal(principal)?;
        Ok(p.roles
            .iter()
            .copied()
            .filter(|role| self.roles.get(role).is_some_and(|r| r.enabled))
            .collect())
    }

    pub fn has_recovery_principal(&self) -> bool {
        self.has_recovery_principal_excluding(None)
    }

    pub fn set_passphrase(
        &mut self,
        principal: PrincipalId,
        passphrase: &str,
        salt: &[u8],
    ) -> Result<()> {
        if passphrase.is_empty() {
            return Err(LoomError::invalid("passphrase must not be empty"));
        }
        let p = self
            .principals
            .get_mut(&principal)
            .ok_or_else(|| LoomError::new(Code::NotFound, "principal not found"))?;
        if !p.enabled {
            return Err(LoomError::new(Code::PermissionDenied, "principal disabled"));
        }
        let verifier = PassphraseVerifier {
            salt: salt.to_vec(),
            hash: derive_passphrase_hash(passphrase.as_bytes(), salt)?,
            mysql_native_password_hash: Some(mysql_native_password_hash(passphrase.as_bytes())),
        };
        self.passphrases.insert(principal, verifier);
        p.has_passphrase = true;
        Ok(())
    }

    pub fn authenticate_passphrase(
        &mut self,
        principal: PrincipalId,
        passphrase: &str,
        session_id: impl Into<String>,
    ) -> Result<Session> {
        let session_id = session_id.into();
        if session_id.is_empty() {
            return Err(LoomError::invalid("session id must not be empty"));
        }
        let p = self.principal(principal)?;
        if !p.enabled {
            return Err(LoomError::new(
                Code::AuthenticationFailed,
                "principal disabled",
            ));
        }
        let verifier = self
            .passphrases
            .get(&principal)
            .ok_or_else(|| LoomError::new(Code::AuthenticationFailed, "passphrase not set"))?;
        if derive_passphrase_hash(passphrase.as_bytes(), &verifier.salt)? != verifier.hash {
            return Err(LoomError::new(Code::AuthenticationFailed, "bad passphrase"));
        }
        self.sessions.insert(session_id.clone(), principal);
        Ok(Session {
            id: session_id,
            principal,
        })
    }

    pub fn create_app_credential(
        &mut self,
        principal: PrincipalId,
        id: WorkspaceId,
        label: impl Into<String>,
        secret: &[u8],
        salt: &[u8],
    ) -> Result<AppCredential> {
        if secret.is_empty() {
            return Err(LoomError::invalid(
                "app credential secret must not be empty",
            ));
        }
        if self.app_credentials.contains_key(&id) {
            return Err(LoomError::new(Code::AlreadyExists, "app credential exists"));
        }
        let p = self.principal(principal)?;
        if !p.enabled {
            return Err(LoomError::new(Code::PermissionDenied, "principal disabled"));
        }
        let label = label.into();
        if label.is_empty() {
            return Err(LoomError::invalid("app credential label must not be empty"));
        }
        let credential = AppCredential {
            id,
            principal,
            label,
            enabled: true,
        };
        self.app_credentials.insert(
            id,
            AppCredentialVerifier {
                credential: credential.clone(),
                salt: salt.to_vec(),
                hash: derive_passphrase_hash(secret, salt)?,
                mysql_native_password_hash: Some(mysql_native_password_hash(
                    app_credential_token(id, secret).as_bytes(),
                )),
            },
        );
        Ok(credential)
    }

    pub fn revoke_app_credential(&mut self, id: WorkspaceId) -> Result<AppCredential> {
        self.app_credentials
            .remove(&id)
            .map(|entry| entry.credential)
            .ok_or_else(|| LoomError::new(Code::NotFound, "app credential not found"))
    }

    pub fn create_external_credential(
        &mut self,
        principal: PrincipalId,
        spec: ExternalCredentialSpec,
    ) -> Result<ExternalCredential> {
        let ExternalCredentialSpec {
            id,
            kind,
            label,
            issuer,
            subject,
            material_digest,
        } = spec;
        if self.external_credentials.contains_key(&id) {
            return Err(LoomError::new(
                Code::AlreadyExists,
                "external credential exists",
            ));
        }
        let p = self.principal(principal)?;
        if !p.enabled {
            return Err(LoomError::new(Code::PermissionDenied, "principal disabled"));
        }
        let label = validate_external_credential_text("external credential label", label)?;
        let issuer = validate_external_credential_text("external credential issuer", issuer)?;
        let subject = validate_external_credential_text("external credential subject", subject)?;
        let material_digest = material_digest
            .map(|value| {
                validate_external_credential_text("external credential material digest", value)
            })
            .transpose()?;
        if self.external_credentials.values().any(|entry| {
            let credential = &entry.credential;
            credential.enabled
                && credential.kind == kind
                && credential.issuer == issuer
                && credential.subject == subject
                && credential.material_digest == material_digest
        }) {
            return Err(LoomError::new(
                Code::AlreadyExists,
                "external credential proof already exists",
            ));
        }
        let credential = ExternalCredential {
            id,
            principal,
            kind,
            label,
            issuer,
            subject,
            material_digest,
            enabled: true,
        };
        self.external_credentials.insert(
            id,
            ExternalCredentialVerifier {
                credential: credential.clone(),
            },
        );
        Ok(credential)
    }

    pub fn revoke_external_credential(&mut self, id: WorkspaceId) -> Result<ExternalCredential> {
        let credential = self
            .external_credentials
            .remove(&id)
            .map(|entry| entry.credential)
            .ok_or_else(|| LoomError::new(Code::NotFound, "external credential not found"))?;
        self.external_challenges
            .retain(|_, entry| entry.challenge.credential != id);
        Ok(credential)
    }

    pub fn create_external_credential_challenge(
        &mut self,
        credential: WorkspaceId,
        id: WorkspaceId,
        nonce: Vec<u8>,
        issued_at_ms: u64,
        expires_at_ms: u64,
    ) -> Result<ExternalCredentialChallenge> {
        if self.external_challenges.contains_key(&id) {
            return Err(LoomError::new(
                Code::AlreadyExists,
                "external credential challenge exists",
            ));
        }
        if nonce.len() < 16 {
            return Err(LoomError::invalid(
                "external credential challenge nonce must be at least 16 bytes",
            ));
        }
        if nonce.len() > 1024 {
            return Err(LoomError::invalid(
                "external credential challenge nonce too long",
            ));
        }
        if expires_at_ms <= issued_at_ms {
            return Err(LoomError::invalid(
                "external credential challenge expiry must be after issue time",
            ));
        }
        let entry = self
            .external_credentials
            .get(&credential)
            .ok_or_else(|| LoomError::new(Code::NotFound, "external credential not found"))?;
        if !entry.credential.enabled {
            return Err(LoomError::new(
                Code::PermissionDenied,
                "external credential disabled",
            ));
        }
        let p = self.principal(entry.credential.principal)?;
        if !p.enabled {
            return Err(LoomError::new(Code::PermissionDenied, "principal disabled"));
        }
        let challenge = ExternalCredentialChallenge {
            id,
            credential,
            nonce,
            issued_at_ms,
            expires_at_ms,
        };
        self.external_challenges.insert(
            id,
            ExternalCredentialChallengeEntry {
                challenge: challenge.clone(),
            },
        );
        Ok(challenge)
    }

    pub fn consume_external_credential_challenge(
        &mut self,
        id: WorkspaceId,
        now_ms: u64,
    ) -> Result<ExternalCredentialChallenge> {
        let challenge = self
            .external_challenges
            .remove(&id)
            .map(|entry| entry.challenge)
            .ok_or_else(|| {
                LoomError::new(
                    Code::AuthenticationFailed,
                    "external credential challenge not found",
                )
            })?;
        if now_ms > challenge.expires_at_ms {
            return Err(LoomError::new(
                Code::AuthenticationFailed,
                "external credential challenge expired",
            ));
        }
        let entry = self
            .external_credentials
            .get(&challenge.credential)
            .ok_or_else(|| {
                LoomError::new(Code::AuthenticationFailed, "external credential not found")
            })?;
        if !entry.credential.enabled {
            return Err(LoomError::new(
                Code::AuthenticationFailed,
                "external credential disabled",
            ));
        }
        let p = self.principal(entry.credential.principal)?;
        if !p.enabled {
            return Err(LoomError::new(
                Code::AuthenticationFailed,
                "principal disabled",
            ));
        }
        Ok(challenge)
    }

    pub fn prune_external_credential_challenges(&mut self, now_ms: u64) -> usize {
        let before = self.external_challenges.len();
        self.external_challenges
            .retain(|_, entry| now_ms <= entry.challenge.expires_at_ms);
        before - self.external_challenges.len()
    }

    pub fn authenticate_verified_external_credential(
        &mut self,
        request: VerifiedExternalCredentialAuth<'_>,
    ) -> Result<Session> {
        let entry = self
            .external_credentials
            .values()
            .find(|entry| {
                let credential = &entry.credential;
                credential.enabled
                    && credential.kind == request.kind
                    && credential.issuer == request.issuer
                    && credential.subject == request.subject
                    && credential.material_digest.as_deref() == request.material_digest
            })
            .ok_or_else(|| {
                LoomError::new(Code::AuthenticationFailed, "external credential not found")
            })?;
        let credential_id = entry.credential.id;
        let principal = entry.credential.principal;
        let p = self.principal(principal)?;
        if !p.enabled {
            return Err(LoomError::new(
                Code::AuthenticationFailed,
                "principal disabled",
            ));
        }
        if let Some(challenge_id) = request.challenge_id {
            let challenge = self
                .external_challenges
                .get(&challenge_id)
                .map(|entry| &entry.challenge)
                .ok_or_else(|| {
                    LoomError::new(
                        Code::AuthenticationFailed,
                        "external credential challenge not found",
                    )
                })?;
            if challenge.credential != credential_id {
                return Err(LoomError::new(
                    Code::AuthenticationFailed,
                    "external credential challenge does not match credential",
                ));
            }
            let challenge =
                self.consume_external_credential_challenge(challenge_id, request.now_ms)?;
            debug_assert_eq!(challenge.credential, credential_id);
        }
        self.bind_session(principal, request.session_id)
    }

    pub fn authenticate_app_credential(
        &mut self,
        token: &str,
        session_id: impl Into<String>,
    ) -> Result<Session> {
        let (credential_id, secret) = parse_app_credential_token(token)?;
        let entry = self.app_credentials.get(&credential_id).ok_or_else(|| {
            LoomError::new(Code::AuthenticationFailed, "app credential not found")
        })?;
        if !entry.credential.enabled {
            return Err(LoomError::new(
                Code::AuthenticationFailed,
                "app credential disabled",
            ));
        }
        let principal = entry.credential.principal;
        let p = self.principal(principal)?;
        if !p.enabled {
            return Err(LoomError::new(
                Code::AuthenticationFailed,
                "principal disabled",
            ));
        }
        if derive_passphrase_hash(&secret, &entry.salt)? != entry.hash {
            return Err(LoomError::new(
                Code::AuthenticationFailed,
                "bad app credential",
            ));
        }
        self.bind_session(principal, session_id)
    }

    pub fn authenticate_mysql_native_password(
        &mut self,
        principal: PrincipalId,
        scramble: &[u8],
        challenge: &[u8],
        session_id: impl Into<String>,
    ) -> Result<Session> {
        let p = self.principal(principal)?;
        if !p.enabled {
            return Err(LoomError::new(
                Code::AuthenticationFailed,
                "principal disabled",
            ));
        }
        if scramble.len() != MYSQL_NATIVE_PASSWORD_HASH_LEN {
            return Err(LoomError::new(
                Code::AuthenticationFailed,
                "bad mysql_native_password scramble",
            ));
        }
        let passphrase_verified = self
            .passphrases
            .get(&principal)
            .and_then(|entry| entry.mysql_native_password_hash)
            .is_some_and(|hash| mysql_native_password_verify(&hash, challenge, scramble));
        let app_credential_verified = self.app_credentials.values().any(|entry| {
            entry.credential.enabled
                && entry.credential.principal == principal
                && entry
                    .mysql_native_password_hash
                    .is_some_and(|hash| mysql_native_password_verify(&hash, challenge, scramble))
        });
        let verified = passphrase_verified || app_credential_verified;
        if !verified {
            return Err(LoomError::new(
                Code::AuthenticationFailed,
                "bad mysql_native_password",
            ));
        }
        self.bind_session(principal, session_id)
    }

    pub fn bind_session(
        &mut self,
        principal: PrincipalId,
        session_id: impl Into<String>,
    ) -> Result<Session> {
        let session_id = session_id.into();
        if session_id.is_empty() {
            return Err(LoomError::invalid("session id must not be empty"));
        }
        let p = self.principal(principal)?;
        if !p.enabled {
            return Err(LoomError::new(
                Code::AuthenticationFailed,
                "principal disabled",
            ));
        }
        self.sessions.insert(session_id.clone(), principal);
        Ok(Session {
            id: session_id,
            principal,
        })
    }

    pub fn session_principal(&self, session_id: &str) -> Result<PrincipalId> {
        self.sessions
            .get(session_id)
            .copied()
            .ok_or_else(|| LoomError::new(Code::AuthenticationFailed, "session not found"))
    }

    pub fn effective_principal(&self, session_id: Option<&str>) -> Result<PrincipalId> {
        match session_id {
            Some(id) => self.session_principal(id),
            None if !self.authenticated_mode() => self.root.ok_or_else(|| {
                LoomError::new(Code::AuthenticationFailed, "root principal removed")
            }),
            None => Err(LoomError::new(
                Code::AuthenticationFailed,
                "authentication required",
            )),
        }
    }

    pub fn remove_principal(&mut self, principal: PrincipalId) -> Result<Principal> {
        let removed = self
            .principals
            .get(&principal)
            .cloned()
            .ok_or_else(|| LoomError::new(Code::NotFound, "principal not found"))?;
        if self.authenticated_mode() && !self.has_recovery_principal_excluding(Some(principal)) {
            return Err(LoomError::new(
                Code::IdentityNoRootCredential,
                "removing principal would leave no recovery identity",
            ));
        }
        self.principals.remove(&principal);
        self.passphrases.remove(&principal);
        self.app_credentials
            .retain(|_, entry| entry.credential.principal != principal);
        self.external_credentials
            .retain(|_, entry| entry.credential.principal != principal);
        self.public_keys.retain(|_, key| key.principal != principal);
        let valid_credentials: BTreeSet<_> = self.external_credentials.keys().copied().collect();
        self.external_challenges
            .retain(|_, entry| valid_credentials.contains(&entry.challenge.credential));
        self.sessions.retain(|_, p| *p != principal);
        if self.root == Some(principal) {
            self.root = None;
        }
        Ok(removed)
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"LID9");
        match self.root {
            None => out.push(0),
            Some(root) => {
                out.push(1);
                out.extend_from_slice(root.as_bytes());
            }
        }
        out.push(authority_mode_to_u8(self.authority.mode));
        out.extend_from_slice(self.authority.authority.as_bytes());
        put_uvarint(&mut out, self.authority.generation);
        put_optional_digest(&mut out, self.authority.head);
        put_uvarint(&mut out, self.handoffs.len() as u64);
        for handoff in self.handoffs.values() {
            out.extend_from_slice(handoff.from.as_bytes());
            out.extend_from_slice(handoff.to.as_bytes());
            put_uvarint(&mut out, handoff.generation);
            put_optional_digest(&mut out, handoff.head);
            put_lp(&mut out, &handoff.signed_record);
        }
        match &self.detach {
            None => out.push(0),
            Some(detach) => {
                out.push(1);
                out.extend_from_slice(detach.previous_authority.as_bytes());
                out.extend_from_slice(detach.new_authority.as_bytes());
                put_uvarint(&mut out, detach.generation);
                put_lp(&mut out, detach.reason.as_bytes());
            }
        }
        put_uvarint(&mut out, self.roles.len() as u64);
        for role in self.roles.values() {
            out.extend_from_slice(role.id.as_bytes());
            put_lp(&mut out, role.name.as_bytes());
            out.push(u8::from(role.enabled));
        }
        put_uvarint(&mut out, self.principals.len() as u64);
        for principal in self.principals.values() {
            out.extend_from_slice(principal.id.as_bytes());
            put_lp(&mut out, principal.handle.as_bytes());
            put_lp(&mut out, principal.name.as_bytes());
            out.push(principal_kind_to_u8(principal.kind));
            out.push(u8::from(principal.enabled));
            out.push(u8::from(principal.has_passphrase));
            put_uvarint(&mut out, principal.roles.len() as u64);
            for role in &principal.roles {
                out.extend_from_slice(role.as_bytes());
            }
        }
        put_uvarint(&mut out, self.passphrases.len() as u64);
        for (principal, verifier) in &self.passphrases {
            out.extend_from_slice(principal.as_bytes());
            put_lp(&mut out, &verifier.salt);
            out.extend_from_slice(&verifier.hash);
            match verifier.mysql_native_password_hash {
                Some(hash) => {
                    out.push(1);
                    out.extend_from_slice(&hash);
                }
                None => out.push(0),
            }
        }
        put_uvarint(&mut out, self.app_credentials.len() as u64);
        for (id, verifier) in &self.app_credentials {
            out.extend_from_slice(id.as_bytes());
            out.extend_from_slice(verifier.credential.principal.as_bytes());
            put_lp(&mut out, verifier.credential.label.as_bytes());
            out.push(u8::from(verifier.credential.enabled));
            put_lp(&mut out, &verifier.salt);
            out.extend_from_slice(&verifier.hash);
            match verifier.mysql_native_password_hash {
                Some(hash) => {
                    out.push(1);
                    out.extend_from_slice(&hash);
                }
                None => out.push(0),
            }
        }
        put_uvarint(&mut out, self.external_credentials.len() as u64);
        for (id, verifier) in &self.external_credentials {
            let credential = &verifier.credential;
            out.extend_from_slice(id.as_bytes());
            out.extend_from_slice(credential.principal.as_bytes());
            out.push(external_credential_kind_to_u8(credential.kind));
            put_lp(&mut out, credential.label.as_bytes());
            put_lp(&mut out, credential.issuer.as_bytes());
            put_lp(&mut out, credential.subject.as_bytes());
            match credential.material_digest.as_deref() {
                None => out.push(0),
                Some(digest) => {
                    out.push(1);
                    put_lp(&mut out, digest.as_bytes());
                }
            }
            out.push(u8::from(credential.enabled));
        }
        put_uvarint(&mut out, self.external_challenges.len() as u64);
        for (id, entry) in &self.external_challenges {
            let challenge = &entry.challenge;
            out.extend_from_slice(id.as_bytes());
            out.extend_from_slice(challenge.credential.as_bytes());
            put_lp(&mut out, &challenge.nonce);
            put_uvarint(&mut out, challenge.issued_at_ms);
            put_uvarint(&mut out, challenge.expires_at_ms);
        }
        put_uvarint(&mut out, self.public_keys.len() as u64);
        for key in self.public_keys.values() {
            out.extend_from_slice(key.id.as_bytes());
            out.extend_from_slice(key.principal.as_bytes());
            put_lp(&mut out, key.label.as_bytes());
            put_lp(&mut out, key.algorithm.as_bytes());
            put_lp(&mut out, &key.public_key);
            out.push(u8::from(key.enabled));
        }
        put_uvarint(&mut out, self.handles.len() as u64);
        for (handle, principal) in &self.handles {
            put_lp(&mut out, handle.as_bytes());
            out.extend_from_slice(principal.as_bytes());
        }
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let mut cur = Cur { bytes, pos: 0 };
        let magic = cur.take(4)?;
        if magic != b"LID8" && magic != b"LID9" {
            return Err(LoomError::corrupt("unsupported identity-store format"));
        }
        let mysql_native_password_verifiers = magic == b"LID9";
        let root = match cur.u8()? {
            0 => None,
            1 => Some(PrincipalId::from_bytes(cur.take16()?)),
            other => {
                return Err(LoomError::corrupt(format!(
                    "invalid identity root tag {other:#x}"
                )));
            }
        };
        let (authority, handoffs, detach) = {
            let mode = authority_mode_from_u8(cur.u8()?)?;
            let authority = PrincipalId::from_bytes(cur.take16()?);
            let generation = cur.uvarint()?;
            let head = cur.optional_digest()?;
            let handoff_count = cur.uvarint()?;
            let mut handoffs = BTreeMap::new();
            for _ in 0..handoff_count {
                let from = PrincipalId::from_bytes(cur.take16()?);
                let to = PrincipalId::from_bytes(cur.take16()?);
                let generation = cur.uvarint()?;
                let head = cur.optional_digest()?;
                let signed_record = cur.lp_bytes()?.to_vec();
                if signed_record.is_empty() {
                    return Err(LoomError::corrupt("authority handoff signature empty"));
                }
                let handoff = IdentityAuthorityHandoff {
                    from,
                    to,
                    generation,
                    head,
                    signed_record,
                };
                validate_authority_handoff_record(&handoff).map_err(|_| {
                    LoomError::corrupt("authority handoff record does not match payload")
                })?;
                handoffs.insert(generation, handoff);
            }
            let detach = match cur.u8()? {
                0 => None,
                1 => Some(IdentityAuthorityDetach {
                    previous_authority: PrincipalId::from_bytes(cur.take16()?),
                    new_authority: PrincipalId::from_bytes(cur.take16()?),
                    generation: cur.uvarint()?,
                    reason: cur.lp_str()?,
                }),
                other => {
                    return Err(LoomError::corrupt(format!(
                        "invalid authority detach tag {other:#x}"
                    )));
                }
            };
            (
                IdentityAuthorityState {
                    mode,
                    authority,
                    generation,
                    head,
                },
                handoffs,
                detach,
            )
        };
        let role_count = cur.uvarint()?;
        let mut roles = BTreeMap::new();
        for _ in 0..role_count {
            let id = RoleId::from_bytes(cur.take16()?);
            let name = cur.lp_str()?;
            let enabled = cur.bool()?;
            roles.insert(id, IdentityRole { id, name, enabled });
        }
        ensure_builtin_roles(&mut roles);
        let principal_count = cur.uvarint()?;
        let mut principals = BTreeMap::new();
        for _ in 0..principal_count {
            let id = PrincipalId::from_bytes(cur.take16()?);
            let handle = normalize_principal_handle(&cur.lp_str()?)?;
            let name = cur.lp_str()?;
            let kind = principal_kind_from_u8(cur.u8()?)?;
            let enabled = cur.bool()?;
            let has_passphrase = cur.bool()?;
            let mut principal_roles = BTreeSet::new();
            let role_count = cur.uvarint()?;
            for _ in 0..role_count {
                let role = RoleId::from_bytes(cur.take16()?);
                if !roles.contains_key(&role) {
                    return Err(LoomError::corrupt("principal references missing role"));
                }
                principal_roles.insert(role);
            }
            principals.insert(
                id,
                Principal {
                    id,
                    handle,
                    name,
                    kind,
                    enabled,
                    has_passphrase,
                    roles: principal_roles,
                },
            );
        }
        let verifier_count = cur.uvarint()?;
        let mut passphrases = BTreeMap::new();
        for _ in 0..verifier_count {
            let principal = PrincipalId::from_bytes(cur.take16()?);
            let salt = cur.lp_bytes()?.to_vec();
            let hash = cur.take32()?;
            let mysql_native_password_hash = if mysql_native_password_verifiers {
                match cur.u8()? {
                    0 => None,
                    1 => Some(cur.take20()?),
                    other => {
                        return Err(LoomError::corrupt(format!(
                            "invalid passphrase mysql verifier tag {other:#x}"
                        )));
                    }
                }
            } else {
                None
            };
            if !principals.contains_key(&principal) {
                return Err(LoomError::corrupt("identity verifier without principal"));
            }
            passphrases.insert(
                principal,
                PassphraseVerifier {
                    salt,
                    hash,
                    mysql_native_password_hash,
                },
            );
        }
        let mut app_credentials = BTreeMap::new();
        let credential_count = cur.uvarint()?;
        for _ in 0..credential_count {
            let id = WorkspaceId::from_bytes(cur.take16()?);
            let principal = PrincipalId::from_bytes(cur.take16()?);
            let label = cur.lp_str()?;
            let enabled = cur.bool()?;
            let salt = cur.lp_bytes()?.to_vec();
            let hash = cur.take32()?;
            let mysql_native_password_hash = if mysql_native_password_verifiers {
                match cur.u8()? {
                    0 => None,
                    1 => Some(cur.take20()?),
                    other => {
                        return Err(LoomError::corrupt(format!(
                            "invalid app credential mysql verifier tag {other:#x}"
                        )));
                    }
                }
            } else {
                None
            };
            if !principals.contains_key(&principal) {
                return Err(LoomError::corrupt(
                    "app credential references missing principal",
                ));
            }
            app_credentials.insert(
                id,
                AppCredentialVerifier {
                    credential: AppCredential {
                        id,
                        principal,
                        label,
                        enabled,
                    },
                    salt,
                    hash,
                    mysql_native_password_hash,
                },
            );
        }
        let mut external_credentials = BTreeMap::new();
        let credential_count = cur.uvarint()?;
        for _ in 0..credential_count {
            let id = WorkspaceId::from_bytes(cur.take16()?);
            let principal = PrincipalId::from_bytes(cur.take16()?);
            let kind = external_credential_kind_from_u8(cur.u8()?)?;
            let label = cur.lp_str()?;
            let issuer = cur.lp_str()?;
            let subject = cur.lp_str()?;
            let material_digest = match cur.u8()? {
                0 => None,
                1 => Some(cur.lp_str()?),
                other => {
                    return Err(LoomError::corrupt(format!(
                        "invalid external credential digest tag {other:#x}"
                    )));
                }
            };
            let enabled = cur.bool()?;
            if !principals.contains_key(&principal) {
                return Err(LoomError::corrupt(
                    "external credential references missing principal",
                ));
            }
            external_credentials.insert(
                id,
                ExternalCredentialVerifier {
                    credential: ExternalCredential {
                        id,
                        principal,
                        kind,
                        label,
                        issuer,
                        subject,
                        material_digest,
                        enabled,
                    },
                },
            );
        }
        let mut external_challenges = BTreeMap::new();
        let challenge_count = cur.uvarint()?;
        for _ in 0..challenge_count {
            let id = WorkspaceId::from_bytes(cur.take16()?);
            let credential = WorkspaceId::from_bytes(cur.take16()?);
            let nonce = cur.lp_bytes()?.to_vec();
            let issued_at_ms = cur.uvarint()?;
            let expires_at_ms = cur.uvarint()?;
            if !external_credentials.contains_key(&credential) {
                return Err(LoomError::corrupt(
                    "external challenge references missing credential",
                ));
            }
            external_challenges.insert(
                id,
                ExternalCredentialChallengeEntry {
                    challenge: ExternalCredentialChallenge {
                        id,
                        credential,
                        nonce,
                        issued_at_ms,
                        expires_at_ms,
                    },
                },
            );
        }
        let mut public_keys = BTreeMap::new();
        let key_count = cur.uvarint()?;
        for _ in 0..key_count {
            let id = WorkspaceId::from_bytes(cur.take16()?);
            let principal = PrincipalId::from_bytes(cur.take16()?);
            let label = cur.lp_str()?;
            let algorithm = cur.lp_str()?;
            let public_key = cur.lp_bytes()?.to_vec();
            let enabled = cur.bool()?;
            if !principals.contains_key(&principal) {
                return Err(LoomError::corrupt(
                    "identity public key references missing principal",
                ));
            }
            validate_public_key_algorithm(&algorithm)
                .map_err(|_| LoomError::corrupt("identity public key algorithm invalid"))?;
            validate_public_key_material(&algorithm, &public_key)
                .map_err(|_| LoomError::corrupt("identity public key material invalid"))?;
            public_keys.insert(
                id,
                IdentityPublicKey {
                    id,
                    principal,
                    label,
                    algorithm,
                    public_key,
                    enabled,
                },
            );
        }
        let handle_count = cur.uvarint()?;
        let mut handles = BTreeMap::new();
        for _ in 0..handle_count {
            let handle = normalize_principal_handle(&cur.lp_str()?)?;
            let principal = PrincipalId::from_bytes(cur.take16()?);
            if !principals.contains_key(&principal) || handles.insert(handle, principal).is_some() {
                return Err(LoomError::corrupt("invalid principal handle registry"));
            }
        }
        if cur.pos != bytes.len() {
            return Err(LoomError::corrupt("trailing identity-store bytes"));
        }
        for (id, principal) in &principals {
            if principal.has_passphrase != passphrases.contains_key(id) {
                return Err(LoomError::corrupt(
                    "identity passphrase flag does not match verifier set",
                ));
            }
        }
        if let Some(root) = root
            && !principals.contains_key(&root)
        {
            return Err(LoomError::corrupt("identity root principal missing"));
        }
        if !principals.contains_key(&authority.authority) {
            return Err(LoomError::corrupt("identity authority principal missing"));
        }
        if let Some(detach) = &detach
            && detach.new_authority != authority.authority
        {
            return Err(LoomError::corrupt(
                "identity detach authority does not match current authority",
            ));
        }
        Ok(Self {
            root,
            authority,
            handoffs,
            detach,
            principals,
            handles,
            roles,
            passphrases,
            app_credentials,
            external_credentials,
            external_challenges,
            public_keys,
            sessions: BTreeMap::new(),
        })
    }

    fn has_recovery_principal_excluding(&self, excluded: Option<PrincipalId>) -> bool {
        self.principals.iter().any(|(id, p)| {
            let has_app_credential = self
                .app_credentials
                .values()
                .any(|entry| entry.credential.principal == *id && entry.credential.enabled);
            let has_external_credential = self
                .external_credentials
                .values()
                .any(|entry| entry.credential.principal == *id && entry.credential.enabled);
            Some(*id) != excluded
                && p.enabled
                && p.roles.contains(&ROLE_ADMIN_ID)
                && (self.passphrases.contains_key(id)
                    || has_app_credential
                    || has_external_credential)
        })
    }
}

pub fn identity_authority_handoff_payload(
    from: PrincipalId,
    to: PrincipalId,
    generation: u64,
    head: Option<Digest>,
) -> Vec<u8> {
    cbor::encode(&Value::Array(vec![
        Value::Text(IDENTITY_AUTHORITY_HANDOFF_PAYLOAD_TYPE.to_string()),
        Value::Bytes(from.as_bytes().to_vec()),
        Value::Bytes(to.as_bytes().to_vec()),
        Value::Uint(generation),
        head.map_or(Value::Null, |digest| Value::Bytes(digest.bytes().to_vec())),
    ]))
}

pub fn principal_signature_payload(
    principal: PrincipalId,
    key_id: WorkspaceId,
    suite: &str,
    purpose: &str,
    payload: &[u8],
) -> Result<Vec<u8>> {
    validate_signature_suite(suite)?;
    if purpose.is_empty() {
        return Err(LoomError::invalid(
            "principal signature purpose must not be empty",
        ));
    }
    Ok(cbor::encode(&Value::Array(vec![
        Value::Text(IDENTITY_PRINCIPAL_SIGNED_PAYLOAD_TYPE.to_string()),
        Value::Text(suite.to_string()),
        Value::Text(purpose.to_string()),
        Value::Bytes(principal.as_bytes().to_vec()),
        Value::Bytes(key_id.as_bytes().to_vec()),
        Value::Bytes(payload.to_vec()),
    ])))
}

pub fn identity_authority_handoff_record(
    from: PrincipalId,
    to: PrincipalId,
    generation: u64,
    head: Option<Digest>,
    algorithm: &str,
    key_id: &[u8],
    signature: &[u8],
) -> Result<Vec<u8>> {
    if algorithm.is_empty() {
        return Err(LoomError::invalid(
            "authority handoff algorithm must not be empty",
        ));
    }
    if key_id.is_empty() {
        return Err(LoomError::invalid(
            "authority handoff key id must not be empty",
        ));
    }
    if signature.is_empty() {
        return Err(LoomError::invalid(
            "authority handoff signature must not be empty",
        ));
    }
    let payload = identity_authority_handoff_payload(from, to, generation, head);
    Ok(cbor::encode(&Value::Array(vec![
        Value::Text(IDENTITY_AUTHORITY_HANDOFF_RECORD_TYPE.to_string()),
        Value::Map(vec![
            (
                Value::Text("alg".to_string()),
                Value::Text(algorithm.to_string()),
            ),
            (
                Value::Text("kid".to_string()),
                Value::Bytes(key_id.to_vec()),
            ),
        ]),
        Value::Bytes(payload),
        Value::Bytes(signature.to_vec()),
    ])))
}

fn validate_authority_handoff_record(handoff: &IdentityAuthorityHandoff) -> Result<()> {
    let parsed = parse_authority_handoff_record(&handoff.signed_record)?;
    let expected = identity_authority_handoff_payload(
        handoff.from,
        handoff.to,
        handoff.generation,
        handoff.head,
    );
    if parsed.payload != expected {
        return Err(LoomError::corrupt("authority handoff payload mismatch"));
    }
    Ok(())
}

struct AuthorityHandoffRecord {
    algorithm: String,
    key_id: Vec<u8>,
    payload: Vec<u8>,
    signature: Vec<u8>,
}

fn parse_authority_handoff_record(bytes: &[u8]) -> Result<AuthorityHandoffRecord> {
    let mut fields = cbor::Fields::new(cbor::decode_array(bytes)?);
    let record_type = fields.text()?;
    if record_type != IDENTITY_AUTHORITY_HANDOFF_RECORD_TYPE {
        return Err(LoomError::corrupt("invalid authority handoff record type"));
    }
    let protected = fields.map()?;
    let payload = fields.bytes()?;
    let signature = fields.bytes()?;
    fields.end()?;
    let mut algorithm = None;
    let mut key_id = None;
    for (key, value) in protected {
        match cbor::as_text(key)?.as_str() {
            "alg" => algorithm = Some(cbor::as_text(value)?),
            "kid" => key_id = Some(cbor::as_bytes(value)?),
            _ => {
                return Err(LoomError::corrupt(
                    "unknown authority handoff protected header",
                ));
            }
        }
    }
    if algorithm.as_deref().is_none_or(str::is_empty) {
        return Err(LoomError::corrupt("authority handoff algorithm missing"));
    }
    if key_id.as_deref().is_none_or(<[u8]>::is_empty) {
        return Err(LoomError::corrupt("authority handoff key id missing"));
    }
    if payload.is_empty() {
        return Err(LoomError::corrupt("authority handoff payload missing"));
    }
    if signature.is_empty() {
        return Err(LoomError::corrupt("authority handoff signature missing"));
    }
    Ok(AuthorityHandoffRecord {
        algorithm: algorithm.unwrap(),
        key_id: key_id.unwrap(),
        payload,
        signature,
    })
}

pub fn app_credential_token(id: WorkspaceId, secret: &[u8]) -> String {
    format!("loom_app_{}_{}", id, hex::encode(secret))
}

pub fn mysql_native_password_hash(password: &[u8]) -> [u8; MYSQL_NATIVE_PASSWORD_HASH_LEN] {
    let stage1 = Sha1::digest(password);
    Sha1::digest(stage1).into()
}

fn mysql_native_password_verify(
    stored_hash: &[u8; MYSQL_NATIVE_PASSWORD_HASH_LEN],
    challenge: &[u8],
    scramble: &[u8],
) -> bool {
    if scramble.len() != MYSQL_NATIVE_PASSWORD_HASH_LEN {
        return false;
    }
    let mut hasher = Sha1::new();
    hasher.update(challenge);
    hasher.update(stored_hash);
    let challenge_hash = hasher.finalize();
    let mut stage1 = [0u8; MYSQL_NATIVE_PASSWORD_HASH_LEN];
    for (idx, out) in stage1.iter_mut().enumerate() {
        *out = scramble[idx] ^ challenge_hash[idx];
    }
    let candidate = Sha1::digest(stage1);
    candidate.as_slice() == stored_hash
}

impl ExternalCredentialKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PublicKey => "public_key",
            Self::MtlsCertificate => "mtls_certificate",
            Self::Passkey => "passkey",
            Self::OidcSubject => "oidc_subject",
            Self::SamlSubject => "saml_subject",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "public_key" | "public-key" | "publickey" => Ok(Self::PublicKey),
            "mtls_certificate" | "mtls-certificate" | "mtls" | "certificate" => {
                Ok(Self::MtlsCertificate)
            }
            "passkey" | "webauthn" => Ok(Self::Passkey),
            "oidc_subject" | "oidc-subject" | "oidc" => Ok(Self::OidcSubject),
            "saml_subject" | "saml-subject" | "saml" => Ok(Self::SamlSubject),
            other => Err(LoomError::invalid(format!(
                "unknown external credential kind {other:?}"
            ))),
        }
    }

    /// The stable one-byte tag for this external-credential kind (`PublicKey=0`, `MtlsCertificate=1`,
    /// `Passkey=2`, `OidcSubject=3`, `SamlSubject=4`), the shared numeric contract used by both durable
    /// encoding and the API/wire codecs.
    pub const fn stable_tag(self) -> u8 {
        match self {
            Self::PublicKey => 0,
            Self::MtlsCertificate => 1,
            Self::Passkey => 2,
            Self::OidcSubject => 3,
            Self::SamlSubject => 4,
        }
    }

    /// The external-credential kind for a stable tag, or `None` for an unknown tag. Callers choose the
    /// error code (durable decode reports `CorruptObject`; wire decode reports `InvalidArgument`).
    pub const fn from_stable_tag(tag: u8) -> Option<Self> {
        Some(match tag {
            0 => Self::PublicKey,
            1 => Self::MtlsCertificate,
            2 => Self::Passkey,
            3 => Self::OidcSubject,
            4 => Self::SamlSubject,
            _ => return None,
        })
    }
}

fn parse_app_credential_token(token: &str) -> Result<(WorkspaceId, Vec<u8>)> {
    let Some(rest) = token.strip_prefix("loom_app_") else {
        return Err(LoomError::new(
            Code::AuthenticationFailed,
            "bad app credential prefix",
        ));
    };
    let Some((id, secret_hex)) = rest.rsplit_once('_') else {
        return Err(LoomError::new(
            Code::AuthenticationFailed,
            "bad app credential format",
        ));
    };
    let id = WorkspaceId::parse(id)
        .map_err(|_| LoomError::new(Code::AuthenticationFailed, "bad app credential identifier"))?;
    let secret = hex::decode(secret_hex)
        .map_err(|_| LoomError::new(Code::AuthenticationFailed, "bad app credential secret"))?;
    Ok((id, secret))
}

fn built_in_roles() -> BTreeMap<RoleId, IdentityRole> {
    [
        (ROLE_ADMIN_ID, "admin"),
        (ROLE_READER_ID, "reader"),
        (ROLE_WRITER_ID, "writer"),
        (ROLE_OPERATOR_ID, "operator"),
        (ROLE_SERVICE_ID, "service"),
    ]
    .into_iter()
    .map(|(id, name)| {
        (
            id,
            IdentityRole {
                id,
                name: name.to_string(),
                enabled: true,
            },
        )
    })
    .collect()
}

fn ensure_builtin_roles(roles: &mut BTreeMap<RoleId, IdentityRole>) {
    for (id, role) in built_in_roles() {
        roles.entry(id).or_insert(role);
    }
}

fn derive_passphrase_hash(passphrase: &[u8], salt: &[u8]) -> Result<[u8; KEY_LEN]> {
    use argon2::{Algorithm, Argon2, Params, Version};

    if salt.len() < 8 {
        return Err(LoomError::invalid(
            "passphrase salt must be at least 8 bytes",
        ));
    }
    let params = Params::new(ARGON2_M_KIB, ARGON2_T, ARGON2_P, Some(KEY_LEN))
        .map_err(|e| LoomError::new(Code::Internal, format!("argon2 params: {e}")))?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut out = Zeroizing::new([0u8; KEY_LEN]);
    argon2
        .hash_password_into(passphrase, salt, out.as_mut_slice())
        .map_err(|e| LoomError::new(Code::AuthenticationFailed, format!("argon2 kdf: {e}")))?;
    Ok(*out)
}

impl PrincipalKind {
    /// The stable one-byte tag for this principal kind (`Root=0`, `User=1`, `Service=2`), the shared
    /// numeric contract used by both durable encoding and the API/wire codecs.
    pub const fn stable_tag(self) -> u8 {
        match self {
            PrincipalKind::Root => 0,
            PrincipalKind::User => 1,
            PrincipalKind::Service => 2,
        }
    }

    /// The principal kind for a stable tag, or `None` for an unknown tag. Callers choose the error
    /// code (durable decode reports `CorruptObject`; wire decode reports `InvalidArgument`).
    pub const fn from_stable_tag(tag: u8) -> Option<Self> {
        Some(match tag {
            0 => PrincipalKind::Root,
            1 => PrincipalKind::User,
            2 => PrincipalKind::Service,
            _ => return None,
        })
    }
}

fn principal_handle_from_display_name(name: &str) -> Result<String> {
    let mut handle = String::new();
    let mut separator_pending = false;
    for character in name.chars() {
        if character.is_ascii_alphanumeric() {
            if separator_pending && !handle.is_empty() {
                handle.push('-');
            }
            handle.push(character.to_ascii_lowercase());
            separator_pending = false;
        } else {
            separator_pending = true;
        }
    }
    normalize_principal_handle(&handle)
}

fn normalize_principal_handle(value: &str) -> Result<String> {
    let value = value.to_ascii_lowercase();
    let bytes = value.as_bytes();
    if !(1..=64).contains(&bytes.len())
        || !bytes[0].is_ascii_alphanumeric()
        || !bytes[bytes.len() - 1].is_ascii_alphanumeric()
        || !bytes.iter().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
    {
        return Err(LoomError::invalid(
            "principal handle must use 1 to 64 ascii letters, digits, dot, underscore, or hyphen",
        ));
    }
    Ok(value)
}

fn principal_kind_to_u8(kind: PrincipalKind) -> u8 {
    kind.stable_tag()
}

fn principal_kind_from_u8(byte: u8) -> Result<PrincipalKind> {
    PrincipalKind::from_stable_tag(byte)
        .ok_or_else(|| LoomError::corrupt(format!("unknown principal kind {byte:#x}")))
}

fn external_credential_kind_to_u8(kind: ExternalCredentialKind) -> u8 {
    kind.stable_tag()
}

fn external_credential_kind_from_u8(byte: u8) -> Result<ExternalCredentialKind> {
    ExternalCredentialKind::from_stable_tag(byte)
        .ok_or_else(|| LoomError::corrupt(format!("unknown external credential kind {byte:#x}")))
}

fn authority_mode_to_u8(mode: IdentityAuthorityMode) -> u8 {
    match mode {
        IdentityAuthorityMode::Authority => 0,
        IdentityAuthorityMode::Mirror => 1,
        IdentityAuthorityMode::Detached => 2,
    }
}

fn authority_mode_from_u8(byte: u8) -> Result<IdentityAuthorityMode> {
    match byte {
        0 => Ok(IdentityAuthorityMode::Authority),
        1 => Ok(IdentityAuthorityMode::Mirror),
        2 => Ok(IdentityAuthorityMode::Detached),
        other => Err(LoomError::corrupt(format!(
            "unknown identity authority mode {other:#x}"
        ))),
    }
}

fn validate_external_credential_text(name: &str, value: impl Into<String>) -> Result<String> {
    let value = value.into();
    if value.is_empty() {
        return Err(LoomError::invalid(format!("{name} must not be empty")));
    }
    if value.len() > 512 {
        return Err(LoomError::invalid(format!("{name} too long")));
    }
    Ok(value)
}

fn validate_public_key_algorithm(value: &str) -> Result<()> {
    validate_signature_suite(value)
}

fn validate_public_key_material(algorithm: &str, value: &[u8]) -> Result<()> {
    if value.is_empty() {
        return Err(LoomError::invalid(
            "identity public key material must not be empty",
        ));
    }
    if value.len() > IDENTITY_MAX_PUBLIC_KEY_LEN {
        return Err(LoomError::invalid("identity public key material too long"));
    }
    if algorithm == IDENTITY_SIGNATURE_SUITE_ED25519 && value.len() != 32 {
        return Err(LoomError::invalid(
            "identity Ed25519 public key material must be 32 bytes",
        ));
    }
    Ok(())
}

fn validate_signature_suite(value: &str) -> Result<()> {
    match value {
        IDENTITY_AUTHORITY_HANDOFF_ALG_ES256 | IDENTITY_SIGNATURE_SUITE_ED25519 => Ok(()),
        _ => Err(LoomError::invalid(
            "identity signature suite is not supported",
        )),
    }
}

fn key_id_from_header(value: &[u8]) -> Result<WorkspaceId> {
    let bytes: [u8; 16] = value
        .try_into()
        .map_err(|_| LoomError::invalid("authority handoff key id must be a UUID"))?;
    Ok(WorkspaceId::from_bytes(bytes))
}

fn verify_identity_signature(
    algorithm: &str,
    public_key: &[u8],
    payload: &[u8],
    signature: &[u8],
) -> Result<()> {
    match algorithm {
        IDENTITY_AUTHORITY_HANDOFF_ALG_ES256 => {
            verify_es256_signature(public_key, payload, signature)
        }
        IDENTITY_SIGNATURE_SUITE_ED25519 => {
            verify_ed25519_signature(public_key, payload, signature)
        }
        _ => Err(LoomError::invalid(
            "identity signature suite is not supported",
        )),
    }
}

fn verify_es256_signature(public_key: &[u8], payload: &[u8], signature: &[u8]) -> Result<()> {
    use p256::ecdsa::signature::Verifier as _;
    let verifying_key = p256::ecdsa::VerifyingKey::from_sec1_bytes(public_key)
        .map_err(|_| LoomError::invalid("identity public key material is not valid ES256"))?;
    let signature = p256::ecdsa::Signature::from_slice(signature)
        .map_err(|_| LoomError::invalid("identity signature is not valid ES256"))?;
    verifying_key
        .verify(payload, &signature)
        .map_err(|_| LoomError::new(Code::AuthenticationFailed, "identity signature rejected"))
}

fn verify_ed25519_signature(public_key: &[u8], payload: &[u8], signature: &[u8]) -> Result<()> {
    use ed25519_dalek::Verifier as _;
    let key_bytes: [u8; 32] = public_key
        .try_into()
        .map_err(|_| LoomError::invalid("identity public key material is not valid Ed25519"))?;
    let verifying_key = ed25519_dalek::VerifyingKey::from_bytes(&key_bytes)
        .map_err(|_| LoomError::invalid("identity public key material is not valid Ed25519"))?;
    let signature = ed25519_dalek::Signature::try_from(signature)
        .map_err(|_| LoomError::invalid("identity signature is not valid Ed25519"))?;
    verifying_key
        .verify(payload, &signature)
        .map_err(|_| LoomError::new(Code::AuthenticationFailed, "identity signature rejected"))
}

fn put_uvarint(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(byte);
            break;
        }
        out.push(byte | 0x80);
    }
}

fn put_lp(out: &mut Vec<u8>, bytes: &[u8]) {
    put_uvarint(out, bytes.len() as u64);
    out.extend_from_slice(bytes);
}

fn put_optional_digest(out: &mut Vec<u8>, digest: Option<Digest>) {
    match digest {
        None => out.push(0),
        Some(digest) => {
            out.push(1);
            out.push(digest.algo().code());
            out.extend_from_slice(digest.bytes());
        }
    }
}

fn optional_digest_value(digest: Option<Digest>) -> Value {
    digest.map_or(Value::Null, digest_value)
}

fn digest_value(digest: Digest) -> Value {
    Value::Array(vec![
        Value::Uint(u64::from(digest.algo().code())),
        Value::Bytes(digest.bytes().to_vec()),
    ])
}

struct Cur<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Cur<'a> {
    fn take(&mut self, len: usize) -> Result<&'a [u8]> {
        let end = self
            .pos
            .checked_add(len)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| LoomError::corrupt("identity-store bytes truncated"))?;
        let out = &self.bytes[self.pos..end];
        self.pos = end;
        Ok(out)
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn bool(&mut self) -> Result<bool> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            other => Err(LoomError::corrupt(format!(
                "invalid identity bool {other:#x}"
            ))),
        }
    }

    fn uvarint(&mut self) -> Result<u64> {
        let mut shift = 0u32;
        let mut out = 0u64;
        loop {
            let byte = self.u8()?;
            out |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Ok(out);
            }
            shift += 7;
            if shift >= 64 {
                return Err(LoomError::corrupt("identity varint too long"));
            }
        }
    }

    fn take16(&mut self) -> Result<[u8; 16]> {
        let mut out = [0u8; 16];
        out.copy_from_slice(self.take(16)?);
        Ok(out)
    }

    fn take20(&mut self) -> Result<[u8; 20]> {
        let mut out = [0u8; 20];
        out.copy_from_slice(self.take(20)?);
        Ok(out)
    }

    fn take32(&mut self) -> Result<[u8; 32]> {
        let mut out = [0u8; 32];
        out.copy_from_slice(self.take(32)?);
        Ok(out)
    }

    fn optional_digest(&mut self) -> Result<Option<Digest>> {
        match self.u8()? {
            0 => Ok(None),
            1 => {
                let algo = Algo::from_code(self.u8()?)?;
                Ok(Some(Digest::of(algo, self.take32()?)))
            }
            other => Err(LoomError::corrupt(format!(
                "invalid identity digest tag {other:#x}"
            ))),
        }
    }

    fn lp_bytes(&mut self) -> Result<&'a [u8]> {
        let len = self.uvarint()? as usize;
        self.take(len)
    }

    fn lp_str(&mut self) -> Result<String> {
        String::from_utf8(self.lp_bytes()?.to_vec())
            .map_err(|e| LoomError::corrupt(format!("invalid identity utf8: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(byte: u8) -> PrincipalId {
        WorkspaceId::from_bytes([byte; 16])
    }

    fn add_test_public_key(
        store: &mut IdentityStore,
        principal: PrincipalId,
        key_id: WorkspaceId,
        signing_key: &p256::ecdsa::SigningKey,
    ) {
        store
            .add_public_key(
                principal,
                IdentityPublicKeySpec {
                    id: key_id,
                    label: format!("key-{key_id}"),
                    algorithm: IDENTITY_AUTHORITY_HANDOFF_ALG_ES256.to_string(),
                    public_key: signing_key
                        .verifying_key()
                        .to_encoded_point(false)
                        .as_bytes()
                        .to_vec(),
                },
            )
            .unwrap();
    }

    fn signed_handoff(
        from: PrincipalId,
        to: PrincipalId,
        generation: u64,
        head: Option<Digest>,
        key_id: WorkspaceId,
        signing_key: &p256::ecdsa::SigningKey,
    ) -> IdentityAuthorityHandoff {
        use p256::ecdsa::signature::Signer as _;

        let payload = identity_authority_handoff_payload(from, to, generation, head);
        let signature: p256::ecdsa::Signature = signing_key.sign(&payload);
        IdentityAuthorityHandoff {
            from,
            to,
            generation,
            head,
            signed_record: identity_authority_handoff_record(
                from,
                to,
                generation,
                head,
                IDENTITY_AUTHORITY_HANDOFF_ALG_ES256,
                key_id.as_bytes(),
                signature.to_bytes().as_slice(),
            )
            .unwrap(),
        }
    }

    #[test]
    fn single_root_without_passphrase_is_unauthenticated_mode() {
        let store = IdentityStore::new(id(1));
        assert!(!store.authenticated_mode());
        assert_eq!(store.effective_principal(None).unwrap(), id(1));
    }

    #[test]
    fn setting_root_passphrase_requires_authentication() {
        let mut store = IdentityStore::new(id(1));
        store.set_passphrase(id(1), "secret", b"12345678").unwrap();
        assert!(store.authenticated_mode());
        assert_eq!(
            store.effective_principal(None).unwrap_err().code,
            Code::AuthenticationFailed
        );
        let session = store
            .authenticate_passphrase(id(1), "secret", "session-1")
            .unwrap();
        assert_eq!(session.principal, id(1));
        assert_eq!(store.effective_principal(Some("session-1")).unwrap(), id(1));
        assert_eq!(
            store
                .authenticate_passphrase(id(1), "wrong", "session-2")
                .unwrap_err()
                .code,
            Code::AuthenticationFailed
        );
    }

    #[test]
    fn adding_second_principal_enforces_authentication() {
        let mut store = IdentityStore::new(id(1));
        store
            .add_principal(id(2), "alice", PrincipalKind::User)
            .unwrap();
        assert!(store.authenticated_mode());
        assert_eq!(
            store.effective_principal(None).unwrap_err().code,
            Code::AuthenticationFailed
        );
    }

    #[test]
    fn principal_handles_resolve_to_uuid_and_retain_renamed_aliases() {
        let mut store = IdentityStore::new(id(1));
        store
            .add_principal_with_handle(id(2), "alex", "Alex Nguyen", PrincipalKind::User)
            .unwrap();
        assert_eq!(store.resolve_handle("alex").unwrap(), Some(id(2)));
        store.rename_principal_handle(id(2), "alex-nguyen").unwrap();
        assert_eq!(store.resolve_handle("alex").unwrap(), Some(id(2)));
        assert_eq!(store.resolve_handle("ALEX-NGUYEN").unwrap(), Some(id(2)));
        assert_eq!(store.principal(id(2)).unwrap().handle, "alex-nguyen");
        assert_eq!(IdentityStore::decode(&store.encode()).unwrap(), store);
        assert_eq!(
            store
                .add_principal_with_handle(id(3), "alex", "Another Alex", PrincipalKind::User)
                .unwrap_err()
                .code,
            Code::AlreadyExists
        );
    }

    #[test]
    fn identity_decode_rejects_pre_handle_format() {
        let store = IdentityStore::new(id(1));
        let mut bytes = store.encode();
        bytes[3] = b'7';
        assert_eq!(
            IdentityStore::decode(&bytes).unwrap_err().code,
            Code::CorruptObject
        );
    }

    #[test]
    fn root_can_be_removed_after_a_credentialed_replacement_exists() {
        let mut store = IdentityStore::new(id(1));
        store.set_passphrase(id(1), "root", b"12345678").unwrap();
        store
            .add_principal(id(2), "alice", PrincipalKind::User)
            .unwrap();
        store.set_passphrase(id(2), "alice", b"abcdefgh").unwrap();
        assert_eq!(
            store.remove_principal(id(1)).unwrap_err().code,
            Code::IdentityNoRootCredential
        );
        store.assign_role(id(2), ROLE_ADMIN_ID).unwrap();
        let removed = store.remove_principal(id(1)).unwrap();
        assert_eq!(removed.kind, PrincipalKind::Root);
        assert_eq!(store.root_principal(), None);
        assert_eq!(
            store
                .authenticate_passphrase(id(2), "alice", "session")
                .unwrap()
                .principal,
            id(2)
        );
    }

    #[test]
    fn removal_rejects_credential_lockout() {
        let mut store = IdentityStore::new(id(1));
        store.set_passphrase(id(1), "root", b"12345678").unwrap();
        assert_eq!(
            store.remove_principal(id(1)).unwrap_err().code,
            Code::IdentityNoRootCredential
        );
    }

    #[test]
    fn admin_role_revocation_rejects_recovery_lockout() {
        let mut store = IdentityStore::new(id(1));
        store.set_passphrase(id(1), "root", b"12345678").unwrap();
        assert_eq!(
            store.revoke_role(id(1), ROLE_ADMIN_ID).unwrap_err().code,
            Code::IdentityNoRootCredential
        );
    }

    #[test]
    fn identity_store_codec_round_trips_without_sessions() {
        let mut store = IdentityStore::new(id(1));
        store.set_passphrase(id(1), "root", b"12345678").unwrap();
        store
            .add_principal(id(2), "svc", PrincipalKind::Service)
            .unwrap();
        store.set_passphrase(id(2), "svc", b"abcdefgh").unwrap();
        store.assign_role(id(2), ROLE_SERVICE_ID).unwrap();
        store.authenticate_passphrase(id(2), "svc", "live").unwrap();

        let mut decoded = IdentityStore::decode(&store.encode()).unwrap();
        assert_eq!(decoded.principals().count(), 2);
        assert!(decoded.role(ROLE_ADMIN_ID).is_ok());
        assert!(
            decoded
                .principal(id(2))
                .unwrap()
                .roles
                .contains(&ROLE_SERVICE_ID)
        );
        assert_eq!(
            decoded
                .authenticate_passphrase(id(2), "svc", "new")
                .unwrap()
                .principal,
            id(2)
        );
        assert_eq!(
            decoded.session_principal("live").unwrap_err().code,
            Code::AuthenticationFailed
        );
    }

    #[test]
    fn identity_authority_defaults_to_root_authority() {
        let store = IdentityStore::new(id(1));
        assert_eq!(
            store.authority_state(),
            &IdentityAuthorityState {
                mode: IdentityAuthorityMode::Authority,
                authority: id(1),
                generation: 0,
                head: None
            }
        );
        assert_eq!(store.authority_handoffs().count(), 0);
        assert_eq!(store.forced_detach(), None);
    }

    #[test]
    fn identity_authority_handoff_round_trips() {
        let mut store = IdentityStore::new(id(1));
        store
            .add_principal(id(2), "replica-authority", PrincipalKind::Service)
            .unwrap();
        let signing_key = p256::ecdsa::SigningKey::from_slice(&[7u8; 32]).unwrap();
        let key_id = id(9);
        add_test_public_key(&mut store, id(1), key_id, &signing_key);
        let head = Digest::hash(Algo::Sha256, b"authority generation 1");
        let handoff = signed_handoff(id(1), id(2), 1, Some(head), key_id, &signing_key);
        store
            .apply_authority_handoff(handoff.clone(), false)
            .unwrap();
        assert_eq!(
            store.authority_state(),
            &IdentityAuthorityState {
                mode: IdentityAuthorityMode::Mirror,
                authority: id(2),
                generation: 1,
                head: Some(head)
            }
        );
        assert_eq!(
            store.authority_handoffs().cloned().collect::<Vec<_>>(),
            vec![handoff]
        );

        let decoded = IdentityStore::decode(&store.encode()).unwrap();
        assert_eq!(decoded.authority_state(), store.authority_state());
        assert_eq!(
            decoded.authority_handoffs().cloned().collect::<Vec<_>>(),
            store.authority_handoffs().cloned().collect::<Vec<_>>()
        );
    }

    #[test]
    fn identity_public_keys_round_trip_and_verify_authority_handoff() {
        use p256::ecdsa::signature::Signer as _;

        let mut store = IdentityStore::new(id(1));
        store
            .add_principal(id(2), "replica-authority", PrincipalKind::Service)
            .unwrap();
        let signing_key = p256::ecdsa::SigningKey::from_slice(&[7u8; 32]).unwrap();
        let verifying_key = signing_key.verifying_key();
        let public_key = verifying_key.to_encoded_point(false).as_bytes().to_vec();
        let key_id = id(9);
        store
            .add_public_key(
                id(1),
                IdentityPublicKeySpec {
                    id: key_id,
                    label: "root-signing".to_string(),
                    algorithm: IDENTITY_AUTHORITY_HANDOFF_ALG_ES256.to_string(),
                    public_key: public_key.clone(),
                },
            )
            .unwrap();
        let payload = identity_authority_handoff_payload(id(1), id(2), 1, None);
        let signature: p256::ecdsa::Signature = signing_key.sign(&payload);
        let handoff = IdentityAuthorityHandoff {
            from: id(1),
            to: id(2),
            generation: 1,
            head: None,
            signed_record: identity_authority_handoff_record(
                id(1),
                id(2),
                1,
                None,
                IDENTITY_AUTHORITY_HANDOFF_ALG_ES256,
                key_id.as_bytes(),
                signature.to_bytes().as_slice(),
            )
            .unwrap(),
        };
        store
            .apply_verified_authority_handoff(handoff.clone(), false)
            .unwrap();

        let decoded = IdentityStore::decode(&store.encode()).unwrap();
        assert_eq!(
            decoded.public_keys().cloned().collect::<Vec<_>>(),
            vec![IdentityPublicKey {
                id: key_id,
                principal: id(1),
                label: "root-signing".to_string(),
                algorithm: IDENTITY_AUTHORITY_HANDOFF_ALG_ES256.to_string(),
                public_key,
                enabled: true,
            }]
        );
        assert_eq!(
            decoded.authority_handoffs().cloned().collect::<Vec<_>>(),
            vec![handoff]
        );
    }

    #[test]
    fn principal_signature_payload_verifies_ed25519_purpose_and_owner() {
        use ed25519_dalek::Signer as _;

        let mut store = IdentityStore::new(id(1));
        store
            .add_principal(id(2), "checkpoint-signer", PrincipalKind::Service)
            .unwrap();
        let signing_key = ed25519_dalek::SigningKey::from_bytes(&[7u8; 32]);
        let key_id = id(10);
        store
            .add_public_key(
                id(2),
                IdentityPublicKeySpec {
                    id: key_id,
                    label: "ledger-checkpoint".to_string(),
                    algorithm: IDENTITY_SIGNATURE_SUITE_ED25519.to_string(),
                    public_key: signing_key.verifying_key().to_bytes().to_vec(),
                },
            )
            .unwrap();
        let payload = b"ledger checkpoint payload";
        let signed_payload = principal_signature_payload(
            id(2),
            key_id,
            IDENTITY_SIGNATURE_SUITE_ED25519,
            "ledger.checkpoint",
            payload,
        )
        .unwrap();
        let signature = signing_key.sign(&signed_payload);
        store
            .verify_principal_signature(
                id(2),
                key_id,
                IDENTITY_SIGNATURE_SUITE_ED25519,
                "ledger.checkpoint",
                payload,
                signature.to_bytes().as_slice(),
            )
            .unwrap();
        assert_eq!(
            store
                .verify_principal_signature(
                    id(2),
                    key_id,
                    IDENTITY_SIGNATURE_SUITE_ED25519,
                    "authority.handoff",
                    payload,
                    signature.to_bytes().as_slice(),
                )
                .unwrap_err()
                .code,
            Code::AuthenticationFailed
        );
        assert_eq!(
            store
                .verify_principal_signature(
                    id(1),
                    key_id,
                    IDENTITY_SIGNATURE_SUITE_ED25519,
                    "ledger.checkpoint",
                    payload,
                    signature.to_bytes().as_slice(),
                )
                .unwrap_err()
                .code,
            Code::PermissionDenied
        );
        store.revoke_public_key(key_id).unwrap();
        assert_eq!(
            store
                .verify_principal_signature(
                    id(2),
                    key_id,
                    IDENTITY_SIGNATURE_SUITE_ED25519,
                    "ledger.checkpoint",
                    payload,
                    signature.to_bytes().as_slice(),
                )
                .unwrap_err()
                .code,
            Code::NotFound
        );
    }

    #[test]
    fn verified_authority_handoff_rejects_wrong_key_or_signature() {
        use p256::ecdsa::signature::Signer as _;

        let mut store = IdentityStore::new(id(1));
        store
            .add_principal(id(2), "replica-authority", PrincipalKind::Service)
            .unwrap();
        let signing_key = p256::ecdsa::SigningKey::from_slice(&[9u8; 32]).unwrap();
        let key_id = id(8);
        store
            .add_public_key(
                id(2),
                IdentityPublicKeySpec {
                    id: key_id,
                    label: "wrong-owner".to_string(),
                    algorithm: IDENTITY_AUTHORITY_HANDOFF_ALG_ES256.to_string(),
                    public_key: signing_key
                        .verifying_key()
                        .to_encoded_point(false)
                        .as_bytes()
                        .to_vec(),
                },
            )
            .unwrap();
        let payload = identity_authority_handoff_payload(id(1), id(2), 1, None);
        let signature: p256::ecdsa::Signature = signing_key.sign(&payload);
        let wrong_owner = IdentityAuthorityHandoff {
            from: id(1),
            to: id(2),
            generation: 1,
            head: None,
            signed_record: identity_authority_handoff_record(
                id(1),
                id(2),
                1,
                None,
                IDENTITY_AUTHORITY_HANDOFF_ALG_ES256,
                key_id.as_bytes(),
                signature.to_bytes().as_slice(),
            )
            .unwrap(),
        };
        assert_eq!(
            store
                .apply_verified_authority_handoff(wrong_owner, false)
                .unwrap_err()
                .code,
            Code::PermissionDenied
        );

        let mut valid_store = IdentityStore::new(id(1));
        valid_store
            .add_principal(id(2), "replica-authority", PrincipalKind::Service)
            .unwrap();
        valid_store
            .add_public_key(
                id(1),
                IdentityPublicKeySpec {
                    id: key_id,
                    label: "root-signing".to_string(),
                    algorithm: IDENTITY_AUTHORITY_HANDOFF_ALG_ES256.to_string(),
                    public_key: signing_key
                        .verifying_key()
                        .to_encoded_point(false)
                        .as_bytes()
                        .to_vec(),
                },
            )
            .unwrap();
        let mut bad_sig = signature.to_bytes().to_vec();
        bad_sig[0] ^= 0x01;
        let bad_handoff = IdentityAuthorityHandoff {
            from: id(1),
            to: id(2),
            generation: 1,
            head: None,
            signed_record: identity_authority_handoff_record(
                id(1),
                id(2),
                1,
                None,
                IDENTITY_AUTHORITY_HANDOFF_ALG_ES256,
                key_id.as_bytes(),
                &bad_sig,
            )
            .unwrap(),
        };
        assert_eq!(
            valid_store
                .apply_verified_authority_handoff(bad_handoff, false)
                .unwrap_err()
                .code,
            Code::AuthenticationFailed
        );
    }

    #[test]
    fn identity_authority_handoff_rejects_non_advancing_or_unsigned_records() {
        let mut store = IdentityStore::new(id(1));
        store
            .add_principal(id(2), "replica-authority", PrincipalKind::Service)
            .unwrap();
        let signing_key = p256::ecdsa::SigningKey::from_slice(&[8u8; 32]).unwrap();
        let key_id = id(9);
        add_test_public_key(&mut store, id(1), key_id, &signing_key);
        let unsigned = IdentityAuthorityHandoff {
            from: id(1),
            to: id(2),
            generation: 1,
            head: None,
            signed_record: Vec::new(),
        };
        assert_eq!(
            store
                .apply_authority_handoff(unsigned, false)
                .unwrap_err()
                .code,
            Code::InvalidArgument
        );
        store
            .apply_authority_handoff(
                signed_handoff(id(1), id(2), 1, None, key_id, &signing_key),
                false,
            )
            .unwrap();
        assert_eq!(
            store
                .apply_authority_handoff(
                    signed_handoff(id(2), id(1), 1, None, key_id, &signing_key),
                    false,
                )
                .unwrap_err()
                .code,
            Code::Conflict
        );
    }

    #[test]
    fn identity_authority_handoff_rejects_malformed_or_mismatched_records() {
        let mut store = IdentityStore::new(id(1));
        store
            .add_principal(id(2), "replica-authority", PrincipalKind::Service)
            .unwrap();
        assert_eq!(
            store
                .apply_authority_handoff(
                    IdentityAuthorityHandoff {
                        from: id(1),
                        to: id(2),
                        generation: 1,
                        head: None,
                        signed_record: b"not-cbor".to_vec(),
                    },
                    false,
                )
                .unwrap_err()
                .code,
            Code::InvalidArgument
        );
        let mismatched_record = identity_authority_handoff_record(
            id(1),
            id(2),
            2,
            None,
            "ES256",
            b"root-key",
            b"signature",
        )
        .unwrap();
        assert_eq!(
            store
                .apply_authority_handoff(
                    IdentityAuthorityHandoff {
                        from: id(1),
                        to: id(2),
                        generation: 1,
                        head: None,
                        signed_record: mismatched_record,
                    },
                    false,
                )
                .unwrap_err()
                .code,
            Code::InvalidArgument
        );
    }

    #[test]
    fn identity_authority_decode_rejects_malformed_handoff_records() {
        let mut store = IdentityStore::new(id(1));
        store.handoffs.insert(
            1,
            IdentityAuthorityHandoff {
                from: id(1),
                to: id(2),
                generation: 1,
                head: None,
                signed_record: b"not-cbor".to_vec(),
            },
        );

        assert_eq!(
            IdentityStore::decode(&store.encode()).unwrap_err().code,
            Code::CorruptObject
        );
    }

    #[test]
    fn identity_authority_forced_detach_round_trips() {
        let mut store = IdentityStore::new(id(1));
        store
            .add_principal(id(2), "local-authority", PrincipalKind::Service)
            .unwrap();
        let detach = store
            .force_detach_authority(id(2), 7, "authority unreachable")
            .unwrap();
        assert_eq!(
            detach,
            IdentityAuthorityDetach {
                previous_authority: id(1),
                new_authority: id(2),
                generation: 7,
                reason: "authority unreachable".to_string()
            }
        );
        assert_eq!(
            store.authority_state(),
            &IdentityAuthorityState {
                mode: IdentityAuthorityMode::Detached,
                authority: id(2),
                generation: 7,
                head: None
            }
        );
        let decoded = IdentityStore::decode(&store.encode()).unwrap();
        assert_eq!(decoded.authority_state(), store.authority_state());
        assert_eq!(decoded.forced_detach(), store.forced_detach());
        assert_eq!(
            store
                .force_detach_authority(id(2), 7, "again")
                .unwrap_err()
                .code,
            Code::Conflict
        );
    }

    #[test]
    fn identity_authority_witness_is_stable_and_snapshot_bound() {
        let mut store = IdentityStore::new(id(1));
        let first = store.authority_witness(Algo::Sha256);
        assert_eq!(first.authority, id(1));
        assert_eq!(first.generation, 0);
        assert_eq!(first.latest_handoff_digest, None);
        assert_eq!(first, store.authority_witness(Algo::Sha256));
        assert_eq!(
            cbor::decode_array(&first.encode()).unwrap()[0],
            Value::Text(IDENTITY_AUTHORITY_WITNESS_RECORD_TYPE.to_string())
        );

        store
            .add_principal(id(2), "replica-authority", PrincipalKind::Service)
            .unwrap();
        let second = store.authority_witness(Algo::Sha256);
        assert_ne!(first.snapshot_digest, second.snapshot_digest);
        assert_ne!(first.digest(Algo::Sha256), second.digest(Algo::Sha256));
    }

    #[test]
    fn identity_authority_replication_fast_forwards_signed_chain() {
        let mut source = IdentityStore::new(id(1));
        let mut destination = IdentityStore::new(id(1));
        source
            .add_principal(id(2), "replica-authority", PrincipalKind::Service)
            .unwrap();
        let signing_key = p256::ecdsa::SigningKey::from_slice(&[7u8; 32]).unwrap();
        let key_id = id(9);
        add_test_public_key(&mut source, id(1), key_id, &signing_key);
        let head = Digest::hash(Algo::Sha256, b"authority generation 1");
        source
            .apply_verified_authority_handoff(
                signed_handoff(id(1), id(2), 1, Some(head), key_id, &signing_key),
                true,
            )
            .unwrap();

        let report = destination
            .replicate_authority_from(&source, Algo::Sha256, false)
            .unwrap();
        assert_eq!(report.from_generation, 0);
        assert_eq!(report.to_generation, 1);
        assert!(report.applied);
        assert_eq!(
            destination.authority_state(),
            &IdentityAuthorityState {
                mode: IdentityAuthorityMode::Mirror,
                authority: id(2),
                generation: 1,
                head: Some(head)
            }
        );
        assert_eq!(
            destination.public_keys().cloned().collect::<Vec<_>>(),
            source.public_keys().cloned().collect::<Vec<_>>()
        );
        assert!(
            !destination
                .replicate_authority_from(&source, Algo::Sha256, false)
                .unwrap()
                .applied
        );
    }

    #[test]
    fn identity_authority_replication_rejects_forks_and_detach() {
        let mut source = IdentityStore::new(id(1));
        let mut destination = IdentityStore::new(id(1));
        source
            .add_principal(id(3), "same-generation-drift", PrincipalKind::User)
            .unwrap();
        assert_eq!(
            destination
                .replicate_authority_from(&source, Algo::Sha256, false)
                .unwrap_err()
                .code,
            Code::Conflict
        );

        let mut detached = IdentityStore::new(id(1));
        detached
            .add_principal(id(2), "detached-authority", PrincipalKind::Service)
            .unwrap();
        detached
            .force_detach_authority(id(2), 1, "authority unreachable")
            .unwrap();
        assert_eq!(
            destination
                .replicate_authority_from(&detached, Algo::Sha256, false)
                .unwrap_err()
                .code,
            Code::Conflict
        );
    }

    #[test]
    fn app_credentials_authenticate_round_trip_and_revoke() {
        let mut store = IdentityStore::new(id(1));
        store.set_passphrase(id(1), "root", b"12345678").unwrap();
        store
            .add_principal(id(2), "svc", PrincipalKind::Service)
            .unwrap();
        store.assign_role(id(2), ROLE_SERVICE_ID).unwrap();
        let secret = [7u8; 32];
        let credential = store
            .create_app_credential(id(2), id(9), "qdrant", &secret, b"salt-123")
            .unwrap();
        assert_eq!(credential.principal, id(2));
        assert_eq!(credential.label, "qdrant");
        assert!(store.authenticated_mode());
        let token = app_credential_token(id(9), &secret);
        assert_eq!(
            store
                .authenticate_app_credential(&token, "app-session")
                .unwrap()
                .principal,
            id(2)
        );

        let mut decoded = IdentityStore::decode(&store.encode()).unwrap();
        let listed: Vec<_> = decoded.app_credentials().cloned().collect();
        assert_eq!(listed, vec![credential.clone()]);
        assert_eq!(
            decoded
                .authenticate_app_credential(&token, "decoded-session")
                .unwrap()
                .principal,
            id(2)
        );
        assert_eq!(decoded.revoke_app_credential(id(9)).unwrap(), credential);
        assert_eq!(
            decoded
                .authenticate_app_credential(&token, "revoked-session")
                .unwrap_err()
                .code,
            Code::AuthenticationFailed
        );
    }

    #[test]
    fn external_credentials_bind_verified_assertions() {
        let mut store = IdentityStore::new(id(1));
        store.set_passphrase(id(1), "root", b"12345678").unwrap();
        store
            .add_principal(id(2), "svc", PrincipalKind::Service)
            .unwrap();
        store.assign_role(id(2), ROLE_SERVICE_ID).unwrap();
        let credential = store
            .create_external_credential(
                id(2),
                ExternalCredentialSpec {
                    id: id(9),
                    kind: ExternalCredentialKind::OidcSubject,
                    label: "okta-prod".to_string(),
                    issuer: "https://issuer.example".to_string(),
                    subject: "00u123".to_string(),
                    material_digest: Some("sha256:metadata".to_string()),
                },
            )
            .unwrap();
        assert_eq!(credential.principal, id(2));
        assert_eq!(credential.kind, ExternalCredentialKind::OidcSubject);
        assert_eq!(credential.label, "okta-prod");
        assert!(store.authenticated_mode());
        assert_eq!(
            store
                .authenticate_verified_external_credential(VerifiedExternalCredentialAuth {
                    kind: ExternalCredentialKind::OidcSubject,
                    issuer: "https://issuer.example",
                    subject: "00u123",
                    material_digest: Some("sha256:metadata"),
                    challenge_id: None,
                    now_ms: 100,
                    session_id: "oidc-session",
                })
                .unwrap()
                .principal,
            id(2)
        );
        assert_eq!(
            store
                .authenticate_verified_external_credential(VerifiedExternalCredentialAuth {
                    kind: ExternalCredentialKind::OidcSubject,
                    issuer: "https://issuer.example",
                    subject: "wrong",
                    material_digest: Some("sha256:metadata"),
                    challenge_id: None,
                    now_ms: 100,
                    session_id: "bad-session",
                })
                .unwrap_err()
                .code,
            Code::AuthenticationFailed
        );

        let mut decoded = IdentityStore::decode(&store.encode()).unwrap();
        let listed: Vec<_> = decoded.external_credentials().cloned().collect();
        assert_eq!(listed, vec![credential.clone()]);
        assert_eq!(
            decoded
                .authenticate_verified_external_credential(VerifiedExternalCredentialAuth {
                    kind: ExternalCredentialKind::OidcSubject,
                    issuer: "https://issuer.example",
                    subject: "00u123",
                    material_digest: Some("sha256:metadata"),
                    challenge_id: None,
                    now_ms: 100,
                    session_id: "decoded-session",
                })
                .unwrap()
                .principal,
            id(2)
        );
        assert_eq!(
            decoded.revoke_external_credential(id(9)).unwrap(),
            credential
        );
        assert_eq!(
            decoded
                .authenticate_verified_external_credential(VerifiedExternalCredentialAuth {
                    kind: ExternalCredentialKind::OidcSubject,
                    issuer: "https://issuer.example",
                    subject: "00u123",
                    material_digest: Some("sha256:metadata"),
                    challenge_id: None,
                    now_ms: 100,
                    session_id: "revoked-session",
                })
                .unwrap_err()
                .code,
            Code::AuthenticationFailed
        );

        let mut challenge_bound = IdentityStore::new(id(1));
        challenge_bound
            .set_passphrase(id(1), "root", b"12345678")
            .unwrap();
        challenge_bound
            .create_external_credential(
                id(1),
                ExternalCredentialSpec {
                    id: id(20),
                    kind: ExternalCredentialKind::PublicKey,
                    label: "key-a".to_string(),
                    issuer: "loom".to_string(),
                    subject: "a".to_string(),
                    material_digest: Some("sha256:key-a".to_string()),
                },
            )
            .unwrap();
        challenge_bound
            .create_external_credential(
                id(1),
                ExternalCredentialSpec {
                    id: id(21),
                    kind: ExternalCredentialKind::PublicKey,
                    label: "key-b".to_string(),
                    issuer: "loom".to_string(),
                    subject: "b".to_string(),
                    material_digest: Some("sha256:key-b".to_string()),
                },
            )
            .unwrap();
        challenge_bound
            .create_external_credential_challenge(id(21), id(22), vec![1; 32], 100, 200)
            .unwrap();
        let err = challenge_bound
            .authenticate_verified_external_credential(VerifiedExternalCredentialAuth {
                kind: ExternalCredentialKind::PublicKey,
                issuer: "loom",
                subject: "a",
                material_digest: Some("sha256:key-a"),
                challenge_id: Some(id(22)),
                now_ms: 150,
                session_id: "wrong-key-challenge",
            })
            .unwrap_err();
        assert_eq!(err.code, Code::AuthenticationFailed);
        assert!(
            challenge_bound
                .external_challenges()
                .any(|challenge| challenge.id == id(22))
        );
    }

    #[test]
    fn external_credential_duplicate_proofs_are_rejected() {
        let mut store = IdentityStore::new(id(1));
        store.set_passphrase(id(1), "root", b"12345678").unwrap();
        store
            .create_external_credential(
                id(1),
                ExternalCredentialSpec {
                    id: id(9),
                    kind: ExternalCredentialKind::MtlsCertificate,
                    label: "device-a".to_string(),
                    issuer: "ca-root".to_string(),
                    subject: "subject-a".to_string(),
                    material_digest: Some("sha256:cert".to_string()),
                },
            )
            .unwrap();
        assert_eq!(
            store
                .create_external_credential(
                    id(1),
                    ExternalCredentialSpec {
                        id: id(10),
                        kind: ExternalCredentialKind::MtlsCertificate,
                        label: "device-a-duplicate".to_string(),
                        issuer: "ca-root".to_string(),
                        subject: "subject-a".to_string(),
                        material_digest: Some("sha256:cert".to_string()),
                    },
                )
                .unwrap_err()
                .code,
            Code::AlreadyExists
        );
    }

    #[test]
    fn external_admin_credential_counts_as_recovery() {
        let mut store = IdentityStore::new(id(1));
        store.set_passphrase(id(1), "root", b"12345678").unwrap();
        store
            .add_principal(id(2), "admin-svc", PrincipalKind::Service)
            .unwrap();
        store.assign_role(id(2), ROLE_ADMIN_ID).unwrap();
        assert_eq!(
            store.revoke_role(id(1), ROLE_ADMIN_ID).unwrap_err().code,
            Code::IdentityNoRootCredential
        );
        store
            .create_external_credential(
                id(2),
                ExternalCredentialSpec {
                    id: id(9),
                    kind: ExternalCredentialKind::PublicKey,
                    label: "admin-key".to_string(),
                    issuer: "loom".to_string(),
                    subject: "key-1".to_string(),
                    material_digest: Some("sha256:key".to_string()),
                },
            )
            .unwrap();
        assert!(store.revoke_role(id(1), ROLE_ADMIN_ID).unwrap());
    }

    #[test]
    fn external_credential_challenges_round_trip_consume_and_prune() {
        let mut store = IdentityStore::new(id(1));
        store.set_passphrase(id(1), "root", b"12345678").unwrap();
        store
            .create_external_credential(
                id(1),
                ExternalCredentialSpec {
                    id: id(9),
                    kind: ExternalCredentialKind::PublicKey,
                    label: "admin-key".to_string(),
                    issuer: "loom".to_string(),
                    subject: "key-1".to_string(),
                    material_digest: Some("sha256:key".to_string()),
                },
            )
            .unwrap();
        let challenge = store
            .create_external_credential_challenge(id(9), id(10), vec![7; 32], 100, 200)
            .unwrap();
        assert_eq!(challenge.credential, id(9));
        assert_eq!(challenge.nonce, vec![7; 32]);

        let mut decoded = IdentityStore::decode(&store.encode()).unwrap();
        let listed: Vec<_> = decoded.external_challenges().cloned().collect();
        assert_eq!(listed, vec![challenge.clone()]);
        assert_eq!(
            decoded
                .consume_external_credential_challenge(id(10), 200)
                .unwrap(),
            challenge
        );
        assert_eq!(
            decoded
                .consume_external_credential_challenge(id(10), 200)
                .unwrap_err()
                .code,
            Code::AuthenticationFailed
        );

        decoded
            .create_external_credential_challenge(id(9), id(11), vec![8; 32], 300, 400)
            .unwrap();
        decoded
            .create_external_credential_challenge(id(9), id(12), vec![9; 32], 300, 500)
            .unwrap();
        assert_eq!(decoded.prune_external_credential_challenges(450), 1);
        assert_eq!(
            decoded
                .external_challenges()
                .map(|challenge| challenge.id)
                .collect::<Vec<_>>(),
            vec![id(12)]
        );
        decoded.revoke_external_credential(id(9)).unwrap();
        assert_eq!(decoded.external_challenges().count(), 0);
    }

    #[test]
    fn external_credential_challenges_reject_invalid_lifecycle() {
        let mut store = IdentityStore::new(id(1));
        store.set_passphrase(id(1), "root", b"12345678").unwrap();
        store
            .create_external_credential(
                id(1),
                ExternalCredentialSpec {
                    id: id(9),
                    kind: ExternalCredentialKind::Passkey,
                    label: "admin-passkey".to_string(),
                    issuer: "loom".to_string(),
                    subject: "credential-1".to_string(),
                    material_digest: Some("sha256:passkey".to_string()),
                },
            )
            .unwrap();
        assert_eq!(
            store
                .create_external_credential_challenge(id(9), id(10), vec![1; 15], 100, 200)
                .unwrap_err()
                .code,
            Code::InvalidArgument
        );
        assert_eq!(
            store
                .create_external_credential_challenge(id(9), id(10), vec![1; 16], 200, 200)
                .unwrap_err()
                .code,
            Code::InvalidArgument
        );
        store
            .create_external_credential_challenge(id(9), id(10), vec![1; 16], 100, 200)
            .unwrap();
        assert_eq!(
            store
                .create_external_credential_challenge(id(9), id(10), vec![2; 16], 100, 200)
                .unwrap_err()
                .code,
            Code::AlreadyExists
        );
        assert_eq!(
            store
                .consume_external_credential_challenge(id(10), 201)
                .unwrap_err()
                .code,
            Code::AuthenticationFailed
        );
    }

    #[test]
    fn bind_session_rehydrates_authenticated_principal() {
        let mut store = IdentityStore::new(id(1));
        store.set_passphrase(id(1), "root", b"12345678").unwrap();
        let mut decoded = IdentityStore::decode(&store.encode()).unwrap();
        let session = decoded.bind_session(id(1), "live").unwrap();
        assert_eq!(session.principal, id(1));
        assert_eq!(decoded.effective_principal(Some("live")).unwrap(), id(1));
        assert_eq!(
            decoded.bind_session(id(2), "missing").unwrap_err().code,
            Code::NotFound
        );
        assert_eq!(
            decoded.bind_session(id(1), "").unwrap_err().code,
            Code::InvalidArgument
        );
    }
}
