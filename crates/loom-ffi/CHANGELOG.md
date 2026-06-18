# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Namespace-level history verbs `loom_vcs_blame` / `loom_vcs_diff` (RD12).** The direct engine
  surface gains path-level history that spans every facet, not just SQL tables: `loom_vcs_blame`
  attributes each current path on a branch to the commit that last set it, and `loom_vcs_diff`
  resolves which entries changed between two commits. Both return Loom Canonical CBOR
  (`PathBlame` / `PathDiff` envelopes) built by `loom-sql`'s `result_cbor` (`path_blame_cbor` /
  `path_diff_cbor`) over the new `loom_core::vcs::Loom::blame` and existing `diff`. Mirrored across
  all eight bindings.

### Changed

- **TableReaders dissolution: `loom_table_*` renamed to `loom_sql_*`, facet argument dropped
  (breaking, RD12).** The four direct table readers move under the `sql` area and no longer take a
  `facet` parameter (they are SQL-facet-scoped by construction): `loom_read_table` ->
  `loom_sql_read_table`, `loom_index_scan` -> `loom_sql_index_scan`, `loom_table_blame` ->
  `loom_sql_blame`, `loom_table_diff` -> `loom_sql_diff` (and the matching `_async` forms). This
  separates row-level SQL history (`loom_sql_blame` / `loom_sql_diff`) from the new path-level
  namespace history (`loom_vcs_blame` / `loom_vcs_diff`). Renamed across the IDL, `loom.h`, and all
  eight language bindings (method names follow each language's convention, e.g. `sqlReadTable`,
  `sql_read_table`).

- **react-native keyed read/write.** Closes the last bindings fork: every stateless
  TurboModule op (`sqlExec`/`sqlBatch`/`sqlExecJson`/`sqlExecBytes`/`sqlCommit`) now carries an optional
  `{ passphrase | kek }` so the per-op reopen can unlock an **encrypted** store. The native layer (iOS
  `.mm` + Android Kotlin/JNI) picks the opener - a 32-byte `kek` -> `loom_sql_open_with_kek` /
  `loom_sql_batch_begin_with_kek`; else a non-empty passphrase -> `loom_sql_open_keyed` /
  `loom_sql_batch_begin_keyed`; else the plain open - via shared `openSession`/`beginBatch` (ObjC) and
  `openSessionKeyed`/`beginBatchKeyed` (C++/JNI) helpers. The public TS API stays backward-compatible: the
  key is an optional trailing `LoomKey` arg, so existing unencrypted callers are unchanged. Verified: every
  C-ABI call site compiles against `loom.h` (`cc -fsyntax-only`), brace/paren balance across all five RN
  layers; the RN codegen + iOS/Android builds are user-verified with `just test-bindings`.

