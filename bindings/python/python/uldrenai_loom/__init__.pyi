from typing import Any

class Fence:
    authority: int
    epoch: int
    sequence: int
    def low(self) -> int: ...
    def high(self) -> int: ...

class LockToken:
    key: str
    principal: str
    session: str
    mode: str
    permits: int
    capacity: int
    fence: Fence
    lease_deadline_ms: int

class LockGuard:
    path: str
    token: LockToken
    def __init__(self, path: str, token: LockToken) -> None: ...
    def refresh(self, lease_ms: int = 60000) -> LockToken: ...
    def release(self) -> None: ...
    def __enter__(self) -> "LockGuard": ...
    def __exit__(self, exc_type: Any, exc: Any, tb: Any) -> None: ...

def version() -> str:
    """The library version."""
    ...

def capabilities() -> bytes:
    """The build capability report (0010 section 5) as canonical CBOR: a CapabilitySet map with
    schema_version and records. Build-aware: capabilities owned by the linked crates are reported
    with operational state supported."""
    ...

def runtime_profile() -> bytes:
    """The runtime provider/profile report as canonical CBOR."""
    ...

def studio_surface_catalog_json(workspace: str, set: str = "all") -> str: ...

def meetings_import_snapshot(
    path: str,
    workspace: str,
    input_profile: str,
    snapshot: bytes,
    dry_run: bool,
    passphrase: str | None = None,
) -> str: ...

def meetings_source_read(
    path: str,
    workspace: str,
    source_id: str,
    leaf: str,
    passphrase: str | None = None,
) -> bytes: ...

def drive_list_json(path: str, workspace: str, drive_workspace_id: str, folder_id: str, passphrase: str | None = None) -> str: ...

def drive_stat_json(path: str, workspace: str, drive_workspace_id: str, folder_id: str, name: str, passphrase: str | None = None) -> str: ...

def drive_read_file(path: str, workspace: str, drive_workspace_id: str, file_id: str, passphrase: str | None = None) -> bytes: ...

def drive_list_versions_json(path: str, workspace: str, drive_workspace_id: str, file_id: str, passphrase: str | None = None) -> str: ...

def drive_list_conflicts_json(path: str, workspace: str, drive_workspace_id: str, passphrase: str | None = None) -> str: ...

def drive_list_shares_json(path: str, workspace: str, drive_workspace_id: str, passphrase: str | None = None) -> str: ...

def drive_list_retention_json(path: str, workspace: str, drive_workspace_id: str, passphrase: str | None = None) -> str: ...

def drive_create_folder_json(path: str, workspace: str, drive_workspace_id: str, parent_folder_id: str, folder_id: str, name: str, expected_root: str, passphrase: str | None = None) -> str: ...

def drive_create_upload_json(path: str, workspace: str, drive_workspace_id: str, upload_id: str, parent_folder_id: str, name: str, file_id: str, expected_root: str, created_at_ms: int, replace_file: bool, passphrase: str | None = None) -> str: ...

def drive_upload_chunk_json(path: str, workspace: str, drive_workspace_id: str, upload_id: str, chunk: bytes, passphrase: str | None = None) -> str: ...

def drive_commit_upload_json(path: str, workspace: str, drive_workspace_id: str, upload_id: str, passphrase: str | None = None) -> str: ...

def drive_rename_json(path: str, workspace: str, drive_workspace_id: str, folder_id: str, node_id: str, new_name: str, expected_root: str, passphrase: str | None = None) -> str: ...

def drive_move_json(path: str, workspace: str, drive_workspace_id: str, source_folder_id: str, target_folder_id: str, node_id: str, expected_root: str, passphrase: str | None = None) -> str: ...

def drive_delete_json(path: str, workspace: str, drive_workspace_id: str, folder_id: str, node_id: str, expected_root: str, passphrase: str | None = None) -> str: ...

def drive_resolve_conflict_json(path: str, workspace: str, drive_workspace_id: str, conflict_id: str, resolution: str, passphrase: str | None = None) -> str: ...

def drive_grant_share_json(path: str, workspace: str, drive_workspace_id: str, grant_id: str, target_kind: str, target_id: str, principal: str, role: str, granted_at_ms: int, expires_at_ms: int | None = None, passphrase: str | None = None) -> str: ...

def drive_revoke_share_json(path: str, workspace: str, drive_workspace_id: str, grant_id: str, passphrase: str | None = None) -> str: ...

def drive_apply_share_expiry_json(path: str, workspace: str, drive_workspace_id: str, now_ms: int, passphrase: str | None = None) -> str: ...

def drive_pin_retention_json(path: str, workspace: str, drive_workspace_id: str, pin_id: str, kind: str, root: str, target_entity_id: str | None, added_at_ms: int, expires_at_ms: int | None = None, passphrase: str | None = None) -> str: ...

def drive_unpin_retention_json(path: str, workspace: str, drive_workspace_id: str, pin_id: str, passphrase: str | None = None) -> str: ...

def drive_apply_retention_json(path: str, workspace: str, drive_workspace_id: str, now_ms: int, passphrase: str | None = None) -> str: ...

def tickets_project_create_json(path: str, workspace: str, ticket_workspace_id: str, project_id: str, key_prefix: str, name: str, expected_root: str | None = None, passphrase: str | None = None) -> str: ...

def tickets_project_rekey_json(path: str, workspace: str, ticket_workspace_id: str, project_id: str, key_prefix: str, expected_root: str | None = None, passphrase: str | None = None) -> str: ...

def tickets_project_settings_get_json(path: str, workspace: str, ticket_workspace_id: str, project_id: str, passphrase: str | None = None) -> str: ...

def tickets_project_settings_set_json(path: str, workspace: str, ticket_workspace_id: str, project_id: str, default_projection: str | None = None, enable_projections_json: str = "[]", disable_projections_json: str = "[]", actor_enforcement: str | None = None, project_owner_principal: str | None = None, clear_project_owner_principal: bool = False, acceptance_authorities_json: str | None = None, expected_root: str | None = None, passphrase: str | None = None) -> str: ...

def tickets_fields_json(path: str, workspace: str, ticket_workspace_id: str, project_id: str | None = None, projection: str | None = None, operation: str | None = None, passphrase: str | None = None) -> str: ...

def tickets_field_put_json(path: str, workspace: str, ticket_workspace_id: str, project_id: str, field_id: str, key: str, name: str, description: str | None = None, field_type: str = "string", option_set: str | None = None, max_length: int | None = None, required: bool = False, searchable: bool = True, orderable: bool = False, cardinality: str = "optional", applicable_type_ids_json: str = "[]", expected_root: str | None = None, passphrase: str | None = None) -> str: ...

def tickets_field_retire_json(path: str, workspace: str, ticket_workspace_id: str, project_id: str, field_id: str, expected_root: str | None = None, passphrase: str | None = None) -> str: ...

def tickets_create_json(path: str, workspace: str, ticket_workspace_id: str, project_id: str, ticket_type: str, external_source: str | None, external_id: str | None, fields_json: str, policy_labels_json: str, expected_root: str | None = None, passphrase: str | None = None) -> str: ...

def tickets_update_json(path: str, workspace: str, ticket_workspace_id: str, ticket_id: str, set_fields_json: str | None, delete_fields_json: str, action: str | None = None, target_status: str | None = None, observed_source_status: str | None = None, observed_workflow_version: str | None = None, assignee: str | None = None, comment_id: str | None = None, comment_type: str | None = None, comment_body: str | None = None, expected_root: str | None = None, passphrase: str | None = None, comments_json: str | None = None, relation_sets_json: str | None = None, relation_removes_json: str | None = None) -> str: ...

def tickets_delete_json(path: str, workspace: str, ticket_workspace_id: str, ticket_id: str, expected_root: str | None = None, passphrase: str | None = None) -> str: ...

def tickets_comments_json(path: str, workspace: str, ticket_workspace_id: str, ticket_id: str, passphrase: str | None = None) -> str: ...

def tickets_comment_add_json(path: str, workspace: str, ticket_workspace_id: str, ticket_id: str, comment_id: str | None, comment_type: str | None, body: str, expected_root: str | None = None, passphrase: str | None = None) -> str: ...

def tickets_comment_update_json(path: str, workspace: str, ticket_workspace_id: str, ticket_id: str, comment_id: str, comment_type: str | None = None, body: str | None = None, expected_root: str | None = None, passphrase: str | None = None) -> str: ...

def tickets_comment_delete_json(path: str, workspace: str, ticket_workspace_id: str, ticket_id: str, comment_id: str, expected_root: str | None = None, passphrase: str | None = None) -> str: ...

def tickets_relation_set_json(path: str, workspace: str, ticket_workspace_id: str, ticket_id: str, relation_id: str | None, kind: str, target_id: str, expected_root: str | None = None, passphrase: str | None = None) -> str: ...

def tickets_relation_remove_json(path: str, workspace: str, ticket_workspace_id: str, ticket_id: str, relation_id: str, expected_root: str | None = None, passphrase: str | None = None) -> str: ...

def tickets_relation_list_json(path: str, workspace: str, ticket_workspace_id: str, ticket_id: str, passphrase: str | None = None) -> str: ...

def tickets_get_json(path: str, workspace: str, ticket_workspace_id: str, ticket_id: str, projection: str | None = None, passphrase: str | None = None) -> str: ...

def tickets_list_json(path: str, workspace: str, ticket_workspace_id: str, projection: str | None = None, passphrase: str | None = None) -> str: ...

def tickets_history_json(path: str, workspace: str, ticket_workspace_id: str, ticket_id: str | None = None, passphrase: str | None = None) -> str: ...

def chat_create_channel_json(path: str, workspace: str, chat_workspace_id: str, channel_id: str, channel_handle: str, name: str, passphrase: str | None = None) -> str: ...

def chat_rename_channel_json(path: str, workspace: str, chat_workspace_id: str, selector: str, channel_handle: str, passphrase: str | None = None) -> str: ...

def chat_list_channels_json(path: str, workspace: str, chat_workspace_id: str, passphrase: str | None = None) -> str: ...

def chat_post_message_json(path: str, workspace: str, chat_workspace_id: str, channel_id: str, message_id: str, thread_id: str | None, body_text: str, passphrase: str | None = None) -> str: ...

def chat_edit_message_json(path: str, workspace: str, chat_workspace_id: str, channel_id: str, message_id: str, body_text: str, passphrase: str | None = None) -> str: ...

def chat_redact_message_json(path: str, workspace: str, chat_workspace_id: str, channel_id: str, message_id: str, reason: str | None = None, passphrase: str | None = None) -> str: ...

