# 0067c - Client-parity coverage report (Queue 11 task 430)

Status: generated 2026-07-13 from `idl/loom.idl` (source of truth) cross-referenced against `specs/0067` §12.

This report is the task-430 "matrix-backed parity report". It classifies **every** IDL method so there are no hidden gaps. Local-vs-remote *byte* parity is proven directly by the shared `run_client_parity_suite` (crate `loom-protocol-conformance`, module `client_parity`) for the suite-covered set; the remainder is accounted for by existing live/protocol tests, by local-only contract, or as explicit follow-up.

## Classifications

- **suite-covered** - exercised by `run_client_parity_suite`; the local (`LocalClientDriver`, in-sandbox) and remote (`RemoteClientDriver` over `loom serve remote`, owner-run) drivers produce byte-identical `ParityReport` entries.
- **live+protocol-covered** - a remote round-trip method whose family is exercised end-to-end by the MCP live test (task 370) and/or the loom-cli facade live test (task 350), on top of generated server dispatch (R2.2) + typed client stubs (R2.3) verified by `uldren-loom-remote-codegen --check`.
- **protocol-covered** - a remote round-trip method with generated dispatch + client stubs + codegen `--check` + carrier streaming tests (R4c), but no dedicated end-to-end live test yet; a client-parity fixture is a recorded follow-up.
- **session/connection** - session establishment (`Cn`); exercised by every live test and the client-facing carrier session-open (task 350a-0).
- **local-only** - runs entirely in the client and never crosses the wire (§12 `Lo`/`Lp`/`Lc`); parity is not applicable by contract.
- **follow-up** - not covered above; explicitly recorded (most are methods added after §12 was written - a §12 refresh is task 460).

## Summary (353 methods across 41 interfaces)

| Classification | Methods |
| --- | --- |
| suite-covered | 18 |
| live+protocol-covered | 228 |
| protocol-covered | 50 |
| session/connection | 16 |
| local-only | 41 |
| follow-up | 0 |

These counts tally the per-method rows below and sum to 341. The count grew from the task-430 snapshot (323, split `18 / 203 / 33 / 16 / 41 / 12`) as the surface reached 341: the 12 filesystem/archive/CAR methods that were "follow-up" at draft time now have §12 classifications and generated dispatch/client coverage (task 460); the `Metrics` (4), `Document` text/binary (5), Calendar `put_ics`, Contacts `put_vcard`, Search `source_digest`/`status`, and VersionControl `head_branch` methods landed; and the FileSystem directory/metadata surface (`create_directory`, `remove_directory`, `stat`, `list_directory` - 4) was added. The task-600 live tests promote several newer methods to `live+protocol-covered`: the four FileSystem directory/metadata methods via `files_dir_surface_local_and_remote_over_tls`; and VersionControl `head_branch`, Document text/binary (`put_text`/`get_text`/`put_binary`/`get_binary`/`list_binary`), Calendar `put_ics`, and Contacts `put_vcard` via the `run_client_parity_suite`/`client_parity_local_matches_remote` served-endpoint parity test. Metrics (`put_descriptor`/`get_descriptor`/`put_observation`/`query`) and Search (`source_digest`/`status`) are also `live+protocol-covered` via that same parity test. The surface then grew to 353/41 with the observability families: `Logs` (3) and `Traces` (4) are `live+protocol-covered` via the same `client_parity_local_matches_remote` test (their `ParityDriver` steps run over the served endpoint), while `Program` (5) is `protocol-covered` (generated dispatch/client/codegen, no dedicated client-parity fixture yet). That is why `live+protocol-covered` is 228 (203 + 4 FileSystem + 1 `head_branch` + 5 Document text/binary + 2 PIM + 4 Metrics + 2 Search + 3 Logs + 4 Traces), `protocol-covered` is 50 (45 + 5 Program), and `follow-up` is 0. No method is `live+protocol-covered` without an actual live test.

## Per-interface classification

### Store

| Method | §12 | Parity | Evidence |
| --- | --- | --- | --- |
| `version` | Sm | suite-covered | client parity suite `run_client_parity_suite` - byte-parity local vs remote |
| `capabilities` | Sm | live+protocol-covered | §12 `Sm`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `runtime_profile` | Sm | live+protocol-covered | §12 `Sm`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `blob_digest` | Lo | local-only | §12 transport `Lo` - never crosses the wire |
| `digest_algo` | - | live+protocol-covered | added after the first draft; live-verified via task 395/397/350; classified in §12 per its interface (task 460) |
| `create` | Lp | local-only | §12 transport `Lp` - never crosses the wire |
| `create_with_kek` | Lp | local-only | §12 transport `Lp` - never crosses the wire |
| `open` | Cn | session/connection | §12 transport `Cn` - remote session establishment; exercised by every live test + carrier session-open (task 350a-0) |
| `open_keyed` | Cn | session/connection | §12 transport `Cn` - remote session establishment; exercised by every live test + carrier session-open (task 350a-0) |
| `open_with_kek` | Cn | session/connection | §12 transport `Cn` - remote session establishment; exercised by every live test + carrier session-open (task 350a-0) |
| `close` | Cn | session/connection | §12 transport `Cn` - remote session establishment; exercised by every live test + carrier session-open (task 350a-0) |

### KeySource

