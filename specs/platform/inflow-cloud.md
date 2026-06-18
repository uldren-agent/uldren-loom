# InFlow Cloud AWS Storage Backend

**Status:** Draft | **Version:** 0.1.0-draft | **Normative?** Informative architecture note

This note explores an AWS storage backend for an Uldren-operated cloud daemon written in Rust. It
assumes the service receives Loom sync data such as encrypted frames, Merkle tree nodes, bundles, ref
labels, and sync metadata. It focuses on the normal AWS shape that scales with stored bytes and request
volume.

## 1. Recommendation

Use **Amazon S3 for immutable content-addressed data** and **Amazon DynamoDB for mutable metadata and
compare-and-swap state**.

S3 is the natural fit for Merkle nodes, chunks, sealed frames, bundles, and pack files. The Loom data
model produces immutable objects addressed by digest, and S3 is a durable object store with strong
read-after-write consistency for object writes, deletes, and metadata reads. S3 also scales by prefix,
so digest-sharded keys can spread write and read traffic proportionally.

DynamoDB is the natural fit for branch heads, refs, sync cursors, leases, account state, billing
counters, object indexes, and idempotency records. DynamoDB on-demand mode is serverless,
pay-per-request, and scales without capacity planning for most workloads. DynamoDB condition
expressions and transactions cover Loom's small mutable state transitions.

## 2. Backend shape

```text
loom-cloud daemon
  |
  | immutable object writes and reads
  v
S3 bucket
  objects/{tenant}/{loom}/{algo}/{prefix}/{digest}
  packs/{tenant}/{loom}/{pack_id}
  bundles/{tenant}/{loom}/{bundle_id}

  |
  | conditional ref updates and metadata writes
  v
DynamoDB
  Looms
  Refs
  SyncSessions
  ObjectIndex optional
```

The daemon owns Loom protocol validation, tenancy, authorization, request shaping, retries,
observability, and billing events. S3 and DynamoDB are storage substrates, not product boundaries.

## 3. S3 object layout

Store immutable bytes in S3 under digest-derived keys:

```text
objects/{tenant_id}/{loom_id}/{algo}/{digest[0..2]}/{digest[2..4]}/{digest}
```

The first digest bytes create many prefixes without introducing lookup metadata. That matters because
AWS documents S3 scaling per partitioned prefix, with at least 3,500 write-class requests or 5,500
read-class requests per second per prefix, and no limit on the number of prefixes in a bucket. The
daemon can increase parallelism by using more prefixes as traffic grows.

For small objects, store one Loom object per S3 object. For high-ingest workloads with many tiny Merkle
nodes, add pack files:

```text
packs/{tenant_id}/{loom_id}/{pack_id}
pack-index/{tenant_id}/{loom_id}/{digest}
```

The pack file reduces request count and cost. The pack index can live in DynamoDB if lookup latency is
important, or as S3 sidecar data if lookup is mostly sequential during sync.

## 4. DynamoDB tables

A conservative first design can use a small set of tables:

| Table | Key | Purpose |
| --- | --- | --- |
| `Looms` | `tenant_id`, `loom_id` | Loom metadata, plan, retention, region, encryption mode, created time |
| `Refs` | `loom_id`, `workspace_id#ref_name` | Current ref tip, generation, last writer, updated time |
| `SyncSessions` | `tenant_id`, `session_id` | resumable sync state, idempotency, expiry |
| `ObjectIndex` | `loom_id`, `digest` | optional presence, size, S3 key, pack locator, retention class |

Do not store object bodies in DynamoDB. DynamoDB items are limited to 400 KB, and Loom object bytes can
be larger than that. DynamoDB should hold small metadata and conditional state only.

## 5. Write paths

### 5.1 Immutable object write

1. The daemon receives a frame labeled by digest.
2. The daemon validates request shape and tenant authorization.
3. The daemon writes to S3 using a digest key.
4. The write uses an S3 conditional write to avoid overwriting an existing key.
5. The daemon optionally records metadata in `ObjectIndex`.

S3 conditional writes are useful because content-addressed objects are immutable. If the key already
exists, the daemon can treat that as deduplication after confirming the object metadata matches the
expected digest, size, and frame profile.

### 5.2 Ref update

Use DynamoDB condition expressions for Loom ref compare-and-swap:

```text
update Refs
set tip = new_digest, generation = generation + 1
where loom_id = ...
  and ref_key = ...
  and tip = expected_old_digest
```

