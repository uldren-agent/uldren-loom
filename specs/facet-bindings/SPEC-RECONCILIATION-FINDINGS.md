# Spec <-> Build Reconciliation - Findings (TEMPORARY working doc)

**Status:** Captured into `../IMPLEMENTATION-PLAN.md`. Keep this file only as source context until the
owner deletes or archives the temporary P9 finding log.

> **⚠ Temporary.** This is a working backlog, **not** part of the LCS spec series. It collects every
> spec/build divergence and spec-amendment item surfaced while drafting the P9 binding specs
> (`P9-0001`...`P9-0015`) and the fidelity analysis (`IMPLEMENTATION-FIDELITY.md`). **Run it in a dedicated
> session to resolve each item, then delete this file.** Each finding names where the truth is, the
> recommended resolution, and the doc that raised it. Nothing here has been silently applied to the specs.

**Created:** 2026-06-18 - **Source:** P9 binding work (Batches 1-5) + 0025 verification + control-plane (CP) bindings - **Items:** 13

## How to use

Work top-to-bottom: **Group A** (spec-text reconciliations) are cheap "decide the canonical truth and edit
the spec" fixes that remove client-facing confusion; **Group B** are build lifts to make the implementation
meet its own spec. For each item: confirm the recommended resolution (or override), apply it to the cited
spec/code, and check it off. The (high risk) item (F1) is the most important - it is the only one where the build
contradicts a *normative* data model.

## Priority table

| ID | Facet | Type | What's wrong | Effort | Recommended resolution | Raised by |
| --- | --- | --- | --- | --- | --- | --- |
| **F1** (high risk) | `columnar` | build vs spec | 0023 §2 mandates **Arrow/Parquet segments**; build `columnar.rs` is a **bespoke row encoding** (`append_row`, not `append_segment`), fails 0023 §2 | **L** | Make substrate emit/read real Arrow/Parquet (Polars as native accel) | P9-0009 OQ-CO1; fidelity (high risk) |
| **F2** | `vector` | spec text stale | Captured: 0017 now matches the built **exact contract + metadata(Eq/And) filter + typed `Value` metadata** model; id-prefix scan is exposed as `ids` | **done** | Keep 0017 and P9-0012 aligned with the source-backed local facade | P9-0012 OQ-VE1 |
| **F3** | `kv` | spec <-> build | 0019 facade is **bytes-key + prefix scan**; build is **typed `Value` key + `range(lo,hi)`** | **S-M** | Promote typed-key + range into 0019 (the ordering is the feature) | P9-0007 OQ-K1 |
| **F4** | `time-series` | spec <-> build | 0022 facade `range` says **inclusive**; build is **half-open `[from,to)`** | **S** | Amend 0022 to half-open (matches build + `kv`/`queue` convention) | P9-0010 OQ-TS1 |
| **F5** | `cas` | build > spec | Build ships `cas_list` enumeration; 0024 facade is `put/get/has` and N2 says "no enumeration without an external index" | **S** | Promote `list` into 0024 as **workspace-scoped** enumeration | P9-0005 OQ-C1 |
| **F6** | `sql` | naming split | Captured: wire surfaces use `sql`; `Db` remains an in-engine or IDL alias where useful | **done** | Keep companion binding docs aligned with the `sql` wire name | P9-0006 P9-RD-S1 |
| **F7** | REST (all) | editorial | Captured: 0008 §3.1 now uses canonical `/v1/workspaces/{workspace_id}/...` paths | **done** | Keep companion binding docs aligned with the canonical UUID path | P9-0001 OQ4 / P9-0002 §8 |
| **F8** | `document` | spec ahead of build | `find` requires **declared secondary indexes** (0020 §5) that **don't exist**; no index-declaration API | **M** | Add `create_index`/`drop_index` to 0020 + build index machinery | P9-0008 OQ-D1 |
| **F9** | `queue` | spec ahead of build | Facade `dequeue(consumer)` (consumer refs) but build **stores no consumer offsets**; ordering substrate remains owned by 0021 | **M** | Model consumers as workspace refs; pick seq-keyed prolly-tree substrate | P9-0011 OQ-Q1/Q2 |
| **F10** | `sql` | target facade split | Captured: current source includes GlueSQL DDL/DML/query, alter-table, C ABI sessions/batches, direct readers, blame, diff, result views, and selected binding projections; generated SQL facade, hosted protocols, full `as_of`, schema-change diff, stable SQL errors, and foreign wire adapters are tracked by 0011a | **done** | Keep current source boundary in 0011 and target facade/conformance work in 0011a | P9-0006 §1, OQ-S3 |
| **F11** | `files` / `vcs` | partial build | `files.append_file`, **random/offset writes**, + **symlink ops** unbuilt (`fs` is whole-file replace + byte-range *read* only; `Symlink` EntryKind has no ops); `vcs.cherry_pick`/`revert` build status **unverified**. **Blocks the read-write FUSE tip mount (P9-0017).** | **S-M** | Implement `append_file` + offset/random writes; implement symlink ops; confirm cherry_pick/revert | P9-0003 OQ-F2; P9-0004 §1; P9-0017 §4 |
| **F13** | identity / acl (0026/0027) | spec collision | Captured: 0026 projects as `Identity`; 0027 projects as `Acl` | **done** | Keep generated IDL/proto surfaces distinct | CP-0004 / CP-0005 OQ-I1 |
| **F12** | conformance (0025) | doc overstates reality | `IMPLEMENTATION-PLAN.md` marks 0025 **"built"**, but `loom-conformance` only runs the framework + digest/object vectors + the `cas` behavioral suite (~4 `#[test]` total); the other facets' suites (`FS_SCENARIOS`...`COLUMNAR_SCENARIOS`) are **inert scenario data** awaiting their facades, per the crate's own comment | **S** (doc) / **L** (to wire) | Correct the plan's status to "framework + digest vectors + cas behavioral; per-facet behavioral suites pending facade builds"; stand up executed suites as each facet lands | this turn's `loom-conformance` inspection |

