use crate::{KeyOpts, mount_open_auth};
use uldren_loom_mcp::{LoomMcp, StoreAccess};

use crate::cli::RefsCmd;

pub(crate) fn run_refs(action: RefsCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        RefsCmd::Status {
            store,
            workspace,
            format,
        } => {
            let status = mcp_for_store(&store, keys)?
                .read_substrate_reference_reconciliation_status(&workspace)
                .map_err(|error| error.to_string())?;
            print_status(&status, &format)
        }
        RefsCmd::Reconcile {
            store,
            workspace,
            max,
            format,
        } => {
            let workspace_id = workspace_profile_id(&store, &workspace, keys)?;
            let status = mcp_for_store(&store, keys)?
                .write_substrate_reference_reconcile(&workspace, &workspace_id, max)
                .map_err(|error| error.to_string())?;
            print_status(&status, &format)
        }
    }
}

fn mcp_for_store(store: &str, keys: &KeyOpts) -> Result<LoomMcp, String> {
    let auth = mount_open_auth(store, keys)?;
    let access = match StoreAccess::per_request_attached_auth(store, auth.clone()) {
        Ok(access) => access,
        Err(error) if error.code == loom_core::Code::NotFound => {
            StoreAccess::per_request_auth(store, auth)
        }
        Err(error) => return Err(error.to_string()),
    };
    Ok(LoomMcp::new(access))
}

fn workspace_profile_id(store: &str, workspace: &str, keys: &KeyOpts) -> Result<String, String> {
    let loom = crate::cli_open_loom(store, keys)?;
    loom.registry()
        .open(&loom_core::WsSelector::Name(workspace.to_string()))
        .map(|workspace_id| workspace_id.to_string())
        .map_err(|_| "reference reconciliation workspace was not found".to_string())
}

fn print_status(
    status: &uldren_loom_mcp::reads::ReferenceReconciliationSummary,
    format: &str,
) -> Result<(), String> {
    match format {
        "text" => {
            println!("pending\tresolved\tfailed\tprocessed");
            println!(
                "{}\t{}\t{}\t{}",
                status.pending, status.resolved, status.failed, status.processed
            );
            Ok(())
        }
        "json" => serde_json::to_string(status)
            .map(|value| println!("{value}"))
            .map_err(|error| error.to_string()),
        _ => Err("--format must be text or json".to_string()),
    }
}
