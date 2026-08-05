from pathlib import Path
import re


ROOT = Path(__file__).parents[3]
PACKAGE = ROOT / "bindings" / "python" / "python" / "uldrenai_loom"

INVENTORY = [
    ("serve_listener_configure_json", "request_json: str"),
    ("serve_listener_list_json", ""),
    ("serve_listener_set_enabled_json", "listener_id: str, enabled: bool"),
    ("serve_listener_remove_json", "listener_id: str"),
    ("serve_web_route_list_json", "listener_id: str"),
    ("serve_web_route_set_json", "request_json: str"),
    ("serve_web_route_remove_json", "listener_id: str, route_id: str"),
]


def _compact(value: str) -> str:
    compact = " ".join(value.split())
    for token in ("(", ")", ",", ";"):
        compact = compact.replace(f" {token}", token).replace(f"{token} ", token)
    return compact


def test_serve_config_generated_inventory_matches_source_boundaries():
    source = (ROOT / "bindings" / "python" / "src" / "serve_config.rs").read_text()
    module = (ROOT / "bindings" / "python" / "src" / "lib.rs").read_text()
    package = (PACKAGE / "__init__.py").read_text()
    stubs = (PACKAGE / "__init__.pyi").read_text()
    idl = _compact((ROOT / "idl" / "loom.idl").read_text())
    generated = (ROOT / "crates" / "loom-remote-protocol" / "src" / "generated_api.rs").read_text()

    wrappers = [
        match.group(1)
        for match in re.finditer(
            r"#\[pyfunction\](?:\s*#\[[^\]]+\])*\s+pub\(crate\)\s+fn\s+([a-z0-9_]+)\s*\(",
            source,
        )
    ]
    assert wrappers == [name for name, _ in INVENTORY]
    assert len(set(wrappers)) == 7

    registrations = [
        match.group(1)
        for match in re.finditer(r"wrap_pyfunction!\(\s*serve_config::([a-z0-9_]+),", module)
    ]
    assert registrations == [name for name, _ in INVENTORY]

    package_imports = [
        line.strip().rstrip(",")
        for line in package.splitlines()
        if line.startswith("    ")
        and any(line.strip().rstrip(",") == name for name, _ in INVENTORY)
    ]
    assert package_imports == [name for name, _ in INVENTORY]

    package_all = [
        line.strip().strip('",')
        for line in package.splitlines()
        if line.startswith("    \"")
        and any(line.strip().strip('",') == name for name, _ in INVENTORY)
    ]
    assert package_all == [name for name, _ in INVENTORY]

    stub_functions = [
        (match.group(1), match.group(2))
        for match in re.finditer(r"def ([a-z0-9_]+)\(([^)]*)\) -> str: \.\.\.", stubs)
        if any(match.group(1) == name for name, _ in INVENTORY)
    ]
    assert stub_functions == [
        (
            name,
            ", ".join(
                part
                for part in [
                    "path: str",
                    idl_args,
                    "store_passphrase: str | None = None",
                    "auth_principal: str | None = None",
                    "auth_passphrase: str | None = None",
                ]
                if part
            ),
        )
        for name, idl_args in INVENTORY
    ]

    for name, _ in INVENTORY:
        assert f"Generated binding for `ServeConfig.{name}`" in generated
        assert f">::{name}" in source
        assert "generated_session::open_generated_session" in source

    for signature in [
        "string serve_listener_configure_json(LoomSession handle,string request_json);",
        "string serve_listener_list_json(LoomSession handle);",
        "string serve_listener_set_enabled_json(LoomSession handle,string listener_id,bool enabled);",
        "string serve_listener_remove_json(LoomSession handle,string listener_id);",
        "string serve_web_route_list_json(LoomSession handle,string listener_id);",
        "string serve_web_route_set_json(LoomSession handle,string request_json);",
        "string serve_web_route_remove_json(LoomSession handle,string listener_id,string route_id);",
    ]:
        assert _compact(signature) in idl

    assert "LocalLoomClient::new" not in source
    assert ".close(" not in source
    assert "serde_json" not in source
    assert ".serve_listener_" not in source
    assert ".serve_web_route_" not in source
