"""Smoke tests for the Uldren Loom Python binding.

The canonical "abc" vector lives in the `uldren-loom-conformance` crate; here we only assert the
address shape so the test needs no hard-coded digest: "blake3:" + 64 hex chars.
"""

import asyncio
import json
import re
from pathlib import Path

import pytest
import uldrenai_loom

# Shared cross-language conformance fixture: every binding must reproduce these exact canonical
# bytes through its own typed exec path. bindings/python/tests -> bindings/ is two parents up.
_FIXTURE = json.loads(
    (Path(__file__).parents[2] / "conformance" / "result-vectors.json").read_text()
)


def test_version_is_non_empty():
    assert uldrenai_loom.version() != ""


def test_capabilities_report():
    # capabilities() returns canonical CBOR; decode it to JSON via the result renderer.
    raw = uldrenai_loom.capabilities()
    assert isinstance(raw, (bytes, bytearray)) and len(raw) > 0
    report = json.loads(uldrenai_loom.result_to_json(raw))
    by_name = {c["capability_id"]: c for c in report["records"]}
    assert by_name["object-store"]["operational_state"] == "supported"
    # Build-aware overlay: this binding links loom-store and loom-sql.
    assert by_name["single-file-store"]["operational_state"] == "supported"
    assert by_name["sql"]["operational_state"] == "supported"
    # This binding exposes the promoted Lane API over canonical CBOR records.
    assert by_name["lanes"]["operational_state"] == "supported"
    # A target capability is present in the catalog but not operationally supported.
    assert by_name["acl"]["operational_state"] == "unsupported"
    assert "supported" not in by_name["acl"]


def test_lane_functions_are_exported():
    for name in (
        "lanes_create",
        "lanes_get",
        "lanes_list",
        "lanes_update",
        "lanes_ticket_add",
        "lanes_ticket_remove",
    ):
        assert callable(getattr(uldrenai_loom, name))


def test_runtime_profile_report():
    raw = uldrenai_loom.runtime_profile()
    assert isinstance(raw, (bytes, bytearray)) and len(raw) > 0
    report = json.loads(uldrenai_loom.result_to_json(raw))
    assert report["binary_channel"] in {"standard", "fips"}
    assert report["runtime_policy"] in {"capable", "strict"}
    assert report["default_identity_profile"] in {"blake3", "sha256"}
    assert "crypto_provider" in report


def test_studio_surface_catalog_json():
    catalog = json.loads(uldrenai_loom.studio_surface_catalog_json("studio", "core"))
    assert catalog["workspace"] == "studio"
    assert catalog["set"] == "core"
    assert any(app["app_id"] == "ticket-details" for app in catalog["apps"])
    with pytest.raises(RuntimeError, match="unsupported Studio surface catalog set"):
        uldrenai_loom.studio_surface_catalog_json("studio", "bogus")


def test_meetings_import_snapshot_and_source_read(tmp_path):
    path = str(tmp_path / "meetings.loom")
    uldrenai_loom.create_loom(path, "default", None, None)
    uldrenai_loom.workspace_create(path, "studio", "vcs")
    snapshot = json.dumps(
        {
            "snapshot_version": 1,
            "profile": "granola-app",
            "source_system": "granola-app",
            "source_scope": "local-cache",
            "observed_at": 500,
            "coverage": "complete",
            "items": [
                {
                    "source_entity_id": "note-1",
                    "source_digest": f"blake3:{'0' * 64}",
                    "source_sidecar": {"id": "note-1", "raw": True},
                    "title": "Planning",
                    "summary_text": "Planning summary",
                    "transcript_spans": [{"text": "Capture decisions."}],
                    "decisions": [{"label": "Use normalized meeting imports."}],
                }
            ],
        }
    ).encode()
    report = json.loads(
        uldrenai_loom.meetings_import_snapshot(
            path, "studio", "granola-app", snapshot, False
        )
    )
    assert report["profile"] == "meetings"
    assert report["rows_imported"] == 1
    assert (
        uldrenai_loom.meetings_source_read(path, "studio", "note-1", "summary.txt")
        == b"Planning summary"
    )

def test_drive_round_trip(tmp_path):
    path = str(tmp_path / "drive.loom")
    uldrenai_loom.create_loom(path, "default", None, None)
    uldrenai_loom.workspace_create(path, "studio", "vcs")
    root = json.loads(uldrenai_loom.drive_list_json(path, "studio", "drive-main", "root"))
    assert root["folder_id"] == "root"
    assert root["entries"] == []
    uldrenai_loom.drive_create_folder_json(
        path, "studio", "drive-main", "root", "folder-1", "Specs", root["profile_root"]
    )
    folder_root = json.loads(
        uldrenai_loom.drive_list_json(path, "studio", "drive-main", "root")
    )["profile_root"]
    upload = json.loads(
        uldrenai_loom.drive_create_upload_json(
            path,
            "studio",
            "drive-main",
            "upload-1",
            "folder-1",
            "readme.txt",
            "file-1",
            folder_root,
            1000,
            False,
        )
    )
    assert upload["upload_id"] == "upload-1"
    uldrenai_loom.drive_upload_chunk_json(
        path, "studio", "drive-main", "upload-1", b"drive bytes"
    )
    committed = json.loads(
        uldrenai_loom.drive_commit_upload_json(path, "studio", "drive-main", "upload-1")
    )
    assert committed["target_entity_id"] == "file-1"
    assert uldrenai_loom.drive_read_file(path, "studio", "drive-main", "file-1") == b"drive bytes"
    assert len(json.loads(uldrenai_loom.drive_list_versions_json(path, "studio", "drive-main", "file-1"))) == 1
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
    )
    assert len(json.loads(uldrenai_loom.drive_list_shares_json(path, "studio", "drive-main"))) == 1
    uldrenai_loom.drive_pin_retention_json(
        path,
        "studio",
        "drive-main",
        "pin-1",
        "legal_hold",
        committed["profile_root"],
        "file-1",
        3000,
    )
    assert len(json.loads(uldrenai_loom.drive_list_retention_json(path, "studio", "drive-main"))) == 1


def test_blob_digest_shape():
    digest = uldrenai_loom.blob_digest(b"abc")
    assert re.fullmatch(r"blake3:[0-9a-f]{64}", digest) is not None


def test_watch_subscribe_poll_round_trip(tmp_path):
    path = str(tmp_path / "watch.loom")
    uldrenai_loom.create_loom(path, "default", None, None)
    db = uldrenai_loom.LoomSql(path, "watchapp", "main")
    db.exec("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)")
    db.exec("INSERT INTO t VALUES (1, 'a')")
    cursor = uldrenai_loom.watch_subscribe(path, "watchapp", "main")
    assert re.fullmatch(r"blake3:[0-9a-f]{64}", db.commit("seed", "python")) is not None
    batch = uldrenai_loom.watch_poll(path, cursor, 10)
    assert b"loom.watch.batch.v1" in batch
    assert b"unsupported_domains" in batch
    assert b"sql" in batch


