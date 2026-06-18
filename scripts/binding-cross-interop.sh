#!/usr/bin/env bash
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp_root="$(mktemp -d "${TMPDIR:-/tmp}/loom-binding-interop.XXXXXX")"
trap 'rm -rf "$tmp_root"' EXIT

store="$tmp_root/interop.loom"
python_bin="${PYTHON:-$repo/bindings/python/.venv/bin/python}"
if [ ! -x "$python_bin" ]; then
    python_bin=python3
fi

node - "$store" "$repo" <<'JS'
const assert = require("node:assert/strict");
const path = require("node:path");

const store = process.argv[2];
const repo = process.argv[3];
const loom = require(path.join(repo, "bindings", "node"));

loom.createLoom(store, "default", null, null);
const db = new loom.LoomSql(store, "interop", "main");
db.exec("CREATE TABLE t (origin TEXT PRIMARY KEY, value TEXT)");
db.exec("INSERT INTO t VALUES ('node', 'one')");
assert.match(db.commit("node seed", "node"), /^blake3:[0-9a-f]{64}$/);

const digest = loom.docPutText(store, "docs", "notes", "node", "from node", null, null);
assert.match(digest, /^blake3:[0-9a-f]{64}$/);
JS

"$python_bin" - "$store" <<'PY'
import re
import sys

import uldrenai_loom

store = sys.argv[1]
db = uldrenai_loom.LoomSql(store, "interop", "main")
rows = db.exec("SELECT origin, value FROM t ORDER BY origin")[0]["rows"]
assert rows == [["node", "one"]], rows

db.exec("INSERT INTO t VALUES ('python', 'two')")
assert re.fullmatch(r"blake3:[0-9a-f]{64}", db.commit("python add", "python"))

text, digest = uldrenai_loom.doc_get_text(store, "docs", "notes", "node")
assert text == "from node"
assert re.fullmatch(r"blake3:[0-9a-f]{64}", digest)
py_digest = uldrenai_loom.doc_put_text(store, "docs", "notes", "python", "from python")
assert re.fullmatch(r"blake3:[0-9a-f]{64}", py_digest)
PY

node - "$store" "$repo" <<'JS'
const assert = require("node:assert/strict");
const path = require("node:path");

const store = process.argv[2];
const repo = process.argv[3];
const loom = require(path.join(repo, "bindings", "node"));
const db = new loom.LoomSql(store, "interop", "main");

const rows = db.exec("SELECT origin, value FROM t ORDER BY origin")[0].rows;
assert.deepEqual(rows, [
  ["node", "one"],
  ["python", "two"],
]);

assert.deepEqual(loom.docGetText(store, "docs", "notes", "node", null).text, "from node");
assert.deepEqual(loom.docGetText(store, "docs", "notes", "python", null).text, "from python");
JS

echo "binding cross interop passed"
