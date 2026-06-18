use super::*;

fn users() -> Table {
    let schema = Schema::new(
        vec![
            ("id".into(), ColumnType::Int),
            ("name".into(), ColumnType::Text),
            ("active".into(), ColumnType::Bool),
        ],
        vec![0],
    )
    .unwrap();
    let mut t = Table::new(schema);
    t.insert(vec![
        Value::Int(2),
        Value::Text("bob".into()),
        Value::Bool(false),
    ])
    .unwrap();
    t.insert(vec![
        Value::Int(1),
        Value::Text("ann".into()),
        Value::Bool(true),
    ])
    .unwrap();
    t
}

#[test]
fn insert_get_delete_and_pk_upsert() {
    let mut t = users();
    assert_eq!(t.len(), 2);
    assert_eq!(
        t.get(&[Value::Int(1)]).unwrap()[1],
        Value::Text("ann".into())
    );
    // Upsert by primary key: same id replaces, not appends.
    t.insert(vec![
        Value::Int(1),
        Value::Text("annie".into()),
        Value::Bool(true),
    ])
    .unwrap();
    assert_eq!(t.len(), 2);
    assert_eq!(
        t.get(&[Value::Int(1)]).unwrap()[1],
        Value::Text("annie".into())
    );
    assert!(t.delete(&[Value::Int(2)]));
    assert!(!t.delete(&[Value::Int(2)]));
    assert_eq!(t.len(), 1);
}

#[test]
fn scan_pre_filter_in_key_order() {
    let mut t = users();
    t.insert(vec![
        Value::Int(3),
        Value::Text("cer".into()),
        Value::Bool(true),
    ])
    .unwrap();
    let active = Predicate::Compare {
        col: 2,
        op: CmpOp::Eq,
        value: Value::Bool(true),
    };
    let rows = t.scan(&active);
    // ann(1) and cer(3), returned in primary-key order.
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0][0], Value::Int(1));
    assert_eq!(rows[1][0], Value::Int(3));
    // A compound pre-filter: active AND id > 1.
    let p = Predicate::And(
        Box::new(active),
        Box::new(Predicate::Compare {
            col: 0,
            op: CmpOp::Gt,
            value: Value::Int(1),
        }),
    );
    let rows = t.scan(&p);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0][0], Value::Int(3));
}

#[test]
fn type_and_arity_validation() {
    let mut t = users();
    // Wrong arity.
    assert!(t.insert(vec![Value::Int(9)]).is_err());
    // Wrong type for the Text column.
    assert!(
        t.insert(vec![Value::Int(9), Value::Int(0), Value::Bool(true)])
            .is_err()
    );
    // Null is accepted in any column.
    assert!(
        t.insert(vec![Value::Int(9), Value::Null, Value::Bool(true)])
            .is_ok()
    );
}

#[test]
fn tables_version_with_commits() {
    use crate::provider::memory::MemoryStore;
    use crate::workspace::{FacetKind, WorkspaceId};

    let mut loom = Loom::new(MemoryStore::new());
    let ns = loom
        .registry_mut()
        .create(FacetKind::Sql, None, WorkspaceId::from_bytes([1; 16]))
        .unwrap();

    let mut t = users(); // 2 rows
    put_table(&mut loom, ns, "users", &t).unwrap();
    let c1 = loom.commit(ns, "nas", "two users", 1).unwrap();

    // A third row, committed again.
    t.insert(vec![
        Value::Int(3),
        Value::Text("cy".into()),
        Value::Bool(true),
    ])
    .unwrap();
    put_table(&mut loom, ns, "users", &t).unwrap();
    loom.commit(ns, "nas", "three users", 2).unwrap();
    assert_eq!(get_table(&loom, ns, "users").unwrap().len(), 3);
    assert_eq!(list_tables(&loom, ns), vec!["users".to_string()]);

    // Checking out the first commit restores the 2-row version of the table.
    loom.checkout_commit(ns, c1).unwrap();
    assert_eq!(get_table(&loom, ns, "users").unwrap().len(), 2);
}

#[test]
fn table_commit_shares_prolly_nodes_across_a_one_row_edit() {
    use crate::provider::memory::MemoryStore;
    use crate::workspace::{FacetKind, WorkspaceId};
    use std::collections::BTreeSet;

    let mut loom = Loom::new(MemoryStore::new());
    let ns = loom
        .registry_mut()
        .create(FacetKind::Sql, None, WorkspaceId::from_bytes([2; 16]))
        .unwrap();

    // A multi-leaf row map so structural sharing is observable.
    let mut t = big_users(300);
    put_table(&mut loom, ns, "users", &t).unwrap();
    let c1 = loom.commit(ns, "nas", "v1", 1).unwrap();

    // Change exactly one row and recommit.
    t.delete(&[Value::Int(100)]);
    t.insert(vec![Value::Int(100), Value::Text("renamed".into())])
        .unwrap();
    put_table(&mut loom, ns, "users", &t).unwrap();
    let c2 = loom.commit(ns, "nas", "v2", 2).unwrap();

    // The row map is a prolly tree, so a one-row edit rewrites only the touched leaf + spine; the
    // two commits share the bulk of their objects.
    let live1 = loom.reachable(&[c1], &BTreeSet::new()).unwrap();
    let live2 = loom.reachable(&[c2], &BTreeSet::new()).unwrap();
    let shared = live1.intersection(&live2).count();
    assert!(
        shared * 2 > live1.len(),
        "prolly row map should share most nodes across a one-row edit: shared={shared} of {}",
        live1.len()
    );

    // Each commit round-trips to its own version of the table.
    loom.checkout_commit(ns, c1).unwrap();
    assert_eq!(get_table(&loom, ns, "users").unwrap().len(), 300);
    loom.checkout_commit(ns, c2).unwrap();
    assert_eq!(get_table(&loom, ns, "users").unwrap().len(), 300);
}

#[test]
fn table_tree_canonical_digest_is_pinned() {
    // Conformance: a fixed schema + rows must always produce the same TABLE-entry Tree digest, so
    // peers share table objects byte-for-byte (identity profile). A change to the canonical table
    // form (schema codec, row encoding, prolly params, or the Tree entry layout) breaks this on
    // purpose - update the pin only with a deliberate format-version bump. The canonical schema Blob
    // ends with a secondary-index count (0 here), which contributes to the schema Blob and table
    // Tree addresses.
    use crate::provider::memory::MemoryStore;
    use crate::workspace::{FacetKind, WorkspaceId};

    let schema = Schema::new(
        vec![
            ("id".into(), ColumnType::Int),
            ("name".into(), ColumnType::Text),
        ],
        vec![0],
    )
    .unwrap();
    let mut t = Table::new(schema);
    t.insert(vec![Value::Int(1), Value::Text("alice".into())])
        .unwrap();
    t.insert(vec![Value::Int(2), Value::Text("bob".into())])
        .unwrap();

    let mut loom = Loom::new(MemoryStore::new());
    let ns = loom
        .registry_mut()
        .create(FacetKind::Sql, None, WorkspaceId::from_bytes([5; 16]))
        .unwrap();
    put_table(&mut loom, ns, "t", &t).unwrap();
    let root = loom.staged_table_root(ns, "t").unwrap();
    assert_eq!(
        root.to_hex(),
        "dda0b58f35b8f93b723465937707651d1a73a0fdaa73207e5882d63c343ca57a"
    );
}

