# Facet Market Fit

This file captures the relative market-fit ranking used to prioritize Queue 2 facet work. Scores are
0-10 planning estimates relative to the current Queue 2 backlog, not implementation status.

## Source Anchors

- DB-Engines ranking: https://db-engines.com/en/ranking
- OpenSearch overview: https://opensearch.org/docs/latest/about/
- Vector database market context: https://www.itpro.com/infrastructure/database-management/what-is-a-vector-database
- Queue source: [`../_QUEUE2.md`](../_QUEUE2.md)

## Ranking Table

| Overall rank | Task # | Technology / task | Overall | Market importance | ROI | Profitability | Market usage | Strategic foundation | Implementation readiness | Rationale |
| ---: | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 1 | 290 | Columnar Arrow/Parquet base plus analytical presentations | 10.0 | 10 | 9 | 10 | 10 | 10 | 5 | Highest foundation value. Arrow/Parquet unlocks data lakes, analytics, BI, ML pipelines, DuckDB-like local use, Snowflake-like hosted use, Spark-like batch use, and BigQuery-like job use. Current storage gap blocks much of the Tier-2 ecosystem. |
| 2 | 340 | SQL wire and client ecosystem presentations | 9.5 | 10 | 9 | 10 | 10 | 9 | 6 | SQL is the dominant enterprise data access contract. A PostgreSQL or MySQL-compatible profile has immediate client and BI value, but the row SQL base is already partly present, so the missing lift is wire compatibility and correctness. |
| 3 | 370 | Object, blob, and archive presentation families | 9.2 | 10 | 9 | 9 | 10 | 9 | 6 | S3, OCI, CAR, archive, blob, and file-adjacent workflows are critical for storage adoption, migration, backups, model artifacts, and cloud interoperability. |
| 4 | 350 | KV and document presentation families | 8.8 | 9 | 8 | 9 | 9 | 8 | 6 | Redis, Memcache, native document indexes/query, MongoDB-like, Couchbase-class, and etcd-class shapes map to common application storage needs and can create visible compatibility wins. CouchDB is no longer active queue work. |
| 5 | 280 | OpenSearch-compatible search served surface backed by Tantivy | 8.5 | 9 | 8 | 8 | 9 | 8 | 7 | Search has strong product value and a clear compatibility target. OpenSearch-compatible query and aggregation coverage would make Loom useful to existing dashboards and search clients. |
| 6 | 300 | Vector Qdrant-shaped served adapter | 8.4 | 9 | 8 | 8 | 8 | 8 | 7 | Vector search is strategically important for AI workloads. Qdrant-shaped compatibility is valuable, but market usage is narrower than SQL, object storage, and general columnar analytics. |
| 7 | 360 | Queue and time-series presentation families | 8.1 | 8 | 8 | 8 | 8 | 8 | 5 | Kafka-style, NATS or AMQP candidates, Influx-compatible pieces, Prometheus-adjacent pieces, and Grafana visibility have strong operational value, but protocol correctness and semantics are broad. |
| 8 | 320 | Hosted conformance, drift coverage, and capability reporting | 8.0 | 7 | 8 | 8 | 7 | 10 | 7 | This is not a marketable feature by itself, but it is necessary to prevent compatibility claims from becoming untrustworthy as the surface area grows. |
| 9 | 310 | Non-CAS gRPC services | 7.9 | 8 | 7 | 8 | 8 | 8 | 5 | gRPC matters for service ecosystems and generated clients. It should follow stable native and presentation semantics rather than lead them. |
| 10 | 330 | Device, browser, provider, and package certification | 7.5 | 7 | 7 | 8 | 7 | 9 | 6 | Certification increases adoption confidence and reduces release risk, especially after hosted and binding surfaces expand. |
| 11 | 400 | Close facet and facet-binding specs from executable evidence | 7.2 | 6 | 7 | 7 | 6 | 10 | 8 | Necessary to close planning debt and make public claims reliable, but it depends on implementation evidence from earlier tasks. |
| 12 | 390 | FUSE mount behavior and certification | 6.9 | 6 | 7 | 6 | 7 | 7 | 6 | Important for local developer ergonomics and filesystem projection, but less commercially central than hosted data compatibility surfaces. |
| 13 | 380 | Graph and ledger presentation families | 6.4 | 6 | 6 | 6 | 5 | 7 | 4 | Graph and ledger are strategically useful, but the graph grammar decision remains unresolved and market usage is narrower than SQL, object storage, columnar, KV, search, and vector. |

## Document Store Compatibility Split

The combined Queue 2 `350` score is not a directive to build every document database presentation.
The current decision is to keep the reusable native document primitives as the active foundation and
move MongoDB/Couchbase product ports to P3/spec-owned work until their product compatibility scope is
separately reviewed.

| Candidate | Overall | Market importance | ROI | Market usage | Strategic foundation | Implementation readiness | Complexity | Active direction |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| Native document indexes/query | 9 | 8 | 9 | 8 | 10 | 8 | 7 | Source-backed foundation for `document.find`, native hosted query, MCP, CLI, bindings, and later product compatibility. |
| MongoDB-compatible surface | 8 | 10 | 8 | 10 | 8 | 4 | 9 | P3/spec-owned. Keep planned now that the native foundation exists, but require a compatibility matrix and product-wire scope before build. |
| Couchbase-compatible surface | 7 | 7 | 7 | 7 | 8 | 3 | 10 | P3/spec-owned. Requires integrated document, KV, query, and analytics design before build. |
| CouchDB-compatible surface | 3 | 3 | 3 | 3 | 4 | 3 | 8 | Cut from active Queue 2. Reconsider only if revision trees, conflicts, `_changes`, and replication become strategic. |

## Recommended Priority Reading

- If optimizing for enterprise foundation, Task 290 should lead because Arrow/Parquet is the data-model
  unlock for analytics, BI, ML, and multiple presentation families.
- If optimizing for the smallest visible compatibility win, Task 280 is attractive because OpenSearch
  compatibility has a narrower but clearer served-surface target.
- If optimizing for broad client adoption after the base data layers are stable, Task 340 and Task 370
  should follow closely.
