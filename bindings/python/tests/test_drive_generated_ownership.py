from pathlib import Path


ROOT = Path(__file__).parents[3]

INVENTORY = [
    "drive_list_json",
    "drive_stat_json",
    "drive_read_file",
    "drive_list_versions_json",
    "drive_list_conflicts_json",
    "drive_list_shares_json",
    "drive_list_retention_json",
    "drive_create_folder_json",
    "drive_create_upload_json",
    "drive_upload_chunk_json",
    "drive_commit_upload_json",
    "drive_rename_json",
    "drive_move_json",
    "drive_delete_json",
    "drive_resolve_conflict_json",
    "drive_grant_share_json",
    "drive_revoke_share_json",
    "drive_apply_share_expiry_json",
    "drive_pin_retention_json",
    "drive_unpin_retention_json",
    "drive_apply_retention_json",
]


def _compact(value: str) -> str:
    compact = " ".join(value.split())
    for token in ("(", ")", ",", ";"):
        compact = compact.replace(f" {token}", token).replace(f"{token} ", token)
    return compact


def _body_for(source: str, name: str) -> str:
    start = source.index(f"pub(crate) fn {name}")
    next_start = source.find("\n#[pyfunction]", start + 1)
    return source[start:] if next_start == -1 else source[start:next_start]


def test_drive_generated_ownership_matches_source_boundaries():
    source = (ROOT / "bindings" / "python" / "src" / "drive.rs").read_text()
    idl = _compact((ROOT / "idl" / "loom.idl").read_text())
    generated = (ROOT / "crates" / "loom-remote-protocol" / "src" / "generated_api.rs").read_text()

    assert "use loom_client::generated_api::Drive as GeneratedDrive;" in source
    assert "loom_drive::" not in source
    assert "use loom_drive" not in source
    assert "fn drive_write" not in source
    assert "fn to_json" not in source
    assert "fn parse_resolution" not in source
    for name in INVENTORY:
        body = _body_for(source, name)
        assert "generated_session::open_generated_session" in body
        assert f">::{name}" in body
        assert "drive_read(" not in body
        assert "drive_write(" not in body
        assert f"Generated binding for `Drive.{name}`" in generated

    for signature in [
        "string drive_list_json(LoomSession handle,string workspace,string drive_workspace_id,string folder_id);",
        "string drive_stat_json(LoomSession handle,string workspace,string drive_workspace_id,string folder_id,string name);",
        "bytes drive_read_file(LoomSession handle,string workspace,string drive_workspace_id,string file_id);",
        "string drive_list_versions_json(LoomSession handle,string workspace,string drive_workspace_id,string file_id);",
        "string drive_list_conflicts_json(LoomSession handle,string workspace,string drive_workspace_id);",
        "string drive_list_shares_json(LoomSession handle,string workspace,string drive_workspace_id);",
        "string drive_list_retention_json(LoomSession handle,string workspace,string drive_workspace_id);",
        "string drive_create_folder_json(LoomSession handle,string workspace,string drive_workspace_id,string parent_folder_id,string folder_id,string name,string expected_root);",
        "string drive_create_upload_json(LoomSession handle,string workspace,string drive_workspace_id,string upload_id,string parent_folder_id,string name,string file_id,string expected_root,u64 created_at_ms,bool replace_file);",
        "string drive_upload_chunk_json(LoomSession handle,string workspace,string drive_workspace_id,string upload_id,bytes chunk);",
        "string drive_commit_upload_json(LoomSession handle,string workspace,string drive_workspace_id,string upload_id);",
        "string drive_rename_json(LoomSession handle,string workspace,string drive_workspace_id,string folder_id,string node_id,string new_name,string expected_root);",
        "string drive_move_json(LoomSession handle,string workspace,string drive_workspace_id,string source_folder_id,string target_folder_id,string node_id,string expected_root);",
        "string drive_delete_json(LoomSession handle,string workspace,string drive_workspace_id,string folder_id,string node_id,string expected_root);",
        "string drive_resolve_conflict_json(LoomSession handle,string workspace,string drive_workspace_id,string conflict_id,string resolution);",
        "string drive_grant_share_json(LoomSession handle,string workspace,string drive_workspace_id,string grant_id,string target_kind,string target_id,string principal,string role,u64 granted_at_ms,optional u64 expires_at_ms);",
        "string drive_revoke_share_json(LoomSession handle,string workspace,string drive_workspace_id,string grant_id);",
        "string drive_apply_share_expiry_json(LoomSession handle,string workspace,string drive_workspace_id,u64 now_ms);",
        "string drive_pin_retention_json(LoomSession handle,string workspace,string drive_workspace_id,string pin_id,string kind,string root,optional string target_entity_id,u64 added_at_ms,optional u64 expires_at_ms);",
        "string drive_unpin_retention_json(LoomSession handle,string workspace,string drive_workspace_id,string pin_id);",
        "string drive_apply_retention_json(LoomSession handle,string workspace,string drive_workspace_id,u64 now_ms);",
    ]:
        assert _compact(signature) in idl
