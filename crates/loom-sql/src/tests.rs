use super::*;
use futures::executor::block_on;
use gluesql_core::prelude::{Glue, Payload, Value};
use loom_core::error::Code;
use loom_core::{FacetKind, Loom, MemoryStore, WorkspaceId};

#[test]
fn conformance_vector_is_stable() {
    // The fixed create/insert/commit script must always yield the pinned commit address. The wasm32
    // build recomputes this live in-browser and asserts equality with CONFORMANCE_COMMIT, so
    // any 32-bit-vs-64-bit canonical-encoding drift fails there.
    let got = conformance_commit_digest(MemoryStore::new()).unwrap();
    assert_eq!(got, CONFORMANCE_COMMIT, "conformance commit digest drifted");
}

#[test]
fn sql_open_read_and_write_use_separate_acl_rights() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = loom
        .registry_mut()
        .create(
            FacetKind::Sql,
            Some("app"),
            WorkspaceId::from_bytes([77; 16]),
        )
        .unwrap();
    let root = WorkspaceId::from_bytes([1; 16]);
    let mut identity = loom_core::IdentityStore::new(root);
    identity.set_passphrase(root, "root", b"12345678").unwrap();
    let session = identity
        .authenticate_passphrase(root, "root", "session")
        .unwrap();
    loom.set_identity_store(identity);
    loom.set_session(session.id);

    assert_eq!(
        LoomSqlStore::load_eager_read(&loom, ns, "main")
            .unwrap_err()
            .code,
        loom_core::error::Code::PermissionDenied
    );
    assert_eq!(
        LoomSqlStore::load_eager_write(&loom, ns, "main")
            .unwrap_err()
            .code,
        loom_core::error::Code::PermissionDenied
    );

    loom.acl_store_mut()
        .allow(
            loom_core::AclSubject::Principal(root),
            Some(ns),
            Some(FacetKind::Sql),
            [loom_core::AclRight::Read],
        )
        .unwrap();
    LoomSqlStore::load_eager_read(&loom, ns, "main").unwrap();
    assert_eq!(
        LoomSqlStore::load_eager_write(&loom, ns, "main")
            .unwrap_err()
            .code,
        loom_core::error::Code::PermissionDenied
    );

    loom.acl_store_mut()
        .allow(
            loom_core::AclSubject::Principal(root),
            Some(ns),
            Some(FacetKind::Sql),
            [loom_core::AclRight::Write],
        )
        .unwrap();
    LoomSqlStore::load_eager_write(&loom, ns, "main").unwrap();
}

#[test]
fn sql_parameter_inference_uses_insert_column_types() {
    let mut store = LoomSqlStore::default();
    store
        .exec_cbor("CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT, active BOOLEAN)")
        .unwrap();

    let inferred = store
        .infer_parameter_types("INSERT INTO t (name, active, id) VALUES ($1, $2, $3)")
        .unwrap();
    assert_eq!(
        inferred,
        vec![
            Some(DataType::Text),
            Some(DataType::Boolean),
            Some(DataType::Int),
        ]
    );
}

#[test]
fn sql_parameter_inference_uses_predicate_column_types() {
    let mut store = LoomSqlStore::default();
    store
        .exec_cbor("CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT, active BOOLEAN)")
        .unwrap();

    let inferred = store
        .infer_parameter_types("SELECT * FROM t WHERE id = $1 AND name <> $2 AND $3 = active")
        .unwrap();
    assert_eq!(
        inferred,
        vec![
            Some(DataType::Int),
            Some(DataType::Text),
            Some(DataType::Boolean),
        ]
    );
}

#[test]
fn sql_parameter_inference_uses_explicit_casts_and_leaves_unknowns() {
    let store = LoomSqlStore::default();

    let inferred = store.infer_parameter_types("SELECT $1::TEXT, $3").unwrap();
    assert_eq!(inferred, vec![Some(DataType::Text), None, None]);
}