- **All remaining bindings: idiomatic create + keyed/KEK open.**
  Continuing past node/python to **wasm, cpp, ios, jvm, android, react-native** (all eight bindings
  now expose the create + encrypted-open surface). The **wasm** binding (OPFS/in-memory backing model,
  not file paths) gains `LoomSql.create(path, ns, db, profile, suite?, passphrase?)`,
  `LoomSql.createWithKek(... kek)`, `LoomSql.openEncrypted(... passphrase)`, and
  `LoomSql.openWithKek(... kek)` - create routes through two new loom-store backing openers,
  `loom_over_backing_profile` (fresh, profiled, unencrypted) and `loom_over_backing_encrypted` (fresh,
  encrypted; `FileStore::with_backing_encrypted` now also takes a `digest_algo`, so FIPS-encrypted stores
  can be created over OPFS); keyed/KEK open route through the existing `loom_over_backing_unlocked`.
  Randomness uses the wasm `getrandom` (`js` feature -> `crypto.getRandomValues`). The **cpp** header-only
  wrapper adds `uldren::loom::create` / `create_with_kek` free functions and `sql::open_keyed` /
  `open_with_kek` + `batch::begin_keyed` / `begin_with_kek` static factories (private pointer ctors). The
  **ios/Swift** binding adds `Loom.create` / `createWithKek` and keyed/KEK initializers on `LoomSql` /
  `LoomSqlBatch` (passphrase as `String`, KEK as `[UInt8]`, so the two open paths don't collide). The
  **jvm/FFM** binding adds `Loom.create` / `createWithKek` and keyed (`String passphrase`) / KEK
  (`byte[] kek`) constructors on `LoomSql` / `LoomSqlBatch`, with new `MethodHandle`s for the six C calls.
  The **android** KMP binding extends the JNI shim (`uldren_loom_jni.c`) with `nativeCreate` /
  `nativeCreateWithKek` / `nativeOpenKeyed` / `nativeOpenWithKek` / `nativeBeginKeyed` /
  `nativeBeginWithKek`, plus `Loom.create` / `createWithKek` on the expect/actual object and keyed
  (`String`) / KEK (`ByteArray`) constructors on `LoomSql` / `LoomSqlBatch` across commonMain + both
  actuals. The **react-native** TurboModule adds `create` / `createWithKek` (the create surface, which
  fits its stateless one-shot model) to the codegen spec, the TS API, the iOS `.mm`, and the Android
  Kotlin module + JNI (`UldrenLoom.cpp`). Verified: loom-core/loom-store + host `cargo check` green; the
  wasm non-wasm path checks; **every C-ABI call site compiles against `loom.h`** (`cc -fsyntax-only` over
  all six new symbols). The per-toolchain builds (wasm32 / CMake / Swift / Gradle-FFM / NDK / RN codegen)
  are user-verified with `just test-bindings`. **Deliberate design fork (not in scope here):** the
  react-native *keyed read/write* path - because RN's ops are stateless one-shots (each reopens the
  loom), an encrypted store's reads/writes would need a passphrase/KEK threaded through every op
  (`sqlExecTyped`/`sqlBatch`/...), an API-shape decision tracked separately.

- **Host-supplied KEK unlock.** A caller can now wrap/unwrap the DEK
  under an externally-computed 256-bit **KEK** instead of a passphrase - the common entry point every
  external provider (OS keychain, Secure Enclave/TPM, passkey PRF, KMS/HSM) funnels through. loom-core
  gains `KeySpec::RawKek`: it bypasses the passphrase KDF (the KEK *is* the master key), records a
  non-passphrase `WrapSource`, and flows through the existing `EncryptionMeta::create`/`unlock`/`rewrap`
  and every loom-store opener unchanged. A passphrase only opens passphrase wraps; a KEK only opens
  external wraps (no cross-credential unlock). Surfaces: the CLI adds `--key-source raw-kek:file:<path>`
  / `raw-kek:fd:<n>` (a 64-hex-char 256-bit KEK); the C ABI adds `loom_create_with_kek`,
  `loom_open_with_kek`, `loom_sql_open_with_kek`, `loom_sql_batch_begin_with_kek` (32-byte KEK buffer;
  wrong length -> `INVALID_ARGUMENT`, wrong KEK -> `E2E_KEY_INVALID`). The FFI's stored credential is now a
  `KeySpec` (passphrase or KEK), so the existing keyed openers are unchanged. Verified: loom-core
  round-trip + cross-credential rejection, an FFI create/open-with-KEK round-trip, CLI grammar/hex unit
  tests, and an end-to-end CLI `init`/`put`/`get` over `raw-kek:file` (wrong KEK and passphrase-against-
  KEK-store both fail `E2E_KEY_INVALID`).

- **Versioned, multi-wrap, source-tagged `encryption_meta` descriptor.**
  `encryption_meta` was a single passphrase wrap; it is now a **v2** descriptor carrying a count-prefixed
  list of source-tagged [`WrapEntry`]s - each with a `WrapSource` (passphrase / keystore / secure-enclave
  / passkey / kms / raw-kek), the `wrap_alg`, KDF salt + wrap nonce + wrapped DEK, and an opaque
  per-source `params` blob (key id, passkey credential-id + PRF salt, KMS ARN). This **reserves the
  on-disk shape now** so Shape-B sources and multi-wrap (N sources unlocking one DEK) become additive
  later, not a format break - and so an existing store can gain a second unlock method by an
  additive metadata write. v1 (single passphrase) records still **decode** (older single-passphrase
  stores keep opening); `create` writes one passphrase entry, `unlock` tries each passphrase entry (first that
  unwraps wins). Only `EncryptionMeta::active_suite` is read by other crates, so the restructure is
  contained to `loom-core::keys`; loom-store/cli/ffi/node compile unchanged. Verified: v2 multi-wrap
  round-trip + v1-legacy decode + the existing codec/round-trip/corruption tests.