def chat_create_thread_json(path: str, workspace: str, chat_workspace_id: str, channel_id: str, thread_id: str, parent_message_id: str, passphrase: str | None = None) -> str: ...

def chat_create_task_json(path: str, workspace: str, chat_workspace_id: str, channel_id: str, task_id: str, message_id: str | None, title: str, passphrase: str | None = None) -> str: ...

def chat_claim_task_json(path: str, workspace: str, chat_workspace_id: str, channel_id: str, task_id: str, claim_id: str, lease_token: str | None = None, passphrase: str | None = None) -> str: ...

def chat_complete_task_json(path: str, workspace: str, chat_workspace_id: str, channel_id: str, task_id: str, claim_id: str, result_message_id: str | None = None, passphrase: str | None = None) -> str: ...

def chat_invoke_agent_json(path: str, workspace: str, chat_workspace_id: str, channel_id: str, invocation_id: str, agent_principal: str, source_message_ids_json: str, prompt_text: str, passphrase: str | None = None) -> str: ...

def chat_agent_reply_json(path: str, workspace: str, chat_workspace_id: str, channel_id: str, invocation_id: str, message_id: str, passphrase: str | None = None) -> str: ...

def chat_request_handoff_json(path: str, workspace: str, chat_workspace_id: str, channel_id: str, handoff_id: str, from_agent_principal: str, to_principal: str | None = None, reason: str | None = None, passphrase: str | None = None) -> str: ...

def chat_add_reaction_json(path: str, workspace: str, chat_workspace_id: str, channel_id: str, message_id: str, kind: str, passphrase: str | None = None) -> str: ...

def chat_remove_reaction_json(path: str, workspace: str, chat_workspace_id: str, channel_id: str, message_id: str, kind: str, passphrase: str | None = None) -> str: ...

def chat_emoji_list_json(path: str, workspace: str, chat_workspace_id: str, passphrase: str | None = None) -> str: ...

def chat_emoji_register_json(path: str, workspace: str, chat_workspace_id: str, kind: str, passphrase: str | None = None) -> str: ...

def chat_emoji_unregister_json(path: str, workspace: str, chat_workspace_id: str, kind: str, passphrase: str | None = None) -> str: ...

def chat_messages_json(path: str, workspace: str, chat_workspace_id: str, channel_id: str, passphrase: str | None = None) -> str: ...

def chat_cursor_json(path: str, workspace: str, chat_workspace_id: str, channel_id: str, passphrase: str | None = None) -> str: ...

def chat_update_cursor_json(path: str, workspace: str, chat_workspace_id: str, channel_id: str, next_sequence: int, passphrase: str | None = None) -> str: ...

def chat_fetch_events_json(path: str, workspace: str, chat_workspace_id: str, channel_id: str, from_sequence: int, max: int, passphrase: str | None = None) -> str: ...

def blob_digest(data: bytes) -> str:
    """Compute the Blob content address (`"algo:hex"`) of the given bytes."""
    ...

def create_loom(
    path: str,
    profile: str,
    suite: str | None = None,
    passphrase: str | None = None,
) -> None:
    """Create a fresh ``.loom`` under an identity profile, optionally encrypted under ``passphrase``."""
    ...

def exec_cbor(path: str, request: bytes, passphrase: str | None = None) -> bytes:
    """Execute canonical ``loom.exec.request.v1`` bytes and return canonical ``loom.exec.result.v1``."""
    ...

def authenticate_passphrase(
    path: str,
    principal: str,
    principal_passphrase: str,
    passphrase: str | None = None,
) -> None:
    """Verify principal credentials for one local call."""
    ...

def identity_list_json(
    path: str,
    passphrase: str | None = None,
    auth_principal: str | None = None,
    auth_passphrase: str | None = None,
) -> str:
    """List local principals as JSON."""
    ...

def identity_add_principal(
    path: str,
    principal_handle: str,
    name: str,
    kind: str,
    passphrase: str | None = None,
    auth_principal: str | None = None,
    auth_passphrase: str | None = None,
) -> str:
    """Add a local principal and return its UUID string."""
    ...

def identity_rename_principal_handle(
    path: str,
    principal: str,
    principal_handle: str,
    passphrase: str | None = None,
    auth_principal: str | None = None,
    auth_passphrase: str | None = None,
) -> None:
    """Rename a local principal handle while retaining the previous handle as an alias."""
    ...

def identity_set_passphrase(
    path: str,
    principal: str,
    principal_passphrase: str,
    passphrase: str | None = None,
    auth_principal: str | None = None,
    auth_passphrase: str | None = None,
) -> None:
    """Set or replace a local principal passphrase."""
    ...

def identity_remove_principal(
    path: str,
    principal: str,
    passphrase: str | None = None,
    auth_principal: str | None = None,
    auth_passphrase: str | None = None,
) -> None:
    """Remove a local principal."""
    ...

def identity_assign_role(
    path: str,
    principal: str,
    role: str,
    passphrase: str | None = None,
    auth_principal: str | None = None,
    auth_passphrase: str | None = None,
) -> None:
    """Assign a local role to a principal."""
    ...

def identity_revoke_role(
    path: str,
    principal: str,
    role: str,
    passphrase: str | None = None,
    auth_principal: str | None = None,
    auth_passphrase: str | None = None,
) -> bool:
    """Revoke a local role from a principal."""
    ...

def identity_create_external_credential(
    path: str,
    principal: str,
    kind: str,
    label: str,
    issuer: str,
    subject: str,
    material_digest: str | None = None,
    passphrase: str | None = None,
    auth_principal: str | None = None,
    auth_passphrase: str | None = None,
) -> str:
    """Create an external credential binding and return its UUID string."""
    ...

def identity_revoke_external_credential(
    path: str,
    credential: str,
    passphrase: str | None = None,
    auth_principal: str | None = None,
    auth_passphrase: str | None = None,
) -> None:
    """Revoke an external credential binding."""
    ...

def identity_add_public_key(
    path: str,
    principal: str,
    label: str,
    algorithm: str,
    public_key_hex: str,
    passphrase: str | None = None,
    auth_principal: str | None = None,
    auth_passphrase: str | None = None,
) -> str:
    """Add a principal-bound public verification key and return its UUID string."""
    ...

def identity_revoke_public_key(
    path: str,
    key: str,
    passphrase: str | None = None,
    auth_principal: str | None = None,
    auth_passphrase: str | None = None,
) -> None:
    """Revoke a principal-bound public verification key."""
    ...

def acl_list_json(
    path: str,
    passphrase: str | None = None,
    auth_principal: str | None = None,
    auth_passphrase: str | None = None,
) -> str:
    """List direct ACL grants as JSON."""
    ...

def acl_grant(
    path: str,
    effect: int,
    subject: str,
    workspace: str | None = None,
    domain: str | None = None,
    rights_mask: int = 0,
    passphrase: str | None = None,
    auth_principal: str | None = None,
    auth_passphrase: str | None = None,
    predicate_cel: str | None = None,
) -> None:
    """Add a direct ACL grant."""
    ...

def acl_grant_scoped(
    path: str,
    effect: int,
    subject: str,
    workspace: str | None = None,
    domain: str | None = None,
    rights_mask: int = 0,
    ref_glob: str | None = None,
    scopes: list[str] | None = None,
    passphrase: str | None = None,
    auth_principal: str | None = None,
    auth_passphrase: str | None = None,
    predicate_cel: str | None = None,
) -> None:
    """Add a direct ACL grant with ref glob and typed prefix scopes."""
    ...

def acl_revoke(
    path: str,
    effect: int,
    subject: str,
    workspace: str | None = None,
    domain: str | None = None,
    rights_mask: int = 0,
    passphrase: str | None = None,
    auth_principal: str | None = None,
    auth_passphrase: str | None = None,
    predicate_cel: str | None = None,
) -> bool:
    """Remove direct ACL grants exactly matching the supplied fields."""
    ...

def acl_revoke_scoped(
    path: str,
    effect: int,
    subject: str,
    workspace: str | None = None,
    domain: str | None = None,
    rights_mask: int = 0,
    ref_glob: str | None = None,
    scopes: list[str] | None = None,
    passphrase: str | None = None,
    auth_principal: str | None = None,
    auth_passphrase: str | None = None,
    predicate_cel: str | None = None,
) -> bool:
    """Remove direct ACL grants exactly matching ref glob and typed prefix scopes."""
    ...

def protected_ref_list_json(
    path: str,
    workspace: str,
    passphrase: str | None = None,
    auth_principal: str | None = None,
    auth_passphrase: str | None = None,
) -> str:
    """List protected-ref policies for one workspace as JSON."""
    ...

def protected_ref_get_json(
    path: str,
    workspace: str,
    ref_name: str,
    passphrase: str | None = None,
    auth_principal: str | None = None,
    auth_passphrase: str | None = None,
) -> str:
    """Return one protected-ref policy as JSON, or ``null`` when absent."""
    ...

def protected_ref_set(
    path: str,
    workspace: str,
    ref_name: str,
    fast_forward_only: bool,
    signed_commits_required: bool,
    signed_ref_advance_required: bool,
    required_review_count: int,
    retention_lock: bool,
    governance_lock: bool,
    passphrase: str | None = None,
    auth_principal: str | None = None,
    auth_passphrase: str | None = None,
) -> None:
    """Create or replace one protected-ref policy."""
    ...

def protected_ref_remove(
    path: str,
    workspace: str,
    ref_name: str,
    passphrase: str | None = None,
    auth_principal: str | None = None,
    auth_passphrase: str | None = None,
) -> bool:
    """Remove one protected-ref policy and return whether it existed."""
    ...

def daemon_status_json(path: str) -> str:
    """Local daemon status for ``path`` as JSON. Missing daemons return a STOPPED JSON payload."""
    ...

def daemon_session_attach(path: str, session: str) -> None:
    """Attach a named session to a running local daemon."""
    ...

def daemon_session_detach(path: str, session: str) -> None:
    """Detach a named session from a running local daemon."""
    ...

def daemon_pin_add(path: str, pin: str) -> None:
    """Add a long-lived pin on a running local daemon."""
    ...

def daemon_pin_remove(path: str, pin: str) -> None:
    """Remove a long-lived pin from a running local daemon."""
    ...

def lock_acquire_json(
    path: str,
    key: str,
    principal: str,
    session: str,
    mode: str,
    permits: int,
    capacity: int,
    lease_ms: int,
    wait_ms: int | None = None,
) -> str:
    """Acquire a daemon-backed lock and return its token as JSON."""
    ...

def lock_acquire(
    path: str,
    key: str,
    principal: str,
    session: str,
    mode: str = "exclusive",
    permits: int = 1,
    capacity: int = 1,
    lease_ms: int = 60000,
    wait_ms: int | None = None,
) -> LockToken:
    """Acquire a daemon-backed lock and return a typed token."""
    ...