| Method | §12 | Parity | Evidence |
| --- | --- | --- | --- |
| `key_add_wrap_keyed` | U | protocol-covered | §12 `U`; generated dispatch (R2.2) + client stubs (R2.3) + codegen `--check` + carrier tests (R4c); client-parity fixture is follow-up |
| `key_add_wrap_with_kek` | U | protocol-covered | §12 `U`; generated dispatch (R2.2) + client stubs (R2.3) + codegen `--check` + carrier tests (R4c); client-parity fixture is follow-up |
| `key_remove_wrap` | U | protocol-covered | §12 `U`; generated dispatch (R2.2) + client stubs (R2.3) + codegen `--check` + carrier tests (R4c); client-parity fixture is follow-up |

### Workspaces

| Method | §12 | Parity | Evidence |
| --- | --- | --- | --- |
| `workspace_create` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `workspace_list` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `workspace_rename` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `workspace_delete` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |

### Exec

| Method | §12 | Parity | Evidence |
| --- | --- | --- | --- |
| `exec_cbor` | U | protocol-covered | §12 `U`; generated dispatch (R2.2) + client stubs (R2.3) + codegen `--check` + carrier tests (R4c); client-parity fixture is follow-up |

### Sessions

| Method | §12 | Parity | Evidence |
| --- | --- | --- | --- |
| `authenticate_passphrase` | U | protocol-covered | §12 `U`; generated dispatch (R2.2) + client stubs (R2.3) + codegen `--check` + carrier tests (R4c); client-parity fixture is follow-up |
| `clear_authentication` | U | protocol-covered | §12 `U`; generated dispatch (R2.2) + client stubs (R2.3) + codegen `--check` + carrier tests (R4c); client-parity fixture is follow-up |

### Identity

| Method | §12 | Parity | Evidence |
| --- | --- | --- | --- |
| `identity_list` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `identity_add_principal` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `identity_rename_principal_handle` | - | live+protocol-covered | added after the first draft; live-verified via task 395/397/350; classified in §12 per its interface (task 460) |
| `identity_set_passphrase` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `identity_remove_principal` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `identity_assign_role` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `identity_revoke_role` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `identity_create_external_credential` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` Audited-result contract landed in Queue 11 task 510: the four methods return IDL `IdentityAuditResult` (audit seq + minted id + action + redacted target); the `loom identity` CLI routes through the `StoreClient` facade (remote path reconstructs the full printed record via a follow-up `identity_list`). loom-cli build/live parity owner-run. |
| `identity_revoke_external_credential` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` Audited-result contract landed in Queue 11 task 510: the four methods return IDL `IdentityAuditResult` (audit seq + minted id + action + redacted target); the `loom identity` CLI routes through the `StoreClient` facade (remote path reconstructs the full printed record via a follow-up `identity_list`). loom-cli build/live parity owner-run. |
| `identity_add_public_key` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` Audited-result contract landed in Queue 11 task 510: the four methods return IDL `IdentityAuditResult` (audit seq + minted id + action + redacted target); the `loom identity` CLI routes through the `StoreClient` facade (remote path reconstructs the full printed record via a follow-up `identity_list`). loom-cli build/live parity owner-run. |
| `identity_revoke_public_key` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` Audited-result contract landed in Queue 11 task 510: the four methods return IDL `IdentityAuditResult` (audit seq + minted id + action + redacted target); the `loom identity` CLI routes through the `StoreClient` facade (remote path reconstructs the full printed record via a follow-up `identity_list`). loom-cli build/live parity owner-run. |
| `identity_create_app_credential` | U | live+protocol-covered | Queue 11 task 520 (owner decision 2, server-generated secret): returns IDL `AppCredentialCreateResult` (audit seq + record fields + the one-time plaintext bearer token). The secret+salt are minted by the storing authority through the shared `loom_client::local::mint_app_credential` (the hosted server via `LocalLoomClient::identity_create_app_credential`, and the local CLI arm calling the same function), the store keeps only the salted verifier, and the token is returned exactly once and never echoed by list/get/audit/revoke. `loom identity create-app-credential` routes through the `StoreClient` facade; authenticated remote sessions use `SessionAuth::Passphrase`. Live parity GREEN owner-run 2026-07-17. |
| `identity_revoke_app_credential` | U | live+protocol-covered | Queue 11 task 520: returns IDL `IdentityAuditResult` (no secret material); `loom identity revoke-app-credential` routes through the `StoreClient` facade (remote path reads the record before revoking, matching local output). loom-cli build/live parity owner-run. |

### Acl

| Method | §12 | Parity | Evidence |
| --- | --- | --- | --- |
| `acl_list` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `acl_grant` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `acl_revoke` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |

### ProtectedRefs

| Method | §12 | Parity | Evidence |
| --- | --- | --- | --- |
| `protected_ref_list` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `protected_ref_get` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `protected_ref_set` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `protected_ref_remove` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |

### Daemon

| Method | §12 | Parity | Evidence |
| --- | --- | --- | --- |
| `daemon_start` | Lc | local-only | §12 transport `Lc` - local control plane (path-keyed daemon) |
| `daemon_stop` | Lc | local-only | §12 transport `Lc` - local control plane (path-keyed daemon) |
| `daemon_restart` | Lc | local-only | §12 transport `Lc` - local control plane (path-keyed daemon) |
| `daemon_status` | Lc | local-only | §12 transport `Lc` - local control plane (path-keyed daemon) |
| `daemon_doctor` | Lc | local-only | §12 transport `Lc` - local control plane (path-keyed daemon) |
| `daemon_session_attach` | Lc | local-only | §12 transport `Lc` - local control plane (path-keyed daemon) |
| `daemon_session_detach` | Lc | local-only | §12 transport `Lc` - local control plane (path-keyed daemon) |
| `daemon_pin_add` | Lc | local-only | §12 transport `Lc` - local control plane (path-keyed daemon) |
| `daemon_pin_remove` | Lc | local-only | §12 transport `Lc` - local control plane (path-keyed daemon) |