def test_sql_session_round_trip(tmp_path):
    path = str(tmp_path / "app.loom")
    db = uldrenai_loom.LoomSql(path, "app", "main")
    db.exec("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)")
    db.exec("INSERT INTO t VALUES (1, 'hello')")
    # `exec` returns typed results: a list of statement dicts; a select carries idiomatic cells.
    payloads = db.exec("SELECT id, v FROM t")
    assert payloads[0]["kind"] == "select"
    assert payloads[0]["rows"] == [[1, "hello"]]
    # `exec_json` is the JSON debug form; `exec_bytes` is the canonical wire form.
    assert "hello" in json.dumps(json.loads(db.exec_json("SELECT v FROM t")))
    assert isinstance(db.exec_bytes("SELECT v FROM t"), bytes)
    commit = db.commit("seed", "tester")
    assert re.fullmatch(r"blake3:[0-9a-f]{64}", commit) is not None

    # The committed table is durable: a fresh session over the same .loom queries it.
    reopened = uldrenai_loom.LoomSql(path, "app", "main")
    assert reopened.exec("SELECT v FROM t")[0]["rows"] == [["hello"]]


def test_document_text_and_binary_binding_parity(tmp_path):
    path = str(tmp_path / "docs.loom")
    uldrenai_loom.create_loom(path, "default", None, None)
    text_digest = uldrenai_loom.doc_put_text(path, "docs", "notes", "a", "hello text")
    assert re.fullmatch(r"blake3:[0-9a-f]{64}", text_digest) is not None
    assert uldrenai_loom.doc_get_text(path, "docs", "notes", "a") == (
        "hello text",
        text_digest,
    )
    assert uldrenai_loom.doc_get_text(path, "docs", "notes", "missing") is None
    with pytest.raises(RuntimeError, match="CAS_MISMATCH"):
        uldrenai_loom.doc_put_text(
            path,
            "docs",
            "notes",
            "a",
            "stale",
            uldrenai_loom.blob_digest(b"stale"),
        )
    updated_digest = uldrenai_loom.doc_put_text(
        path,
        "docs",
        "notes",
        "a",
        "updated text",
        text_digest,
    )
    assert updated_digest != text_digest
    binary_digest = uldrenai_loom.doc_put_binary(
        path,
        "docs",
        "notes",
        "raw",
        b"\xff\x00",
    )
    assert re.fullmatch(r"blake3:[0-9a-f]{64}", binary_digest) is not None
    assert uldrenai_loom.doc_get_binary(path, "docs", "notes", "raw") == (
        b"\xff\x00",
        binary_digest,
    )
    assert len(uldrenai_loom.doc_list_binary(path, "docs", "notes")) > 0
    with pytest.raises(RuntimeError, match="DOCUMENT_NOT_TEXT"):
        uldrenai_loom.doc_get_text(path, "docs", "notes", "raw")


def test_cross_language_result_vector(tmp_path):
    """Reproduce the shared exec vector through the Python typed exec and assert byte-for-byte equality
    with the engine-pinned fixture. Identical canonical CBOR means identical typed values across all
    eight bindings, since they decode through the one shared Rust decoder."""
    vec = _FIXTURE["vectors"]["result_exec_select"]
    db = uldrenai_loom.LoomSql(str(tmp_path / "vec.loom"), "app", "main")
    db.exec(vec["sql"][0])  # CREATE TABLE t (id INTEGER PRIMARY KEY, n TEXT)
    db.exec(vec["sql"][1])  # INSERT INTO t VALUES (1, 'hi'), (2, NULL)
    # Raw canonical bytes must equal the fixture exactly.
    raw = db.exec_bytes(vec["exec_sql"])
    assert raw.hex() == vec["canonical_hex"]
    # And the typed decode must yield the same values (i64 1/2, text "hi", NULL -> None).
    payloads = db.exec(vec["exec_sql"])
    assert payloads[0]["kind"] == "select"
    assert payloads[0]["rows"] == [[1, "hi"], [2, None]]


def test_async_session_round_trip(tmp_path):
    """The asyncio wrapper runs engine work off the event loop (the native calls release the GIL)."""

    async def scenario() -> list:
        db = uldrenai_loom.AsyncLoomSql(str(tmp_path / "async.loom"), "app", "main")
        await db.exec("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)")
        await db.exec("INSERT INTO t VALUES (1, 'hi')")
        commit = await db.commit("seed", "tester")
        assert re.fullmatch(r"blake3:[0-9a-f]{64}", commit) is not None
        assert isinstance(await db.exec_bytes("SELECT v FROM t"), bytes)
        return (await db.exec("SELECT v FROM t"))[0]["rows"]

    assert asyncio.run(scenario()) == [["hi"]]


def test_query_row_iterator(tmp_path):
    """`query` returns a lazy row iterator (the Python iterator protocol): one typed row per step."""
    db = uldrenai_loom.LoomSql(str(tmp_path / "iter.loom"), "app", "main")
    db.exec("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)")
    db.exec("INSERT INTO t VALUES (1, 'a'), (2, 'b'), (3, 'c')")
    rows = db.query("SELECT id, v FROM t ORDER BY id")
    assert iter(rows) is rows  # __iter__ returns self
    assert [row for row in rows] == [[1, "a"], [2, "b"], [3, "c"]]
    with pytest.raises(RuntimeError, match="PERMISSION_DENIED: sql query is read-only"):
        list(db.query("CREATE TABLE u (id INTEGER PRIMARY KEY)"))


def test_multi_wrap_add_remove(tmp_path):
    """Add a second passphrase wrap and a raw-KEK wrap, then remove the original recovery wrap."""
    import pytest

    path = str(tmp_path / "keys.loom")
    uldrenai_loom.create_loom(path, "default", None, "first-pass")
    # A second passphrase wrap opens the same store.
    uldrenai_loom.key_add_wrap_keyed(path, "first-pass", "second-pass", False)
    assert uldrenai_loom.workspace_list_json(path, "second-pass") == "[]"
    # Re-adding an existing credential is rejected, not a silent no-op.
    with pytest.raises(Exception):
        uldrenai_loom.key_add_wrap_keyed(path, "first-pass", "second-pass", False)
    # A raw 256-bit KEK wrap; a wrong length is rejected.
    uldrenai_loom.key_add_wrap_with_kek(path, "first-pass", b"\x5a" * 32, False)
    with pytest.raises(Exception):
        uldrenai_loom.key_add_wrap_with_kek(path, "first-pass", b"\x00" * 16, False)
    # Removing one passphrase wrap is allowed while another passphrase recovery wrap remains.
    uldrenai_loom.key_remove_wrap(path, "first-pass", 0, False)
    with pytest.raises(Exception):
        uldrenai_loom.workspace_list_json(path, "first-pass")
    assert uldrenai_loom.workspace_list_json(path, "second-pass") == "[]"

    # Removing the last passphrase recovery wrap while an external KEK remains needs the override.
    recovery_path = str(tmp_path / "keys-recovery.loom")
    uldrenai_loom.create_loom(recovery_path, "default", None, "recovery-pass")
    uldrenai_loom.key_add_wrap_with_kek(recovery_path, "recovery-pass", b"\x33" * 32, False)
    with pytest.raises(Exception):
        uldrenai_loom.key_remove_wrap(recovery_path, "recovery-pass", 0, False)