def lock_try_acquire(
    path: str,
    key: str,
    principal: str,
    session: str,
    mode: str = "exclusive",
    permits: int = 1,
    capacity: int = 1,
    lease_ms: int = 60000,
) -> LockToken:
    """Acquire a daemon-backed lock without waiting and return a typed token."""
    ...

def lock_refresh_json(
    path: str,
    key: str,
    principal: str,
    session: str,
    mode: str,
    permits: int,
    capacity: int,
    fence_low: int,
    fence_high: int,
    lease_ms: int,
) -> str:
    """Refresh a daemon-backed lock and return its token as JSON."""
    ...

def lock_refresh(path: str, token: LockToken, lease_ms: int = 60000) -> LockToken:
    """Refresh a daemon-backed lock token."""
    ...

def lock_release(
    path: str,
    key: str,
    principal: str,
    session: str,
    mode: str,
    permits: int,
    capacity: int,
    fence_low: int,
    fence_high: int,
) -> None:
    """Release a daemon-backed lock."""
    ...

def lock_release_token(path: str, token: LockToken) -> None:
    """Release a daemon-backed lock token."""
    ...

def lock_guard(
    path: str,
    key: str,
    principal: str,
    session: str,
    mode: str = "exclusive",
    permits: int = 1,
    capacity: int = 1,
    lease_ms: int = 60000,
    wait_ms: int | None = None,
) -> LockGuard:
    """Acquire a daemon-backed lock as a context manager."""
    ...

def fs_export(
    path: str,
    workspace: str,
    dst_path: str,
    revision: str | None = None,
    dry_run: bool = False,
    passphrase: str | None = None,
) -> bytes:
    ...

def fs_import(
    path: str,
    workspace: str,
    src_path: str,
    commit: bool = False,
    dry_run: bool = False,
    passphrase: str | None = None,
) -> bytes:
    ...

def archive_export(
    path: str,
    workspace: str,
    dst_path: str,
    kind: str,
    revision: str | None = None,
    dry_run: bool = False,
    passphrase: str | None = None,
) -> bytes:
    ...

def archive_import(
    path: str,
    workspace: str,
    src_path: str,
    kind: str,
    dry_run: bool = False,
    passphrase: str | None = None,
) -> bytes:
    ...

def car_export(
    path: str,
    workspace: str,
    dst_path: str,
    dry_run: bool = False,
    passphrase: str | None = None,
) -> bytes:
    ...

def car_import(
    path: str,
    src_path: str,
    dry_run: bool = False,
    passphrase: str | None = None,
) -> bytes:
    ...

def cas_put(
    path: str,
    workspace: str,
    content: bytes,
    passphrase: str | None = None,
) -> str:
    """Store ``content`` in a workspace's ``cas`` facet (by UUID or name, created if absent); returns the
    content address (``"algo:hex"``). Idempotent: identical bytes yield the same address."""
    ...

def kv_put(
    path: str,
    workspace: str,
    collection: str,
    key: bytes,
    value: bytes,
    passphrase: str | None = None,
) -> None:
    """Put ``value`` at the typed ``key`` (Loom Canonical CBOR cell) in map ``collection`` of a workspace
    (created with the ``kv`` facet if absent). A later put at the same key replaces the value."""
    ...

def kv_get(
    path: str,
    workspace: str,
    collection: str,
    key: bytes,
    passphrase: str | None = None,
) -> bytes | None:
    """Fetch the value at typed ``key`` in map ``collection``, or ``None`` if the key or map is absent."""
    ...

def kv_delete(
    path: str,
    workspace: str,
    collection: str,
    key: bytes,
    passphrase: str | None = None,
) -> bool:
    """Remove the typed ``key`` from map ``collection``; returns whether it was present."""
    ...

def kv_list(
    path: str,
    workspace: str,
    collection: str,
    passphrase: str | None = None,
) -> bytes:
    """List map ``collection`` as the Loom Canonical CBOR array of ``[key, value]`` pairs in key order."""
    ...

def kv_range(
    path: str,
    workspace: str,
    collection: str,
    lo: bytes,
    hi: bytes,
    passphrase: str | None = None,
) -> bytes:
    """The entries of map ``collection`` with ``lo <= key < hi`` (half-open, key order) as Loom Canonical CBOR
    ``[key, value]`` pairs. ``lo``/``hi`` are typed-cell CBOR keys."""
    ...

def spaces_create_json(
    path: str,
    workspace: str,
    page_workspace_id: str,
    space_id: str,
    title: str,
    expected_root: str | None = None,
    passphrase: str | None = None,
) -> str: ...

def spaces_list_json(
    path: str,
    workspace: str,
    page_workspace_id: str,
    passphrase: str | None = None,
) -> str: ...

def spaces_get_json(
    path: str,
    workspace: str,
    page_workspace_id: str,
    space_id: str,
    passphrase: str | None = None,
) -> str: ...

def pages_create_json(
    path: str,
    workspace: str,
    page_workspace_id: str,
    page_id: str,
    space_id: str,
    parent_page_id: str | None = None,
    title: str = "",
    expected_root: str | None = None,
    passphrase: str | None = None,
) -> str: ...

def pages_update_json(
    path: str,
    workspace: str,
    page_workspace_id: str,
    page_id: str,
    body_text: str,
    expected_root: str | None = None,
    passphrase: str | None = None,
) -> str: ...

def pages_publish_json(
    path: str,
    workspace: str,
    page_workspace_id: str,
    page_id: str,
    expected_root: str | None = None,
    passphrase: str | None = None,
) -> str: ...

def pages_get_json(
    path: str,
    workspace: str,
    page_workspace_id: str,
    page_id: str,
    passphrase: str | None = None,
) -> str: ...

def pages_list_json(
    path: str,
    workspace: str,
    page_workspace_id: str,
    passphrase: str | None = None,
) -> str: ...

def pages_history_json(
    path: str,
    workspace: str,
    page_workspace_id: str,
    page_id: str,
    passphrase: str | None = None,
) -> str: ...

def structures_create_json(
    path: str,
    workspace: str,
    page_workspace_id: str,
    structure_id: str,
    space_id: str,
    kind: str,
    title: str,
    expected_root: str | None = None,
    passphrase: str | None = None,
) -> str: ...

def structures_add_node_json(
    path: str,
    workspace: str,
    page_workspace_id: str,
    structure_id: str,
    node_id: str,
    kind: str,
    label: str,
    body_digest: str | None = None,
    entity_ref: str | None = None,
    expected_root: str | None = None,
    passphrase: str | None = None,
) -> str: ...

def structures_update_node_json(
    path: str,
    workspace: str,
    page_workspace_id: str,
    structure_id: str,
    node_id: str,
    kind: str,
    label: str,
    body_digest: str | None = None,
    entity_ref: str | None = None,
    expected_root: str | None = None,
    passphrase: str | None = None,
) -> str: ...

def structures_bind_json(
    path: str,
    workspace: str,
    page_workspace_id: str,
    structure_id: str,
    node_id: str,
    entity_ref: str | None = None,
    expected_root: str | None = None,
    passphrase: str | None = None,
) -> str: ...

def structures_move_node_json(
    path: str,
    workspace: str,
    page_workspace_id: str,
    structure_id: str,
    node_id: str,
    parent_node_id: str | None = None,
    label: str | None = None,
    expected_root: str | None = None,
    passphrase: str | None = None,
) -> str: ...

def structures_link_node_json(
    path: str,
    workspace: str,
    page_workspace_id: str,
    structure_id: str,
    edge_id: str,
    src_node_id: str,
    dst_node_id: str,
    label: str,
    target_ref: str | None = None,
    expected_root: str | None = None,
    passphrase: str | None = None,
) -> str: ...

def structures_decompose_to_tickets_json(
    path: str,
    workspace: str,
    page_workspace_id: str,
    structure_id: str,
    items_json: str,
    passphrase: str | None = None,
) -> str: ...

def structures_get_json(
    path: str,
    workspace: str,
    page_workspace_id: str,
    structure_id: str,
    passphrase: str | None = None,
) -> str: ...

def structures_list_json(
    path: str,
    workspace: str,
    page_workspace_id: str,
    passphrase: str | None = None,
) -> str: ...

def graph_upsert_node(
    path: str,
    workspace: str,
    name: str,
    id: str,
    props: bytes,
    passphrase: str | None = None,
) -> None:
    """Insert or replace node ``id`` (with ``props`` as a Loom Canonical CBOR ``text -> bytes`` map, or
    empty for none) in graph ``name`` of a workspace (created with the ``graph`` facet if absent)."""
    ...

def graph_get_node(
    path: str,
    workspace: str,
    name: str,
    id: str,
    passphrase: str | None = None,
) -> bytes | None:
    """Fetch node ``id``'s properties in graph ``name`` as a CBOR ``text -> bytes`` map, or ``None`` when
    the node or graph is absent."""
    ...

def graph_remove_node(
    path: str,
    workspace: str,
    name: str,
    id: str,
    cascade: bool,
    passphrase: str | None = None,
) -> None:
    """Remove node ``id`` from graph ``name``. ``cascade`` false rejects with ``CONFLICT`` while incident
    edges exist; ``cascade`` true removes the node and its incident edges. ``NOT_FOUND`` if the node is
    absent."""
    ...

def graph_upsert_edge(
    path: str,
    workspace: str,
    name: str,
    id: str,
    src: str,
    dst: str,
    label: str,
    props: bytes,
    passphrase: str | None = None,
) -> None:
    """Insert or replace edge ``id`` from ``src`` to ``dst`` (both endpoints must already exist, else
    ``NOT_FOUND``) with ``label`` and ``props`` (CBOR ``text -> bytes`` map, or empty) in graph
    ``name``."""
    ...

def graph_get_edge(
    path: str,
    workspace: str,
    name: str,
    id: str,
    passphrase: str | None = None,
) -> bytes | None:
    """Fetch edge ``id`` in graph ``name`` as the CBOR array ``[src, dst, label, props]``, or ``None``
    when the edge or graph is absent."""
    ...

def graph_remove_edge(
    path: str,
    workspace: str,
    name: str,
    id: str,
    passphrase: str | None = None,
) -> bool:
    """Remove edge ``id`` from graph ``name``; returns whether it was present. An absent edge or graph is
    a no-op."""
    ...

def graph_neighbors(
    path: str,
    workspace: str,
    name: str,
    id: str,
    passphrase: str | None = None,
) -> bytes:
    """The distinct adjacent node ids of ``id`` in graph ``name``, sorted, as a CBOR array of text (empty
    when the node or graph is absent)."""
    ...