### Locks

| Method | §12 | Parity | Evidence |
| --- | --- | --- | --- |
| `lock_acquire` | U | protocol-covered | §12 `U`; generated dispatch (R2.2) + client stubs (R2.3) + codegen `--check` + carrier tests (R4c); client-parity fixture is follow-up |
| `lock_refresh` | U | protocol-covered | §12 `U`; generated dispatch (R2.2) + client stubs (R2.3) + codegen `--check` + carrier tests (R4c); client-parity fixture is follow-up |
| `lock_release` | U | protocol-covered | §12 `U`; generated dispatch (R2.2) + client stubs (R2.3) + codegen `--check` + carrier tests (R4c); client-parity fixture is follow-up |

### VersionControl

| Method | §12 | Parity | Evidence |
| --- | --- | --- | --- |
| `commit` | U | suite-covered | client parity suite `run_client_parity_suite` - byte-parity local vs remote |
| `branch` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `checkout` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `log` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `head_branch` | U | live+protocol-covered | §12 `U` (task 590); generated dispatch + client + codegen `--check` at 341; local-vs-remote byte parity over a served self-signed-TLS endpoint via `run_client_parity_suite`/`client_parity_local_matches_remote` (task 600; `ParityDriver::vcs_head_branch`); loom-client unit test (task 590) |
| `merge` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `merge_in_progress` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `merge_conflicts` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `merge_resolve` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `merge_abort` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `merge_continue` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `diff` | Uc | live+protocol-covered | §12 `Uc`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `blame` | Uc | live+protocol-covered | §12 `Uc`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `log_async` | Ua | live+protocol-covered | §12 `Ua`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `merge_async` | Ua | live+protocol-covered | §12 `Ua`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `status` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `stage` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `stage_all` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `unstage` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `commit_staged` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `tag_create` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `tag_list` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `tag_target` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `tag_delete` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `tag_rename` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `restore_file` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `restore_path` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `cherry_pick` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `revert` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `rebase` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `squash` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |

### Watch

| Method | §12 | Parity | Evidence |
| --- | --- | --- | --- |
| `subscribe` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `poll` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `stream` | St | live+protocol-covered | §12 `St`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |

### FileSystem

| Method | §12 | Parity | Evidence |
| --- | --- | --- | --- |
| `write_file` | Uc | live+protocol-covered | §12 `Uc`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `read_file` | Uc | live+protocol-covered | §12 `Uc`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `append_file` | Uc | live+protocol-covered | §12 `Uc`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `remove_file` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `read_at` | Uc | live+protocol-covered | §12 `Uc`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `write_at` | Uc | live+protocol-covered | §12 `Uc`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `truncate` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `symlink` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `read_link` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `create_directory` | U | live+protocol-covered | §12 `U` (task 570); generated dispatch + client + codegen `--check` at 341; end-to-end over a served self-signed-TLS endpoint by the task-600 live fixture `files_dir_surface_local_and_remote_over_tls` (loom-cli), asserting local-vs-remote parity; loom-client round-trip unit test `filesystem_directory_and_stat_surface_round_trips` (task 570) |
| `remove_directory` | U | live+protocol-covered | §12 `U` (task 570); generated dispatch + client + codegen `--check` at 341; end-to-end over TLS by the task-600 live fixture `files_dir_surface_local_and_remote_over_tls` (incl. non-empty-without-recursive error + recursive delete parity); loom-client round-trip unit test (task 570) |
| `stat` | U | live+protocol-covered | §12 `U` (task 570); returns canonical-CBOR `loom.fs.stat.v1` (loom-wire `fs_stat_to_cbor`/`fs_stat_from_cbor`, round-trip test); generated dispatch + client + codegen `--check` at 341; end-to-end over TLS by the task-600 live fixture `files_dir_surface_local_and_remote_over_tls` (used by `files delete` to classify path); loom-client round-trip unit test (task 570) |
| `list_directory` | U | live+protocol-covered | §12 `U` (task 570); returns canonical-CBOR `loom.fs.dir-listing.v1` (loom-wire `dir_listing_to_cbor`/`dir_listing_from_cbor`, round-trip test); generated dispatch + client + codegen `--check` at 341; end-to-end over TLS by the task-600 live fixture `files_dir_surface_local_and_remote_over_tls` (recursive-walk listing parity with local `staged_paths`); loom-client round-trip unit test (task 570) |
| `import_fs` | U | protocol-covered | §12 `U` (task 460); generated dispatch + client + codegen `--check` at 332 (task 456); loom-client round-trip unit test `filesystem_import_export_roundtrips_sync_and_async` (task 456); remote parity/live fixture pending |
| `export_fs` | U | protocol-covered | §12 `U` (task 460); generated dispatch + client + codegen `--check` at 332 (task 456); loom-client round-trip unit test (task 456); remote parity/live fixture pending |
| `import_fs_async` | Ua | protocol-covered | §12 `Ua` (task 460); generated dispatch + client + codegen `--check` at 332 (task 456); loom-client immediate-complete task unit test (task 456); remote parity/live fixture pending |
| `export_fs_async` | Ua | protocol-covered | §12 `Ua` (task 460); generated dispatch + client + codegen `--check` at 332 (task 456); loom-client immediate-complete task unit test (task 456); remote parity/live fixture pending |

### Archive