def test_identity_and_acl_management(tmp_path):
    """Identity and ACL management: bootstrap root, authenticate per call, grant and revoke."""
    path = str(tmp_path / "auth.loom")
    uldrenai_loom.create_loom(path, "default", None, None)
    bootstrap = json.loads(uldrenai_loom.identity_list_json(path))
    assert bootstrap["authenticated_mode"] is False
    root_id = bootstrap["root"]
    admin_role_id = next(r["id"] for r in bootstrap["roles"] if r["name"] == "admin")
    assert any(
        p["id"] == root_id and admin_role_id in p["roles"] for p in bootstrap["principals"]
    )
    uldrenai_loom.workspace_create(path, "policy", "vcs")

    uldrenai_loom.identity_set_passphrase(path, root_id, "root-pass")
    with pytest.raises(Exception):
        uldrenai_loom.identity_list_json(path)
    uldrenai_loom.authenticate_passphrase(path, root_id, "root-pass")

    alice_id = uldrenai_loom.identity_add_principal(
        path,
        "alice",
        "Alice",
        "user",
        auth_principal=root_id,
        auth_passphrase="root-pass",
    )
    uldrenai_loom.identity_set_passphrase(
        path,
        alice_id,
        "alice-pass",
        auth_principal=root_id,
        auth_passphrase="root-pass",
    )
    uldrenai_loom.identity_assign_role(
        path,
        alice_id,
        admin_role_id,
        auth_principal=root_id,
        auth_passphrase="root-pass",
    )
    auth_identity = json.loads(
        uldrenai_loom.identity_list_json(path, auth_principal=root_id, auth_passphrase="root-pass")
    )
    assert auth_identity["authenticated_mode"] is True
    assert any(
        p["id"] == alice_id and p["has_passphrase"] and admin_role_id in p["roles"]
        for p in auth_identity["principals"]
    )
    assert auth_identity["app_credentials"] == []
    assert auth_identity["external_credentials"] == []
    assert auth_identity["public_keys"] == []
    external_id = uldrenai_loom.identity_create_external_credential(
        path,
        alice_id,
        "oidc-subject",
        "okta-prod",
        "https://issuer.example",
        "00u123",
        "sha256:metadata",
        auth_principal=root_id,
        auth_passphrase="root-pass",
    )
    external_identity = json.loads(
        uldrenai_loom.identity_list_json(path, auth_principal=root_id, auth_passphrase="root-pass")
    )
    assert any(
        credential["id"] == external_id
        and credential["principal"] == alice_id
        and credential["kind"] == "oidc_subject"
        and credential["issuer"] == "https://issuer.example"
        and credential["subject"] == "00u123"
        and credential["material_digest"] == "sha256:metadata"
        for credential in external_identity["external_credentials"]
    )
    uldrenai_loom.identity_revoke_external_credential(
        path,
        external_id,
        auth_principal=root_id,
        auth_passphrase="root-pass",
    )
    revoked_external_identity = json.loads(
        uldrenai_loom.identity_list_json(path, auth_principal=root_id, auth_passphrase="root-pass")
    )
    assert not any(
        credential["id"] == external_id
        for credential in revoked_external_identity["external_credentials"]
    )
    public_key_hex = (
        "046b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c296"
        "4fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5"
    )
    public_key_id = uldrenai_loom.identity_add_public_key(
        path,
        alice_id,
        "authority-laptop",
        "ES256",
        public_key_hex,
        auth_principal=root_id,
        auth_passphrase="root-pass",
    )
    public_key_identity = json.loads(
        uldrenai_loom.identity_list_json(path, auth_principal=root_id, auth_passphrase="root-pass")
    )
    assert any(
        key["id"] == public_key_id
        and key["principal"] == alice_id
        and key["label"] == "authority-laptop"
        and key["algorithm"] == "ES256"
        and key["public_key_hex"] == public_key_hex
        for key in public_key_identity["public_keys"]
    )
    uldrenai_loom.identity_revoke_public_key(
        path,
        public_key_id,
        auth_principal=root_id,
        auth_passphrase="root-pass",
    )
    revoked_public_key_identity = json.loads(
        uldrenai_loom.identity_list_json(path, auth_principal=root_id, auth_passphrase="root-pass")
    )
    assert not any(key["id"] == public_key_id for key in revoked_public_key_identity["public_keys"])

    uldrenai_loom.acl_grant(
        path,
        0,
        alice_id,
        facet="files",
        rights_mask=1,
        auth_principal=root_id,
        auth_passphrase="root-pass",
    )
    uldrenai_loom.acl_grant(
        path,
        0,
        f"role:{admin_role_id}",
        facet="kv",
        rights_mask=1,
        auth_principal=root_id,
        auth_passphrase="root-pass",
    )
    uldrenai_loom.acl_grant_scoped(
        path,
        0,
        alice_id,
        facet="kv",
        rights_mask=3,
        ref_glob="branch/main",
        scopes=["key:tenant/a/", "key:tenant/b/"],
        auth_principal=root_id,
        auth_passphrase="root-pass",
    )
    uldrenai_loom.acl_grant_scoped(
        path,
        0,
        alice_id,
        facet="files",
        rights_mask=1,
        ref_glob="branch/main",
        scopes=["path:reports/"],
        auth_principal=root_id,
        auth_passphrase="root-pass",
        predicate_cel="principal == 'alice'",
    )
    grants = json.loads(
        uldrenai_loom.acl_list_json(path, auth_principal=root_id, auth_passphrase="root-pass")
    )
    assert any(
        g["subject"] == alice_id and g["domain"] == "files" and "read" in g["rights"]
        for g in grants
    )
    assert any(
        g["subject"] == f"role:{admin_role_id}" and g["subject_kind"] == "role" and g["domain"] == "kv"
        for g in grants
    )
    assert any(
        g["subject"] == alice_id
        and g["domain"] == "kv"
        and g["ref_glob"] == "branch/main"
        and len(g["scopes"]) == 2
        for g in grants
    )
    assert any(
        g["subject"] == alice_id
        and g["domain"] == "files"
        and g["ref_glob"] == "branch/main"
        and g["predicate"]["language"] == "cel"
        and g["predicate"]["expression"] == "principal == 'alice'"
        for g in grants
    )
    assert (
        uldrenai_loom.acl_revoke(
            path,
            0,
            alice_id,
            facet="files",
            rights_mask=1,
            auth_principal=root_id,
            auth_passphrase="root-pass",
        )
        is True
    )
    assert (
        uldrenai_loom.acl_revoke(
            path,
            0,
            alice_id,
            facet="files",
            rights_mask=1,
            auth_principal=root_id,
            auth_passphrase="root-pass",
        )
        is False
    )
    assert (
        uldrenai_loom.acl_revoke_scoped(
            path,
            0,
            alice_id,
            facet="files",
            rights_mask=1,
            ref_glob="branch/main",
            scopes=["path:reports/"],
            auth_principal=root_id,
            auth_passphrase="root-pass",
            predicate_cel="principal == 'alice'",
        )
        is True
    )
    assert (
        uldrenai_loom.acl_revoke_scoped(
            path,
            0,
            alice_id,
            facet="kv",
            rights_mask=3,
            ref_glob="branch/main",
            scopes=["key:tenant/a/", "key:tenant/b/"],
            auth_principal=root_id,
            auth_passphrase="root-pass",
        )
        is True
    )
    uldrenai_loom.protected_ref_set(
        path,
        "policy",
        "branch/main",
        True,
        False,
        False,
        0,
        True,
        False,
        auth_principal=root_id,
        auth_passphrase="root-pass",
    )
    protected_policy = json.loads(
        uldrenai_loom.protected_ref_get_json(
            path, "policy", "branch/main", auth_principal=root_id, auth_passphrase="root-pass"
        )
    )
    assert protected_policy["fast_forward_only"] is True
    assert protected_policy["retention_lock"] is True
    protected_policies = json.loads(
        uldrenai_loom.protected_ref_list_json(
            path, "policy", auth_principal=root_id, auth_passphrase="root-pass"
        )
    )
    assert any(policy["ref"] == "branch/main" for policy in protected_policies)
    assert (
        uldrenai_loom.protected_ref_remove(
            path, "policy", "branch/main", auth_principal=root_id, auth_passphrase="root-pass"
        )
        is True
    )
    assert (
        uldrenai_loom.protected_ref_get_json(
            path, "policy", "branch/main", auth_principal=root_id, auth_passphrase="root-pass"
        )
        == "null"
    )
    assert (
        uldrenai_loom.identity_revoke_role(
            path,
            alice_id,
            admin_role_id,
            auth_principal=root_id,
            auth_passphrase="root-pass",
        )
        is True
    )
    assert (
        uldrenai_loom.identity_revoke_role(
            path,
            alice_id,
            admin_role_id,
            auth_principal=root_id,
            auth_passphrase="root-pass",
        )
        is False
    )
    auth_sql = uldrenai_loom.LoomSql.authenticated(path, "authsql", "main", root_id, "root-pass")
    auth_sql.exec("CREATE TABLE secured (id INTEGER PRIMARY KEY, v TEXT)")
    auth_sql.exec("INSERT INTO secured VALUES (1, 'ok')")
    assert auth_sql.exec("SELECT v FROM secured WHERE id = 1")[0]["rows"] == [["ok"]]

    auth_batch = uldrenai_loom.LoomSqlBatch.authenticated(
        path, "authbatch", "main", root_id, "root-pass"
    )
    auth_batch.exec("CREATE TABLE secured (id INTEGER PRIMARY KEY, v TEXT)")
    auth_batch.exec("INSERT INTO secured VALUES (1, 'batch')")
    auth_batch.commit()
    assert auth_batch.exec("SELECT v FROM secured WHERE id = 1")[0]["rows"] == [["batch"]]
    auth_batch.close()