def graph_out_edges(
    path: str,
    workspace: str,
    name: str,
    id: str,
    passphrase: str | None = None,
) -> bytes:
    """Out-edges of ``id`` in graph ``name`` as a CBOR array of ``[edge_id, [src, dst, label, props]]`` in
    edge-id order."""
    ...

def graph_in_edges(
    path: str,
    workspace: str,
    name: str,
    id: str,
    passphrase: str | None = None,
) -> bytes:
    """In-edges of ``id`` in graph ``name`` as a CBOR array of ``[edge_id, [src, dst, label, props]]`` in
    edge-id order."""
    ...

def graph_reachable(
    path: str,
    workspace: str,
    name: str,
    start: str,
    max_depth: int,
    via_label: str | None = None,
    passphrase: str | None = None,
) -> bytes:
    """Node ids reachable from ``start`` in graph ``name`` as a CBOR array of text. ``max_depth`` below
    ``0`` is no limit; ``via_label`` ``None`` follows every edge, else only edges with that label."""
    ...

def graph_shortest_path(
    path: str,
    workspace: str,
    name: str,
    from_: str,
    to: str,
    via_label: str | None = None,
    passphrase: str | None = None,
) -> bytes | None:
    """A shortest directed path from ``from`` to ``to`` in graph ``name`` as a CBOR array of node-id text,
    or ``None`` when no path exists or an endpoint or the graph is absent. ``via_label`` ``None`` follows
    every edge, else only edges with that label."""
    ...

def vector_create(
    path: str,
    workspace: str,
    name: str,
    dim: int,
    metric: int,
    passphrase: str | None = None,
) -> None:
    """Create vector set ``name`` of width ``dim`` and ``metric`` (1 cosine, 2 L2, 3 dot) in a workspace
    (created with the ``vector`` facet if absent). ``CONFLICT`` if the set already exists."""
    ...

def vector_upsert(
    path: str,
    workspace: str,
    name: str,
    id: str,
    vector: bytes,
    metadata: bytes,
    passphrase: str | None = None,
) -> None:
    """Insert or replace the vector at ``id`` in set ``name``: ``vector`` is little-endian ``f32`` bytes
    (4 per component); ``metadata`` is a CBOR ``text -> cell`` map (or empty). ``NOT_FOUND`` if the set
    was never created; ``DIMENSION_MISMATCH`` on a wrong width."""
    ...

def vector_upsert_source(
    path: str,
    workspace: str,
    name: str,
    id: str,
    vector: bytes,
    metadata: bytes,
    source_text: bytes,
    model_id: str | None = None,
    weights_digest: str | None = None,
    passphrase: str | None = None,
) -> None:
    """Insert or replace a vector with UTF-8 source text and optional embedding model profile. The
    profile crosses as ``[1, model_id, dimension, weights_digest]``."""
    ...

def vector_get(
    path: str,
    workspace: str,
    name: str,
    id: str,
    passphrase: str | None = None,
) -> bytes | None:
    """Fetch the vector + metadata at ``id`` in set ``name`` as the CBOR array
    ``[vector_bytes, metadata]`` (``vector_bytes`` little-endian ``f32``; ``metadata`` a ``text -> cell``
    map), or ``None`` when ``id`` is absent. ``NOT_FOUND`` if the set does not exist."""
    ...

def vector_source_text(
    path: str,
    workspace: str,
    name: str,
    id: str,
    passphrase: str | None = None,
) -> bytes | None:
    """Fetch UTF-8 source text for vector ``id``, or ``None`` if no source text is stored."""
    ...

def vector_embedding_model(
    path: str,
    workspace: str,
    name: str,
    passphrase: str | None = None,
) -> bytes | None:
    """Fetch the set embedding model profile as CBOR ``[1, model_id, dimension, weights_digest]``."""
    ...

def vector_ids(
    path: str,
    workspace: str,
    name: str,
    prefix: str | None = None,
    passphrase: str | None = None,
) -> bytes:
    """Vector ids in set ``name``, sorted ascending, as a CBOR array of text. ``prefix``, when present,
    restricts results to ids starting with that string prefix. ``NOT_FOUND`` if the set does not exist."""
    ...

def vector_metadata_index_keys(
    path: str,
    workspace: str,
    name: str,
    passphrase: str | None = None,
) -> bytes:
    """Metadata equality index keys declared for set ``name``, sorted ascending, as a CBOR array of text.
    ``NOT_FOUND`` if the set does not exist."""
    ...

def vector_create_metadata_index(
    path: str,
    workspace: str,
    name: str,
    key: str,
    passphrase: str | None = None,
) -> bool:
    """Declare and build a metadata equality index for ``key``; returns whether a new index was declared."""
    ...

def vector_drop_metadata_index(
    path: str,
    workspace: str,
    name: str,
    key: str,
    passphrase: str | None = None,
) -> bool:
    """Drop a metadata equality index for ``key``; returns whether an index was present."""
    ...

def vector_delete(
    path: str,
    workspace: str,
    name: str,
    id: str,
    passphrase: str | None = None,
) -> bool:
    """Remove ``id`` from set ``name``; returns whether it was present. ``NOT_FOUND`` if the set does not
    exist."""
    ...

def vector_search(
    path: str,
    workspace: str,
    name: str,
    query: bytes,
    k: int,
    filter: bytes,
    passphrase: str | None = None,
) -> bytes:
    """Exact top-``k`` nearest neighbours of ``query`` (little-endian ``f32`` bytes) in set ``name`` among
    vectors passing ``filter``, as a CBOR array of ``[id, score_cell]``, highest score first. The filter
    is a recursive CBOR array: ``[0]`` all, ``[1, key, value_cell]`` equality, ``[2, a, b]`` AND; an empty
    buffer is all. ``NOT_FOUND`` if the set does not exist; ``DIMENSION_MISMATCH`` on a wrong-width
    query."""
    ...

def vector_search_policy(
    path: str,
    workspace: str,
    name: str,
    query: bytes,
    k: int,
    filter: bytes,
    policy: int,
    threshold: int,
    ef: int,
    pq_m: int,
    pq_k: int,
    pq_iters: int,
    passphrase: str | None = None,
) -> bytes:
    """Top-``k`` nearest neighbours with explicit accelerator policy over built-in PQ.

    ``policy`` is 0 for exact and 1 for approximate-above-threshold. Result CBOR matches
    ``vector_search``.
    """
    ...

def columnar_create(
    path: str,
    workspace: str,
    name: str,
    columns: bytes,
    target_segment_rows: int,
    passphrase: str | None = None,
) -> None:
    """Create columnar dataset ``name`` with ``columns`` (a CBOR array of ``[name, type_tag]``) and
    ``target_segment_rows`` (0 for the engine default) in a workspace (created with the ``columnar`` facet
    if absent). ``CONFLICT`` if the dataset already exists."""
    ...

def columnar_append(
    path: str,
    workspace: str,
    name: str,
    row: bytes,
    passphrase: str | None = None,
) -> None:
    """Append ``row`` (a CBOR cell array) to dataset ``name``, validating arity + column types.
    ``NOT_FOUND`` if the dataset was never created; ``INVALID_ARGUMENT`` on an arity or type mismatch."""
    ...

def columnar_scan(
    path: str,
    workspace: str,
    name: str,
    passphrase: str | None = None,
) -> bytes:
    """All rows of dataset ``name`` in append order as a CBOR array of cell arrays. ``NOT_FOUND`` if the
    dataset does not exist."""
    ...

def columnar_columns(
    path: str,
    workspace: str,
    name: str,
    passphrase: str | None = None,
) -> bytes:
    """The ``(name, type_tag)`` columns of dataset ``name`` as a CBOR array of ``[name, type_tag]``.
    ``NOT_FOUND`` if the dataset does not exist."""
    ...

def columnar_rows(
    path: str,
    workspace: str,
    name: str,
    passphrase: str | None = None,
) -> int:
    """The total row count of dataset ``name``. ``NOT_FOUND`` if the dataset does not exist."""
    ...

def columnar_compact(
    path: str,
    workspace: str,
    name: str,
    passphrase: str | None = None,
) -> None:
    """Compact dataset ``name`` at its target segment size."""
    ...

def columnar_inspect(
    path: str,
    workspace: str,
    name: str,
    passphrase: str | None = None,
) -> bytes:
    """Inspect dataset metadata as CBOR ``[columns, rows, segment_count, target_segment_rows,
    source_digest]``."""
    ...

def columnar_source_digest(
    path: str,
    workspace: str,
    name: str,
    passphrase: str | None = None,
) -> bytes:
    """The source digest used by derived columnar projections as CBOR text."""
    ...

def columnar_select(
    path: str,
    workspace: str,
    name: str,
    columns: bytes,
    filter: bytes,
    passphrase: str | None = None,
) -> bytes:
    """Project ``columns`` (a CBOR array of text) from dataset ``name``'s rows matching ``filter`` as a
    CBOR array of cell arrays. The filter is the CBOR array ``[column, op, value_cell]`` (op: 0 eq, 1 ne,
    2 lt, 3 le, 4 gt, 5 ge); an empty filter buffer scans every row. ``NOT_FOUND`` if the dataset does
    not exist; ``INVALID_ARGUMENT`` on an unknown column."""
    ...

def columnar_aggregate(
    path: str,
    workspace: str,
    name: str,
    aggregates: bytes,
    filter: bytes,
    passphrase: str | None = None,
) -> bytes:
    """Evaluate aggregate expressions from CBOR ``[[op, column?] ...]``, with optional select filter."""
    ...

def dataframe_create(
    path: str,
    workspace: str,
    name: str,
    plan: bytes,
    passphrase: str | None = None,
) -> None:
    """Create dataframe frame ``name`` from canonical DataframePlan CBOR."""
    ...

def dataframe_collect(
    path: str,
    workspace: str,
    name: str,
    passphrase: str | None = None,
) -> bytes:
    """Execute dataframe frame ``name`` and return canonical CBOR ``[columns, rows]``."""
    ...

def dataframe_preview(
    path: str,
    workspace: str,
    name: str,
    rows: int,
    passphrase: str | None = None,
) -> bytes:
    """Execute dataframe frame ``name`` and return at most ``rows`` rows as canonical CBOR
    ``[columns, rows]``."""
    ...

def dataframe_materialize(
    path: str,
    workspace: str,
    name: str,
    passphrase: str | None = None,
) -> str | None:
    """Materialize dataframe frame ``name``; returns a CAS digest when the materialization target emits
    one."""
    ...