- **FIPS key-management purity: PBKDF2 + AES-256-GCM DEK-wrap.** The encrypted FIPS
  store's *key* path was not FIPS-pure: the DEK was wrapped with XChaCha20-Poly1305 under an Argon2id
  master key regardless of profile (only the object-sealing AEAD switched to AES-256-GCM). Now
  `keys.rs` selects the passphrase KDF and DEK-wrap AEAD from the identity profile and records the choice
  in `encryption_meta.wrap_alg`: `0x01` is XChaCha wrap + Argon2id (default profile, byte-for-byte
  unchanged), new `0x02` is AES-256-GCM wrap + PBKDF2-HMAC-SHA-256 (FIPS). A FIPS store now has no
  Argon2id/XChaCha/BLAKE3 anywhere in its cryptographic path. `unlock`/`rekey` recover the pairing from
  `wrap_alg`. Because the wrap algorithm and KDF are fixed at creation, this had to land before encrypted
  FIPS stores ship widely (changing it later is a rekey migration). Verified: keys-layer tests
  (`fips_profile_uses_aes_wrap_and_pbkdf2`, `pbkdf2_kdf_is_deterministic_and_salt_sensitive`) + an
  encrypted store round-trip.

- **Node + Python bindings: idiomatic profile choice + encryption.** Both
  scripting bindings now expose `createLoom(path, profile, suite?, passphrase?)` / `create_loom(...)`
  (choose `default`/`fips`, optionally encrypt) and an encrypted-open path - 
  `LoomSql.openEncrypted(path, ns, db, passphrase)` / `LoomSql.open_encrypted(...)` and the matching
  batch form. The passphrase is held for the session/batch lifetime and threaded through every reopen
  (exec/query/commit/async/batch), so an encrypted store is fully usable, not half-open. A wrong
  passphrase surfaces `E2E_KEY_INVALID`, an unkeyed open of an encrypted store `E2E_LOCKED`; no
  environment variable is read. Both compile clean (node `cargo check`; python `cargo check` + clippy).
  (Continued by the wasm + cpp/ios/jvm entry above; android KMP + react-native TurboModule still remain.)

- **Binding ABI to create a store under an identity profile and open it encrypted.**
  The C ABI now lets a binding choose **default vs FIPS** and set up at-rest encryption without
  shelling out to the CLI. `loom_create(path, profile, suite, passphrase, passphrase_len)` creates a
  `.loom` under `profile` (`"default"`/`"blake3"` or `"fips"`/`"sha256"`), unencrypted when the
  passphrase is null/empty or encrypted (fresh DEK wrapped under the passphrase via Argon2id, `suite` or
  the profile default as the AEAD) - the counterpart of `loom init [--identity-profile fips]
  [--encrypt]`. New keyed openers `loom_sql_open_keyed` / `loom_open_keyed` /
  `loom_sql_batch_begin_keyed` take `(passphrase, passphrase_len)` and unlock an encrypted store for the
  session/handle/batch lifetime (each op reopens the loom per the per-op lock model); the non-keyed forms
  are unchanged and pass no key. A null/empty passphrase behaves like the non-keyed form (and fails
  `E2E_LOCKED` on an encrypted store); a wrong passphrase fails `E2E_KEY_INVALID`. No environment variable
  is consulted - the host supplies the passphrase bytes. `include/loom.h` is regenerated so every
  C-header consumer (cpp/ios/jvm/android/react-native) sees the new calls. Idiomatic per-language wrapper
  methods can layer over these ABI calls.

