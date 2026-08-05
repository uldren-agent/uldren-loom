//! Shared identity authority-policy administration over an already-open store.
//!
//! Licensed under BUSL-1.1.

use loom_core::identity::{
    IdentityAuthorityDetach, IdentityAuthorityMode, IdentityAuthoritySyncReport,
    IdentityAuthorityWitness, IdentityStore, PrincipalId,
};
use loom_core::{Algo, Loom};
use loom_store::{AuthorityReplicationPolicy, FileStore};
use loom_types::{Code, LoomError};

pub struct IdentityAuthorityPolicyService;

impl IdentityAuthorityPolicyService {
    pub fn authority_witness(
        loom: &mut Loom<FileStore>,
    ) -> Result<IdentityAuthorityWitness, LoomError> {
        loom.authorize_global_admin()?;
        let identity = loom.identity_store().ok_or_else(identity_unsupported)?;
        Ok(identity.authority_witness(loom.store().digest_algo()))
    }

    pub fn authority_replication_policies(
        loom: &mut Loom<FileStore>,
    ) -> Result<Vec<AuthorityReplicationPolicy>, LoomError> {
        loom.authorize_global_admin()?;
        loom.store().authority_replication_policies()
    }

    pub fn source_identity_snapshot(loom: &mut Loom<FileStore>) -> Result<Vec<u8>, LoomError> {
        loom.authorize_global_admin()?;
        Ok(loom
            .identity_store()
            .ok_or_else(identity_unsupported)?
            .encode())
    }

    pub fn force_detach_authority_json(
        loom: &mut Loom<FileStore>,
        principal: PrincipalId,
        generation: u64,
        reason: &str,
    ) -> Result<String, LoomError> {
        loom.authorize_global_admin()?;
        let actor = loom.effective_principal()?;
        let (detach, snapshot) = {
            let identity = loom.identity_store_mut().ok_or_else(identity_unsupported)?;
            identity.principal(principal)?;
            let detach = identity.force_detach_authority(principal, generation, reason)?;
            (detach, identity.clone())
        };
        let target = format!(
            "previous_authority={};new_authority={};generation={}",
            detach.previous_authority, detach.new_authority, detach.generation
        );
        let seq = loom.store().save_identity_store_audited(
            &snapshot,
            actor,
            "identity.authority.force_detach",
            Some(&target),
        )?;
        loom.set_identity_store(snapshot);
        json_string(&serde_json::json!({
            "seq": seq,
            "detach": identity_authority_detach_json_value(&detach),
        }))
    }

    pub fn replicate_authority_json(
        loom: &mut Loom<FileStore>,
        source_identity: &IdentityStore,
        source: &str,
        become_authority: bool,
    ) -> Result<String, LoomError> {
        loom.authorize_global_admin()?;
        Self::replicate_authority_json_authorized(loom, source_identity, source, become_authority)
    }

    pub fn replicate_authority_snapshot_json(
        loom: &mut Loom<FileStore>,
        source_identity_snapshot: &[u8],
        source: &str,
        become_authority: bool,
    ) -> Result<String, LoomError> {
        loom.authorize_global_admin()?;
        let source_identity = IdentityStore::decode(source_identity_snapshot)?;
        Self::replicate_authority_json_authorized(loom, &source_identity, source, become_authority)
    }

