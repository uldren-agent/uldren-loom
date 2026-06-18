//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

//! Executable `exec` (program-execution) conformance runner. Mirrors the spec `0015` compute facade
//! over a WASM guest: gated `dry_run`/`apply`, direct, and batched modes, deterministic proposals, the
//! ACL/manifest-grant intersection, out-of-fuel metering, and manifest identity. Runs the real
//! `loom-compute` facade against a live [`Loom`], so any backend that advertises `exec` must satisfy the
//! same contract the other facade suites hold.

use super::*;

use std::collections::BTreeMap;

use loom_compute::{
    BatchExecRequest, Capability, DirectExecRequest, ExecContext, ExecError, ExecRequest, ExecStep,
    Grant, GrantSet, Manifest, Mode, Scope, StateAccess, apply, batch, direct, dry_run,
};
use loom_core::{
    DataframeInputFormat, DataframeMaterialization, DataframeMaterializationTarget,
    DataframeOperation, DataframePlan, DataframeSourceBinding, DataframeSourceKind, FieldMapping,
    FieldValue, LoomError, MetaFilter, Metric, PrincipalId, Query, QueryRequest, key_to_cbor,
};

fn nid(seed: u8) -> WorkspaceId {
    WorkspaceId::from_bytes([seed; 16])
}

fn pid(seed: u8) -> PrincipalId {
    PrincipalId::from_bytes([seed; 16])
}

/// Normalize an `ExecError` into the crate-wide `LoomError` result type, preserving the stable code and
/// message so `?` composes with the loom-core facet helpers the other runners use.
fn le(err: ExecError) -> LoomError {
    LoomError::new(err.code(), err.to_string())
}

fn all_grants() -> GrantSet {
    GrantSet::new(vec![
        Grant {
            facet: Capability::Files,
            mode: Mode::ReadWrite,
            scopes: vec![Scope::All],
        },
        Grant {
            facet: Capability::Kv,
            mode: Mode::ReadWrite,
            scopes: vec![Scope::All],
        },
    ])
}

fn files_grants() -> GrantSet {
    GrantSet::new(vec![Grant {
        facet: Capability::Files,
        mode: Mode::ReadWrite,
        scopes: vec![Scope::All],
    }])
}

fn full_context(ns: WorkspaceId) -> ExecContext {
    ExecContext {
        workspace: ns,
        principal: pid(9),
        roles: Vec::new(),
        authenticated: true,
        base_branch: "main".to_string(),
        grants: all_grants(),
    }
}

fn files_only_context(ns: WorkspaceId) -> ExecContext {
    ExecContext {
        workspace: ns,
        principal: pid(9),
        roles: Vec::new(),
        authenticated: true,
        base_branch: "main".to_string(),
        grants: files_grants(),
    }
}

fn multifacet_grants() -> GrantSet {
    GrantSet::new(
        [
            Capability::Files,
            Capability::Cas,
            Capability::Document,
            Capability::Queue,
            Capability::TimeSeries,
            Capability::Ledger,
            Capability::Graph,
            Capability::Vector,
            Capability::Columnar,
            Capability::Search,
            Capability::Dataframe,
        ]
        .into_iter()
        .map(|facet| Grant {
            facet,
            mode: Mode::ReadWrite,
            scopes: vec![Scope::All],
        })
        .collect(),
    )
}

fn multifacet_context(ns: WorkspaceId) -> ExecContext {
    ExecContext {
        workspace: ns,
        principal: pid(9),
        roles: Vec::new(),
        authenticated: true,
        base_branch: "main".to_string(),
        grants: multifacet_grants(),
    }
}

fn manifest(wasm: &[u8], grants: GrantSet) -> Manifest {
    Manifest::for_wasm("test", wasm, grants)
}

fn step<'a>(wasm: &'a [u8], key: &Value) -> ExecStep<'a> {
    ExecStep {
        manifest: manifest(wasm, all_grants()),
        wasm,
        inputs: BTreeMap::from([("nk".to_string(), key_to_cbor(key))]),
        fuel: 1_000_000,
    }
}