#[test]
fn tables_and_files_coexist_in_one_commit() {
    use crate::provider::memory::MemoryStore;
    use crate::workspace::{FacetKind, WorkspaceId};

    let mut loom = Loom::new(MemoryStore::new());
    let ns = loom
        .registry_mut()
        .create(FacetKind::Sql, None, WorkspaceId::from_bytes([3; 16]))
        .unwrap();

    // A table and an ordinary file staged together in one workspace, one commit.
    put_table(&mut loom, ns, "users", &users()).unwrap();
    loom.write_file(ns, "notes.txt", b"hello", 0o100644)
        .unwrap();
    let c1 = loom.commit(ns, "nas", "table + file", 1).unwrap();

    // Clear the staging area, then restore from the mixed commit: both kinds come back.
    loom.remove_file(ns, "users").unwrap();
    loom.remove_file(ns, "notes.txt").unwrap();
    loom.checkout_commit(ns, c1).unwrap();
    assert_eq!(get_table(&loom, ns, "users").unwrap().len(), 2);
    assert_eq!(loom.read_file(ns, "notes.txt").unwrap(), b"hello");
    // A file path is not readable as a table, and vice versa.
    assert!(get_table(&loom, ns, "notes.txt").is_err());
    assert!(loom.read_file(ns, "users").is_err());
}

#[test]
fn staged_table_survives_export_import() {
    use crate::provider::memory::MemoryStore;
    use crate::workspace::{FacetKind, WorkspaceId};

    let mut loom = Loom::new(MemoryStore::new());
    let ns = loom
        .registry_mut()
        .create(FacetKind::Sql, None, WorkspaceId::from_bytes([4; 16]))
        .unwrap();
    put_table(&mut loom, ns, "users", &users()).unwrap();

    // The staging index (with the table slot) round-trips through export/import; the objects ride
    // along in the (cloned) store, so the table reads back without a commit.
    let state = loom.export_state();
    let mut reloaded = Loom::new(loom.store().clone());
    reloaded.import_state(&state).unwrap();
    assert_eq!(get_table(&reloaded, ns, "users").unwrap().len(), 2);
}

fn rows_schema() -> Schema {
    Schema::new(
        vec![
            ("id".into(), ColumnType::Int),
            ("v".into(), ColumnType::Text),
        ],
        vec![0],
    )
    .unwrap()
}

fn build(
    store: &mut crate::provider::memory::MemoryStore,
    rows: &[(i64, &str)],
) -> Option<crate::digest::Digest> {
    let mut t = Table::new(rows_schema());
    for (id, v) in rows {
        t.insert(vec![Value::Int(*id), Value::Text((*v).into())])
            .unwrap();
    }
    t.build_rows(store).unwrap()
}

#[test]
fn row_cursor_streams_rows_ascending_and_ranged() {
    use crate::provider::memory::MemoryStore;
    let mut store = MemoryStore::new();
    let root = build(&mut store, &[(1, "a"), (2, "b"), (3, "c"), (4, "d")]).unwrap();
    let schema = rows_schema();

    // Full streaming scan yields every row in ascending primary-key order.
    let mut cur = RowCursor::open(&store, &schema, &root).unwrap();
    let mut got = Vec::new();
    while let Some(r) = cur.next().unwrap() {
        got.push(r);
    }
    assert_eq!(got.len(), 4);
    assert_eq!(got[0][0], Value::Int(1));
    assert_eq!(got[3][1], Value::Text("d".into()));

    // Range [2, 4) over the row-map keys: rows id=2 and id=3 (order-preserving PK encoding).
    let start = row_map_key(&[Value::Int(2)]);
    let upper = row_map_key(&[Value::Int(4)]);
    let mut rc = RowCursor::open_range(&store, &schema, &root, Some(&start), Some(upper)).unwrap();
    let mut ids = Vec::new();
    while let Some(r) = rc.next().unwrap() {
        ids.push(r[0].clone());
    }
    assert_eq!(ids, vec![Value::Int(2), Value::Int(3)]);
}

#[test]
fn diff_rows_reports_added_updated_removed() {
    use crate::provider::memory::MemoryStore;
    let mut store = MemoryStore::new();
    let base = build(&mut store, &[(1, "a"), (2, "b"), (3, "c")]);
    // row 2 updated, row 3 removed, row 4 added; row 1 unchanged.
    let other = build(&mut store, &[(1, "a"), (2, "B!"), (4, "d")]);
    let diffs = diff_rows(&store, &rows_schema(), base.as_ref(), other.as_ref()).unwrap();
    assert_eq!(diffs.len(), 3);
    assert!(
        diffs
            .iter()
            .any(|d| matches!(d, RowDiff::Added(r) if r[0] == Value::Int(4)))
    );
    assert!(
        diffs
            .iter()
            .any(|d| matches!(d, RowDiff::Removed(r) if r[0] == Value::Int(3)))
    );
    assert!(diffs.iter().any(|d| matches!(
        d,
        RowDiff::Updated { to, .. } if to[0] == Value::Int(2) && to[1] == Value::Text("B!".into())
    )));
}

#[test]
fn merge_rows_is_row_level_when_sides_touch_different_rows() {
    use crate::provider::memory::MemoryStore;
    let mut store = MemoryStore::new();
    let base = build(&mut store, &[(1, "a"), (2, "b")]);
    let ours = build(&mut store, &[(1, "A"), (2, "b")]); // changed row 1
    let theirs = build(&mut store, &[(1, "a"), (2, "B")]); // changed row 2
    let out = merge_rows(
        &mut store,
        &rows_schema(),
        base.as_ref(),
        ours.as_ref(),
        theirs.as_ref(),
    )
    .unwrap();
    let TableMerge::Merged(root) = out else {
        panic!("expected a clean row-level merge");
    };
    // Both independent row changes survive (the whole-table merge would have had to conflict).
    let merged = Table::load_rows(&store, rows_schema(), root.as_ref().unwrap()).unwrap();
    let rows = merged.scan(&Predicate::All);
    assert_eq!(rows[0], &vec![Value::Int(1), Value::Text("A".into())]);
    assert_eq!(rows[1], &vec![Value::Int(2), Value::Text("B".into())]);
}

#[test]
fn merge_rows_conflicts_when_both_change_the_same_row() {
    use crate::provider::memory::MemoryStore;
    let mut store = MemoryStore::new();
    let base = build(&mut store, &[(1, "a")]);
    let ours = build(&mut store, &[(1, "X")]);
    let theirs = build(&mut store, &[(1, "Y")]);
    let out = merge_rows(
        &mut store,
        &rows_schema(),
        base.as_ref(),
        ours.as_ref(),
        theirs.as_ref(),
    )
    .unwrap();
    assert!(matches!(out, TableMerge::Conflicts(c) if c.len() == 1));
}

