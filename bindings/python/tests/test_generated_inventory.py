from pathlib import Path
import re

import uldrenai_loom


ROOT = Path(__file__).parents[3]
PYTHON_PACKAGE = ROOT / "bindings" / "python" / "python" / "uldrenai_loom"

INVENTORY = [
    {
        "export": "lifecycle_define_standard_json",
        "idl": "string lifecycle_define_standard_json(LoomSession handle, string workspace, string kind, string version, string completion_predicate_digest);",
        "stub": "def lifecycle_define_standard_json(path: str, workspace: str, kind: str, version: str, completion_predicate_digest: str, store_passphrase: str | None = None, auth_principal: str | None = None, auth_passphrase: str | None = None) -> str: ...",
        "result": "str",
        "owner": "GeneratedLifecycle",
        "trait": "lifecycle_define_standard_json",
        "source": "src/lifecycle_refs.rs",
    },
    {
        "export": "lifecycle_define_json",
        "idl": "string lifecycle_define_json(LoomSession handle, string workspace, bytes definition);",
        "stub": "def lifecycle_define_json(path: str, workspace: str, definition: bytes, store_passphrase: str | None = None, auth_principal: str | None = None, auth_passphrase: str | None = None) -> str: ...",
        "result": "str",
        "owner": "GeneratedLifecycle",
        "trait": "lifecycle_define_json",
        "source": "src/lifecycle_refs.rs",
    },
    {
        "export": "lifecycle_instantiate_json",
        "idl": "string lifecycle_instantiate_json(LoomSession handle, string workspace, string instance_id, string definition_id, list<string> subject_refs);",
        "stub": "def lifecycle_instantiate_json(path: str, workspace: str, instance_id: str, definition_id: str, subject_refs: list[str], store_passphrase: str | None = None, auth_principal: str | None = None, auth_passphrase: str | None = None) -> str: ...",
        "result": "str",
        "owner": "GeneratedLifecycle",
        "trait": "lifecycle_instantiate_json",
        "source": "src/lifecycle_refs.rs",
    },
    {
        "export": "lifecycle_transition_json",
        "idl": "string lifecycle_transition_json(LoomSession handle, string workspace, string instance_id, string transition_id, string to_stage_id, optional string actor_principal_id, string gate_evaluations_json, optional string snapshot_digest);",
        "stub": "def lifecycle_transition_json(path: str, workspace: str, instance_id: str, transition_id: str, to_stage_id: str, actor_principal_id: str | None, gate_evaluations_json: str, snapshot_digest: str | None = None, store_passphrase: str | None = None, auth_principal: str | None = None, auth_passphrase: str | None = None) -> str: ...",
        "result": "str",
        "owner": "GeneratedLifecycle",
        "trait": "lifecycle_transition_json",
        "source": "src/lifecycle_refs.rs",
    },
    {
        "export": "refs_reconcile_json",
        "idl": "string refs_reconcile_json(LoomSession handle, string workspace, u64 max);",
        "stub": "def refs_reconcile_json(path: str, workspace: str, max: int, store_passphrase: str | None = None, auth_principal: str | None = None, auth_passphrase: str | None = None) -> str: ...",
        "result": "str",
        "owner": "GeneratedRefs",
        "trait": "refs_reconcile_json",
        "source": "src/lifecycle_refs.rs",
    },
    {
        "export": "apply_cbor",
        "idl": "bytes apply_cbor(LoomSession handle, bytes request);",
        "stub": "def apply_cbor(path: str, request: bytes, store_passphrase: str | None = None, auth_principal: str | None = None, auth_passphrase: str | None = None) -> bytes:",
        "result": "bytes",
        "owner": "GeneratedExec",
        "trait": "apply_cbor",
        "source": "src/exec_generated.rs",
    },
    {
        "export": "meetings_import_snapshot",
        "idl": "string meetings_import_snapshot(LoomSession handle, string workspace, string input_profile, bytes snapshot, bool dry_run );",
        "stub": "def meetings_import_snapshot( path: str, workspace: str, input_profile: str, snapshot: bytes, dry_run: bool, store_passphrase: str | None = None, auth_principal: str | None = None, auth_passphrase: str | None = None, ) -> str: ...",
        "result": "str",
        "owner": "GeneratedMeetings",
        "trait": "meetings_import_snapshot",
        "source": "src/meetings.rs",
    },
    {
        "export": "sql_exec_result",
        "idl": "bytes sql_exec_result(LoomSession handle, string workspace, string db, string sql);",
        "stub": "def sql_exec_result(path: str, workspace: str, db: str, sql: str, store_passphrase: str | None = None, auth_principal: str | None = None, auth_passphrase: str | None = None) -> bytes:",
        "result": "bytes",
        "owner": "GeneratedSql",
        "trait": "sql_exec_result",
        "source": "src/sql_generated.rs",
    },
]


