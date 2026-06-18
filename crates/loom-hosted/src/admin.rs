use std::collections::{BTreeMap, BTreeSet};

use loom_core::{
    AclDomain, AclEffect, AclGrant, AclPredicate, AclRight, AclScope, AclScopeKind, AclStore,
    AclSubject, AppCredential, CapabilityVisibility, Code, ExternalCredential,
    ExternalCredentialKind, ExternalCredentialSpec, IdentityPublicKeySpec, IdentityRole,
    IdentityStore, Loom, LoomError, Principal, PrincipalKind, ProtectedRefPolicy, WorkspaceId,
    WsSelector, app_credential_token,
};
use loom_store::{
    AuditConfig, AuditPruneStats, AuditRecord, FileStore, NetworkAccessPolicyRecord,
    NetworkAccessRule, ServedListenerRecord,
};
use loom_substrate::web::{
    WebListener, WebMethod, WebProtocol, WebRoute, WebRouteMode, WebRouteTable,
    web_profile_listener_key,
};

use crate::{HostedAuth, HostedKernel, HostedOutcome, hosted_outcome};

pub struct HostedAdminAdapter<'a> {
    kernel: &'a HostedKernel,
}

#[derive(Clone, Debug)]
pub struct HostedExternalCredentialInput {
    pub principal: WorkspaceId,
    pub kind: ExternalCredentialKind,
    pub label: String,
    pub issuer: String,
    pub subject: String,
    pub material_digest: Option<String>,
}

#[derive(Clone, Debug)]
pub struct HostedPublicKeyInput {
    pub id: Option<WorkspaceId>,
    pub principal: WorkspaceId,
    pub label: String,
    pub algorithm: String,
    pub public_key: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct HostedAclGrantInput {
    pub effect: AclEffect,
    pub subject: AclSubject,
    pub workspace: Option<String>,
    pub domain: Option<AclDomain>,
    pub rights: BTreeSet<AclRight>,
    pub ref_glob: Option<String>,
    pub scopes: Vec<AclScope>,
    pub predicate: Option<AclPredicate>,
}

#[derive(Clone, Debug)]
pub struct HostedNetworkAccessPolicyInput {
    pub name: String,
    pub description: Option<String>,
    pub default_action: loom_store::NetworkAccessAction,
    pub rules: Vec<NetworkAccessRule>,
}

#[derive(Clone, Debug)]
pub struct HostedWebRouteInput {
    pub route_id: String,
    pub host_pattern: Option<String>,
    pub path_prefix: String,
    pub workspace: Option<String>,
    pub root_path: String,
}

impl HostedKernel {
    pub fn admin(&self) -> HostedAdminAdapter<'_> {
        HostedAdminAdapter { kernel: self }
    }
}

