from pathlib import Path
import re
from io import BytesIO
from zipfile import ZIP_STORED, ZipFile

import pytest
import uldrenai_loom


ROOT = Path(__file__).parents[3]
PACKAGE = ROOT / "bindings" / "python" / "python" / "uldrenai_loom"

INVENTORY = [
    (
        "import_table_csv",
        "bytes import_table_csv(LoomSession handle, string workspace, string source_scope, bytes csv_payload, string database, string table, string schema, string primary_key, string mode, bool commit, optional string author, optional string message, bool dry_run);",
        "def import_table_csv(path: str, workspace: str, source_scope: str, csv_payload: bytes, database: str, table: str, schema: str, primary_key: str, mode: str, commit: bool, author: str | None, message: str | None, dry_run: bool, store_passphrase: str | None = None, auth_principal: str | None = None, auth_passphrase: str | None = None) -> bytes: ...",
        [
            ("py", "Python<'py>"),
            ("path", "&str"),
            ("workspace", "&str"),
            ("source_scope", "&str"),
            ("csv_payload", "&[u8]"),
            ("database", "&str"),
            ("table", "&str"),
            ("schema", "&str"),
            ("primary_key", "&str"),
            ("mode", "&str"),
            ("commit", "bool"),
            ("author", "Option<&str>"),
            ("message", "Option<&str>"),
            ("dry_run", "bool"),
            ("store_passphrase", "Option<&str>"),
            ("auth_principal", "Option<&str>"),
            ("auth_passphrase", "Option<&str>"),
        ],
    ),
    (
        "import_redmine",
        "bytes import_redmine(LoomSession handle, string workspace, string profile, string source_scope, bytes snapshot_payload, string field_policy, bool dry_run);",
        "def import_redmine(path: str, workspace: str, profile: str, source_scope: str, snapshot_payload: bytes, field_policy: str, dry_run: bool, store_passphrase: str | None = None, auth_principal: str | None = None, auth_passphrase: str | None = None) -> bytes: ...",
        "field_policy",
    ),
    (
        "import_asana",
        "bytes import_asana(LoomSession handle, string workspace, string profile, string source_scope, bytes snapshot_payload, string field_policy, bool dry_run);",
        "def import_asana(path: str, workspace: str, profile: str, source_scope: str, snapshot_payload: bytes, field_policy: str, dry_run: bool, store_passphrase: str | None = None, auth_principal: str | None = None, auth_passphrase: str | None = None) -> bytes: ...",
        "field_policy",
    ),
    (
        "import_jira",
        "bytes import_jira(LoomSession handle, string workspace, string profile, string source_scope, bytes snapshot_payload, string field_policy, bool dry_run);",
        "def import_jira(path: str, workspace: str, profile: str, source_scope: str, snapshot_payload: bytes, field_policy: str, dry_run: bool, store_passphrase: str | None = None, auth_principal: str | None = None, auth_passphrase: str | None = None) -> bytes: ...",
        "field_policy",
    ),
    (
        "import_confluence",
        "bytes import_confluence(LoomSession handle, string workspace, string profile, string source_scope, bytes snapshot_payload, string default_space, bool dry_run);",
        "def import_confluence(path: str, workspace: str, profile: str, source_scope: str, snapshot_payload: bytes, default_space: str, dry_run: bool, store_passphrase: str | None = None, auth_principal: str | None = None, auth_passphrase: str | None = None) -> bytes: ...",
        "default_space",
    ),
    (
        "import_slack",
        "bytes import_slack(LoomSession handle, string workspace, string profile, string source_scope, bytes snapshot_payload, bool dry_run);",
        "def import_slack(path: str, workspace: str, profile: str, source_scope: str, snapshot_payload: bytes, dry_run: bool, store_passphrase: str | None = None, auth_principal: str | None = None, auth_passphrase: str | None = None) -> bytes: ...",
        "payload",
    ),
    (
        "import_drive",
        "bytes import_drive(LoomSession handle, string workspace, string profile, string source_scope, bytes archive_payload, bool dry_run);",
        "def import_drive(path: str, workspace: str, profile: str, source_scope: str, archive_payload: bytes, dry_run: bool, store_passphrase: str | None = None, auth_principal: str | None = None, auth_passphrase: str | None = None) -> bytes: ...",
        "archive",
    ),
    (
        "import_markdown",
        "bytes import_markdown(LoomSession handle, string workspace, string profile, string source_scope, bytes archive_payload, string space, bool dry_run);",
        "def import_markdown(path: str, workspace: str, profile: str, source_scope: str, archive_payload: bytes, space: str, dry_run: bool, store_passphrase: str | None = None, auth_principal: str | None = None, auth_passphrase: str | None = None) -> bytes: ...",
        "space",
    ),
    (
        "import_notion",
        "bytes import_notion(LoomSession handle, string workspace, string profile, string source_scope, bytes snapshot_payload, string default_space, bool dry_run);",
        "def import_notion(path: str, workspace: str, profile: str, source_scope: str, snapshot_payload: bytes, default_space: str, dry_run: bool, store_passphrase: str | None = None, auth_principal: str | None = None, auth_passphrase: str | None = None) -> bytes: ...",
        "default_space",
    ),
]