def _normalized(path: Path) -> str:
    return " ".join(path.read_text().split())


def _compact_idl(value: str) -> str:
    compact = " ".join(value.split())
    for token in ("(", ")", ",", ";"):
        compact = compact.replace(f" {token}", token).replace(f"{token} ", token)
    return compact


def _rust_wrappers(binding: str, source: str) -> list[str]:
    text = (ROOT / "bindings" / binding / source).read_text()
    marker = "#[pyfunction]" if binding == "python" else "#[napi]"
    return [
        match.group(1)
        for match in re.finditer(
            rf"{re.escape(marker)}(?:\s*#\[[^\]]+\])*\s+(?:pub(?:\(crate\))?\s+)?fn\s+([a-z0-9_]+)(?:<[^>]+>)?\s*\(",
            text,
        )
    ]


def _is_python_group_name(name: str) -> bool:
    return (
        (name.startswith("lifecycle_") and name.endswith("_json"))
        or name == "refs_reconcile_json"
        or name == "apply_cbor"
        or name == "meetings_import_snapshot"
        or name == "sql_exec_result"
    )


def _is_node_group_name(name: str) -> bool:
    return (
        (name.startswith("lifecycle") and name.endswith("Json"))
        or name == "refsReconcileJson"
        or name == "applyCbor"
        or name == "meetingsImportSnapshot"
        or name == "sqlExecResult"
    )


def _snake_to_camel(value: str) -> str:
    parts = value.split("_")
    return parts[0] + "".join(part.capitalize() for part in parts[1:])