| Method | §12 | Parity | Evidence |
| --- | --- | --- | --- |
| `archive_import` | U | protocol-covered | §12 `U` (task 460); generated dispatch + client + codegen `--check` at 332 (task 456); loom-client round-trip unit test `archive_export_import_roundtrips_sync_and_async` (task 456); remote parity/live fixture pending |
| `archive_export` | U | protocol-covered | §12 `U` (task 460); generated dispatch + client + codegen `--check` at 332 (task 456); loom-client round-trip unit test (task 456); remote parity/live fixture pending |
| `archive_import_async` | Ua | protocol-covered | §12 `Ua` (task 460); generated dispatch + client + codegen `--check` at 332 (task 456); loom-client immediate-complete task unit test (task 456); remote parity/live fixture pending |
| `archive_export_async` | Ua | protocol-covered | §12 `Ua` (task 460); generated dispatch + client + codegen `--check` at 332 (task 456); loom-client immediate-complete task unit test (task 456); remote parity/live fixture pending |

### Car

| Method | §12 | Parity | Evidence |
| --- | --- | --- | --- |
| `car_import` | U | protocol-covered | §12 `U`, store-wide (task 460); generated dispatch + client + codegen `--check` at 332 (task 456); loom-client cross-store round-trip unit test `car_export_import_roundtrips_sync_and_async` (task 456); remote parity/live fixture pending |
| `car_export` | U | protocol-covered | §12 `U` (task 460); generated dispatch + client + codegen `--check` at 332 (task 456); loom-client round-trip unit test (task 456); remote parity/live fixture pending |
| `car_import_async` | Ua | protocol-covered | §12 `Ua` (task 460); generated dispatch + client + codegen `--check` at 332 (task 456); loom-client immediate-complete task unit test (task 456); remote parity/live fixture pending |
| `car_export_async` | Ua | protocol-covered | §12 `Ua` (task 460); generated dispatch + client + codegen `--check` at 332 (task 456); loom-client immediate-complete task unit test (task 456); remote parity/live fixture pending |

### FileHandle

| Method | §12 | Parity | Evidence |
| --- | --- | --- | --- |
| `open` | U | protocol-covered | §12 `U`; generated dispatch (R2.2) + client stubs (R2.3) + codegen `--check` + carrier tests (R4c); client-parity fixture is follow-up |
| `read` | Uc | protocol-covered | §12 `Uc`; generated dispatch (R2.2) + client stubs (R2.3) + codegen `--check` + carrier tests (R4c); client-parity fixture is follow-up |
| `read_at` | Uc | protocol-covered | §12 `Uc`; generated dispatch (R2.2) + client stubs (R2.3) + codegen `--check` + carrier tests (R4c); client-parity fixture is follow-up |
| `write` | Uc | protocol-covered | §12 `Uc`; generated dispatch (R2.2) + client stubs (R2.3) + codegen `--check` + carrier tests (R4c); client-parity fixture is follow-up |
| `write_at` | Uc | protocol-covered | §12 `Uc`; generated dispatch (R2.2) + client stubs (R2.3) + codegen `--check` + carrier tests (R4c); client-parity fixture is follow-up |
| `truncate` | U | protocol-covered | §12 `U`; generated dispatch (R2.2) + client stubs (R2.3) + codegen `--check` + carrier tests (R4c); client-parity fixture is follow-up |
| `flush` | U | protocol-covered | §12 `U`; generated dispatch (R2.2) + client stubs (R2.3) + codegen `--check` + carrier tests (R4c); client-parity fixture is follow-up |
| `stat` | U | protocol-covered | §12 `U`; generated dispatch (R2.2) + client stubs (R2.3) + codegen `--check` + carrier tests (R4c); client-parity fixture is follow-up |
| `close` | U | protocol-covered | §12 `U`; generated dispatch (R2.2) + client stubs (R2.3) + codegen `--check` + carrier tests (R4c); client-parity fixture is follow-up |

### Cas

| Method | §12 | Parity | Evidence |
| --- | --- | --- | --- |
| `put` | Uc | suite-covered | client parity suite `run_client_parity_suite` - byte-parity local vs remote |
| `get` | Uc | suite-covered | client parity suite `run_client_parity_suite` - byte-parity local vs remote |
| `has` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `delete` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `list` | Uc | live+protocol-covered | §12 `Uc`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |

### Kv

| Method | §12 | Parity | Evidence |
| --- | --- | --- | --- |
| `put` | U | suite-covered | client parity suite `run_client_parity_suite` - byte-parity local vs remote |
| `get` | U | suite-covered | client parity suite `run_client_parity_suite` - byte-parity local vs remote |
| `delete` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `list` | Uc | live+protocol-covered | §12 `Uc`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `range` | Uc | live+protocol-covered | §12 `Uc`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `list_collections` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |

### Graph

| Method | §12 | Parity | Evidence |
| --- | --- | --- | --- |
| `upsert_node` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `get_node` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `remove_node` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `upsert_edge` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `upsert_edge_indexed` | - | live+protocol-covered | added after the first draft; live-verified via task 395/397/350; classified in §12 per its interface (task 460) |
| `get_edge` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `remove_edge` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `remove_edge_indexed` | - | live+protocol-covered | added after the first draft; live-verified via task 395/397/350; classified in §12 per its interface (task 460) |
| `neighbors` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `out_edges` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `in_edges` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `reachable` | Uc | live+protocol-covered | §12 `Uc`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `shortest_path` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `query` | Uc | live+protocol-covered | §12 `Uc`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `explain_query` | Uc | live+protocol-covered | §12 `Uc`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |

### Vector

