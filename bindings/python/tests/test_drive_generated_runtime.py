import json
import re

import pytest
import uldrenai_loom


def test_drive_generated_read_and_hierarchy_wrappers_round_trip(tmp_path):
    path = str(tmp_path / "drive.loom")
    uldrenai_loom.create_loom(path, "default", None, None)
    uldrenai_loom.workspace_create(path, "studio", "vcs")

    root = json.loads(uldrenai_loom.drive_list_json(path, "studio", "drive-main", "root"))
    assert root["folder_id"] == "root"
    assert root["entries"] == []

    folder_a = json.loads(
        uldrenai_loom.drive_create_folder_json(
            path, "studio", "drive-main", "root", "folder-a", "A", root["profile_root"]
        )
    )
    assert folder_a["target_entity_id"] == "folder-a"
    with pytest.raises(RuntimeError, match=re.compile("CONFLICT|expected_root|profile root", re.I)):
        uldrenai_loom.drive_create_folder_json(
            path, "studio", "drive-main", "root", "stale", "Stale", root["profile_root"]
        )

    renamed = json.loads(
        uldrenai_loom.drive_rename_json(
            path,
            "studio",
            "drive-main",
            "root",
            "folder-a",
            "A2",
            folder_a["profile_root"],
        )
    )
    assert renamed["target_entity_id"] == "folder-a"
    stat = json.loads(uldrenai_loom.drive_stat_json(path, "studio", "drive-main", "root", "A2"))
    assert stat["node_id"] == "folder-a"

    folder_b = json.loads(
        uldrenai_loom.drive_create_folder_json(
            path, "studio", "drive-main", "root", "folder-b", "B", renamed["profile_root"]
        )
    )
    moved = json.loads(
        uldrenai_loom.drive_move_json(
            path,
            "studio",
            "drive-main",
            "root",
            "folder-b",
            "folder-a",
            folder_b["profile_root"],
        )
    )
    held_delete = json.loads(
        uldrenai_loom.drive_delete_json(
            path, "studio", "drive-main", "folder-b", "folder-a", renamed["profile_root"]
        )
    )
    assert held_delete["operation_kind"] == "folder.delete_held"
    assert len(json.loads(uldrenai_loom.drive_list_conflicts_json(path, "studio", "drive-main"))) == 1
    resolved = json.loads(
        uldrenai_loom.drive_resolve_conflict_json(
            path, "studio", "drive-main", held_delete["conflict_id"], "keep_current"
        )
    )
    assert resolved["operation_kind"] == "conflict.resolved"
    assert any(
        conflict["conflict_id"] == held_delete["conflict_id"] and conflict["resolution"] == "keep_current"
        for conflict in json.loads(uldrenai_loom.drive_list_conflicts_json(path, "studio", "drive-main"))
    )
    deleted = json.loads(
        uldrenai_loom.drive_delete_json(
            path, "studio", "drive-main", "folder-b", "folder-a", moved["profile_root"]
        )
    )
    assert deleted["target_entity_id"] == "folder-a"

    upload = json.loads(
        uldrenai_loom.drive_create_upload_json(
            path,
            "studio",
            "drive-main",
            "upload-1",
            "root",
            "nul.bin",
            "file-1",
            deleted["profile_root"],
            1000,
            False,
        )
    )
    assert upload["upload_id"] == "upload-1"
    payload = b"drive\x00bytes"
    uldrenai_loom.drive_upload_chunk_json(path, "studio", "drive-main", "upload-1", payload)
    committed = json.loads(
        uldrenai_loom.drive_commit_upload_json(path, "studio", "drive-main", "upload-1")
    )
    assert committed["target_entity_id"] == "file-1"
    assert uldrenai_loom.drive_read_file(path, "studio", "drive-main", "file-1") == payload
    assert len(json.loads(uldrenai_loom.drive_list_versions_json(path, "studio", "drive-main", "file-1"))) == 1
    assert len(json.loads(uldrenai_loom.drive_list_conflicts_json(path, "studio", "drive-main"))) >= 1

    uldrenai_loom.drive_grant_share_json(
        path,
        "studio",
        "drive-main",
        "grant-1",
        "file",
        "file-1",
        "05050505-0505-4505-8505-050505050505",
        "editor",
        2000,
        2500,
    )
    assert len(json.loads(uldrenai_loom.drive_list_shares_json(path, "studio", "drive-main"))) == 1
    share_no_op = json.loads(uldrenai_loom.drive_apply_share_expiry_json(path, "studio", "drive-main", 2100))
    assert share_no_op["remaining_grants"] == 1
    revoked = json.loads(uldrenai_loom.drive_revoke_share_json(path, "studio", "drive-main", "grant-1"))
    assert revoked["operation_kind"] == "share.revoked"
    assert len(json.loads(uldrenai_loom.drive_list_shares_json(path, "studio", "drive-main"))) == 0
    uldrenai_loom.drive_grant_share_json(
        path,
        "studio",
        "drive-main",
        "grant-expiring",
        "file",
        "file-1",
        "05050505-0505-4505-8505-050505050505",
        "viewer",
        2200,
        2300,
    )
    expired_share = json.loads(uldrenai_loom.drive_apply_share_expiry_json(path, "studio", "drive-main", 2300))
    assert expired_share["expired_grant_ids"] == ["grant-expiring"]
    assert len(json.loads(uldrenai_loom.drive_list_shares_json(path, "studio", "drive-main"))) == 0
    uldrenai_loom.drive_pin_retention_json(
        path,
        "studio",
        "drive-main",
        "pin-1",
        "legal_hold",
        committed["profile_root"],
        "file:file-1",
        3000,
    )
    assert len(json.loads(uldrenai_loom.drive_list_retention_json(path, "studio", "drive-main"))) == 1
    retention_no_op = json.loads(uldrenai_loom.drive_apply_retention_json(path, "studio", "drive-main", 3100))
    assert retention_no_op["remaining_pins"] == 1
    unpinned = json.loads(uldrenai_loom.drive_unpin_retention_json(path, "studio", "drive-main", "pin-1"))
    assert unpinned["operation_kind"] == "retention.unpinned"
    assert len(json.loads(uldrenai_loom.drive_list_retention_json(path, "studio", "drive-main"))) == 0
    uldrenai_loom.drive_pin_retention_json(
        path,
        "studio",
        "drive-main",
        "pin-expiring",
        "trash_subtree",
        committed["profile_root"],
        "file:file-1",
        3200,
        3300,
    )
    expired_retention = json.loads(uldrenai_loom.drive_apply_retention_json(path, "studio", "drive-main", 3300))
    assert expired_retention["expired_pin_ids"] == ["pin-expiring"]
    assert len(json.loads(uldrenai_loom.drive_list_retention_json(path, "studio", "drive-main"))) == 0

    reopened = json.loads(uldrenai_loom.drive_list_json(path, "studio", "drive-main", "root"))
    assert any(entry["node_id"] == "file-1" for entry in reopened["entries"])