impl HostedAdminAdapter<'_> {
    pub fn capabilities_json(&self, auth: &HostedAuth, detailed: bool) -> HostedOutcome<String> {
        hosted_outcome(self.kernel.read(auth, |loom| {
            authorize_admin(loom)?;
            let visibility = if detailed {
                CapabilityVisibility::Detailed
            } else {
                CapabilityVisibility::Default
            };
            Ok(loom.capabilities().to_json(visibility))
        }))
    }

    pub fn listeners_json(&self, auth: &HostedAuth) -> HostedOutcome<String> {
        hosted_outcome(self.kernel.read(auth, |loom| {
            authorize_admin(loom)?;
            Ok(served_listeners_json(&loom.store().served_listeners()?))
        }))
    }

    pub fn network_access_policies_json(&self, auth: &HostedAuth) -> HostedOutcome<String> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            authorize_admin(loom)?;
            let policies = loom.store().network_access_policies()?;
            let references = network_access_served_listener_reference_map(loom.store())?;
            let seq = loom.store().audit_append(
                auth.principal,
                "network-access.policy.list",
                Some("network-access"),
            )?;
            network_access_policies_json(loom.store(), seq, &policies, &references)
        }))
    }

    pub fn network_access_policy_json(
        &self,
        auth: &HostedAuth,
        name: &str,
    ) -> HostedOutcome<String> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            authorize_admin(loom)?;
            let policy = loom
                .store()
                .network_access_policy(name)?
                .ok_or_else(|| LoomError::not_found("network access policy not found"))?;
            let references = network_access_served_listener_references(loom.store(), name)?;
            let target = network_access_policy_target(name);
            let seq = loom.store().audit_append(
                auth.principal,
                "network-access.policy.audit",
                Some(&target),
            )?;
            network_access_policy_json(loom.store(), &policy, seq, &references)
        }))
    }

    pub fn set_network_access_policy(
        &self,
        auth: &HostedAuth,
        input: HostedNetworkAccessPolicyInput,
    ) -> HostedOutcome<String> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            authorize_admin(loom)?;
            let mut policy = FileStore::network_access_policy_record(
                &input.name,
                input.description,
                input.default_action,
                input.rules,
            )?;
            let target = network_access_policy_target(&input.name);
            let seq = loom.store().save_network_access_policy_audited(
                &policy,
                auth.principal,
                "network-access.policy.set",
                Some(&target),
            )?;
            policy.created_audit_seq = policy.created_audit_seq.or(Some(seq));
            policy.updated_audit_seq = Some(seq);
            network_access_policy_json(loom.store(), &policy, seq, &[])
        }))
    }

    pub fn remove_network_access_policy(
        &self,
        auth: &HostedAuth,
        name: &str,
    ) -> HostedOutcome<String> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            authorize_admin(loom)?;
            let references = network_access_served_listener_references(loom.store(), name)?;
            let target = network_access_policy_target(name);
            if !references.is_empty() {
                let denied_target = network_access_denied_remove_target(name, &references);
                loom.store().audit_append(
                    auth.principal,
                    "network-access.policy.remove.denied",
                    Some(&denied_target),
                )?;
                return Err(LoomError::invalid(format!(
                    "network access policy {name:?} is referenced by served listeners: {}",
                    references.join(", ")
                )));
            }
            let seq = loom.store().remove_network_access_policy_audited(
                name,
                auth.principal,
                "network-access.policy.remove",
                Some(&target),
            )?;
            Ok(format!("{{\"seq\":{seq},\"name\":{}}}", json_string(name)))
        }))
    }

    pub fn audit_json(&self, auth: &HostedAuth) -> HostedOutcome<String> {
        hosted_outcome(self.kernel.read(auth, |loom| {
            authorize_admin(loom)?;
            Ok(audit_records_json(&loom.store().audit_records()?))
        }))
    }

    pub fn audit_export_json(&self, auth: &HostedAuth) -> HostedOutcome<String> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            authorize_admin(loom)?;
            let records = loom.store().audit_records()?;
            loom.store()
                .audit_append(auth.principal, "audit.export", None)?;
            Ok(audit_records_json(&records))
        }))
    }

    pub fn audit_config_json(&self, auth: &HostedAuth) -> HostedOutcome<String> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            authorize_admin(loom)?;
            let config = loom.store().audit_config()?;
            loom.store()
                .audit_append(auth.principal, "audit.config.show", None)?;
            Ok(audit_config_json(config))
        }))
    }

    pub fn set_audit_config(
        &self,
        auth: &HostedAuth,
        retention_days: Option<u32>,
        legal_hold: Option<bool>,
    ) -> HostedOutcome<String> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            authorize_admin(loom)?;
            let mut config = loom.store().audit_config()?;
            if let Some(value) = retention_days {
                config.retention_days = value;
            }
            if let Some(value) = legal_hold {
                config.legal_hold = value;
            }
            let target = format!(
                "retention_days={};legal_hold={}",
                config.retention_days, config.legal_hold
            );
            let seq = loom.store().save_audit_config_audited(
                config,
                auth.principal,
                "audit.config.set",
                Some(&target),
            )?;
            Ok(format!(
                "{{\"seq\":{seq},\"config\":{}}}",
                audit_config_json(config)
            ))
        }))
    }

    pub fn prune_audit(&self, auth: &HostedAuth, through_seq: u64) -> HostedOutcome<String> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            authorize_admin(loom)?;
            let stats = loom
                .store()
                .audit_prune_through(auth.principal, through_seq)?;
            Ok(audit_prune_stats_json(stats))
        }))
    }

    pub fn acl_json(&self, auth: &HostedAuth) -> HostedOutcome<String> {
        hosted_outcome(self.kernel.read(auth, |loom| {
            authorize_admin(loom)?;
            Ok(acl_list_json(loom.acl_store()))
        }))
    }

    pub fn identity_json(&self, auth: &HostedAuth) -> HostedOutcome<String> {
        hosted_outcome(self.kernel.read(auth, |loom| {
            authorize_admin(loom)?;
            Ok(identity_list_json(identity_store(loom)?))
        }))
    }

    pub fn authority_witness_json(&self, auth: &HostedAuth) -> HostedOutcome<String> {
        hosted_outcome(self.kernel.read(auth, |loom| {
            authorize_admin(loom)?;
            let algo = loom.store().digest_algo();
            let witness = identity_store(loom)?.authority_witness(algo);
            Ok(identity_authority_witness_json(&witness, algo))
        }))
    }

    pub fn add_principal(
        &self,
        auth: &HostedAuth,
        id: WorkspaceId,
        name: String,
        kind: PrincipalKind,
    ) -> HostedOutcome<String> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            authorize_admin(loom)?;
            let snapshot = {
                let identity = identity_store_mut(loom)?;
                identity.add_principal(id, name, kind)?;
                identity.clone()
            };
            let target = id.to_string();
            let seq = loom.store().save_identity_store_audited(
                &snapshot,
                auth.principal,
                "identity.add_principal",
                Some(&target),
            )?;
            Ok(format!(
                "{{\"seq\":{seq},\"principal\":{}}}",
                principal_json(snapshot.principal(id)?)
            ))
        }))
    }

    pub fn set_principal_passphrase(
        &self,
        auth: &HostedAuth,
        principal: WorkspaceId,
        passphrase: &str,
    ) -> HostedOutcome<String> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            authorize_admin(loom)?;
            let mut salt = [0u8; 16];
            getrandom::fill(&mut salt).map_err(|e| LoomError::invalid(format!("rng: {e}")))?;
            let snapshot = {
                let identity = identity_store_mut(loom)?;
                identity.set_passphrase(principal, passphrase, &salt)?;
                identity.clone()
            };
            let target = principal.to_string();
            let seq = loom.store().save_identity_store_audited(
                &snapshot,
                auth.principal,
                "identity.set_passphrase",
                Some(&target),
            )?;
            Ok(format!(
                "{{\"seq\":{seq},\"principal\":{}}}",
                json_string(&target)
            ))
        }))
    }

    pub fn force_detach_authority(
        &self,
        auth: &HostedAuth,
        principal: WorkspaceId,
        generation: u64,
        reason: String,
    ) -> HostedOutcome<String> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            authorize_admin(loom)?;
            let (detach, snapshot) = {
                let identity = identity_store_mut(loom)?;
                let detach = identity.force_detach_authority(principal, generation, reason)?;
                (detach, identity.clone())
            };
            let target = format!(
                "previous={};new={};generation={}",
                detach.previous_authority, detach.new_authority, detach.generation
            );
            let seq = loom.store().save_identity_store_audited(
                &snapshot,
                auth.principal,
                "identity.authority.force_detach",
                Some(&target),
            )?;
            Ok(format!(
                "{{\"seq\":{seq},\"detach\":{}}}",
                identity_authority_detach_json(&detach)
            ))
        }))
    }

    pub fn create_app_credential(
        &self,
        auth: &HostedAuth,
        principal: WorkspaceId,
        label: String,
    ) -> HostedOutcome<String> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            authorize_admin(loom)?;
            let id = random_workspace_id()?;
            let mut secret = [0u8; 32];
            getrandom::fill(&mut secret).map_err(|e| LoomError::invalid(format!("rng: {e}")))?;
            let mut salt = [0u8; 16];
            getrandom::fill(&mut salt).map_err(|e| LoomError::invalid(format!("rng: {e}")))?;
            let (credential, snapshot) = {
                let identity = identity_store_mut(loom)?;
                let credential =
                    identity.create_app_credential(principal, id, label, &secret, &salt)?;
                (credential, identity.clone())
            };
            let target = app_credential_target(principal, id);
            let seq = loom.store().save_identity_store_audited(
                &snapshot,
                auth.principal,
                "identity.app_credential.create",
                Some(&target),
            )?;
            let token = app_credential_token(id, &secret);
            Ok(format!(
                "{{\"seq\":{seq},\"credential\":{},\"secret\":{}}}",
                app_credential_json(&credential),
                json_string(&token)
            ))
        }))
    }

    pub fn revoke_app_credential(
        &self,
        auth: &HostedAuth,
        credential: WorkspaceId,
    ) -> HostedOutcome<String> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            authorize_admin(loom)?;
            let (credential, snapshot) = {
                let identity = identity_store_mut(loom)?;
                let credential = identity.revoke_app_credential(credential)?;
                (credential, identity.clone())
            };
            let target = app_credential_target(credential.principal, credential.id);
            let seq = loom.store().save_identity_store_audited(
                &snapshot,
                auth.principal,
                "identity.app_credential.revoke",
                Some(&target),
            )?;
            Ok(format!(
                "{{\"seq\":{seq},\"credential\":{}}}",
                app_credential_json(&credential)
            ))
        }))
    }

    pub fn create_external_credential(
        &self,
        auth: &HostedAuth,
        input: HostedExternalCredentialInput,
    ) -> HostedOutcome<String> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            authorize_admin(loom)?;
            let HostedExternalCredentialInput {
                principal,
                kind,
                label,
                issuer,
                subject,
                material_digest,
            } = input;
            let id = random_workspace_id()?;
            let (credential, snapshot) = {
                let identity = identity_store_mut(loom)?;
                let credential = identity.create_external_credential(
                    principal,
                    ExternalCredentialSpec {
                        id,
                        kind,
                        label,
                        issuer,
                        subject,
                        material_digest,
                    },
                )?;
                (credential, identity.clone())
            };
            let target = external_credential_target(principal, id);
            let seq = loom.store().save_identity_store_audited(
                &snapshot,
                auth.principal,
                "identity.external_credential.create",
                Some(&target),
            )?;
            Ok(format!(
                "{{\"seq\":{seq},\"credential\":{}}}",
                external_credential_json(&credential)
            ))
        }))
    }

    pub fn revoke_external_credential(
        &self,
        auth: &HostedAuth,
        credential: WorkspaceId,
    ) -> HostedOutcome<String> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            authorize_admin(loom)?;
            let (credential, snapshot) = {
                let identity = identity_store_mut(loom)?;
                let credential = identity.revoke_external_credential(credential)?;
                (credential, identity.clone())
            };
            let target = external_credential_target(credential.principal, credential.id);
            let seq = loom.store().save_identity_store_audited(
                &snapshot,
                auth.principal,
                "identity.external_credential.revoke",
                Some(&target),
            )?;
            Ok(format!(
                "{{\"seq\":{seq},\"credential\":{}}}",
                external_credential_json(&credential)
            ))
        }))
    }

    pub fn add_public_key(
        &self,
        auth: &HostedAuth,
        input: HostedPublicKeyInput,
    ) -> HostedOutcome<String> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            authorize_admin(loom)?;
            let HostedPublicKeyInput {
                id,
                principal,
                label,
                algorithm,
                public_key,
            } = input;
            let id = match id {
                Some(id) => id,
                None => random_workspace_id()?,
            };
            let (key, snapshot) = {
                let identity = identity_store_mut(loom)?;
                let key = identity.add_public_key(
                    principal,
                    IdentityPublicKeySpec {
                        id,
                        label,
                        algorithm,
                        public_key,
                    },
                )?;
                (key, identity.clone())
            };
            let target = public_key_target(principal, id);
            let seq = loom.store().save_identity_store_audited(
                &snapshot,
                auth.principal,
                "identity.public_key.add",
                Some(&target),
            )?;
            Ok(format!(
                "{{\"seq\":{seq},\"public_key\":{}}}",
                identity_public_key_json(&key)
            ))
        }))
    }

    pub fn revoke_public_key(&self, auth: &HostedAuth, key: WorkspaceId) -> HostedOutcome<String> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            authorize_admin(loom)?;
            let (key, snapshot) = {
                let identity = identity_store_mut(loom)?;
                let key = identity.revoke_public_key(key)?;
                (key, identity.clone())
            };
            let target = public_key_target(key.principal, key.id);
            let seq = loom.store().save_identity_store_audited(
                &snapshot,
                auth.principal,
                "identity.public_key.revoke",
                Some(&target),
            )?;
            Ok(format!(
                "{{\"seq\":{seq},\"public_key\":{}}}",
                identity_public_key_json(&key)
            ))
        }))
    }

    pub fn remove_principal(
        &self,
        auth: &HostedAuth,
        principal: WorkspaceId,
    ) -> HostedOutcome<String> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            authorize_admin(loom)?;
            let snapshot = {
                let identity = identity_store_mut(loom)?;
                identity.remove_principal(principal)?;
                identity.clone()
            };
            let target = principal.to_string();
            let seq = loom.store().save_identity_store_audited(
                &snapshot,
                auth.principal,
                "identity.remove_principal",
                Some(&target),
            )?;
            Ok(format!(
                "{{\"seq\":{seq},\"principal\":{}}}",
                json_string(&target)
            ))
        }))
    }

    pub fn assign_role(
        &self,
        auth: &HostedAuth,
        principal: WorkspaceId,
        role: WorkspaceId,
    ) -> HostedOutcome<String> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            authorize_admin(loom)?;
            let snapshot = {
                let identity = identity_store_mut(loom)?;
                identity.assign_role(principal, role)?;
                identity.clone()
            };
            let target = role_assignment_target(principal, role);
            let seq = loom.store().save_identity_store_audited(
                &snapshot,
                auth.principal,
                "identity.assign_role",
                Some(&target),
            )?;
            Ok(format!(
                "{{\"seq\":{seq},\"target\":{}}}",
                json_string(&target)
            ))
        }))
    }

    pub fn revoke_role(
        &self,
        auth: &HostedAuth,
        principal: WorkspaceId,
        role: WorkspaceId,
    ) -> HostedOutcome<String> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            authorize_admin(loom)?;
            let (removed, snapshot) = {
                let identity = identity_store_mut(loom)?;
                let removed = identity.revoke_role(principal, role)?;
                (removed, identity.clone())
            };
            let target = role_assignment_target(principal, role);
            let seq = if removed {
                Some(loom.store().save_identity_store_audited(
                    &snapshot,
                    auth.principal,
                    "identity.revoke_role",
                    Some(&target),
                )?)
            } else {
                None
            };
            let seq = seq.map_or_else(|| "null".to_string(), |seq| seq.to_string());
            Ok(format!(
                "{{\"seq\":{seq},\"removed\":{removed},\"target\":{}}}",
                json_string(&target)
            ))
        }))
    }

    pub fn grant_acl(
        &self,
        auth: &HostedAuth,
        input: HostedAclGrantInput,
    ) -> HostedOutcome<String> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            authorize_admin(loom)?;
            let grant = acl_grant_from_input(loom, input)?;
            let target = acl_grant_json(&grant);
            let snapshot = {
                let acl = loom.acl_store_mut();
                acl.grant(grant)?;
                acl.clone()
            };
            loom.store().save_acl_store_audited(
                &snapshot,
                auth.principal,
                "acl.grant",
                Some(&target),
            )?;
            Ok(format!("{{\"granted\":true,\"grant\":{target}}}"))
        }))
    }

    pub fn revoke_acl(
        &self,
        auth: &HostedAuth,
        input: HostedAclGrantInput,
    ) -> HostedOutcome<String> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            authorize_admin(loom)?;
            let grant = acl_grant_from_input(loom, input)?;
            let target = acl_grant_json(&grant);
            let (removed, snapshot) = {
                let acl = loom.acl_store_mut();
                let removed = acl.revoke(&grant);
                (removed, acl.clone())
            };
            if removed {
                loom.store().save_acl_store_audited(
                    &snapshot,
                    auth.principal,
                    "acl.revoke",
                    Some(&target),
                )?;
            }
            Ok(format!("{{\"removed\":{removed},\"grant\":{target}}}"))
        }))
    }

    pub fn set_listener_enabled(
        &self,
        auth: &HostedAuth,
        id: &str,
        enabled: bool,
    ) -> HostedOutcome<String> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            authorize_admin(loom)?;
            let mut record = loom
                .store()
                .served_listener(id)?
                .ok_or_else(|| loom_core::LoomError::not_found("served listener not found"))?;
            record.enabled = enabled;
            let action = if enabled {
                "serve.listener.enable"
            } else {
                "serve.listener.disable"
            };
            let seq = loom.store().save_served_listener_audited(
                &record,
                auth.principal,
                action,
                Some(&served_listener_target(&record)),
            )?;
            record.last_modified_audit_seq = Some(seq);
            Ok(served_listener_json(&record, Some(seq)))
        }))
    }

    pub fn remove_listener(&self, auth: &HostedAuth, id: &str) -> HostedOutcome<String> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            authorize_admin(loom)?;
            let record = loom
                .store()
                .served_listener(id)?
                .ok_or_else(|| loom_core::LoomError::not_found("served listener not found"))?;
            let seq = loom.store().remove_served_listener_audited(
                id,
                auth.principal,
                "serve.listener.remove",
                Some(&served_listener_target(&record)),
            )?;
            Ok(format!("{{\"seq\":{seq},\"id\":{}}}", json_string(id)))
        }))
    }

    pub fn web_routes_json(&self, auth: &HostedAuth, id: &str) -> HostedOutcome<String> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            authorize_admin(loom)?;
            let record = require_web_listener_record(loom, id)?;
            let listener = web_listener_from_record(loom, &record)?;
            let seq = loom.store().audit_append(
                auth.principal,
                "serve.web.route.list",
                Some(&format!("listener={id}")),
            )?;
            Ok(web_listener_json(&listener, Some(seq)))
        }))
    }

    pub fn set_web_route(
        &self,
        auth: &HostedAuth,
        id: &str,
        input: HostedWebRouteInput,
    ) -> HostedOutcome<String> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            authorize_admin(loom)?;
            let record = require_web_listener_record(loom, id)?;
            let mut listener = web_listener_from_record(loom, &record)?;
            let workspace = input
                .workspace
                .as_deref()
                .map(|workspace| resolve_ns(loom, workspace))
                .transpose()?;
            let mut route = WebRoute::new(
                input.route_id.clone(),
                vec![WebMethod::Get, WebMethod::Head],
                input.host_pattern,
                &input.path_prefix,
                &input.root_path,
                WebRouteMode::StaticFile,
            )?;
            route.workspace = workspace;
            listener
                .routes
                .routes
                .retain(|existing| existing.route_id != route.route_id);
            listener.routes.routes.push(route);
            listener.routes = WebRouteTable::new(listener.routes.routes)?;
            let seq = save_web_listener_config(
                loom,
                auth.principal,
                &listener,
                "serve.web.route.set",
                &format!("listener={id};route={}", input.route_id),
            )?;
            Ok(web_listener_json(&listener, Some(seq)))
        }))
    }

    pub fn remove_web_route(
        &self,
        auth: &HostedAuth,
        id: &str,
        route_id: &str,
    ) -> HostedOutcome<String> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            authorize_admin(loom)?;
            let record = require_web_listener_record(loom, id)?;
            let mut listener = web_listener_from_record(loom, &record)?;
            let before = listener.routes.routes.len();
            listener
                .routes
                .routes
                .retain(|route| route.route_id != route_id);
            if listener.routes.routes.len() == before {
                return Err(LoomError::not_found("web route not found"));
            }
            listener.routes = WebRouteTable::new(listener.routes.routes)?;
            let seq = save_web_listener_config(
                loom,
                auth.principal,
                &listener,
                "serve.web.route.remove",
                &format!("listener={id};route={route_id}"),
            )?;
            Ok(web_listener_json(&listener, Some(seq)))
        }))
    }

    pub fn protected_ref_list_json(
        &self,
        auth: &HostedAuth,
        workspace: &str,
    ) -> HostedOutcome<String> {
        hosted_outcome(self.kernel.read(auth, |loom| {
            let ns = resolve_ns(loom, workspace)?;
            Ok(protected_ref_policies_json(
                &loom.protected_ref_policies(ns)?,
            ))
        }))
    }

    pub fn protected_ref_get_json(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        ref_name: &str,
    ) -> HostedOutcome<String> {
        hosted_outcome(self.kernel.read(auth, |loom| {
            let ns = resolve_ns(loom, workspace)?;
            Ok(match loom.protected_ref_policy(ns, ref_name)? {
                Some(policy) => protected_ref_policy_json(ref_name, &policy),
                None => "null".to_string(),
            })
        }))
    }

    pub fn protected_ref_set(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        ref_name: &str,
        policy: ProtectedRefPolicy,
    ) -> HostedOutcome<String> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let actor = loom.effective_principal()?;
            let ns = resolve_ns(loom, workspace)?;
            let body = protected_ref_policy_json(ref_name, &policy);
            loom.set_protected_ref_policy(ns, ref_name, policy)?;
            let target = protected_ref_target(ns, ref_name);
            loom.store()
                .audit_append(actor, "protected_ref.set", Some(&target))?;
            Ok(body)
        }))
    }

    pub fn protected_ref_remove(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        ref_name: &str,
    ) -> HostedOutcome<String> {
        hosted_outcome(self.kernel.write(auth, |loom| {
            let actor = loom.effective_principal()?;
            let ns = resolve_ns(loom, workspace)?;
            let removed = loom.remove_protected_ref_policy(ns, ref_name)?;
            if removed {
                let target = protected_ref_target(ns, ref_name);
                loom.store()
                    .audit_append(actor, "protected_ref.remove", Some(&target))?;
            }
            Ok(format!("{{\"removed\":{removed}}}"))
        }))
    }
}