def dataframe_plan_digest(
    path: str,
    workspace: str,
    name: str,
    passphrase: str | None = None,
) -> str:
    """Return the canonical dataframe plan digest as ``algo:hex``."""
    ...

def dataframe_source_digests(
    path: str,
    workspace: str,
    name: str,
    passphrase: str | None = None,
) -> bytes:
    """Return source digests pinned in the dataframe plan as canonical CBOR array of ``algo:hex``
    strings."""
    ...

def search_create(
    path: str,
    workspace: str,
    name: str,
    mapping: bytes,
    passphrase: str | None = None,
) -> None:
    """Create search collection ``name`` with the field ``mapping`` (a CBOR map of
    ``field -> [type_tag, stored, faceted]``; type 0 text, 1 keyword) in a workspace (created with the
    ``search`` facet if absent). ``CONFLICT`` if the collection already exists."""
    ...

def search_index(
    path: str,
    workspace: str,
    name: str,
    id: bytes,
    doc: bytes,
    passphrase: str | None = None,
) -> None:
    """Insert or replace the document at ``id`` (opaque bytes) in collection ``name``; ``doc`` is a CBOR
    ``field -> value`` map (each value text or bytes). ``NOT_FOUND`` if the collection was never
    created."""
    ...

def search_get(
    path: str,
    workspace: str,
    name: str,
    id: bytes,
    passphrase: str | None = None,
) -> bytes | None:
    """Fetch the document at ``id`` in collection ``name`` as a CBOR ``field -> value`` map, or ``None``
    when ``id`` is absent. ``NOT_FOUND`` if the collection does not exist."""
    ...

def search_delete(
    path: str,
    workspace: str,
    name: str,
    id: bytes,
    passphrase: str | None = None,
) -> bool:
    """Remove ``id`` from collection ``name``; returns whether it was present. ``NOT_FOUND`` if the
    collection does not exist."""
    ...

def search_ids(
    path: str,
    workspace: str,
    name: str,
    prefix: bytes,
    has_prefix: bool,
    passphrase: str | None = None,
) -> bytes:
    """Document ids in collection ``name`` as a CBOR array of byte strings. When ``has_prefix`` is true
    only ids starting with ``prefix`` are returned; otherwise every id is returned. ``NOT_FOUND`` if the
    collection does not exist."""
    ...

def search_remap(
    path: str,
    workspace: str,
    name: str,
    mapping: bytes,
    passphrase: str | None = None,
) -> None:
    """Replace the field mapping of collection ``name`` (CBOR ``field -> [type_tag, stored, faceted]``).
    ``NOT_FOUND`` if the collection does not exist."""
    ...

def search_query(
    path: str,
    workspace: str,
    name: str,
    request: bytes,
    passphrase: str | None = None,
) -> bytes:
    """Run the portable linear-scan query over collection ``name``. ``request`` is the CBOR array
    ``[query, limit, offset]`` (``query`` a recursive node: ``[0, field, text]`` match,
    ``[1, field, value]`` term, ``[2, field, [terms], slop]`` phrase,
    ``[3, field, lower, upper, incl_lower, incl_upper]`` range,
    ``[4, [must], [should], [must_not]]`` bool). The response is the CBOR array
    ``[reduced, [[id, score_cell, highlights] ...], facets, aggregations]``. ``NOT_FOUND`` if the collection does not exist;
    ``NO_SUCH_FIELD`` for an unmapped query field."""
    ...

def doc_put_text(
    path: str,
    workspace: str,
    collection: str,
    id: str,
    text: str,
    expected_entity_tag: str | None = None,
    passphrase: str | None = None,
) -> tuple[str, str]:
    """Put UTF-8 text at string ``id`` in collection ``collection`` and return the new document tags."""
    ...

def doc_get_text(
    path: str,
    workspace: str,
    collection: str,
    id: str,
    passphrase: str | None = None,
) -> tuple[str, str, str] | None:
    """Fetch ``id`` as UTF-8 text with its content digest, or ``None`` if absent."""
    ...

def doc_put_binary(
    path: str,
    workspace: str,
    collection: str,
    id: str,
    value: bytes,
    expected_entity_tag: str | None = None,
    passphrase: str | None = None,
) -> tuple[str, str]:
    """Put binary bytes at string ``id`` in collection ``collection`` and return the new document tags."""
    ...

def doc_get_binary(
    path: str,
    workspace: str,
    collection: str,
    id: str,
    passphrase: str | None = None,
) -> tuple[bytes, str, str] | None:
    """Fetch ``id`` as binary bytes with its content digest, or ``None`` if absent."""
    ...

def doc_delete(
    path: str,
    workspace: str,
    collection: str,
    id: str,
    passphrase: str | None = None,
) -> bool:
    """Remove ``id`` from collection ``collection``; returns whether it was present."""
    ...

def doc_list_binary(
    path: str,
    workspace: str,
    collection: str,
    passphrase: str | None = None,
) -> bytes:
    """List collection ``collection`` as its canonical binary representation."""
    ...

def doc_index_create(
    path: str,
    workspace: str,
    collection: str,
    name: str,
    field_path: str,
    unique: bool = False,
    passphrase: str | None = None,
) -> None:
    """Create a native document index over a dotted JSON field path."""
    ...

def doc_index_create_json(
    path: str,
    workspace: str,
    collection: str,
    declaration_json: bytes,
    passphrase: str | None = None,
) -> None:
    """Create a native document index from a full declaration JSON object."""
    ...

def doc_index_drop(
    path: str,
    workspace: str,
    collection: str,
    name: str,
    passphrase: str | None = None,
) -> bool:
    """Drop a native document index and return whether it was present."""
    ...

def doc_index_rebuild(
    path: str,
    workspace: str,
    collection: str,
    name: str,
    passphrase: str | None = None,
) -> None:
    """Rebuild a native document index."""
    ...

def doc_index_list_json(
    path: str,
    workspace: str,
    collection: str,
    passphrase: str | None = None,
) -> str:
    """List native document indexes as JSON."""
    ...

def doc_index_status_json(
    path: str,
    workspace: str,
    collection: str,
    passphrase: str | None = None,
) -> str:
    """Return native document index readiness as JSON."""
    ...

def doc_find_json(
    path: str,
    workspace: str,
    collection: str,
    index: str,
    value_json: str,
    passphrase: str | None = None,
) -> str:
    """Find document ids by exact index value and return JSON."""
    ...

def doc_query_json(
    path: str,
    workspace: str,
    collection: str,
    query_json: str,
    passphrase: str | None = None,
) -> str:
    """Execute a native document query and return JSON."""
    ...

def ts_put(
    path: str,
    workspace: str,
    collection: str,
    ts: int,
    value: bytes,
    passphrase: str | None = None,
) -> None:
    """Put ``value`` at timestamp ``ts`` in series ``collection`` (created with the ``time-series`` facet if
    absent)."""
    ...

def ts_get(
    path: str,
    workspace: str,
    collection: str,
    ts: int,
    passphrase: str | None = None,
) -> bytes | None:
    """The point at timestamp ``ts`` in series ``collection``, or ``None`` if absent."""
    ...

def ts_range(
    path: str,
    workspace: str,
    collection: str,
    from_ts: int,
    to_ts: int,
    passphrase: str | None = None,
) -> bytes:
    """The points of series ``collection`` with ``from_ts <= ts < to_ts`` (half-open) as Loom Canonical CBOR
    ``[ts, value]`` pairs."""
    ...

def ts_latest(
    path: str,
    workspace: str,
    collection: str,
    passphrase: str | None = None,
) -> bytes | None:
    """The most recent point of series ``collection`` as a one-point CBOR array, or ``None`` if absent/empty."""
    ...

def ledger_append(
    path: str,
    workspace: str,
    collection: str,
    payload: bytes,
    passphrase: str | None = None,
) -> int:
    """Append ``payload`` to ledger ``collection`` (created with the ``ledger`` facet if absent); returns the
    sequence."""
    ...

def ledger_get(
    path: str,
    workspace: str,
    collection: str,
    seq: int,
    passphrase: str | None = None,
) -> bytes | None:
    """The payload at ``seq`` in ledger ``collection``, or ``None`` if absent."""
    ...

def ledger_head(
    path: str,
    workspace: str,
    collection: str,
    passphrase: str | None = None,
) -> bytes | None:
    """The 32-byte head chain hash of ledger ``collection``, or ``None`` when absent or empty."""
    ...

def ledger_len(
    path: str,
    workspace: str,
    collection: str,
    passphrase: str | None = None,
) -> int:
    """The number of entries in ledger ``collection`` (0 when absent)."""
    ...

def ledger_verify(
    path: str,
    workspace: str,
    collection: str,
    passphrase: str | None = None,
) -> None:
    """Recompute and verify ledger ``collection``'s hash chain; an altered payload or broken link raises."""
    ...

def cal_create_collection(
    path: str,
    workspace: str,
    principal: str,
    collection: str,
    display_name: str,
    components: str,
    passphrase: str | None = None,
) -> None:
    """Create (or replace the metadata of) calendar collection ``collection`` under ``principal``
    (created with the ``calendar`` facet if absent). ``components`` is a comma-separated component set
    ("event,todo"; "" is the empty set)."""
    ...

def cal_delete_collection(
    path: str,
    workspace: str,
    principal: str,
    collection: str,
    passphrase: str | None = None,
) -> bool:
    """Delete calendar collection ``collection`` under ``principal`` and every entry in it; returns
    whether it existed."""
    ...

def cal_list_collections(
    path: str,
    workspace: str,
    principal: str,
    passphrase: str | None = None,
) -> bytes:
    """List the calendar collection ids under ``principal`` as the Loom Canonical CBOR array of text
    strings (sorted)."""
    ...

def cal_put_entry(
    path: str,
    workspace: str,
    principal: str,
    collection: str,
    entry: bytes,
    passphrase: str | None = None,
) -> None:
    """Put the calendar ``entry`` (its ``CalendarEntry`` canonical CBOR) into collection ``collection``
    under ``principal``, keyed by its UID. A later put at the same UID replaces it."""
    ...

def cal_get_entry(
    path: str,
    workspace: str,
    principal: str,
    collection: str,
    uid: str,
    passphrase: str | None = None,
) -> bytes | None:
    """Fetch the calendar entry at ``uid`` as its ``CalendarEntry`` canonical CBOR, or ``None`` if
    absent."""
    ...

def cal_delete_entry(
    path: str,
    workspace: str,
    principal: str,
    collection: str,
    uid: str,
    passphrase: str | None = None,
) -> bool:
    """Remove the calendar entry at ``uid`` in collection ``collection``; returns whether it was
    present."""
    ...