#[test]
fn result_vector_bytes_are_stable_and_decode_faithfully() {
    // The hard-typed reader payload must hold its content address (so every backend/target and
    // every binding sees the same bytes), AND the one shared decoder must round-trip each tricky
    // cell exactly, AND the RN bridge projection must tag them losslessly. One vector, three proofs.
    let bytes = result_vector_payload();
    assert_eq!(
        loom_core::Digest::blake3(&bytes).to_string(),
        RESULT_VECTOR_DIGEST,
        "result-payload vector bytes drifted"
    );

    // (1) Shared typed decoder (the form node/python/wasm/cpp/ios/jvm/android consume).
    let loom_result::result_view::ResultPayload::Reader(loom_result::result_view::Reader::Rows {
        columns,
        rows,
    }) = loom_result::result_view::decode(&bytes).unwrap()
    else {
        panic!("expected a Rows reader");
    };
    let names: Vec<&str> = columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "id", "name", "flag", "big", "amount", "raw", "ratio", "missing"
        ]
    );
    let r = &rows[0];
    use loom_core::tabular::Value as TV;
    assert_eq!(r[0], TV::Int(1));
    assert_eq!(r[1], TV::Text("hi".into()));
    assert_eq!(r[2], TV::Bool(true));
    assert_eq!(r[3], TV::U128(u128::from(u64::MAX) + 1));
    assert_eq!(
        r[4],
        TV::Decimal {
            mantissa: 12_345,
            scale: 2
        }
    );
    assert_eq!(r[5], TV::Bytes(vec![0, 1, 2, 255]));
    assert_eq!(r[6], TV::F32(0.1f32));
    match r[7] {
        TV::Float(f) => assert!(f.is_nan(), "expected NaN, got {f}"),
        ref other => panic!("expected Float(NaN), got {other:?}"),
    }

    // (2) RN lossless bridge projection (the form react-native consumes).
    let bj = loom_result::to_bridge_json(&bytes).unwrap();
    let v: serde_json::Value = serde_json::from_str(&bj).unwrap();
    let jr = &v["rows"][0];
    assert_eq!(jr[0]["$i64"], serde_json::json!("1"));
    assert_eq!(jr[1], serde_json::json!("hi"));
    assert_eq!(jr[2], serde_json::json!(true));
    assert_eq!(
        jr[3]["$u128"],
        serde_json::json!((u128::from(u64::MAX) + 1).to_string())
    );
    assert_eq!(jr[4]["$decimal"]["mantissa"], serde_json::json!("12345"));
    assert_eq!(jr[4]["$decimal"]["scale"], serde_json::json!(2));
    assert_eq!(jr[5]["$bytes"], serde_json::json!("AAEC/w=="));
    assert_eq!(jr[6]["$f32"], serde_json::json!(0.1f32.to_bits()));
    assert_eq!(
        jr[7]["$f64"],
        serde_json::json!(f64::NAN.to_bits().to_string())
    );
}

#[test]
fn result_exec_vector_bytes_are_stable_and_decode() {
    // The portable exec payload (Int + Text + NULL) is the cross-language reproduction target: every
    // binding's own typed `exec` over the same SQL MUST yield these exact bytes and decode them to
    // the same typed statement.
    let bytes = result_exec_vector();
    assert_eq!(
        loom_core::Digest::blake3(&bytes).to_string(),
        RESULT_EXEC_VECTOR_DIGEST,
        "result-exec vector bytes drifted"
    );
    let loom_result::result_view::ResultPayload::Statements(stmts) =
        loom_result::result_view::decode(&bytes).unwrap()
    else {
        panic!("expected statements");
    };
    let loom_result::result_view::Statement::Select { labels, rows } = &stmts[0] else {
        panic!("expected a Select, got {:?}", stmts[0]);
    };
    use loom_core::tabular::Value as TV;
    assert_eq!(labels, &vec!["id".to_string(), "n".to_string()]);
    assert_eq!(rows[0], vec![TV::Int(1), TV::Text("hi".into())]);
    assert_eq!(rows[1], vec![TV::Int(2), TV::Null]);
}

#[test]
fn result_vectors_match_cross_language_fixture() {
    // The shared fixture (bindings/conformance/result-vectors.json) is what every language
    // binding's test suite asserts against. Bind it to the engine here so it can never silently
    // drift: the live canonical bytes and their digest MUST equal the fixture's, and the fixture's
    // digest MUST equal the pinned const. If any of these moves, this test fails before a binding
    // ever consults a stale vector.
    let fixture = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../bindings/conformance/result-vectors.json"
    ))
    .expect("read cross-language result-vector fixture");
    let f: serde_json::Value = serde_json::from_str(&fixture).expect("parse fixture json");
    let hex = |b: &[u8]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();

    let reader = &f["vectors"]["result_reader_hard_types"];
    let rv = result_vector_payload();
    assert_eq!(reader["canonical_hex"], serde_json::json!(hex(&rv)));
    assert_eq!(reader["digest"], serde_json::json!(RESULT_VECTOR_DIGEST));

    let exec = &f["vectors"]["result_exec_select"];
    let ev = result_exec_vector();
    assert_eq!(exec["canonical_hex"], serde_json::json!(hex(&ev)));
    assert_eq!(exec["digest"], serde_json::json!(RESULT_EXEC_VECTOR_DIGEST));
}

fn assert_sql_code(store: &mut LoomSqlStore, sql: &str, code: Code) {
    let err = store.exec_cbor(sql).unwrap_err();
    assert_eq!(err.code, code, "{sql}: {err}");
}

#[test]
fn sql_failures_use_stable_codes() {
    let mut s = LoomSqlStore::default();
    assert_sql_code(&mut s, "SELEC 1", Code::SqlSyntax);
    assert_sql_code(&mut s, "SELECT * FROM missing", Code::SqlTableNotFound);

    s.exec_cbor("CREATE TABLE t (id INTEGER PRIMARY KEY, v INTEGER NOT NULL)")
        .unwrap();
    s.exec_cbor("INSERT INTO t VALUES (1, 7)").unwrap();
    assert_sql_code(
        &mut s,
        "INSERT INTO t VALUES (1, 8)",
        Code::SqlConstraintViolation,
    );
    assert_sql_code(
        &mut s,
        "INSERT INTO t VALUES (2, 'not-an-int')",
        Code::SqlTypeMismatch,
    );
    assert_sql_code(&mut s, "SELECT 1 / 0", Code::SqlExecutionFailed);
}

