# uldrenai-loom (Python binding)

PyO3 binding over the Uldren Loom Rust core. The package is configured for `abi3` wheels, where one
wheel covers CPython 3.9+.

Licensed under **BUSL-1.1** - the binding embeds the engine (see the repo `LICENSE`).

## Build (Python >= 3.9)

Requires the Rust toolchain and [`maturin`](https://www.maturin.rs). Work inside a virtualenv so
`maturin develop` installs into it:

```bash
python3 -m venv .venv && source .venv/bin/activate   # Windows: .venv\Scripts\activate
pip install maturin pytest
maturin develop --release   # build the abi3 extension + install uldrenai_loom
python -m pytest            # version() + blob_digest(b"abc")
```

`pytest` asserts the same digest shape as `loom hash` (`blake3:314b0f56...4058`).

## API

- `version() -> str`
- `blob_digest(data: bytes) -> str`
- `class LoomSql` - a SQL session over a workspace SQL facet in a `.loom`, exposing the whole versioned
  tabular + SQL stack:
  - `LoomSql(loom_path, ns_name, db)` - open (the workspace is created on first use).
  - `.exec(sql) -> list[dict]` - run SQL; returns **typed** results: a list of statement dicts. A
    `select` carries `columns` and `rows` of idiomatic cells (`int` - arbitrary precision, so 64/128-bit
    values are exact - `float`, `bytes`, `str`, `bool`, `None`, and `decimal.Decimal`). Mutations are
    staged and persisted to the working tree.
  - `.exec_bytes(sql) -> bytes` - the canonical-CBOR result payload; the type-faithful wire
    form for your own decoding.
  - `.query(sql) -> LoomRows` - a lazy read-only row iterator (`for row in db.query(sql)`); each row is a
    list of idiomatic cells (the streaming form). Use `.exec(sql)` for statements that mutate state.
  - `.exec_json(sql) -> str` - a JSON array of result payloads (debug/admin form, not type-faithful).
  - `.commit(message, author) -> str` - commit the staged state; returns the commit's content address.
- `class LoomSqlBatch` - an explicit transaction/batch scope: holds the `.loom` open
  across statements so a SQL transaction (`BEGIN`/`COMMIT`/`ROLLBACK`) can span `exec` calls, made
  durable by one atomic save at `.commit()` (`.commit_vcs(message, author)` also records a history
  entry; `.abort()` discards; `.close()` releases the write lock).
- `class AsyncLoomSql` - the `asyncio` form: same methods as `LoomSql`, each `await`-able. The native
  calls release the GIL, so `asyncio.to_thread` runs them truly off the event loop - no third-party
  dependency, the idiomatic stdlib path.

```python
from uldrenai_loom import LoomSql

db = LoomSql("app.loom", "app", "main")
db.exec("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)")
db.exec("INSERT INTO t VALUES (1, 'hello')")
result = db.exec("SELECT id, v FROM t")
rows = result[0]["rows"]  # [[1, "hello"]]
db.commit("seed", "you@example.com")
```