def cal_list_entries(
    path: str,
    workspace: str,
    principal: str,
    collection: str,
    passphrase: str | None = None,
) -> bytes:
    """List collection ``collection`` as the Loom Canonical CBOR array of per-entry ``CalendarEntry``
    canonical CBOR byte strings (UID order)."""
    ...

def cal_range(
    path: str,
    workspace: str,
    principal: str,
    collection: str,
    from_: str,
    to: str,
    passphrase: str | None = None,
) -> bytes:
    """Expand collection ``collection`` into occurrences within the half-open wall-clock window
    ``[from, to)`` (both ``YYYYMMDDTHHMMSS``) as the Loom Canonical CBOR array of
    ``[uid, "YYYYMMDDTHHMMSS"]`` pairs."""
    ...

def cal_search(
    path: str,
    workspace: str,
    principal: str,
    collection: str,
    component: str,
    text: str,
    passphrase: str | None = None,
) -> bytes:
    """Search collection ``collection`` by component filter ("" any, "event", "todo") and a
    case-insensitive substring over the summary. Returns the Loom Canonical CBOR array of per-entry
    ``CalendarEntry`` canonical CBOR byte strings (UID order)."""
    ...

def cal_entry_ics(
    path: str,
    workspace: str,
    principal: str,
    collection: str,
    uid: str,
    passphrase: str | None = None,
) -> str | None:
    """The on-demand iCalendar (``.ics``) projection of the entry at ``uid``, or ``None`` if absent."""
    ...

def cal_put_ics(
    path: str,
    workspace: str,
    principal: str,
    collection: str,
    ics: str,
    passphrase: str | None = None,
) -> str:
    """Parse iCalendar document ``ics`` and store it as a record in collection ``collection``; returns
    the new ETag as a ``"algo:hex"`` string."""
    ...

def card_create_book(
    path: str,
    workspace: str,
    principal: str,
    book: str,
    display_name: str,
    passphrase: str | None = None,
) -> None:
    """Create (or replace the metadata of) address book ``book`` under ``principal`` (created with the
    ``contacts`` facet if absent)."""
    ...

def card_delete_book(
    path: str,
    workspace: str,
    principal: str,
    book: str,
    passphrase: str | None = None,
) -> bool:
    """Delete address book ``book`` under ``principal`` and every contact in it; returns whether it
    existed."""
    ...

def card_list_books(
    path: str,
    workspace: str,
    principal: str,
    passphrase: str | None = None,
) -> bytes:
    """List the address-book ids under ``principal`` as the Loom Canonical CBOR array of text strings
    (sorted)."""
    ...

def card_put_entry(
    path: str,
    workspace: str,
    principal: str,
    book: str,
    entry: bytes,
    passphrase: str | None = None,
) -> None:
    """Put the contact ``entry`` (its ``ContactEntry`` canonical CBOR) into address book ``book`` under
    ``principal``, keyed by its UID. A later put at the same UID replaces it."""
    ...

def card_get_entry(
    path: str,
    workspace: str,
    principal: str,
    book: str,
    uid: str,
    passphrase: str | None = None,
) -> bytes | None:
    """Fetch the contact at ``uid`` as its ``ContactEntry`` canonical CBOR, or ``None`` if absent."""
    ...

def card_delete_entry(
    path: str,
    workspace: str,
    principal: str,
    book: str,
    uid: str,
    passphrase: str | None = None,
) -> bool:
    """Remove the contact at ``uid`` in address book ``book``; returns whether it was present."""
    ...

def card_list_entries(
    path: str,
    workspace: str,
    principal: str,
    book: str,
    passphrase: str | None = None,
) -> bytes:
    """List address book ``book`` as the Loom Canonical CBOR array of per-contact ``ContactEntry``
    canonical CBOR byte strings (UID order)."""
    ...

def card_search(
    path: str,
    workspace: str,
    principal: str,
    book: str,
    text: str,
    passphrase: str | None = None,
) -> bytes:
    """Search address book ``book`` by a case-insensitive substring over the formatted name,
    organization, and email values. Returns the Loom Canonical CBOR array of per-contact ``ContactEntry``
    canonical CBOR byte strings (UID order)."""
    ...

def card_entry_vcard(
    path: str,
    workspace: str,
    principal: str,
    book: str,
    uid: str,
    passphrase: str | None = None,
) -> str | None:
    """The on-demand vCard (``.vcf``) projection of the contact at ``uid``, or ``None`` if absent."""
    ...

def card_put_vcard(
    path: str,
    workspace: str,
    principal: str,
    book: str,
    vcf: str,
    passphrase: str | None = None,
) -> str:
    """Parse vCard document ``vcf`` and store it as a record in address book ``book``; returns the new
    ETag as a ``"algo:hex"`` string."""
    ...

def mail_create_mailbox(
    path: str,
    workspace: str,
    principal: str,
    mailbox: str,
    display_name: str,
    passphrase: str | None = None,
) -> None:
    """Create (or replace the metadata of) mailbox ``mailbox`` under ``principal`` (created with the
    ``mail`` facet if absent)."""
    ...

def mail_delete_mailbox(
    path: str,
    workspace: str,
    principal: str,
    mailbox: str,
    passphrase: str | None = None,
) -> bool:
    """Delete mailbox ``mailbox`` under ``principal`` and every message in it; returns whether it
    existed."""
    ...

def mail_list_mailboxes(
    path: str,
    workspace: str,
    principal: str,
    passphrase: str | None = None,
) -> bytes:
    """List the mailbox ids under ``principal`` as the Loom Canonical CBOR array of text strings
    (sorted)."""
    ...

def mail_ingest_message(
    path: str,
    workspace: str,
    principal: str,
    mailbox: str,
    uid: str,
    raw: bytes,
    passphrase: str | None = None,
) -> str:
    """Ingest the raw RFC 5322 message ``raw`` into mailbox ``mailbox`` under ``principal``, keyed by
    ``uid``; returns the body's content address as a ``"algo:hex"`` string."""
    ...

def mail_get_message(
    path: str,
    workspace: str,
    principal: str,
    mailbox: str,
    uid: str,
    passphrase: str | None = None,
) -> bytes | None:
    """Fetch the message index record at ``uid`` as its ``MailMessage`` canonical CBOR, or ``None`` if
    absent."""
    ...

def mail_to_eml(
    path: str,
    workspace: str,
    principal: str,
    mailbox: str,
    uid: str,
    passphrase: str | None = None,
) -> bytes | None:
    """Fetch the immutable raw RFC 5322 body of the message at ``uid``, or ``None`` if absent."""
    ...

def mail_delete_message(
    path: str,
    workspace: str,
    principal: str,
    mailbox: str,
    uid: str,
    passphrase: str | None = None,
) -> bool:
    """Remove the message at ``uid`` in mailbox ``mailbox`` (index record, flags, and body reference);
    returns whether it was present."""
    ...

def mail_list_messages(
    path: str,
    workspace: str,
    principal: str,
    mailbox: str,
    passphrase: str | None = None,
) -> bytes:
    """List mailbox ``mailbox`` as the Loom Canonical CBOR array of per-message ``MailMessage`` canonical
    CBOR byte strings (UID order)."""
    ...

def mail_get_flags(
    path: str,
    workspace: str,
    principal: str,
    mailbox: str,
    uid: str,
    passphrase: str | None = None,
) -> bytes:
    """Fetch the flag set of the message at ``uid`` as the Loom Canonical CBOR array of text strings
    (sorted)."""
    ...

def mail_set_flags(
    path: str,
    workspace: str,
    principal: str,
    mailbox: str,
    uid: str,
    flags: bytes,
    passphrase: str | None = None,
) -> None:
    """Replace the flag set of the message at ``uid``. ``flags`` is a canonical-CBOR ``Array(Text)`` byte
    buffer."""
    ...

def mail_search(
    path: str,
    workspace: str,
    principal: str,
    mailbox: str,
    text: str,
    passphrase: str | None = None,
) -> bytes:
    """Search mailbox ``mailbox`` by a case-insensitive substring over the subject and address values.
    Returns the Loom Canonical CBOR array of per-message ``MailMessage`` canonical CBOR byte strings (UID
    order)."""
    ...

def cas_get(
    path: str,
    workspace: str,
    digest: str,
    passphrase: str | None = None,
) -> bytes | None:
    """Fetch the blob addressed by ``digest`` from a workspace, or ``None`` if absent."""
    ...

def cas_has(
    path: str,
    workspace: str,
    digest: str,
    passphrase: str | None = None,
) -> bool:
    """Whether a blob addressed by ``digest`` is present in a workspace."""
    ...

def cas_delete(
    path: str,
    workspace: str,
    digest: str,
    passphrase: str | None = None,
) -> bool:
    """Drop the blob addressed by ``digest`` from a workspace's working tree (unreachable going
    forward); returns whether it was present. CAS stays immutable; bytes are GC-reclaimed once
    unreferenced, and an earlier commit that held the blob still restores it."""
    ...

def cas_list_json(
    path: str,
    workspace: str,
    passphrase: str | None = None,
) -> str:
    """List the content addresses in a workspace's ``cas`` facet as a JSON string array, sorted."""
    ...

def workspace_create(
    path: str,
    name: str | None = None,
    facet: str | None = None,
    passphrase: str | None = None,
) -> str:
    """Create a workspace and return its UUID string."""
    ...

def workspace_list_json(path: str, passphrase: str | None = None) -> str:
    """List workspaces as JSON records with id, name, facets, and head."""
    ...

def workspace_rename(
    path: str,
    workspace: str,
    new_name: str,
    passphrase: str | None = None,
) -> None:
    """Rename a workspace selected by UUID or current name."""
    ...

def workspace_delete(
    path: str,
    workspace: str,
    passphrase: str | None = None,
) -> None:
    """Delete a workspace selected by UUID or name."""
    ...

def key_add_wrap_keyed(
    path: str,
    passphrase: str,
    new_passphrase: str,
    allow_no_recovery: bool = False,
) -> None:
    """Add a passphrase unlock wrap to an encrypted store, unlocking it with the existing
    ``passphrase``. ``allow_no_recovery`` permits leaving no passphrase recovery wrap."""
    ...

def key_add_wrap_with_kek(
    path: str,
    passphrase: str,
    kek: bytes,
    allow_no_recovery: bool = False,
) -> None:
    """Add a host-supplied 256-bit raw-KEK unlock wrap to an encrypted store, unlocking it with
    ``passphrase``. ``kek`` must be exactly 32 bytes."""
    ...

def key_remove_wrap(
    path: str,
    passphrase: str,
    index: int,
    allow_no_recovery: bool = False,
) -> None:
    """Remove one unlock wrap by zero-based ``index`` from an encrypted store, unlocking it with
    ``passphrase``. ``allow_no_recovery`` permits removing the last passphrase recovery wrap."""
    ...