- **Ledger chain hash is identity-profile-aware; FIPS audit logs have no BLAKE3.**
  The ledger's per-entry tamper-evidence chain (`entry_hash[i] = H(entry_hash[i-1] ||
  payload)`) previously always used BLAKE3, leaving BLAKE3 in a FIPS store's cryptographic path. The
  chain now hashes under the store's identity profile: BLAKE3-256 in the default profile, SHA-256 in the
  FIPS profile. A `Ledger` carries its `Algo` (`Ledger::with_algo`, default `Ledger::new` stays
  BLAKE3 so the default profile is byte-for-byte unchanged); `get_ledger` decodes under the store's
  profile (the 32-byte chain hashes carry no algorithm tag) and `put_ledger` rejects a ledger whose
  chain algorithm disagrees with the store, so a BLAKE3 ledger cannot be smuggled into a FIPS store.
  Both `default/blake3` and `fips/sha256` ledger heads over a fixed payload sequence are pinned as
  conformance vectors (`LEDGER_HEAD_VECTORS` / `run_ledger_head_vectors_profiled`, exercised under both
  profiles in `run_all_vectors`).

- **CLI passphrase acquisition: prompt / file / fd, environment removed.** The encryption
  passphrase is no longer read from `LOOM_PASSPHRASE`. The CLI now takes a `--key-source` (and
  `--new-key-source` for rekey) selector with grammar `prompt` (default), `file:<path>`, or `fd:<n>`.
  `prompt` reads the passphrase with no echo via `rpassword` (confirming on `init`/`rekey`, rejecting an
  empty input, and erroring with a clear pointer to `file:`/`fd:` when there is no TTY); `file:` reads and
  trims a single line from a file; `fd:` reads from a file descriptor. Per the workspace `unsafe_code =
  "forbid"` lint, v1 supports only `fd:0` (stdin) using safe std I/O; arbitrary inherited descriptors are
  deferred. Acquisition is lazy: the passphrase is requested only when the
  target loom is actually encrypted. Verified end to end: `init --encrypt --key-source file:` -> `put`/`get` under
  `file:` and `fd:0` round-trip; a wrong passphrase fails `E2E_KEY_INVALID`; a no-TTY prompt errors
  cleanly; and `LOOM_PASSPHRASE` is ignored.

- **Encrypted stores are usable from every CLI verb.** The CLI now unlocks an encrypted Loom from
  `LOOM_PASSPHRASE` across all object-touching verbs, not just `put`/`get`. New loom-store openers
  `open_loom_unlocked` / `open_loom_read_unlocked` / `loom_over_backing_unlocked` unlock the store
  *before* loading engine state - necessary because the engine-state root is itself a sealed object, so
  `load_state` cannot read it while locked; `open_loom`/`open_loom_read` now return a clear `E2eLocked`
  on an encrypted loom instead of a confusing decode failure. The CLI routes `write`/`read`/`ls`/
  `commit`/`log`/`branch`/`checkout`/`merge`/`sql`/`bundle`/`clone`/`blame`/`diff`/`ns-*` through helpers
  that supply the passphrase. Verified end to end: a full encrypted vcs workflow (ns-create -> write ->
  commit -> read -> log) round-trips with no plaintext on disk, and a verb run without the passphrase fails
  with a clear `E2E_LOCKED`.

- **Identity profiles: parallel conformance vectors, both profiles certified.** Each
  canonical blob vector now pins both its `default/blake3` and `fips/sha256` address (the canonical CBOR
  bytes are identical across profiles - only the digest layer differs), and
  `run_blob_vectors_profiled(store, algo)` certifies a backend under either profile. A FIPS (SHA-256)
  `FileStore` is certified against the sha256 vectors in the standard test gate, so both profiles are
  exercised in CI without a separate matrix. The ledger chain-hash FIPS-purity refinement is tracked
  separately (it re-pins ledger vectors). This completes the FIPS identity-profile work end to end:
  digest foundation, store profile, engine content addressing, sync/bundle profile agreement, and
  parallel conformance - a full vcs/files Loom runs under SHA-256 addresses while the default BLAKE3
  profile is byte-for-byte unchanged.

- **Identity profiles: sync/bundle profile agreement.** The `Bundle` wire
  format now carries the source identity profile (digest algorithm) - a v2 frame
  (`[magic, version, digest_algo, ...]`); v1 bundles read back as the default BLAKE3 profile.
  `bundle_import` rejects a bundle whose profile differs from the destination Loom (`Conflict`), and the
  direct-sync entry points `clone_namespace` / `push_branch` require source and destination profiles to
  match - a cross-profile transfer is refused loudly rather than silently relabeling object addresses
  (rehash transfer is a separate, explicit migration). A backend can still host BLAKE3 and SHA-256 Looms
  side by side by partitioning on Loom id + profile and treating object ids as opaque.

- **Identity profiles: engine content addressing under the store profile.** The
  engine does not hard-code BLAKE3: `ObjectStore::digest_algo()` exposes the store's profile, and
  `content_address_with` / `Object::digest_with` plus the vcs file-content + content-map paths address
  content under it. `Digest` equality/ordering/hashing are now **bytes-only** - within a store every
  digest shares one algorithm, so a digest reconstructed during decode (tagged BLAKE3 by convention)
  still compares equal to the same address produced under the store's real profile; the store
  re-verifies a `get` under its own profile rather than the requested digest's tag. Result: a full
  vcs/files Loom over a FIPS (SHA-256) store commits, persists, reopens, and reads back end to end
  (test `full_loom_over_fips_profile_round_trips`), and the default (BLAKE3) profile is byte-for-byte
  unchanged. Prolly's chunking boundary stays BLAKE3 (structural, not identity); the ledger chain hash
  remains BLAKE3 as a tracked facet-internal FIPS-purity refinement.

- **Identity profiles: SHA-256 content addresses for a FIPS profile, foundation +
  store layer.** `loom_core::digest` gains `Algo::Sha256` and a profile-aware `Digest::hash(algo, data)`
  / `Digest::of(algo, bytes)` chokepoint (BLAKE3-256 and SHA-256 both fit the 32-byte slot; `Algo` stays
  `#[non_exhaustive]` for a future SHA3-256). The page-engine superblock records the digest algorithm at
  offset [20,21) (already reserved), CRC-covered and immutable; `FileStore` carries the profile, reads it
  on open, and routes `put`/`put_batch`/`get` record verification through it, so a FIPS store addresses
  objects with SHA-256 end to end while the default profile stays BLAKE3 (existing digests and
  conformance vectors are byte-for-byte unchanged). New constructors `FileStore::create_with_profile` /
  `create_encrypted_with_profile` / `with_backing_profile`; CLI `loom init --fips` (or `--identity-profile
  <default|fips>`), with `--fips` pairing AES-256-GCM when encrypting. The profile is chosen only at
  creation; changing it is an explicit migration, never an in-place rekey. **Scope note:** this lands the
  object-store (CAS) layer; threading the profile through the engine's content addressing (Tree/Commit
  links, file-content dedup, prolly node ids), bundle headers, the sync handshake, and parallel
  conformance vectors are the remaining staged sub-tasks, so engine-level use (commits/fs)
  of a FIPS store is not yet fully coherent.