| Method | §12 | Parity | Evidence |
| --- | --- | --- | --- |
| `create` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `upsert` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `upsert_source` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `get` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `source_text` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `embedding_model` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `ids` | Uc | live+protocol-covered | §12 `Uc`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `metadata_index_keys` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `create_metadata_index` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `drop_metadata_index` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `delete` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `search` | Uc | live+protocol-covered | §12 `Uc`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `search_policy` | Uc | live+protocol-covered | §12 `Uc`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |

### Columnar

| Method | §12 | Parity | Evidence |
| --- | --- | --- | --- |
| `create` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `append` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `compact` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `inspect` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `source_digest` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `scan` | Uc | live+protocol-covered | §12 `Uc`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `columns` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `rows` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `select` | Uc | live+protocol-covered | §12 `Uc`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `aggregate` | Uc | live+protocol-covered | §12 `Uc`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |

### Dataframe

| Method | §12 | Parity | Evidence |
| --- | --- | --- | --- |
| `create` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `collect` | Uc | live+protocol-covered | §12 `Uc`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `preview` | Uc | live+protocol-covered | §12 `Uc`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `materialize` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `plan_digest` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `source_digests` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |

### Search

| Method | §12 | Parity | Evidence |
| --- | --- | --- | --- |
| `create` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `index` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `get` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `delete` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `ids` | Uc | live+protocol-covered | §12 `Uc`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `remap` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `query` | Uc | live+protocol-covered | §12 `Uc`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `source_digest` | U | live+protocol-covered | §12 `U` (task 580); local-vs-remote byte parity over a served self-signed-TLS endpoint via `run_client_parity_suite`/`client_parity_local_matches_remote` (task 600; index a fixed doc, compare the source digest); loom-client unit test (task 580) |
| `status` | U | live+protocol-covered | §12 `U` (task 580); returns canonical `[source_digest, DerivedArtifactStatus]` bytes (loom-store codec, round-trip test GREEN); local-vs-remote byte parity via `client_parity_local_matches_remote` (task 600; never-rebuilt status is deterministic); `LocalLoomClient::search_status` + `StoreClient::search_status` facade + `SearchCmd::Status` routing |

### ManagementKv

| Method | §12 | Parity | Evidence |
| --- | --- | --- | --- |
| `set_config` | U | protocol-covered | §12 `U`; generated dispatch (R2.2) + client stubs (R2.3) + codegen `--check` + carrier tests (R4c); client-parity fixture is follow-up |
| `get_config` | U | protocol-covered | §12 `U`; generated dispatch (R2.2) + client stubs (R2.3) + codegen `--check` + carrier tests (R4c); client-parity fixture is follow-up |

### Document

| Method | §12 | Parity | Evidence |
| --- | --- | --- | --- |
| `put_binary_indexed` | - | live+protocol-covered | added after the first draft; live-verified via task 395/397/350; classified in §12 per its interface (task 460) |
| `put_text` | U | live+protocol-covered | §12 `U` (task DOC-TEXT-001B); local-vs-remote byte parity over a served self-signed-TLS endpoint via `run_client_parity_suite`/`client_parity_local_matches_remote` (task 600); loom-client round-trip unit test `document_text_binary_roundtrip_and_errors` |
| `get_text` | U | live+protocol-covered | §12 `U` (task DOC-TEXT-001B); local-vs-remote parity via `client_parity_local_matches_remote` (task 600); loom-client + remote stub unit tests (incl. `DOCUMENT_NOT_TEXT`) |
| `put_binary` | U | live+protocol-covered | §12 `U` (task DOC-TEXT-001B); local-vs-remote parity via `client_parity_local_matches_remote` (task 600); loom-client + remote stub unit tests (incl. CAS-mismatch guard) |
| `get_binary` | U | live+protocol-covered | §12 `U` (task DOC-TEXT-001B); local-vs-remote parity via `client_parity_local_matches_remote` (task 600); loom-client + remote stub unit tests |
| `list_binary` | U | live+protocol-covered | §12 `U` (task DOC-TEXT-001B); local-vs-remote parity via `client_parity_local_matches_remote` (task 600); loom-client + remote stub unit tests |
| `delete` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `delete_indexed` | - | live+protocol-covered | added after the first draft; live-verified via task 395/397/350; classified in §12 per its interface (task 460) |
| `replace_text_indexed` | - | live+protocol-covered | added after the first draft; live-verified via task 395/397/350; classified in §12 per its interface (task 460) |
| `list` | Uc | live+protocol-covered | §12 `Uc`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `list_collections` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `index_create` | - | live+protocol-covered | added after the first draft; live-verified via task 395/397/350; classified in §12 per its interface (task 460) |
| `index_drop` | - | live+protocol-covered | added after the first draft; live-verified via task 395/397/350; classified in §12 per its interface (task 460) |
| `index_rebuild` | - | live+protocol-covered | added after the first draft; live-verified via task 395/397/350; classified in §12 per its interface (task 460) |
| `index_list_json` | - | live+protocol-covered | added after the first draft; live-verified via task 395/397/350; classified in §12 per its interface (task 460) |
| `index_status_json` | - | live+protocol-covered | added after the first draft; live-verified via task 395/397/350; classified in §12 per its interface (task 460) |
| `find_json` | - | live+protocol-covered | added after the first draft; live-verified via task 395/397/350; classified in §12 per its interface (task 460) |
| `query_json` | - | suite-covered | client parity suite `run_client_parity_suite` - byte-parity local vs remote |

### Calendar

