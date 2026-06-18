# 0034 - Key Sources and Unlock Providers

**Status:** Partial, current passphrase, raw-KEK, and multi-wrap surfaces documented. **Version:**
0.1.0-target.

**Depends on:** 0007 (bindings and C ABI), 0009 (encryption at rest), 0026 (principals and identity),
0031 (end-to-end encrypted sync), 0032 (platform parity). **Relates to:** 0005 (single-file
superblock metadata), the CLI, and host language bindings.

This spec defines how Loom obtains the key-encrypting key that wraps or unwraps an encrypted store's
data-encryption key. It sits above the object encryption frame and below principal authentication.
The current source implements passphrase-derived unlock and caller-supplied raw 256-bit KEK unlock.
Provider-specific acquisition from OS keychains, passkeys, KMS, HSM, TPM, or secure enclave is target
work.

## Current implementation

Current source-backed behavior:

- `loom_core::keys::KeySpec` supports `Passphrase` and `RawKek`.
- Passphrases are stretched through the profile-selected KDF before wrapping or unwrapping the DEK.
  Raw KEKs bypass the passphrase KDF and directly wrap or unwrap the DEK.
- `EncryptionMeta` encodes source-tagged wrap entries and decodes both legacy single-passphrase
  metadata and the current count-prefixed wrap list.
- `WrapSource` reserves stable on-disk codes for passphrase, keystore, secure enclave, passkey, KMS,
  raw KEK, TPM, and HSM. Current create paths write passphrase or raw-KEK entries.
- Unlock attempts are credential-kind scoped: passphrases try passphrase entries, while raw KEKs try
  external entries.
- `EncryptionMeta::rewrap` and `FileStore::rekey` replace the current wrap set with one new wrap.
- `EncryptionMeta::add_wrap`, `EncryptionMeta::remove_wrap`, `FileStore::add_wrap`, and
  `FileStore::remove_wrap` append or remove unlock methods for the same DEK.
- Multi-wrap writes enforce the recovery decision: an external-only wrap set is rejected unless the
  caller explicitly allows no recovery path.
- `FileStore::rekey_reseal` can rotate object encryption by rewriting object frames under a new
  DEK/session.
- The CLI parses `--key-source prompt`, `file:<path>`, `fd:<n>`, `raw-kek:file:<path>`, and
  `raw-kek:fd:<n>`. Safe descriptor reads currently support `fd:0`; other descriptor numbers parse
  but fail at acquisition.
- The C ABI exposes passphrase and raw-KEK create/open paths for store, SQL session, and SQL batch
  operations, plus add-wrap and remove-wrap operations on an open file-store handle.
- C++, iOS, JVM, Android, React Native, and wasm expose raw-KEK store create/open paths. Node and
  Python expose the raw-KEK wrap-management wrapper (`key_add_wrap_with_kek`) but not a raw-KEK store
  create/open path.

Current source does not implement:

- Provider-specific KEK acquisition from OS keychains, secure enclave, TPM, WebAuthn/passkey PRF,
  cloud KMS, or HSM.
- Host callback ABI for unlock providers. The implemented model is caller-supplied passphrase bytes
  or caller-supplied 32-byte KEK bytes.
- Arbitrary inherited file descriptor reads in the CLI.
- Provider posture or attestation metadata.
- Conformance that covers every binding key-source projection.

## Model

Loom always encrypts object bytes with a random local DEK. A key source protects only that DEK. It is
never the DEK and never touches plaintext object bytes directly.

Two integration shapes are supported by the model:

| Shape | Source yields | DEK protection | Current status |
| --- | --- | --- | --- |
| Passphrase-derived KEK | Secret passphrase bytes | Profile KDF derives a 256-bit KEK, then AEAD wraps the DEK | Implemented |
| External KEK | Host supplies or derives a 256-bit KEK | KEK directly AEAD-wraps the DEK, no passphrase KDF | Implemented as raw KEK plumbing |