- **Rekey data pass: rotate the DEK and AEAD suite by re-sealing every object.**
  `FileStore::rekey_reseal` reads every object under the current unlocked DEK and re-seals it under a
  fresh DEK (and optionally a new suite) while rewriting the store via the compaction machinery,
  recording the new `encryption_meta` and leaving the handle unlocked under the new session. This is
  distinct from the cheap `rekey` (DEK re-wrap under a new passphrase; same DEK and suite). CLI: `loom
  rekey --reseal [--suite <s>]`. Plaintext digests are unchanged, so object identity, the index, and
  conformance vectors are unaffected - only the sealed bytes change. A latent bug was fixed in passing:
  `compact` reopened the file and silently relocked an encrypted store; it now carries the unlocked DEK
  session across the reopen. CLI `put`/`get` now unlock from `LOOM_PASSPHRASE` on an encrypted store, so
  `put` seals (never writes a plaintext frame into an encrypted Loom) and `get` decrypts; the remaining
  object-touching verbs are tracked for the same treatment.

- **At-rest encryption: per-object AEAD frames, over the key layer.** An
  encrypted `.loom` seals every object below the content-address boundary: the store frames the payload
  (identity / DEFLATE / LZ4) then AEAD-seals it into frame id `0x10`/`0x11`/`0x12` as
  `suite_id || nonce || ciphertext || tag`, with a fresh per-object nonce from the OS CSPRNG and a
  per-object content-encryption key derived from the DEK (keyed-BLAKE3 for the XChaCha20-Poly1305
  default; HKDF-SHA-256 for the NIST AES-256-GCM suite - no BLAKE3 on the FIPS path). The associated
  data binds only pre-decrypt metadata (domain tag, frame version, on-disk frame id, suite, plaintext
  digest, plaintext/stored lengths) - **not** object type, which the record format does not carry. The
  digest is still over the plaintext, so an encrypted Loom keeps object identity and conformance digests
  unchanged. Reads return `E2eLocked` when a frame is encrypted but the store is locked, authenticate
  before returning any plaintext, and verify the digest after decrypt-then-decompress. All write paths
  (commit, segment GC, compaction) re-seal under the live DEK and never demote to plaintext;
  compaction now also carries the unlocked DEK session across its internal reopen. Tests cover both
  suites and all inner codecs, tamper / wrong-key / rebound-AD / wrong-suite rejection, no-plaintext
  leakage in the backing, AES-GCM nonce uniqueness, corrupted metadata / ciphertext, rekey, and
  byte-pinned golden conformance vectors for the sealed-frame wire format.