| Method | §12 | Parity | Evidence |
| --- | --- | --- | --- |
| `create_collection` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `get_collection` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `list_collections` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `delete_collection` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `put_entry` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `put_ics` | U | live+protocol-covered | §12 `U` (task 540); local-vs-remote byte parity over a served self-signed-TLS endpoint via `run_client_parity_suite`/`client_parity_local_matches_remote` (task 600; create-collection then import, etag compared); loom-client unit test `pim_search_vcs_growth_methods_are_wired` (task 540) |
| `get_entry` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `delete_entry` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `list_entries` | Uc | live+protocol-covered | §12 `Uc`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `range` | Uc | live+protocol-covered | §12 `Uc`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `search` | Uc | live+protocol-covered | §12 `Uc`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `to_ics` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |

### Contacts

| Method | §12 | Parity | Evidence |
| --- | --- | --- | --- |
| `create_book` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `get_book` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `list_books` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `delete_book` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `put_entry` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `put_vcard` | U | live+protocol-covered | §12 `U` (task 540); local-vs-remote byte parity over a served self-signed-TLS endpoint via `run_client_parity_suite`/`client_parity_local_matches_remote` (task 600; create-book then import, etag compared); loom-client unit test (task 540) |
| `get_entry` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `delete_entry` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `list_entries` | Uc | live+protocol-covered | §12 `Uc`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `search` | Uc | live+protocol-covered | §12 `Uc`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `to_vcard` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |

### Mail

| Method | §12 | Parity | Evidence |
| --- | --- | --- | --- |
| `create_mailbox` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `get_mailbox` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `list_mailboxes` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `delete_mailbox` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `ingest_message` | Uc | live+protocol-covered | §12 `Uc`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `get_message` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `to_eml` | Uc | live+protocol-covered | §12 `Uc`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `delete_message` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `list_messages` | Uc | live+protocol-covered | §12 `Uc`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `get_flags` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `set_flags` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `search` | Uc | live+protocol-covered | §12 `Uc`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |

### TimeSeries

| Method | §12 | Parity | Evidence |
| --- | --- | --- | --- |
| `put` | U | suite-covered | client parity suite `run_client_parity_suite` - byte-parity local vs remote |
| `get` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `range` | Uc | live+protocol-covered | §12 `Uc`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `latest` | U | suite-covered | client parity suite `run_client_parity_suite` - byte-parity local vs remote |
| `list_collections` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |

### Metrics

| Method | §12 | Parity | Evidence |
| --- | --- | --- | --- |
| `put_descriptor` | U | live+protocol-covered | §12 `U` (task 460); local-vs-remote byte parity over a served self-signed-TLS endpoint via `run_client_parity_suite`/`client_parity_local_matches_remote` (task 600); loom-client round-trip unit test `metrics_descriptor_observation_and_query_roundtrip` (task 460) |
| `get_descriptor` | U | live+protocol-covered | §12 `U` (task 460); local-vs-remote parity via `client_parity_local_matches_remote` (task 600); loom-client round-trip unit test (task 460) |
| `put_observation` | U | live+protocol-covered | §12 `U` (task 460); local-vs-remote parity via `client_parity_local_matches_remote` (task 600); loom-client round-trip unit test (task 460) |
| `query` | U | live+protocol-covered | §12 `U` (task 460); local-vs-remote parity via `client_parity_local_matches_remote` (task 600; fixed descriptor+observation, window `[0,10)`); loom-client round-trip unit test (task 460) |

### Ledger

| Method | §12 | Parity | Evidence |
| --- | --- | --- | --- |
| `append` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `get` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `head` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `len` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `verify` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `list_collections` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |

### Queue

| Method | §12 | Parity | Evidence |
| --- | --- | --- | --- |
| `append` | U | suite-covered | client parity suite `run_client_parity_suite` - byte-parity local vs remote |
| `get` | U | suite-covered | client parity suite `run_client_parity_suite` - byte-parity local vs remote |
| `range` | Uc | live+protocol-covered | §12 `Uc`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `len` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `list_streams` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |

### QueueConsumers

| Method | §12 | Parity | Evidence |
| --- | --- | --- | --- |
| `consumer_position` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `consumer_read` | Uc | live+protocol-covered | §12 `Uc`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `consumer_advance` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `consumer_reset` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |

### Sql

