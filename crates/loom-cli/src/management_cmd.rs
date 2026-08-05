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
            let client = crate::remote::open_cli_generated_client(&store, keys)?;
            let facet = match facet.as_deref() {
                Some(facet) => Some(vec![
                    FacetKind::parse(facet)
                        .map_err(|e| e.to_string())?
                        .stable_tag(),
                ]),
                None => None,
            };
            let id = execute_generated_uuid_string(
                &client,
                "Workspaces",
                "workspace_create",
                vec![Some(name.clone()).to_value(), optional_bytes_value(facet)],
            )?;
            println!("{id}\t{name}");
            Ok(())
        }
        WorkspaceCmd::List { store } => {
            let client = crate::remote::open_cli_read_only_generated_client(&store, keys)?;
            let infos = client.workspace_list()?;
            crate::helpers::print_workspaces_infos(&infos);
            Ok(())
        }
        WorkspaceCmd::Rename {
            store,
            workspace,
            new_name,
        } => {
            let client = crate::remote::open_cli_generated_client(&store, keys)?;
            let ns = generated_workspace_id(&client, &workspace)?;
            execute_generated_void(
                &client,
                "Workspaces",
                "workspace_rename",
                vec![workspace.to_value(), new_name.to_value()],
            )?;
            println!("{ns}\t{new_name}");
            Ok(())
        }
        WorkspaceCmd::Delete { store, workspace } => {
            let client = crate::remote::open_cli_generated_client(&store, keys)?;
            let ns = generated_workspace_id(&client, &workspace)?;
            execute_generated_void(
                &client,
                "Workspaces",
                "workspace_delete",
                vec![workspace.to_value()],
            )?;
            println!("{ns}");
            Ok(())
        }
    }
}

