# authz-prototype (throwaway)

A vertical slice of the principal authorization model. It mirrors the compute layer's
capability grammar (`../loom-compute/src/access.rs`: facet + scope + mode) and extends it into the
access-control core. Detached from the workspace; pure std, no dependencies. Safe to delete.

```
cargo run --release    # runs the worked examples as a self-checking demo
cargo test             # the same checks as unit tests
```

What it implements:

- **The grant grammar:** `effect` (Allow/Deny) + `subject` (principal or role) + `workspace`
  + `ref_glob` + `scope` (facet-specific prefix) + `facet` + `rights`. `Right` widens the
  program-facing Read/Write/ReadWrite into Read, Write, Advance, Merge, Admin, Exec.
- **The policy enforcement point:** `authorize(request)` with **deny-precedence** then allow
  then **default-deny**. A broad Deny beats a narrow Allow (specificity does not override a deny).
- **Roles** as grant bundles; a principal's grants are its direct grants plus its roles' grants.
- **Cross-workspace reads** require a Read grant on every touched workspace.
- **Fail-closed** for unknown or disabled principals (the property triggers rely on).

The demo encodes the worked examples: read-write a workspace except `secrets/`; read-only on
`main` but read-write on `dev`; a facet-only grant via a role; exec-but-not-write on a path prefix;
and deny-beats-allow. Stubbed (the real engine has these): credentials and the principal store,
and conditional CEL predicates; this slice is the matching core only.