| Method | §12 | Parity | Evidence |
| --- | --- | --- | --- |
| `sql_open` | Cn | suite-covered | client parity suite `run_client_parity_suite` - byte-parity local vs remote |
| `sql_open_keyed` | Cn | session/connection | §12 transport `Cn` - remote session establishment; exercised by every live test + carrier session-open (task 350a-0) |
| `sql_open_with_kek` | Cn | session/connection | §12 transport `Cn` - remote session establishment; exercised by every live test + carrier session-open (task 350a-0) |
| `sql_open_authenticated` | Cn | session/connection | §12 transport `Cn` - remote session establishment; exercised by every live test + carrier session-open (task 350a-0) |
| `sql_open_keyed_authenticated` | Cn | session/connection | §12 transport `Cn` - remote session establishment; exercised by every live test + carrier session-open (task 350a-0) |
| `sql_open_with_kek_authenticated` | Cn | session/connection | §12 transport `Cn` - remote session establishment; exercised by every live test + carrier session-open (task 350a-0) |
| `sql_authenticate_passphrase` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `sql_exec` | U | suite-covered | client parity suite `run_client_parity_suite` - byte-parity local vs remote |
| `sql_query` | St | live+protocol-covered | §12 `St`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `sql_commit` | U | suite-covered | client parity suite `run_client_parity_suite` - byte-parity local vs remote |
| `sql_close` | Cn | suite-covered | client parity suite `run_client_parity_suite` - byte-parity local vs remote |
| `sql_batch_begin` | Cn | session/connection | §12 transport `Cn` - remote session establishment; exercised by every live test + carrier session-open (task 350a-0) |
| `sql_batch_begin_keyed` | Cn | session/connection | §12 transport `Cn` - remote session establishment; exercised by every live test + carrier session-open (task 350a-0) |
| `sql_batch_begin_with_kek` | Cn | session/connection | §12 transport `Cn` - remote session establishment; exercised by every live test + carrier session-open (task 350a-0) |
| `sql_batch_begin_authenticated` | Cn | session/connection | §12 transport `Cn` - remote session establishment; exercised by every live test + carrier session-open (task 350a-0) |
| `sql_batch_begin_keyed_authenticated` | Cn | session/connection | §12 transport `Cn` - remote session establishment; exercised by every live test + carrier session-open (task 350a-0) |
| `sql_batch_begin_with_kek_authenticated` | Cn | session/connection | §12 transport `Cn` - remote session establishment; exercised by every live test + carrier session-open (task 350a-0) |
| `sql_batch_exec` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `sql_batch_commit` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `sql_batch_commit_vcs` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `sql_batch_abort` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `sql_batch_close` | Cn | session/connection | §12 transport `Cn` - remote session establishment; exercised by every live test + carrier session-open (task 350a-0) |
| `sql_read_table` | Uc | live+protocol-covered | §12 `Uc`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `sql_read_table_at` | Uc | live+protocol-covered | §12 `Uc`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `sql_index_scan` | Uc | live+protocol-covered | §12 `Uc`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `sql_index_scan_at` | Uc | live+protocol-covered | §12 `Uc`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `sql_blame` | Uc | live+protocol-covered | §12 `Uc`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `sql_diff` | Uc | live+protocol-covered | §12 `Uc`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `sql_table_diff` | Uc | live+protocol-covered | §12 `Uc`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `sql_read_table_async` | Ua | live+protocol-covered | §12 `Ua`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `sql_index_scan_async` | Ua | live+protocol-covered | §12 `Ua`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `sql_blame_async` | Ua | live+protocol-covered | §12 `Ua`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `sql_diff_async` | Ua | live+protocol-covered | §12 `Ua`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `sql_list_databases` | U | live+protocol-covered | §12 `U`; end-to-end via MCP live (task 370) / facade live (task 350) + generated dispatch (R2.2) + codegen `--check` |
| `sql_query_result` | - | suite-covered | client parity suite `run_client_parity_suite` - byte-parity local vs remote |

### Diagnostics

| Method | §12 | Parity | Evidence |
| --- | --- | --- | --- |
| `result_to_json` | Lo | local-only | §12 transport `Lo` - never crosses the wire |
| `result_to_bridge_json` | Lo | local-only | §12 transport `Lo` - never crosses the wire |
| `last_error` | Lo | local-only | §12 transport `Lo` - never crosses the wire |

### Tasks

| Method | §12 | Parity | Evidence |
| --- | --- | --- | --- |
| `iter_next` | Uc | protocol-covered | §12 `Uc`; generated dispatch (R2.2) + client stubs (R2.3) + codegen `--check` + carrier tests (R4c); client-parity fixture is follow-up |
| `iter_free` | Lo | local-only | §12 transport `Lo` - never crosses the wire |
| `sql_exec_async` | Ua | protocol-covered | §12 `Ua`; generated dispatch (R2.2) + client stubs (R2.3) + codegen `--check` + carrier tests (R4c); client-parity fixture is follow-up |
| `task_poll` | U | protocol-covered | §12 `U`; generated dispatch (R2.2) + client stubs (R2.3) + codegen `--check` + carrier tests (R4c); client-parity fixture is follow-up |
| `task_status` | U | protocol-covered | §12 `U`; generated dispatch (R2.2) + client stubs (R2.3) + codegen `--check` + carrier tests (R4c); client-parity fixture is follow-up |
| `task_result` | U | protocol-covered | §12 `U`; generated dispatch (R2.2) + client stubs (R2.3) + codegen `--check` + carrier tests (R4c); client-parity fixture is follow-up |
| `task_cancel` | U | protocol-covered | §12 `U`; generated dispatch (R2.2) + client stubs (R2.3) + codegen `--check` + carrier tests (R4c); client-parity fixture is follow-up |
| `task_free` | Lo | local-only | §12 transport `Lo` - never crosses the wire |
| `task_wait` | U | protocol-covered | §12 `U`; generated dispatch (R2.2) + client stubs (R2.3) + codegen `--check` + carrier tests (R4c); client-parity fixture is follow-up |

### ResultViews