Provider-specific sources are target front ends over the external-KEK shape. A keychain, passkey,
KMS, HSM, TPM, or secure enclave provider derives or releases a 32-byte KEK in host code, then calls
the existing raw-KEK open/create primitive unless a later callback ABI is explicitly approved.

## Current key-source selector

The current CLI selector grammar is:

```text
prompt
file:<path>
fd:<n>
raw-kek:file:<path>
raw-kek:fd:<n>
```

Rules:

- `prompt` is the default interactive passphrase source.
- `file:<path>` reads a passphrase from a file and trims the trailing newline.
- `fd:<n>` reads a passphrase from an inherited descriptor. Current safe implementation accepts
  only `fd:0` at acquisition.
- `raw-kek:file:<path>` and `raw-kek:fd:<n>` read exactly 64 hex characters and decode them to a
  32-byte KEK.
- Environment variables and command-line passphrase values are not key sources.

Target provider selectors are reserved design space:

```text
keystore[:<service>/<account>]
secure-enclave[:<label>]
passkey[:<rp-id>]
kms:<provider>:<key-uri>
hsm:pkcs11:<module>?slot=<n>&label=<l>
```

These selectors must not be advertised as implemented until a host provider, error mapping,
conformance coverage, and binding projection exist.

## Provider placement

The enterprise posture is to keep provider SDKs and platform crypto outside `loom-core`.

- `loom-core` owns DEK wrap metadata, KDFs, AEAD wrap/unwrap, and stable on-disk source tags.
- `loom-store` owns encrypted file opening, object sealing, unlock state, rekey, and reseal.
- The CLI and bindings acquire passphrases or KEKs from their host environment.
- Provider integrations for Keychain, DPAPI, Secret Service, Secure Enclave, TPM, passkeys, KMS, and
  HSM live in host surfaces or binding packages unless a reviewed callback ABI becomes necessary.

This keeps the core portable, dependency-light, and suitable for wasm.

## Target provider matrix

| Provider | Shape | Target owner |
| --- | --- | --- |
| OS keychain | External KEK | Native host or binding |
| Secure enclave | External KEK | Native/mobile host or binding |
| TPM | External KEK | Native/mobile host or binding |
| Passkey PRF | External KEK | Browser/mobile/native host or binding |
| Cloud KMS | External KEK | Server host or binding |
| HSM or PKCS#11 | External KEK | Server host or binding |
| Docker/Kubernetes secret file | Passphrase or external KEK | CLI or host |
| systemd credentials | Passphrase or external KEK | CLI or host |

The on-disk `WrapSource` enum has separate stable TPM and HSM tags, so enterprise audit and policy
code can distinguish secure enclave from TPM, and cloud KMS from HSM, without parsing
provider-specific `params`.

## Binding projection

Multi-wrap management wrappers are source-backed in the C ABI and the Node, Python, and C++ bindings.
Raw-KEK store create/open projection is uneven and is what remains before 0034 is complete.

- C ABI: passphrase and raw-KEK create/open/session/batch entry points exist, plus add-wrap
  (passphrase and raw-KEK) and remove-wrap on an open handle.
- Node, Python, and C++ expose public multi-wrap management wrappers: `key_add_wrap_keyed`,
  `key_add_wrap_with_kek`, and `key_remove_wrap`.
- C++, iOS, JVM, Android, React Native, and wasm expose raw-KEK store create/open paths. Node and
  Python expose the raw-KEK wrap-management wrapper but not a raw-KEK store create/open path.
- Every binding must document that host code acquires the secret and Loom receives only passphrase
  bytes or a 32-byte KEK.

## Current multi-wrap behavior

Multi-wrap metadata decode exists, and current source can append and remove wraps for an encrypted
store. The caller must already unlock the store, proving possession of one valid credential. Loom
then wraps the same DEK under the new credential or removes the selected wrap entry.

Current source-backed behavior:

- Add-wrap operation: open with an existing credential, unwrap the DEK, wrap the same DEK under the
  new credential, append a `WrapEntry`, and persist the updated metadata.
- Remove-wrap operation: remove one selected wrap while preserving at least one remaining unlock path.
- Recovery policy: external wraps require a passphrase recovery wrap unless an explicit no-recovery
  override is chosen.
- CLI coverage: `loom key add-wrap <store>` uses `--key-source` for the current credential and
  `--new-key-source` for the added credential. `loom key remove-wrap <store> <index>` removes a
  zero-based wrap index.
- C ABI coverage: `loom_key_add_wrap_keyed`, `loom_key_add_wrap_with_kek`, and
  `loom_key_remove_wrap` update an encrypted store through an open handle.
- Duplicate detection: a duplicate add of the same passphrase or raw KEK is rejected with
  `AlreadyExists` and the message "wrap credential already exists", in core and over the C ABI.
- Binding coverage: Node, Python, and C++ expose public add-wrap (keyed and raw-KEK) and remove-wrap
  wrappers.

Still-unfinished multi-wrap behavior:

- Public multi-wrap management wrappers in the remaining bindings (iOS, JVM, Android, React Native,
  wasm).
- Shared cross-binding and conformance coverage for add, remove, duplicate-wrap rejection, last-wrap
  rejection, recovery-policy rejection, and wrong-credential behavior.

## Resolved decisions

These decisions define the target provider contract. The multi-wrap recovery rule is source-backed in
core, store, CLI, and the C ABI. The stable TPM and HSM `WrapSource` tag split is source-backed in
core; provider acquisition, params encodings, binding projection, and conformance remain target work.

1. **Multi-wrap recovery policy:** credential-kind-scoped unlock attempts plus a recovery passphrase
   requirement for hardware/provider wraps, with an explicit no-recovery override for deployments
   that intentionally accept provider-only lockout risk.
2. **Provider tag taxonomy:** split provider-class `WrapSource` tags before release. The target
   taxonomy must distinguish TPM from secure enclave and HSM from cloud KMS in stable on-disk tags;
   `params` remains for provider-specific handles, credential ids, ARNs, labels, salts, and posture
   details.

## Owner decisions still needed

These decisions still require owner approval before code or wire contracts are promoted.

1. **Passkey descriptor shape:** recommended target is storing credential id plus PRF salt in
   `WrapEntry.params`; they are not secret and make unlock deterministic across passkey managers.
2. **KMS/HSM caching:** recommended target is no persisted cache, no default DEK cache, and an
   optional in-memory TTL cache owned by the host process.
3. **Unlock integration shape:** recommended target is the current pull model, where the host obtains
   the passphrase or 32-byte KEK and calls Loom. A callback ABI should be added only if a provider
   cannot work through pull acquisition.
4. **FIPS/provider posture:** recommended target is an informational provider posture field in
   `params`, not a trust boundary. Verified attestation can be a later deployment-specific extension.

## Unfinished work

- (P0) Add public multi-wrap management wrappers in the remaining bindings (iOS, JVM, Android, React
  Native, wasm); Node, Python, and C++ already expose them.
- (P0) Finish raw-KEK store create/open parity for Node and Python or explicitly mark them unsupported
  for that surface.
- (P0) Implement host provider acquisition for OS keychains, secure enclave or TPM, passkeys, KMS, and
  HSM without adding provider SDK dependencies to `loom-core`.
- (P0) Define provider params encoding for each promoted provider.
- (P1) Add arbitrary inherited descriptor support outside `loom-core` if the CLI keeps advertising
  descriptor numbers other than `fd:0`.
- (P0) Add conformance for key-source selector parsing, wrong credential behavior, raw-KEK length
  validation, passphrase/KEK non-interchangeability, multi-wrap add/remove, recovery-policy
  rejection, cross-binding duplicate-detection coverage, provider unsupported behavior, and every
  binding projection. Duplicate-wrap rejection itself is source-backed in core and over the C ABI.