This mirrors Loom's ref CAS rule. A failed condition is a normal sync conflict, not infrastructure
failure.

### 5.3 Multi-item update

Use DynamoDB transactions only for small metadata changes that must commit together, such as creating a
Loom record plus its initial refs. DynamoDB transactions support all-or-nothing writes, but they are
bounded by item count and aggregate size. The object bytes should already be in S3 before ref
advancement.

## 6. Read paths

Object reads are S3 `GET` or `HEAD` by digest key. Ref reads are DynamoDB strongly consistent reads
when the caller needs the latest tip. Listing and negotiation should prefer explicit refs, sync cursors,
and object indexes over scanning S3 prefixes.

The service should avoid making S3 list operations part of correctness. S3 can list at scale, but Loom
already has digest labels and refs. Correctness should come from known object keys, ref tips, and
client-supplied have/want negotiation.

## 7. Scaling model

This backend scales proportionally along the same axes as the product:

- Storage cost scales with S3 bytes stored.
- Object request cost scales with S3 PUT, GET, and HEAD volume.
- Metadata cost scales with DynamoDB read and write request units.
- Hot ref contention is isolated to the specific ref item in DynamoDB.
- Object ingest can scale horizontally through digest-sharded S3 prefixes.

The main hotspots to watch are not Merkle objects. They are branch heads, sync cursors, account-level
quota counters, and pack-index rows. Those need explicit per-tenant rate limits, idempotency keys, and
backoff.

## 8. Storage class choices

Start with S3 Standard for active sync data. Add lifecycle transitions later for cold unreachable
objects, old bundles, or retention snapshots.

S3 Express One Zone is a possible hot-path optimization if object latency becomes the bottleneck. AWS
positions it for single-digit millisecond access and high request rates, but it is a single-zone
storage class. It is not the default backup tier for a zero-knowledge durability product.

DynamoDB on-demand is the recommended first capacity mode. Move a mature, predictable workload to
provisioned mode only when cost modeling shows stable traffic and enough operational benefit.

## 9. Zero-knowledge boundary

For `loom-cloud`, AWS server-side encryption is not the product confidentiality boundary. It is
infrastructure hardening. The zero-knowledge boundary is Loom's client-side sealing: the daemon stores
frames and labels without receiving the customer content key.

That means:

- AWS KMS can encrypt S3 buckets and DynamoDB tables at rest for infrastructure defense.
- AWS KMS should not hold the customer Loom content key if the product promise is that Uldren cannot
  read the data.
- `loom-cloud` should not add S3 Object Lambda, Athena, OpenSearch, Lambda processors, or other
  plaintext data-processing paths over customer content.
- Any metadata analysis should be explicitly limited to account metadata, billing metadata, request
  metrics, object sizes, object counts, timing, and opaque labels.

## 10. Alternatives rejected for v1

| Option | Why not first |
| --- | --- |
| DynamoDB-only object store | 400 KB item limit, higher cost for binary payloads, worse fit for large immutable data |
| S3-only state store | S3 can store refs, but DynamoDB is better for conditional updates, leases, idempotency, and hot mutable metadata |
| Aurora or PostgreSQL | Good for relational product metadata, but unnecessary as the primary CAS backend and less proportional for object volume |
| EFS or FSx | Filesystem semantics are unnecessary for digest-addressed immutable objects and add operational coupling |
| S3 Tables or S3 Vectors | Useful for analytics or vector workloads, but `loom-cloud` is not a plaintext compute surface |

## 11. Source checks

- AWS S3 overview: S3 is object storage for any amount of data, with strong read-after-write
  consistency for object writes, deletes, and metadata reads:
  https://docs.aws.amazon.com/AmazonS3/latest/userguide/Welcome.html
- AWS S3 performance guidance: S3 scales by prefix and documents baseline request rates per prefix:
  https://docs.aws.amazon.com/AmazonS3/latest/userguide/optimizing-performance.html
- AWS S3 conditional requests: conditional writes can prevent overwrites or check ETags before
  updates:
  https://docs.aws.amazon.com/AmazonS3/latest/userguide/conditional-requests.html
- AWS DynamoDB capacity modes: on-demand mode is serverless, pay-per-request, and scales without
  capacity planning:
  https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/capacity-mode.html
- AWS DynamoDB condition expressions: conditional puts and updates fail when their condition does not
  hold:
  https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/Expressions.ConditionExpressions.html
- AWS DynamoDB constraints: item size is 400 KB, transactions are bounded by item count and aggregate
  size:
  https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/Constraints.html