/// Decode a SELECT's first-statement rows as `[[text]]` for transaction assertions.
fn select_text_rows(store: &mut LoomSqlStore, sql: &str) -> Vec<String> {
    let bytes = store.exec_cbor(sql).unwrap();
    let loom_result::result_view::ResultPayload::Statements(stmts) =
        loom_result::result_view::decode(&bytes).unwrap()
    else {
        panic!("expected statements");
    };
    let loom_result::result_view::Statement::Select { rows, .. } = &stmts[0] else {
        panic!("expected a select, got {:?}", stmts[0]);
    };
    rows.iter()
        .map(|r| match &r[0] {
            loom_core::tabular::Value::Text(s) => s.clone(),
            other => panic!("expected text, got {other:?}"),
        })
        .collect()
}

#[test]
fn transaction_commit_keeps_changes() {
    let mut s = LoomSqlStore::default();
    s.exec_cbor("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)")
        .unwrap();
    s.exec_cbor("INSERT INTO t VALUES (1, 'a')").unwrap();
    s.exec_cbor("BEGIN").unwrap();
    assert!(s.in_transaction());
    s.exec_cbor("INSERT INTO t VALUES (2, 'b')").unwrap();
    s.exec_cbor("COMMIT").unwrap();
    assert!(!s.in_transaction());
    assert_eq!(
        select_text_rows(&mut s, "SELECT v FROM t ORDER BY id"),
        vec!["a".to_string(), "b".to_string()]
    );
}

#[test]
fn transaction_rollback_discards_changes() {
    let mut s = LoomSqlStore::default();
    s.exec_cbor("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)")
        .unwrap();
    s.exec_cbor("INSERT INTO t VALUES (1, 'a')").unwrap();
    s.exec_cbor("BEGIN").unwrap();
    s.exec_cbor("INSERT INTO t VALUES (2, 'b')").unwrap();
    s.exec_cbor("DELETE FROM t WHERE id = 1").unwrap();
    s.exec_cbor("ROLLBACK").unwrap();
    assert!(!s.in_transaction());
    // The committed row before BEGIN survives; the in-transaction insert and delete are undone.
    assert_eq!(
        select_text_rows(&mut s, "SELECT v FROM t ORDER BY id"),
        vec!["a".to_string()]
    );
}

#[test]
fn nested_begin_is_rejected() {
    let mut s = LoomSqlStore::default();
    s.exec_cbor("BEGIN").unwrap();
    let err = s.exec_cbor("BEGIN").unwrap_err();
    assert!(
        err.to_string().contains("nested transactions"),
        "unexpected error: {err}"
    );
    // The original transaction is still open and resolvable.
    assert!(s.in_transaction());
    s.exec_cbor("ROLLBACK").unwrap();
}

#[test]
fn bare_commit_and_rollback_are_rejected() {
    let mut s = LoomSqlStore::default();
    let c = s.exec_cbor("COMMIT").unwrap_err();
    assert!(
        c.to_string()
            .contains("COMMIT without an active transaction")
    );
    let r = s.exec_cbor("ROLLBACK").unwrap_err();
    assert!(
        r.to_string()
            .contains("ROLLBACK without an active transaction")
    );
    assert!(!s.in_transaction());
}

/// Decode a SELECT's first-statement rows as `[i64]` (first column) for index-path assertions.
fn select_i64_rows(store: &mut LoomSqlStore, sql: &str) -> Vec<i64> {
    let bytes = store.exec_cbor(sql).unwrap();
    let loom_result::result_view::ResultPayload::Statements(stmts) =
        loom_result::result_view::decode(&bytes).unwrap()
    else {
        panic!("expected statements");
    };
    let loom_result::result_view::Statement::Select { rows, .. } = &stmts[0] else {
        panic!("expected a select, got {:?}", stmts[0]);
    };
    rows.iter()
        .map(|r| match &r[0] {
            loom_core::tabular::Value::Int(i) => *i,
            other => panic!("expected int, got {other:?}"),
        })
        .collect()
}

