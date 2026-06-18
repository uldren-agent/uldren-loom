//! Licensed under BUSL-1.1 (see the repo `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

use loom_interchange_io::{
    import_meetings_bytes, import_report_json, meetings_source_payload_path,
    parse_meetings_input_profile, validate_meetings_source_payload_leaf,
};

#[napi]
pub fn meetings_import_snapshot(
    loom_path: String,
    workspace: String,
    input_profile: String,
    snapshot: Uint8Array,
    dry_run: bool,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let workspace_id = resolve_workspace_arg(&loom, &workspace)?;
    let profile = parse_meetings_input_profile(&input_profile).map_err(reason)?;
    let result = import_meetings_bytes(&mut loom, workspace_id, profile, &snapshot, dry_run)
        .map_err(reason)?;
    import_report_json(&result.report).map_err(reason)
}

#[napi]
pub fn meetings_source_read(
    loom_path: String,
    workspace: String,
    source_id: String,
    leaf: String,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let workspace_id = resolve_workspace_arg(&loom, &workspace)?;
    validate_meetings_source_payload_leaf(&leaf).map_err(reason)?;
    let profile_id = workspace_id.to_string();
    let path = meetings_source_payload_path(&profile_id, &source_id, &leaf);
    let bytes = loom
        .read_file_reserved(workspace_id, &path)
        .map_err(reason)?;
    Ok(Uint8Array::from(bytes))
}