### Changed

- **`SELECT` now uses the durable secondary indexes through the planner.** A
  `Planner::plan` override adds GlueSQL's `plan_index` pass, so an indexed equality / range predicate is
  rewritten to a `NonClustered` index item the executor serves via `Index::scan_indexed_data` instead of
  a full scan. The implementation reads through the durable `index/<name>` prolly tree over the base
  snapshot and is sublinear for both equality (prefix scan) and ranges (`<`, `>`, `<=`, `>=` translate to
  an encoded `[start, upper)` byte span over the tree - exact because the index key encoding is
  order-preserving for every scalar column type); it merges overlay changes (a base row the overlay
  shadows is dropped; overlay rows are evaluated against the predicate) and returns rows in
  indexed-column order. A range over a composite (`List`/`Map`) column, a table whose index has no
  durable tree yet, or a predicate with no usable index still resolves correctly (index walk / base /
  full scan). loom-sql tests pin that unordered indexed equality and range `SELECT`s return rows in
  index order (not primary-key order) - proving the index path served them - including signed-integer
  range bounds across the negative/positive boundary and over uncommitted overlay rows.
- **SQL sessions now read through an owned, lock-free base snapshot.** A
  `LoomSqlStore` is a **lazy base + transaction overlay**: the base is an owned read view of the `.loom`
  captured at open (native: a separate `FileStore::open_read`, which takes no lock and coexists with a
  writer), and rows are read on demand - `SELECT` streams from the durable per-table prolly row
  maps instead of loading the whole table, so a table far larger than RAM can be queried with memory
  bounded by the tree height. The overlay holds only the rows changed this session; reads merge it over
  the base, `persist` flushes its deltas to a separate write loom, and a transaction snapshots the
  (small) overlay, not the whole table. A read-only `exec`/`query` now takes **no** write lock at all
  (the session opens a read snapshot and only acquires the writer when the statement actually changed
  something). The `LoomSqlBatch` snapshots its base at begin and holds the write loom for its lifetime as
  before. Result bytes and the pinned conformance digest are unchanged.

### Fixed