PARAM_GROUPS = {
    "field_policy": [
        ("py", "Python<'py>"),
        ("path", "&str"),
        ("workspace", "&str"),
        ("profile", "&str"),
        ("source_scope", "&str"),
        ("snapshot_payload", "&[u8]"),
        ("field_policy", "&str"),
        ("dry_run", "bool"),
        ("store_passphrase", "Option<&str>"),
        ("auth_principal", "Option<&str>"),
        ("auth_passphrase", "Option<&str>"),
    ],
    "default_space": [
        ("py", "Python<'py>"),
        ("path", "&str"),
        ("workspace", "&str"),
        ("profile", "&str"),
        ("source_scope", "&str"),
        ("snapshot_payload", "&[u8]"),
        ("default_space", "&str"),
        ("dry_run", "bool"),
        ("store_passphrase", "Option<&str>"),
        ("auth_principal", "Option<&str>"),
        ("auth_passphrase", "Option<&str>"),
    ],
    "payload": [
        ("py", "Python<'py>"),
        ("path", "&str"),
        ("workspace", "&str"),
        ("profile", "&str"),
        ("source_scope", "&str"),
        ("snapshot_payload", "&[u8]"),
        ("dry_run", "bool"),
        ("store_passphrase", "Option<&str>"),
        ("auth_principal", "Option<&str>"),
        ("auth_passphrase", "Option<&str>"),
    ],
    "archive": [
        ("py", "Python<'py>"),
        ("path", "&str"),
        ("workspace", "&str"),
        ("profile", "&str"),
        ("source_scope", "&str"),
        ("archive_payload", "&[u8]"),
        ("dry_run", "bool"),
        ("store_passphrase", "Option<&str>"),
        ("auth_principal", "Option<&str>"),
        ("auth_passphrase", "Option<&str>"),
    ],
    "space": [
        ("py", "Python<'py>"),
        ("path", "&str"),
        ("workspace", "&str"),
        ("profile", "&str"),
        ("source_scope", "&str"),
        ("archive_payload", "&[u8]"),
        ("space", "&str"),
        ("dry_run", "bool"),
        ("store_passphrase", "Option<&str>"),
        ("auth_principal", "Option<&str>"),
        ("auth_passphrase", "Option<&str>"),
    ],
}


def _compact(value: str) -> str:
    compact = " ".join(value.split())
    for token in ("(", ")", ",", ";"):
        compact = compact.replace(f" {token}", token).replace(f"{token} ", token)
    return compact


def _rust_wrappers(path: Path) -> list[str]:
    text = path.read_text()
    return [
        match.group(1)
        for match in re.finditer(
            r"#\[pyfunction\](?:\s*#\[[^\]]+\])*\s+pub\(crate\)\s+fn\s+([a-z0-9_]+)(?:<[^>]+>)?\s*\(",
            text,
        )
    ]