fn gated_request<'a>(
    ns: WorkspaceId,
    wasm: &'a [u8],
    key: &Value,
    fork_branch: &str,
) -> ExecRequest<'a> {
    ExecRequest {
        context: full_context(ns),
        fork_branch: fork_branch.to_string(),
        step: step(wasm, key),
        author: "program".to_string(),
        message: "gated".to_string(),
        timestamp_ms: 2,
    }
}

/// Seed a workspace with a base commit. When `grant_exec` is set the principal receives the workspace
/// `Execute` right so the ACL/manifest intersection can permit an operation; otherwise the store is
/// authenticated but ungranted (default-deny).
fn seed_ns<S: ObjectStore>(
    loom: &mut Loom<S>,
    seed: u8,
    grant_exec: bool,
) -> Result<(WorkspaceId, Digest)> {
    let name = format!("exec-{seed}");
    let ns = loom
        .registry_mut()
        .create(FacetKind::Files, Some(&name), nid(seed))?;
    if grant_exec {
        loom.acl_store_mut().allow(
            AclSubject::Principal(pid(9)),
            Some(ns),
            None,
            [AclRight::Execute],
        )?;
    }
    loom.write_file(ns, "/seed", b"s", 0o100644)?;
    let base = loom.commit(ns, "nas", "base", 1)?;
    Ok((ns, base))
}

// Fetches typed key from input `nk`, writes `/out` and `cache/<key>=v` through the host ABI, then logs.
fn program() -> Vec<u8> {
    wat::parse_str(
        r#"(module
             (import "env" "input_get" (func $in (param i32 i32 i32 i32) (result i32)))
             (import "env" "kv_put" (func $put (param i32 i32 i32 i32 i32 i32)))
             (import "env" "file_write" (func $fw (param i32 i32 i32 i32)))
             (import "env" "log" (func $log (param i32 i32)))
             (memory (export "memory") 1)
             (data (i32.const 0) "cache")
             (data (i32.const 16) "nk")
             (data (i32.const 32) "v")
             (data (i32.const 48) "/out")
             (data (i32.const 64) "done")
             (func (export "run") (local $l i32)
               (call $fw (i32.const 48)(i32.const 4)(i32.const 32)(i32.const 1))
               (local.set $l (call $in (i32.const 16)(i32.const 2)(i32.const 200)(i32.const 64)))
               (call $put (i32.const 0)(i32.const 5)(i32.const 200)(local.get $l)(i32.const 32)(i32.const 1))
               (call $log (i32.const 64)(i32.const 4))))"#,
    )
    .expect("assemble exec conformance program")
}

// Writes `/second` and logs `two`; used to prove a batch applies every step in one commit.
fn second_program() -> Vec<u8> {
    wat::parse_str(
        r#"(module
             (import "env" "file_write" (func $fw (param i32 i32 i32 i32)))
             (import "env" "log" (func $log (param i32 i32)))
             (memory (export "memory") 1)
             (data (i32.const 0) "/second")
             (data (i32.const 16) "w")
             (data (i32.const 32) "two")
             (func (export "run")
               (call $fw (i32.const 0)(i32.const 7)(i32.const 16)(i32.const 1))
               (call $log (i32.const 32)(i32.const 3))))"#,
    )
    .expect("assemble exec conformance second program")
}

fn seed_multifacet_ns<S: ObjectStore>(loom: &mut Loom<S>) -> Result<WorkspaceId> {
    let ns = loom
        .registry_mut()
        .create(FacetKind::Files, Some("exec-state-access"), nid(10))?;
    for facet in [
        FacetKind::Cas,
        FacetKind::Document,
        FacetKind::Queue,
        FacetKind::TimeSeries,
        FacetKind::Ledger,
        FacetKind::Graph,
        FacetKind::Vector,
        FacetKind::Columnar,
        FacetKind::Search,
        FacetKind::Dataframe,
    ] {
        loom.registry_mut().add_facet(ns, facet)?;
    }
    loom.acl_store_mut().allow(
        AclSubject::Principal(pid(9)),
        Some(ns),
        None,
        [AclRight::Execute],
    )?;
    Ok(ns)
}