fn authorize_admin(loom: &Loom<FileStore>) -> loom_core::Result<()> {
    loom.authorize_global_admin()
}

fn identity_store(loom: &Loom<FileStore>) -> loom_core::Result<&IdentityStore> {
    loom.identity_store()
        .ok_or_else(|| LoomError::new(Code::NotFound, "identity store not initialized"))
}

fn identity_store_mut(loom: &mut Loom<FileStore>) -> loom_core::Result<&mut IdentityStore> {
    loom.identity_store_mut()
        .ok_or_else(|| LoomError::new(Code::NotFound, "identity store not initialized"))
}

fn role_assignment_target(principal: WorkspaceId, role: WorkspaceId) -> String {
    format!("principal={principal};role={role}")
}

fn app_credential_target(principal: WorkspaceId, credential: WorkspaceId) -> String {
    format!("principal={principal};credential={credential}")
}

fn external_credential_target(principal: WorkspaceId, credential: WorkspaceId) -> String {
    format!("principal={principal};credential={credential}")
}

fn public_key_target(principal: WorkspaceId, key: WorkspaceId) -> String {
    format!("principal={principal};key={key}")
}

fn random_workspace_id() -> loom_core::Result<WorkspaceId> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|e| LoomError::invalid(format!("rng: {e}")))?;
    Ok(WorkspaceId::v4_from_bytes(bytes))
}