#[test]
fn vcs_merge_resolves_a_table_at_row_level() {
    use crate::provider::memory::MemoryStore;
    use crate::vcs::{Loom, MergeOutcome};
    use crate::workspace::{FacetKind, WorkspaceId};

    let table = |rows: &[(i64, &str)]| {
        let mut t = Table::new(rows_schema());
        for (id, v) in rows {
            t.insert(vec![Value::Int(*id), Value::Text((*v).into())])
                .unwrap();
        }
        t
    };

    let mut loom = Loom::new(MemoryStore::new());
    let ns = loom
        .registry_mut()
        .create(FacetKind::Sql, None, WorkspaceId::from_bytes([6; 16]))
        .unwrap();

    // base on main: rows 1,2.
    put_table(&mut loom, ns, "users", &table(&[(1, "a"), (2, "b")])).unwrap();
    loom.commit(ns, "nas", "base", 1).unwrap();
    loom.branch(ns, "feature").unwrap();

    // main edits row 1; feature edits row 2 - disjoint rows.
    put_table(&mut loom, ns, "users", &table(&[(1, "A"), (2, "b")])).unwrap();
    loom.commit(ns, "nas", "main edit", 2).unwrap();
    loom.checkout_branch(ns, "feature").unwrap();
    put_table(&mut loom, ns, "users", &table(&[(1, "a"), (2, "B")])).unwrap();
    loom.commit(ns, "nas", "feature edit", 3).unwrap();

    // Merge feature into main: row-level, so it resolves cleanly (whole-table would conflict).
    loom.checkout_branch(ns, "main").unwrap();
    let out = loom.merge(ns, "feature", "nas", 4).unwrap();
    let MergeOutcome::Merged(_) = out else {
        panic!("disjoint-row edits must merge cleanly at row level, got {out:?}");
    };
    let merged = get_table(&loom, ns, "users").unwrap();
    let rows = merged.scan(&Predicate::All);
    assert_eq!(rows[0], &vec![Value::Int(1), Value::Text("A".into())]);
    assert_eq!(rows[1], &vec![Value::Int(2), Value::Text("B".into())]);
}

#[test]
fn vcs_merge_conflicts_when_both_branches_change_the_same_row() {
    use crate::provider::memory::MemoryStore;
    use crate::vcs::{Loom, MergeOutcome};
    use crate::workspace::{FacetKind, WorkspaceId};

    let table = |rows: &[(i64, &str)]| {
        let mut t = Table::new(rows_schema());
        for (id, v) in rows {
            t.insert(vec![Value::Int(*id), Value::Text((*v).into())])
                .unwrap();
        }
        t
    };

    let mut loom = Loom::new(MemoryStore::new());
    let ns = loom
        .registry_mut()
        .create(FacetKind::Sql, None, WorkspaceId::from_bytes([8; 16]))
        .unwrap();
    put_table(&mut loom, ns, "users", &table(&[(1, "a")])).unwrap();
    loom.commit(ns, "nas", "base", 1).unwrap();
    loom.branch(ns, "feature").unwrap();

    put_table(&mut loom, ns, "users", &table(&[(1, "X")])).unwrap();
    loom.commit(ns, "nas", "main edit", 2).unwrap();
    loom.checkout_branch(ns, "feature").unwrap();
    put_table(&mut loom, ns, "users", &table(&[(1, "Y")])).unwrap();
    loom.commit(ns, "nas", "feature edit", 3).unwrap();

    loom.checkout_branch(ns, "main").unwrap();
    match loom.merge(ns, "feature", "nas", 4).unwrap() {
        MergeOutcome::Conflicts(c) => assert_eq!(c, vec!["users".to_string()]),
        other => panic!("same-row edits must conflict, got {other:?}"),
    }
}

#[test]
fn blame_attributes_each_row_to_its_last_change() {
    use crate::provider::memory::MemoryStore;
    use crate::workspace::{FacetKind, WorkspaceId};

    let table = |rows: &[(i64, &str)]| {
        let mut t = Table::new(rows_schema());
        for (id, v) in rows {
            t.insert(vec![Value::Int(*id), Value::Text((*v).into())])
                .unwrap();
        }
        t
    };

    let mut loom = Loom::new(MemoryStore::new());
    let ns = loom
        .registry_mut()
        .create(FacetKind::Sql, None, WorkspaceId::from_bytes([9; 16]))
        .unwrap();

    put_table(&mut loom, ns, "users", &table(&[(1, "a"), (2, "b")])).unwrap();
    let c1 = loom.commit(ns, "nas", "c1", 1).unwrap();
    put_table(&mut loom, ns, "users", &table(&[(1, "A"), (2, "b")])).unwrap(); // row 1 changed
    let c2 = loom.commit(ns, "nas", "c2", 2).unwrap();
    put_table(
        &mut loom,
        ns,
        "users",
        &table(&[(1, "A"), (2, "b"), (3, "c")]),
    )
    .unwrap(); // row 3 added
    let c3 = loom.commit(ns, "nas", "c3", 3).unwrap();

    let blame = loom.blame_table(ns, "main", "users").unwrap();
    assert_eq!(blame.len(), 3); // rows in primary-key order: 1, 2, 3
    assert_eq!(blame[0].0[0], Value::Int(1));
    assert_eq!(blame[0].1, c2, "row 1 last changed in c2");
    assert_eq!(blame[1].0[0], Value::Int(2));
    assert_eq!(blame[1].1, c1, "row 2 unchanged since c1");
    assert_eq!(blame[2].0[0], Value::Int(3));
    assert_eq!(blame[2].1, c3, "row 3 added in c3");
}

#[test]
fn diff_table_reports_row_changes_between_commits() {
    use crate::provider::memory::MemoryStore;
    use crate::workspace::{FacetKind, WorkspaceId};

    let table = |rows: &[(i64, &str)]| {
        let mut t = Table::new(rows_schema());
        for (id, v) in rows {
            t.insert(vec![Value::Int(*id), Value::Text((*v).into())])
                .unwrap();
        }
        t
    };

    let mut loom = Loom::new(MemoryStore::new());
    let ns = loom
        .registry_mut()
        .create(FacetKind::Sql, None, WorkspaceId::from_bytes([11; 16]))
        .unwrap();

    put_table(&mut loom, ns, "users", &table(&[(1, "a"), (2, "b")])).unwrap();
    let c1 = loom.commit(ns, "nas", "c1", 1).unwrap();
    // Update row 1, remove row 2, add row 3.
    put_table(&mut loom, ns, "users", &table(&[(1, "A"), (3, "c")])).unwrap();
    let c2 = loom.commit(ns, "nas", "c2", 2).unwrap();

    let diff = loom.diff_table(ns, "users", c1, c2).unwrap();
    assert_eq!(diff.len(), 3); // in primary-key order: 1 (updated), 2 (removed), 3 (added)
    assert!(
        matches!(&diff[0], RowDiff::Updated { from, to } if from[0] == Value::Int(1) && to[1] == Value::Text("A".into()))
    );
    assert!(matches!(&diff[1], RowDiff::Removed(r) if r[0] == Value::Int(2)));
    assert!(matches!(&diff[2], RowDiff::Added(r) if r[0] == Value::Int(3)));
}