#[test]
fn select_uses_durable_index_over_base_and_overlay() {
    // Build a table whose primary-key order (1,2,3) differs from its indexed-column order (c,a,b),
    // so the index path is observable: a `WHERE` predicate on the indexed column with no ORDER BY
    // returns rows in index order, whereas a full scan would return primary-key order.
    let mut loom = Loom::new(MemoryStore::new());
    let ns = loom
        .registry_mut()
        .ensure_for_write(
            &loom_core::WsSelector::Default(FacetKind::Sql),
            WorkspaceId::from_bytes([0x44; 16]),
        )
        .unwrap();
    let mut w = LoomSqlStore::default();
    w.exec_cbor("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)")
        .unwrap();
    w.exec_cbor("INSERT INTO t VALUES (1,'c'),(2,'a'),(3,'b')")
        .unwrap();
    w.exec_cbor("CREATE INDEX idx_v ON t (v)").unwrap();
    w.persist(&mut loom, ns, "db").unwrap();

    // Re-open over the lazy base (the durable `idx_v` tree is present).
    let mut s = LoomSqlStore::load(&loom, ns, "db").unwrap();
    // Equality is served by the durable index (prefix scan).
    assert_eq!(
        select_i64_rows(&mut s, "SELECT id FROM t WHERE v = 'b'"),
        vec![3]
    );
    // A range with no ORDER BY comes back in index (v) order [2,3,1] - proof the index path served
    // it; a full scan would return primary-key order [1,2,3].
    assert_eq!(
        select_i64_rows(&mut s, "SELECT id FROM t WHERE v >= 'a'"),
        vec![2, 3, 1]
    );

    // The overlay shadows the base *through* the index: update id=2's indexed value and insert id=4.
    s.exec_cbor("UPDATE t SET v='z' WHERE id=2").unwrap();
    s.exec_cbor("INSERT INTO t VALUES (4,'a')").unwrap();
    // index order over base+overlay: a(4), b(3), c(1), z(2).
    assert_eq!(
        select_i64_rows(&mut s, "SELECT id FROM t WHERE v >= 'a'"),
        vec![4, 3, 1, 2]
    );
    // Equality now matches the overlay insert, and the shadowed base row (id 2, formerly 'a') is gone.
    assert_eq!(
        select_i64_rows(&mut s, "SELECT id FROM t WHERE v = 'a'"),
        vec![4]
    );
}

#[test]
fn range_index_scan_uses_encoded_bounds() {
    // A signed-integer index exercises the order-preserving sign-flip encoding and the encoded
    // `[start, upper)` range bounds. Results come back in index (value) order; where that differs
    // from primary-key order (the negatives), it proves the range index path - not a full scan -
    // served the query.
    let mut loom = Loom::new(MemoryStore::new());
    let ns = loom
        .registry_mut()
        .ensure_for_write(
            &loom_core::WsSelector::Default(FacetKind::Sql),
            WorkspaceId::from_bytes([0x45; 16]),
        )
        .unwrap();
    let mut w = LoomSqlStore::default();
    w.exec_cbor("CREATE TABLE t (id INTEGER PRIMARY KEY, n INTEGER)")
        .unwrap();
    w.exec_cbor("INSERT INTO t VALUES (1,-5),(2,0),(3,5),(4,10),(5,-100)")
        .unwrap();
    w.exec_cbor("CREATE INDEX idx_n ON t (n)").unwrap();
    w.persist(&mut loom, ns, "db").unwrap();

    // Value order is -100(id5), -5(id1), 0(id2), 5(id3), 10(id4).
    let mut s = LoomSqlStore::load(&loom, ns, "db").unwrap();
    assert_eq!(
        select_i64_rows(&mut s, "SELECT id FROM t WHERE n > 0"),
        vec![3, 4]
    );
    assert_eq!(
        select_i64_rows(&mut s, "SELECT id FROM t WHERE n >= 0"),
        vec![2, 3, 4]
    );
    // `< 0` / `<= 0` return [5,1(,2)] in value order - primary-key order would be [1,5(,2)].
    assert_eq!(
        select_i64_rows(&mut s, "SELECT id FROM t WHERE n < 0"),
        vec![5, 1]
    );
    assert_eq!(
        select_i64_rows(&mut s, "SELECT id FROM t WHERE n <= 0"),
        vec![5, 1, 2]
    );
    // Boundary at the minimum value: strictly-greater excludes it; less-or-equal isolates it.
    assert_eq!(
        select_i64_rows(&mut s, "SELECT id FROM t WHERE n > -100"),
        vec![1, 2, 3, 4]
    );
    assert_eq!(
        select_i64_rows(&mut s, "SELECT id FROM t WHERE n <= -100"),
        vec![5]
    );

    // Range bounds also reflect the overlay: move id=3 below zero and re-query `< 0`.
    s.exec_cbor("UPDATE t SET n = -1 WHERE id = 3").unwrap();
    assert_eq!(
        select_i64_rows(&mut s, "SELECT id FROM t WHERE n < 0"),
        vec![5, 1, 3]
    );
}

#[test]
fn lazy_base_reads_without_preloading_then_overlay_shadows() {
    // Persist a 3-row table into a Loom.
    let mut loom = Loom::new(MemoryStore::new());
    let ns = loom
        .registry_mut()
        .ensure_for_write(
            &loom_core::WsSelector::Default(FacetKind::Sql),
            WorkspaceId::from_bytes([0x33; 16]),
        )
        .unwrap();
    let mut w = LoomSqlStore::default();
    w.exec_cbor("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)")
        .unwrap();
    w.exec_cbor("INSERT INTO t VALUES (1,'a'),(2,'b'),(3,'c')")
        .unwrap();
    w.persist(&mut loom, ns, "db").unwrap();

    // Re-open over the lazy base: nothing is preloaded - the overlay is empty, yet
    // SELECT and a point lookup still return the persisted rows, so they are streamed from the base.
    let mut s = LoomSqlStore::load(&loom, ns, "db").unwrap();
    assert!(
        s.overlay.is_empty(),
        "load must not preload rows into the overlay (lazy base)"
    );
    assert_eq!(
        select_text_rows(&mut s, "SELECT v FROM t ORDER BY id"),
        vec!["a".to_string(), "b".to_string(), "c".to_string()]
    );
    assert_eq!(
        select_text_rows(&mut s, "SELECT v FROM t WHERE id = 2"),
        vec!["b".to_string()]
    );

    // The overlay shadows the base: update id=2, delete id=3, insert id=4. Only the three changed
    // keys land in the overlay - the rest of the table is never materialized.
    s.exec_cbor("UPDATE t SET v='B' WHERE id=2").unwrap();
    s.exec_cbor("DELETE FROM t WHERE id=3").unwrap();
    s.exec_cbor("INSERT INTO t VALUES (4,'d')").unwrap();
    assert_eq!(
        select_text_rows(&mut s, "SELECT v FROM t ORDER BY id"),
        vec!["a".to_string(), "B".to_string(), "d".to_string()]
    );
    assert_eq!(
        s.overlay.get("t").map(|m| m.len()).unwrap_or(0),
        3,
        "overlay holds only the changed rows (update/delete/insert), not the whole table"
    );
}