def _rust_signature(source: str, name: str):
    start = source.index(f"fn {name}")
    params_start = source.index("(", start)
    depth = 0
    for index in range(params_start, len(source)):
        if source[index] == "(":
            depth += 1
        elif source[index] == ")":
            depth -= 1
        if depth == 0:
            params = []
            for part in source[params_start + 1 : index].split(","):
                part = part.strip()
                if not part:
                    continue
                param, kind = part.split(":", 1)
                params.append((param.strip(), kind.strip()))
            result = " ".join(source[index + 1 : source.index("{", index)].split())
            return params, result
    raise AssertionError(f"unterminated signature for {name}")


def _read_uint(data: bytes, offset: int, info: int) -> tuple[int, int]:
    if info < 24:
        return info, offset
    if info == 24:
        return data[offset], offset + 1
    if info == 25:
        return int.from_bytes(data[offset : offset + 2], "big"), offset + 2
    if info == 26:
        return int.from_bytes(data[offset : offset + 4], "big"), offset + 4
    raise AssertionError(f"unsupported uint width {info}")


def _cbor(data: bytes, offset: int = 0):
    head = data[offset]
    offset += 1
    major = head >> 5
    info = head & 0x1F
    if major == 0:
        return _read_uint(data, offset, info)
    if major == 3:
        length, offset = _read_uint(data, offset, info)
        return data[offset : offset + length].decode(), offset + length
    if major == 4:
        length, offset = _read_uint(data, offset, info)
        values = []
        for _ in range(length):
            value, offset = _cbor(data, offset)
            values.append(value)
        return values, offset
    if major == 7 and info == 20:
        return False, offset
    if major == 7 and info == 21:
        return True, offset
    if major == 7 and info == 22:
        return None, offset
    raise AssertionError(f"unsupported cbor major {major}")


def _report(data: bytes) -> dict[str, object]:
    value, offset = _cbor(data)
    assert offset == len(data)
    return {
        "profile": value[0],
        "source_scope": value[1],
        "commit": value[2],
        "bytes_in": value[4],
        "rows_imported": value[6],
        "operations_planned": value[8],
        "operations_applied": value[9],
        "dry_run": value[10],
    }


def _stable_error(fn, token: str):
    with pytest.raises(RuntimeError, match=token):
        fn()

def _zip_bytes(entries: list[tuple[str, bytes]]) -> bytes:
    buf = BytesIO()
    with ZipFile(buf, "w", ZIP_STORED) as archive:
        for name, content in entries:
            archive.writestr(name, content)
    return buf.getvalue()


def test_interchange_profile_inventory_matches_exact_generated_contracts():
    expected = [name for name, _, _, _ in INVENTORY]
    source = ROOT / "bindings" / "python" / "src" / "interchange_profiles.rs"
    rust_names = _rust_wrappers(source)
    assert rust_names == expected
    assert len(rust_names) == 9
    assert len(set(rust_names)) == 9

    idl = _compact((ROOT / "idl" / "loom.idl").read_text())
    generated = (ROOT / "crates" / "loom-remote-protocol" / "src" / "generated_api.rs").read_text()
    source_text = source.read_text()
    package = (PACKAGE / "__init__.py").read_text()
    stubs = " ".join((PACKAGE / "__init__.pyi").read_text().split())
    exports = [name for name in uldrenai_loom.__all__ if name.startswith("import_")]
    package_imports = [
        line.strip().rstrip(",")
        for line in package.splitlines()
        if line.startswith("    import_")
    ]
    stub_names = [
        line.split("def ", 1)[1].split("(", 1)[0]
        for line in (PACKAGE / "__init__.pyi").read_text().splitlines()
        if line.startswith("def import_")
    ]

    assert exports == expected
    assert package_imports == expected
    assert stub_names == expected
    for name, idl_signature, stub, params in INVENTORY:
        expected_params = params if isinstance(params, list) else PARAM_GROUPS[params]
        actual_params, result = _rust_signature(source_text, name)
        assert actual_params == expected_params
        assert result == "-> PyResult<Bound<'py, PyBytes>>"
        assert callable(getattr(uldrenai_loom, name))
        assert name in package
        assert " ".join(stub.split()) in stubs
        assert _compact(idl_signature) in idl
        assert f"Generated binding for `InterchangeProfiles.{name}`" in generated
        assert f"as GeneratedInterchangeProfiles>::{name}" in source_text
    assert "generated_session::open_generated_session" in source_text
    assert "LocalLoomClient::new" not in source_text
    assert ".close(" not in source_text