#[test]
fn incremental_row_mutations_equal_full_restage() {
    use crate::provider::memory::MemoryStore;
    use crate::workspace::{FacetKind, WorkspaceId};

    // A table with a secondary index, so the assertion covers row map + index identity together.
    let schema = || {
        Schema::new(
            vec![
                ("id".into(), ColumnType::Int),
                ("city".into(), ColumnType::Text),
            ],
            vec![0],
        )
        .unwrap()
        .with_index("by_city", &["city"], false)
        .unwrap()
    };
    let table = |rows: &[(i64, &str)]| {
        let mut t = Table::new(schema());
        for (id, c) in rows {
            t.insert(vec![Value::Int(*id), Value::Text((*c).into())])
                .unwrap();
        }
        t
    };

    let mut loom = Loom::new(MemoryStore::new());
    let ns = loom
        .registry_mut()
        .create(FacetKind::Sql, None, WorkspaceId::from_bytes([13; 16]))
        .unwrap();

    // Start staged with two rows, then apply incremental insert / replace / delete.
    put_table(&mut loom, ns, "t", &table(&[(1, "paris"), (2, "berlin")])).unwrap();
    loom.insert_row(ns, "t", vec![Value::Int(3), Value::Text("paris".into())])
        .unwrap(); // insert new row
    loom.insert_row(ns, "t", vec![Value::Int(2), Value::Text("rome".into())])
        .unwrap(); // replace row 2
    loom.delete_row(ns, "t", &[Value::Int(1)]).unwrap(); // delete row 1
    let incremental = loom.staged_table_root(ns, "t").unwrap();

    // The equivalent final row set, staged fresh: rows {2->rome, 3->paris}.
    put_table(&mut loom, ns, "full", &table(&[(2, "rome"), (3, "paris")])).unwrap();
    let full = loom.staged_table_root(ns, "full").unwrap();

    assert_eq!(
        incremental, full,
        "incremental row mutations must yield the same table Tree as a full re-stage"
    );

    // The durable index still answers correctly after incremental maintenance.
    let paris = loom
        .index_scan(ns, "t", "by_city", &[Value::Text("paris".into())])
        .unwrap();
    assert_eq!(paris.len(), 1);
    assert_eq!(paris[0][0], Value::Int(3));
}

#[test]
fn unique_index_rejects_incremental_duplicate() {
    use crate::provider::memory::MemoryStore;
    use crate::workspace::{FacetKind, WorkspaceId};

    let schema = Schema::new(
        vec![
            ("id".into(), ColumnType::Int),
            ("email".into(), ColumnType::Text),
        ],
        vec![0],
    )
    .unwrap()
    .with_index("by_email", &["email"], true)
    .unwrap();
    let mut t = Table::new(schema);
    t.insert(vec![Value::Int(1), Value::Text("a@x".into())])
        .unwrap();

    let mut loom = Loom::new(MemoryStore::new());
    let ns = loom
        .registry_mut()
        .create(FacetKind::Sql, None, WorkspaceId::from_bytes([14; 16]))
        .unwrap();
    put_table(&mut loom, ns, "u", &t).unwrap();

    // A different pk with the same unique email is rejected; replacing the same pk is fine.
    assert!(
        loom.insert_row(ns, "u", vec![Value::Int(2), Value::Text("a@x".into())])
            .is_err()
    );
    loom.insert_row(ns, "u", vec![Value::Int(1), Value::Text("a@x".into())])
        .unwrap();
}

fn people_schema() -> Schema {
    Schema::new(
        vec![
            ("id".into(), ColumnType::Int),
            ("name".into(), ColumnType::Text),
            ("city".into(), ColumnType::Text),
        ],
        vec![0],
    )
    .unwrap()
    .with_index("by_city", &["city"], false)
    .unwrap()
    .with_index("by_name", &["name"], true)
    .unwrap()
}

fn people(schema: Schema, rows: &[(i64, &str, &str)]) -> Table {
    let mut t = Table::new(schema);
    for (id, name, city) in rows {
        t.insert(vec![
            Value::Int(*id),
            Value::Text((*name).into()),
            Value::Text((*city).into()),
        ])
        .unwrap();
    }
    t
}

#[test]
fn secondary_index_is_maintained_and_accelerates_lookup() {
    use crate::provider::memory::MemoryStore;
    use crate::workspace::{FacetKind, WorkspaceId};

    let t = people(
        people_schema(),
        &[
            (1, "ann", "paris"),
            (2, "bob", "paris"),
            (3, "cat", "berlin"),
            (4, "dan", "paris"),
        ],
    );

    let mut loom = Loom::new(MemoryStore::new());
    let ns = loom
        .registry_mut()
        .create(FacetKind::Sql, None, WorkspaceId::from_bytes([7; 16]))
        .unwrap();
    put_table(&mut loom, ns, "people", &t).unwrap();

    // Non-unique index: the three paris rows come back in (city, primary-key) order.
    let paris = loom
        .index_scan(ns, "people", "by_city", &[Value::Text("paris".into())])
        .unwrap();
    assert_eq!(
        paris.iter().map(|r| r[0].clone()).collect::<Vec<_>>(),
        vec![Value::Int(1), Value::Int(2), Value::Int(4)]
    );
    // The index result equals a full scan + filter on the same column (correctness vs substrate).
    let scanned: Vec<Row> = t
        .scan(&Predicate::Compare {
            col: 2,
            op: CmpOp::Eq,
            value: Value::Text("paris".into()),
        })
        .into_iter()
        .cloned()
        .collect();
    assert_eq!(paris, scanned);

    // Unique index: a point lookup returns the single matching row.
    let bob = loom
        .index_scan(ns, "people", "by_name", &[Value::Text("bob".into())])
        .unwrap();
    assert_eq!(bob.len(), 1);
    assert_eq!(bob[0][0], Value::Int(2));

    // A value present in no row yields nothing.
    assert!(
        loom.index_scan(ns, "people", "by_city", &[Value::Text("rome".into())])
            .unwrap()
            .is_empty()
    );

    // The index lives in the table Tree, so it survives commit + checkout.
    let c1 = loom.commit(ns, "nas", "people", 1).unwrap();
    loom.remove_file(ns, "people").unwrap();
    loom.checkout_commit(ns, c1).unwrap();
    let paris2 = loom
        .index_scan(ns, "people", "by_city", &[Value::Text("paris".into())])
        .unwrap();
    assert_eq!(paris2.len(), 3);
}

#[test]
fn unique_secondary_index_rejects_duplicates() {
    use crate::provider::memory::MemoryStore;
    use crate::workspace::{FacetKind, WorkspaceId};

    // Two rows with distinct primary keys but the same `name`: staging must fail the unique index.
    let t = people(
        people_schema(),
        &[(1, "ann", "paris"), (2, "ann", "berlin")],
    );
    let mut loom = Loom::new(MemoryStore::new());
    let ns = loom
        .registry_mut()
        .create(FacetKind::Sql, None, WorkspaceId::from_bytes([9; 16]))
        .unwrap();
    assert!(put_table(&mut loom, ns, "people", &t).is_err());
}