// ALTER TABLE ADD/DROP/RENAME COLUMN works through GlueSQL's default `AlterTable` trait methods
// (they operate on `Store`/`StoreMut`, which `LoomSqlStore` fully implements); `insert_schema` marks
// the table `schema_dirty`, so `persist` fully re-projects the tabular table with the new column set.
// This regression test pins the in-memory behavior AND the persist+reload round-trip.
#[test]
fn alter_table_column_ops_persist_and_reload() {
    let mut s = LoomSqlStore::default();
    s.exec_cbor("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)")
        .unwrap();
    s.exec_cbor("INSERT INTO t VALUES (1, 'a')").unwrap();
    // ADD COLUMN with a default.
    s.exec_cbor("ALTER TABLE t ADD COLUMN n INTEGER DEFAULT 7")
        .unwrap();
    assert_eq!(
        select_text_rows(&mut s, "SELECT v FROM t"),
        vec!["a".to_string()]
    );
    // The new column carries the default.
    let bytes = s.exec_cbor("SELECT n FROM t").unwrap();
    let loom_result::result_view::ResultPayload::Statements(st) =
        loom_result::result_view::decode(&bytes).unwrap()
    else {
        panic!()
    };
    let loom_result::result_view::Statement::Select { rows, .. } = &st[0] else {
        panic!()
    };
    assert_eq!(rows[0][0], loom_core::tabular::Value::Int(7));
    // RENAME COLUMN.
    s.exec_cbor("ALTER TABLE t RENAME COLUMN v TO w").unwrap();
    assert_eq!(
        select_text_rows(&mut s, "SELECT w FROM t"),
        vec!["a".to_string()]
    );
    // DROP COLUMN.
    s.exec_cbor("ALTER TABLE t DROP COLUMN n").unwrap();
    assert!(
        s.exec_cbor("SELECT n FROM t").is_err(),
        "dropped column gone"
    );

    // Persist + reload round-trip: the altered schema/rows survive into a fresh store.
    let mut loom = Loom::new(MemoryStore::new());
    let ns = loom
        .registry_mut()
        .ensure_for_write(
            &loom_core::WsSelector::Default(FacetKind::Sql),
            WorkspaceId::from_bytes([0x22; 16]),
        )
        .unwrap();
    s.persist(&mut loom, ns, "db").unwrap();
    let mut reloaded = LoomSqlStore::load(&loom, ns, "db").unwrap();
    assert_eq!(
        select_text_rows(&mut reloaded, "SELECT w FROM t"),
        vec!["a".to_string()]
    );
    // The dropped column did not survive the re-projection either.
    assert!(
        reloaded.exec_cbor("SELECT n FROM t").is_err(),
        "dropped column must not reappear after reload"
    );
}

#[test]
fn select_rows_cbor_yields_one_row_per_item() {
    let mut s = LoomSqlStore::default();
    s.exec_cbor("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)")
        .unwrap();
    s.exec_cbor("INSERT INTO t VALUES (1, 'a'), (2, 'b')")
        .unwrap();
    let items = s
        .select_rows_cbor("SELECT id, v FROM t ORDER BY id")
        .unwrap();
    assert_eq!(items.len(), 2, "one canonical-CBOR item per row");
    // Each item is independently a canonical-CBOR cell array decoding to the row's typed cells.
    use loom_core::tabular::{Value, cell_from};
    for (i, item) in items.iter().enumerate() {
        let loom_codec::Value::Array(cells) = loom_codec::decode(item).unwrap() else {
            panic!("row item is not a cell array");
        };
        let row: Vec<Value> = cells.into_iter().map(|c| cell_from(c).unwrap()).collect();
        assert_eq!(row[0], Value::Int((i + 1) as i64));
        assert_eq!(
            row[1],
            Value::Text(if i == 0 { "a" } else { "b" }.to_string())
        );
    }
    // The low-level selector returns no rows and leaves dirty-state policy to its caller.
    assert!(
        s.select_rows_cbor("CREATE TABLE u (id INTEGER PRIMARY KEY)")
            .unwrap()
            .is_empty()
    );
}