def test_table_csv_wrapper_preserves_bytes_dry_run_and_reopen(tmp_path: Path):
    store = str(tmp_path / "table.loom")
    uldrenai_loom.create_loom(store, "default")
    uldrenai_loom.workspace_create(store, "main", "sql")
    payload = b'id,name,note\n1,alpha,"nul\x00byte"\n'

    dry = uldrenai_loom.import_table_csv(
        store,
        "main",
        "memory://items-dry.csv",
        payload,
        "app",
        "items",
        "id:int,name:text,note:text",
        "id",
        "snapshot",
        False,
        None,
        None,
        True,
    )
    assert isinstance(dry, bytes)
    dry_report = _report(dry)
    assert dry_report["profile"] == "table-csv"
    assert dry_report["source_scope"] == "memory://items-dry.csv"
    assert dry_report["bytes_in"] == len(payload)
    assert dry_report["rows_imported"] == 1
    assert dry_report["operations_applied"] == 0
    assert dry_report["dry_run"] is True
    assert dry_report["commit"] is None
    fresh = uldrenai_loom.import_table_csv(
        store,
        "main",
        "memory://items-dry.csv",
        payload,
        "app",
        "items",
        "id:int,name:text,note:text",
        "id",
        "snapshot",
        False,
        "Author",
        "Message",
        True,
    )
    assert fresh == dry
    _stable_error(
        lambda: uldrenai_loom.sql_read_table(store, "main", ".loom/facets/sql/app/tables/items"),
        "NOT_FOUND|not found|unknown|no such",
    )

    written = uldrenai_loom.import_table_csv(
        store,
        "main",
        "memory://items-write.csv",
        b"id,name,note\n1,alpha,persisted\n",
        "app",
        "items",
        "id:int,name:text,note:text",
        "id",
        "snapshot",
        True,
        "Author",
        "Message",
        False,
    )
    assert _report(written)["operations_applied"] == 1
    assert _report(written)["commit"].startswith("blake3:")
    assert len(uldrenai_loom.sql_read_table(store, "main", ".loom/facets/sql/app/tables/items")) > 0
    assert not __import__("json").loads(uldrenai_loom.status_json(store, "sql", "main"))["untracked"]


def test_table_csv_commit_false_publishes_without_vcs_commit(tmp_path: Path):
    store = str(tmp_path / "table_no_commit.loom")
    uldrenai_loom.create_loom(store, "default")
    uldrenai_loom.workspace_create(store, "main", "sql")

    written = uldrenai_loom.import_table_csv(
        store,
        "main",
        "memory://items-write-no-commit.csv",
        b"id,name,note\n1,alpha,published\n",
        "app",
        "items",
        "id:int,name:text,note:text",
        "id",
        "snapshot",
        False,
        "Author",
        "Message",
        False,
    )
    assert _report(written)["commit"] is None
    assert _report(written)["operations_applied"] == 1
    assert len(uldrenai_loom.sql_read_table(store, "main", ".loom/facets/sql/app/tables/items")) > 0
    assert __import__("json").loads(uldrenai_loom.status_json(store, "sql", "main"))["untracked"] == [
        ".loom/facets/sql/app/tables/items"
    ]


