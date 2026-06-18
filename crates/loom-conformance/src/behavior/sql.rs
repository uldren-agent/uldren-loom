use loom_core::tabular::{TableDiffRecord, Value};
use loom_core::{Code, FacetKind, Loom, MemoryStore, Result, WorkspaceId};
use loom_sql::LoomSqlStore;

pub fn run_sql_error_behavior() -> Result<()> {
    let mut store = LoomSqlStore::default();
    assert_sql_code(&mut store, "SELEC 1", Code::SqlSyntax);
    assert_sql_code(&mut store, "SELECT * FROM missing", Code::SqlTableNotFound);

    store.exec_cbor("CREATE TABLE t (id INTEGER PRIMARY KEY, v INTEGER NOT NULL)")?;
    store.exec_cbor("INSERT INTO t VALUES (1, 7)")?;
    assert_sql_code(
        &mut store,
        "INSERT INTO t VALUES (1, 8)",
        Code::SqlConstraintViolation,
    );
    assert_sql_code(
        &mut store,
        "INSERT INTO t VALUES (2, 'not-an-int')",
        Code::SqlTypeMismatch,
    );
    assert_sql_code(&mut store, "SELECT 1 / 0", Code::SqlExecutionFailed);
    Ok(())
}

fn assert_sql_code(store: &mut LoomSqlStore, sql: &str, code: Code) {
    let err = store.exec_cbor(sql).expect_err("SQL statement should fail");
    assert_eq!(err.code, code, "{sql}: {err}");
}

pub fn run_sql_history_behavior() -> Result<()> {
    let mut store = LoomSqlStore::default();
    store.exec_cbor("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)")?;
    store.exec_cbor("CREATE INDEX idx_v ON t (v)")?;
    store.exec_cbor("INSERT INTO t VALUES (1,'a'),(2,'b')")?;
    store.exec_cbor("CREATE TABLE u (id INTEGER PRIMARY KEY, v TEXT)")?;
    store.exec_cbor("INSERT INTO u VALUES (1,'a')")?;

    let mut loom = Loom::new(MemoryStore::new());
    let ns = loom
        .registry_mut()
        .create(FacetKind::Sql, None, WorkspaceId::from_bytes([23; 16]))?;
    let indexed_table = ".loom/facets/sql/main/tables/t";
    let schema_table = ".loom/facets/sql/main/tables/u";
    store.persist(&mut loom, ns, "main")?;
    let c1 = loom.commit(ns, "conformance", "v1", 1)?;

    store.exec_cbor("INSERT INTO t VALUES (3,'c')")?;
    store.persist(&mut loom, ns, "main")?;
    let c2 = loom.commit(ns, "conformance", "v2", 2)?;

    let old = loom.read_table_at(ns, indexed_table, c1)?;
    let old_rows = old.scan(&loom_core::tabular::Predicate::All);
    assert_eq!(old_rows.len(), 2);
    assert_eq!(old_rows[1][2], Value::Text("b".into()));

    let old_scan =
        loom.index_scan_at(ns, indexed_table, "idx_v", &[Value::Text("b".into())], c1)?;
    assert_eq!(old_scan.len(), 1);
    assert_eq!(old_scan[0][1], Value::Int(2));

    let current = loom.read_table(ns, indexed_table)?;
    let current_rows = current.scan(&loom_core::tabular::Predicate::All);
    assert_eq!(current_rows.len(), 3);
    assert_eq!(current_rows[2][2], Value::Text("c".into()));

    store.exec_cbor("ALTER TABLE t ADD COLUMN n INTEGER DEFAULT 7")?;
    store.persist(&mut loom, ns, "main")?;
    let c3 = loom.commit(ns, "conformance", "v3", 3)?;

    let schema_diff = loom.diff_table_records(ns, indexed_table, c2, c3)?;
    assert_eq!(schema_diff.len(), 1);
    let TableDiffRecord::SchemaChanged { from, to } = &schema_diff[0] else {
        panic!("expected schema change record")
    };
    assert_eq!(from.as_ref().expect("old schema").columns.len(), 3);
    assert_eq!(to.as_ref().expect("new schema").columns.len(), 4);

    let schema_table_diff = loom.diff_table_records(ns, schema_table, c2, c3)?;
    assert!(schema_table_diff.is_empty());

    let json = loom_result::result_to_json(&loom_sql::result_cbor::table_diff_cbor(&schema_diff)?)?;
    assert!(json.contains("\"kind\":\"TableDiff\""), "{json}");
    assert!(json.contains("\"change\":\"schema_changed\""), "{json}");
    assert!(json.contains("\"name\":\"n\""), "{json}");
    Ok(())
}