Effort key: **S** = spec edit / small code - **M** = moderate build - **L** = significant build.

---

## Detail

### Group A - spec-text reconciliations (decide canonical truth, edit spec)

**F2 - `vector` facade text is aligned.** 0017 and P9-0012 now describe the shipped local facade:
create/upsert/get/ids/delete/exact-search over named vector sets, fixed dim+metric, typed metadata,
and `MetaFilter::All|Eq|And`. Hosted protocol projection remains target work.

**F3 - `kv` key model.** 0019 §4 facade: `get/put/delete(key: bytes)` + `scan(prefix)`. Build:
`BTreeMap<Value, _>` typed keys + `range(lo,hi)`. *Resolution (recommended):* promote the **typed-key +
range** model into 0019 and define a deterministic wire key-encoding - the ordering is what makes `kv`
etcd-like (see fidelity doc). Alternative: standardize on bytes+prefix and drop typed ordering (loses the
feature). Pick one before the `kv` binding is normative.

**F4 - `time-series` range bound.** 0022 §4 says `range` is an inclusive window; build returns half-open
`[from,to)`. *Resolution:* amend 0022 to **half-open** (matches build and the `kv`/`queue` range
convention; avoids boundary double-counting).

**F5 - `cas` enumeration.** Build has `cas_list`; 0024 facade is `put/get/has` and N2 forbids enumeration
"without an external index." But the `cas` workspace **is** a reachable-digest manifest (0024 resolved
decision 1), so workspace-scoped enumeration is well-defined. *Resolution:* add `list` to the 0024 facade
as explicitly **workspace-scoped** (not a global store walk); update N2 accordingly.

**F6 - `sql` vs `db` name.** Captured. The relational facet uses **`sql`** on the wire (REST root `/sql`,
JSON-RPC `sql.*`, MCP `sql.*`) to match the workspace/capability and the single-name convention. `Db`
remains an in-engine or IDL alias where useful.

**F7 - workspace segment in 0008.** Captured. 0008 §3.1 now uses canonical
`/v1/workspaces/{workspace_id}/...` paths. Companion binding docs should keep using that
UUID path shape, with alias lookup routes treated only as service conveniences.

### Group B - build lifts (make implementation meet spec)