def test_python_generated_group_inventory_matches_exports_and_stubs():
    assert [entry["export"] for entry in INVENTORY] == [
        "lifecycle_define_standard_json",
        "lifecycle_define_json",
        "lifecycle_instantiate_json",
        "lifecycle_transition_json",
        "refs_reconcile_json",
        "apply_cbor",
        "meetings_import_snapshot",
        "sql_exec_result",
    ]
    package_source = (PYTHON_PACKAGE / "__init__.py").read_text()
    package_all = set(uldrenai_loom.__all__)
    stubs = _normalized(PYTHON_PACKAGE / "__init__.pyi")
    idl = _compact_idl((ROOT / "idl" / "loom.idl").read_text())
    generated_api = (ROOT / "crates" / "loom-remote-protocol" / "src" / "generated_api.rs").read_text()
    expected_python = [entry["export"] for entry in INVENTORY]
    actual_python_rust = [
        *(_rust_wrappers("python", "src/lifecycle_refs.rs")),
        *(_rust_wrappers("python", "src/exec_generated.rs")),
        *(
            name
            for name in _rust_wrappers("python", "src/meetings.rs")
            if name != "meetings_source_read"
        ),
        *(_rust_wrappers("python", "src/sql_generated.rs")),
    ]
    assert len(actual_python_rust) == len(INVENTORY)
    assert len(set(actual_python_rust)) == len(INVENTORY)
    assert actual_python_rust == expected_python

    actual_node_rust = [
        _snake_to_camel(name)
        for name in (
            *(_rust_wrappers("node", "src/lifecycle_refs.rs")),
            *(_rust_wrappers("node", "src/exec_generated.rs")),
            *(
                name
                for name in _rust_wrappers("node", "src/meetings.rs")
                if name != "meetings_source_read"
            ),
            *(_rust_wrappers("node", "src/sql_generated.rs")),
        )
    ]
    expected_node = [_snake_to_camel(name) for name in expected_python]
    assert len(actual_node_rust) == len(INVENTORY)
    assert len(set(actual_node_rust)) == len(INVENTORY)
    assert actual_node_rust == expected_node

    actual_module_public = sorted(name for name in dir(uldrenai_loom) if _is_python_group_name(name))
    assert len(actual_module_public) == len(INVENTORY)
    assert actual_module_public == sorted(expected_python)

    actual_all = sorted(name for name in package_all if _is_python_group_name(name))
    assert len(actual_all) == len(INVENTORY)
    assert actual_all == sorted(expected_python)

    actual_imports = sorted(
        line.strip().rstrip(",")
        for line in package_source.splitlines()
        if line.startswith("    ") and _is_python_group_name(line.strip().rstrip(","))
    )
    assert len(actual_imports) == len(INVENTORY)
    assert actual_imports == sorted(expected_python)

    actual_stubs = sorted(
        name
        for name in (
            line.split("def ", 1)[1].split("(", 1)[0]
            for line in (PYTHON_PACKAGE / "__init__.pyi").read_text().splitlines()
            if line.startswith("def ")
        )
        if _is_python_group_name(name)
    )
    assert len(actual_stubs) == len(INVENTORY)
    assert actual_stubs == sorted(expected_python)

    node_index = (ROOT / "bindings" / "node" / "index.js").read_text()
    actual_node_index = sorted(
        {
            line.split("module.exports.", 1)[1].split(" = ", 1)[0]
            for line in node_index.splitlines()
            if line.startswith("module.exports.") and _is_node_group_name(line.split("module.exports.", 1)[1].split(" = ", 1)[0])
        }
    )
    assert len(actual_node_index) == len(INVENTORY)
    assert actual_node_index == sorted(expected_node)

    node_dts = (ROOT / "bindings" / "node" / "index.d.ts").read_text()
    actual_node_dts = sorted(
        {
            line.split("export declare function ", 1)[1].split("(", 1)[0]
            for line in node_dts.splitlines()
            if line.startswith("export declare function ")
            and _is_node_group_name(line.split("export declare function ", 1)[1].split("(", 1)[0])
        }
    )
    assert len(actual_node_dts) == len(INVENTORY)
    assert actual_node_dts == sorted(expected_node)

    for entry in INVENTORY:
        assert callable(getattr(uldrenai_loom, entry["export"]))
        assert entry["export"] in package_all
        assert entry["export"] in package_source
        assert " ".join(entry["stub"].split()) in stubs
        assert _compact_idl(entry["idl"]) in idl
        owner = entry["owner"].removeprefix("Generated")
        assert f"Generated binding for `{owner}.{entry['trait']}`" in generated_api


def test_generated_bindings_and_security_admin_share_one_session_helper():
    for binding in ("node", "python"):
        generated_session = (ROOT / "bindings" / binding / "src" / "generated_session.rs").read_text()
        assert "pub(crate) fn open_generated_session" in generated_session
        assert "authenticate_passphrase" in generated_session
        assert "impl Drop for GeneratedSession" in generated_session

        security_admin = (ROOT / "bindings" / binding / "src" / "security_admin.rs").read_text()
        assert "generated_session::open_generated_session" in security_admin
        assert "LocalLoomClient::new" not in security_admin
        assert ".close(" not in security_admin
        assert "authenticate_passphrase" not in security_admin

    for entry in INVENTORY:
        for binding in ("node", "python"):
            source = (ROOT / "bindings" / binding / entry["source"]).read_text()
            assert "generated_session::open_generated_session" in source
            assert f"as {entry['owner']}" in source
            assert f"as {entry['owner']}>::{entry['trait']}" in source
            assert "LocalLoomClient::new" not in source
            assert ".close(" not in source