#[test]
fn secondary_index_rebuilds_after_row_merge() {
    use crate::provider::memory::MemoryStore;
    use crate::vcs::{Loom, MergeOutcome};
    use crate::workspace::{FacetKind, WorkspaceId};

    let schema = || {
        Schema::new(
            vec![
                ("id".into(), ColumnType::Int),
                ("city".into(), ColumnType::Text),
            ],
            vec![0],
        )
        .unwrap()
        .with_index("by_city", &["city"], false)
        .unwrap()
    };
    let table = |rows: &[(i64, &str)]| {
        let mut t = Table::new(schema());
        for (id, city) in rows {
            t.insert(vec![Value::Int(*id), Value::Text((*city).into())])
                .unwrap();
        }
        t
    };

    let mut loom = Loom::new(MemoryStore::new());
    let ns = loom
        .registry_mut()
        .create(FacetKind::Sql, None, WorkspaceId::from_bytes([10; 16]))
        .unwrap();

    // base: row 1 in paris. main adds row 2 (paris); feature adds row 3 (berlin) - disjoint rows.
    put_table(&mut loom, ns, "t", &table(&[(1, "paris")])).unwrap();
    loom.commit(ns, "nas", "base", 1).unwrap();
    loom.branch(ns, "feature").unwrap();
    put_table(&mut loom, ns, "t", &table(&[(1, "paris"), (2, "paris")])).unwrap();
    loom.commit(ns, "nas", "main", 2).unwrap();
    loom.checkout_branch(ns, "feature").unwrap();
    put_table(&mut loom, ns, "t", &table(&[(1, "paris"), (3, "berlin")])).unwrap();
    loom.commit(ns, "nas", "feature", 3).unwrap();

    loom.checkout_branch(ns, "main").unwrap();
    let MergeOutcome::Merged(_) = loom.merge(ns, "feature", "nas", 4).unwrap() else {
        panic!("disjoint-row inserts must merge cleanly");
    };
    // The merged table has rows 1,2,3. The rebuilt index must see both paris rows (1,2) and the
    // berlin row (3) - i.e. the index reflects the merged row set, not either parent's.
    let paris = loom
        .index_scan(ns, "t", "by_city", &[Value::Text("paris".into())])
        .unwrap();
    assert_eq!(
        paris.iter().map(|r| r[0].clone()).collect::<Vec<_>>(),
        vec![Value::Int(1), Value::Int(2)]
    );
    let berlin = loom
        .index_scan(ns, "t", "by_city", &[Value::Text("berlin".into())])
        .unwrap();
    assert_eq!(berlin.len(), 1);
    assert_eq!(berlin[0][0], Value::Int(3));
}

#[test]
fn rich_values_round_trip_and_keys_are_self_delimiting() {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
    let samples = vec![
        Value::Null,
        Value::Bool(true),
        Value::Int(-5),
        Value::Float(1.5),
        Value::Text("hi".into()),
        Value::Bytes(vec![1, 2, 3]),
        Value::I8(-1),
        Value::I16(-300),
        Value::I32(-70_000),
        Value::I128(-(1i128 << 100)),
        Value::U8(255),
        Value::U16(60_000),
        Value::U32(4_000_000_000),
        Value::U64(u64::MAX),
        Value::U128(u128::MAX),
        Value::F32(-2.5),
        Value::Decimal {
            mantissa: 150,
            scale: 2,
        },
        Value::Date(19_000),
        Value::Time(123_456_789),
        Value::Timestamp(-1),
        Value::Interval {
            months: 3,
            micros: -100,
        },
        Value::Uuid(0x1234_5678_9abc_def0_1122_3344_5566_7788),
        Value::Inet(IpAddr::V4(Ipv4Addr::new(192, 168, 0, 1))),
        Value::Inet(IpAddr::V6(Ipv6Addr::LOCALHOST)),
        Value::Point { x: 1.0, y: -2.0 },
        Value::List(vec![Value::Int(1), Value::Text("a".into())]),
        Value::Map(BTreeMap::from([("k".to_string(), Value::Int(9))])),
    ];
    for v in &samples {
        // Cell codec: encode then decode reproduces the value bit-exact.
        let back = cell_from(cell_value(v)).unwrap();
        assert_eq!(&back, v, "cell round-trip for {v:?}");
        // Key codec is self-delimiting: skip consumes exactly the key bytes.
        let kb = key_bytes(v);
        let mut kc = Cur::new(&kb);
        skip_key_value(&mut kc).unwrap();
        assert_eq!(kc.pos, kb.len(), "key skip consumed all bytes for {v:?}");
    }
}

#[test]
fn new_scalar_keys_match_value_order() {
    let cases: Vec<Vec<Value>> = vec![
        vec![Value::I8(-128), Value::I8(-1), Value::I8(0), Value::I8(127)],
        vec![Value::U64(0), Value::U64(1), Value::U64(u64::MAX)],
        vec![
            Value::I128(i128::MIN),
            Value::I128(-1),
            Value::I128(0),
            Value::I128(i128::MAX),
        ],
        vec![Value::F32(-1.0), Value::F32(0.0), Value::F32(1.0)],
        vec![Value::Date(-10), Value::Date(0), Value::Date(10)],
        vec![
            Value::Timestamp(-5),
            Value::Timestamp(0),
            Value::Timestamp(5),
        ],
        vec![Value::Uuid(0), Value::Uuid(1), Value::Uuid(u128::MAX)],
    ];
    for seq in cases {
        for w in seq.windows(2) {
            assert!(
                key_bytes(&w[0]) < key_bytes(&w[1]),
                "key {:?} < {:?}",
                w[0],
                w[1]
            );
            assert!(w[0] < w[1], "ord {:?} < {:?}", w[0], w[1]);
        }
    }
}

#[test]
fn decimal_key_is_order_preserving_and_normalizes_scale() {
    let d = |m, s| Value::Decimal {
        mantissa: m,
        scale: s,
    };
    // Strictly ascending by numeric value: negatives, zero, fractions, integers.
    let asc = vec![
        d(-200, 2), // -2.00
        d(-15, 1),  // -1.5
        d(-123, 2), // -1.23
        d(-12, 1),  // -1.2
        d(0, 0),    // 0
        d(5, 1),    // 0.5
        d(12, 1),   // 1.2
        d(123, 2),  // 1.23
        d(15, 1),   // 1.5
        d(2, 0),    // 2
        d(15, 0),   // 15
        d(150, 0),  // 150
    ];
    for w in asc.windows(2) {
        assert!(
            key_bytes(&w[0]) < key_bytes(&w[1]),
            "decimal key order {:?} < {:?}",
            w[0],
            w[1]
        );
        assert!(w[0] < w[1], "decimal value order {:?} < {:?}", w[0], w[1]);
    }
    // Cross-scale equality: trailing zeros change neither the value nor its key.
    assert_eq!(key_bytes(&d(15, 1)), key_bytes(&d(150, 2))); // 1.5 == 1.50
    assert_eq!(d(15, 1), d(150, 2));
    assert_eq!(d(15, 0), d(150, 1)); // 15 == 15.0
}

#[test]
fn index_scan_supports_multi_column_and_leading_prefix() {
    use crate::provider::memory::MemoryStore;
    use crate::workspace::{FacetKind, WorkspaceId};

    let schema = Schema::new(
        vec![
            ("id".into(), ColumnType::Int),
            ("region".into(), ColumnType::Text),
            ("city".into(), ColumnType::Text),
        ],
        vec![0],
    )
    .unwrap()
    .with_index("by_loc", &["region", "city"], false)
    .unwrap();
    let mut t = Table::new(schema);
    for (id, r, c) in [(1, "west", "sf"), (2, "west", "la"), (3, "east", "ny")] {
        t.insert(vec![
            Value::Int(id),
            Value::Text(r.into()),
            Value::Text(c.into()),
        ])
        .unwrap();
    }
    let mut loom = Loom::new(MemoryStore::new());
    let ns = loom
        .registry_mut()
        .create(FacetKind::Sql, None, WorkspaceId::from_bytes([16; 16]))
        .unwrap();
    put_table(&mut loom, ns, "loc", &t).unwrap();

    // Full two-column equality.
    let exact = loom
        .index_scan(
            ns,
            "loc",
            "by_loc",
            &[Value::Text("west".into()), Value::Text("sf".into())],
        )
        .unwrap();
    assert_eq!(
        exact.iter().map(|r| r[0].clone()).collect::<Vec<_>>(),
        vec![Value::Int(1)]
    );

    // Leading-column prefix: all "west" rows, ordered by (city, pk) -> la(2), sf(1).
    let west = loom
        .index_scan(ns, "loc", "by_loc", &[Value::Text("west".into())])
        .unwrap();
    assert_eq!(
        west.iter().map(|r| r[0].clone()).collect::<Vec<_>>(),
        vec![Value::Int(2), Value::Int(1)]
    );

    // A leading value with no rows.
    assert!(
        loom.index_scan(ns, "loc", "by_loc", &[Value::Text("north".into())])
            .unwrap()
            .is_empty()
    );
}