- **Sessions/handles no longer hold the `.loom` single-writer lock for their lifetime.** A
  `LoomSqlSession` / `LoomHandle` is now a reopenable handle of `(path, ns, db)`: each op opens the loom
  for its duration and releases it - reads via the lock-free read path, writes via the exclusive lock
  only for that op (matching the engine's single-writer model and the `loom` CLI). Previously a second session
  to the same file (even in-process) hit `CONFLICT: open for writing by another process`. Multiple
  sessions over one `.loom` now coexist; a regression test covers it. (Per-op reload is the safe
  default; an opt-in transaction/batch scope for throughput is tracked separately.)

### Changed

- **`loom_index_scan` lookup prefix is now canonical CBOR, not JSON (breaking).** The prefix crosses as
  `(prefix_ptr, prefix_len)` - a canonical-CBOR cell array (the same codec as a result row), parsed by
  `loom-sql`'s new `lookup_cbor` - instead of a JSON string. With this the whole ABI speaks one
  canonical form for both argument and result; there is no JSON on the wire anywhere. Callers build the
  prefix with `loom_core::tabular::encode_cells`. The `_async` form changed identically. (The old
  `loom-sql` `lookup_json` module is removed - one input form, no shim.)
- **Result payloads are now type-faithful canonical CBOR (breaking wire-form change).** Structured
  results (`loom_sql_exec`, `loom_read_table`, `loom_index_scan`, `loom_table_blame`,
  `loom_table_diff`, `loom_log`, `loom_merge`) are built directly from explicit canonical envelopes
  whose every scalar rides through the one shared, type-faithful cell codec
  (`loom_core::tabular::cell_value`, the same codec the facets and storage identity use). The previous
  path routed cells through `serde_json`, which broke 128-bit integers outside `i64`/`u64` range (a hard
  encode error), turned non-finite floats into `null`, widened `f32`, and stringified decimals. Those
  values now cross bit-exact. `loom_result_to_json` remains a debug-only renderer over the canonical
  bytes; JSON is never the wire form. (Persisted/object-identity bytes are unaffected - they already
  used this codec - so conformance digests are unchanged.)

- **Error contract reshaped to the normative form (breaking).** Every fallible C function now
  returns an `int32_t` status (`0` = ok, else the stable error `Code` as an integer via the new
  `loom_core::error::Code::as_i32`) and writes its result through an out-pointer, instead of returning
  an owned pointer / null sentinel. `loom_last_error` now takes `(&code, &msg, &len)` and exposes the
  stable code, not just a message. `loom_version` / `loom_blob_digest` stay infallible direct returns.
  Remaining gaps to the normative contract (length-prefixed canonical-bytes codec, async forms) are
  tracked separately.

### Added

- **`loom_row_open`** - the typed bridge for the streaming iterator: decode one streamed row (a
  canonical-CBOR cell array from `loom_iter_next`) into a one-row `LoomResultView`, read via the
  existing `loom_result_row_len` / `loom_result_cell` accessors and freed with `loom_result_close`. So a
  C-ABI binding iterates a `SELECT` row-by-row with typed cells and never parses CBOR. Reuses the one
  shared cell decoder (`loom-sql` `lookup_cbor::values_from_cbor`). The C-ABI bindings (cpp `row_stream`,
  ios/jvm `LoomRowStream`, Android KMP `LoomRowStream` + JNI) wrap `loom_sql_query` + `loom_iter_next` +
  `loom_row_open` as idiomatic lazy row iterators; node/python/wasm use their in-process `query`.
- **Streaming iterators** (`loom_sql_query` / `loom_iter_next` / `loom_iter_free`): the row-at-a-time
  reader surface bindings wrap as async iterators. `loom_sql_query` runs SQL and opens a
  forward-only iterator over the rows of its first `SELECT`; `loom_iter_next` yields one self-describing
  canonical-CBOR cell array per call (freed with `loom_bytes_free`) and sets `done = 1` at the end. The
  rows are computed eagerly behind the handle. Backed by `loom-sql` `select_rows_cbor`.
- **Transaction / batch scope** (`loom_sql_batch_begin` / `loom_sql_batch_exec` /
  `loom_sql_batch_commit` / `loom_sql_batch_commit_vcs` / `loom_sql_batch_abort` /
  `loom_sql_batch_close`): the opt-in held-open writer. A batch holds the `.loom` open -
  and its single-writer lock - for its lifetime and loads the SQL store once, so an explicit SQL
  transaction (`BEGIN`/`COMMIT`/`ROLLBACK`) spans `exec` calls and changes are made durable by one
  atomic save at `commit` (or `commit_vcs`, which also records a history entry). SQL `COMMIT` (a logical
  transaction) is kept distinct from the batch's atomic save and from a VCS commit. Backed by real
  GlueSQL transaction semantics in `loom-sql` (`BEGIN` snapshots, `COMMIT` promotes, `ROLLBACK`
  restores; nested transactions and a bare `COMMIT`/`ROLLBACK` are rejected, never silently accepted). A
  per-op `loom_sql_exec` now also rejects a transaction left open at the end of a single call.
- **Result-view surface** (`loom_result_open` / `loom_result_len` / `loom_result_item_kind` /
  `loom_result_column_*` / `loom_result_row_*` / `loom_result_cell` / `loom_result_count` /
  `loom_result_string_*` / `loom_result_variable_kind` / `loom_result_row_commit` /
  `loom_result_diff_*` / `loom_result_merge_outcome` / `loom_result_close`): C-ABI consumers
  (cpp/jvm/android/react-native) read a result without decoding CBOR or replicating the cell tag table.
  `loom_result_open` decodes a buffer once through the one shared decoder (`loom-sql`
  `result_view::decode`) into an owned, immutable, indexed view; scalars come back in a `LoomValue`
  (128-bit ints / decimal mantissas / UUIDs as 16-byte little-endian, `scale` for decimal; floats as
  raw bits plus a convenience `f64`; borrowed text/bytes; `LIST`/`MAP` as canonical CBOR).
- **Lossless bridge-JSON projection** (`loom_result_to_bridge_json`): the React Native bridge cannot
  carry `BigInt` / `Uint8Array` / non-finite numbers, so this renders a result to a lossless tagged-JSON
  string (single-key `$i64`/`$u64`/`$i128`/`$u128`/`$f32`/`$f64`/`$decimal`/`$bytes`(base64)/`$uuid`/
  `$inet`/`$date`/`$time`/`$timestamp`/`$interval`/`$point`/`$map`; bare primitives where exact) decoded
  once in Rust on top of `result_view::decode` (`loom-sql` `bridge_json`). It is a binding projection,
  not a second wire form; only React Native uses it.
- **Cross-language result-payload conformance vectors**: a C-ABI test reproduces the shared exec
  vector (`bindings/conformance/result-vectors.json`, pinned by `loom-sql` `result_exec_vector` /
  `RESULT_EXEC_VECTOR_DIGEST`) through `loom_sql_exec` and asserts the raw bytes are byte-for-byte the
  engine-pinned vector - the same bytes every language binding's suite asserts against the fixture.
- C-ABI SQL session: `loom_sql_open` / `loom_sql_exec` / `loom_sql_commit` / `loom_sql_close` plus
  `loom_last_error`. Opens a namespace SQL facet in a `.loom` (created on first use), runs arbitrary SQL
  (results returned as a JSON array of result payloads), and commits the staged result. This single
  path exposes the whole versioned tabular + SQL stack to any language binding - callers exchange SQL
  text and JSON instead of marshalling each rich column type across the boundary. GlueSQL stays
  contained in `loom-sql` behind a new `LoomSqlStore::exec_json`.
- C-ABI direct engine surface (`loom_open` / `loom_close` and handle ops): version-control verbs
  `loom_commit` / `loom_branch` / `loom_checkout` / `loom_log` / `loom_merge` (with a `cell_level`
  flag) and table inspection `loom_read_table` / `loom_index_scan` / `loom_table_blame` /
  `loom_table_diff`. Rich tabular values cross as JSON in the same shape as the SQL session, via new
  `loom_sql::result_json` marshalling helpers (tabular -> GlueSQL -> JSON).

## [0.0.1](https://github.com/uldrenai/uldren-loom/compare/uldren-loom-ffi-v0.0.0...uldren-loom-ffi-v0.0.1) - 2026-06-15

### Other

- Migrate crates.io to use oidc.