pub(crate) fn run_identity(action: IdentityCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        IdentityCmd::List { store } => {
            let client = crate::remote::open_cli_read_only_generated_client(&store, keys)?;
            let view = generated_identity_snapshot(&client)?;
            println!("{}", identity_snapshot_json(&view));
            Ok(())
        }
        IdentityCmd::Add {
            store,
            handle,
            name,
            kind,
        } => {
            let kind = parse_principal_kind(&kind)?;
            let client = crate::remote::open_cli_generated_client(&store, keys)?;
            let id = execute_generated_uuid_string(
                &client,
                "Identity",
                "identity_add_principal",
                vec![
                    handle.to_value(),
                    name.to_value(),
                    WireValue::Bytes(vec![kind.stable_tag()]),
                ],
            )?;
            println!("{id}");
            Ok(())
        }
        IdentityCmd::RenameHandle {
            store,
            principal,
            handle,
        } => {
            let client = crate::remote::open_cli_generated_client(&store, keys)?;
            let principal = WorkspaceId::parse(&principal).map_err(|e| e.to_string())?;
            execute_generated_void(
                &client,
                "Identity",
                "identity_rename_principal_handle",
                vec![uuid_value(principal), handle.to_value()],
            )?;
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
            let client = crate::remote::open_cli_generated_client(&store, keys)?;
            let principal = WorkspaceId::parse(&principal).map_err(|e| e.to_string())?;
            execute_generated_void(
                &client,
                "Identity",
                "identity_set_passphrase",
                vec![
                    uuid_value(principal),
                    WireValue::Bytes(passphrase.into_bytes()),
                ],
            )?;
            Ok(())
        }
        IdentityCmd::CreateAppCredential {
            store,
            principal,
            label,
        } => {
            let principal = WorkspaceId::parse(&principal).map_err(|e| e.to_string())?;
            let client = crate::remote::open_cli_generated_client(&store, keys)?;
            println!(
                "{}",
                generated_app_credential_create(&client, principal, label)?
            );
            Ok(())
        }
        IdentityCmd::RevokeAppCredential { store, credential } => {
            let id = WorkspaceId::parse(&credential).map_err(|e| e.to_string())?;
            let client = crate::remote::open_cli_generated_client(&store, keys)?;
            println!("{}", generated_app_credential_revoke(&client, id)?);
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
            let client = crate::remote::open_cli_generated_client(&store, keys)?;
            println!(
                "{}",
                generated_external_credential_create(
                    &client,
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
            let client = crate::remote::open_cli_generated_client(&store, keys)?;
            println!("{}", generated_external_credential_revoke(&client, id)?);
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
            let client = crate::remote::open_cli_generated_client(&store, keys)?;
            let principal = WorkspaceId::parse(&principal).map_err(|e| e.to_string())?;
            println!(
                "{}",
                client.generated_json(
                    "Identity",
                    "identity_force_detach_authority_json",
                    vec![
                        uuid_value(principal),
                        generation.to_value(),
                        reason.to_value()
                    ],
                )?
            );
            Ok(())
        }
        IdentityCmd::AuthorityWitness { store } => {
            let client = crate::remote::open_cli_read_only_generated_client(&store, keys)?;
            let WireValue::Bytes(bytes) = execute_generated_value(
                &client,
                "Identity",
                "identity_authority_witness",
                Vec::new(),
            )?
            else {
                return Err(
                    "Identity.identity_authority_witness returned unexpected value".to_string(),
                );
            };
            let witness = loom_wire::identity::identity_authority_witness_from_cbor(&bytes)
                .map_err(|e| e.to_string())?;
            let algo = witness.snapshot_digest.algo();
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
            let client = crate::remote::open_cli_generated_client(&store, keys)?;
            println!(
                "{}",
                client.generated_json(
                    "Identity",
                    "identity_replicate_authority_json",
                    vec![source.to_value(), become_authority.to_value()],
                )?
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
            let client = crate::remote::open_cli_generated_client(&store, keys)?;
            println!(
                "{}",
                client.generated_json(
                    "Identity",
                    "identity_configure_authority_replication_json",
                    vec![
                        id.to_value(),
                        source.to_value(),
                        disabled.to_value(),
                        pull_on_start.to_value(),
                        interval_ms
                            .map(|value| value.to_value())
                            .unwrap_or(WireValue::Null),
                        jitter_ms.to_value(),
                        backoff_ms.to_value(),
                        publish_witness.to_value(),
                    ],
                )?
            );
            Ok(())
        }
        IdentityCmd::ListAuthorityReplication { store } => {
            let client = crate::remote::open_cli_read_only_generated_client(&store, keys)?;
            let WireValue::Array(records) = execute_generated_value(
                &client,
                "Identity",
                "identity_list_authority_replication",
                Vec::new(),
            )?
            else {
                return Err(
                    "Identity.identity_list_authority_replication returned unexpected value"
                        .to_string(),
                );
            };
            let policies = records
                .iter()
                .map(|record| match record {
                    WireValue::Bytes(bytes) => {
                        authority_replication_policy_from_generated_record(bytes)
                    }
                    other => Err(format!(
                        "Identity.identity_list_authority_replication returned unexpected record {other:?}"
                    )),
                })
                .collect::<Result<Vec<_>, String>>()?;
            println!("{}", authority_replication_policies_json(&policies));
            Ok(())
        }
        IdentityCmd::RemoveAuthorityReplication { store, id } => {
            crate::locator_cx::current().require_local_admin(&store)?;
            let client = crate::remote::open_cli_generated_client(&store, keys)?;
            println!(
                "{}",
                client.generated_json(
                    "Identity",
                    "identity_remove_authority_replication_json",
                    vec![id.to_value()],
                )?
            );
            Ok(())
        }
        IdentityCmd::Remove { store, principal } => {
            let client = crate::remote::open_cli_generated_client(&store, keys)?;
            let principal = WorkspaceId::parse(&principal).map_err(|e| e.to_string())?;
            execute_generated_void(
                &client,
                "Identity",
                "identity_remove_principal",
                vec![uuid_value(principal)],
            )?;
            Ok(())
        }
        IdentityCmd::AssignRole {
            store,
            principal,
            role,
        } => {
            let client = crate::remote::open_cli_generated_client(&store, keys)?;
            let principal = WorkspaceId::parse(&principal).map_err(|e| e.to_string())?;
            let role = WorkspaceId::parse(&role).map_err(|e| e.to_string())?;
            execute_generated_void(
                &client,
                "Identity",
                "identity_assign_role",
                vec![uuid_value(principal), uuid_value(role)],
            )?;
            Ok(())
        }
        IdentityCmd::RevokeRole {
            store,
            principal,
            role,
        } => {
            let client = crate::remote::open_cli_generated_client(&store, keys)?;
            let principal = WorkspaceId::parse(&principal).map_err(|e| e.to_string())?;
            let role = WorkspaceId::parse(&role).map_err(|e| e.to_string())?;
            let removed = execute_generated_bool(
                &client,
                "Identity",
                "identity_revoke_role",
                vec![uuid_value(principal), uuid_value(role)],
            )?;
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
            let client = crate::remote::open_cli_generated_client(&store, keys)?;
            println!(
                "{}",
                generated_public_key_add(&client, principal, label, algorithm, public_key)?
            );
            Ok(())
        }
        IdentityPublicKeyCmd::List { store } => {
            let client = crate::remote::open_cli_read_only_generated_client(&store, keys)?;
            let view = generated_identity_snapshot(&client)?;
            println!("{}", identity_public_keys_json(&view.public_keys));
            Ok(())
        }
        IdentityPublicKeyCmd::Revoke { store, key } => {
            let key = WorkspaceId::parse(&key).map_err(|e| e.to_string())?;
            let client = crate::remote::open_cli_generated_client(&store, keys)?;
            println!("{}", generated_public_key_revoke(&client, key)?);
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

fn uuid_value(id: WorkspaceId) -> WireValue {
    WireValue::Bytes(id.as_bytes().to_vec())
}

fn optional_bytes_value(bytes: Option<Vec<u8>>) -> WireValue {
    bytes.map(WireValue::Bytes).unwrap_or(WireValue::Null)
}

fn optional_bytes_array_value(bytes: Option<Vec<Vec<u8>>>) -> WireValue {
    bytes
        .map(|items| WireValue::Array(items.into_iter().map(WireValue::Bytes).collect()))
        .unwrap_or(WireValue::Null)
}

fn execute_generated_uuid_string(
    client: &crate::remote::CliGeneratedClient,
    interface: &str,
    method: &str,
    args: Vec<WireValue>,
) -> Result<String, String> {
    match execute_generated_value(client, interface, method, args)? {
        WireValue::Bytes(bytes) => {
            let bytes: [u8; 16] = bytes
                .try_into()
                .map_err(|_| format!("{interface}.{method} returned invalid uuid bytes"))?;
            Ok(WorkspaceId::from_bytes(bytes).to_string())
        }
        value => Err(format!(
            "{interface}.{method} returned unexpected value {value:?}"
        )),
    }
}

fn generated_workspace_id(
    client: &crate::remote::CliGeneratedClient,
    workspace: &str,
) -> Result<WorkspaceId, String> {
    if let Ok(id) = WorkspaceId::parse(workspace) {
        return Ok(id);
    }
    let WireValue::Array(items) =
        execute_generated_value(client, "Workspaces", "workspace_list", Vec::new())?
    else {
        return Err("Workspaces.workspace_list returned unexpected value".to_string());
    };
    let mut found = None;
    for item in items {
        let WireValue::Bytes(bytes) = item else {
            return Err("workspace list item must be bytes".to_string());
        };
        let info = workspace_info_from_cbor(&bytes)?;
        if info.name == workspace {
            if found.is_some() {
                return Err(format!("workspace name {workspace:?} is ambiguous"));
            }
            found = Some(info.id);
        }
    }
    found.ok_or_else(|| format!("workspace {workspace:?} not found"))
}

fn workspace_info_from_cbor(bytes: &[u8]) -> Result<loom_core::WorkspaceInfo, String> {
    let WireValue::Array(items) = loom_codec::decode(bytes).map_err(|e| e.to_string())? else {
        return Err("workspace info must be a CBOR array".to_string());
    };
    let id = match items.first() {
        Some(WireValue::Text(text)) => WorkspaceId::parse(text).map_err(|e| e.to_string())?,
        _ => return Err("workspace info id must be text".to_string()),
    };
    let name = match items.get(1) {
        Some(WireValue::Text(text)) => text.clone(),
        _ => return Err("workspace info name must be text".to_string()),
    };
    let facets = match items.get(2) {
        Some(WireValue::Array(tags)) => tags
            .iter()
            .map(|tag| match tag {
                WireValue::Uint(value) => {
                    let tag = u8::try_from(*value)
                        .map_err(|_| "workspace facet tag out of range".to_string())?;
                    FacetKind::from_stable_tag(tag)
                        .ok_or_else(|| format!("unknown workspace facet tag {tag}"))
                }
                _ => Err("workspace facet tag must be uint".to_string()),
            })
            .collect::<Result<Vec<_>, String>>()?,
        _ => return Err("workspace facets must be an array".to_string()),
    };
    let head = match items.get(3) {
        None | Some(WireValue::Null) => None,
        Some(WireValue::Text(text)) => Some(Digest::parse(text).map_err(|e| e.to_string())?),
        _ => return Err("workspace head must be text or null".to_string()),
    };
    Ok(loom_core::WorkspaceInfo {
        id,
        name,
        facets,
        head,
    })
}

struct AclWireArgs {
    effect: Vec<u8>,
    subject: String,
    workspace: Option<String>,
    domain: Option<Vec<u8>>,
    ref_glob: Option<String>,
    scopes: Option<Vec<Vec<u8>>>,
    rights: Option<Vec<Vec<u8>>>,
    predicate: Option<Vec<u8>>,
}

fn acl_wire_args(args: AclGrantArgs<'_>) -> Result<AclWireArgs, String> {
    let effect = loom_wire::acl::acl_effect_to_wire(parse_acl_effect(args.effect)?);
    let domain = optional_acl_domain_arg(args.domain)?.map(|domain| vec![domain.stable_tag()]);
    let scopes = if args.scopes.is_empty() {
        None
    } else {
        Some(
            args.scopes
                .iter()
                .map(|scope| {
                    loom_wire::acl::acl_scope_to_wire(&parse_acl_scope(scope)?)
                        .map_err(|e| e.to_string())
                })
                .collect::<Result<Vec<_>, String>>()?,
        )
    };
    let rights = Some(
        args.rights
            .iter()
            .map(|right| Ok(loom_wire::acl::acl_right_to_wire(parse_acl_right(right)?)))
            .collect::<Result<Vec<_>, String>>()?,
    );
    let predicate = match optional_acl_predicate(args.predicate_cel)? {
        Some(predicate) => {
            Some(loom_wire::acl::acl_predicate_to_wire(&predicate).map_err(|e| e.to_string())?)
        }
        None => None,
    };
    Ok(AclWireArgs {
        effect,
        subject: args.subject.to_string(),
        workspace: args.workspace.map(str::to_string),
        domain,
        ref_glob: args
            .ref_glob
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        scopes,
        rights,
        predicate,
    })
}

fn generated_identity_snapshot(
    client: &crate::remote::CliGeneratedClient,
) -> Result<loom_wire::identity::IdentitySnapshotView, String> {
    let bytes = execute_generated_bytes(client, "Identity", "identity_list", Vec::new())?;
    loom_wire::identity::identity_snapshot_from_cbor(&bytes).map_err(|e| e.to_string())
}

fn authority_replication_policy_from_generated_record(
    bytes: &[u8],
) -> Result<loom_store::AuthorityReplicationPolicy, String> {
    let record = loom_wire::identity::authority_replication_policy_record_from_cbor(bytes)
        .map_err(|e| e.to_string())?;
    let schema_version = u16::try_from(record.schema_version)
        .map_err(|_| "authority replication schema_version exceeds u16".to_string())?;
    Ok(loom_store::AuthorityReplicationPolicy {
        id: record.id,
        schema_version,
        source: record.source,
        enabled: record.enabled,
        pull_on_start: record.pull_on_start,
        interval_ms: record.interval_ms,
        jitter_ms: record.jitter_ms,
        backoff_ms: record.backoff_ms,
        publish_witness: record.publish_witness,
        last_success_ms: record.last_success_ms,
        last_failure_ms: record.last_failure_ms,
        last_error: record.last_error,
        last_modified_audit_seq: record.last_modified_audit_seq,
    })
}

fn generated_external_credential_create(
    client: &crate::remote::CliGeneratedClient,
    principal: WorkspaceId,
    kind: ExternalCredentialKind,
    label: String,
    issuer: String,
    subject: String,
    material_digest: Option<String>,
) -> Result<String, String> {
    let spec = loom_core::ExternalCredentialSpec {
        id: random_workspace_id()?,
        kind,
        label,
        issuer,
        subject,
        material_digest,
    };
    let wire =
        loom_wire::identity::external_credential_spec_to_wire(&spec).map_err(|e| e.to_string())?;
    let audit = execute_generated_bytes(
        client,
        "Identity",
        "identity_create_external_credential",
        vec![uuid_value(principal), WireValue::Bytes(wire)],
    )?;
    let result =
        loom_wire::identity::identity_audit_result_from_cbor(&audit).map_err(|e| e.to_string())?;
    let id = result
        .id
        .ok_or_else(|| "create did not return a credential id".to_string())?;
    let view = generated_identity_snapshot(client)?;
    let credential = view
        .external_credentials
        .iter()
        .find(|credential| credential.id == id)
        .ok_or_else(|| "created credential not found on read-back".to_string())?;
    Ok(format!(
        "{{\"seq\":{},\"credential\":{}}}",
        result.audit_seq,
        external_credential_json(credential)
    ))
}

fn generated_external_credential_revoke(
    client: &crate::remote::CliGeneratedClient,
    id: WorkspaceId,
) -> Result<String, String> {
    let view = generated_identity_snapshot(client)?;
    let credential = view
        .external_credentials
        .iter()
        .find(|credential| credential.id == id)
        .cloned()
        .ok_or_else(|| "external credential not found".to_string())?;
    let audit = execute_generated_bytes(
        client,
        "Identity",
        "identity_revoke_external_credential",
        vec![uuid_value(id)],
    )?;
    let result =
        loom_wire::identity::identity_audit_result_from_cbor(&audit).map_err(|e| e.to_string())?;
    Ok(format!(
        "{{\"seq\":{},\"credential\":{}}}",
        result.audit_seq,
        external_credential_json(&credential)
    ))
}

fn generated_public_key_add(
    client: &crate::remote::CliGeneratedClient,
    principal: WorkspaceId,
    label: String,
    algorithm: String,
    public_key: Vec<u8>,
) -> Result<String, String> {
    let audit = execute_generated_bytes(
        client,
        "Identity",
        "identity_add_public_key",
        vec![
            uuid_value(principal),
            label.to_value(),
            algorithm.to_value(),
            WireValue::Bytes(public_key),
        ],
    )?;
    let result =
        loom_wire::identity::identity_audit_result_from_cbor(&audit).map_err(|e| e.to_string())?;
    let id = result
        .id
        .ok_or_else(|| "add did not return a public key id".to_string())?;
    let view = generated_identity_snapshot(client)?;
    let key = view
        .public_keys
        .iter()
        .find(|key| key.id == id)
        .ok_or_else(|| "added public key not found on read-back".to_string())?;
    Ok(format!(
        "{{\"seq\":{},\"public_key\":{}}}",
        result.audit_seq,
        identity_public_key_json(key)
    ))
}

fn generated_public_key_revoke(
    client: &crate::remote::CliGeneratedClient,
    id: WorkspaceId,
) -> Result<String, String> {
    let view = generated_identity_snapshot(client)?;
    let key = view
        .public_keys
        .iter()
        .find(|key| key.id == id)
        .cloned()
        .ok_or_else(|| "public key not found".to_string())?;
    let audit = execute_generated_bytes(
        client,
        "Identity",
        "identity_revoke_public_key",
        vec![uuid_value(id)],
    )?;
    let result =
        loom_wire::identity::identity_audit_result_from_cbor(&audit).map_err(|e| e.to_string())?;
    Ok(format!(
        "{{\"seq\":{},\"public_key\":{}}}",
        result.audit_seq,
        identity_public_key_json(&key)
    ))
}

fn generated_app_credential_create(
    client: &crate::remote::CliGeneratedClient,
    principal: WorkspaceId,
    label: String,
) -> Result<String, String> {
    let result =
        loom_wire::identity::app_credential_create_result_from_cbor(&execute_generated_bytes(
            client,
            "Identity",
            "identity_create_app_credential",
            vec![uuid_value(principal), label.to_value()],
        )?)
        .map_err(|e| e.to_string())?;
    let credential = AppCredential {
        id: result.id,
        principal: result.principal,
        label: result.label,
        enabled: result.enabled,
    };
    Ok(format!(
        "{{\"seq\":{},\"credential\":{},\"secret\":{}}}",
        result.audit_seq,
        app_credential_json(&credential),
        json_string(&result.secret_token)
    ))
}

fn generated_app_credential_revoke(
    client: &crate::remote::CliGeneratedClient,
    id: WorkspaceId,
) -> Result<String, String> {
    let view = generated_identity_snapshot(client)?;
    let credential = view
        .app_credentials
        .iter()
        .find(|credential| credential.id == id)
        .cloned()
        .ok_or_else(|| "app credential not found".to_string())?;
    let audit = execute_generated_bytes(
        client,
        "Identity",
        "identity_revoke_app_credential",
        vec![uuid_value(id)],
    )?;
    let result =
        loom_wire::identity::identity_audit_result_from_cbor(&audit).map_err(|e| e.to_string())?;
    Ok(format!(
        "{{\"seq\":{},\"credential\":{}}}",
        result.audit_seq,
        app_credential_json(&credential)
    ))
}

pub(crate) fn run_acl(action: AclCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        AclCmd::List { store } => {
            let client = crate::remote::open_cli_read_only_generated_client(&store, keys)?;
            let WireValue::Array(records) =
                execute_generated_value(&client, "Acl", "acl_list", Vec::new())?
            else {
                return Err("Acl.acl_list returned unexpected value".to_string());
            };
            let grants = records
                .iter()
                .map(|record| match record {
                    WireValue::Bytes(bytes) => {
                        loom_wire::acl::acl_grant_from_cbor(bytes).map_err(|e| e.to_string())
                    }
                    other => Err(format!("Acl.acl_list returned unexpected record {other:?}")),
                })
                .collect::<Result<Vec<_>, String>>()?;
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
            let client = crate::remote::open_cli_generated_client(&store, keys)?;
            let wire = acl_wire_args(AclGrantArgs {
                effect: &effect,
                subject: &subject,
                workspace: workspace.as_deref(),
                domain: domain.as_deref(),
                rights: &rights,
                ref_glob: ref_glob.as_deref(),
                scopes: &scopes,
                predicate_cel: predicate_cel.as_deref(),
            })?;
            execute_generated_void(
                &client,
                "Acl",
                "acl_grant",
                vec![
                    WireValue::Bytes(wire.effect),
                    wire.subject.to_value(),
                    wire.workspace.to_value(),
                    optional_bytes_value(wire.domain),
                    wire.ref_glob.to_value(),
                    optional_bytes_array_value(wire.scopes),
                    optional_bytes_array_value(wire.rights),
                    optional_bytes_value(wire.predicate),
                ],
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
            let client = crate::remote::open_cli_generated_client(&store, keys)?;
            let wire = acl_wire_args(AclGrantArgs {
                effect: &effect,
                subject: &subject,
                workspace: workspace.as_deref(),
                domain: domain.as_deref(),
                rights: &rights,
                ref_glob: ref_glob.as_deref(),
                scopes: &scopes,
                predicate_cel: predicate_cel.as_deref(),
            })?;
            let removed = execute_generated_bool(
                &client,
                "Acl",
                "acl_revoke",
                vec![
                    WireValue::Bytes(wire.effect),
                    wire.subject.to_value(),
                    wire.workspace.to_value(),
                    optional_bytes_value(wire.domain),
                    wire.ref_glob.to_value(),
                    optional_bytes_array_value(wire.scopes),
                    optional_bytes_array_value(wire.rights),
                    optional_bytes_value(wire.predicate),
                ],
            )?;
            println!("{}", if removed { "removed" } else { "not-found" });
            Ok(())
        }
    }
}

pub(crate) fn run_protected_ref(action: ProtectedRefCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        ProtectedRefCmd::List { store, workspace } => {
            let client = crate::remote::open_cli_read_only_generated_client(&store, keys)?;
            let WireValue::Array(records) = execute_generated_value(
                &client,
                "ProtectedRefs",
                "protected_ref_list",
                vec![workspace.to_value()],
            )?
            else {
                return Err(
                    "ProtectedRefs.protected_ref_list returned unexpected value".to_string()
                );
            };
            let policies = records
                .iter()
                .map(|record| match record {
                    WireValue::Bytes(bytes) => {
                        crate::remote::cli_named_protected_ref_from_remote(bytes)
                    }
                    other => Err(format!(
                        "ProtectedRefs.protected_ref_list returned unexpected record {other:?}"
                    )),
                })
                .collect::<Result<Vec<_>, String>>()?;
            println!("{}", protected_ref_policies_json(&policies));
            Ok(())
        }
        ProtectedRefCmd::Get {
            store,
            workspace,
            ref_name,
        } => {
            let client = crate::remote::open_cli_read_only_generated_client(&store, keys)?;
            let policy = execute_generated_value(
                &client,
                "ProtectedRefs",
                "protected_ref_get",
                vec![workspace.to_value(), ref_name.to_value()],
            )?;
            match policy {
                WireValue::Null => println!("null"),
                WireValue::Bytes(bytes) => {
                    let policy = crate::remote::cli_protected_ref_policy_from_remote(&bytes)?;
                    println!("{}", protected_ref_policy_json(&ref_name, &policy));
                }
                other => {
                    return Err(format!(
                        "ProtectedRefs.protected_ref_get returned unexpected value {other:?}"
                    ));
                }
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
            let client = crate::remote::open_cli_generated_client(&store, keys)?;
            execute_generated_void(
                &client,
                "ProtectedRefs",
                "protected_ref_set",
                vec![
                    workspace.to_value(),
                    ref_name.to_value(),
                    policy.fast_forward_only.to_value(),
                    policy.signed_commits_required.to_value(),
                    policy.signed_ref_advance_required.to_value(),
                    policy.required_review_count.to_value(),
                    policy.retention_lock.to_value(),
                    policy.governance_lock.to_value(),
                ],
            )
        }
        ProtectedRefCmd::Remove {
            store,
            workspace,
            ref_name,
        } => {
            let client = crate::remote::open_cli_generated_client(&store, keys)?;
            let removed = execute_generated_bool(
                &client,
                "ProtectedRefs",
                "protected_ref_remove",
                vec![workspace.to_value(), ref_name.to_value()],
            )?;
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
            let client = crate::remote::open_cli_generated_client(&store, keys)?;
            execute_generated_void(
                &client,
                "ManagementKv",
                "set_config",
                vec![
                    workspace.to_value(),
                    name.to_value(),
                    WireValue::Bytes(config.encode()),
                ],
            )
        }
        ManagementKvConfigCmd::Get {
            store,
            workspace,
            name,
        } => {
            let client = crate::remote::open_cli_read_only_generated_client(&store, keys)?;
            let bytes = execute_generated_bytes(
                &client,
                "ManagementKv",
                "get_config",
                vec![workspace.to_value(), name.to_value()],
            )?;
            let config = KvMapConfig::decode(&bytes).map_err(|e| e.to_string())?;
            println!("{}", kv_map_config_json(config));
            Ok(())
        }
    }
}