#[test]
fn merge_unique_violation_is_a_conflict() {
    use crate::provider::memory::MemoryStore;
    use crate::vcs::{Loom, MergeOutcome};
    use crate::workspace::{FacetKind, WorkspaceId};

    let schema = || {
        Schema::new(
            vec![
                ("id".into(), ColumnType::Int),
                ("email".into(), ColumnType::Text),
            ],
            vec![0],
        )
        .unwrap()
        .with_index("by_email", &["email"], true)
        .unwrap()
    };
    let table = |rows: &[(i64, &str)]| {
        let mut t = Table::new(schema());
        for (id, e) in rows {
            t.insert(vec![Value::Int(*id), Value::Text((*e).into())])
                .unwrap();
        }
        t
    };

    let mut loom = Loom::new(MemoryStore::new());
    let ns = loom
        .registry_mut()
        .create(FacetKind::Sql, None, WorkspaceId::from_bytes([17; 16]))
        .unwrap();
    put_table(&mut loom, ns, "u", &table(&[(1, "a@x")])).unwrap();
    loom.commit(ns, "nas", "base", 1).unwrap();
    loom.branch(ns, "feature").unwrap();

    // main adds row 2, feature adds row 3 - disjoint rows, but both with the same unique email.
    put_table(&mut loom, ns, "u", &table(&[(1, "a@x"), (2, "dup@x")])).unwrap();
    loom.commit(ns, "nas", "main", 2).unwrap();
    loom.checkout_branch(ns, "feature").unwrap();
    put_table(&mut loom, ns, "u", &table(&[(1, "a@x"), (3, "dup@x")])).unwrap();
    loom.commit(ns, "nas", "feature", 3).unwrap();

    // Rows merge cleanly (disjoint pks), but their union violates the unique email -> conflict.
    loom.checkout_branch(ns, "main").unwrap();
    match loom.merge(ns, "feature", "nas", 4).unwrap() {
        MergeOutcome::Conflicts(c) => assert_eq!(c, vec!["u".to_string()]),
        other => panic!("expected a unique-violation conflict, got {other:?}"),
    }
}

#[test]
fn cell_level_merge_reconciles_disjoint_column_edits() {
    use crate::provider::memory::MemoryStore;
    use crate::vcs::{Loom, MergeOutcome};
    use crate::workspace::{FacetKind, WorkspaceId};

    // (id pk, name, city): base row 1 = (Ann, Paris). main edits name; feature edits city.
    let schema = || {
        Schema::new(
            vec![
                ("id".into(), ColumnType::Int),
                ("name".into(), ColumnType::Text),
                ("city".into(), ColumnType::Text),
            ],
            vec![0],
        )
        .unwrap()
    };
    let row = |id: i64, n: &str, c: &str| {
        vec![Value::Int(id), Value::Text(n.into()), Value::Text(c.into())]
    };
    let table = |r: Vec<Value>| {
        let mut t = Table::new(schema());
        t.insert(r).unwrap();
        t
    };

    let setup = || {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Sql, None, WorkspaceId::from_bytes([18; 16]))
            .unwrap();
        put_table(&mut loom, ns, "t", &table(row(1, "Ann", "Paris"))).unwrap();
        loom.commit(ns, "nas", "base", 1).unwrap();
        loom.branch(ns, "feature").unwrap();
        // main: change name only.
        put_table(&mut loom, ns, "t", &table(row(1, "Annie", "Paris"))).unwrap();
        loom.commit(ns, "nas", "main", 2).unwrap();
        loom.checkout_branch(ns, "feature").unwrap();
        // feature: change city only.
        put_table(&mut loom, ns, "t", &table(row(1, "Ann", "Lyon"))).unwrap();
        loom.commit(ns, "nas", "feature", 3).unwrap();
        loom.checkout_branch(ns, "main").unwrap();
        (loom, ns)
    };

    // Row-level (default) conflicts: both sides changed the same row.
    let (mut loom, ns) = setup();
    match loom.merge(ns, "feature", "nas", 4).unwrap() {
        MergeOutcome::Conflicts(c) => assert_eq!(c, vec!["t".to_string()]),
        other => panic!("row-level should conflict, got {other:?}"),
    }

    // Cell-level (opt-in) auto-merges: name from main, city from feature.
    let (mut loom, ns) = setup();
    let MergeOutcome::Merged(_) = loom.merge_cell_level(ns, "feature", "nas", 4).unwrap() else {
        panic!("cell-level should merge disjoint-column edits");
    };
    let merged = get_table(&loom, ns, "t").unwrap();
    let r = merged.get(&[Value::Int(1)]).unwrap();
    assert_eq!(r[1], Value::Text("Annie".into())); // name from main
    assert_eq!(r[2], Value::Text("Lyon".into())); // city from feature
}

#[test]
fn cell_level_merge_conflicts_on_same_column() {
    use crate::provider::memory::MemoryStore;
    use crate::vcs::{Loom, MergeOutcome};
    use crate::workspace::{FacetKind, WorkspaceId};

    let schema = || {
        Schema::new(
            vec![
                ("id".into(), ColumnType::Int),
                ("name".into(), ColumnType::Text),
            ],
            vec![0],
        )
        .unwrap()
    };
    let table = |n: &str| {
        let mut t = Table::new(schema());
        t.insert(vec![Value::Int(1), Value::Text(n.into())])
            .unwrap();
        t
    };
    let mut loom = Loom::new(MemoryStore::new());
    let ns = loom
        .registry_mut()
        .create(FacetKind::Sql, None, WorkspaceId::from_bytes([19; 16]))
        .unwrap();
    put_table(&mut loom, ns, "t", &table("Ann")).unwrap();
    loom.commit(ns, "nas", "base", 1).unwrap();
    loom.branch(ns, "feature").unwrap();
    put_table(&mut loom, ns, "t", &table("Annie")).unwrap();
    loom.commit(ns, "nas", "main", 2).unwrap();
    loom.checkout_branch(ns, "feature").unwrap();
    put_table(&mut loom, ns, "t", &table("Anna")).unwrap();
    loom.commit(ns, "nas", "feature", 3).unwrap();
    loom.checkout_branch(ns, "main").unwrap();

    // Both sides changed the same column differently: cell-level still conflicts.
    match loom.merge_cell_level(ns, "feature", "nas", 4).unwrap() {
        MergeOutcome::Conflicts(c) => assert_eq!(c, vec!["t".to_string()]),
        other => panic!("same-column edits must conflict even cell-level, got {other:?}"),
    }
}