fn acl_grant_from_input(
    loom: &Loom<FileStore>,
    input: HostedAclGrantInput,
) -> loom_core::Result<AclGrant> {
    let workspace = input
        .workspace
        .as_deref()
        .map(|selector| resolve_ns(loom, selector))
        .transpose()?;
    let scopes = if input.scopes.is_empty() {
        vec![AclScope::All]
    } else {
        input.scopes
    };
    Ok(AclGrant {
        subject: input.subject,
        workspace,
        domain: input.domain,
        ref_glob: input.ref_glob.filter(|value| !value.is_empty()),
        scopes,
        rights: input.rights,
        effect: input.effect,
        predicate: input.predicate,
    })
}

fn resolve_ns(loom: &Loom<FileStore>, selector: &str) -> loom_core::Result<WorkspaceId> {
    if let Ok(id) = WorkspaceId::parse(selector) {
        return Ok(id);
    }
    loom.registry()
        .open(&WsSelector::Name(selector.to_string()))
}

fn require_web_listener_record(
    loom: &Loom<FileStore>,
    listener: &str,
) -> loom_core::Result<ServedListenerRecord> {
    let record = loom
        .store()
        .served_listener(listener)?
        .ok_or_else(|| LoomError::not_found("served listener not found"))?;
    if record.surface != "web" || record.transport != "rest" {
        return Err(LoomError::invalid(
            "served listener is not a web rest listener",
        ));
    }
    if record.selectors.len() != 1 {
        return Err(LoomError::invalid(
            "web served listener must have exactly one workspace selector",
        ));
    }
    Ok(record)
}

fn web_listener_from_record(
    loom: &Loom<FileStore>,
    record: &ServedListenerRecord,
) -> loom_core::Result<WebListener> {
    let key = web_profile_listener_key(&record.id)?;
    if let Some(bytes) = loom.store().control_get(&key)? {
        return WebListener::decode(&bytes);
    }
    let workspace = resolve_ns(loom, &record.selectors[0])?;
    let addr = record
        .bind
        .parse::<std::net::SocketAddr>()
        .map_err(|e| LoomError::invalid(format!("invalid listener bind address: {e}")))?;
    WebListener::new(
        &record.id,
        addr.ip().to_string(),
        addr.port(),
        WebProtocol::Http,
        workspace,
        "/",
    )
}

