use loom_interchange_io::{
    import_meetings_bytes, import_report_json, meetings_source_payload_path,
    parse_meetings_input_profile, validate_meetings_source_payload_leaf,
};
use wasm_bindgen::prelude::*;

use super::{LoomStore, le, resolve_workspace_arg, save_loom};

#[wasm_bindgen]
impl LoomStore {
    pub fn meetings_import_snapshot(
        &mut self,
        workspace: String,
        input_profile: String,
        snapshot: Vec<u8>,
        dry_run: bool,
    ) -> Result<String, JsError> {
        let workspace_id = resolve_workspace_arg(&self.loom, &workspace)?;
        let profile = parse_meetings_input_profile(&input_profile).map_err(le)?;
        let result =
            import_meetings_bytes(&mut self.loom, workspace_id, profile, &snapshot, dry_run)
                .map_err(le)?;
        if !dry_run {
            save_loom(&mut self.loom).map_err(le)?;
        }
        import_report_json(&result.report).map_err(le)
    }

    pub fn meetings_source_read(
        &self,
        workspace: String,
        source_id: String,
        leaf: String,
    ) -> Result<Vec<u8>, JsError> {
        let workspace_id = resolve_workspace_arg(&self.loom, &workspace)?;
        validate_meetings_source_payload_leaf(&leaf).map_err(le)?;
        let profile_id = workspace_id.to_string();
        let path = meetings_source_payload_path(&profile_id, &source_id, &leaf);
        self.loom
            .read_file_reserved(workspace_id, &path)
            .map_err(le)
    }
}