| Method | §12 | Parity | Evidence |
| --- | --- | --- | --- |
| `result_open` | Lo | local-only | §12 transport `Lo` - never crosses the wire |
| `row_open` | Lo | local-only | §12 transport `Lo` - never crosses the wire |
| `result_close` | Lo | local-only | §12 transport `Lo` - never crosses the wire |
| `result_len` | Lo | local-only | §12 transport `Lo` - never crosses the wire |
| `result_is_statements` | Lo | local-only | §12 transport `Lo` - never crosses the wire |
| `result_item_kind` | Lo | local-only | §12 transport `Lo` - never crosses the wire |
| `result_column_count` | Lo | local-only | §12 transport `Lo` - never crosses the wire |
| `result_column_name` | Lo | local-only | §12 transport `Lo` - never crosses the wire |
| `result_column_type` | Lo | local-only | §12 transport `Lo` - never crosses the wire |
| `result_row_count` | Lo | local-only | §12 transport `Lo` - never crosses the wire |
| `result_row_len` | Lo | local-only | §12 transport `Lo` - never crosses the wire |
| `result_cell` | Lo | local-only | §12 transport `Lo` - never crosses the wire |
| `result_row_commit` | Lo | local-only | §12 transport `Lo` - never crosses the wire |
| `result_count` | Lo | local-only | §12 transport `Lo` - never crosses the wire |
| `result_string_count` | Lo | local-only | §12 transport `Lo` - never crosses the wire |
| `result_string` | Lo | local-only | §12 transport `Lo` - never crosses the wire |
| `result_variable_kind` | Lo | local-only | §12 transport `Lo` - never crosses the wire |
| `result_merge_outcome` | Lo | local-only | §12 transport `Lo` - never crosses the wire |
| `result_diff_count` | Lo | local-only | §12 transport `Lo` - never crosses the wire |
| `result_diff_change` | Lo | local-only | §12 transport `Lo` - never crosses the wire |
| `result_diff_len` | Lo | local-only | §12 transport `Lo` - never crosses the wire |
| `result_diff_cell` | Lo | local-only | §12 transport `Lo` - never crosses the wire |
| `result_map_len` | Lo | local-only | §12 transport `Lo` - never crosses the wire |
| `result_map_entry` | Lo | local-only | §12 transport `Lo` - never crosses the wire |

### Triggers

| Method | §12 | Parity | Evidence |
| --- | --- | --- | --- |
| `trigger_put` | U | protocol-covered | §12 `U`; generated dispatch (R2.2) + client stubs (R2.3) + codegen `--check` + carrier tests (R4c); client-parity fixture is follow-up |
| `trigger_get` | U | protocol-covered | §12 `U`; generated dispatch (R2.2) + client stubs (R2.3) + codegen `--check` + carrier tests (R4c); client-parity fixture is follow-up |
| `trigger_list` | Uc | protocol-covered | §12 `Uc`; generated dispatch (R2.2) + client stubs (R2.3) + codegen `--check` + carrier tests (R4c); client-parity fixture is follow-up |
| `trigger_enable` | U | protocol-covered | §12 `U`; generated dispatch (R2.2) + client stubs (R2.3) + codegen `--check` + carrier tests (R4c); client-parity fixture is follow-up |
| `trigger_remove` | U | protocol-covered | §12 `U`; generated dispatch (R2.2) + client stubs (R2.3) + codegen `--check` + carrier tests (R4c); client-parity fixture is follow-up |
| `trigger_history` | Uc | protocol-covered | §12 `Uc`; generated dispatch (R2.2) + client stubs (R2.3) + codegen `--check` + carrier tests (R4c); client-parity fixture is follow-up |

### Logs

| Method | §12 | Parity | Evidence |
| --- | --- | --- | --- |
| `put_record` | U | live+protocol-covered | local-vs-remote byte parity over a served self-signed-TLS endpoint via `run_client_parity_suite`/`client_parity_local_matches_remote` (task 600) |
| `get_record` | U | live+protocol-covered | local-vs-remote parity via `client_parity_local_matches_remote` (task 600) |
| `query` | U | live+protocol-covered | local-vs-remote parity via `client_parity_local_matches_remote` (task 600) |

### Traces

| Method | §12 | Parity | Evidence |
| --- | --- | --- | --- |
| `put_span` | U | live+protocol-covered | local-vs-remote byte parity over a served self-signed-TLS endpoint via `run_client_parity_suite`/`client_parity_local_matches_remote` (task 600) |
| `get_span` | U | live+protocol-covered | local-vs-remote parity via `client_parity_local_matches_remote` (task 600) |
| `query` | U | live+protocol-covered | local-vs-remote parity via `client_parity_local_matches_remote` (task 600) |
| `trace_spans` | U | live+protocol-covered | local-vs-remote parity via `client_parity_local_matches_remote` (task 600) |

### Program

| Method | §12 | Parity | Evidence |
| --- | --- | --- | --- |
| `program_put` | U | protocol-covered | generated dispatch + client + codegen `--check`; no dedicated client-parity fixture yet |
| `program_get` | U | protocol-covered | generated dispatch + client + codegen `--check`; no dedicated client-parity fixture yet |
| `program_list` | U | protocol-covered | generated dispatch + client + codegen `--check`; no dedicated client-parity fixture yet |
| `program_inspect` | U | protocol-covered | generated dispatch + client + codegen `--check`; no dedicated client-parity fixture yet |
| `program_remove` | U | protocol-covered | generated dispatch + client + codegen `--check`; no dedicated client-parity fixture yet |

## Follow-up methods (recorded, not hidden)

These 12 filesystem/archive/CAR methods were added to `idl/loom.idl` after §12 was first drafted. As of
tasks 456 and 460 they are classified in §12 (FileSystem `U`/`Ua`, and the new `Archive` and `Car`
interface tables), implemented on `LocalLoomClient` through `loom-interchange-io`, cross the generated
dispatch and client at 332 (codegen `--check` clean), and carry loom-client round-trip unit tests
(`filesystem_import_export_roundtrips_sync_and_async`, `archive_export_import_roundtrips_sync_and_async`,
`car_export_import_roundtrips_sync_and_async`). They are therefore `protocol-covered`. The remaining
follow-up for each is a **remote parity/live fixture** (end-to-end over a served endpoint), not yet
written:

- `FileSystem.import_fs`
- `FileSystem.export_fs`
- `FileSystem.import_fs_async`
- `FileSystem.export_fs_async`
- `Archive.archive_import`
- `Archive.archive_export`
- `Archive.archive_import_async`
- `Archive.archive_export_async`
- `Car.car_import`
- `Car.car_export`
- `Car.car_import_async`
- `Car.car_export_async`