#[test]
fn glue_runs_create_insert_select_over_loom_store() {
    block_on(async {
        let mut glue = Glue::new(LoomSqlStore::default());
        glue.execute("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)")
            .await
            .unwrap();
        glue.execute("INSERT INTO users VALUES (1, 'ann'), (2, 'bob')")
            .await
            .unwrap();
        let out = glue
            .execute("SELECT name FROM users WHERE id = 2")
            .await
            .unwrap();
        match &out[0] {
            Payload::Select { rows, .. } => {
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0][0], Value::Str("bob".to_owned()));
            }
            other => panic!("expected Select, got {other:?}"),
        }
    });
}

#[test]
fn sql_database_versions_through_loom() {
    block_on(async {
        let mut glue = Glue::new(LoomSqlStore::default());
        glue.execute("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)")
            .await
            .unwrap();
        glue.execute("INSERT INTO t VALUES (1,'a'),(2,'b')")
            .await
            .unwrap();

        // Persist v1 (two rows) into a workspace and commit.
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Sql, None, WorkspaceId::from_bytes([5; 16]))
            .unwrap();
        glue.storage.persist(&mut loom, ns, "main").unwrap();
        let c1 = loom.commit(ns, "nas", "v1: two rows", 1).unwrap();

        // Add a third row, persist v2, commit.
        glue.execute("INSERT INTO t VALUES (3,'c')").await.unwrap();
        glue.storage.persist(&mut loom, ns, "main").unwrap();
        loom.commit(ns, "nas", "v2: three rows", 2).unwrap();

        // Current state: three rows.
        let mut g_now = Glue::new(LoomSqlStore::load(&loom, ns, "main").unwrap());
        let out = g_now.execute("SELECT id FROM t").await.unwrap();
        match &out[0] {
            Payload::Select { rows, .. } => assert_eq!(rows.len(), 3),
            other => panic!("expected Select, got {other:?}"),
        }

        // Check out the first commit: the SQL database is back to two rows.
        loom.checkout_commit(ns, c1).unwrap();
        let mut g_old = Glue::new(LoomSqlStore::load(&loom, ns, "main").unwrap());
        let out = g_old.execute("SELECT id FROM t").await.unwrap();
        match &out[0] {
            Payload::Select { rows, .. } => assert_eq!(rows.len(), 2),
            other => panic!("expected Select, got {other:?}"),
        }
    });
}

#[test]
fn historical_table_readers_do_not_checkout_working_tree() {
    let mut store = LoomSqlStore::default();
    store
        .exec_cbor("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)")
        .unwrap();
    store.exec_cbor("CREATE INDEX idx_v ON t (v)").unwrap();
    store
        .exec_cbor("INSERT INTO t VALUES (1,'a'),(2,'b')")
        .unwrap();

    let mut loom = Loom::new(MemoryStore::new());
    let ns = loom
        .registry_mut()
        .create(FacetKind::Sql, None, WorkspaceId::from_bytes([15; 16]))
        .unwrap();
    let table = ".loom/facets/sql/main/tables/t";
    store.persist(&mut loom, ns, "main").unwrap();
    let c1 = loom.commit(ns, "nas", "v1", 1).unwrap();

    store.exec_cbor("UPDATE t SET v='B' WHERE id=2").unwrap();
    store.exec_cbor("INSERT INTO t VALUES (3,'c')").unwrap();
    store.persist(&mut loom, ns, "main").unwrap();
    let c2 = loom.commit(ns, "nas", "v2", 2).unwrap();

    let old = loom.read_table_at(ns, table, c1).unwrap();
    let old_rows = old.scan(&loom_core::tabular::Predicate::All);
    assert_eq!(old_rows.len(), 2);
    assert_eq!(old_rows[1][2], LValue::Text("b".into()));

    let old_scan = loom
        .index_scan_at(ns, table, "idx_v", &[LValue::Text("b".into())], c1)
        .unwrap();
    assert_eq!(old_scan.len(), 1);
    assert_eq!(old_scan[0][1], LValue::Int(2));

    let current = loom.read_table(ns, table).unwrap();
    let current_rows = current.scan(&loom_core::tabular::Predicate::All);
    assert_eq!(current_rows.len(), 3);
    assert_eq!(current_rows[1][2], LValue::Text("B".into()));
    assert_ne!(c1, c2);
}

#[test]
fn schema_aware_table_diff_reports_schema_changes() {
    let mut store = LoomSqlStore::default();
    store
        .exec_cbor("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)")
        .unwrap();
    store.exec_cbor("INSERT INTO t VALUES (1,'a')").unwrap();

    let mut loom = Loom::new(MemoryStore::new());
    let ns = loom
        .registry_mut()
        .create(FacetKind::Sql, None, WorkspaceId::from_bytes([16; 16]))
        .unwrap();
    let table = ".loom/facets/sql/main/tables/t";
    store.persist(&mut loom, ns, "main").unwrap();
    let c1 = loom.commit(ns, "nas", "v1", 1).unwrap();

    store
        .exec_cbor("ALTER TABLE t ADD COLUMN n INTEGER DEFAULT 7")
        .unwrap();
    store.persist(&mut loom, ns, "main").unwrap();
    let c2 = loom.commit(ns, "nas", "v2", 2).unwrap();

    let records = loom.diff_table_records(ns, table, c1, c2).unwrap();
    assert_eq!(records.len(), 1);
    let loom_core::tabular::TableDiffRecord::SchemaChanged { from, to } = &records[0] else {
        panic!("expected schema change record")
    };
    assert_eq!(from.as_ref().unwrap().columns.len(), 3);
    assert_eq!(to.as_ref().unwrap().columns.len(), 4);

    let json = loom_result::result_to_json(&crate::result_cbor::table_diff_cbor(&records).unwrap())
        .unwrap();
    assert!(json.contains("\"kind\":\"TableDiff\""), "{json}");
    assert!(json.contains("\"change\":\"schema_changed\""), "{json}");
    assert!(json.contains("\"name\":\"n\""), "{json}");
}

