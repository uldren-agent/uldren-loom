# 0004a - Hosted Provider Facade

**Status:** Draft target. **Version:** 0.1.0-draft. **Normative only after promotion.**

This sub-spec holds the enterprise provider facade that is useful for hosted, generated, or remote
provider surfaces but is not required to close 0004 as the current source-backed provider boundary.
The source-backed 0004 contract remains the lean Rust `ObjectStore` trait, `FileStore`, and
`BackingIo` support.

## Current Source Boundary

Implemented today:

- `ObjectStore` stores immutable canonical object bytes by computed digest;
- `FileStore` persists objects in a `.loom` page engine or caller-supplied `BackingIo`;
- `FileStore` persists engine state through one `reference_root`;
- `Loom` and `Registry` own workspace refs, branches, working trees, sync state, and exported engine
  state above `ObjectStore`;
- native writable `FileStore` opens use a single-writer advisory lock;
- native read-only opens are lock-free;
- non-file and browser hosts supply their own `BackingIo` coordination;
- direct local sync and bundle sync transfer objects and refs between Loom values.

Not implemented today:

- (P1) a public `Provider` facade object;
- (P1) generated provider interfaces from IDL;
- (P0) public provider capability reports - the local source-backed `capabilities()` facade now exists
  (`loom_core::capability`, 0004 §4 / 0010 §5); the *hosted/remote* provider capability report over a
  transport remains target;
- (P1) remote object and ref calls over a hosted transport;
- (P0) authenticated remote refs;
- (P0) hosted ref compare-and-swap exposed as a public provider API;
- (P2) reflogs, pins, remote GC policy, hosted retention policy, or remote provider conformance.

## Target Interface

The target hosted Provider interface combines object storage, mutable refs, lifecycle, capability
reporting, and remote-service concerns. It is not the Rust `ObjectStore` trait.

```idl
interface Provider {
  identity_profile(): IdentityProfile
  capabilities(): CapabilitySet

  get_object(d: Digest): Future<Option<bytes>>
  put_object(canonical: bytes): Future<Digest>
  has_object(d: Digest): Future<bool>
  iter_objects(filter: ObjectFilter): Stream<Digest>
  delete_object(d: Digest): Future<void>

  get_ref(name: string): Future<Option<Digest>>
  list_refs(prefix: Option<string>): Future<List<Ref>>
  cas_ref(name: string, expected_old: Option<Digest>, new: Option<Digest>): Future<void>
  append_reflog(name: string, entry: ReflogEntry): Future<void>

  begin(): Future<Txn>
  flush(): Future<void>
  close(): Future<void>
  stats(): Future<ProviderStats>
}
```

The target interface may also expose optional working-tree materialization, but bare providers return
`UNSUPPORTED` for working-tree operations.

### Conditional mutation at the hosted boundary

A hosted provider consumes, rather than redefines, the conditional-mutation contract in 0003 section
9.1. `cas_ref` is a projection of the ref owner's compare-and-apply operation; its optional digest is a
provider-specific input shape, not a universal Loom condition-token serialization. A provider extension
for another mutable resource must declare its owning native primitive and atomic scope before assigning
product syntax to `any`, `absent`, `exact`, `generation`, or `operation_anchor`.

The hosted kernel order is fixed: resolve and authorize the principal, invoke the owning native
primitive at its documented atomic read point, retain only redacted result material for audit under
0009, then map the stable 0003 section 8 outcome to a transport response. A provider transport must not
perform an independent comparison, disclose a protected current value or raw opaque token, or turn a
failed comparison into a partial mutation.

## Promotion Requirements

Before this facade becomes part of the main provider contract:

- (P0) define the generated IDL, C ABI, and binding projection;
- (P0) define capability reporting names through 0010;
- (P0) define remote authentication and served write authorization through 0026-0028;
- (P1) define protocol projection through 0008;
- (P0) define hosted ref update, non-fast-forward, protected-ref, and audit behavior;
- (P2) define remote GC, retention, pin, and deletion refusal behavior;
- (P0) add provider conformance for object identity, get/has, digest-profile honesty, ref CAS, recovery,
  unsupported capability errors, and hosted transport error mapping;
- (P0) update 0004 to move promoted pieces from this sub-spec into the source-backed provider
  contract.