fn save_web_listener_config(
    loom: &Loom<FileStore>,
    actor: Option<WorkspaceId>,
    listener: &WebListener,
    action: &str,
    target: &str,
) -> loom_core::Result<u64> {
    let key = web_profile_listener_key(&listener.listener_id)?;
    loom.store()
        .control_set_audited(&key, listener.encode()?, actor, action, Some(target))
}

fn protected_ref_target(ns: WorkspaceId, ref_name: &str) -> String {
    format!("workspace={ns};ref={ref_name}")
}

fn served_listeners_json(records: &[ServedListenerRecord]) -> String {
    let mut out = String::from("{\"listeners\":[");
    for (idx, record) in records.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&served_listener_json(record, None));
    }
    out.push_str("]}");
    out
}

fn web_listener_json(listener: &WebListener, seq: Option<u64>) -> String {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"seq\":");
    push_json_u64_option(&mut out, seq);
    out.push_str(",\"listener\":");
    out.push_str(&json_string(&listener.listener_id));
    out.push_str(",\"default_workspace\":");
    out.push_str(&json_string(&listener.default_workspace.to_string()));
    out.push_str(",\"root_path\":");
    out.push_str(&json_string(&listener.root_path));
    out.push_str(",\"routes\":[");
    for (idx, route) in listener.routes.routes.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&web_route_json(route));
    }
    out.push_str("]}");
    out
}

fn web_route_json(route: &WebRoute) -> String {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"route_id\":");
    out.push_str(&json_string(&route.route_id));
    out.push_str(",\"methods\":[");
    for (idx, method) in route.methods.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&json_string(web_method_name(*method)));
    }
    out.push_str("],\"host_pattern\":");
    push_json_option(&mut out, route.host_pattern.as_deref());
    out.push_str(",\"path_prefix\":");
    out.push_str(&json_string(&route.path_prefix));
    out.push_str(",\"workspace\":");
    match route.workspace {
        Some(workspace) => out.push_str(&json_string(&workspace.to_string())),
        None => out.push_str("null"),
    }
    out.push_str(",\"root_path\":");
    out.push_str(&json_string(&route.root_path));
    out.push_str(",\"mode\":");
    out.push_str(&json_string(web_route_mode_name(route.mode)));
    out.push('}');
    out
}

fn web_method_name(method: WebMethod) -> &'static str {
    match method {
        WebMethod::Get => "GET",
        WebMethod::Head => "HEAD",
        WebMethod::Post => "POST",
        WebMethod::Put => "PUT",
        WebMethod::Patch => "PATCH",
        WebMethod::Delete => "DELETE",
        WebMethod::Options => "OPTIONS",
    }
}

fn web_route_mode_name(mode: WebRouteMode) -> &'static str {
    match mode {
        WebRouteMode::StaticFile => "static-file",
        WebRouteMode::Presentation => "presentation",
        WebRouteMode::Program => "program",
        WebRouteMode::Redirect => "redirect",
        WebRouteMode::ReverseProxy => "reverse-proxy",
        WebRouteMode::Error => "error",
    }
}

fn network_access_policies_json(
    store: &FileStore,
    seq: u64,
    policies: &[NetworkAccessPolicyRecord],
    references: &BTreeMap<String, Vec<String>>,
) -> loom_core::Result<String> {
    let mut out = format!("{{\"seq\":{seq},\"policies\":[");
    for (idx, policy) in policies.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&network_access_policy_record_json(
            store,
            policy,
            network_access_references_for(references, &policy.name),
        )?);
    }
    out.push_str("]}");
    Ok(out)
}

fn network_access_policy_json(
    store: &FileStore,
    policy: &NetworkAccessPolicyRecord,
    seq: u64,
    references: &[String],
) -> loom_core::Result<String> {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"seq\":");
    out.push_str(&seq.to_string());
    out.push(',');
    out.push_str(&network_access_policy_record_json(store, policy, references)?[1..]);
    Ok(out)
}

fn network_access_policy_record_json(
    store: &FileStore,
    policy: &NetworkAccessPolicyRecord,
    references: &[String],
) -> loom_core::Result<String> {
    let digest = store.network_access_policy_digest(policy)?;
    let mut out = String::new();
    out.push('{');
    out.push_str("\"name\":");
    out.push_str(&json_string(&policy.name));
    out.push_str(",\"schema_version\":");
    out.push_str(&policy.schema_version.to_string());
    out.push_str(",\"digest\":");
    out.push_str(&json_string(&digest.to_string()));
    out.push_str(",\"description\":");
    push_json_option(&mut out, policy.description.as_deref());
    out.push_str(",\"default_action\":");
    out.push_str(&json_string(policy.default_action.as_str()));
    out.push_str(",\"created_audit_seq\":");
    push_json_u64_option(&mut out, policy.created_audit_seq);
    out.push_str(",\"updated_audit_seq\":");
    push_json_u64_option(&mut out, policy.updated_audit_seq);
    out.push_str(",\"references\":[");
    for (idx, reference) in references.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&json_string(reference));
    }
    out.push_str("],\"rules\":[");
    for (idx, rule) in policy.rules.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&network_access_rule_json(rule));
    }
    out.push_str("]}");
    Ok(out)
}

fn network_access_rule_json(rule: &NetworkAccessRule) -> String {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"id\":");
    out.push_str(&json_string(&rule.id));
    out.push_str(",\"action\":");
    out.push_str(&json_string(rule.action.as_str()));
    out.push_str(",\"source_cidr\":");
    push_json_option(
        &mut out,
        rule.source_cidr
            .as_ref()
            .map(|value| value.to_string())
            .as_deref(),
    );
    out.push_str(",\"trusted_proxy_cidr\":");
    push_json_option(
        &mut out,
        rule.trusted_proxy_cidr
            .as_ref()
            .map(|value| value.to_string())
            .as_deref(),
    );
    out.push_str(",\"require_mtls\":");
    out.push_str(if rule.require_mtls { "true" } else { "false" });
    out.push_str(",\"client_cert_subject\":");
    push_json_option(&mut out, rule.client_cert_subject.as_deref());
    out.push_str(",\"client_cert_san\":");
    push_json_option(&mut out, rule.client_cert_san.as_deref());
    out.push_str(",\"client_cert_issuer\":");
    push_json_option(&mut out, rule.client_cert_issuer.as_deref());
    out.push_str(",\"description\":");
    push_json_option(&mut out, rule.description.as_deref());
    out.push('}');
    out
}

fn acl_list_json(acl: &AclStore) -> String {
    let mut out = String::from("{\"grants\":[");
    for (idx, grant) in acl.grants().iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&acl_grant_json(grant));
    }
    out.push_str("]}");
    out
}