#[test]
fn row_level_per_table_granularity() {
    block_on(async {
        // Two tables; persist v1.
        let mut glue = Glue::new(LoomSqlStore::default());
        glue.execute("CREATE TABLE a (id INTEGER PRIMARY KEY, v TEXT)")
            .await
            .unwrap();
        glue.execute("CREATE TABLE b (id INTEGER PRIMARY KEY, v TEXT)")
            .await
            .unwrap();
        glue.execute("INSERT INTO a VALUES (1,'a1')").await.unwrap();
        glue.execute("INSERT INTO b VALUES (1,'b1')").await.unwrap();

        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Sql, None, WorkspaceId::from_bytes([7; 16]))
            .unwrap();
        glue.storage.persist(&mut loom, ns, "main").unwrap();
        let c1 = loom.commit(ns, "nas", "v1", 1).unwrap();
        let b_v1 = loom
            .staged_table_root(ns, ".loom/facets/sql/main/tables/b")
            .unwrap();
        let a_v1 = loom
            .staged_table_root(ns, ".loom/facets/sql/main/tables/a")
            .unwrap();

        // Change ONLY table a; persist v2.
        glue.execute("INSERT INTO a VALUES (2,'a2')").await.unwrap();
        glue.storage.persist(&mut loom, ns, "main").unwrap();
        loom.commit(ns, "nas", "v2", 2).unwrap();
        let b_v2 = loom
            .staged_table_root(ns, ".loom/facets/sql/main/tables/b")
            .unwrap();
        let a_v2 = loom
            .staged_table_root(ns, ".loom/facets/sql/main/tables/a")
            .unwrap();

        // The unchanged table b keeps the same TABLE-entry Tree digest across commits (content-
        // addressed dedup); the changed table a re-addresses. This is the per-table granularity.
        assert_eq!(
            b_v1, b_v2,
            "unchanged table must keep the same table-Tree digest"
        );
        assert_ne!(a_v1, a_v2, "changed table must re-address");

        // Data still round-trips: a has 2 rows now, b has 1.
        let mut g = Glue::new(LoomSqlStore::load(&loom, ns, "main").unwrap());
        match &g.execute("SELECT id FROM a").await.unwrap()[0] {
            Payload::Select { rows, .. } => assert_eq!(rows.len(), 2),
            other => panic!("expected Select, got {other:?}"),
        }
        match &g.execute("SELECT v FROM b WHERE id = 1").await.unwrap()[0] {
            Payload::Select { rows, .. } => {
                assert_eq!(rows[0][0], Value::Str("b1".to_owned()))
            }
            other => panic!("expected Select, got {other:?}"),
        }

        // Checkout v1 restores a to one row (per-table versioning through the engine).
        loom.checkout_commit(ns, c1).unwrap();
        let mut g1 = Glue::new(LoomSqlStore::load(&loom, ns, "main").unwrap());
        match &g1.execute("SELECT id FROM a").await.unwrap()[0] {
            Payload::Select { rows, .. } => assert_eq!(rows.len(), 1),
            other => panic!("expected Select, got {other:?}"),
        }
    });
}

#[test]
fn typed_columns_project_and_round_trip_through_loom() {
    block_on(async {
        let mut glue = Glue::new(LoomSqlStore::default());
        glue.execute("CREATE TABLE m (id INTEGER PRIMARY KEY, price DECIMAL, d DATE, ok BOOLEAN)")
            .await
            .unwrap();
        glue.execute("INSERT INTO m VALUES (1, 19.95, DATE '2026-06-20', TRUE)")
            .await
            .unwrap();

        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Sql, None, WorkspaceId::from_bytes([11; 16]))
            .unwrap();
        glue.storage.persist(&mut loom, ns, "main").unwrap();
        loom.commit(ns, "nas", "v1", 1).unwrap();

        // The persisted tabular table has real typed columns (__key, id, price, d, ok), not an
        // opaque row blob - proving the column projection.
        let t = loom
            .read_table(ns, ".loom/facets/sql/main/tables/m")
            .unwrap();
        assert_eq!(t.schema().arity(), 5, "key + 4 SQL columns");

        // Reload through GlueSQL: the decimal, date, and boolean values survive the round-trip.
        let mut g = Glue::new(LoomSqlStore::load(&loom, ns, "main").unwrap());
        match &g
            .execute("SELECT price, d, ok FROM m WHERE id = 1")
            .await
            .unwrap()[0]
        {
            Payload::Select { rows, .. } => {
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0][2], Value::Bool(true));
            }
            other => panic!("expected Select, got {other:?}"),
        }
    });
}