def test_cas_round_trip(tmp_path):
    """CAS: put returns a content address (the raw content hash, distinct from the Object::Blob digest),
    is idempotent, get/has round-trip, list enumerates, and a missing digest reads as None."""
    path = str(tmp_path / "cas.loom")
    uldrenai_loom.create_loom(path, "default", None, None)
    addr = uldrenai_loom.cas_put(path, "blobs", b"hello loom")
    assert re.fullmatch(r"blake3:[0-9a-f]{64}", addr) is not None
    # Idempotent: identical bytes yield the same address.
    assert uldrenai_loom.cas_put(path, "blobs", b"hello loom") == addr
    assert uldrenai_loom.cas_has(path, "blobs", addr) is True
    assert uldrenai_loom.cas_get(path, "blobs", addr) == b"hello loom"
    # A digest that was never stored is absent.
    missing = uldrenai_loom.blob_digest(b"never stored")
    assert uldrenai_loom.cas_has(path, "blobs", missing) is False
    assert uldrenai_loom.cas_get(path, "blobs", missing) is None
    assert json.loads(uldrenai_loom.cas_list_json(path, "blobs")) == [addr]


def test_file_ops_round_trip(tmp_path):
    """Files facade: write/read round-trip, append concatenation, remove."""
    import pytest

    path = str(tmp_path / "files.loom")
    uldrenai_loom.create_loom(path, "default", None, None)
    uldrenai_loom.workspace_create(path, "docs", "files")
    uldrenai_loom.write_file(path, "files", "docs", "a.txt", b"hello")
    assert uldrenai_loom.read_file(path, "files", "docs", "a.txt") == b"hello"
    uldrenai_loom.append_file(path, "files", "docs", "a.txt", b"!")
    assert uldrenai_loom.read_file(path, "files", "docs", "a.txt") == b"hello!"
    # Append into a nonexistent directory fails (Mac/Linux semantics).
    with pytest.raises(Exception):
        uldrenai_loom.append_file(path, "files", "docs", "missing/b.txt", b"x")
    uldrenai_loom.remove_file(path, "files", "docs", "a.txt")
    with pytest.raises(Exception):
        uldrenai_loom.read_file(path, "files", "docs", "a.txt")

    # Symlink: create + read (git-style, opaque).
    uldrenai_loom.symlink(path, "files", "docs", "some/target", "link")
    assert uldrenai_loom.read_link(path, "files", "docs", "link") == "some/target"
    with pytest.raises(Exception):
        uldrenai_loom.read_link(path, "files", "docs", "missing")

    # Restore: commit, edit, restore the path back from HEAD.
    uldrenai_loom.write_file(path, "files", "docs", "r.txt", b"v1")
    uldrenai_loom.stage_all(path, "files", "docs")
    uldrenai_loom.commit_staged(path, "files", "docs", "nas", "init")
    uldrenai_loom.write_file(path, "files", "docs", "r.txt", b"v2")
    uldrenai_loom.restore_file(path, "files", "docs", "HEAD", "r.txt")
    assert uldrenai_loom.read_file(path, "files", "docs", "r.txt") == b"v1"
    uldrenai_loom.restore_path(path, "files", "docs", "HEAD", "")

    # Replay: commit a change, revert it (replayed); empty cherry-pick and no-op rebase.
    import json

    uldrenai_loom.write_file(path, "files", "docs", "rep.txt", b"x")
    uldrenai_loom.stage_all(path, "files", "docs")
    rep = uldrenai_loom.commit_staged(path, "files", "docs", "nas", "add rep")
    rev = json.loads(uldrenai_loom.revert(path, "files", "docs", [rep], "nas"))
    assert rev["outcome"] == "replayed"
    with pytest.raises(Exception):
        uldrenai_loom.read_file(path, "files", "docs", "rep.txt")
    assert json.loads(uldrenai_loom.cherry_pick(path, "files", "docs", []))["outcome"] == "empty"
    assert json.loads(uldrenai_loom.rebase(path, "files", "docs", "HEAD"))["outcome"] == "empty"

    # Squash: two commits after a base collapse into one.
    sq_base = uldrenai_loom.commit_staged(path, "files", "docs", "nas", "sq base")
    uldrenai_loom.write_file(path, "files", "docs", "s1.txt", b"1")
    uldrenai_loom.stage_all(path, "files", "docs")
    uldrenai_loom.commit_staged(path, "files", "docs", "nas", "s1")
    uldrenai_loom.write_file(path, "files", "docs", "s2.txt", b"2")
    uldrenai_loom.stage_all(path, "files", "docs")
    uldrenai_loom.commit_staged(path, "files", "docs", "nas", "s2")
    squashed = uldrenai_loom.squash(path, "files", "docs", sq_base, "nas", "squashed")
    assert re.fullmatch(r"blake3:[0-9a-f]{64}", squashed)


