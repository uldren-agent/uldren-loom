//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

pub(crate) fn run_management(action: ManagementCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        ManagementCmd::Workspace { action } => run_management_workspace(action, keys),
        ManagementCmd::Identity { action } => run_identity(action, keys),
        ManagementCmd::Acl { action } => run_acl(action, keys),
        ManagementCmd::Kv { action } => run_management_kv(action, keys),
        ManagementCmd::ProtectedRef { action } => run_protected_ref(action, keys),
    }
}

pub(crate) fn run_management_workspace(action: WorkspaceCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        WorkspaceCmd::Create { store, name, facet } => {
            let client = crate::remote::open_store_client(&store)?;
            let id = client.ws_create(keys, &name, facet.as_deref())?;
            println!("{id}\t{name}");
            Ok(())
        }
        WorkspaceCmd::List { store } => {
            let client = crate::remote::open_store_client(&store)?;
            let infos = client.ws_list(keys)?;
            crate::helpers::print_workspaces_infos(&infos);
            Ok(())
        }
        WorkspaceCmd::Rename {
            store,
            workspace,
            new_name,
        } => {
            let client = crate::remote::open_store_client(&store)?;
            let ns = client.ws_rename(keys, &workspace, &new_name)?;
            println!("{ns}\t{new_name}");
            Ok(())
        }
        WorkspaceCmd::Delete { store, workspace } => {
            let client = crate::remote::open_store_client(&store)?;
            let ns = client.ws_delete(keys, &workspace)?;
            println!("{ns}");
            Ok(())
        }
    }
}