fn assert_multifacet_state_access<S: ObjectStore>(loom: &mut Loom<S>) -> Result<()> {
    let ns = seed_multifacet_ns(loom)?;
    loom.create_directory(ns, "inputs", true)?;
    loom.write_file(ns, "inputs/events.csv", b"id,name\n1,ada\n", 0o100644)?;

    let digest;
    {
        let mut state = StateAccess::new(loom, multifacet_context(ns));

        state.file_write("/notes.txt", b"note").map_err(le)?;
        let names = state
            .file_list("/")
            .map_err(le)?
            .into_iter()
            .map(|entry| entry.name)
            .collect::<Vec<_>>();
        assert!(
            names.iter().any(|name| name == "notes.txt"),
            "file listing must expose the file written through StateAccess"
        );

        digest = state.cas_put(b"cas bytes").map_err(le)?;
        assert_eq!(
            state.cas_get(&digest).map_err(le)?,
            Some(b"cas bytes".to_vec()),
            "CAS put/get must round-trip through StateAccess"
        );

        state
            .doc_put("docs", "d1", b"document".to_vec())
            .map_err(le)?;
        assert_eq!(
            state.doc_get("docs", "d1").map_err(le)?,
            Some(b"document".to_vec()),
            "document put/get must round-trip through StateAccess"
        );

        assert_eq!(state.queue_append("events", b"e1").map_err(le)?, 0);
        assert_eq!(
            state.queue_get("events", 0).map_err(le)?,
            Some(b"e1".to_vec()),
            "queue append/get must round-trip through StateAccess"
        );

        state
            .time_series_put("cpu", 100, b"low".to_vec())
            .map_err(le)?;
        assert_eq!(
            state.time_series_latest("cpu").map_err(le)?,
            Some((100, b"low".to_vec())),
            "time-series latest must reflect the written point"
        );

        assert_eq!(state.ledger_append("audit", b"a1".to_vec()).map_err(le)?, 0);
        state.ledger_verify("audit").map_err(le)?;
        assert_eq!(
            state.ledger_get("audit", 0).map_err(le)?,
            Some(b"a1".to_vec()),
            "ledger append/get must round-trip through StateAccess"
        );

        state
            .graph_upsert_node("deps", "root", Props::new())
            .map_err(le)?;
        state
            .graph_upsert_node("deps", "leaf", Props::new())
            .map_err(le)?;
        state
            .graph_upsert_edge("deps", "e1", "root", "leaf", "uses", Props::new())
            .map_err(le)?;
        assert_eq!(
            state.graph_neighbors("deps", "root").map_err(le)?,
            vec!["leaf".to_string()],
            "graph neighbors must reflect the inserted edge"
        );

        state
            .columnar_create(
                "metrics",
                vec![
                    ("id".to_string(), ColumnType::Int),
                    ("name".to_string(), ColumnType::Text),
                ],
                0,
            )
            .map_err(le)?;
        state
            .columnar_append(
                "metrics",
                vec![Value::Int(1), Value::Text("latency".to_string())],
            )
            .map_err(le)?;
        assert_eq!(
            state.columnar_rows("metrics").map_err(le)?,
            1,
            "columnar append must be visible through row count"
        );

        let mut mapping = search::Mapping::new();
        mapping.insert("body".to_string(), FieldMapping::text());
        state.search_create("docs", mapping).map_err(le)?;
        let mut document = search::Document::new();
        document.insert(
            "body".to_string(),
            FieldValue::Text("hello world".to_string()),
        );
        state
            .search_index("docs", b"doc-1".to_vec(), document)
            .map_err(le)?;
        assert_eq!(
            state
                .search_query(
                    "docs",
                    &QueryRequest::new(
                        Query::Match {
                            field: "body".to_string(),
                            text: "hello".to_string(),
                        },
                        10,
                        0,
                    ),
                )
                .map_err(le)?
                .hits
                .len(),
            1,
            "search query must find the indexed document"
        );

        state.vector_create("emb", 2, Metric::Cosine).map_err(le)?;
        state
            .vector_upsert("emb", "a", vec![1.0, 0.0], BTreeMap::new())
            .map_err(le)?;
        assert_eq!(
            state
                .vector_search("emb", &[1.0, 0.0], 1, &MetaFilter::All)
                .map_err(le)?[0]
                .id,
            "a",
            "vector search must find the inserted vector"
        );

        let plan = DataframePlan::new(vec![DataframeSourceBinding::new(
            "events",
            DataframeSourceKind::Files,
            "inputs/events.csv",
            DataframeInputFormat::Csv,
        )])?
        .with_operations(vec![
            DataframeOperation::Scan {
                source: "events".to_string(),
            },
            DataframeOperation::Select {
                columns: vec!["id".to_string()],
            },
        ])?
        .with_materialization(DataframeMaterialization::new(
            DataframeMaterializationTarget::Columnar,
            Some("analytics/events".to_string()),
            DataframeInputFormat::Parquet,
        ))?;
        state.dataframe_create("etl/events", &plan).map_err(le)?;
        assert_eq!(
            state
                .dataframe_preview("etl/events", 5)
                .map_err(le)?
                .row_count(),
            1,
            "dataframe preview must read its file source through StateAccess grants"
        );
        state.dataframe_materialize("etl/events").map_err(le)?;
        assert_eq!(
            state.columnar_rows("analytics/events").map_err(le)?,
            1,
            "dataframe materialization must write the configured columnar target"
        );
    }

    let mut denied = StateAccess::new(
        loom,
        ExecContext {
            workspace: ns,
            principal: pid(9),
            roles: Vec::new(),
            authenticated: true,
            base_branch: "main".to_string(),
            grants: GrantSet::new(vec![Grant {
                facet: Capability::Files,
                mode: Mode::ReadWrite,
                scopes: vec![Scope::All],
            }]),
        },
    );
    assert!(
        matches!(
            denied.doc_get("docs", "d1"),
            Err(ExecError::Denied(_) | ExecError::Core(_))
        ),
        "manifest grants must deny a facet absent from the execution grant set"
    );
    drop(denied);

    assert!(
        cas_has(loom, ns, &digest)?,
        "CAS object must remain reachable"
    );
    Ok(())
}

