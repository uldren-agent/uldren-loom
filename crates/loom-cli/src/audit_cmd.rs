//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

pub(crate) fn run_audit(action: AuditCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        AuditCmd::Compact { store, through_seq } => run_audit_compact(&store, through_seq, keys),
        AuditCmd::Config { action } => run_audit_config(action, keys),
        AuditCmd::List { store } => run_audit_list(&store, keys),
        AuditCmd::View { store, record } => run_audit_view(&store, &record, keys),
    }
}

fn run_audit_compact(store: &str, through_seq: u64, keys: &KeyOpts) -> Result<(), String> {
    let client = remote::open_cli_generated_client(store, keys)?;
    let bytes = execute_generated_bytes(
        &client,
        "Audit",
        "audit_compact",
        vec![through_seq.to_value()],
    )?;
    let result =
        loom_wire::audit::audit_compact_result_from_cbor(&bytes).map_err(|e| e.to_string())?;
    println!("{}", audit_compact_result_json(&result));
    Ok(())
}

fn run_audit_config(action: AuditConfigCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        AuditConfigCmd::Show { store } => {
            let client = remote::open_cli_generated_client(&store, keys)?;
            let out = execute_generated_string(&client, "Audit", "audit_config_show_json", vec![])?;
            println!("{out}");
            Ok(())
        }
        AuditConfigCmd::Set {
            store,
            retention_days,
            legal_hold,
        } => {
            let client = remote::open_cli_generated_client(&store, keys)?;
            let out = execute_generated_string(
                &client,
                "Audit",
                "audit_config_set_json",
                vec![retention_days.to_value(), legal_hold.to_value()],
            )?;
            println!("{out}");
            Ok(())
        }
    }
}

fn run_audit_list(store: &str, keys: &KeyOpts) -> Result<(), String> {
    let client = remote::open_cli_generated_client(store, keys)?;
    let out = execute_generated_string(&client, "Audit", "audit_list_json", vec![])?;
    println!("{out}");
    Ok(())
}

fn run_audit_view(store: &str, record: &str, keys: &KeyOpts) -> Result<(), String> {
    let client = remote::open_cli_generated_client(store, keys)?;
    let out =
        execute_generated_string(&client, "Audit", "audit_view_json", vec![record.to_value()])?;
    println!("{out}");
    Ok(())
}

fn audit_compact_result_json(result: &loom_wire::audit::AuditCompactResult) -> String {
    let checkpoint_seq = result
        .checkpoint_seq
        .map_or_else(|| "null".to_string(), |value| value.to_string());
    let checkpoint_hash = result.checkpoint_hash.as_ref().map_or_else(
        || "null".to_string(),
        |value| json_string(&value.to_string()),
    );
    format!(
        "{{\"pruned\":{},\"checkpoint_seq\":{},\"checkpoint_hash\":{},\"audit_seq\":{}}}",
        result.pruned, checkpoint_seq, checkpoint_hash, result.audit_seq
    )
}

#[cfg(test)]
mod mu6i_d4_tests {
    use super::*;

    #[test]
    fn audit_compact_generated_result_preserves_json_shape_and_nulls() {
        let result = loom_wire::audit::audit_compact_result_from_cbor(
            &loom_wire::audit::audit_compact_result_to_cbor(
                &loom_wire::audit::AuditCompactResult {
                    pruned: 7,
                    checkpoint_seq: None,
                    checkpoint_hash: None,
                    audit_seq: 9,
                },
            ),
        )
        .unwrap();

        assert_eq!(
            audit_compact_result_json(&result),
            "{\"pruned\":7,\"checkpoint_seq\":null,\"checkpoint_hash\":null,\"audit_seq\":9}"
        );
    }
}