pub(crate) fn run_identity(action: IdentityCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        IdentityCmd::List { store } => {
            let client = crate::remote::open_store_client(&store)?;
            println!("{}", client.id_list(keys)?);
            Ok(())
        }
        IdentityCmd::Add {
            store,
            handle,
            name,
            kind,
        } => {
            let kind = parse_principal_kind(&kind)?;
            let client = crate::remote::open_store_client(&store)?;
            let id = client.id_add(keys, &handle, &name, kind)?;
            println!("{id}");
            Ok(())
        }
        IdentityCmd::RenameHandle {
            store,
            principal,
            handle,
        } => {
            let client = crate::remote::open_store_client(&store)?;
            let principal = client.id_rename_handle(keys, &principal, &handle)?;
            println!("{principal}");
            Ok(())
        }
        IdentityCmd::SetPassphrase {
            store,
            principal,
            new_key_source,
        } => {
            let new_source = resolve_new_key_source(new_key_source.as_deref(), keys)?;
            let passphrase = acquire(&new_source, "Principal passphrase", true)?;
            let client = crate::remote::open_store_client(&store)?;
            client.id_set_passphrase(keys, &principal, passphrase.as_bytes())?;
            Ok(())
        }
        IdentityCmd::CreateAppCredential {
            store,
            principal,
            label,
        } => {
            let principal = WorkspaceId::parse(&principal).map_err(|e| e.to_string())?;
            let client = crate::remote::open_store_client(&store)?;
            println!(
                "{}",
                client.id_app_credential_create(keys, principal, label)?
            );
            Ok(())
        }
        IdentityCmd::RevokeAppCredential { store, credential } => {
            let id = WorkspaceId::parse(&credential).map_err(|e| e.to_string())?;
            let client = crate::remote::open_store_client(&store)?;
            println!("{}", client.id_app_credential_revoke(keys, id)?);
            Ok(())
        }
        IdentityCmd::CreateExternalCredential {
            store,
            principal,
            kind,
            label,
            issuer,
            subject,
            material_digest,
        } => {
            let principal = WorkspaceId::parse(&principal).map_err(|e| e.to_string())?;
            let kind = ExternalCredentialKind::parse(&kind).map_err(|e| e.to_string())?;
            let client = crate::remote::open_store_client(&store)?;
            println!(
                "{}",
                client.id_external_credential_create(
                    keys,
                    principal,
                    kind,
                    label,
                    issuer,
                    subject,
                    material_digest,
                )?
            );
            Ok(())
        }
        IdentityCmd::RevokeExternalCredential { store, credential } => {
            let id = WorkspaceId::parse(&credential).map_err(|e| e.to_string())?;
            let client = crate::remote::open_store_client(&store)?;
            println!("{}", client.id_external_credential_revoke(keys, id)?);
            Ok(())
        }
        IdentityCmd::PublicKey { action } => run_identity_public_key(action, keys),
        IdentityCmd::ForceDetachAuthority {
            store,
            principal,
            generation,
            reason,
        } => {
            crate::locator_cx::current().require_local_admin(&store)?;
            let mut loom = cli_open_loom(&store, keys)?;
            let actor = require_global_admin_actor(&loom)?;
            let principal = WorkspaceId::parse(&principal).map_err(|e| e.to_string())?;
            let (detach, snapshot) = {
                let identity = loom
                    .identity_store_mut()
                    .ok_or_else(|| "identity store not initialized".to_string())?;
                identity.principal(principal).map_err(|e| e.to_string())?;
                let detach = identity
                    .force_detach_authority(principal, generation, reason)
                    .map_err(|e| e.to_string())?;
                (detach, identity.clone())
            };
            let target = format!(
                "previous_authority={};new_authority={};generation={}",
                detach.previous_authority, detach.new_authority, detach.generation
            );
            let seq = loom
                .store()
                .save_identity_store_audited(
                    &snapshot,
                    Some(actor),
                    "identity.authority.force_detach",
                    Some(&target),
                )
                .map_err(|e| e.to_string())?;
            println!(
                "{{\"seq\":{seq},\"detach\":{}}}",
                identity_authority_detach_json(&detach)
            );
            Ok(())
        }
        IdentityCmd::AuthorityWitness { store } => {
            crate::locator_cx::current().require_local_admin(&store)?;
            let loom = cli_open_loom_read(&store, keys)?;
            require_global_admin(&loom)?;
            let identity = loom
                .identity_store()
                .ok_or_else(|| "identity store not initialized".to_string())?;
            let algo = loom.store().digest_algo();
            let witness = identity.authority_witness(algo);
            println!("{}", identity_authority_witness_json(&witness, algo));
            Ok(())
        }
        IdentityCmd::ReplicateAuthority {
            store,
            source,
            become_authority,
        } => {
            crate::locator_cx::current().require_local_admin(&source)?;
            crate::locator_cx::current().require_local_admin(&store)?;
            let source_loom = cli_open_loom_read(&source, keys)?;
            require_global_admin(&source_loom)?;
            let source_identity = source_loom
                .identity_store()
                .ok_or_else(|| "source identity store not initialized".to_string())?
                .clone();
            let mut destination_loom = cli_open_loom(&store, keys)?;
            let actor = require_global_admin_actor(&destination_loom)?;
            let algo = destination_loom.store().digest_algo();
            let (report, snapshot) = {
                let destination_identity = destination_loom
                    .identity_store_mut()
                    .ok_or_else(|| "destination identity store not initialized".to_string())?;
                let report = destination_identity
                    .replicate_authority_from(&source_identity, algo, become_authority)
                    .map_err(|e| e.to_string())?;
                (report, destination_identity.clone())
            };
            let target = format!(
                "source={source};from_generation={};to_generation={};applied={}",
                report.from_generation, report.to_generation, report.applied
            );
            let seq = destination_loom
                .store()
                .save_identity_store_audited(
                    &snapshot,
                    Some(actor),
                    "identity.authority.replicate",
                    Some(&target),
                )
                .map_err(|e| e.to_string())?;
            println!(
                "{}",
                identity_authority_sync_report_json(&report, algo, seq)
            );
            Ok(())
        }
        IdentityCmd::ConfigureAuthorityReplication {
            store,
            id,
            source,
            disabled,
            pull_on_start,
            interval_ms,
            jitter_ms,
            backoff_ms,
            publish_witness,
        } => {
            crate::locator_cx::current().require_local_admin(&store)?;
            let loom = cli_open_loom(&store, keys)?;
            let actor = require_global_admin_actor(&loom)?;
            let mut policy = FileStore::authority_replication_policy(&id, &source, !disabled)
                .map_err(|e| e.to_string())?;
            policy.pull_on_start = pull_on_start;
            policy.interval_ms = interval_ms;
            policy.jitter_ms = jitter_ms;
            policy.backoff_ms = backoff_ms;
            policy.publish_witness = publish_witness;
            let target = format!("id={id};source={source}");
            let seq = loom
                .store()
                .save_authority_replication_policy_audited(
                    &policy,
                    Some(actor),
                    "authority.replication.configure",
                    Some(&target),
                )
                .map_err(|e| e.to_string())?;
            let stored = loom
                .store()
                .authority_replication_policy_by_id(&id)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "authority replication policy not found after save".to_string())?;
            println!(
                "{{\"seq\":{seq},\"policy\":{}}}",
                authority_replication_policy_json(&stored)
            );
            Ok(())
        }
        IdentityCmd::ListAuthorityReplication { store } => {
            crate::locator_cx::current().require_local_admin(&store)?;
            let loom = cli_open_loom_read(&store, keys)?;
            require_global_admin(&loom)?;
            let policies = loom
                .store()
                .authority_replication_policies()
                .map_err(|e| e.to_string())?;
            println!("{}", authority_replication_policies_json(&policies));
            Ok(())
        }
        IdentityCmd::RemoveAuthorityReplication { store, id } => {
            crate::locator_cx::current().require_local_admin(&store)?;
            let loom = cli_open_loom(&store, keys)?;
            let actor = require_global_admin_actor(&loom)?;
            let target = format!("id={id}");
            let seq = loom
                .store()
                .remove_authority_replication_policy_audited(
                    &id,
                    Some(actor),
                    "authority.replication.remove",
                    Some(&target),
                )
                .map_err(|e| e.to_string())?;
            println!("{{\"seq\":{seq},\"id\":{}}}", json_string(&id));
            Ok(())
        }
        IdentityCmd::Remove { store, principal } => {
            let client = crate::remote::open_store_client(&store)?;
            client.id_remove(keys, &principal)?;
            Ok(())
        }
        IdentityCmd::AssignRole {
            store,
            principal,
            role,
        } => {
            let client = crate::remote::open_store_client(&store)?;
            client.id_assign_role(keys, &principal, &role)?;
            Ok(())
        }
        IdentityCmd::RevokeRole {
            store,
            principal,
            role,
        } => {
            let client = crate::remote::open_store_client(&store)?;
            let removed = client.id_revoke_role(keys, &principal, &role)?;
            println!("{removed}");
            Ok(())
        }
    }
}