def test_byte_range_and_file_handle(tmp_path):
    """Byte-range write_at/read_at/truncate and a file handle (open, positional write, stat, read)."""
    path = str(tmp_path / "handles.loom")
    uldrenai_loom.create_loom(path, "default", None, None)
    uldrenai_loom.workspace_create(path, "docs", "files")

    # write_at zero-fills the gap; read_at clamps past the end.
    uldrenai_loom.write_at(path, "files", "docs", "b.bin", 5, b"XY")
    assert uldrenai_loom.read_at(path, "files", "docs", "b.bin", 0, 100) == b"\x00\x00\x00\x00\x00XY"
    # truncate shrinks.
    uldrenai_loom.truncate_file(path, "files", "docs", "b.bin", 6)
    assert uldrenai_loom.read_at(path, "files", "docs", "b.bin", 0, 100) == b"\x00\x00\x00\x00\x00X"

    # File handle: open read-write, positional write, stat, positional read, close.
    fh = uldrenai_loom.file_open(path, "files", "docs", "b.bin", "read_write")
    assert uldrenai_loom.file_write_at(path, fh, 0, b"Z") == 1
    size, _mode = uldrenai_loom.file_stat(path, fh)
    assert size == 6
    assert uldrenai_loom.file_read_at(path, fh, 0, 100) == b"Z\x00\x00\x00\x00X"
    uldrenai_loom.file_close(path, fh)


def test_tags_round_trip(tmp_path):
    """Tags: commit, lightweight + annotated create, list/target, rename, delete."""
    import pytest

    path = str(tmp_path / "tags.loom")
    uldrenai_loom.create_loom(path, "default", None, None)
    uldrenai_loom.workspace_create(path, "docs", "files")
    uldrenai_loom.write_file(path, "files", "docs", "a.txt", b"hello")
    uldrenai_loom.stage_all(path, "files", "docs")
    commit = uldrenai_loom.commit_staged(path, "files", "docs", "nas", "init")
    assert re.fullmatch(r"blake3:[0-9a-f]{64}", commit)

    # Lightweight tag at HEAD returns the commit digest.
    assert uldrenai_loom.tag_create(path, "files", "docs", "v1", "HEAD") == commit
    # Annotated tag returns the tag object digest (not the commit).
    ann = uldrenai_loom.tag_create(path, "files", "docs", "v1-ann", "HEAD", "nas", "release 1")
    assert ann != commit
    assert uldrenai_loom.tag_list(path, "files", "docs") == ["v1", "v1-ann"]
    assert uldrenai_loom.tag_target(path, "files", "docs", "v1") == commit
    uldrenai_loom.tag_rename(path, "files", "docs", "v1", "v2")
    assert uldrenai_loom.tag_target(path, "files", "docs", "v2") == commit
    uldrenai_loom.tag_delete(path, "files", "docs", "v2")
    assert uldrenai_loom.tag_target(path, "files", "docs", "v2") is None
    with pytest.raises(Exception):
        uldrenai_loom.tag_delete(path, "files", "docs", "v2")


def test_staging_index_over_sql_facet(tmp_path):
    """Staging index: an uncommitted SQL change is unstaged; staging moves it to the shared index;
    commit_staged records only the index; status reports each transition."""
    path = str(tmp_path / "staging.loom")
    db = uldrenai_loom.LoomSql(path, "app", "main")
    db.exec("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)")
    db.exec("INSERT INTO t VALUES (1, 'a')")
    db.commit("c1", "seed")  # commit everything: clean
    tbl = ".loom/facets/sql/main/tables/t"
    db.exec("INSERT INTO t VALUES (2, 'b')")  # uncommitted working change
    st = json.loads(uldrenai_loom.status_json(path, "sql", "app"))
    assert any(c["path"] == tbl for c in st["unstaged"])
    uldrenai_loom.stage(path, "sql", "app", [tbl])
    st = json.loads(uldrenai_loom.status_json(path, "sql", "app"))
    assert any(c["path"] == tbl for c in st["staged"]) and st["unstaged"] == []
    commit = uldrenai_loom.commit_staged(path, "sql", "app", "seed", "staged insert")
    assert re.fullmatch(r"blake3:[0-9a-f]{64}", commit) is not None
    st = json.loads(uldrenai_loom.status_json(path, "sql", "app"))
    assert st["staged"] == [] and st["unstaged"] == []


