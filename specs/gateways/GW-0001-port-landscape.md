# GW-0001 — Port Landscape (data-/file-store protocols)

**Series:** Gateways (exploratory) · **Version:** 0.1.0-draft · **Status:** Draft · **Updated:** 2026-06-18
**Reads first:** [`GW-0000-index.md`](./GW-0000-index.md). Implementation lift/libraries are in
[`GW-0002-implementation-lift.md`](./GW-0002-implementation-lift.md).

Curated IANA-registered and de-facto ports whose protocol has a **data-store / file-store aspect** Loom
could gateway. **Mode:** **B** = Loom backs/terminates · **T** = Loom taps (record → filter → forward).
**→** = where the data lands in Loom (facet/workspace).

> **Source-backed reconciliation (2026-07-18):** B-mode adapters for **OpenSearch (9200), S3 (9000),
> PostgreSQL (5432), MySQL (3306), Redis (6379), Memcached (11211), IMAP (143/993), and JMAP** already
> ship as bounded `loom-hosted` wire adapters — this landscape table is *not* their status of record.
> Authoritative per-facade status is [`../0008-wire-protocols.md`](../0008-wire-protocols.md), the owning
> facet specs, and the MX-124 compatibility matrix (rows marked **[B: shipped]** below). The novel
> unbuilt capability remains the **T-mode** taps.

## Mail — the flagship tap

| Port | Protocol | Store aspect | Mode | → facet |
|---|---|---|---|---|
| 25, 465, 587 | SMTP / submission (465 = implicit-TLS submission) | message relay + store | **T** (record→LLM/heuristic filter→forward) or **B** (final mailstore) | document + search (+ ledger audit) |
| 143 / 993, 110 / 995 | IMAP **[B: shipped]** / POP3 (+TLS) | mailbox store | B or T | mail (+ search) |

JMAP (RFC 8620/8621, over HTTP) is also **[B: shipped]** as a bounded `loom-hosted` adapter (mail facet); it has no dedicated legacy port row.

## File transfer & network filesystems

| Port | Protocol | Store aspect | Mode | → facet |
|---|---|---|---|---|
| 21/20, 989/990 | FTP / FTPS | file store | B / T | files |
| 22 | SSH / SFTP / SCP | file transfer | T (record) / B (serve) | files |
| 115 | SFTP (legacy, RFC 913) | file transfer | B / T | files |
| 69 | TFTP | boot/config file fetch | T | files |
| 873 | rsync | file sync | T / B | files + vcs |
| 445, 137–139 | SMB/CIFS, NetBIOS | network share | B / T | files |
| 2049 (+111) | NFS (+rpcbind) | network share | B / T | files |
| 548 | AFP | Apple share | B | files |
| 80/443 | WebDAV | files over HTTP | B | files |

## Version control

| Port | Protocol | Store aspect | Mode | → facet |
|---|---|---|---|---|
| 9418 | git protocol | versioned file store | T (mirror; git *backing* excluded, 0012) | vcs + files |
| 3690 | svnserve | versioned file store | T | vcs + files |

## Object / content-addressed

| Port | Protocol | Store aspect | Mode | → facet |
|---|---|---|---|---|
| 9000 (+443) | S3 API (MinIO/AWS) | object store | **B: shipped** (loom-hosted) / T | cas + files |
| 4001/5001/8080 | IPFS | content-addressed | B / T | cas |
| 5000/443 | OCI registry | content-addressed blobs | B / T | cas |

## Directory / identity / naming — strong tap candidates

| Port | Protocol | Store aspect | Mode | → facet |
|---|---|---|---|---|
| 389, 636 | LDAP / LDAPS | directory store | B / **T** (record lookups) | document / kv / graph (+ identity) |
| 53, 853, 5353 | DNS / DoT / mDNS | record store (KV) | **T** (sinkhole/audit/filter→forward) | kv / document (+ ledger) |
| 42 | WINS / host-name server | name store | T | kv |
| 88 | Kerberos | tickets / credentials | T (audit) | identity / acl |

## Config / lease / management

| Port | Protocol | Store aspect | Mode | → facet |
|---|---|---|---|---|
| 67/68 (547/546) | DHCP(v6) | lease store | **T** (record leases) | kv / document (+ time-series) |
| 161/162 | SNMP / trap | MIB data store + metrics | **T** (poll / record) | time-series + ledger |
| 123 | NTP | time source | T (audit; weak store aspect) | ledger (audit) |
| 623 | IPMI | hardware telemetry | T | time-series |

## Logs / telemetry — append-only, maps to ledger / queue / time-series

| Port | Protocol | Store aspect | Mode | → facet |
|---|---|---|---|---|
| 514, 601, 6514 | syslog (UDP/TCP/TLS) | **log store** | **T / B** (ingest→filter→store/forward) | ledger / queue / search |
| 4317/4318 | OTLP (OpenTelemetry) | traces / metrics / logs | B / T | time-series + ledger + search |
| 24224 | Fluentd forward | log forwarding | T | ledger / search |
| 2003, 8125 | Graphite / StatsD | metrics | B / T | time-series |
| 1883 / 8883 | MQTT (+TLS) | telemetry / messages | B / T | queue + time-series |

## Databases & engines (mostly P9 Tier-2 overlap; tap = query/change audit)

| Port | Protocol | → facet |
|---|---|---|
| 3306 / 5432 / 1433 / 1521 | MySQL **[B: shipped]** / PostgreSQL **[B: shipped]** / MSSQL / Oracle | sql |
| 27017 / 5984 | MongoDB / CouchDB | document |
| 6379 / 11211 / 2379 | Redis **[B: shipped]** / Memcached **[B: shipped]** / etcd | kv |
| 9092 / 5672 / 4222 | Kafka / AMQP / NATS | queue |
| 9200 / 8983 / 7700 / 8108 | Elasticsearch-OpenSearch **[B: shipped]** / Solr / Meilisearch / Typesense | search |
| 7687 / 8182 | Neo4j Bolt / Gremlin | graph |
| 8086 / 8123 | InfluxDB / ClickHouse | time-series / columnar |
| 6333 / 19530 | Qdrant / Milvus | vector |
| 3322 | immudb | ledger |

## Block storage (advanced)

| Port | Protocol | Mode | → facet |
|---|---|---|---|
| 3260, 860 | iSCSI | B / T | files / cas (block-level) |

## Observations

- The infrastructure protocols (DNS 53, DHCP 67/68, SNMP 161/162, LDAP 389/636, NTP 123, SSH 22, SMTP
  465) are almost all **tap-mode** candidates — services that *hold or move* data and gain audit/filtering
  value from an intercepting recorder. Richest taps beyond email: **DNS, DHCP, SNMP, LDAP, syslog** — each
  becomes a versioned, queryable, filterable workspace.
- The **append-only / telemetry** group (syslog, OTLP, MQTT, SNMP traps, NTP/Kerberos audit) maps almost
  1:1 onto Loom's `ledger` / `queue` / `time-series` facets — the cleanest first vertical, with email
  (465) as the marquee "filter-and-forward" demo.