/// Execute the workspace-scoped `exec` behavioral suite against a live [`Loom`] over the public
/// `loom-compute` facade. Proves the spec `0015` contract end to end: a gated `dry_run` proposes a
/// deterministic result without touching the base branch and `apply` adopts it; direct mode commits to
/// the base; a batch applies every step in a single commit; the ACL/manifest-grant intersection is
/// fail-closed (a per-operation denial rolls back and a step whose grants exceed the context upper bound
/// is rejected before any write); out-of-fuel maps to `ResourceExhausted` and commits nothing; and the
/// program manifest round-trips byte-for-byte through Loom Canonical CBOR v1.
pub fn run_exec_behavior<S: ObjectStore>(loom: &mut Loom<S>) -> Result<()> {
    let key = Value::Text("k".to_string());
    let wasm = program();

    // gated: dry_run proposes deterministically off two independent forks of the same base, leaves the
    // base untouched, and apply then adopts the program's file and KV writes.
    let (ns, base) = seed_ns(loom, 1, true)?;
    let r1 = dry_run(loom, gated_request(ns, &wasm, &key, "proposed-a")).map_err(le)?;
    assert_eq!(
        loom.registry().branch_tip(ns, "main")?,
        Some(base),
        "a gated dry_run must not advance the base branch"
    );
    let r2 = dry_run(loom, gated_request(ns, &wasm, &key, "proposed-b")).map_err(le)?;
    assert_eq!(
        r1.after, r2.after,
        "identical program, inputs, and commit metadata must yield the same proposal digest"
    );
    assert_eq!(
        r1.fuel_used, r2.fuel_used,
        "fuel accounting must be deterministic"
    );
    assert!(r1.fuel_used > 0, "a program that runs must consume fuel");
    assert_eq!(
        r1.logs,
        vec!["done".to_string()],
        "program logs are captured in order"
    );
    assert!(
        r1.changes.iter().any(|c| c.path == "out"),
        "the proposal must carry the program's file write"
    );

    let outcome = apply(loom, ns, "main", "proposed-a", "nas", 3).map_err(le)?;
    assert!(
        matches!(
            outcome,
            MergeOutcome::FastForward(_) | MergeOutcome::Merged(_)
        ),
        "apply must merge the proposal into the base"
    );
    assert_eq!(
        loom.read_file(ns, "/out")?,
        b"v",
        "the adopted state carries the program's file write"
    );
    assert_eq!(
        kv_get(loom, ns, "cache", &key)?,
        Some(b"v".to_vec()),
        "the adopted state carries the program's KV write"
    );

    // direct: a low-risk immediate apply commits straight to the base branch.
    let (ns, base) = seed_ns(loom, 3, true)?;
    let report = direct(
        loom,
        DirectExecRequest {
            context: full_context(ns),
            step: step(&wasm, &key),
            author: "program".to_string(),
            message: "direct".to_string(),
            timestamp_ms: 4,
        },
    )
    .map_err(le)?;
    assert_eq!(report.branch, "main", "direct commits to the base branch");
    assert_ne!(report.after, base, "direct advances the base branch");
    assert_eq!(loom.registry().branch_tip(ns, "main")?, Some(report.after));
    assert_eq!(loom.read_file(ns, "/out")?, b"v");

    // batch: every per-manifest step applies in a single commit.
    let second = second_program();
    let (ns, _base) = seed_ns(loom, 8, true)?;
    let report = batch(
        loom,
        BatchExecRequest {
            context: full_context(ns),
            steps: vec![
                step(&wasm, &key),
                ExecStep {
                    manifest: manifest(&second, files_grants()),
                    wasm: &second,
                    inputs: BTreeMap::new(),
                    fuel: 1_000_000,
                },
            ],
            author: "program".to_string(),
            message: "batch".to_string(),
            timestamp_ms: 5,
        },
    )
    .map_err(le)?;
    assert_eq!(loom.registry().branch_tip(ns, "main")?, Some(report.after));
    assert_eq!(loom.read_file(ns, "/out")?, b"v");
    assert_eq!(loom.read_file(ns, "/second")?, b"w");
    assert_eq!(
        report.logs,
        vec!["done".to_string(), "two".to_string()],
        "batch preserves per-step log order"
    );

    // intersection (fail-closed): a per-operation denial inside a direct run rolls the whole run back.
    // Here the context grants both facets but the step manifest grants only files, so the program's KV
    // write is denied and the file write it already made is rolled back.
    let (ns, base) = seed_ns(loom, 5, true)?;
    let err = direct(
        loom,
        DirectExecRequest {
            context: full_context(ns),
            step: ExecStep {
                manifest: manifest(&wasm, files_grants()),
                wasm: &wasm,
                inputs: BTreeMap::from([("nk".to_string(), key_to_cbor(&key))]),
                fuel: 1_000_000,
            },
            author: "program".to_string(),
            message: "direct denied".to_string(),
            timestamp_ms: 6,
        },
    )
    .expect_err("a per-operation denial must fail the direct run");
    assert_eq!(
        err.code(),
        Code::PermissionDenied,
        "a denial normalizes to PermissionDenied"
    );
    assert_eq!(
        loom.registry().branch_tip(ns, "main")?,
        Some(base),
        "a denied direct run must not advance the base"
    );
    assert!(
        loom.read_file(ns, "/out").is_err(),
        "the rolled-back run leaves no file behind"
    );
    assert_eq!(
        kv_get(loom, ns, "cache", &key)?,
        None,
        "the rolled-back run leaves no KV entry behind"
    );

    // intersection (upper bound): a step whose grants exceed the context upper bound is rejected before
    // any state changes.
    let (ns, base) = seed_ns(loom, 6, true)?;
    let err = direct(
        loom,
        DirectExecRequest {
            context: files_only_context(ns),
            step: step(&wasm, &key),
            author: "program".to_string(),
            message: "over grant".to_string(),
            timestamp_ms: 6,
        },
    )
    .expect_err("a step grant beyond the context upper bound must be denied");
    assert!(
        matches!(err, ExecError::Denied(_)),
        "an over-broad step is denied before running"
    );
    assert_eq!(
        loom.registry().branch_tip(ns, "main")?,
        Some(base),
        "a rejected step must not advance the base"
    );

    // metering: exhausting the fuel budget maps to ResourceExhausted and commits nothing to the base.
    let (ns, base) = seed_ns(loom, 9, true)?;
    let mut starved = gated_request(ns, &wasm, &key, "starved");
    starved.step.fuel = 1;
    let err = dry_run(loom, starved).expect_err("a starved program must fail rather than propose");
    assert_eq!(
        err.code(),
        Code::ResourceExhausted,
        "out-of-fuel maps to ResourceExhausted"
    );
    assert_eq!(
        loom.registry().branch_tip(ns, "main")?,
        Some(base),
        "an out-of-fuel run commits nothing to the base"
    );
    assert_eq!(
        loom.registry().branch_tip(ns, "starved")?,
        None,
        "an out-of-fuel gated run discards the scratch branch"
    );
    assert_eq!(
        loom.registry().head_branch(ns)?,
        "main",
        "a failed gated run restores HEAD to the base branch"
    );

    // manifest identity: the program manifest round-trips byte-for-byte through Loom Canonical CBOR v1.
    let bytes = manifest(&wasm, all_grants()).encode();
    let decoded = Manifest::decode(&bytes).expect("a well-formed manifest must decode");
    assert_eq!(
        decoded.encode(),
        bytes,
        "manifest identity is a stable canonical byte string"
    );

    assert_multifacet_state_access(loom)?;

    Ok(())
}

