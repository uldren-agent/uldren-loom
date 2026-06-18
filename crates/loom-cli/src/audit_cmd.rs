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
    let loom = cli_open_loom(store, keys)?;
    let actor = require_global_admin_actor(&loom)?;
    let stats = loom
        .store()
        .audit_prune_through(Some(actor), through_seq)
        .map_err(|e| e.to_string())?;
    println!("{}", audit_prune_stats_json(stats));
    Ok(())
}

fn run_audit_config(action: AuditConfigCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        AuditConfigCmd::Show { store } => {
            let loom = cli_open_loom(&store, keys)?;
            let actor = require_global_admin_actor(&loom)?;
            let config = loom.store().audit_config().map_err(|e| e.to_string())?;
            loom.store()
                .audit_append(Some(actor), "audit.config.show", None)
                .map_err(|e| e.to_string())?;
            println!("{}", audit_config_json(config));
            Ok(())
        }
        AuditConfigCmd::Set {
            store,
            retention_days,
            legal_hold,
        } => {
            let loom = cli_open_loom(&store, keys)?;
            let actor = require_global_admin_actor(&loom)?;
            let mut config = loom.store().audit_config().map_err(|e| e.to_string())?;
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
            let seq = loom
                .store()
                .save_audit_config_audited(config, Some(actor), "audit.config.set", Some(&target))
                .map_err(|e| e.to_string())?;
            println!("{}", audit_config_set_json(seq, config));
            Ok(())
        }
    }
}

fn run_audit_list(store: &str, keys: &KeyOpts) -> Result<(), String> {
    let loom = cli_open_loom(store, keys)?;
    let actor = require_global_admin_actor(&loom)?;
    let records = loom.store().audit_records().map_err(|e| e.to_string())?;
    loom.store()
        .audit_append(Some(actor), "audit.list", None)
        .map_err(|e| e.to_string())?;
    println!("seq\thash\tprincipal\taction\ttarget");
    for record in records {
        println!(
            "{}\t{}\t{}\t{}\t{}",
            record.seq,
            record.hash,
            record
                .principal
                .map_or_else(|| "-".to_string(), |principal| principal.to_string()),
            record.action,
            record.target.unwrap_or_else(|| "-".to_string())
        );
    }
    Ok(())
}

fn run_audit_view(store: &str, record: &str, keys: &KeyOpts) -> Result<(), String> {
    let loom = cli_open_loom(store, keys)?;
    let actor = require_global_admin_actor(&loom)?;
    let records = loom.store().audit_records().map_err(|e| e.to_string())?;
    let found = find_audit_record(&records, record)?;
    let target = format!("seq={}", found.seq);
    loom.store()
        .audit_append(Some(actor), "audit.view", Some(&target))
        .map_err(|e| e.to_string())?;
    println!("{}", audit_record_json(found));
    Ok(())
}

fn find_audit_record<'a>(
    records: &'a [loom_store::AuditRecord],
    record: &str,
) -> Result<&'a loom_store::AuditRecord, String> {
    if let Ok(seq) = record.parse::<u64>() {
        return records
            .iter()
            .find(|entry| entry.seq == seq)
            .ok_or_else(|| format!("audit record not found: {record}"));
    }
    let digest = Digest::parse(record).map_err(|e| e.to_string())?;
    records
        .iter()
        .find(|entry| entry.hash == digest)
        .ok_or_else(|| format!("audit record not found: {record}"))
}

fn audit_config_json(config: AuditConfig) -> String {
    format!(
        "{{\"retention_days\":{},\"legal_hold\":{}}}",
        config.retention_days, config.legal_hold
    )
}

fn audit_config_set_json(seq: u64, config: AuditConfig) -> String {
    format!(
        "{{\"seq\":{},\"config\":{}}}",
        seq,
        audit_config_json(config)
    )
}

fn audit_prune_stats_json(stats: loom_store::AuditPruneStats) -> String {
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

fn audit_record_json(record: &loom_store::AuditRecord) -> String {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"seq\":");
    out.push_str(&record.seq.to_string());
    out.push_str(",\"hash\":");
    out.push_str(&json_string(&record.hash.to_string()));
    out.push_str(",\"principal\":");
    match record.principal {
        Some(principal) => out.push_str(&json_string(&principal.to_string())),
        None => out.push_str("null"),
    }
    out.push_str(",\"action\":");
    out.push_str(&json_string(&record.action));
    out.push_str(",\"target\":");
    match record.target.as_deref() {
        Some(target) => out.push_str(&json_string(target)),
        None => out.push_str("null"),
    }
    out.push_str(",\"prev_hash\":");
    match record.prev_hash {
        Some(hash) => out.push_str(&json_string(&hash.to_string())),
        None => out.push_str("null"),
    }
    out.push('}');
    out
}