**F1 - `columnar` is not Arrow/Parquet ((high risk) the important one).** 0023 §2 is **normative**: a dataset *is*
"a set of Parquet/Arrow segments," and §5 requires the format "stays Arrow/Parquet-compatible." The build
(`columnar.rs`) stores a Loom Canonical CBOR row encoding (ADR-0010, P3) via `append_row`, drops segment
boundaries from identity, and emits no Arrow/Parquet - it would **fail a 0023 §2 conformance check**, and it blocks
the entire Arrow Flight / Parquet / DuckDB interop story (P9-0009). *Resolution:* replace the substrate so
`append_segment`/`scan` read/write real Arrow/Parquet (the pure-Rust default the spec assumes), Polars as
the native accelerator (ADR-0008). Stopgap (transcode-at-boundary) leaves the conformance gap; amending
0023 to bless a bespoke format abandons ecosystem interop. **This is the one build<->normative-spec
contradiction; treat as highest priority among the build items.**

**F8 - `document` indexes/`find`.** 0020 facade exposes `find(field, value)` "by declared index," but no
index machinery or declaration API exists; the wire `find` is dead until it does. *Resolution:* add
`create_index`/`drop_index` to the 0020 facade and build the back-index pass (0020 §5 hints at it); or, per
0020 §6, push field querying onto `sql`. Decide before `document.find` is advertised.

**F9 - `queue` consumer offsets + substrate.** Facade `dequeue(stream, consumer)` advances a consumer ref,
but the build stores **no** consumer offsets (caller-tracked), so `dequeue` is effectively unbuilt; and
0021 still owns the ordering substrate decision (commit-log vs seq-keyed prolly tree), which changes merge/
offset semantics under sync. *Resolution:* model consumers as **workspace refs** (versioned, syncable
positions) backing `dequeue`/Kafka offset-commit, and adopt the **seq-keyed prolly-tree** substrate.

**F10 - `sql` target facade split.** Captured. Current source includes GlueSQL DDL/DML/query,
alter-table, C ABI sessions/batches, direct table read, index scan, table blame, table diff, result
views, and selected binding projections. 0011a owns generated facade parity, hosted protocols, full
`as_of`, schema-change diff records, stable SQL error extensions, and foreign wire adapters.

**F11 - `files`/`vcs` gaps (blocks read-write FUSE).** The `fs` facade today is **whole-file replace +
byte-range *read***; it lacks `append_file` (in the 0008 §3.2 mapping) and **random/offset writes**, and
**symlink operations** are unbuilt (the `Symlink` `EntryKind` exists but has no ops). The **read-write FUSE
tip mount (P9-0017 §4) depends on all three** - FUSE issues partial writes at arbitrary offsets and real
tools (`git`, build systems) use symlinks; the **read-only** FUSE modes (commit mount, tip `-o ro`) work
against the facade as built. Separately, `vcs.cherry_pick`/`revert` build status is **unverified** (the
inventory was inconsistent). *Resolution:* implement `append_file` + offset/random writes (a stateful
open/seek/write path); implement symlink ops; **verify** cherry_pick/revert against `loom-core::vcs` and
either confirm or mark unbuilt in 0003 §5.

### Group C - process / doc accuracy

**F12 - `IMPLEMENTATION-PLAN.md` overstates 0025 conformance.** The plan's "Current build state" table marks
"canonical digest vectors + behavioral suites (0025)" as **built**. Inspection of `crates/loom-conformance`
shows: the framework, the canonical digest/object vectors, and the **`cas`** behavioral suite run, but
`behavior.rs` carries every other facet's suite (`FS_SCENARIOS`, `VCS_SCENARIOS`, ... `COLUMNAR_SCENARIOS`)
as **inert `Scenario` data** with only ~4 `#[test]`s total - the file's own header says the facade traits
"are not yet built ... so their scenarios are carried here as data and become executable runners" once the
facets exist, and the one meta-test merely asserts the tables are non-empty. *Resolution:* reword the plan
to "framework + digest vectors + `cas` behavioral built; per-facet behavioral suites are scenario data
pending facade builds," and **wire each facet's suite to its facade as it lands** - this is the mechanism
that would have caught F2/F3/F4/F5 automatically.

**F13 - `Identity` and `Acl` facade names.** Captured. 0026 sketches `interface Identity`, and 0027
sketches `interface Acl`. Generated IDL/proto surfaces must keep authentication and authorization
distinct.