/// Execute the SQL `StateAccess` behavior: SQL exec persists through the SQL facade, SQL query returns
/// canonical result CBOR, query is read-only, and manifest grants enforce read/write mode.
pub fn run_sql_state_access_behavior<S: ObjectStore>(loom: &mut Loom<S>) -> Result<()> {
    let ns = loom
        .registry_mut()
        .create(FacetKind::Sql, Some("sql-state-access"), nid(18))?;
    loom.acl_store_mut().allow(
        AclSubject::Principal(pid(9)),
        Some(ns),
        None,
        [AclRight::Execute],
    )?;
    let grants = GrantSet::new(vec![Grant {
        facet: Capability::Sql,
        mode: Mode::ReadWrite,
        scopes: vec![Scope::Prefix("app/".to_string())],
    }]);
    let mut state = StateAccess::new(
        loom,
        ExecContext {
            workspace: ns,
            principal: pid(9),
            roles: Vec::new(),
            authenticated: true,
            base_branch: "main".to_string(),
            grants,
        },
    );
    state
        .sql_exec_cbor(
            "app",
            "CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT); INSERT INTO t VALUES (1, 'a')",
        )
        .map_err(le)?;
    let result = state
        .sql_query_cbor("app", "SELECT id, v FROM t ORDER BY id")
        .map_err(le)?;
    let json = loom_result::result_to_json(&result)?;
    assert!(
        json.contains("\"id\"") && json.contains("\"v\"") && json.contains("\"a\""),
        "SQL StateAccess returns canonical result CBOR for the query surface"
    );
    let err = state
        .sql_query_cbor("app", "INSERT INTO t VALUES (2, 'b')")
        .unwrap_err();
    assert_eq!(
        err.code(),
        Code::PermissionDenied,
        "SQL query rejects mutating statements"
    );
    assert!(
        state.sql_query_cbor("other", "SELECT id FROM t").is_err(),
        "SQL StateAccess enforces the database scope"
    );
    Ok(())
}