fn identity_list_json(identity: &IdentityStore) -> String {
    let mut out = String::from("{\"authenticated_mode\":");
    out.push_str(if identity.authenticated_mode() {
        "true"
    } else {
        "false"
    });
    out.push_str(",\"root\":");
    match identity.root_principal() {
        Some(root) => out.push_str(&json_string(&root.to_string())),
        None => out.push_str("null"),
    }
    out.push_str(",\"authority\":");
    out.push_str(&identity_authority_state_json(identity.authority_state()));
    out.push_str(",\"authority_handoffs\":[");
    for (idx, handoff) in identity.authority_handoffs().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&identity_authority_handoff_json(handoff));
    }
    out.push_str("],\"forced_detach\":");
    match identity.forced_detach() {
        Some(detach) => out.push_str(&identity_authority_detach_json(detach)),
        None => out.push_str("null"),
    }
    out.push_str(",\"principals\":[");
    for (idx, principal) in identity.principals().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&principal_json(principal));
    }
    out.push_str("],\"roles\":[");
    for (idx, role) in identity.roles().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&role_json(role));
    }
    out.push_str("],\"app_credentials\":[");
    for (idx, credential) in identity.app_credentials().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&app_credential_json(credential));
    }
    out.push_str("],\"external_credentials\":[");
    for (idx, credential) in identity.external_credentials().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&external_credential_json(credential));
    }
    out.push_str("],\"public_keys\":[");
    for (idx, key) in identity.public_keys().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&identity_public_key_json(key));
    }
    out.push_str("]}");
    out
}

fn identity_public_key_json(key: &loom_core::IdentityPublicKey) -> String {
    let mut out = String::from("{\"id\":");
    out.push_str(&json_string(&key.id.to_string()));
    out.push_str(",\"principal\":");
    out.push_str(&json_string(&key.principal.to_string()));
    out.push_str(",\"label\":");
    out.push_str(&json_string(&key.label));
    out.push_str(",\"algorithm\":");
    out.push_str(&json_string(&key.algorithm));
    out.push_str(",\"public_key_hex\":");
    out.push_str(&json_string(&hex_bytes(&key.public_key)));
    out.push_str(",\"enabled\":");
    out.push_str(if key.enabled { "true" } else { "false" });
    out.push('}');
    out
}

fn identity_authority_state_json(state: &loom_core::IdentityAuthorityState) -> String {
    let mut out = String::from("{\"mode\":");
    out.push_str(&json_string(identity_authority_mode_str(state.mode)));
    out.push_str(",\"authority\":");
    out.push_str(&json_string(&state.authority.to_string()));
    out.push_str(",\"generation\":");
    out.push_str(&state.generation.to_string());
    out.push_str(",\"head\":");
    match state.head {
        Some(head) => out.push_str(&json_string(&head.to_string())),
        None => out.push_str("null"),
    }
    out.push('}');
    out
}

fn identity_authority_handoff_json(handoff: &loom_core::IdentityAuthorityHandoff) -> String {
    let mut out = String::from("{\"from\":");
    out.push_str(&json_string(&handoff.from.to_string()));
    out.push_str(",\"to\":");
    out.push_str(&json_string(&handoff.to.to_string()));
    out.push_str(",\"generation\":");
    out.push_str(&handoff.generation.to_string());
    out.push_str(",\"head\":");
    match handoff.head {
        Some(head) => out.push_str(&json_string(&head.to_string())),
        None => out.push_str("null"),
    }
    out.push_str(",\"signed_record_hex\":");
    out.push_str(&json_string(&hex_bytes(&handoff.signed_record)));
    out.push('}');
    out
}

fn identity_authority_detach_json(detach: &loom_core::IdentityAuthorityDetach) -> String {
    let mut out = String::from("{\"previous_authority\":");
    out.push_str(&json_string(&detach.previous_authority.to_string()));
    out.push_str(",\"new_authority\":");
    out.push_str(&json_string(&detach.new_authority.to_string()));
    out.push_str(",\"generation\":");
    out.push_str(&detach.generation.to_string());
    out.push_str(",\"reason\":");
    out.push_str(&json_string(&detach.reason));
    out.push('}');
    out
}

fn identity_authority_witness_json(
    witness: &loom_core::IdentityAuthorityWitness,
    algo: loom_core::Algo,
) -> String {
    let record = witness.encode();
    let record_digest = witness.digest(algo);
    let mut out = String::from("{\"authority\":");
    out.push_str(&json_string(&witness.authority.to_string()));
    out.push_str(",\"mode\":");
    out.push_str(&json_string(identity_authority_mode_str(witness.mode)));
    out.push_str(",\"generation\":");
    out.push_str(&witness.generation.to_string());
    out.push_str(",\"head\":");
    match witness.head {
        Some(head) => out.push_str(&json_string(&head.to_string())),
        None => out.push_str("null"),
    }
    out.push_str(",\"snapshot_digest\":");
    out.push_str(&json_string(&witness.snapshot_digest.to_string()));
    out.push_str(",\"latest_handoff_digest\":");
    match witness.latest_handoff_digest {
        Some(digest) => out.push_str(&json_string(&digest.to_string())),
        None => out.push_str("null"),
    }
    out.push_str(",\"record_hex\":");
    out.push_str(&json_string(&hex_bytes(&record)));
    out.push_str(",\"record_digest\":");
    out.push_str(&json_string(&record_digest.to_string()));
    out.push('}');
    out
}

fn identity_authority_mode_str(mode: loom_core::IdentityAuthorityMode) -> &'static str {
    match mode {
        loom_core::IdentityAuthorityMode::Authority => "authority",
        loom_core::IdentityAuthorityMode::Mirror => "mirror",
        loom_core::IdentityAuthorityMode::Detached => "detached",
    }
}

fn principal_json(principal: &Principal) -> String {
    let mut out = String::from("{\"id\":");
    out.push_str(&json_string(&principal.id.to_string()));
    out.push_str(",\"name\":");
    out.push_str(&json_string(&principal.name));
    out.push_str(",\"kind\":");
    out.push_str(&json_string(principal_kind_str(principal.kind)));
    out.push_str(",\"enabled\":");
    out.push_str(if principal.enabled { "true" } else { "false" });
    out.push_str(",\"has_passphrase\":");
    out.push_str(if principal.has_passphrase {
        "true"
    } else {
        "false"
    });
    out.push_str(",\"roles\":[");
    for (idx, role) in principal.roles.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&json_string(&role.to_string()));
    }
    out.push_str("]}");
    out
}

fn role_json(role: &IdentityRole) -> String {
    let mut out = String::from("{\"id\":");
    out.push_str(&json_string(&role.id.to_string()));
    out.push_str(",\"name\":");
    out.push_str(&json_string(&role.name));
    out.push_str(",\"enabled\":");
    out.push_str(if role.enabled { "true" } else { "false" });
    out.push('}');
    out
}

fn app_credential_json(credential: &AppCredential) -> String {
    let mut out = String::from("{\"id\":");
    out.push_str(&json_string(&credential.id.to_string()));
    out.push_str(",\"principal\":");
    out.push_str(&json_string(&credential.principal.to_string()));
    out.push_str(",\"label\":");
    out.push_str(&json_string(&credential.label));
    out.push_str(",\"enabled\":");
    out.push_str(if credential.enabled { "true" } else { "false" });
    out.push('}');
    out
}

fn external_credential_json(credential: &ExternalCredential) -> String {
    let mut out = String::from("{\"id\":");
    out.push_str(&json_string(&credential.id.to_string()));
    out.push_str(",\"principal\":");
    out.push_str(&json_string(&credential.principal.to_string()));
    out.push_str(",\"kind\":");
    out.push_str(&json_string(credential.kind.as_str()));
    out.push_str(",\"label\":");
    out.push_str(&json_string(&credential.label));
    out.push_str(",\"issuer\":");
    out.push_str(&json_string(&credential.issuer));
    out.push_str(",\"subject\":");
    out.push_str(&json_string(&credential.subject));
    out.push_str(",\"material_digest\":");
    match credential.material_digest.as_deref() {
        Some(digest) => out.push_str(&json_string(digest)),
        None => out.push_str("null"),
    }
    out.push_str(",\"enabled\":");
    out.push_str(if credential.enabled { "true" } else { "false" });
    out.push('}');
    out
}