def test_merge_in_progress_surface(tmp_path):
    """Merge in-progress surface: a fresh workspace has no merge in progress, no conflicts, and abort
    with no merge raises. The conflict happy-path is covered by the core and C ABI suites; the Python
    binding does not yet project merge/branch to create a conflict."""
    import pytest

    path = str(tmp_path / "merge.loom")
    uldrenai_loom.create_loom(path, "default", None, None)
    uldrenai_loom.workspace_create(path, "work", "files")
    assert uldrenai_loom.merge_in_progress(path, "files", "work") is False
    assert uldrenai_loom.merge_conflicts(path, "files", "work") == []
    with pytest.raises(Exception):
        uldrenai_loom.merge_abort(path, "files", "work")


def test_queue_round_trip(tmp_path):
    """Append-log queue: append assigns 0 then 1, len reflects appends, get/range round-trip."""
    import pytest

    path = str(tmp_path / "queue.loom")
    uldrenai_loom.create_loom(path, "default", None, None)
    assert uldrenai_loom.queue_append(path, "events", "orders", b"a") == 0
    assert uldrenai_loom.queue_append(path, "events", "orders", b"b") == 1
    assert uldrenai_loom.queue_append(path, "events", "orders", b"c") == 2
    assert uldrenai_loom.queue_len(path, "events", "orders") == 3
    assert uldrenai_loom.queue_get(path, "events", "orders", 1) == b"b"
    assert uldrenai_loom.queue_get(path, "events", "orders", 9) is None
    assert uldrenai_loom.queue_range(path, "events", "orders", 1, 3) == [b"b", b"c"]
    with pytest.raises(Exception):
        uldrenai_loom.queue_append(path, "events", "../escape", b"x")


def test_queue_consumer_offsets(tmp_path):
    """Consumer offsets: missing reads as 0, read does not advance, advance is monotonic and persists."""
    import pytest

    path = str(tmp_path / "queue.loom")
    uldrenai_loom.create_loom(path, "default", None, None)
    for payload in (b"a", b"b", b"c"):
        uldrenai_loom.queue_append(path, "events", "orders", payload)

    assert uldrenai_loom.queue_consumer_position(path, "events", "orders", "worker") == 0
    assert uldrenai_loom.queue_consumer_read(path, "events", "orders", "worker", 2) == [b"a", b"b"]
    # Read does not advance; rereads redeliver.
    assert uldrenai_loom.queue_consumer_position(path, "events", "orders", "worker") == 0
    assert uldrenai_loom.queue_consumer_read(path, "events", "orders", "worker", 2) == [b"a", b"b"]

    uldrenai_loom.queue_consumer_advance(path, "events", "orders", "worker", 2)
    # Advance persists across a fresh open.
    assert uldrenai_loom.queue_consumer_position(path, "events", "orders", "worker") == 2
    with pytest.raises(Exception):
        uldrenai_loom.queue_consumer_advance(path, "events", "orders", "worker", 1)
    uldrenai_loom.queue_consumer_reset(path, "events", "orders", "worker", 0)
    assert uldrenai_loom.queue_consumer_position(path, "events", "orders", "worker") == 0
    with pytest.raises(Exception):
        uldrenai_loom.queue_consumer_position(path, "events", "orders", "a/b")


def test_workspace_lifecycle(tmp_path):
    """Workspace lifecycle through the binding: create with name + facet, list fields, rename by name
    and UUID, delete by UUID and name. Each call reopens the path, so a later read sees an earlier
    write."""
    import pytest

    path = str(tmp_path / "ns.loom")
    uldrenai_loom.create_loom(path, "default", None, None)
    ns_id = uldrenai_loom.workspace_create(path, "work", "files")
    assert re.fullmatch(r"[0-9a-f-]{36}", ns_id) is not None
    records = json.loads(uldrenai_loom.workspace_list_json(path))
    assert len(records) == 1
    assert records[0]["id"] == ns_id
    assert records[0]["name"] == "work"
    assert records[0]["facets"] == ["files"]
    assert records[0]["head"] is None
    # Rename by name, then by UUID.
    uldrenai_loom.workspace_rename(path, "work", "client")
    assert json.loads(uldrenai_loom.workspace_list_json(path))[0]["name"] == "client"
    uldrenai_loom.workspace_rename(path, ns_id, "client2")
    assert json.loads(uldrenai_loom.workspace_list_json(path))[0]["name"] == "client2"
    with pytest.raises(Exception):
        uldrenai_loom.workspace_rename(path, "missing", "x")
    # Delete the first by UUID and a second by name; deleted workspaces no longer appear.
    uldrenai_loom.workspace_create(path, "second", "cas")
    uldrenai_loom.workspace_delete(path, ns_id)
    uldrenai_loom.workspace_delete(path, "second")
    assert uldrenai_loom.workspace_list_json(path) == "[]"
    with pytest.raises(Exception):
        uldrenai_loom.workspace_delete(path, ns_id)


def test_batch_transaction_round_trip(tmp_path):
    """The held-open batch runs a SQL transaction across exec calls; rollback discards, commit persists."""
    batch = uldrenai_loom.LoomSqlBatch(str(tmp_path / "batch.loom"), "app", "main")
    batch.exec("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)")
    batch.exec("INSERT INTO t VALUES (1, 'a')")
    batch.exec("BEGIN")
    batch.exec("INSERT INTO t VALUES (2, 'b')")
    batch.exec("ROLLBACK")
    batch.commit()
    assert batch.exec("SELECT v FROM t ORDER BY id")[0]["rows"] == [["a"]]
    batch.close()