fn run_identity_public_key(action: IdentityPublicKeyCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        IdentityPublicKeyCmd::Add {
            store,
            principal,
            label,
            algorithm,
            public_key_hex,
        } => {
            let principal = WorkspaceId::parse(&principal).map_err(|e| e.to_string())?;
            let public_key = decode_hex_arg(&public_key_hex)?;
            let client = crate::remote::open_store_client(&store)?;
            println!(
                "{}",
                client.id_add_public_key(keys, principal, label, algorithm, public_key)?
            );
            Ok(())
        }
        IdentityPublicKeyCmd::List { store } => {
            let client = crate::remote::open_store_client(&store)?;
            println!("{}", client.id_public_key_list(keys)?);
            Ok(())
        }
        IdentityPublicKeyCmd::Revoke { store, key } => {
            let key = WorkspaceId::parse(&key).map_err(|e| e.to_string())?;
            let client = crate::remote::open_store_client(&store)?;
            println!("{}", client.id_revoke_public_key(keys, key)?);
            Ok(())
        }
    }
}

fn decode_hex_arg(value: &str) -> Result<Vec<u8>, String> {
    let value = value.strip_prefix("0x").unwrap_or(value);
    if !value.len().is_multiple_of(2) {
        return Err("hex input must have an even number of digits".to_string());
    }
    let mut out = Vec::with_capacity(value.len() / 2);
    for pair in value.as_bytes().chunks_exact(2) {
        let high = hex_nibble(pair[0])?;
        let low = hex_nibble(pair[1])?;
        out.push((high << 4) | low);
    }
    Ok(out)
}

fn hex_nibble(byte: u8) -> Result<u8, String> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err("hex input contains a non-hex digit".to_string()),
    }
}