fn principal_kind_str(kind: PrincipalKind) -> &'static str {
    match kind {
        PrincipalKind::Root => "root",
        PrincipalKind::User => "user",
        PrincipalKind::Service => "service",
    }
}

fn acl_grant_json(grant: &AclGrant) -> String {
    let mut out = String::from("{\"effect\":");
    out.push_str(&json_string(acl_effect_str(grant.effect)));
    out.push_str(",\"subject\":");
    match grant.subject {
        AclSubject::Everyone => out.push_str(&json_string("*")),
        AclSubject::Principal(principal) => out.push_str(&json_string(&principal.to_string())),
        AclSubject::Role(role) => out.push_str(&json_string(&format!("role:{role}"))),
    }
    out.push_str(",\"subject_kind\":");
    match grant.subject {
        AclSubject::Everyone => out.push_str(&json_string("everyone")),
        AclSubject::Principal(_) => out.push_str(&json_string("principal")),
        AclSubject::Role(_) => out.push_str(&json_string("role")),
    }
    out.push_str(",\"workspace\":");
    match grant.workspace {
        Some(ns) => out.push_str(&json_string(&ns.to_string())),
        None => out.push_str("null"),
    }
    out.push_str(",\"domain\":");
    match grant.domain {
        Some(domain) => out.push_str(&json_string(domain.as_str())),
        None => out.push_str("null"),
    }
    out.push_str(",\"ref_glob\":");
    push_json_option(&mut out, grant.ref_glob.as_deref());
    out.push_str(",\"rights\":[");
    for (idx, right) in grant.rights.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&json_string(acl_right_str(*right)));
    }
    out.push_str("],\"scopes\":[");
    for (idx, scope) in grant.scopes.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&acl_scope_json(scope));
    }
    out.push_str("],\"predicate\":");
    out.push_str(&acl_predicate_json(grant.predicate.as_ref()));
    out.push('}');
    out
}

fn acl_predicate_json(predicate: Option<&loom_core::AclPredicate>) -> String {
    match predicate {
        None => String::from("null"),
        Some(predicate) => {
            let mut out = String::from("{\"language\":");
            out.push_str(&json_string(&predicate.language));
            out.push_str(",\"expression\":");
            out.push_str(&json_string(&predicate.expression));
            out.push('}');
            out
        }
    }
}

fn acl_effect_str(effect: AclEffect) -> &'static str {
    match effect {
        AclEffect::Allow => "allow",
        AclEffect::Deny => "deny",
    }
}

fn acl_right_str(right: AclRight) -> &'static str {
    match right {
        AclRight::Read => "read",
        AclRight::Write => "write",
        AclRight::Advance => "advance",
        AclRight::Merge => "merge",
        AclRight::Execute => "execute",
        AclRight::Admin => "admin",
    }
}

fn acl_scope_json(scope: &AclScope) -> String {
    match scope {
        AclScope::All => String::from("{\"kind\":\"all\"}"),
        AclScope::Prefix { kind, prefix } => {
            let mut out = String::from("{\"kind\":");
            out.push_str(&json_string(acl_scope_kind_str(*kind)));
            out.push_str(",\"prefix_hex\":");
            out.push_str(&json_string(&hex_bytes(prefix)));
            out.push('}');
            out
        }
    }
}

fn acl_scope_kind_str(kind: AclScopeKind) -> &'static str {
    match kind {
        AclScopeKind::Ref => "ref",
        AclScopeKind::Collection => "collection",
        AclScopeKind::Path => "path",
        AclScopeKind::Key => "key",
        AclScopeKind::Table => "table",
        AclScopeKind::Exec => "exec",
    }
}

fn protected_ref_policy_json(ref_name: &str, policy: &ProtectedRefPolicy) -> String {
    let mut out = String::from("{\"ref\":");
    out.push_str(&json_string(ref_name));
    out.push_str(",\"fast_forward_only\":");
    out.push_str(if policy.fast_forward_only {
        "true"
    } else {
        "false"
    });
    out.push_str(",\"signed_commits_required\":");
    out.push_str(if policy.signed_commits_required {
        "true"
    } else {
        "false"
    });
    out.push_str(",\"signed_ref_advance_required\":");
    out.push_str(if policy.signed_ref_advance_required {
        "true"
    } else {
        "false"
    });
    out.push_str(",\"required_review_count\":");
    out.push_str(&policy.required_review_count.to_string());
    out.push_str(",\"retention_lock\":");
    out.push_str(if policy.retention_lock {
        "true"
    } else {
        "false"
    });
    out.push_str(",\"governance_lock\":");
    out.push_str(if policy.governance_lock {
        "true"
    } else {
        "false"
    });
    out.push('}');
    out
}

fn protected_ref_policies_json(policies: &[(String, ProtectedRefPolicy)]) -> String {
    let mut out = String::from("{\"policies\":[");
    for (idx, (ref_name, policy)) in policies.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&protected_ref_policy_json(ref_name, policy));
    }
    out.push_str("]}");
    out
}

fn served_listener_json(record: &ServedListenerRecord, seq: Option<u64>) -> String {
    let mut out = String::from("{");
    if let Some(seq) = seq {
        out.push_str("\"seq\":");
        out.push_str(&seq.to_string());
        out.push(',');
    }
    out.push_str("\"id\":");
    out.push_str(&json_string(&record.id));
    out.push_str(",\"schema_version\":");
    out.push_str(&record.schema_version.to_string());
    out.push_str(",\"surface\":");
    out.push_str(&json_string(&record.surface));
    out.push_str(",\"selectors\":[");
    for (idx, selector) in record.selectors.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&json_string(selector));
    }
    out.push_str("],\"transport\":");
    out.push_str(&json_string(&record.transport));
    out.push_str(",\"bind\":");
    out.push_str(&json_string(&record.bind));
    out.push_str(",\"enabled\":");
    out.push_str(if record.enabled { "true" } else { "false" });
    out.push_str(",\"route_scope\":");
    out.push_str(&json_string(&record.route_scope));
    out.push_str(",\"exposure\":");
    out.push_str(&json_string(&record.exposure));
    out.push_str(",\"network_access_policy\":");
    push_json_option(&mut out, record.network_access_policy_ref.as_deref());
    out.push_str(",\"last_modified_audit_seq\":");
    match record.last_modified_audit_seq {
        Some(seq) => out.push_str(&seq.to_string()),
        None => out.push_str("null"),
    }
    out.push('}');
    out
}

fn audit_records_json(records: &[AuditRecord]) -> String {
    let mut out = String::from("{\"records\":[");
    for (idx, record) in records.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str("{\"seq\":");
        out.push_str(&record.seq.to_string());
        out.push_str(",\"principal\":");
        push_principal(&mut out, record.principal);
        out.push_str(",\"hash\":");
        out.push_str(&json_string(&record.hash.to_string()));
        out.push_str(",\"prev_hash\":");
        match record.prev_hash {
            Some(hash) => out.push_str(&json_string(&hash.to_string())),
            None => out.push_str("null"),
        }
        out.push_str(",\"action\":");
        out.push_str(&json_string(&record.action));
        out.push_str(",\"target\":");
        push_json_option(&mut out, record.target.as_deref());
        out.push('}');
    }
    out.push_str("]}");
    out
}