def test_direct_table_readers(tmp_path):
    """Direct readers: sql_read_table, sql_index_scan (empty-array prefix matches all rows), sql_blame, and
    sql_diff. Seed a table + index + rows through a SQL session, mirroring the C ABI direct-ops test,
    then decode each canonical-CBOR payload through result_to_json / result_to_bridge_json and assert
    structure and values."""
    path = str(tmp_path / "readers.loom")
    db = uldrenai_loom.LoomSql(path, "app", "main")
    db.exec("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)")
    db.exec("CREATE INDEX idx_v ON t (v)")
    db.exec("INSERT INTO t VALUES (1, 'a'), (2, 'b')")
    c1 = db.commit("c1", "seed")
    tbl = ".loom/facets/sql/main/tables/t"

    # sql_read_table: a Rows envelope with the storage key plus user id/v columns.
    rt = json.loads(uldrenai_loom.result_to_json(uldrenai_loom.sql_read_table(path, "app", tbl)))
    assert rt["kind"] == "Rows"
    assert [c["name"] for c in rt["columns"]] == ["__key", "id", "v"]
    assert rt["rows"] == [
        [{"Int": 1}, {"Int": 1}, {"Text": "a"}],
        [{"Int": 2}, {"Int": 2}, {"Text": "b"}],
    ]
    # result_to_bridge_json: lossless RN projection - text is bare, i64 is a tagged object.
    rt_bridge = json.loads(
        uldrenai_loom.result_to_bridge_json(uldrenai_loom.sql_read_table(path, "app", tbl))
    )
    assert rt_bridge["kind"] == "rows"
    assert rt_bridge["rows"] == [
        [{"$i64": "1"}, {"$i64": "1"}, "a"],
        [{"$i64": "2"}, {"$i64": "2"}, "b"],
    ]

    # The canonical CBOR of an empty array (0x80) is the match-all lookup prefix.
    scan = json.loads(
        uldrenai_loom.result_to_json(uldrenai_loom.sql_index_scan(path, "app", tbl, "idx_v", b"\x80"))
    )
    assert scan["kind"] == "Rows"
    assert [r[2] for r in scan["rows"]] == [{"Text": "a"}, {"Text": "b"}]

    # sql_blame: each current row plus the commit that last set it (all set by c1 here).
    blame = json.loads(
        uldrenai_loom.result_to_json(uldrenai_loom.sql_blame(path, "app", "main", tbl))
    )
    assert blame["kind"] == "Blame"
    assert len(blame["rows"]) == 2
    assert all(r["commit"] == c1 for r in blame["rows"])
    assert [r["values"][2] for r in blame["rows"]] == [{"Text": "a"}, {"Text": "b"}]

    # sql_diff c1 -> c2: the third row is added.
    db.exec("INSERT INTO t VALUES (3, 'c')")
    c2 = db.commit("c2", "seed")
    diff = json.loads(
        uldrenai_loom.result_to_json(uldrenai_loom.sql_diff(path, "app", tbl, c1, c2))
    )
    assert diff["kind"] == "Diff"
    assert diff["diffs"] == [
        {"change": "added", "values": [{"Int": 3}, {"Int": 3}, {"Text": "c"}]}
    ]
    old_table = json.loads(
        uldrenai_loom.result_to_json(uldrenai_loom.sql_read_table_at(path, "app", tbl, c1))
    )
    assert [r[2] for r in old_table["rows"]] == [{"Text": "a"}, {"Text": "b"}]
    old_scan = json.loads(
        uldrenai_loom.result_to_json(
            uldrenai_loom.sql_index_scan_at(path, "app", tbl, "idx_v", b"\x80", c1)
        )
    )
    assert [r[2] for r in old_scan["rows"]] == [{"Text": "a"}, {"Text": "b"}]
    table_diff = json.loads(
        uldrenai_loom.result_to_json(uldrenai_loom.sql_table_diff(path, "app", tbl, c1, c2))
    )
    assert table_diff["kind"] == "TableDiff"
    assert table_diff["records"] == [
        {"change": "added", "values": [{"Int": 3}, {"Int": 3}, {"Text": "c"}]}
    ]
    db.exec("ALTER TABLE t ADD COLUMN n INTEGER DEFAULT 7")
    c3 = db.commit("c3", "seed")
    schema_diff = json.loads(
        uldrenai_loom.result_to_json(uldrenai_loom.sql_table_diff(path, "app", tbl, c2, c3))
    )
    assert schema_diff["records"][0]["change"] == "schema_changed"
    assert schema_diff["records"][0]["to"]["columns"][-1]["name"] == "n"


def test_calendar_round_trip(tmp_path):
    """Calendar facade: create a collection, write-in an ICS document, list and delete."""
    path = str(tmp_path / "cal.loom")
    uldrenai_loom.create_loom(path, "default", None, None)
    uldrenai_loom.cal_create_collection(path, "ns", "alice", "work", "Work", "event")
    ics = (
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\n"
        "BEGIN:VEVENT\r\nUID:e1\r\nDTSTART:20240101T090000Z\r\nSUMMARY:Hi\r\n"
        "END:VEVENT\r\nEND:VCALENDAR\r\n"
    )
    etag = uldrenai_loom.cal_put_ics(path, "ns", "alice", "work", ics)
    assert re.match(r"^blake3:[0-9a-f]{64}$", etag)
    cols = uldrenai_loom.cal_list_collections(path, "ns", "alice")
    assert isinstance(cols, bytes) and len(cols) > 0
    entries = uldrenai_loom.cal_list_entries(path, "ns", "alice", "work")
    assert isinstance(entries, bytes) and len(entries) > 0
    assert uldrenai_loom.cal_delete_collection(path, "ns", "alice", "work") is True


def test_contacts_round_trip(tmp_path):
    """Contacts facade: create a book, write-in a vCard, list and delete."""
    path = str(tmp_path / "card.loom")
    uldrenai_loom.create_loom(path, "default", None, None)
    uldrenai_loom.card_create_book(path, "ns", "alice", "main", "Main")
    vcf = "BEGIN:VCARD\r\nVERSION:4.0\r\nUID:c1\r\nFN:Jane\r\nEND:VCARD\r\n"
    etag = uldrenai_loom.card_put_vcard(path, "ns", "alice", "main", vcf)
    assert re.match(r"^blake3:[0-9a-f]{64}$", etag)
    books = uldrenai_loom.card_list_books(path, "ns", "alice")
    assert isinstance(books, bytes) and len(books) > 0
    entries = uldrenai_loom.card_list_entries(path, "ns", "alice", "main")
    assert isinstance(entries, bytes) and len(entries) > 0
    assert uldrenai_loom.card_delete_book(path, "ns", "alice", "main") is True


def test_mail_round_trip(tmp_path):
    """Mail facade: create a mailbox, ingest a raw message, read back body/index/flags, delete."""
    path = str(tmp_path / "mail.loom")
    uldrenai_loom.create_loom(path, "default", None, None)
    uldrenai_loom.mail_create_mailbox(path, "ns", "alice", "inbox", "Inbox")
    raw = b"From: a@b\r\nSubject: Hi\r\n\r\nbody\r\n"
    etag = uldrenai_loom.mail_ingest_message(path, "ns", "alice", "inbox", "m1", raw)
    assert re.match(r"^blake3:[0-9a-f]{64}$", etag)
    assert uldrenai_loom.mail_to_eml(path, "ns", "alice", "inbox", "m1") == raw
    msg = uldrenai_loom.mail_get_message(path, "ns", "alice", "inbox", "m1")
    assert msg is not None and isinstance(msg, bytes)
    msgs = uldrenai_loom.mail_list_messages(path, "ns", "alice", "inbox")
    assert isinstance(msgs, bytes) and len(msgs) > 0
    flags = uldrenai_loom.mail_get_flags(path, "ns", "alice", "inbox", "m1")
    assert isinstance(flags, bytes)
    assert uldrenai_loom.mail_delete_message(path, "ns", "alice", "inbox", "m1") is True