def merge_in_progress(
    path: str,
    facet: str,
    workspace: str,
    passphrase: str | None = None,
) -> bool:
    """Whether ``workspace`` (selected with ``facet``) has a conflicted merge awaiting
    ``merge_continue`` or ``merge_abort``."""
    ...

def merge_conflicts(
    path: str,
    facet: str,
    workspace: str,
    passphrase: str | None = None,
) -> list[str]:
    """The still-unresolved conflict paths of the in-progress merge, in path order; empty when none."""
    ...

def merge_resolve(
    path: str,
    facet: str,
    workspace: str,
    conflict_path: str,
    resolution: str,
    passphrase: str | None = None,
) -> None:
    """Settle one conflicted ``conflict_path`` of the in-progress merge. ``resolution`` is ``"ours"``,
    ``"theirs"``, or ``"working"`` (accept the currently staged content)."""
    ...

def merge_abort(
    path: str,
    facet: str,
    workspace: str,
    passphrase: str | None = None,
) -> None:
    """Abandon the in-progress merge, restoring the pre-merge working tree."""
    ...

def merge_continue(
    path: str,
    facet: str,
    workspace: str,
    author: str,
    passphrase: str | None = None,
) -> str:
    """Finish the in-progress merge: record the two-parent merge commit and return its content address
    (``"algo:hex"``). Raises ``CONFLICT`` if conflicts remain."""
    ...

def stage(
    path: str,
    facet: str,
    workspace: str,
    paths: list[str],
    passphrase: str | None = None,
) -> None:
    """Stage ``paths`` into the workspace's shared index (one stage across all facets)."""
    ...

def stage_all(
    path: str,
    facet: str,
    workspace: str,
    passphrase: str | None = None,
) -> None:
    """Stage the entire working tree (every change across every facet) into the shared index."""
    ...

def unstage(
    path: str,
    facet: str,
    workspace: str,
    paths: list[str],
    passphrase: str | None = None,
) -> None:
    """Unstage ``paths``, reverting each index entry to its HEAD state."""
    ...

def status_json(
    path: str,
    facet: str,
    workspace: str,
    passphrase: str | None = None,
) -> str:
    """The workspace status as a JSON string (``{staged, unstaged, untracked, conflicts}``; staged and
    unstaged are arrays of ``{"path", "kind"}``)."""
    ...

def commit_staged(
    path: str,
    facet: str,
    workspace: str,
    author: str,
    message: str,
    passphrase: str | None = None,
) -> str:
    """Commit only the staged index (``commit --staged``); returns the new commit's content address."""
    ...

def write_file(
    path: str,
    facet: str,
    workspace: str,
    file_path: str,
    content: bytes,
    mode: int | None = None,
    passphrase: str | None = None,
) -> None:
    """Create-or-replace ``file_path`` with ``content`` and ``mode`` (default ``0o100644``). The parent
    directory must exist."""
    ...

def read_file(
    path: str,
    facet: str,
    workspace: str,
    file_path: str,
    passphrase: str | None = None,
) -> bytes:
    """Read ``file_path`` from the workspace working tree."""
    ...

def append_file(
    path: str,
    facet: str,
    workspace: str,
    file_path: str,
    content: bytes,
    passphrase: str | None = None,
) -> None:
    """Append ``content`` to ``file_path``, creating it if absent (the parent directory must exist)."""
    ...

def remove_file(
    path: str,
    facet: str,
    workspace: str,
    file_path: str,
    passphrase: str | None = None,
) -> None:
    """Remove ``file_path`` from the workspace working tree."""
    ...

def symlink(
    path: str,
    facet: str,
    workspace: str,
    target: str,
    link_path: str,
    passphrase: str | None = None,
) -> None:
    """Create a symbolic link at ``link_path`` whose target is ``target`` (opaque; may be dangling).
    The parent must exist; ``link_path`` must be free."""
    ...

def read_link(
    path: str,
    facet: str,
    workspace: str,
    file_path: str,
    passphrase: str | None = None,
) -> str:
    """Read the target of the symbolic link at ``file_path`` (errors if absent or not a symlink)."""
    ...

def restore_file(
    path: str,
    facet: str,
    workspace: str,
    rev: str,
    file_path: str,
    passphrase: str | None = None,
) -> None:
    """Restore one ``file_path`` in the working tree to the snapshot ``rev`` resolves to (absent =>
    removed). Working tree only; ``HEAD`` is untouched."""
    ...

def restore_path(
    path: str,
    facet: str,
    workspace: str,
    rev: str,
    prefix: str,
    passphrase: str | None = None,
) -> None:
    """Restore the subtree under ``prefix`` to the snapshot ``rev`` resolves to (a ``""`` prefix
    restores the whole tree). Working tree only."""
    ...

def cherry_pick(
    path: str,
    facet: str,
    workspace: str,
    commits: list[str],
    dry_run: bool = False,
    passphrase: str | None = None,
) -> str:
    """Cherry-pick ``commits`` (digest strings) onto the current branch, preserving each author and
    message. ``dry_run`` previews conflicts. Returns the outcome JSON."""
    ...

def revert(
    path: str,
    facet: str,
    workspace: str,
    commits: list[str],
    author: str,
    dry_run: bool = False,
    passphrase: str | None = None,
) -> str:
    """Revert ``commits`` (digest strings) as new commits authored by ``author``. ``dry_run`` previews
    conflicts. Returns the outcome JSON."""
    ...

def rebase(
    path: str,
    facet: str,
    workspace: str,
    onto: str,
    dry_run: bool = False,
    passphrase: str | None = None,
) -> str:
    """Rebase the current branch onto ``onto`` (HEAD|branch|digest), replaying first-parent commits
    linearly. ``dry_run`` previews conflicts. Returns the outcome JSON."""
    ...

def squash(
    path: str,
    facet: str,
    workspace: str,
    onto: str,
    author: str,
    message: str,
    passphrase: str | None = None,
) -> str:
    """Squash the commits after ``onto`` up to the tip into one commit (``author``/``message``); returns
    the new commit digest. ``onto`` must be an ancestor of the tip and not the tip itself."""
    ...

def read_at(
    path: str,
    facet: str,
    workspace: str,
    file_path: str,
    offset: int,
    len: int,
    passphrase: str | None = None,
) -> bytes:
    """Read up to ``len`` bytes from byte ``offset`` of ``file_path`` (reads past the end clamp)."""
    ...

def write_at(
    path: str,
    facet: str,
    workspace: str,
    file_path: str,
    offset: int,
    content: bytes,
    passphrase: str | None = None,
) -> None:
    """Write ``content`` at byte ``offset`` of ``file_path``, creating it if absent and zero-filling
    any gap."""
    ...

def truncate_file(
    path: str,
    facet: str,
    workspace: str,
    file_path: str,
    size: int,
    passphrase: str | None = None,
) -> None:
    """Resize ``file_path`` to ``size``, zero-extending or dropping bytes; a missing file is created
    zero-filled."""
    ...

def file_open(
    path: str,
    facet: str,
    workspace: str,
    file_path: str,
    mode: str,
    passphrase: str | None = None,
) -> int:
    """Open a file handle on ``file_path`` with ``mode`` (``read``|``write``|``read_write``|``append``),
    returning the handle id (valid until ``file_close``)."""
    ...

def file_read(path: str, file: int, len: int, passphrase: str | None = None) -> bytes:
    """Sequentially read up to ``len`` bytes from handle ``file`` at its cursor, advancing it."""
    ...

def file_read_at(
    path: str, file: int, offset: int, len: int, passphrase: str | None = None
) -> bytes:
    """Positionally read up to ``len`` bytes at ``offset`` from handle ``file`` without moving its
    cursor."""
    ...

def file_write(path: str, file: int, content: bytes, passphrase: str | None = None) -> int:
    """Sequentially write ``content`` to handle ``file`` at its cursor (or end of file for an append
    handle), advancing it; returns the byte count."""
    ...

def file_write_at(
    path: str, file: int, offset: int, content: bytes, passphrase: str | None = None
) -> int:
    """Positionally write ``content`` at ``offset`` of handle ``file`` without moving its cursor;
    returns the byte count."""
    ...

def file_truncate(path: str, file: int, size: int, passphrase: str | None = None) -> None:
    """Resize handle ``file`` to ``size`` bytes."""
    ...

def file_flush(path: str, file: int, passphrase: str | None = None) -> None:
    """Flush handle ``file`` (validates the handle; writes already apply per operation)."""
    ...

def file_stat(path: str, file: int, passphrase: str | None = None) -> tuple[int, int]:
    """The live ``(size, mode)`` of handle ``file``."""
    ...

def file_close(path: str, file: int, passphrase: str | None = None) -> None:
    """Close handle ``file``, releasing it (delete-on-last-close for an unlinked inode)."""
    ...

def tag_create(
    path: str,
    facet: str,
    workspace: str,
    name: str,
    rev: str,
    tagger: str | None = None,
    message: str | None = None,
    passphrase: str | None = None,
) -> str:
    """Create tag ``name`` at the commit ``rev`` resolves to (``HEAD``, a branch name, or a digest). A
    non-empty ``message`` makes an annotated tag (with ``tagger``); empty makes a lightweight tag.
    Returns the ref target digest (the commit, or the tag object)."""
    ...

def tag_list(
    path: str, facet: str, workspace: str, passphrase: str | None = None
) -> list[str]:
    """All tag names in the workspace, sorted."""
    ...

def tag_target(
    path: str, facet: str, workspace: str, name: str, passphrase: str | None = None
) -> str | None:
    """The raw ref target digest of tag ``name`` (commit for lightweight, tag object for annotated), or
    ``None`` if absent."""
    ...

def tag_delete(
    path: str, facet: str, workspace: str, name: str, passphrase: str | None = None
) -> None:
    """Delete tag ``name`` (errors if absent)."""
    ...

def tag_rename(
    path: str,
    facet: str,
    workspace: str,
    old_name: str,
    new_name: str,
    passphrase: str | None = None,
) -> None:
    """Rename tag ``old_name`` to ``new_name``, preserving its target."""
    ...

def queue_append(
    path: str,
    workspace: str,
    stream: str,
    entry: bytes,
    passphrase: str | None = None,
) -> int:
    """Append ``entry`` to ``stream`` in a workspace (by UUID or name, created with the ``queue`` facet
    if absent); returns the assigned zero-based sequence."""
    ...

def queue_get(
    path: str,
    workspace: str,
    stream: str,
    seq: int,
    passphrase: str | None = None,
) -> bytes | None:
    """Fetch the entry at ``seq`` in ``stream``, or ``None`` if out of range."""
    ...