/// An `ObjectStore` that counts `put` calls (writes = node hashing), to measure that an
/// incremental mutation touches `O(log n)` nodes rather than re-hashing the whole table.
struct Counting {
    inner: crate::provider::memory::MemoryStore,
    puts: std::sync::atomic::AtomicUsize,
}
impl Counting {
    fn new() -> Self {
        Self {
            inner: crate::provider::memory::MemoryStore::new(),
            puts: std::sync::atomic::AtomicUsize::new(0),
        }
    }
    fn puts(&self) -> usize {
        self.puts.load(std::sync::atomic::Ordering::Relaxed)
    }
    fn reset(&self) {
        self.puts.store(0, std::sync::atomic::Ordering::Relaxed);
    }
}
impl crate::provider::ObjectStore for Counting {
    fn put(&self, canonical: &[u8]) -> Result<crate::digest::Digest> {
        self.puts.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.inner.put(canonical)
    }
    fn get(&self, d: &crate::digest::Digest) -> Result<Option<Vec<u8>>> {
        self.inner.get(d)
    }
    fn has(&self, d: &crate::digest::Digest) -> Result<bool> {
        self.inner.has(d)
    }
    fn len(&self) -> usize {
        self.inner.len()
    }
}

#[test]
fn incremental_insert_cost_is_independent_of_table_size() {
    use crate::workspace::{FacetKind, WorkspaceId};

    // Node `put`s incurred by a single `insert_row` into a table that already has `n` rows. Only
    // the affected leaf + spine of the row map and each index are re-hashed (plus one table Tree),
    // so this is ~constant in `n`, not linear (which a full re-stage would be).
    fn insert_puts(n: i64) -> usize {
        let schema = Schema::new(
            vec![
                ("id".into(), ColumnType::Int),
                ("v".into(), ColumnType::Text),
            ],
            vec![0],
        )
        .unwrap()
        .with_index("by_v", &["v"], false)
        .unwrap();
        let mut t = Table::new(schema);
        for i in 0..n {
            t.insert(vec![Value::Int(i), Value::Text(format!("v{i}"))])
                .unwrap();
        }
        let mut loom = Loom::new(Counting::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Sql, None, WorkspaceId::from_bytes([20; 16]))
            .unwrap();
        put_table(&mut loom, ns, "t", &t).unwrap();
        loom.store().reset(); // count only the incremental insert below
        loom.insert_row(ns, "t", vec![Value::Int(n + 1), Value::Text("zzz".into())])
            .unwrap();
        loom.store().puts()
    }

    let small = insert_puts(500);
    let big = insert_puts(8000);
    // A one-row insert re-hashes only the affected leaf + spine: a handful of nodes, growing at
    // most by tree height (one extra level), never linearly with the 16x larger table.
    assert!(small <= 20, "small-table insert re-hashed {small} nodes");
    assert!(
        big <= small + 8,
        "big-table insert re-hashed {big} nodes vs {small} for a 16x smaller table - should be ~constant, not linear"
    );
}

fn big_users(n: i64) -> Table {
    let schema = Schema::new(
        vec![
            ("id".into(), ColumnType::Int),
            ("name".into(), ColumnType::Text),
        ],
        vec![0],
    )
    .unwrap();
    let mut t = Table::new(schema);
    for i in 0..n {
        t.insert(vec![Value::Int(i), Value::Text(format!("user-{i}"))])
            .unwrap();
    }
    t
}

#[test]
fn rows_shard_round_trip_and_point_lookup() {
    use crate::provider::memory::MemoryStore;
    let mut store = MemoryStore::new();
    let t = big_users(500);
    let root = t.build_rows(&mut store).unwrap().unwrap();

    // Point lookup without loading the whole table.
    let row = Table::get_row(&store, t.schema(), &root, &[Value::Int(250)])
        .unwrap()
        .unwrap();
    assert_eq!(row[1], Value::Text("user-250".into()));
    // A missing key.
    assert!(
        Table::get_row(&store, t.schema(), &root, &[Value::Int(99_999)])
            .unwrap()
            .is_none()
    );

    // Full reload reconstructs an identical table (same canonical bytes).
    let back = Table::load_rows(&store, t.schema().clone(), &root).unwrap();
    assert_eq!(back.len(), 500);
    assert_eq!(back.encode(), t.encode());

    // An empty table has no prolly root.
    assert!(big_users(0).build_rows(&mut store).unwrap().is_none());
}

#[test]
fn one_row_change_shares_most_prolly_nodes() {
    use crate::provider::memory::MemoryStore;
    use std::collections::BTreeSet;
    let mut store = MemoryStore::new();

    let mut t = big_users(2000);
    let root1 = t.build_rows(&mut store).unwrap().unwrap();
    let nodes1: BTreeSet<_> = crate::prolly::reachable_nodes(&store, &root1)
        .unwrap()
        .into_iter()
        .collect();

    // Change one row's non-key column and re-shard.
    t.insert(vec![Value::Int(1000), Value::Text("CHANGED".into())])
        .unwrap();
    let root2 = t.build_rows(&mut store).unwrap().unwrap();
    let nodes2: BTreeSet<_> = crate::prolly::reachable_nodes(&store, &root2)
        .unwrap()
        .into_iter()
        .collect();

    assert_ne!(root1, root2);
    let shared = nodes1.intersection(&nodes2).count();
    let changed = nodes1.symmetric_difference(&nodes2).count();
    assert!(
        shared > changed * 4,
        "row-level structural sharing: shared={shared}, changed={changed}"
    );
}

#[test]
fn row_map_keys_are_in_primary_key_order() {
    use crate::provider::memory::MemoryStore;
    let mut store = MemoryStore::new();
    let schema = Schema::new(
        vec![
            ("id".into(), ColumnType::Int),
            ("name".into(), ColumnType::Text),
        ],
        vec![0],
    )
    .unwrap();
    let mut t = Table::new(schema);
    // PKs whose little-endian bytes mis-sort (2 vs 256) plus a negative; only an order-preserving
    // key encoding makes the prolly scan come back in true PK order.
    for id in [256i64, 2, 10, 1, 1000, -5] {
        t.insert(vec![Value::Int(id), Value::Text(format!("r{id}"))])
            .unwrap();
    }
    let root = t.build_rows(&mut store).unwrap().unwrap();
    // `prolly::entries` yields rows in ascending key-byte order; order-preserving keys give PK order.
    let ids: Vec<i64> = crate::prolly::entries(&store, &root)
        .unwrap()
        .into_iter()
        .map(|(_, v)| match &decode_row(t.schema(), &v).unwrap()[0] {
            Value::Int(i) => *i,
            other => panic!("expected Int pk, got {other:?}"),
        })
        .collect();
    assert_eq!(ids, vec![-5, 1, 2, 10, 256, 1000]);
}

#[test]
fn canonical_encode_is_deterministic_and_round_trips() {
    let t = users();
    let a = t.encode();
    let b = t.encode();
    assert_eq!(a, b, "encoding must be deterministic");
    // Insertion order must not matter (BTreeMap keys by PK).
    let mut t2 = {
        let schema = Schema::new(
            vec![
                ("id".into(), ColumnType::Int),
                ("name".into(), ColumnType::Text),
                ("active".into(), ColumnType::Bool),
            ],
            vec![0],
        )
        .unwrap();
        Table::new(schema)
    };
    t2.insert(vec![
        Value::Int(1),
        Value::Text("ann".into()),
        Value::Bool(true),
    ])
    .unwrap();
    t2.insert(vec![
        Value::Int(2),
        Value::Text("bob".into()),
        Value::Bool(false),
    ])
    .unwrap();
    assert_eq!(
        t.encode(),
        t2.encode(),
        "PK order, not insert order, decides bytes"
    );
    // Round-trip.
    let decoded = Table::decode(&a).unwrap();
    assert_eq!(decoded.len(), 2);
    assert_eq!(decoded.encode(), a);
}
fn acl_nid(seed: u8) -> WorkspaceId {
    WorkspaceId::from_bytes([seed; 16])
}

