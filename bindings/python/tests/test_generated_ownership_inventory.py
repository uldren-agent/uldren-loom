from pathlib import Path
import json
import re


ROOT = Path(__file__).parents[3]
PACKAGE = ROOT / "bindings" / "python" / "python" / "uldrenai_loom"


def _manifest_rows():
    contract = json.loads((ROOT / "idl" / "binding-targets.json").read_text())
    idl = (ROOT / "idl" / "loom.idl").read_text()
    assert contract["schema_version"] == 1
    assert contract["native_targets"] == [
        "c_abi",
        "cpp",
        "jvm",
        "android",
        "ios",
        "react_native",
        "nodejs",
        "python",
    ]
    assert contract["wasm_capability_gated"] == {
        "reason": "profile_unsupported",
        "code": "UNSUPPORTED",
    }
    rows = []
    for entry in contract["methods"]:
        owner, method = entry["name"].split(".")
        signature = _idl_signature(idl, owner, method)
        assert entry["wasm"] in {"supported", "capability_gated"}
        rows.append(
            {
                "owner": owner,
                "method": method,
                "result": "bytes" if signature.startswith("bytes ") else "str",
            }
        )
    assert len(rows) == 80
    assert len({f"{row['owner']}.{row['method']}" for row in rows}) == 80
    assert len({row["method"] for row in rows}) == 80
    return rows


def _idl_signature(idl: str, owner: str, method: str) -> str:
    interface_start = idl.index(f"interface {owner} {{")
    interface_tail = idl[interface_start:]
    interface_end = interface_tail.index("\n}")
    block = interface_tail[:interface_end]
    method_start = block.index(f" {method}(")
    before = max(block.rfind(";", 0, method_start), block.rfind("{", 0, method_start)) + 1
    after = block.index(";", method_start)
    return " ".join(block[before:after].split())


def _wrappers(source: str) -> list[str]:
    return [
        match.group(1)
        for match in re.finditer(
            r"#\[pyfunction\](?:\s*#\[[^\]]+\])*\s+pub\(crate\)\s+fn\s+([a-z0-9_]+)(?:<[^>]+>)?\s*\(",
            source,
        )
    ]


def _body_for(source: str, name: str) -> str:
    start = source.index(f"pub(crate) fn {name}")
    next_start = source.find("\n#[pyfunction]", start + 1)
    return source[start:] if next_start == -1 else source[start:next_start]


def _generated_owned_wrappers(sources: dict[str, str]) -> list[tuple[str, str, str]]:
    wrappers = []
    for path, source in sources.items():
        for name in _wrappers(source):
            body = _body_for(source, name)
            if "generated_session::open_generated_session" in body and f">::{name}" in body:
                wrappers.append((name, path, body))
    return wrappers


def test_generated_ownership_inventory_matches_accepted_manifest():
    rows = _manifest_rows()
    methods = [row["method"] for row in rows]
    source_dir = ROOT / "bindings" / "python" / "src"
    sources = {path.name: path.read_text() for path in source_dir.glob("*.rs")}
    generated = (ROOT / "crates" / "loom-remote-protocol" / "src" / "generated_api.rs").read_text()
    package = (PACKAGE / "__init__.py").read_text()
    stubs = (PACKAGE / "__init__.pyi").read_text()

    wrappers = _generated_owned_wrappers(sources)
    assert sorted(name for name, _, _ in wrappers) == sorted(methods)
    assert len(wrappers) == 80
    assert len({name for name, _, _ in wrappers}) == 80

    for row in rows:
        matches = [(path, body) for name, path, body in wrappers if name == row["method"]]
        assert len(matches) == 1
        path, body = matches[0]
        assert f"Generated binding for `{row['owner']}.{row['method']}`" in generated
        assert "generated_session::open_generated_session" in body
        assert f">::{row['method']}" in body
        assert "loom_chat::" not in body, f"{row['method']} direct chat owner in {path}"
        assert "loom_drive::" not in body, f"{row['method']} direct drive owner in {path}"
        assert "loom_lifecycle::" not in body, f"{row['method']} direct lifecycle owner in {path}"
        assert "loom_reference::" not in body, f"{row['method']} direct refs owner in {path}"
        assert "LoomSqlStore" not in body, f"{row['method']} direct SQL owner in {path}"
        assert "open_loom" not in body, f"{row['method']} direct open in {path}"
        assert "save_loom" not in body, f"{row['method']} direct save in {path}"
        assert "LocalLoomClient::new" not in body, f"{row['method']} direct client construction in {path}"

    package_imports = [
        line.strip().rstrip(",")
        for line in package.splitlines()
        if line.startswith("    ") and line.strip().rstrip(",") in methods
    ]
    assert len(package_imports) == 80
    assert len(set(package_imports)) == 80
    assert sorted(package_imports) == sorted(methods)

    package_all = [
        line.strip().strip('",')
        for line in package.splitlines()
        if line.startswith("    \"") and line.strip().strip('",') in methods
    ]
    assert len(package_all) == 80
    assert len(set(package_all)) == 80
    assert sorted(package_all) == sorted(methods)

    stub_functions = [
        (match.group(1), match.group(2))
        for match in re.finditer(
            r"def ([a-z0-9_]+)\([^)]*\) -> (bytes|str):(?: \.\.\.|\n(?:    .+\n)*?    \.\.\.)",
            stubs,
        )
        if match.group(1) in methods
    ]
    assert len(stub_functions) == 80
    assert len({name for name, _ in stub_functions}) == 80
    assert sorted(stub_functions) == sorted((row["method"], row["result"]) for row in rows)

    exclusions = []
    assert exclusions == []