fn audit_config_json(config: AuditConfig) -> String {
    format!(
        "{{\"retention_days\":{},\"legal_hold\":{}}}",
        config.retention_days, config.legal_hold
    )
}

fn audit_prune_stats_json(stats: AuditPruneStats) -> String {
    let checkpoint_seq = stats
        .checkpoint_seq
        .map_or_else(|| "null".to_string(), |value| value.to_string());
    let checkpoint_hash = stats.checkpoint_hash.map_or_else(
        || "null".to_string(),
        |value| json_string(&value.to_string()),
    );
    format!(
        "{{\"pruned\":{},\"checkpoint_seq\":{},\"checkpoint_hash\":{},\"audit_seq\":{}}}",
        stats.pruned, checkpoint_seq, checkpoint_hash, stats.audit_seq
    )
}

fn served_listener_target(record: &ServedListenerRecord) -> String {
    format!(
        "id={};surface={};transport={};bind={};enabled={}",
        record.id, record.surface, record.transport, record.bind, record.enabled
    )
}

fn network_access_served_listener_references(
    store: &FileStore,
    name: &str,
) -> loom_core::Result<Vec<String>> {
    Ok(network_access_served_listener_reference_map(store)?
        .remove(name)
        .unwrap_or_default())
}

fn network_access_served_listener_reference_map(
    store: &FileStore,
) -> loom_core::Result<BTreeMap<String, Vec<String>>> {
    let mut references = BTreeMap::<String, Vec<String>>::new();
    for record in store.served_listeners()? {
        if let Some(name) = record.network_access_policy_ref.as_deref() {
            references
                .entry(name.to_string())
                .or_default()
                .push(record.id.clone());
        }
    }
    for listeners in references.values_mut() {
        listeners.sort();
    }
    Ok(references)
}

fn network_access_references_for<'a>(
    references: &'a BTreeMap<String, Vec<String>>,
    name: &str,
) -> &'a [String] {
    references.get(name).map(Vec::as_slice).unwrap_or(&[])
}

fn network_access_policy_target(name: &str) -> String {
    format!("network-access;name={name}")
}

fn network_access_denied_remove_target(name: &str, references: &[String]) -> String {
    let mut target = network_access_policy_target(name);
    target.push_str(";served_listener_count=");
    target.push_str(&references.len().to_string());
    target.push_str(";served_listeners=");
    target.push_str(&references.join(","));
    target
}

fn push_principal(out: &mut String, principal: Option<WorkspaceId>) {
    match principal {
        Some(principal) => out.push_str(&json_string(&principal.to_string())),
        None => out.push_str("null"),
    }
}

fn push_json_option(out: &mut String, value: Option<&str>) {
    match value {
        Some(value) => out.push_str(&json_string(value)),
        None => out.push_str("null"),
    }
}

fn push_json_u64_option(out: &mut String, value: Option<u64>) {
    match value {
        Some(value) => out.push_str(&value.to_string()),
        None => out.push_str("null"),
    }
}

fn hex_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn json_string(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use loom_core::{PrincipalKind, ROLE_READER_ID};

    use crate::test_support::{init, nid, temp_path};
    use crate::{HostedAuth, HostedKernel};

    #[test]
    fn hosted_admin_identity_management_round_trips() {
        let path = temp_path("admin-identity");
        init(&path, None);
        let kernel = HostedKernel::new(&path);
        let auth = HostedAuth::passphrase(nid(1), "root-pass", "admin-identity");
        let user = nid(77);

        let identity = kernel.admin().identity_json(&auth).unwrap();
        assert!(identity.contains("\"name\":\"root\""), "{identity}");
        assert!(identity.contains("\"roles\""), "{identity}");
        assert!(identity.contains("\"authority\""), "{identity}");
        assert!(identity.contains("\"authority_handoffs\""), "{identity}");

        let add = kernel
            .admin()
            .add_principal(&auth, user, "alice".to_string(), PrincipalKind::User)
            .unwrap();
        assert!(add.contains("\"name\":\"alice\""), "{add}");

        let passphrase = kernel
            .admin()
            .set_principal_passphrase(&auth, user, "alice-pass")
            .unwrap();
        assert!(passphrase.contains(&user.to_string()), "{passphrase}");

        let assign = kernel
            .admin()
            .assign_role(&auth, user, ROLE_READER_ID)
            .unwrap();
        assert!(assign.contains(&ROLE_READER_ID.to_string()), "{assign}");

        let identity = kernel.admin().identity_json(&auth).unwrap();
        assert!(identity.contains("\"name\":\"alice\""), "{identity}");
        assert!(identity.contains(&ROLE_READER_ID.to_string()), "{identity}");

        let detach = kernel
            .admin()
            .force_detach_authority(&auth, user, 1, "authority unavailable".to_string())
            .unwrap();
        assert!(
            detach.contains("\"reason\":\"authority unavailable\""),
            "{detach}"
        );
        let identity = kernel.admin().identity_json(&auth).unwrap();
        assert!(identity.contains("\"mode\":\"detached\""), "{identity}");
        assert!(identity.contains("\"forced_detach\""), "{identity}");

        let credential = kernel
            .admin()
            .create_app_credential(&auth, user, "pinecone".to_string())
            .unwrap();
        assert!(
            credential.contains("\"label\":\"pinecone\""),
            "{credential}"
        );
        assert!(
            credential.contains("\"secret\":\"loom_app_"),
            "{credential}"
        );
        let credential_id = json_field(&credential, "id");
        let secret = json_field(&credential, "secret");
        let app_principal = kernel
            .read(
                &HostedAuth::app_credential(&secret, "admin-app-key"),
                |loom| loom.effective_principal(),
            )
            .unwrap();
        assert_eq!(app_principal, Some(user));

        let identity = kernel.admin().identity_json(&auth).unwrap();
        assert!(identity.contains("\"app_credentials\""), "{identity}");
        assert!(identity.contains("\"label\":\"pinecone\""), "{identity}");
        assert!(!identity.contains(&secret), "{identity}");

        let credential_id = loom_core::WorkspaceId::parse(&credential_id).unwrap();
        let revoked = kernel
            .admin()
            .revoke_app_credential(&auth, credential_id)
            .unwrap();
        assert!(revoked.contains("\"label\":\"pinecone\""), "{revoked}");
        let err = kernel
            .read(
                &HostedAuth::app_credential(secret, "revoked-app-key"),
                |loom| loom.effective_principal(),
            )
            .unwrap_err();
        assert_eq!(err.code, loom_core::Code::AuthenticationFailed);

        let revoke = kernel
            .admin()
            .revoke_role(&auth, user, ROLE_READER_ID)
            .unwrap();
        assert!(revoke.contains("\"removed\":true"), "{revoke}");

        let remove = kernel.admin().remove_principal(&auth, user).unwrap();
        assert!(remove.contains(&user.to_string()), "{remove}");

        std::fs::remove_file(path).unwrap();
    }

    fn json_field(body: &str, field: &str) -> String {
        let marker = format!("\"{field}\":\"");
        let start = body.find(&marker).unwrap() + marker.len();
        let end = body[start..].find('"').unwrap() + start;
        body[start..end].to_string()
    }
}