#[test]
fn create_index_builds_a_durable_secondary_index() {
    block_on(async {
        let mut glue = Glue::new(LoomSqlStore::default());
        glue.execute("CREATE TABLE u (id INTEGER PRIMARY KEY, email TEXT)")
            .await
            .unwrap();
        glue.execute("INSERT INTO u VALUES (1,'a@x'),(2,'b@x'),(3,'a@x')")
            .await
            .unwrap();
        glue.execute("CREATE INDEX by_email ON u (email)")
            .await
            .unwrap();

        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Sql, None, WorkspaceId::from_bytes([12; 16]))
            .unwrap();
        glue.storage.persist(&mut loom, ns, "main").unwrap();
        loom.commit(ns, "nas", "v1", 1).unwrap();

        // The durable prolly index is on the persisted table: an index_scan by email returns the
        // two matching rows (accelerated via the index, not a full scan).
        let rows = loom
            .index_scan(
                ns,
                ".loom/facets/sql/main/tables/u",
                "by_email",
                &[LValue::Text("a@x".into())],
            )
            .unwrap();
        assert_eq!(rows.len(), 2, "two rows have email a@x");

        // In-memory SQL queries still return correct results (full scan via the default planner).
        let mut g = Glue::new(LoomSqlStore::load(&loom, ns, "main").unwrap());
        match &g
            .execute("SELECT id FROM u WHERE email = 'a@x'")
            .await
            .unwrap()[0]
        {
            Payload::Select { rows, .. } => assert_eq!(rows.len(), 2),
            other => panic!("expected Select, got {other:?}"),
        }

        // DROP INDEX removes it from the durable form: a re-persist drops the prolly index.
        glue.storage.drop_index("u", "by_email").await.unwrap();
        glue.storage.persist(&mut loom, ns, "main").unwrap();
        assert!(
            loom.index_scan(
                ns,
                ".loom/facets/sql/main/tables/u",
                "by_email",
                &[LValue::Text("a@x".into())]
            )
            .is_err(),
            "dropped index is gone from the persisted table"
        );
    });
}

#[test]
fn delta_persist_matches_full_stage() {
    block_on(async {
        // Stage a table, then apply UPDATE / INSERT / DELETE and persist again via the delta path.
        let mut glue = Glue::new(LoomSqlStore::default());
        glue.execute("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)")
            .await
            .unwrap();
        glue.execute("INSERT INTO t VALUES (1,'a'),(2,'b'),(3,'c'),(4,'d')")
            .await
            .unwrap();

        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Sql, None, WorkspaceId::from_bytes([15; 16]))
            .unwrap();
        glue.storage.persist(&mut loom, ns, "main").unwrap(); // full stage

        glue.execute("UPDATE t SET v='B' WHERE id=2").await.unwrap();
        glue.execute("INSERT INTO t VALUES (5,'e')").await.unwrap();
        glue.execute("DELETE FROM t WHERE id=3").await.unwrap();
        glue.storage.persist(&mut loom, ns, "main").unwrap(); // delta: schema unchanged
        let delta_root = loom
            .staged_table_root(ns, ".loom/facets/sql/main/tables/t")
            .unwrap();

        // The same final row set {1:a, 2:B, 4:d, 5:e}, staged from scratch in a fresh database.
        let mut g2 = Glue::new(LoomSqlStore::default());
        g2.execute("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)")
            .await
            .unwrap();
        g2.execute("INSERT INTO t VALUES (1,'a'),(2,'B'),(4,'d'),(5,'e')")
            .await
            .unwrap();
        g2.storage.persist(&mut loom, ns, "ref").unwrap(); // full stage
        let full_root = loom
            .staged_table_root(ns, ".loom/facets/sql/ref/tables/t")
            .unwrap();

        assert_eq!(
            delta_root, full_root,
            "delta persist must produce the same durable table as a from-scratch full stage"
        );

        // And it reads back correctly through GlueSQL.
        let mut g = Glue::new(LoomSqlStore::load(&loom, ns, "main").unwrap());
        match &g.execute("SELECT id FROM t").await.unwrap()[0] {
            Payload::Select { rows, .. } => assert_eq!(rows.len(), 4),
            other => panic!("expected Select, got {other:?}"),
        }
    });
}

#[test]
fn exec_json_runs_sql_and_returns_results() {
    // exec_json is synchronous (it block_on's internally), so it is called outside an async block.
    let mut s = LoomSqlStore::default();
    s.exec_json("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)")
        .unwrap();
    s.exec_json("INSERT INTO t VALUES (1,'hi')").unwrap();
    let json = s.exec_json("SELECT id, v FROM t").unwrap();
    assert!(json.contains("Select"), "payload kind in json: {json}");
    assert!(json.contains("hi"), "row value in json: {json}");
    // A bad statement is a clean error, not a panic.
    assert!(s.exec_json("SELECT * FROM nope").is_err());
}