pub(crate) fn run_acl(action: AclCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        AclCmd::List { store } => {
            let client = crate::remote::open_store_client(&store)?;
            let grants = client.acl_list(keys)?;
            println!("{}", acl_grants_json(&grants));
            Ok(())
        }
        AclCmd::Grant {
            store,
            effect,
            subject,
            rights,
            workspace,
            domain,
            ref_glob,
            scopes,
            predicate_cel,
        } => {
            let client = crate::remote::open_store_client(&store)?;
            client.acl_grant(
                keys,
                AclGrantArgs {
                    effect: &effect,
                    subject: &subject,
                    workspace: workspace.as_deref(),
                    domain: domain.as_deref(),
                    rights: &rights,
                    ref_glob: ref_glob.as_deref(),
                    scopes: &scopes,
                    predicate_cel: predicate_cel.as_deref(),
                },
            )
        }
        AclCmd::Revoke {
            store,
            effect,
            subject,
            rights,
            workspace,
            domain,
            ref_glob,
            scopes,
            predicate_cel,
        } => {
            let client = crate::remote::open_store_client(&store)?;
            let removed = client.acl_revoke(
                keys,
                AclGrantArgs {
                    effect: &effect,
                    subject: &subject,
                    workspace: workspace.as_deref(),
                    domain: domain.as_deref(),
                    rights: &rights,
                    ref_glob: ref_glob.as_deref(),
                    scopes: &scopes,
                    predicate_cel: predicate_cel.as_deref(),
                },
            )?;
            println!("{}", if removed { "removed" } else { "not-found" });
            Ok(())
        }
    }
}

pub(crate) fn run_protected_ref(action: ProtectedRefCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        ProtectedRefCmd::List { store, workspace } => {
            let client = crate::remote::open_store_client(&store)?;
            let policies = client.pr_list(keys, &workspace)?;
            println!("{}", protected_ref_policies_json(&policies));
            Ok(())
        }
        ProtectedRefCmd::Get {
            store,
            workspace,
            ref_name,
        } => {
            let client = crate::remote::open_store_client(&store)?;
            match client.pr_get(keys, &workspace, &ref_name)? {
                Some(policy) => println!("{}", protected_ref_policy_json(&ref_name, &policy)),
                None => println!("null"),
            }
            Ok(())
        }
        ProtectedRefCmd::Set {
            store,
            workspace,
            ref_name,
            fast_forward_only,
            signed_commits_required,
            signed_ref_advance_required,
            required_review_count,
            retention_lock,
            governance_lock,
        } => {
            let policy = ProtectedRefPolicy {
                fast_forward_only,
                signed_commits_required,
                signed_ref_advance_required,
                required_review_count,
                retention_lock,
                governance_lock,
            };
            let client = crate::remote::open_store_client(&store)?;
            client.pr_set(keys, &workspace, &ref_name, policy)
        }
        ProtectedRefCmd::Remove {
            store,
            workspace,
            ref_name,
        } => {
            let client = crate::remote::open_store_client(&store)?;
            let removed = client.pr_remove(keys, &workspace, &ref_name)?;
            println!("{removed}");
            Ok(())
        }
    }
}

pub(crate) fn run_management_kv(action: ManagementKvCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        ManagementKvCmd::Config { action } => run_management_kv_config(action, keys),
    }
}

pub(crate) fn run_management_kv_config(
    action: ManagementKvConfigCmd,
    keys: &KeyOpts,
) -> Result<(), String> {
    match action {
        ManagementKvConfigCmd::Set {
            store,
            workspace,
            name,
            tier,
            default_ttl_ms,
            default_idle_ttl_ms,
            read_through,
            write_through,
        } => {
            let mut loom = cli_open_loom(&store, keys)?;
            let ns = ensure_kv_workspace(&mut loom, &workspace)?;
            let actor = loom.effective_principal().map_err(|e| e.to_string())?;
            let config = KvMapConfig {
                tier: parse_kv_tier(&tier)?,
                default_put: EphemeralPutOptions {
                    ttl_ms: (default_ttl_ms != 0).then_some(default_ttl_ms),
                    idle_ttl_ms: (default_idle_ttl_ms != 0).then_some(default_idle_ttl_ms),
                },
                read_through,
                write_through,
                max_entries: None,
                max_bytes: None,
                eviction: loom_core::EvictionPolicy::None,
                on_evict: loom_core::OnEvict::Drop,
                write_behind: false,
                write_around: false,
                back_pressure: loom_core::BackPressure::Block,
                flush_high_water_pct: None,
                flush_batch: None,
            };
            loom.configure_kv_map(ns, &name, config)
                .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            let target = format!("workspace={ns};collection={name}");
            loom.store()
                .audit_append(actor, "management.kv.set_config", Some(&target))
                .map_err(|e| e.to_string())?;
            Ok(())
        }
        ManagementKvConfigCmd::Get {
            store,
            workspace,
            name,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let ns = resolve_ns(&loom, &workspace)?;
            loom.authorize(ns, FacetKind::Kv, AclRight::Admin)
                .map_err(|e| e.to_string())?;
            println!("{}", kv_map_config_json(loom.kv_map_config(ns, &name)));
            Ok(())
        }
    }
}