## Active key-source provider owner gate

Completion state: active implementation owner. Passphrase, raw-KEK, source-tagged wrap metadata,
multi-wrap add/remove, duplicate rejection, recovery-policy enforcement, selected C ABI operations,
and selected binding projections are source-backed. Provider acquisition, parameter encoding,
remaining binding parity, reporting, conformance, and owner decisions remain implementation work.

Decision Points:

1. Question: What passkey descriptor should a promoted provider store?
   Context: A passkey provider needs deterministic unlock data without storing secret material.
   Examples: Credential id plus PRF salt in `WrapEntry.params` lets a passkey manager find and derive
   the same external KEK; storing opaque provider blobs would make audit and migration harder.
   Options: Store credential id plus PRF salt; store an opaque provider blob; defer passkeys.
   Recommendation: Store credential id plus PRF salt in `WrapEntry.params`.
   Consequence of deferring: Passkey provider work cannot promote beyond target.
2. Question: Should KMS/HSM DEK or KEK material be cached?
   Context: Enterprise deployments may care about latency, but persistent caches change the security
   boundary.
   Examples: No persisted cache keeps the provider as the live authority; an in-memory TTL cache can
   reduce repeated unwrap latency without changing on-disk exposure.
   Options: No cache; optional in-memory TTL cache; persisted cache.
   Recommendation: No persisted cache, with optional host-owned in-memory TTL cache.
   Consequence of deferring: KMS/HSM provider behavior stays target because operational posture is
   undefined.
3. Question: Should provider unlock use pull acquisition or a callback ABI?
   Context: The current model has host code acquire passphrase bytes or a 32-byte KEK and pass it to
   Loom.
   Examples: OS keychain, KMS, HSM, and passkey hosts can usually acquire an external KEK and call the
   raw-KEK primitive; a callback ABI adds cross-language lifetime and security complexity.
   Options: Keep pull acquisition; add callback ABI now; defer providers that cannot use pull
   acquisition.
   Recommendation: Keep pull acquisition and add a callback ABI only for a provider that cannot work
   through pull acquisition.
   Consequence of deferring: Provider projection can proceed only for pull-compatible hosts.
4. Question: How should FIPS and provider posture be represented?
   Context: Provider posture is useful for audit, but it is not itself proof of cryptographic module
   validation.
   Examples: An informational posture field can report provider class, attestation, or FIPS mode;
   treating the field as a trust boundary would overclaim.
   Options: Informational posture field; trust-enforcing posture field; no posture field.
   Recommendation: Use an informational provider posture field in `params`.
   Consequence of deferring: Capability and conformance reports cannot honestly distinguish provider
   posture.

| Gate | Source-backed evidence | Remaining implementation work | Disposition |
| --- | --- | --- | --- |
| Provider acquisition | Passphrase and raw-KEK acquisition are source-backed; provider-specific acquisition is target. | Implement host provider acquisition for OS keychains, secure enclave or TPM, passkeys, KMS, and HSM without adding provider SDK dependencies to `loom-core`. | Target P0, blocked on owner decisions where applicable. |
| Provider params and posture | Stable `WrapSource` tags reserve provider classes. | Define canonical provider params, posture fields, validation, redaction, migration, and unsupported-provider reporting for each promoted provider. | Target P0. |
| Binding parity | C ABI and selected bindings expose passphrase, raw-KEK, and multi-wrap slices. | Finish raw-KEK create/open parity for Node and Python or mark unsupported, and add public multi-wrap wrappers in iOS, JVM, Android, React Native, and WASM. | Target P0. |
| Conformance and reports | Duplicate-wrap rejection and selected core/C ABI behavior are source-backed. | Add conformance for selector parsing, wrong credentials, raw-KEK length, passphrase/KEK non-interchangeability, multi-wrap behavior, provider unsupported states, and every binding projection. | Target P0. |