def test_profile_wrappers_successfully_forward_profile_specific_arguments(tmp_path: Path):
    store = str(tmp_path / "profiles_success.loom")
    uldrenai_loom.create_loom(store, "default")
    uldrenai_loom.workspace_create(store, "main")
    redmine = b'{"projects":[{"id":1,"identifier":"core","key_prefix":"CORE","name":"Core"}],"issues":[{"id":42,"project_identifier":"core","tracker":"Bug","subject":"Login fails","description":"Fails","status":"New","priority":"High","custom_fields":{"severity":"critical"}}]}'
    asana = b'{"projects":[{"gid":"p1","key_prefix":"AS","name":"Project"}],"tasks":[{"gid":"t1","project_gid":"p1","name":"Task","notes":"Notes","resource_subtype":"default_task","completed":false,"custom_fields":{"size":"M"}}]}'
    jira = b'{"projects":[{"id":10001,"key":"CORE","name":"Core"}],"issues":[{"id":10042,"key":"CORE-42","project_key":"CORE","issue_type":"Bug","summary":"Login fails","description":"Fails","status":"To Do","priority":"High","custom_fields":{"severity":"critical"}}]}'
    confluence = b'{"pages":[{"id":"home","title":"Home","text":"Hello"}]}'
    slack = b'{"channels":[{"id":"C1","name":"general","messages":[{"ts":"1710000000.000100","user":"U1","text":"Hello"}]}]}'
    drive = _zip_bytes([("manifest.json", b'{"files":[{"id":"readme","name":"README.md","text":"Inline text"}]}')])
    markdown = _zip_bytes([("Intro.md", b"# Intro\nHello\n")])
    notion = b'{"pages":[{"id":"intro","title":"Intro","markdown":"# Intro"}]}'
    cases = [
        ("redmine", redmine, lambda: uldrenai_loom.import_redmine(store, "main", "redmine", "redmine://fixture", redmine, "infer", True)),
        ("asana", asana, lambda: uldrenai_loom.import_asana(store, "main", "asana", "asana://fixture", asana, "infer", True)),
        ("jira", jira, lambda: uldrenai_loom.import_jira(store, "main", "jira", "jira://fixture", jira, "infer", True)),
        ("confluence", confluence, lambda: uldrenai_loom.import_confluence(store, "main", "confluence", "confluence://fixture", confluence, "wiki", True)),
        ("slack", slack, lambda: uldrenai_loom.import_slack(store, "main", "slack", "slack://fixture", slack, True)),
        ("drive", drive, lambda: uldrenai_loom.import_drive(store, "main", "drive", "drive://fixture", drive, True)),
        ("markdown", markdown, lambda: uldrenai_loom.import_markdown(store, "main", "markdown", "markdown://fixture", markdown, "docs", True)),
        ("notion", notion, lambda: uldrenai_loom.import_notion(store, "main", "notion", "notion://fixture", notion, "wiki", True)),
    ]
    for expected_profile, payload, call in cases:
        parsed = _report(call())
        assert parsed["profile"] == expected_profile
        assert parsed["bytes_in"] == len(payload)
        assert parsed["dry_run"] is True
        assert parsed["operations_applied"] == 0
        assert parsed["rows_imported"] >= 1 or parsed["operations_planned"] >= 1


def test_profile_wrappers_forward_arguments_and_reject_before_payload_parse(tmp_path: Path):
    store = str(tmp_path / "profiles.loom")
    uldrenai_loom.create_loom(store, "default")
    uldrenai_loom.workspace_create(store, "main")
    payload = b"{ bad\x00json"

    _stable_error(lambda: uldrenai_loom.import_redmine(store, "main", "redmine", "redmine://bad", payload, "reject", True), "INVALID_ARGUMENT|invalid")
    _stable_error(lambda: uldrenai_loom.import_notion(store, "main", "notion", "notion://bad", payload, "wiki", True), "INVALID_ARGUMENT|invalid")
    _stable_error(
        lambda: uldrenai_loom.import_notion(
            store,
            "main",
            "notion",
            "notion://bad",
            payload,
            "wiki",
            True,
            None,
            "principal-only",
            None,
        ),
        "auth_principal and auth_passphrase",
    )
