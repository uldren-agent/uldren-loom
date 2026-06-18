# GW-0002 — Implementation Lift & Rust Libraries

**Series:** Gateways (exploratory) · **Version:** 0.1.0-draft · **Status:** Draft · **Updated:** 2026-06-18
**Reads first:** [`GW-0001-port-landscape.md`](./GW-0001-port-landscape.md) (the protocols + modes this
costs out).

Per-protocol estimate of how hard a Loom gateway is to build, and the Rust crates that do the heavy lifting
(or where it's custom).

**Assumed shared infra** (every gateway needs these; not repeated per row): `tokio` (async runtime),
`tokio-rustls`/`rustls` (TLS termination — required for the implicit-TLS variants 465/636/993/995/853/990/
8883), `axum`/`hyper` (HTTP-based gateways), `tonic` (gRPC-based gateways), `serde`/`serde_json`.

**Lift:** **S** ≈ days · **M** ≈ weeks · **L** ≈ significant (no mature crate, complex/stateful protocol,
or crypto-heavy).

**Crate confidence:** unmarked crates are verified to exist and fit; crates marked **\*** are indicative —
confirm at build. Where a row says "custom," no mature server-side crate exists today.

> **Source-backed reconciliation (2026-07-18):** rows tagged **[shipped]** already exist as bounded
> `loom-hosted` / `loom-hosted-pim` wire adapters (see [`../0008-wire-protocols.md`](../0008-wire-protocols.md)
> and the MX-124 compatibility matrix); their lift estimates are historical. Notably several "no mature
> server crate → custom" notes were superseded by bounded custom servers that were actually built. The
> remaining estimates apply to unbuilt gateways (mostly T-mode taps and not-yet-built B-mode adapters).

## Mail

| Protocol (ports) | Lift | Rust crates / custom |
|---|---|---|
| SMTP / submission (25, 465, 587) | **M** | `samotop` or `mailin-embedded` (server) · `mail-parser` (MIME) · `lettre` (forward/client) |
| IMAP / POP3 (143/993, 110/995) | **L** back · M proxy · **[IMAP shipped]** | `async-imap`/`imap` (client); no mature *server* crate → bounded custom server SHIPPED in `loom-hosted-pim` (IMAP4rev2/RFC 9051 subset). JMAP (RFC 8620/8621) also shipped over HTTP. |

## File transfer & network filesystems

| Protocol (ports) | Lift | Rust crates / custom |
|---|---|---|
| FTP / FTPS (21/20, 990) | **M** | `libunftp` (+ a Loom storage backend) |
| SSH / SFTP / SCP (22) | **M–L** | `russh` + `russh-sftp` (server) |
| TFTP (69) | **S** | `async-tftp`\* or custom (UDP, tiny) |
| rsync (873) | **L** | no mature crate → custom (protocol underdocumented) |
| SMB/CIFS (445) | **L** | no mature server crate → custom / FFI |
| NFS (2049) | **M–L** | `nfsserve` / `nfs3_server` (+ a Loom VFS backend) |
| WebDAV (80/443) | **M** | `dav-server` (pluggable filesystem → Loom) |
| AFP (548) | **L** | none → custom (legacy; likely skip) |

## Version control

| Protocol (ports) | Lift | Rust crates / custom |
|---|---|---|
| git (9418) | **M** (mirror/tap; backing excluded, 0012) | `gix` / `git2` |
| svn (3690) | **L** | no good crate → custom |

## Object / content-addressed

| Protocol (ports) | Lift | Rust crates / custom |
|---|---|---|
| S3 (9000/443) | **M** · **[shipped]** | shipped as a custom axum router in `loom-hosted` (not `s3s`) + CAS/files backend, SigV4 |
| OCI registry (5000/443) | **M** | `oci-distribution` (client) + `axum` server |
| IPFS (4001/5001) | **M–L** | `iroh` or `rust-ipfs`\* |

## Directory / naming — tap candidates

| Protocol (ports) | Lift | Rust crates / custom |
|---|---|---|
| **DNS (53, 853, 5353)** | **S–M** | `hickory-server` (authoritative + forwarder + DoT/DoH) — only the record-to-Loom + filter hook is custom |
| LDAP (389/636) | **M** tap · M–L back | `ldap3` (client) · `ldap3_proto`\* (protocol/server scaffolding) |
| Kerberos (88) | **L** (audit-tap M) | `cross-krb5`\* / `sspi`\* |
| WINS (42) | **L** | none → custom (niche; likely skip) |

## Config / management — tap candidates

| Protocol (ports) | Lift | Rust crates / custom |
|---|---|---|
| DHCP (67/68) | **M** | `dhcproto` (parse) + custom relay/record |
| SNMP (161/162) | **M** | `csnmp`\* / `snmp-parser`\* + custom poller / trap-sink |
| NTP (123) | **S–M** (audit) | `sntpc`\* / `ntp`\* or custom (simple UDP) |
| IPMI (623) | **L** | none → custom (niche) |

## Logs / telemetry — the cleanest first vertical

| Protocol (ports) | Lift | Rust crates / custom |
|---|---|---|
| **syslog (514, 601, 6514)** | **S** | `syslog_loose` (parse) + a UDP/TCP receiver → append to `ledger`/`queue` |
| Graphite / StatsD (2003, 8125) | **S** | custom (text / UDP line protocol) |
| MQTT (1883/8883) | **M** | `rumqttd` (broker) + a Loom persistence hook |
| OTLP (4317/4318) | **M** | `tonic` + `opentelemetry-proto` (gRPC) / `axum` (HTTP) |
| Fluentd forward (24224) | **M** | `rmp-serde` (MessagePack) + custom forward |

## Databases (mostly P9 Tier-2 overlap)

| Protocol (ports) | Lift | Rust crates / custom |
|---|---|---|
| Redis (6379) | **S–M** · **[shipped]** | RESP → shipped in `loom-hosted/redis.rs` (+ `loom-redis`), strings/hash/set/list/zset; streams+pubsub still target |
| MySQL / Postgres (3306 / 5432) | **M** · **[shipped]** | shipped bounded profiles: `mysql_wire.rs` (custom) / `pg_wire.rs` (`pgwire`) |
| etcd (2379) / InfluxDB (8086) | **M** | `tonic` / `axum` + line-protocol parse |
| Elasticsearch-OpenSearch (9200) | **M–L** · **[shipped]** | shipped bounded profile: `axum` + custom Query-DSL mapping in `loom-hosted/serve.rs` (full Query DSL still target) |
| MongoDB (27017) / Kafka (9092) / Neo4j Bolt (7687) | **L** | codec crates (`bson`, `kafka-protocol`\*, `bolt-proto`\*) + heavy custom (no server crate) |

## Block

| Protocol (ports) | Lift | Rust crates / custom |
|---|---|---|
| iSCSI (3260) | **L** | none → custom (complex) |

## Reading of the lifts

- **Fast-start set (S):** syslog, StatsD/Graphite, TFTP, DNS (`hickory-server` does ~90%), Redis. These
  plus the telemetry vertical (→ `ledger`/`queue`/`time-series`) are the cheapest wins.
- **Marquee demo (M):** SMTP-465 (`samotop` + `mail-parser` + `lettre`) — the filter-and-forward story,
  exercising `ai/` for the LLM filter.
- **Defer (L):** SMB, rsync, MongoDB/Kafka/Bolt, iSCSI, svn, AFP, WINS, IPMI — a protocol written largely
  from scratch; build only when a concrete use case demands it.
- **Shared pattern:** a B-mode gateway over a store protocol is "a server framework crate + a Loom storage
  backend trait" (libunftp/dav-server/s3s/nfsserve/rumqttd all expose exactly such a backend seam); a
  T-mode tap is "a parser crate + a record-to-workspace + a forward client" — so the recurring engineering
  is the Loom backend/record adapter, not the protocol codec.