    fn replicate_authority_json_authorized(
        loom: &mut Loom<FileStore>,
        source_identity: &IdentityStore,
        source: &str,
        become_authority: bool,
    ) -> Result<String, LoomError> {
        let actor = loom.effective_principal()?;
        let algo = loom.store().digest_algo();
        let (report, snapshot) = {
            let identity = loom.identity_store_mut().ok_or_else(|| {
                LoomError::new(
                    Code::Unsupported,
                    "destination identity store not initialized",
                )
            })?;
            let report =
                identity.replicate_authority_from(source_identity, algo, become_authority)?;
            (report, identity.clone())
        };
        let target = format!(
            "source={source};from_generation={};to_generation={};applied={}",
            report.from_generation, report.to_generation, report.applied
        );
        let seq = loom.store().save_identity_store_audited(
            &snapshot,
            actor,
            "identity.authority.replicate",
            Some(&target),
        )?;
        loom.set_identity_store(snapshot);
        json_string(&identity_authority_sync_report_json_value(
            &report, algo, seq,
        ))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn configure_authority_replication_json(
        loom: &mut Loom<FileStore>,
        id: &str,
        source: &str,
        disabled: bool,
        pull_on_start: bool,
        interval_ms: Option<u64>,
        jitter_ms: u64,
        backoff_ms: u64,
        publish_witness: bool,
    ) -> Result<String, LoomError> {
        loom.authorize_global_admin()?;
        let actor = loom.effective_principal()?;
        let mut policy = FileStore::authority_replication_policy(id, source, !disabled)?;
        policy.pull_on_start = pull_on_start;
        policy.interval_ms = interval_ms;
        policy.jitter_ms = jitter_ms;
        policy.backoff_ms = backoff_ms;
        policy.publish_witness = publish_witness;
        let target = format!("id={id};source={source}");
        let seq = loom.store().save_authority_replication_policy_audited(
            &policy,
            actor,
            "authority.replication.configure",
            Some(&target),
        )?;
        let stored = loom
            .store()
            .authority_replication_policy_by_id(id)?
            .ok_or_else(|| {
                LoomError::new(
                    Code::Internal,
                    "authority replication policy not found after save",
                )
            })?;
        json_string(&serde_json::json!({
            "seq": seq,
            "policy": authority_replication_policy_json_value(&stored),
        }))
    }

    pub fn remove_authority_replication_json(
        loom: &mut Loom<FileStore>,
        id: &str,
    ) -> Result<String, LoomError> {
        loom.authorize_global_admin()?;
        let actor = loom.effective_principal()?;
        let target = format!("id={id}");
        let seq = loom.store().remove_authority_replication_policy_audited(
            id,
            actor,
            "authority.replication.remove",
            Some(&target),
        )?;
        json_string(&serde_json::json!({
            "seq": seq,
            "id": id,
        }))
    }
}

fn identity_unsupported() -> LoomError {
    LoomError::new(
        Code::Unsupported,
        "store is in unauthenticated-root mode; no identity is configured",
    )
}

fn identity_authority_detach_json_value(detach: &IdentityAuthorityDetach) -> serde_json::Value {
    serde_json::json!({
        "previous_authority": detach.previous_authority.to_string(),
        "new_authority": detach.new_authority.to_string(),
        "generation": detach.generation,
        "reason": detach.reason,
    })
}

fn identity_authority_mode_str(mode: IdentityAuthorityMode) -> &'static str {
    match mode {
        IdentityAuthorityMode::Authority => "authority",
        IdentityAuthorityMode::Mirror => "mirror",
        IdentityAuthorityMode::Detached => "detached",
    }
}

fn hex_bytes(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn identity_authority_witness_json_value(
    witness: &IdentityAuthorityWitness,
    algo: Algo,
) -> serde_json::Value {
    let record = witness.encode();
    let record_digest = witness.digest(algo);
    serde_json::json!({
        "authority": witness.authority.to_string(),
        "mode": identity_authority_mode_str(witness.mode),
        "generation": witness.generation,
        "head": witness.head.map(|head| head.to_string()),
        "snapshot_digest": witness.snapshot_digest.to_string(),
        "latest_handoff_digest": witness.latest_handoff_digest.map(|digest| digest.to_string()),
        "record_hex": hex_bytes(&record),
        "record_digest": record_digest.to_string(),
    })
}

fn identity_authority_sync_report_json_value(
    report: &IdentityAuthoritySyncReport,
    algo: Algo,
    seq: u64,
) -> serde_json::Value {
    serde_json::json!({
        "seq": seq,
        "from_generation": report.from_generation,
        "to_generation": report.to_generation,
        "applied": report.applied,
        "witness": identity_authority_witness_json_value(&report.witness, algo),
    })
}

fn authority_replication_policy_json_value(
    policy: &AuthorityReplicationPolicy,
) -> serde_json::Value {
    serde_json::json!({
        "id": policy.id,
        "source": policy.source,
        "enabled": policy.enabled,
        "pull_on_start": policy.pull_on_start,
        "interval_ms": policy.interval_ms,
        "jitter_ms": policy.jitter_ms,
        "backoff_ms": policy.backoff_ms,
        "publish_witness": policy.publish_witness,
        "last_success_ms": policy.last_success_ms,
        "last_failure_ms": policy.last_failure_ms,
        "last_error": policy.last_error,
        "last_modified_audit_seq": policy.last_modified_audit_seq,
    })
}

fn json_string(value: &serde_json::Value) -> Result<String, LoomError> {
    serde_json::to_string(value)
        .map_err(|err| LoomError::new(Code::InvalidArgument, err.to_string()))
}