fn acl_sql_table(rows: &[(i64, &str)]) -> Table {
    let schema = Schema::new(
        vec![
            ("id".to_string(), ColumnType::Int),
            ("value".to_string(), ColumnType::Text),
        ],
        vec![0],
    )
    .unwrap();
    let mut table = Table::new(schema);
    for (id, value) in rows {
        table
            .insert(vec![Value::Int(*id), Value::Text((*value).to_string())])
            .unwrap();
    }
    table
}

#[test]
fn authenticated_sql_table_writes_are_acl_checked() {
    let mut loom = Loom::new(crate::MemoryStore::new());
    let ns = loom
        .registry_mut()
        .create(FacetKind::Sql, None, acl_nid(13))
        .unwrap();
    let root = acl_nid(1);
    let mut identity = crate::IdentityStore::new(root);
    identity.set_passphrase(root, "root", b"12345678").unwrap();
    let session = identity
        .authenticate_passphrase(root, "root", "session")
        .unwrap();
    loom.set_identity_store(identity);
    loom.set_session(session.id);

    let path = ".loom/facets/sql/main/tables/users";
    let table = acl_sql_table(&[(1, "a")]);
    assert_eq!(
        loom.stage_table(ns, path, &table).unwrap_err().code,
        Code::PermissionDenied
    );
    loom.acl_store_mut()
        .allow(
            crate::AclSubject::Principal(root),
            Some(ns),
            Some(FacetKind::Sql),
            [crate::AclRight::Write],
        )
        .unwrap();
    loom.stage_table(ns, path, &table).unwrap();
    loom.insert_row(ns, path, vec![Value::Int(2), Value::Text("b".to_string())])
        .unwrap();
    loom.delete_row(ns, path, &[Value::Int(1)]).unwrap();
}

#[test]
fn authenticated_sql_table_reads_are_acl_checked() {
    let mut loom = Loom::new(crate::MemoryStore::new());
    let ns = loom
        .registry_mut()
        .create(FacetKind::Sql, None, acl_nid(14))
        .unwrap();
    let path = ".loom/facets/sql/main/tables/users";
    loom.stage_table(ns, path, &acl_sql_table(&[(1, "a")]))
        .unwrap();
    let c1 = loom.commit(ns, "root", "c1", 0).unwrap();
    loom.stage_table(ns, path, &acl_sql_table(&[(1, "b")]))
        .unwrap();
    let c2 = loom.commit(ns, "root", "c2", 1).unwrap();

    let root = acl_nid(1);
    let mut identity = crate::IdentityStore::new(root);
    identity.set_passphrase(root, "root", b"12345678").unwrap();
    let session = identity
        .authenticate_passphrase(root, "root", "session")
        .unwrap();
    loom.set_identity_store(identity);
    loom.set_session(session.id);

    assert_eq!(
        loom.read_table(ns, path).unwrap_err().code,
        Code::PermissionDenied
    );
    assert_eq!(
        loom.blame_table(ns, "main", path).unwrap_err().code,
        Code::PermissionDenied
    );
    assert_eq!(
        loom.diff_table(ns, path, c1, c2).unwrap_err().code,
        Code::PermissionDenied
    );
    loom.acl_store_mut()
        .allow(
            crate::AclSubject::Principal(root),
            Some(ns),
            Some(FacetKind::Sql),
            [crate::AclRight::Read],
        )
        .unwrap();
    assert_eq!(loom.read_table(ns, path).unwrap().len(), 1);
    assert_eq!(loom.blame_table(ns, "main", path).unwrap().len(), 1);
    assert_eq!(loom.diff_table(ns, path, c1, c2).unwrap().len(), 1);
    assert_eq!(
        loom.log(ns, "main").unwrap_err().code,
        Code::PermissionDenied
    );
}

#[test]
fn authenticated_sql_table_reads_honor_table_scopes() {
    let mut loom = Loom::new(crate::MemoryStore::new());
    let ns = loom
        .registry_mut()
        .create(FacetKind::Sql, None, acl_nid(17))
        .unwrap();
    let users_path = ".loom/facets/sql/main/tables/users";
    let private_path = ".loom/facets/sql/main/tables/private";
    loom.stage_table(ns, users_path, &acl_sql_table(&[(1, "a")]))
        .unwrap();
    loom.stage_table(ns, private_path, &acl_sql_table(&[(1, "b")]))
        .unwrap();

    let root = acl_nid(1);
    let mut identity = crate::IdentityStore::new(root);
    identity.set_passphrase(root, "root", b"12345678").unwrap();
    let session = identity
        .authenticate_passphrase(root, "root", "session")
        .unwrap();
    loom.set_identity_store(identity);
    loom.set_session(session.id);
    loom.acl_store_mut()
        .grant(crate::AclGrant {
            subject: crate::AclSubject::Principal(root),
            workspace: Some(ns),
            domain: Some(FacetKind::Sql.into()),
            ref_glob: None,
            scopes: vec![crate::AclScope::Prefix {
                kind: crate::AclScopeKind::Table,
                prefix: users_path.as_bytes().to_vec(),
            }],
            rights: [crate::AclRight::Read].into_iter().collect(),
            effect: crate::AclEffect::Allow,
            predicate: None,
        })
        .unwrap();

    assert_eq!(loom.read_table(ns, users_path).unwrap().len(), 1);
    assert_eq!(
        loom.read_table(ns, private_path).unwrap_err().code,
        Code::PermissionDenied
    );
}

#[test]
fn table_diff_rejects_commits_outside_the_workspace() {
    let mut loom = Loom::new(crate::MemoryStore::new());
    let ns_a = loom
        .registry_mut()
        .create(FacetKind::Sql, Some("a"), acl_nid(15))
        .unwrap();
    let ns_b = loom
        .registry_mut()
        .create(FacetKind::Sql, Some("b"), acl_nid(16))
        .unwrap();
    let path = ".loom/facets/sql/main/tables/users";
    loom.stage_table(ns_a, path, &acl_sql_table(&[(1, "a")]))
        .unwrap();
    loom.commit(ns_a, "root", "a1", 0).unwrap();
    loom.stage_table(ns_b, path, &acl_sql_table(&[(1, "a")]))
        .unwrap();
    let b1 = loom.commit(ns_b, "root", "b1", 0).unwrap();
    loom.stage_table(ns_b, path, &acl_sql_table(&[(1, "b")]))
        .unwrap();
    let b2 = loom.commit(ns_b, "root", "b2", 1).unwrap();

    let root = acl_nid(1);
    let mut identity = crate::IdentityStore::new(root);
    identity.set_passphrase(root, "root", b"12345678").unwrap();
    let session = identity
        .authenticate_passphrase(root, "root", "session")
        .unwrap();
    loom.set_identity_store(identity);
    loom.set_session(session.id);
    loom.acl_store_mut()
        .allow(
            crate::AclSubject::Principal(root),
            Some(ns_a),
            Some(FacetKind::Sql),
            [crate::AclRight::Read],
        )
        .unwrap();

    assert_eq!(
        loom.diff_table(ns_a, path, b1, b2).unwrap_err().code,
        Code::PermissionDenied
    );
}