def queue_range(
    path: str,
    workspace: str,
    stream: str,
    lo: int,
    hi: int,
    passphrase: str | None = None,
) -> list[bytes]:
    """Read the half-open range ``[lo, hi)`` of ``stream``, oldest first."""
    ...

def queue_len(
    path: str,
    workspace: str,
    stream: str,
    passphrase: str | None = None,
) -> int:
    """The number of entries in ``stream``."""
    ...

def queue_consumer_position(
    path: str,
    workspace: str,
    stream: str,
    consumer_id: str,
    passphrase: str | None = None,
) -> int:
    """The named consumer's next sequence for ``stream``; ``0`` when none is stored."""
    ...

def queue_consumer_read(
    path: str,
    workspace: str,
    stream: str,
    consumer_id: str,
    max: int,
    passphrase: str | None = None,
) -> list[bytes]:
    """Read up to ``max`` entries from the consumer's stored next sequence; does not advance."""
    ...

def queue_consumer_advance(
    path: str,
    workspace: str,
    stream: str,
    consumer_id: str,
    next_seq: int,
    passphrase: str | None = None,
) -> None:
    """Advance the named consumer's next sequence to ``next_seq``; rejects backward movement."""
    ...

def queue_consumer_reset(
    path: str,
    workspace: str,
    stream: str,
    consumer_id: str,
    next_seq: int,
    passphrase: str | None = None,
) -> None:
    """Set the named consumer's next sequence to ``next_seq``, which may move backward."""
    ...

def sql_read_table(
    path: str,
    workspace: str,
    table: str,
    passphrase: str | None = None,
) -> bytes:
    """Read the staged ``table`` of sql-facet workspace ``workspace`` as canonical-CBOR
    (``{"columns", "rows"}``). ``table`` is the staged table path, e.g.
    ``.loom/facets/sql/<db>/tables/<name>``."""
    ...

def sql_read_table_at(
    path: str,
    workspace: str,
    table: str,
    commit: str,
    passphrase: str | None = None,
) -> bytes:
    """Read ``table`` from historical commit ``commit`` without changing the working tree."""
    ...

def sql_index_scan(
    path: str,
    workspace: str,
    table: str,
    index: str,
    prefix: bytes,
    passphrase: str | None = None,
) -> bytes:
    """Scan secondary ``index`` on ``table`` for the lookup ``prefix`` (a canonical-CBOR cell array; an
    empty prefix is the canonical CBOR of an empty array), returning the matching rows as canonical-CBOR
    (``{"columns", "rows"}``)."""
    ...

def sql_index_scan_at(
    path: str,
    workspace: str,
    table: str,
    index: str,
    prefix: bytes,
    commit: str,
    passphrase: str | None = None,
) -> bytes:
    """Scan secondary ``index`` on ``table`` from historical commit ``commit``."""
    ...

def sql_blame(
    path: str,
    workspace: str,
    branch: str,
    table: str,
    passphrase: str | None = None,
) -> bytes:
    """Blame the rows of ``table`` on ``branch`` for sql-facet workspace ``workspace``:
    each current row plus the commit that last set it, as canonical-CBOR
    (``{"rows": [{"commit", "values"}]}``)."""
    ...

def sql_diff(
    path: str,
    workspace: str,
    table: str,
    from_commit: str,
    to_commit: str,
    passphrase: str | None = None,
) -> bytes:
    """Row-level diff of ``table`` between commits ``from_commit`` and ``to_commit`` (content addresses),
    as canonical-CBOR (``{"diffs": [...]}``)."""
    ...

def sql_table_diff(
    path: str,
    workspace: str,
    table: str,
    from_commit: str,
    to_commit: str,
    passphrase: str | None = None,
) -> bytes:
    """Schema-aware table diff between commits. ``sql_diff`` remains row-only."""
    ...

def vcs_blame(
    path: str,
    workspace: str,
    branch: str,
    passphrase: str | None = None,
) -> bytes:
    """Workspace/entry-level blame for ``branch`` (which commit last set each path), as canonical-CBOR
    (``{"kind": "PathBlame", "paths": [...]}``)."""
    ...

def vcs_diff(
    path: str,
    workspace: str,
    from_commit: str,
    to_commit: str,
    passphrase: str | None = None,
) -> bytes:
    """Cross-facet structural diff between commits as the raw ``LMDIFF`` canonical-CBOR envelope."""
    ...

def watch_subscribe(
    path: str,
    workspace: str,
    branch: str,
    facet: str | None = None,
    path_prefix: str | None = None,
    change_kinds: list[str] | None = None,
    from_commit: str | None = None,
    passphrase: str | None = None,
) -> str:
    """Subscribe to workspace history changes and return an opaque watch cursor string."""
    ...

def watch_poll(
    path: str,
    cursor: str,
    max: int,
    passphrase: str | None = None,
) -> bytes:
    """Poll an opaque watch cursor and return a canonical-CBOR ``loom.watch.batch.v1`` batch."""
    ...

def result_to_json(bytes: bytes) -> str:
    """Render a canonical-CBOR result buffer to debug JSON (inspection only, not the typed API). Faithful
    cells decode to type-tagged scalars such as ``{"Int": 1}`` and ``{"Text": "hi"}``."""
    ...

def result_to_bridge_json(bytes: bytes) -> str:
    """Render a canonical-CBOR result buffer to lossless bridge JSON (the React Native bridge projection;
    not the normative wire form). Values a JSON number cannot hold exactly cross as tagged ``$``-prefixed
    objects."""
    ...

class LoomSql:
    """An open SQL session over a workspace SQL facet in a ``.loom``.

    Exposes the whole versioned tabular + SQL stack to Python; mirrors the C-ABI SQL session.
    """

    def __init__(self, loom_path: str, ns_name: str, db: str) -> None:
        """Open ``loom_path`` and start a SQL session over workspace ``ns_name``'s SQL facet
        (created if absent), database ``db``."""
        ...

    @staticmethod
    def open_encrypted(
        loom_path: str,
        ns_name: str,
        db: str,
        passphrase: str,
    ) -> "LoomSql": ...

    @staticmethod
    def authenticated(
        loom_path: str,
        ns_name: str,
        db: str,
        auth_principal: str,
        auth_passphrase: str,
    ) -> "LoomSql": ...

    @staticmethod
    def open_encrypted_authenticated(
        loom_path: str,
        ns_name: str,
        db: str,
        passphrase: str,
        auth_principal: str,
        auth_passphrase: str,
    ) -> "LoomSql": ...

    def exec(self, sql: str) -> list[dict[str, Any]]:
        """Run one or more ``;``-separated SQL statements and return **typed** results: a list of
        statement dicts (``{"kind": ..., ...}``). A ``select`` carries ``columns`` and ``rows`` of
        idiomatic cells - ``int`` (arbitrary precision, so 64/128-bit values are exact), ``float``,
        ``bytes``, ``str``, ``bool``, ``None``, and ``decimal.Decimal`` for an exact decimal.
        Mutations are staged and persisted; call ``commit`` to record one."""
        ...

    def exec_json(self, sql: str) -> str:
        """Run SQL; returns a JSON array of the result payloads (debug/admin form, rendered from the
        canonical CBOR - not the type-faithful API; use ``exec``)."""
        ...

    def exec_bytes(self, sql: str) -> bytes:
        """Run SQL; returns the result payloads as canonical CBOR ``bytes``."""
        ...

    def query(self, sql: str) -> "LoomRows":
        """Run a ``SELECT`` and return a lazy row iterator (``for row in db.query(sql)``); each row is a
        list of idiomatic cells. Statements that mutate state are rejected."""
        ...

    def commit(self, message: str, author: str) -> str:
        """Commit the staged database state onto the workspace's current branch; returns the new
        commit's content address (``"algo:hex"``)."""
        ...

class LoomRows:
    """A lazy iterator over a ``SELECT``'s rows (the iterator protocol). Yield each row as a list of
    idiomatic cells; obtained from :meth:`LoomSql.query`."""

    def __iter__(self) -> "LoomRows": ...
    def __next__(self) -> list[Any]: ...

class LoomSqlBatch:
    """An explicit transaction/batch scope: holds the ``.loom`` open across statements so
    a SQL transaction (``BEGIN``/``COMMIT``/``ROLLBACK``) can span ``exec`` calls; ``commit`` makes the
    accumulated changes durable with one atomic save. Call ``close`` to release the write lock."""

    def __init__(self, loom_path: str, ns_name: str, db: str) -> None: ...
    @staticmethod
    def open_encrypted(
        loom_path: str,
        ns_name: str,
        db: str,
        passphrase: str,
    ) -> "LoomSqlBatch": ...

    @staticmethod
    def authenticated(
        loom_path: str,
        ns_name: str,
        db: str,
        auth_principal: str,
        auth_passphrase: str,
    ) -> "LoomSqlBatch": ...

    @staticmethod
    def open_encrypted_authenticated(
        loom_path: str,
        ns_name: str,
        db: str,
        passphrase: str,
        auth_principal: str,
        auth_passphrase: str,
    ) -> "LoomSqlBatch": ...

    def exec(self, sql: str) -> list[dict[str, Any]]:
        """Run SQL in the batch (including ``BEGIN``/``COMMIT``/``ROLLBACK``); typed results."""
        ...

    def exec_bytes(self, sql: str) -> bytes:
        """Run SQL in the batch; canonical-CBOR bytes."""
        ...

    def exec_json(self, sql: str) -> str:
        """Run SQL in the batch; JSON debug form."""
        ...

    def commit(self) -> None:
        """Persist the batch's changes with one atomic save (no history entry). Rejected while a SQL
        transaction is open. The batch stays open."""
        ...

    def commit_vcs(self, message: str, author: str) -> str:
        """Like ``commit``, but also records a VCS commit; returns its content address."""
        ...

    def abort(self) -> None:
        """Discard un-persisted in-memory changes (and any open SQL transaction); the batch stays open."""
        ...

    def close(self) -> None:
        """Release the write lock and free the batch. Closing without a commit discards changes."""
        ...

class AsyncLoomSql:
    """An ``asyncio`` wrapper around :class:`LoomSql` (the asyncio form of the async ABI).

    The native calls release the GIL, so :func:`asyncio.to_thread` runs each truly off the event loop.
    """

    def __init__(self, loom_path: str, ns_name: str, db: str) -> None: ...
    async def exec(self, sql: str) -> list[dict[str, Any]]: ...
    async def exec_json(self, sql: str) -> str: ...
    async def exec_bytes(self, sql: str) -> bytes: ...
    async def commit(self, message: str, author: str) -> str: ...