def test_graph_round_trip(tmp_path):
    """Graph facade: nodes and a directed edge, neighbour/reachable traversal, edge removal."""
    path = str(tmp_path / "graph.loom")
    uldrenai_loom.create_loom(path, "default", None, None)
    for node in ("a", "b"):
        uldrenai_loom.graph_upsert_node(path, "graph", "g", node, b"")
    uldrenai_loom.graph_upsert_edge(path, "graph", "g", "e1", "a", "b", "rel", b"")
    assert uldrenai_loom.graph_get_node(path, "graph", "g", "a") is not None
    assert uldrenai_loom.graph_get_node(path, "graph", "g", "zzz") is None
    # Canonical CBOR for the text array ["b"]: 0x81 (array 1), 0x61 'b' (text len 1), 0x62 ('b').
    assert uldrenai_loom.graph_neighbors(path, "graph", "g", "a") == bytes([0x81, 0x61, 0x62])
    assert uldrenai_loom.graph_reachable(path, "graph", "g", "a", -1) == bytes([0x81, 0x61, 0x62])
    assert uldrenai_loom.graph_get_edge(path, "graph", "g", "e1") is not None
    assert uldrenai_loom.graph_remove_edge(path, "graph", "g", "e1") is True
    assert uldrenai_loom.graph_remove_edge(path, "graph", "g", "e1") is False


def test_vector_round_trip(tmp_path):
    """Vector facade: create a cosine set, upsert embeddings, exact search, delete."""
    import struct

    path = str(tmp_path / "vector.loom")
    uldrenai_loom.create_loom(path, "default", None, None)
    uldrenai_loom.vector_create(path, "vec", "emb", 2, 1)
    uldrenai_loom.vector_upsert(path, "vec", "emb", "a", struct.pack("<2f", 1.0, 0.0), b"")
    uldrenai_loom.vector_upsert(path, "vec", "emb", "c", struct.pack("<2f", 0.9, 0.1), b"")
    assert uldrenai_loom.vector_get(path, "vec", "emb", "a") is not None
    assert uldrenai_loom.vector_get(path, "vec", "emb", "zzz") is None
    hits = uldrenai_loom.vector_search(path, "vec", "emb", struct.pack("<2f", 1.0, 0.0), 2, b"")
    # A CBOR array of two hits: leading byte 0x82 (array of 2).
    assert isinstance(hits, bytes) and hits[0] == 0x82
    assert uldrenai_loom.vector_delete(path, "vec", "emb", "a") is True
    assert uldrenai_loom.vector_delete(path, "vec", "emb", "a") is False


def test_columnar_round_trip(tmp_path):
    """Columnar facade: typed columns, row append, count, and a predicate select."""
    path = str(tmp_path / "columnar.loom")
    uldrenai_loom.create_loom(path, "default", None, None)
    # columns [["id", 1 (Int)], ["price", 3 (Text)]] as canonical CBOR.
    columns = bytes([0x82, 0x82, 0x62, 0x69, 0x64, 0x01, 0x82, 0x65, 0x70, 0x72, 0x69, 0x63, 0x65, 0x03])
    uldrenai_loom.columnar_create(path, "col", "t", columns, 0)
    # rows: cell arrays [Int(n), Text("n0")]; Int cell = [2, n], Text cell = [4, "x"].
    row1 = bytes([0x82, 0x82, 0x02, 0x01, 0x82, 0x04, 0x62, 0x31, 0x30])
    row2 = bytes([0x82, 0x82, 0x02, 0x02, 0x82, 0x04, 0x62, 0x32, 0x30])
    uldrenai_loom.columnar_append(path, "col", "t", row1)
    uldrenai_loom.columnar_append(path, "col", "t", row2)
    assert uldrenai_loom.columnar_rows(path, "col", "t") == 2
    scan = uldrenai_loom.columnar_scan(path, "col", "t")
    assert isinstance(scan, bytes) and scan[0] == 0x82  # two rows
    inspect = uldrenai_loom.columnar_inspect(path, "col", "t")
    assert isinstance(inspect, bytes) and inspect[0] == 0x85
    source_digest = uldrenai_loom.columnar_source_digest(path, "col", "t")
    assert isinstance(source_digest, bytes) and source_digest[0] & 0xE0 == 0x60
    # select ["price"] where id (op 5 = ge) >= Int(2): one matching row.
    select_cols = bytes([0x81, 0x65, 0x70, 0x72, 0x69, 0x63, 0x65])
    sel_filter = bytes([0x83, 0x62, 0x69, 0x64, 0x05, 0x82, 0x02, 0x02])
    selected = uldrenai_loom.columnar_select(path, "col", "t", select_cols, sel_filter)
    assert selected[0] == 0x81  # array of exactly one row
    aggregates = bytes([0x82, 0x82, 0x00, 0xF6, 0x82, 0x03, 0x62, 0x69, 0x64])
    aggregate = uldrenai_loom.columnar_aggregate(path, "col", "t", aggregates, b"")
    assert aggregate[0] == 0x82  # two aggregate values
    uldrenai_loom.columnar_compact(path, "col", "t")
    assert uldrenai_loom.columnar_rows(path, "col", "t") == 2


def test_search_round_trip(tmp_path):
    """Search facade: a mapped collection, index/get/delete a document, and the linear-scan query."""
    path = str(tmp_path / "search.loom")
    uldrenai_loom.create_loom(path, "default", None, None)
    title = b"title"
    # mapping {"title": [0 text, true stored, false faceted]}.
    mapping = bytes([0xA1, 0x65]) + title + bytes([0x83, 0x00, 0xF5, 0xF4])
    uldrenai_loom.search_create(path, "search", "docs", mapping)
    # document {"title": "hello world"} (0x6B = text len 11).
    doc = bytes([0xA1, 0x65]) + title + bytes([0x6B]) + b"hello world"
    uldrenai_loom.search_index(path, "search", "docs", b"d1", doc)
    assert uldrenai_loom.search_get(path, "search", "docs", b"d1") is not None
    assert uldrenai_loom.search_get(path, "search", "docs", b"zzz") is None
    # request [Match(title, "hello"), limit 10, offset 0].
    query = bytes([0x83, 0x00, 0x65]) + title + bytes([0x65]) + b"hello"
    request = bytes([0x83]) + query + bytes([0x0A, 0x00])
    response = uldrenai_loom.search_query(path, "search", "docs", request)
    # response = CBOR [reduced (true = 0xF5), [hit], facets, aggregations] -> 0x84 0xF5 ...
    assert response[0] == 0x84 and response[1] == 0xF5
    assert uldrenai_loom.search_delete(path, "search", "docs", b"d1") is True
    assert uldrenai_loom.search_delete(path, "search", "docs", b"d1") is False
