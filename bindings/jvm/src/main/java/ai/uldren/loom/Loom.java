// JVM binding for Uldren Loom via the Foreign Function & Memory API (JDK 22+).
// Licensed under BUSL-1.1 (see the repo LICENSE). (c) Uldren Technologies LLC.
package ai.uldren.loom;

import java.lang.foreign.Arena;
import java.lang.foreign.FunctionDescriptor;
import java.lang.foreign.Linker;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.SymbolLookup;
import java.lang.foreign.ValueLayout;
import java.lang.invoke.MethodHandle;

/** Thin FFM wrapper over the Uldren Loom C ABI (libuldren_loom). */
public final class Loom {
    static final Linker LINKER = Linker.nativeLinker();
    static final SymbolLookup LOOKUP = loadLibrary();

    static final MethodHandle LOOM_VERSION = LINKER.downcallHandle(
            LOOKUP.find("loom_version").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.ADDRESS));

    static final MethodHandle LOOM_BLOB_DIGEST = LINKER.downcallHandle(
            LOOKUP.find("loom_blob_digest").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG));

    static final MethodHandle LOOM_STRING_FREE = LINKER.downcallHandle(
            LOOKUP.find("loom_string_free").orElseThrow(),
            FunctionDescriptor.ofVoid(ValueLayout.ADDRESS));

    static final MethodHandle LOOM_LAST_ERROR = LINKER.downcallHandle(
            LOOKUP.find("loom_last_error").orElseThrow(),
            FunctionDescriptor.ofVoid(ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_SQL_OPEN = LINKER.downcallHandle(
            LOOKUP.find("loom_sql_open").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_SQL_OPEN_KEYED = LINKER.downcallHandle(
            LOOKUP.find("loom_sql_open_keyed").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                    ValueLayout.ADDRESS));

    static final MethodHandle LOOM_SQL_OPEN_WITH_KEK = LINKER.downcallHandle(
            LOOKUP.find("loom_sql_open_with_kek").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                    ValueLayout.ADDRESS));

    static final MethodHandle LOOM_SQL_OPEN_AUTHENTICATED = LINKER.downcallHandle(
            LOOKUP.find("loom_sql_open_authenticated").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                    ValueLayout.ADDRESS));

    static final MethodHandle LOOM_SQL_OPEN_KEYED_AUTHENTICATED = LINKER.downcallHandle(
            LOOKUP.find("loom_sql_open_keyed_authenticated").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                    ValueLayout.ADDRESS));

    static final MethodHandle LOOM_SQL_OPEN_WITH_KEK_AUTHENTICATED = LINKER.downcallHandle(
            LOOKUP.find("loom_sql_open_with_kek_authenticated").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                    ValueLayout.ADDRESS));

    static final MethodHandle LOOM_CREATE = LINKER.downcallHandle(
            LOOKUP.find("loom_create").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG));

    static final MethodHandle LOOM_CREATE_WITH_KEK = LINKER.downcallHandle(
            LOOKUP.find("loom_create_with_kek").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG));

    static final MethodHandle LOOM_OPEN = LINKER.downcallHandle(
            LOOKUP.find("loom_open").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_OPEN_KEYED = LINKER.downcallHandle(
            LOOKUP.find("loom_open_keyed").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.JAVA_LONG, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_OPEN_WITH_KEK = LINKER.downcallHandle(
            LOOKUP.find("loom_open_with_kek").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.JAVA_LONG, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_CLOSE = LINKER.downcallHandle(
            LOOKUP.find("loom_close").orElseThrow(),
            FunctionDescriptor.ofVoid(ValueLayout.ADDRESS));

    static final MethodHandle LOOM_DAEMON_STATUS_JSON = LINKER.downcallHandle(
            LOOKUP.find("loom_daemon_status_json").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_DAEMON_SESSION_ATTACH = LINKER.downcallHandle(
            LOOKUP.find("loom_daemon_session_attach").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_DAEMON_SESSION_DETACH = LINKER.downcallHandle(
            LOOKUP.find("loom_daemon_session_detach").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_DAEMON_PIN_ADD = LINKER.downcallHandle(
            LOOKUP.find("loom_daemon_pin_add").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_DAEMON_PIN_REMOVE = LINKER.downcallHandle(
            LOOKUP.find("loom_daemon_pin_remove").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_LOCK_ACQUIRE_JSON = LINKER.downcallHandle(
            LOOKUP.find("loom_lock_acquire_json").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.JAVA_INT, ValueLayout.JAVA_INT, ValueLayout.JAVA_LONG,
                    ValueLayout.JAVA_LONG, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_LOCK_REFRESH_JSON = LINKER.downcallHandle(
            LOOKUP.find("loom_lock_refresh_json").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.JAVA_INT, ValueLayout.JAVA_INT, ValueLayout.JAVA_LONG,
                    ValueLayout.JAVA_LONG, ValueLayout.JAVA_LONG, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_LOCK_RELEASE = LINKER.downcallHandle(
            LOOKUP.find("loom_lock_release").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.JAVA_INT, ValueLayout.JAVA_INT, ValueLayout.JAVA_LONG,
                    ValueLayout.JAVA_LONG));

    static final MethodHandle LOOM_AUTHENTICATE_PASSPHRASE = LINKER.downcallHandle(
            LOOKUP.find("loom_authenticate_passphrase").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.JAVA_LONG));

    static final MethodHandle LOOM_IDENTITY_LIST_JSON = LINKER.downcallHandle(
            LOOKUP.find("loom_identity_list_json").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_IDENTITY_ADD_PRINCIPAL = LINKER.downcallHandle(
            LOOKUP.find("loom_identity_add_principal").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_IDENTITY_RENAME_PRINCIPAL_HANDLE = LINKER.downcallHandle(
            LOOKUP.find("loom_identity_rename_principal_handle").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS));

    static final MethodHandle LOOM_IDENTITY_SET_PASSPHRASE = LINKER.downcallHandle(
            LOOKUP.find("loom_identity_set_passphrase").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.JAVA_LONG));

    static final MethodHandle LOOM_IDENTITY_REMOVE_PRINCIPAL = LINKER.downcallHandle(
            LOOKUP.find("loom_identity_remove_principal").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_IDENTITY_ASSIGN_ROLE = LINKER.downcallHandle(
            LOOKUP.find("loom_identity_assign_role").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS));

    static final MethodHandle LOOM_IDENTITY_REVOKE_ROLE = LINKER.downcallHandle(
            LOOKUP.find("loom_identity_revoke_role").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_IDENTITY_CREATE_EXTERNAL_CREDENTIAL = LINKER.downcallHandle(
            LOOKUP.find("loom_identity_create_external_credential").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_IDENTITY_REVOKE_EXTERNAL_CREDENTIAL = LINKER.downcallHandle(
            LOOKUP.find("loom_identity_revoke_external_credential").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_IDENTITY_ADD_PUBLIC_KEY = LINKER.downcallHandle(
            LOOKUP.find("loom_identity_add_public_key").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS));

    static final MethodHandle LOOM_IDENTITY_REVOKE_PUBLIC_KEY = LINKER.downcallHandle(
            LOOKUP.find("loom_identity_revoke_public_key").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_ACL_LIST_JSON = LINKER.downcallHandle(
            LOOKUP.find("loom_acl_list_json").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_ACL_GRANT = LINKER.downcallHandle(
            LOOKUP.find("loom_acl_grant").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.JAVA_INT,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.JAVA_INT));

    static final MethodHandle LOOM_ACL_REVOKE = LINKER.downcallHandle(
            LOOKUP.find("loom_acl_revoke").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.JAVA_INT,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.JAVA_INT, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_ACL_GRANT_SCOPED = LINKER.downcallHandle(
            LOOKUP.find("loom_acl_grant_scoped").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.JAVA_INT,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_ACL_GRANT_SCOPED_PREDICATE = LINKER.downcallHandle(
            LOOKUP.find("loom_acl_grant_scoped_predicate").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.JAVA_INT,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_ACL_REVOKE_SCOPED = LINKER.downcallHandle(
            LOOKUP.find("loom_acl_revoke_scoped").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.JAVA_INT,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS));

    static final MethodHandle LOOM_ACL_REVOKE_SCOPED_PREDICATE = LINKER.downcallHandle(
            LOOKUP.find("loom_acl_revoke_scoped_predicate").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.JAVA_INT,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_PROTECTED_REF_LIST_JSON = LINKER.downcallHandle(
            LOOKUP.find("loom_protected_ref_list_json").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS));

    static final MethodHandle LOOM_PROTECTED_REF_GET_JSON = LINKER.downcallHandle(
            LOOKUP.find("loom_protected_ref_get_json").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_PROTECTED_REF_SET = LINKER.downcallHandle(
            LOOKUP.find("loom_protected_ref_set").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.JAVA_BOOLEAN, ValueLayout.JAVA_BOOLEAN,
                    ValueLayout.JAVA_BOOLEAN, ValueLayout.JAVA_INT, ValueLayout.JAVA_BOOLEAN,
                    ValueLayout.JAVA_BOOLEAN));

    static final MethodHandle LOOM_PROTECTED_REF_REMOVE = LINKER.downcallHandle(
            LOOKUP.find("loom_protected_ref_remove").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_WORKSPACE_CREATE = LINKER.downcallHandle(
            LOOKUP.find("loom_workspace_create").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_WORKSPACE_LIST_JSON = LINKER.downcallHandle(
            LOOKUP.find("loom_workspace_list_json").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_WORKSPACE_RENAME = LINKER.downcallHandle(
            LOOKUP.find("loom_workspace_rename").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS));

    static final MethodHandle LOOM_WORKSPACE_DELETE = LINKER.downcallHandle(
            LOOKUP.find("loom_workspace_delete").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_QUEUE_APPEND = LINKER.downcallHandle(
            LOOKUP.find("loom_queue_append").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                    ValueLayout.ADDRESS));

    static final MethodHandle LOOM_QUEUE_GET = LINKER.downcallHandle(
            LOOKUP.find("loom_queue_get").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.JAVA_LONG, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_QUEUE_RANGE = LINKER.downcallHandle(
            LOOKUP.find("loom_queue_range").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.JAVA_LONG, ValueLayout.JAVA_LONG,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_QUEUE_LEN = LINKER.downcallHandle(
            LOOKUP.find("loom_queue_len").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_QUEUE_CONSUMER_POSITION = LINKER.downcallHandle(
            LOOKUP.find("loom_queue_consumer_position").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_QUEUE_CONSUMER_READ = LINKER.downcallHandle(
            LOOKUP.find("loom_queue_consumer_read").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_INT,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_QUEUE_CONSUMER_ADVANCE = LINKER.downcallHandle(
            LOOKUP.find("loom_queue_consumer_advance").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG));

    static final MethodHandle LOOM_QUEUE_CONSUMER_RESET = LINKER.downcallHandle(
            LOOKUP.find("loom_queue_consumer_reset").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG));

    static final MethodHandle LOOM_SQL_READ_TABLE = LINKER.downcallHandle(
            LOOKUP.find("loom_sql_read_table").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_SQL_INDEX_SCAN = LINKER.downcallHandle(
            LOOKUP.find("loom_sql_index_scan").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.JAVA_LONG, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_SQL_BLAME = LINKER.downcallHandle(
            LOOKUP.find("loom_sql_blame").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS));

    static final MethodHandle LOOM_SQL_DIFF = LINKER.downcallHandle(
            LOOKUP.find("loom_sql_diff").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS));

    static final MethodHandle LOOM_VCS_BLAME = LINKER.downcallHandle(
            LOOKUP.find("loom_vcs_blame").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_VCS_DIFF = LINKER.downcallHandle(
            LOOKUP.find("loom_vcs_diff").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS));

    static final MethodHandle LOOM_WATCH_SUBSCRIBE = LINKER.downcallHandle(
            LOOKUP.find("loom_watch_subscribe").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_WATCH_POLL = LINKER.downcallHandle(
            LOOKUP.find("loom_watch_poll").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_SQL_EXEC = LINKER.downcallHandle(
            LOOKUP.find("loom_sql_exec").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_RESULT_TO_JSON = LINKER.downcallHandle(
            LOOKUP.find("loom_result_to_json").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                    ValueLayout.ADDRESS));

    static final MethodHandle LOOM_BYTES_FREE = LINKER.downcallHandle(
            LOOKUP.find("loom_bytes_free").orElseThrow(),
            FunctionDescriptor.ofVoid(ValueLayout.ADDRESS, ValueLayout.JAVA_LONG));

    static final MethodHandle LOOM_CAPABILITIES = LINKER.downcallHandle(
            LOOKUP.find("loom_capabilities").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_RUNTIME_PROFILE = LINKER.downcallHandle(
            LOOKUP.find("loom_runtime_profile").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_STUDIO_SURFACE_CATALOG_JSON = LINKER.downcallHandle(
            LOOKUP.find("loom_studio_surface_catalog_json").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS));

    static final MethodHandle LOOM_EXEC_CBOR = LINKER.downcallHandle(
            LOOKUP.find("loom_exec_cbor").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.JAVA_LONG, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_CAS_PUT = LINKER.downcallHandle(
            LOOKUP.find("loom_cas_put").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.JAVA_LONG, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_CAS_GET = LINKER.downcallHandle(
            LOOKUP.find("loom_cas_get").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_CAS_HAS = LINKER.downcallHandle(
            LOOKUP.find("loom_cas_has").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_CAS_DELETE = LINKER.downcallHandle(
            LOOKUP.find("loom_cas_delete").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_CAS_LIST_JSON = LINKER.downcallHandle(
            LOOKUP.find("loom_cas_list_json").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS));

    static final MethodHandle LOOM_MEETINGS_IMPORT_SNAPSHOT = LINKER.downcallHandle(
            LOOKUP.find("loom_meetings_import_snapshot").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                    ValueLayout.JAVA_INT, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_MEETINGS_SOURCE_READ = LINKER.downcallHandle(
            LOOKUP.find("loom_meetings_source_read").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS));

    static final MethodHandle LOOM_FS_IMPORT = LINKER.downcallHandle(
            LOOKUP.find("loom_fs_import").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.JAVA_INT, ValueLayout.JAVA_INT,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_FS_EXPORT = LINKER.downcallHandle(
            LOOKUP.find("loom_fs_export").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_INT,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_ARCHIVE_IMPORT = LINKER.downcallHandle(
            LOOKUP.find("loom_archive_import").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_INT,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_ARCHIVE_EXPORT = LINKER.downcallHandle(
            LOOKUP.find("loom_archive_export").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_CAR_IMPORT = LINKER.downcallHandle(
            LOOKUP.find("loom_car_import").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_CAR_EXPORT = LINKER.downcallHandle(
            LOOKUP.find("loom_car_export").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.JAVA_INT, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS));

    static final MethodHandle LOOM_KV_PUT = LINKER.downcallHandle(
            LOOKUP.find("loom_kv_put").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                    ValueLayout.ADDRESS, ValueLayout.JAVA_LONG));

    static final MethodHandle LOOM_KV_GET = LINKER.downcallHandle(
            LOOKUP.find("loom_kv_get").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_KV_DELETE = LINKER.downcallHandle(
            LOOKUP.find("loom_kv_delete").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                    ValueLayout.ADDRESS));

    static final MethodHandle LOOM_KV_LIST_CBOR = LINKER.downcallHandle(
            LOOKUP.find("loom_kv_list_cbor").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_KV_RANGE_CBOR = LINKER.downcallHandle(
            LOOKUP.find("loom_kv_range_cbor").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                    ValueLayout.ADDRESS, ValueLayout.JAVA_LONG, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS));

    // --- Graph facet handles. ---

    static final MethodHandle LOOM_GRAPH_UPSERT_NODE = LINKER.downcallHandle(
            LOOKUP.find("loom_graph_upsert_node").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.JAVA_LONG));

    static final MethodHandle LOOM_GRAPH_GET_NODE = LINKER.downcallHandle(
            LOOKUP.find("loom_graph_get_node").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_GRAPH_REMOVE_NODE = LINKER.downcallHandle(
            LOOKUP.find("loom_graph_remove_node").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_INT));

    static final MethodHandle LOOM_GRAPH_UPSERT_EDGE = LINKER.downcallHandle(
            LOOKUP.find("loom_graph_upsert_edge").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.JAVA_LONG));

    static final MethodHandle LOOM_GRAPH_GET_EDGE = LINKER.downcallHandle(
            LOOKUP.find("loom_graph_get_edge").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_GRAPH_REMOVE_EDGE = LINKER.downcallHandle(
            LOOKUP.find("loom_graph_remove_edge").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_GRAPH_NEIGHBORS_CBOR = LINKER.downcallHandle(
            LOOKUP.find("loom_graph_neighbors_cbor").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS));

    static final MethodHandle LOOM_GRAPH_OUT_EDGES_CBOR = LINKER.downcallHandle(
            LOOKUP.find("loom_graph_out_edges_cbor").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS));

    static final MethodHandle LOOM_GRAPH_IN_EDGES_CBOR = LINKER.downcallHandle(
            LOOKUP.find("loom_graph_in_edges_cbor").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS));

    static final MethodHandle LOOM_GRAPH_REACHABLE_CBOR = LINKER.downcallHandle(
            LOOKUP.find("loom_graph_reachable_cbor").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_GRAPH_SHORTEST_PATH_CBOR = LINKER.downcallHandle(
            LOOKUP.find("loom_graph_shortest_path_cbor").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS));

    // --- Vector facet handles. ---

    static final MethodHandle LOOM_VECTOR_CREATE = LINKER.downcallHandle(
            LOOKUP.find("loom_vector_create").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.JAVA_LONG, ValueLayout.JAVA_INT));

    static final MethodHandle LOOM_VECTOR_UPSERT = LINKER.downcallHandle(
            LOOKUP.find("loom_vector_upsert").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.JAVA_LONG, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG));

    static final MethodHandle LOOM_VECTOR_UPSERT_SOURCE = LINKER.downcallHandle(
            LOOKUP.find("loom_vector_upsert_source").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.JAVA_LONG, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                    ValueLayout.ADDRESS, ValueLayout.JAVA_LONG, ValueLayout.ADDRESS,
                    ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.JAVA_INT));

    static final MethodHandle LOOM_VECTOR_GET = LINKER.downcallHandle(
            LOOKUP.find("loom_vector_get").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_VECTOR_SOURCE_TEXT = LINKER.downcallHandle(
            LOOKUP.find("loom_vector_source_text").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_VECTOR_EMBEDDING_MODEL_CBOR = LINKER.downcallHandle(
            LOOKUP.find("loom_vector_embedding_model_cbor").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS));

    static final MethodHandle LOOM_VECTOR_IDS_CBOR = LINKER.downcallHandle(
            LOOKUP.find("loom_vector_ids_cbor").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_INT,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_VECTOR_METADATA_INDEX_KEYS_CBOR = LINKER.downcallHandle(
            LOOKUP.find("loom_vector_metadata_index_keys_cbor").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_VECTOR_CREATE_METADATA_INDEX = LINKER.downcallHandle(
            LOOKUP.find("loom_vector_create_metadata_index").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_VECTOR_DROP_METADATA_INDEX = LINKER.downcallHandle(
            LOOKUP.find("loom_vector_drop_metadata_index").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_VECTOR_DELETE = LINKER.downcallHandle(
            LOOKUP.find("loom_vector_delete").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_VECTOR_SEARCH_CBOR = LINKER.downcallHandle(
            LOOKUP.find("loom_vector_search_cbor").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                    ValueLayout.JAVA_LONG, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_VECTOR_SEARCH_POLICY_CBOR = LINKER.downcallHandle(
            LOOKUP.find("loom_vector_search_policy_cbor").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                    ValueLayout.JAVA_LONG, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                    ValueLayout.JAVA_INT, ValueLayout.JAVA_LONG, ValueLayout.JAVA_LONG,
                    ValueLayout.JAVA_LONG, ValueLayout.JAVA_LONG, ValueLayout.JAVA_LONG,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    // --- Columnar facet handles. ---

    static final MethodHandle LOOM_COLUMNAR_CREATE = LINKER.downcallHandle(
            LOOKUP.find("loom_columnar_create").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                    ValueLayout.JAVA_LONG));

    static final MethodHandle LOOM_COLUMNAR_APPEND = LINKER.downcallHandle(
            LOOKUP.find("loom_columnar_append").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG));

    static final MethodHandle LOOM_COLUMNAR_SCAN_CBOR = LINKER.downcallHandle(
            LOOKUP.find("loom_columnar_scan_cbor").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_COLUMNAR_COLUMNS_CBOR = LINKER.downcallHandle(
            LOOKUP.find("loom_columnar_columns_cbor").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_COLUMNAR_ROWS = LINKER.downcallHandle(
            LOOKUP.find("loom_columnar_rows").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_COLUMNAR_COMPACT = LINKER.downcallHandle(
            LOOKUP.find("loom_columnar_compact").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS));

    static final MethodHandle LOOM_COLUMNAR_INSPECT_CBOR = LINKER.downcallHandle(
            LOOKUP.find("loom_columnar_inspect_cbor").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_COLUMNAR_SOURCE_DIGEST_CBOR = LINKER.downcallHandle(
            LOOKUP.find("loom_columnar_source_digest_cbor").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_COLUMNAR_SELECT_CBOR = LINKER.downcallHandle(
            LOOKUP.find("loom_columnar_select_cbor").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                    ValueLayout.ADDRESS, ValueLayout.JAVA_LONG, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS));

    static final MethodHandle LOOM_COLUMNAR_AGGREGATE_CBOR = LINKER.downcallHandle(
            LOOKUP.find("loom_columnar_aggregate_cbor").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                    ValueLayout.ADDRESS, ValueLayout.JAVA_LONG, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS));

    // --- Dataframe facet handles. ---

    static final MethodHandle LOOM_DATAFRAME_CREATE = LINKER.downcallHandle(
            LOOKUP.find("loom_dataframe_create").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG));

    static final MethodHandle LOOM_DATAFRAME_COLLECT_CBOR = LINKER.downcallHandle(
            LOOKUP.find("loom_dataframe_collect_cbor").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_DATAFRAME_PREVIEW_CBOR = LINKER.downcallHandle(
            LOOKUP.find("loom_dataframe_preview_cbor").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.JAVA_LONG, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS));

    static final MethodHandle LOOM_DATAFRAME_MATERIALIZE = LINKER.downcallHandle(
            LOOKUP.find("loom_dataframe_materialize").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_DATAFRAME_PLAN_DIGEST = LINKER.downcallHandle(
            LOOKUP.find("loom_dataframe_plan_digest").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_DATAFRAME_SOURCE_DIGESTS_CBOR = LINKER.downcallHandle(
            LOOKUP.find("loom_dataframe_source_digests_cbor").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    // --- Search facet handles. ---

    static final MethodHandle LOOM_SEARCH_CREATE = LINKER.downcallHandle(
            LOOKUP.find("loom_search_create").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG));

    static final MethodHandle LOOM_SEARCH_INDEX = LINKER.downcallHandle(
            LOOKUP.find("loom_search_index").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                    ValueLayout.ADDRESS, ValueLayout.JAVA_LONG));

    static final MethodHandle LOOM_SEARCH_GET = LINKER.downcallHandle(
            LOOKUP.find("loom_search_get").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_SEARCH_DELETE = LINKER.downcallHandle(
            LOOKUP.find("loom_search_delete").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                    ValueLayout.ADDRESS));

    static final MethodHandle LOOM_SEARCH_IDS_CBOR = LINKER.downcallHandle(
            LOOKUP.find("loom_search_ids_cbor").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                    ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_SEARCH_REMAP = LINKER.downcallHandle(
            LOOKUP.find("loom_search_remap").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG));

    static final MethodHandle LOOM_SEARCH_QUERY_CBOR = LINKER.downcallHandle(
            LOOKUP.find("loom_search_query_cbor").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    // --- Document / Time-series / Ledger facet handles. ---

    static final MethodHandle LOOM_DOC_PUT_TEXT = LINKER.downcallHandle(
            LOOKUP.find("loom_doc_put_text").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_DOC_GET_TEXT = LINKER.downcallHandle(
            LOOKUP.find("loom_doc_get_text").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_DOC_PUT_BINARY = LINKER.downcallHandle(
            LOOKUP.find("loom_doc_put_binary").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.JAVA_LONG, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS));

    static final MethodHandle LOOM_DOC_GET_BINARY = LINKER.downcallHandle(
            LOOKUP.find("loom_doc_get_binary").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS));

    static final MethodHandle LOOM_DOC_DELETE = LINKER.downcallHandle(
            LOOKUP.find("loom_doc_delete").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_DOC_LIST_BINARY_CBOR = LINKER.downcallHandle(
            LOOKUP.find("loom_doc_list_binary_cbor").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_DOC_INDEX_CREATE = LINKER.downcallHandle(
            LOOKUP.find("loom_doc_index_create").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.JAVA_INT));

    static final MethodHandle LOOM_DOC_INDEX_CREATE_JSON = LINKER.downcallHandle(
            LOOKUP.find("loom_doc_index_create_json").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG));

    static final MethodHandle LOOM_DOC_INDEX_DROP = LINKER.downcallHandle(
            LOOKUP.find("loom_doc_index_drop").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_DOC_INDEX_REBUILD = LINKER.downcallHandle(
            LOOKUP.find("loom_doc_index_rebuild").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_DOC_INDEX_LIST_JSON = LINKER.downcallHandle(
            LOOKUP.find("loom_doc_index_list_json").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_DOC_INDEX_STATUS_JSON = LINKER.downcallHandle(
            LOOKUP.find("loom_doc_index_status_json").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_DOC_FIND_JSON = LINKER.downcallHandle(
            LOOKUP.find("loom_doc_find_json").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.JAVA_LONG, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_DOC_QUERY_JSON = LINKER.downcallHandle(
            LOOKUP.find("loom_doc_query_json").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_TS_PUT = LINKER.downcallHandle(
            LOOKUP.find("loom_ts_put").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.JAVA_LONG, ValueLayout.ADDRESS,
                    ValueLayout.JAVA_LONG));

    static final MethodHandle LOOM_TS_GET = LINKER.downcallHandle(
            LOOKUP.find("loom_ts_get").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.JAVA_LONG, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_TS_RANGE_CBOR = LINKER.downcallHandle(
            LOOKUP.find("loom_ts_range_cbor").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.JAVA_LONG, ValueLayout.JAVA_LONG,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_TS_LATEST = LINKER.downcallHandle(
            LOOKUP.find("loom_ts_latest").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_METRICS_PUT_DESCRIPTOR = LINKER.downcallHandle(
            LOOKUP.find("loom_metrics_put_descriptor").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.JAVA_LONG));

    static final MethodHandle LOOM_METRICS_GET_DESCRIPTOR = LINKER.downcallHandle(
            LOOKUP.find("loom_metrics_get_descriptor").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_METRICS_PUT_OBSERVATION = LINKER.downcallHandle(
            LOOKUP.find("loom_metrics_put_observation").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG));

    static final MethodHandle LOOM_METRICS_QUERY_CBOR = LINKER.downcallHandle(
            LOOKUP.find("loom_metrics_query_cbor").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.JAVA_LONG, ValueLayout.JAVA_LONG,
                    ValueLayout.JAVA_INT, ValueLayout.JAVA_INT, ValueLayout.JAVA_INT,
                    ValueLayout.JAVA_LONG, ValueLayout.JAVA_LONG, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS));

    static final MethodHandle LOOM_LOGS_PUT_RECORD = LINKER.downcallHandle(
            LOOKUP.find("loom_logs_put_record").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.JAVA_LONG, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS));

    static final MethodHandle LOOM_LOGS_GET_RECORD = LINKER.downcallHandle(
            LOOKUP.find("loom_logs_get_record").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_LOGS_QUERY_CBOR = LINKER.downcallHandle(
            LOOKUP.find("loom_logs_query_cbor").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.JAVA_LONG, ValueLayout.JAVA_LONG, ValueLayout.JAVA_INT,
                    ValueLayout.JAVA_LONG, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_TRACES_PUT_SPAN = LINKER.downcallHandle(
            LOOKUP.find("loom_traces_put_span").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.JAVA_LONG));

    static final MethodHandle LOOM_TRACES_GET_SPAN = LINKER.downcallHandle(
            LOOKUP.find("loom_traces_get_span").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_TRACES_TRACE_SPANS_CBOR = LINKER.downcallHandle(
            LOOKUP.find("loom_traces_trace_spans_cbor").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.JAVA_INT, ValueLayout.JAVA_LONG,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_TRACES_QUERY_CBOR = LINKER.downcallHandle(
            LOOKUP.find("loom_traces_query_cbor").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.JAVA_LONG, ValueLayout.JAVA_LONG, ValueLayout.JAVA_INT,
                    ValueLayout.JAVA_LONG, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_LEDGER_APPEND = LINKER.downcallHandle(
            LOOKUP.find("loom_ledger_append").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                    ValueLayout.ADDRESS));

    static final MethodHandle LOOM_LEDGER_GET = LINKER.downcallHandle(
            LOOKUP.find("loom_ledger_get").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.JAVA_LONG, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_LEDGER_HEAD = LINKER.downcallHandle(
            LOOKUP.find("loom_ledger_head").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_LEDGER_LEN = LINKER.downcallHandle(
            LOOKUP.find("loom_ledger_len").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_LEDGER_VERIFY = LINKER.downcallHandle(
            LOOKUP.find("loom_ledger_verify").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS));

    // --- Calendar facet handles. ---

    static final MethodHandle LOOM_CAL_CREATE_COLLECTION = LINKER.downcallHandle(
            LOOKUP.find("loom_cal_create_collection").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS));

    static final MethodHandle LOOM_CAL_DELETE_COLLECTION = LINKER.downcallHandle(
            LOOKUP.find("loom_cal_delete_collection").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_CAL_LIST_COLLECTIONS = LINKER.downcallHandle(
            LOOKUP.find("loom_cal_list_collections").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_CAL_PUT_ENTRY = LINKER.downcallHandle(
            LOOKUP.find("loom_cal_put_entry").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.JAVA_LONG));

    static final MethodHandle LOOM_CAL_GET_ENTRY = LINKER.downcallHandle(
            LOOKUP.find("loom_cal_get_entry").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_CAL_DELETE_ENTRY = LINKER.downcallHandle(
            LOOKUP.find("loom_cal_delete_entry").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS));

    static final MethodHandle LOOM_CAL_LIST_ENTRIES = LINKER.downcallHandle(
            LOOKUP.find("loom_cal_list_entries").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS));

    static final MethodHandle LOOM_CAL_RANGE = LINKER.downcallHandle(
            LOOKUP.find("loom_cal_range").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_CAL_SEARCH = LINKER.downcallHandle(
            LOOKUP.find("loom_cal_search").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_CAL_ENTRY_ICS = LINKER.downcallHandle(
            LOOKUP.find("loom_cal_entry_ics").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_CAL_PUT_ICS = LINKER.downcallHandle(
            LOOKUP.find("loom_cal_put_ics").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS));

    // --- Contacts facet handles. ---

    static final MethodHandle LOOM_CARD_CREATE_BOOK = LINKER.downcallHandle(
            LOOKUP.find("loom_card_create_book").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_CARD_DELETE_BOOK = LINKER.downcallHandle(
            LOOKUP.find("loom_card_delete_book").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_CARD_LIST_BOOKS = LINKER.downcallHandle(
            LOOKUP.find("loom_card_list_books").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_CARD_PUT_ENTRY = LINKER.downcallHandle(
            LOOKUP.find("loom_card_put_entry").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.JAVA_LONG));

    static final MethodHandle LOOM_CARD_GET_ENTRY = LINKER.downcallHandle(
            LOOKUP.find("loom_card_get_entry").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_CARD_DELETE_ENTRY = LINKER.downcallHandle(
            LOOKUP.find("loom_card_delete_entry").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS));

    static final MethodHandle LOOM_CARD_LIST_ENTRIES = LINKER.downcallHandle(
            LOOKUP.find("loom_card_list_entries").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS));

    static final MethodHandle LOOM_CARD_SEARCH = LINKER.downcallHandle(
            LOOKUP.find("loom_card_search").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_CARD_ENTRY_VCARD = LINKER.downcallHandle(
            LOOKUP.find("loom_card_entry_vcard").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_CARD_PUT_VCARD = LINKER.downcallHandle(
            LOOKUP.find("loom_card_put_vcard").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS));

    // --- Mail facet handles. ---

    static final MethodHandle LOOM_MAIL_CREATE_MAILBOX = LINKER.downcallHandle(
            LOOKUP.find("loom_mail_create_mailbox").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_MAIL_DELETE_MAILBOX = LINKER.downcallHandle(
            LOOKUP.find("loom_mail_delete_mailbox").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_MAIL_LIST_MAILBOXES = LINKER.downcallHandle(
            LOOKUP.find("loom_mail_list_mailboxes").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_MAIL_INGEST_MESSAGE = LINKER.downcallHandle(
            LOOKUP.find("loom_mail_ingest_message").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.JAVA_LONG, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_MAIL_GET_MESSAGE = LINKER.downcallHandle(
            LOOKUP.find("loom_mail_get_message").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_MAIL_GET_BODY = LINKER.downcallHandle(
            LOOKUP.find("loom_mail_to_eml").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_MAIL_DELETE_MESSAGE = LINKER.downcallHandle(
            LOOKUP.find("loom_mail_delete_message").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS));

    static final MethodHandle LOOM_MAIL_LIST_MESSAGES = LINKER.downcallHandle(
            LOOKUP.find("loom_mail_list_messages").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS));

    static final MethodHandle LOOM_MAIL_GET_FLAGS = LINKER.downcallHandle(
            LOOKUP.find("loom_mail_get_flags").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_MAIL_SET_FLAGS = LINKER.downcallHandle(
            LOOKUP.find("loom_mail_set_flags").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.JAVA_LONG));

    static final MethodHandle LOOM_MAIL_SEARCH = LINKER.downcallHandle(
            LOOKUP.find("loom_mail_search").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_SQL_EXEC_ASYNC = LINKER.downcallHandle(
            LOOKUP.find("loom_sql_exec_async").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS));

    static final MethodHandle LOOM_TASK_WAIT = LINKER.downcallHandle(
            LOOKUP.find("loom_task_wait").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS));

    static final MethodHandle LOOM_TASK_FREE = LINKER.downcallHandle(
            LOOKUP.find("loom_task_free").orElseThrow(),
            FunctionDescriptor.ofVoid(ValueLayout.ADDRESS));

    static final MethodHandle LOOM_SQL_COMMIT = LINKER.downcallHandle(
            LOOKUP.find("loom_sql_commit").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_SQL_CLOSE = LINKER.downcallHandle(
            LOOKUP.find("loom_sql_close").orElseThrow(),
            FunctionDescriptor.ofVoid(ValueLayout.ADDRESS));

    // --- Transaction/batch handles (the held-open scope). ---

    static final MethodHandle LOOM_SQL_BATCH_BEGIN = LINKER.downcallHandle(
            LOOKUP.find("loom_sql_batch_begin").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_SQL_BATCH_BEGIN_KEYED = LINKER.downcallHandle(
            LOOKUP.find("loom_sql_batch_begin_keyed").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                    ValueLayout.ADDRESS));

    static final MethodHandle LOOM_SQL_BATCH_BEGIN_WITH_KEK = LINKER.downcallHandle(
            LOOKUP.find("loom_sql_batch_begin_with_kek").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                    ValueLayout.ADDRESS));

    static final MethodHandle LOOM_SQL_BATCH_BEGIN_AUTHENTICATED = LINKER.downcallHandle(
            LOOKUP.find("loom_sql_batch_begin_authenticated").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                    ValueLayout.ADDRESS));

    static final MethodHandle LOOM_SQL_BATCH_BEGIN_KEYED_AUTHENTICATED = LINKER.downcallHandle(
            LOOKUP.find("loom_sql_batch_begin_keyed_authenticated").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                    ValueLayout.ADDRESS));

    static final MethodHandle LOOM_SQL_BATCH_BEGIN_WITH_KEK_AUTHENTICATED = LINKER.downcallHandle(
            LOOKUP.find("loom_sql_batch_begin_with_kek_authenticated").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                    ValueLayout.ADDRESS));

    static final MethodHandle LOOM_SQL_BATCH_EXEC = LINKER.downcallHandle(
            LOOKUP.find("loom_sql_batch_exec").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_SQL_BATCH_COMMIT = LINKER.downcallHandle(
            LOOKUP.find("loom_sql_batch_commit").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_SQL_BATCH_COMMIT_VCS = LINKER.downcallHandle(
            LOOKUP.find("loom_sql_batch_commit_vcs").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_SQL_BATCH_ABORT = LINKER.downcallHandle(
            LOOKUP.find("loom_sql_batch_abort").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_SQL_BATCH_CLOSE = LINKER.downcallHandle(
            LOOKUP.find("loom_sql_batch_close").orElseThrow(),
            FunctionDescriptor.ofVoid(ValueLayout.ADDRESS));

    // --- Result-view handles (the typed `exec` path; one shared decoder behind the C ABI). ---

    static final MethodHandle LOOM_RESULT_OPEN = LINKER.downcallHandle(
            LOOKUP.find("loom_result_open").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                    ValueLayout.ADDRESS));

    static final MethodHandle LOOM_RESULT_CLOSE = LINKER.downcallHandle(
            LOOKUP.find("loom_result_close").orElseThrow(),
            FunctionDescriptor.ofVoid(ValueLayout.ADDRESS));

    // --- Streaming iterator handles. ---

    static final MethodHandle LOOM_SQL_QUERY = LINKER.downcallHandle(
            LOOKUP.find("loom_sql_query").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS));

    static final MethodHandle LOOM_ITER_NEXT = LINKER.downcallHandle(
            LOOKUP.find("loom_iter_next").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_ITER_FREE = LINKER.downcallHandle(
            LOOKUP.find("loom_iter_free").orElseThrow(),
            FunctionDescriptor.ofVoid(ValueLayout.ADDRESS));

    static final MethodHandle LOOM_ROW_OPEN = LINKER.downcallHandle(
            LOOKUP.find("loom_row_open").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                    ValueLayout.ADDRESS));

    static final MethodHandle LOOM_RESULT_LEN = LINKER.downcallHandle(
            LOOKUP.find("loom_result_len").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_LONG, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_RESULT_IS_STATEMENTS = LINKER.downcallHandle(
            LOOKUP.find("loom_result_is_statements").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_RESULT_ITEM_KIND = LINKER.downcallHandle(
            LOOKUP.find("loom_result_item_kind").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG));

    static final MethodHandle LOOM_RESULT_COLUMN_COUNT = LINKER.downcallHandle(
            LOOKUP.find("loom_result_column_count").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_LONG, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG));

    // Shape shared by column_name / column_type / string / row_commit: (view, a, b, &ptr, &len) -> int.
    static final FunctionDescriptor BORROWED_STR = FunctionDescriptor.of(ValueLayout.JAVA_INT,
            ValueLayout.ADDRESS, ValueLayout.JAVA_LONG, ValueLayout.JAVA_LONG, ValueLayout.ADDRESS,
            ValueLayout.ADDRESS);

    static final MethodHandle LOOM_RESULT_COLUMN_NAME = LINKER.downcallHandle(
            LOOKUP.find("loom_result_column_name").orElseThrow(), BORROWED_STR);

    static final MethodHandle LOOM_RESULT_COLUMN_TYPE = LINKER.downcallHandle(
            LOOKUP.find("loom_result_column_type").orElseThrow(), BORROWED_STR);

    static final MethodHandle LOOM_RESULT_ROW_COUNT = LINKER.downcallHandle(
            LOOKUP.find("loom_result_row_count").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_LONG, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG));

    static final MethodHandle LOOM_RESULT_ROW_LEN = LINKER.downcallHandle(
            LOOKUP.find("loom_result_row_len").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_LONG, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                    ValueLayout.JAVA_LONG));

    static final MethodHandle LOOM_RESULT_CELL = LINKER.downcallHandle(
            LOOKUP.find("loom_result_cell").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                    ValueLayout.JAVA_LONG, ValueLayout.JAVA_LONG, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_RESULT_COUNT = LINKER.downcallHandle(
            LOOKUP.find("loom_result_count").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                    ValueLayout.ADDRESS));

    static final MethodHandle LOOM_RESULT_STRING_COUNT = LINKER.downcallHandle(
            LOOKUP.find("loom_result_string_count").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_LONG, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG));

    static final MethodHandle LOOM_RESULT_STRING = LINKER.downcallHandle(
            LOOKUP.find("loom_result_string").orElseThrow(), BORROWED_STR);

    static final MethodHandle LOOM_RESULT_VARIABLE_KIND = LINKER.downcallHandle(
            LOOKUP.find("loom_result_variable_kind").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                    ValueLayout.ADDRESS));

    static final MethodHandle LOOM_RESULT_ROW_COMMIT = LINKER.downcallHandle(
            LOOKUP.find("loom_result_row_commit").orElseThrow(), BORROWED_STR);

    static final MethodHandle LOOM_RESULT_DIFF_COUNT = LINKER.downcallHandle(
            LOOKUP.find("loom_result_diff_count").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_LONG, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG));

    static final MethodHandle LOOM_RESULT_DIFF_CHANGE = LINKER.downcallHandle(
            LOOKUP.find("loom_result_diff_change").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                    ValueLayout.JAVA_LONG, ValueLayout.ADDRESS));

    static final MethodHandle LOOM_RESULT_DIFF_LEN = LINKER.downcallHandle(
            LOOKUP.find("loom_result_diff_len").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_LONG, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                    ValueLayout.JAVA_LONG, ValueLayout.JAVA_INT));

    static final MethodHandle LOOM_RESULT_DIFF_CELL = LINKER.downcallHandle(
            LOOKUP.find("loom_result_diff_cell").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                    ValueLayout.JAVA_LONG, ValueLayout.JAVA_INT, ValueLayout.JAVA_LONG,
                    ValueLayout.ADDRESS));

    static final MethodHandle LOOM_RESULT_MERGE_OUTCOME = LINKER.downcallHandle(
            LOOKUP.find("loom_result_merge_outcome").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                    ValueLayout.ADDRESS));

    static final MethodHandle LOOM_RESULT_MAP_LEN = LINKER.downcallHandle(
            LOOKUP.find("loom_result_map_len").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_LONG, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                    ValueLayout.JAVA_LONG));

    static final MethodHandle LOOM_RESULT_MAP_ENTRY = LINKER.downcallHandle(
            LOOKUP.find("loom_result_map_entry").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                    ValueLayout.JAVA_LONG, ValueLayout.JAVA_LONG, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    // `LoomValue` is a 96-byte C struct; these are its field byte offsets (natural alignment, no
    // padding: two int32 fill the first 8 bytes, then 8-byte scalars, the 16-byte array, a pointer,
    // and the length).
    static final long LV_SIZE = 96;
    static final long LV_TAG = 0;
    static final long LV_SCALE = 4;
    static final long LV_INT = 8;
    static final long LV_INT2 = 16;
    static final long LV_UINT = 24;
    static final long LV_FLOAT = 32;
    static final long LV_FLOAT2 = 40;
    static final long LV_BITS = 48;
    static final long LV_BITS2 = 56;
    static final long LV_BYTES16 = 64;
    static final long LV_DATA = 80;
    static final long LV_DATA_LEN = 88;

    private Loom() {}

    static SymbolLookup loadLibrary() {
        // For local development, set java.library.path or LD_LIBRARY_PATH to the cargo target dir;
        // packaged releases extract the per-platform native library at load.
        String name = System.mapLibraryName("uldren_loom"); // e.g. libuldren_loom.so
        return SymbolLookup.libraryLookup(name, Arena.global());
    }

    /**
     * Open a session over the {@code .loom} at {@code path}. The returned {@link LoomSession} carries
     * the path so facet operations need not repeat it; obtain grouped facet accessors from it
     * ({@code session.kv()...}). For an unencrypted store. (Session/instance API; the legacy static
     * methods remain during the transition.)
     */
    public static LoomSession open(String path) {
        return new LoomSession(path, null);
    }

    /**
     * Open a session over the encrypted {@code .loom} at {@code path}, unlocked with {@code passphrase}
     * (held in the session for its facet calls; never re-derived, never read from an environment
     * variable). See {@link #open(String)}.
     */
    public static LoomSession openEncrypted(String path, String passphrase) {
        return new LoomSession(path, passphrase);
    }

    /**
     * Alias for {@link #openEncrypted(String, String)} - open and unlock a session in one call. With a
     * {@code null} passphrase this is equivalent to {@link #open(String)}.
     */
    public static LoomSession authenticate(String path, String passphrase) {
        return openEncrypted(path, passphrase);
    }

    /** Library version, e.g. "0.0.0". */
    public static String version() {
        try {
            MemorySegment ptr = (MemorySegment) LOOM_VERSION.invokeExact();
            return takeOwnedString(ptr);
        } catch (Throwable t) {
            throw new RuntimeException("loom_version failed", t);
        }
    }

    /**
     * Build capability report (0010 section 5) as canonical CBOR: a {@code CapabilitySet} map with
     * {@code schema_version} and {@code records}. Build-aware: capabilities owned by the linked crates
     * are reported with operational state {@code supported}. Mirrors the C ABI {@code loom_capabilities}.
     */
    public static byte[] capabilities() {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
            MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
            int status = (int) LOOM_CAPABILITIES.invokeExact(outPtr, outLen);
            if (status != 0) {
                throw lastError("loom_capabilities");
            }
            return takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                    outLen.get(ValueLayout.JAVA_LONG, 0));
        } catch (LoomException e) {
            throw e;
        } catch (Throwable t) {
            throw new RuntimeException("loom_capabilities failed", t);
        }
    }

    /** Runtime provider/profile report as canonical CBOR. */
    public static byte[] runtimeProfile() {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
            MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
            int status = (int) LOOM_RUNTIME_PROFILE.invokeExact(outPtr, outLen);
            if (status != 0) {
                throw lastError("loom_runtime_profile");
            }
            return takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                    outLen.get(ValueLayout.JAVA_LONG, 0));
        } catch (LoomException e) {
            throw e;
        } catch (Throwable t) {
            throw new RuntimeException("loom_runtime_profile failed", t);
        }
    }

    public static String studioSurfaceCatalogJson(String workspace, String set) {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) LOOM_STUDIO_SURFACE_CATALOG_JSON.invokeExact(
                    arena.allocateFrom(workspace), arena.allocateFrom(set), out);
            if (status != 0) {
                throw lastError("loom_studio_surface_catalog_json");
            }
            return takeOwnedString(out.get(ValueLayout.ADDRESS, 0));
        } catch (LoomException e) {
            throw e;
        } catch (Throwable t) {
            throw new RuntimeException("loom_studio_surface_catalog_json failed", t);
        }
    }

    public static String studioSurfaceCatalogJson(String workspace) {
        return studioSurfaceCatalogJson(workspace, "all");
    }

    public static byte[] execCbor(String path, byte[] request) {
        return execCbor(path, request, null, null);
    }

    public static byte[] execCbor(String path, byte[] request, String passphrase) {
        byte[] pass = passphrase != null
                ? passphrase.getBytes(java.nio.charset.StandardCharsets.UTF_8)
                : null;
        return execCbor(path, request, pass, null);
    }

    public static byte[] execCbor(String path, byte[] request, byte[] passphrase, byte[] kek) {
        return onHandle(path, passphrase, kek, "loom_exec_cbor", (arena, handle) -> {
            MemorySegment req = arena.allocate(Math.max(request.length, 1));
            MemorySegment.copy(request, 0, req, ValueLayout.JAVA_BYTE, 0, request.length);
            MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
            MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
            int status = (int) LOOM_EXEC_CBOR.invokeExact(
                    handle, req, (long) request.length, outPtr, outLen);
            if (status != 0) {
                throw lastError("loom_exec_cbor");
            }
            return takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                    outLen.get(ValueLayout.JAVA_LONG, 0));
        });
    }

    public static String daemonStatusJson(String path) {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) LOOM_DAEMON_STATUS_JSON.invokeExact(arena.allocateFrom(path), out);
            if (status != 0) {
                throw lastError("loom_daemon_status_json");
            }
            return takeOwnedString(out.get(ValueLayout.ADDRESS, 0));
        } catch (LoomException e) {
            throw e;
        } catch (Throwable t) {
            throw new RuntimeException("loom_daemon_status_json failed", t);
        }
    }

    public static void daemonSessionAttach(String path, String session) {
        daemonTwoString(LOOM_DAEMON_SESSION_ATTACH, "loom_daemon_session_attach", path, session);
    }

    public static void daemonSessionDetach(String path, String session) {
        daemonTwoString(LOOM_DAEMON_SESSION_DETACH, "loom_daemon_session_detach", path, session);
    }

    public static void daemonPinAdd(String path, String pin) {
        daemonTwoString(LOOM_DAEMON_PIN_ADD, "loom_daemon_pin_add", path, pin);
    }

    public static void daemonPinRemove(String path, String pin) {
        daemonTwoString(LOOM_DAEMON_PIN_REMOVE, "loom_daemon_pin_remove", path, pin);
    }

    public static String lockAcquireJson(String path, String key, String principal, String session,
            String mode, int permits, int capacity, long leaseMs) {
        return lockAcquireJson(path, key, principal, session, mode, permits, capacity, leaseMs, 30000);
    }

    public static String lockAcquireJson(String path, String key, String principal, String session,
            String mode, int permits, int capacity, long leaseMs, long waitMs) {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) LOOM_LOCK_ACQUIRE_JSON.invokeExact(
                    arena.allocateFrom(path), arena.allocateFrom(key), arena.allocateFrom(principal),
                    arena.allocateFrom(session), arena.allocateFrom(mode), permits, capacity, leaseMs,
                    waitMs, out);
            if (status != 0) {
                throw lastError("loom_lock_acquire_json");
            }
            return takeOwnedString(out.get(ValueLayout.ADDRESS, 0));
        } catch (LoomException e) {
            throw e;
        } catch (Throwable t) {
            throw new RuntimeException("loom_lock_acquire_json failed", t);
        }
    }

    public record FenceToken(int authority, int epoch, long sequence) {
        public long low() {
            return sequence;
        }

        public long high() {
            return ((long) authority << 32) | Integer.toUnsignedLong(epoch);
        }
    }

    public record LockToken(String key, String principal, String session, String mode, int permits,
            int capacity, FenceToken fence, long leaseDeadlineMs) {
    }

    public static LockToken parseLockToken(String json) {
        return new LockToken(lockJsonString(json, "key"), lockJsonString(json, "principal"),
                lockJsonString(json, "session"), lockJsonString(json, "mode"),
                Math.toIntExact(lockJsonLong(json, "permits")),
                Math.toIntExact(lockJsonLong(json, "capacity")),
                new FenceToken(Math.toIntExact(lockJsonLong(json, "authority")),
                        Math.toIntExact(lockJsonLong(json, "epoch")),
                        lockJsonLong(json, "sequence")),
                lockJsonLong(json, "lease_deadline_ms"));
    }

    public static LockToken lockAcquire(String path, String key, String principal, String session) {
        return lockAcquire(path, key, principal, session, "exclusive", 1, 1, 60000, 30000);
    }

    public static LockToken lockAcquire(String path, String key, String principal, String session,
            String mode, int permits, int capacity, long leaseMs, long waitMs) {
        return parseLockToken(lockAcquireJson(path, key, principal, session, mode, permits, capacity,
                leaseMs, waitMs));
    }

    public static LockToken lockTryAcquire(String path, String key, String principal, String session,
            String mode, int permits, int capacity, long leaseMs) {
        return lockAcquire(path, key, principal, session, mode, permits, capacity, leaseMs, 0);
    }

    public static String lockRefreshJson(String path, String key, String principal, String session,
            String mode, int permits, int capacity, long fenceLow, long fenceHigh, long leaseMs) {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) LOOM_LOCK_REFRESH_JSON.invokeExact(
                    arena.allocateFrom(path), arena.allocateFrom(key), arena.allocateFrom(principal),
                    arena.allocateFrom(session), arena.allocateFrom(mode), permits, capacity,
                    fenceLow, fenceHigh, leaseMs, out);
            if (status != 0) {
                throw lastError("loom_lock_refresh_json");
            }
            return takeOwnedString(out.get(ValueLayout.ADDRESS, 0));
        } catch (LoomException e) {
            throw e;
        } catch (Throwable t) {
            throw new RuntimeException("loom_lock_refresh_json failed", t);
        }
    }

    public static LockToken lockRefresh(String path, LockToken token, long leaseMs) {
        return parseLockToken(lockRefreshJson(path, token.key(), token.principal(), token.session(),
                token.mode(), token.permits(), token.capacity(), token.fence().low(),
                token.fence().high(), leaseMs));
    }

    public static void lockRelease(String path, String key, String principal, String session,
            String mode, int permits, int capacity, long fenceLow, long fenceHigh) {
        try (Arena arena = Arena.ofConfined()) {
            int status = (int) LOOM_LOCK_RELEASE.invokeExact(
                    arena.allocateFrom(path), arena.allocateFrom(key), arena.allocateFrom(principal),
                    arena.allocateFrom(session), arena.allocateFrom(mode), permits, capacity,
                    fenceLow, fenceHigh);
            if (status != 0) {
                throw lastError("loom_lock_release");
            }
        } catch (LoomException e) {
            throw e;
        } catch (Throwable t) {
            throw new RuntimeException("loom_lock_release failed", t);
        }
    }

    public static void lockRelease(String path, LockToken token) {
        lockRelease(path, token.key(), token.principal(), token.session(), token.mode(),
                token.permits(), token.capacity(), token.fence().low(), token.fence().high());
    }

    public static LockGuard scopedLock(String path, String key, String principal, String session) {
        return scopedLock(path, key, principal, session, "exclusive", 1, 1, 60000, 30000);
    }

    public static LockGuard scopedLock(String path, String key, String principal, String session,
            String mode, int permits, int capacity, long leaseMs, long waitMs) {
        return new LockGuard(path,
                lockAcquire(path, key, principal, session, mode, permits, capacity, leaseMs, waitMs));
    }

    public static final class LockGuard implements AutoCloseable {
        private final String path;
        private LockToken token;
        private boolean closed;

        private LockGuard(String path, LockToken token) {
            this.path = path;
            this.token = token;
            this.closed = false;
        }

        public LockToken token() {
            return token;
        }

        public LockToken refresh(long leaseMs) {
            token = Loom.lockRefresh(path, token, leaseMs);
            return token;
        }

        public void release() {
            if (!closed) {
                Loom.lockRelease(path, token);
                closed = true;
            }
        }

        @Override
        public void close() {
            release();
        }
    }

    private static String lockJsonString(String json, String name) {
        String needle = "\"" + name + "\":\"";
        int pos = json.indexOf(needle);
        if (pos < 0) {
            throw new IllegalArgumentException("missing lock token string field " + name);
        }
        pos += needle.length();
        StringBuilder out = new StringBuilder();
        boolean escape = false;
        for (; pos < json.length(); pos++) {
            char c = json.charAt(pos);
            if (escape) {
                out.append(c);
                escape = false;
            } else if (c == '\\') {
                escape = true;
            } else if (c == '"') {
                return out.toString();
            } else {
                out.append(c);
            }
        }
        throw new IllegalArgumentException("unterminated lock token string field " + name);
    }

    private static long lockJsonLong(String json, String name) {
        String needle = "\"" + name + "\":";
        int pos = json.indexOf(needle);
        if (pos < 0) {
            throw new IllegalArgumentException("missing lock token numeric field " + name);
        }
        pos += needle.length();
        long value = 0;
        boolean found = false;
        for (; pos < json.length(); pos++) {
            char c = json.charAt(pos);
            if (c < '0' || c > '9') {
                break;
            }
            found = true;
            value = Math.addExact(Math.multiplyExact(value, 10), c - '0');
        }
        if (!found) {
            throw new IllegalArgumentException("invalid lock token numeric field " + name);
        }
        return value;
    }

    // --- Document / Time-series / Ledger facets. ---

    /** The most recent point of a series: its timestamp and value bytes. */
    public record TsPoint(long ts, byte[] value) {
    }



    /** Blob content address ("algo:hex") of the given bytes. */
    public static String blobDigest(byte[] data) {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment buf = arena.allocate(Math.max(data.length, 1));
            MemorySegment.copy(data, 0, buf, ValueLayout.JAVA_BYTE, 0, data.length);
            MemorySegment ptr = (MemorySegment) LOOM_BLOB_DIGEST.invokeExact(buf, (long) data.length);
            return takeOwnedString(ptr);
        } catch (Throwable t) {
            throw new RuntimeException("loom_blob_digest failed", t);
        }
    }

    /** Read a C string returned by the library, then free it (the library owns returned pointers). */
    static String takeOwnedString(MemorySegment ptr) throws Throwable {
        if (ptr.equals(MemorySegment.NULL)) {
            return null;
        }
        String s = ptr.reinterpret(Long.MAX_VALUE).getString(0);
        LOOM_STRING_FREE.invokeExact(ptr);
        return s;
    }

    static void daemonTwoString(MethodHandle handle, String op, String path, String value) {
        try (Arena arena = Arena.ofConfined()) {
            int status = (int) handle.invokeExact(arena.allocateFrom(path), arena.allocateFrom(value));
            if (status != 0) {
                throw lastError(op);
            }
        } catch (LoomException e) {
            throw e;
        } catch (Throwable t) {
            throw new RuntimeException(op + " failed", t);
        }
    }

    /** A failure from the C ABI: the stable numeric {@code code} plus a message. */
    public static final class LoomException extends RuntimeException {
        private static final long serialVersionUID = 1L;
        public final int code;

        LoomException(int code, String message) {
            super(message);
            this.code = code;
        }
    }

    /** Build the calling thread's most recent error (call only after a non-zero status). */
    static LoomException lastError(String op) {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment codeSeg = arena.allocate(ValueLayout.JAVA_INT);
            MemorySegment msgSeg = arena.allocate(ValueLayout.ADDRESS);
            MemorySegment lenSeg = arena.allocate(ValueLayout.JAVA_LONG);
            LOOM_LAST_ERROR.invokeExact(codeSeg, msgSeg, lenSeg);
            int code = codeSeg.get(ValueLayout.JAVA_INT, 0);
            String msg = takeOwnedString(msgSeg.get(ValueLayout.ADDRESS, 0));
            return new LoomException(code, msg != null ? msg : op + " failed");
        } catch (Throwable t) {
            throw new RuntimeException(op + " failed", t);
        }
    }

    /** A confined native copy of {@code bytes}, or {@code NULL} when null/empty (the C ABI's no-key form). */
    static MemorySegment bytesOrNull(Arena arena, byte[] bytes) {
        return (bytes != null && bytes.length > 0)
                ? arena.allocateFrom(ValueLayout.JAVA_BYTE, bytes)
                : MemorySegment.NULL;
    }

    /**
     * Create a fresh {@code .loom} at {@code path} under an identity {@code profile} ("default"/"blake3"
     * or "fips"/"sha256"), optionally encrypted - the binding counterpart of {@code loom
     * init}. A non-null/non-empty {@code passphrase} encrypts the store; the DEK is wrapped under it
     * with {@code suite}, or the profile default when {@code suite} is null; otherwise
     * the store is unencrypted. Throws on failure (e.g. {@code ALREADY_EXISTS}).
     */
    public static void create(String path, String profile, String suite, String passphrase) {
        byte[] pass =
                (passphrase != null) ? passphrase.getBytes(java.nio.charset.StandardCharsets.UTF_8) : null;
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment suiteSeg = suite != null ? arena.allocateFrom(suite) : MemorySegment.NULL;
            int status = (int) LOOM_CREATE.invokeExact(
                    arena.allocateFrom(path), arena.allocateFrom(profile), suiteSeg,
                    bytesOrNull(arena, pass), (long) (pass != null ? pass.length : 0));
            if (status != 0) {
                throw lastError("loom_create");
            }
        } catch (LoomException e) {
            throw e;
        } catch (Throwable t) {
            throw new RuntimeException("loom_create failed", t);
        }
    }

    /**
     * Create a fresh <b>encrypted</b> {@code .loom} whose DEK is wrapped under a host-supplied 256-bit
     * {@code kek}. {@code profile} selects the content-address algorithm and {@code suite} the object
     * AEAD (profile default when null). {@code kek} must be 32 bytes.
     */
    public static void createWithKek(String path, String profile, String suite, byte[] kek) {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment suiteSeg = suite != null ? arena.allocateFrom(suite) : MemorySegment.NULL;
            int status = (int) LOOM_CREATE_WITH_KEK.invokeExact(
                    arena.allocateFrom(path), arena.allocateFrom(profile), suiteSeg,
                    bytesOrNull(arena, kek), (long) (kek != null ? kek.length : 0));
            if (status != 0) {
                throw lastError("loom_create_with_kek");
            }
        } catch (LoomException e) {
            throw e;
        } catch (Throwable t) {
            throw new RuntimeException("loom_create_with_kek failed", t);
        }
    }

    static MemorySegment openHandle(Arena arena, String path, byte[] passphrase, byte[] kek)
            throws Throwable {
        MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
        int status;
        if (kek != null && kek.length > 0) {
            status = (int) LOOM_OPEN_WITH_KEK.invokeExact(
                    arena.allocateFrom(path), bytesOrNull(arena, kek), (long) kek.length, out);
        } else if (passphrase != null && passphrase.length > 0) {
            status = (int) LOOM_OPEN_KEYED.invokeExact(
                    arena.allocateFrom(path), bytesOrNull(arena, passphrase), (long) passphrase.length, out);
        } else {
            status = (int) LOOM_OPEN.invokeExact(arena.allocateFrom(path), out);
        }
        if (status != 0) {
            throw lastError("loom_open");
        }
        return out.get(ValueLayout.ADDRESS, 0);
    }


    /** Copy an owned (ptr, len) result buffer into a byte[] and free the native buffer. */
    static byte[] takeOwnedBytes(MemorySegment ptr, long len) throws Throwable {
        byte[] out;
        if (ptr.equals(MemorySegment.NULL) || len == 0) {
            out = new byte[0];
        } else {
            out = ptr.reinterpret(len).toArray(ValueLayout.JAVA_BYTE);
        }
        LOOM_BYTES_FREE.invokeExact(ptr, len);
        return out;
    }

    /**
     * A native operation against an open store handle: receives a confined {@link Arena} for argument
     * marshalling and the live {@code LoomSession}-style handle, and returns the op's result.
     */
    @FunctionalInterface
    interface HandleOp<T> {
        T run(Arena arena, MemorySegment handle) throws Throwable;
    }

    /**
     * The single home for the per-op open/try-finally-close/error dance shared by the session accessors
     * ({@code CasOps}, {@code KvOps}, ...): opens {@code path} (optionally encrypted with
     * {@code passphrase} or {@code kek}), runs {@code body}, always closes the handle, and maps native
     * failures to a {@link LoomException} (or wraps unexpected {@link Throwable}s under {@code op}).
     */
    static <T> T onHandle(String path, byte[] passphrase, byte[] kek, String op, HandleOp<T> body) {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment handle = openHandle(arena, path, passphrase, kek);
            try {
                return body.run(arena, handle);
            } finally {
                LOOM_CLOSE.invokeExact(handle);
            }
        } catch (LoomException e) {
            throw e;
        } catch (Throwable t) {
            throw new RuntimeException(op + " failed", t);
        }
    }

    static LoomResult openResult(byte[] bytes) throws Throwable {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment buf = arena.allocate(Math.max(bytes.length, 1));
            MemorySegment.copy(bytes, 0, buf, ValueLayout.JAVA_BYTE, 0, bytes.length);
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) LOOM_RESULT_OPEN.invokeExact(buf, (long) bytes.length, out);
            if (status != 0) {
                throw lastError("loom_result_open");
            }
            return new LoomResult(out.get(ValueLayout.ADDRESS, 0));
        }
    }


    /**
     * A SQL session over a workspace SQL facet in a {@code .loom}. A reopenable handle: each
     * {@code exec} / {@code commit} opens the loom for its duration and releases it, so sessions are
     * cheap and coexist. Close with {@link #close()} (try-with-resources). Throws {@link LoomException}.
     */
    public static final class LoomSql implements AutoCloseable {
        private MemorySegment session;

        private LoomSql(MemorySegment session) {
            this.session = session;
        }

        public static LoomSql authenticated(
                String path, String workspace, String db, String authPrincipal, String authPassphrase) {
            byte[] authPass = authPassphrase != null
                    ? authPassphrase.getBytes(java.nio.charset.StandardCharsets.UTF_8)
                    : null;
            try (Arena arena = Arena.ofConfined()) {
                MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
                int status = (int) LOOM_SQL_OPEN_AUTHENTICATED.invokeExact(
                        arena.allocateFrom(path), arena.allocateFrom(workspace), arena.allocateFrom(db),
                        arena.allocateFrom(authPrincipal), bytesOrNull(arena, authPass),
                        (long) (authPass != null ? authPass.length : 0), out);
                if (status != 0) {
                    throw lastError("loom_sql_open_authenticated");
                }
                return new LoomSql(out.get(ValueLayout.ADDRESS, 0));
            } catch (LoomException e) {
                throw e;
            } catch (Throwable t) {
                throw new RuntimeException("loom_sql_open_authenticated failed", t);
            }
        }

        public static LoomSql openEncryptedAuthenticated(
                String path, String workspace, String db, String passphrase,
                String authPrincipal, String authPassphrase) {
            byte[] pass = passphrase != null
                    ? passphrase.getBytes(java.nio.charset.StandardCharsets.UTF_8)
                    : null;
            byte[] authPass = authPassphrase != null
                    ? authPassphrase.getBytes(java.nio.charset.StandardCharsets.UTF_8)
                    : null;
            try (Arena arena = Arena.ofConfined()) {
                MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
                int status = (int) LOOM_SQL_OPEN_KEYED_AUTHENTICATED.invokeExact(
                        arena.allocateFrom(path), arena.allocateFrom(workspace), arena.allocateFrom(db),
                        bytesOrNull(arena, pass), (long) (pass != null ? pass.length : 0),
                        arena.allocateFrom(authPrincipal), bytesOrNull(arena, authPass),
                        (long) (authPass != null ? authPass.length : 0), out);
                if (status != 0) {
                    throw lastError("loom_sql_open_keyed_authenticated");
                }
                return new LoomSql(out.get(ValueLayout.ADDRESS, 0));
            } catch (LoomException e) {
                throw e;
            } catch (Throwable t) {
                throw new RuntimeException("loom_sql_open_keyed_authenticated failed", t);
            }
        }

        public static LoomSql openWithKekAuthenticated(
                String path, String workspace, String db, byte[] kek,
                String authPrincipal, String authPassphrase) {
            byte[] authPass = authPassphrase != null
                    ? authPassphrase.getBytes(java.nio.charset.StandardCharsets.UTF_8)
                    : null;
            try (Arena arena = Arena.ofConfined()) {
                MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
                int status = (int) LOOM_SQL_OPEN_WITH_KEK_AUTHENTICATED.invokeExact(
                        arena.allocateFrom(path), arena.allocateFrom(workspace), arena.allocateFrom(db),
                        bytesOrNull(arena, kek), (long) (kek != null ? kek.length : 0),
                        arena.allocateFrom(authPrincipal), bytesOrNull(arena, authPass),
                        (long) (authPass != null ? authPass.length : 0), out);
                if (status != 0) {
                    throw lastError("loom_sql_open_with_kek_authenticated");
                }
                return new LoomSql(out.get(ValueLayout.ADDRESS, 0));
            } catch (LoomException e) {
                throw e;
            } catch (Throwable t) {
                throw new RuntimeException("loom_sql_open_with_kek_authenticated failed", t);
            }
        }

        /** Open {@code path} and start a SQL session over {@code workspace} (created if absent). */
        public LoomSql(String path, String workspace, String db) {
            try (Arena arena = Arena.ofConfined()) {
                MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
                int status = (int) LOOM_SQL_OPEN.invokeExact(
                        arena.allocateFrom(path), arena.allocateFrom(workspace),
                        arena.allocateFrom(db), out);
                if (status != 0) {
                    throw lastError("loom_sql_open");
                }
                this.session = out.get(ValueLayout.ADDRESS, 0);
            } catch (LoomException e) {
                throw e;
            } catch (Throwable t) {
                throw new RuntimeException("loom_sql_open failed", t);
            }
        }

        /**
         * Open a session over an <b>encrypted</b> loom, unlocking it with {@code passphrase}. The host
         * acquires the passphrase securely; the FFI never reads an environment variable.
         */
        public LoomSql(String path, String workspace, String db, String passphrase) {
            byte[] pass =
                    (passphrase != null) ? passphrase.getBytes(java.nio.charset.StandardCharsets.UTF_8) : null;
            try (Arena arena = Arena.ofConfined()) {
                MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
                int status = (int) LOOM_SQL_OPEN_KEYED.invokeExact(
                        arena.allocateFrom(path), arena.allocateFrom(workspace), arena.allocateFrom(db),
                        bytesOrNull(arena, pass), (long) (pass != null ? pass.length : 0), out);
                if (status != 0) {
                    throw lastError("loom_sql_open_keyed");
                }
                this.session = out.get(ValueLayout.ADDRESS, 0);
            } catch (LoomException e) {
                throw e;
            } catch (Throwable t) {
                throw new RuntimeException("loom_sql_open_keyed failed", t);
            }
        }

        /**
         * Open a session over an <b>encrypted</b> loom with a host-supplied 256-bit {@code kek} that
         * directly unwraps the DEK. {@code kek} may come from a keychain, Secure Enclave, passkey-PRF,
         * or KMS. {@code kek} must be 32 bytes.
         */
        public LoomSql(String path, String workspace, String db, byte[] kek) {
            try (Arena arena = Arena.ofConfined()) {
                MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
                int status = (int) LOOM_SQL_OPEN_WITH_KEK.invokeExact(
                        arena.allocateFrom(path), arena.allocateFrom(workspace), arena.allocateFrom(db),
                        bytesOrNull(arena, kek), (long) (kek != null ? kek.length : 0), out);
                if (status != 0) {
                    throw lastError("loom_sql_open_with_kek");
                }
                this.session = out.get(ValueLayout.ADDRESS, 0);
            } catch (LoomException e) {
                throw e;
            } catch (Throwable t) {
                throw new RuntimeException("loom_sql_open_with_kek failed", t);
            }
        }

        /**
         * Run SQL and return a <b>typed</b>, indexed {@link LoomResult} (decoded once via the shared
         * result-view; no CBOR is parsed in Java). Read cells back as faithful {@link LoomCell}s, and
         * close the result (try-with-resources). For raw bytes use {@link #execBytes}; for the JSON
         * debug form use {@link #execJson}.
         */
        public LoomResult exec(String sql) {
            try (Arena arena = Arena.ofConfined()) {
                MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                int status =
                        (int) LOOM_SQL_EXEC.invokeExact(session, arena.allocateFrom(sql), outPtr, outLen);
                if (status != 0) {
                    throw lastError("loom_sql_exec");
                }
                MemorySegment bytes = outPtr.get(ValueLayout.ADDRESS, 0);
                long len = outLen.get(ValueLayout.JAVA_LONG, 0);
                MemorySegment viewOut = arena.allocate(ValueLayout.ADDRESS);
                int opened = (int) LOOM_RESULT_OPEN.invokeExact(bytes, len, viewOut);
                LOOM_BYTES_FREE.invokeExact(bytes, len); // result_open decodes into an owned view
                if (opened != 0) {
                    throw lastError("loom_result_open");
                }
                return new LoomResult(viewOut.get(ValueLayout.ADDRESS, 0));
            } catch (LoomException e) {
                throw e;
            } catch (Throwable t) {
                throw new RuntimeException("loom_sql_exec failed", t);
            }
        }

        /**
         * Run a {@code SELECT} and return a lazy {@link LoomRowStream} over its rows (the streaming form):
         * pull rows one at a time with {@link LoomRowStream#next}, never materializing the
         * whole result. Close the stream (try-with-resources) when done.
         */
        public LoomRowStream query(String sql) {
            try (Arena arena = Arena.ofConfined()) {
                MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
                int status = (int) LOOM_SQL_QUERY.invokeExact(session, arena.allocateFrom(sql), out);
                if (status != 0) {
                    throw lastError("loom_sql_query");
                }
                return new LoomRowStream(out.get(ValueLayout.ADDRESS, 0));
            } catch (LoomException e) {
                throw e;
            } catch (Throwable t) {
                throw new RuntimeException("loom_sql_query failed", t);
            }
        }

        /**
         * Run SQL; returns a JSON array of the result payloads (debug/admin form, rendered from the
         * canonical-CBOR result - not the type-faithful API; use {@link #exec}).
         */
        public String execJson(String sql) {
            try (Arena arena = Arena.ofConfined()) {
                MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                int status =
                        (int) LOOM_SQL_EXEC.invokeExact(session, arena.allocateFrom(sql), outPtr, outLen);
                if (status != 0) {
                    throw lastError("loom_sql_exec");
                }
                return renderResult(arena, outPtr.get(ValueLayout.ADDRESS, 0),
                        outLen.get(ValueLayout.JAVA_LONG, 0));
            } catch (LoomException e) {
                throw e;
            } catch (Throwable t) {
                throw new RuntimeException("loom_sql_exec failed", t);
            }
        }

        /** Run SQL; returns the result payloads as canonical CBOR bytes. */
        public byte[] execBytes(String sql) {
            try (Arena arena = Arena.ofConfined()) {
                MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                int status =
                        (int) LOOM_SQL_EXEC.invokeExact(session, arena.allocateFrom(sql), outPtr, outLen);
                if (status != 0) {
                    throw lastError("loom_sql_exec");
                }
                return takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                        outLen.get(ValueLayout.JAVA_LONG, 0));
            } catch (LoomException e) {
                throw e;
            } catch (Throwable t) {
                throw new RuntimeException("loom_sql_exec failed", t);
            }
        }

        /**
         * Run SQL asynchronously (the poll/handle form): the future yields the canonical-CBOR
         * result bytes; the blocking wait runs on the common pool, off the caller's thread. The session
         * must outlive the returned future.
         */
        public java.util.concurrent.CompletableFuture<byte[]> execAsync(String sql) {
            return java.util.concurrent.CompletableFuture.supplyAsync(() -> {
                try (Arena arena = Arena.ofConfined()) {
                    MemorySegment outTask = arena.allocate(ValueLayout.ADDRESS);
                    int status = (int) LOOM_SQL_EXEC_ASYNC.invokeExact(
                            session, arena.allocateFrom(sql), outTask);
                    if (status != 0) {
                        throw lastError("loom_sql_exec_async");
                    }
                    MemorySegment task = outTask.get(ValueLayout.ADDRESS, 0);
                    try {
                        MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                        MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                        int wait = (int) LOOM_TASK_WAIT.invokeExact(task, outPtr, outLen);
                        if (wait != 0) {
                            throw lastError("loom_task_wait");
                        }
                        return takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                                outLen.get(ValueLayout.JAVA_LONG, 0));
                    } finally {
                        LOOM_TASK_FREE.invokeExact(task);
                    }
                } catch (LoomException e) {
                    throw e;
                } catch (Throwable t) {
                    throw new RuntimeException("loom_sql_exec_async failed", t);
                }
            });
        }

        /** Copy a library-returned byte buffer into a Java array, then free it (the library owns it). */
        private static byte[] takeOwnedBytes(MemorySegment ptr, long len) throws Throwable {
            byte[] out;
            if (ptr.equals(MemorySegment.NULL) || len == 0) {
                out = new byte[0];
            } else {
                out = ptr.reinterpret(len).toArray(ValueLayout.JAVA_BYTE);
            }
            LOOM_BYTES_FREE.invokeExact(ptr, len);
            return out;
        }

        /**
         * Render a canonical-CBOR result buffer to JSON (debug form), freeing the buffer with
         * {@code loom_bytes_free}.
         */
        private static String renderResult(Arena arena, MemorySegment ptr, long len) throws Throwable {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) LOOM_RESULT_TO_JSON.invokeExact(ptr, len, out);
            LOOM_BYTES_FREE.invokeExact(ptr, len);
            if (status != 0) {
                throw lastError("loom_result_to_json");
            }
            return takeOwnedString(out.get(ValueLayout.ADDRESS, 0));
        }

        /** Commit the staged database state; returns the new commit's content address. */
        public String commit(String message, String author) {
            try (Arena arena = Arena.ofConfined()) {
                MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
                int status = (int) LOOM_SQL_COMMIT.invokeExact(
                        session, arena.allocateFrom(message), arena.allocateFrom(author), out);
                if (status != 0) {
                    throw lastError("loom_sql_commit");
                }
                return takeOwnedString(out.get(ValueLayout.ADDRESS, 0));
            } catch (LoomException e) {
                throw e;
            } catch (Throwable t) {
                throw new RuntimeException("loom_sql_commit failed", t);
            }
        }

        @Override
        public void close() {
            if (session != null && !session.equals(MemorySegment.NULL)) {
                try {
                    LOOM_SQL_CLOSE.invokeExact(session);
                } catch (Throwable t) {
                    throw new RuntimeException("loom_sql_close failed", t);
                }
                session = MemorySegment.NULL;
            }
        }
    }

    /**
     * A lazy, forward stream of a {@code SELECT}'s rows: RAII over the
     * C {@code LoomIter}, it pulls one row at a time and decodes it via {@code loom_row_open}, so a large
     * result is never materialized. Each {@link #next} yields a one-row {@link LoomResult} whose row
     * (item 0, row 0) carries the cells; close the stream (try-with-resources) when done.
     */
    public static final class LoomRowStream implements AutoCloseable {
        private MemorySegment iter;

        LoomRowStream(MemorySegment iter) {
            this.iter = iter;
        }

        /**
         * The next row as a one-row {@link LoomResult} (read cells with {@code cell(0, 0, col)}), or
         * {@code null} at the end of the stream.
         */
        public LoomResult next() {
            try (Arena arena = Arena.ofConfined()) {
                MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                MemorySegment done = arena.allocate(ValueLayout.JAVA_INT);
                int status = (int) LOOM_ITER_NEXT.invokeExact(iter, outPtr, outLen, done);
                if (status != 0) {
                    throw lastError("loom_iter_next");
                }
                if (done.get(ValueLayout.JAVA_INT, 0) == 1) {
                    return null;
                }
                MemorySegment bytes = outPtr.get(ValueLayout.ADDRESS, 0);
                long len = outLen.get(ValueLayout.JAVA_LONG, 0);
                MemorySegment viewOut = arena.allocate(ValueLayout.ADDRESS);
                int opened = (int) LOOM_ROW_OPEN.invokeExact(bytes, len, viewOut);
                LOOM_BYTES_FREE.invokeExact(bytes, len);
                if (opened != 0) {
                    throw lastError("loom_row_open");
                }
                return new LoomResult(viewOut.get(ValueLayout.ADDRESS, 0));
            } catch (LoomException e) {
                throw e;
            } catch (Throwable t) {
                throw new RuntimeException("loom_iter_next failed", t);
            }
        }

        @Override
        public void close() {
            if (iter != null && !iter.equals(MemorySegment.NULL)) {
                try {
                    LOOM_ITER_FREE.invokeExact(iter);
                } catch (Throwable t) {
                    throw new RuntimeException("loom_iter_free failed", t);
                }
                iter = MemorySegment.NULL;
            }
        }
    }

    /**
     * An explicit transaction/batch scope. Unlike {@link LoomSql}, a batch holds the
     * {@code .loom} open - and its exclusive write lock - for its whole lifetime, so an SQL transaction
     * ({@code BEGIN}/{@code COMMIT}/{@code ROLLBACK}) can span {@code exec} calls; changes become durable
     * through a single atomic save at {@link #commit} (or {@link #commitVcs}). The SQL {@code COMMIT} is
     * distinct from the VCS commit. {@link #close} releases the lock; closing without a commit discards
     * un-persisted changes.
     */
    public static final class LoomSqlBatch implements AutoCloseable {
        private MemorySegment batch;

        private LoomSqlBatch(MemorySegment batch) {
            this.batch = batch;
        }

        public static LoomSqlBatch authenticated(
                String path, String workspace, String db, String authPrincipal, String authPassphrase) {
            byte[] authPass = authPassphrase != null
                    ? authPassphrase.getBytes(java.nio.charset.StandardCharsets.UTF_8)
                    : null;
            try (Arena arena = Arena.ofConfined()) {
                MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
                int status = (int) LOOM_SQL_BATCH_BEGIN_AUTHENTICATED.invokeExact(
                        arena.allocateFrom(path), arena.allocateFrom(workspace), arena.allocateFrom(db),
                        arena.allocateFrom(authPrincipal), bytesOrNull(arena, authPass),
                        (long) (authPass != null ? authPass.length : 0), out);
                if (status != 0) {
                    throw lastError("loom_sql_batch_begin_authenticated");
                }
                return new LoomSqlBatch(out.get(ValueLayout.ADDRESS, 0));
            } catch (LoomException e) {
                throw e;
            } catch (Throwable t) {
                throw new RuntimeException("loom_sql_batch_begin_authenticated failed", t);
            }
        }

        public static LoomSqlBatch openEncryptedAuthenticated(
                String path, String workspace, String db, String passphrase,
                String authPrincipal, String authPassphrase) {
            byte[] pass = passphrase != null
                    ? passphrase.getBytes(java.nio.charset.StandardCharsets.UTF_8)
                    : null;
            byte[] authPass = authPassphrase != null
                    ? authPassphrase.getBytes(java.nio.charset.StandardCharsets.UTF_8)
                    : null;
            try (Arena arena = Arena.ofConfined()) {
                MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
                int status = (int) LOOM_SQL_BATCH_BEGIN_KEYED_AUTHENTICATED.invokeExact(
                        arena.allocateFrom(path), arena.allocateFrom(workspace), arena.allocateFrom(db),
                        bytesOrNull(arena, pass), (long) (pass != null ? pass.length : 0),
                        arena.allocateFrom(authPrincipal), bytesOrNull(arena, authPass),
                        (long) (authPass != null ? authPass.length : 0), out);
                if (status != 0) {
                    throw lastError("loom_sql_batch_begin_keyed_authenticated");
                }
                return new LoomSqlBatch(out.get(ValueLayout.ADDRESS, 0));
            } catch (LoomException e) {
                throw e;
            } catch (Throwable t) {
                throw new RuntimeException("loom_sql_batch_begin_keyed_authenticated failed", t);
            }
        }

        public static LoomSqlBatch openWithKekAuthenticated(
                String path, String workspace, String db, byte[] kek,
                String authPrincipal, String authPassphrase) {
            byte[] authPass = authPassphrase != null
                    ? authPassphrase.getBytes(java.nio.charset.StandardCharsets.UTF_8)
                    : null;
            try (Arena arena = Arena.ofConfined()) {
                MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
                int status = (int) LOOM_SQL_BATCH_BEGIN_WITH_KEK_AUTHENTICATED.invokeExact(
                        arena.allocateFrom(path), arena.allocateFrom(workspace), arena.allocateFrom(db),
                        bytesOrNull(arena, kek), (long) (kek != null ? kek.length : 0),
                        arena.allocateFrom(authPrincipal), bytesOrNull(arena, authPass),
                        (long) (authPass != null ? authPass.length : 0), out);
                if (status != 0) {
                    throw lastError("loom_sql_batch_begin_with_kek_authenticated");
                }
                return new LoomSqlBatch(out.get(ValueLayout.ADDRESS, 0));
            } catch (LoomException e) {
                throw e;
            } catch (Throwable t) {
                throw new RuntimeException("loom_sql_batch_begin_with_kek_authenticated failed", t);
            }
        }

        /** Begin a batch over {@code workspace} (created if absent), database {@code db}, in {@code path}. */
        public LoomSqlBatch(String path, String workspace, String db) {
            try (Arena arena = Arena.ofConfined()) {
                MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
                int status = (int) LOOM_SQL_BATCH_BEGIN.invokeExact(
                        arena.allocateFrom(path), arena.allocateFrom(workspace),
                        arena.allocateFrom(db), out);
                if (status != 0) {
                    throw lastError("loom_sql_batch_begin");
                }
                this.batch = out.get(ValueLayout.ADDRESS, 0);
            } catch (LoomException e) {
                throw e;
            } catch (Throwable t) {
                throw new RuntimeException("loom_sql_batch_begin failed", t);
            }
        }

        /**
         * Begin a batch over an <b>encrypted</b> loom, unlocking it with {@code passphrase} for the
         * batch's lifetime.
         */
        public LoomSqlBatch(String path, String workspace, String db, String passphrase) {
            byte[] pass =
                    (passphrase != null) ? passphrase.getBytes(java.nio.charset.StandardCharsets.UTF_8) : null;
            try (Arena arena = Arena.ofConfined()) {
                MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
                int status = (int) LOOM_SQL_BATCH_BEGIN_KEYED.invokeExact(
                        arena.allocateFrom(path), arena.allocateFrom(workspace), arena.allocateFrom(db),
                        bytesOrNull(arena, pass), (long) (pass != null ? pass.length : 0), out);
                if (status != 0) {
                    throw lastError("loom_sql_batch_begin_keyed");
                }
                this.batch = out.get(ValueLayout.ADDRESS, 0);
            } catch (LoomException e) {
                throw e;
            } catch (Throwable t) {
                throw new RuntimeException("loom_sql_batch_begin_keyed failed", t);
            }
        }

        /**
         * Begin a batch over an <b>encrypted</b> loom with a host-supplied 256-bit {@code kek}
         * for the batch's lifetime. {@code kek} must be 32 bytes.
         */
        public LoomSqlBatch(String path, String workspace, String db, byte[] kek) {
            try (Arena arena = Arena.ofConfined()) {
                MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
                int status = (int) LOOM_SQL_BATCH_BEGIN_WITH_KEK.invokeExact(
                        arena.allocateFrom(path), arena.allocateFrom(workspace), arena.allocateFrom(db),
                        bytesOrNull(arena, kek), (long) (kek != null ? kek.length : 0), out);
                if (status != 0) {
                    throw lastError("loom_sql_batch_begin_with_kek");
                }
                this.batch = out.get(ValueLayout.ADDRESS, 0);
            } catch (LoomException e) {
                throw e;
            } catch (Throwable t) {
                throw new RuntimeException("loom_sql_batch_begin_with_kek failed", t);
            }
        }

        /**
         * Run SQL in the batch (including {@code BEGIN}/{@code COMMIT}/{@code ROLLBACK}) and return a
         * typed {@link LoomResult}. Changes accumulate until {@link #commit}.
         */
        public LoomResult exec(String sql) {
            try (Arena arena = Arena.ofConfined()) {
                MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                int status = (int) LOOM_SQL_BATCH_EXEC.invokeExact(
                        batch, arena.allocateFrom(sql), outPtr, outLen);
                if (status != 0) {
                    throw lastError("loom_sql_batch_exec");
                }
                MemorySegment bytes = outPtr.get(ValueLayout.ADDRESS, 0);
                long len = outLen.get(ValueLayout.JAVA_LONG, 0);
                MemorySegment viewOut = arena.allocate(ValueLayout.ADDRESS);
                int opened = (int) LOOM_RESULT_OPEN.invokeExact(bytes, len, viewOut);
                LOOM_BYTES_FREE.invokeExact(bytes, len);
                if (opened != 0) {
                    throw lastError("loom_result_open");
                }
                return new LoomResult(viewOut.get(ValueLayout.ADDRESS, 0));
            } catch (LoomException e) {
                throw e;
            } catch (Throwable t) {
                throw new RuntimeException("loom_sql_batch_exec failed", t);
            }
        }

        /** Run SQL in the batch; returns the result payloads as canonical CBOR bytes. */
        public byte[] execBytes(String sql) {
            try (Arena arena = Arena.ofConfined()) {
                MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                int status = (int) LOOM_SQL_BATCH_EXEC.invokeExact(
                        batch, arena.allocateFrom(sql), outPtr, outLen);
                if (status != 0) {
                    throw lastError("loom_sql_batch_exec");
                }
                return LoomSql.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                        outLen.get(ValueLayout.JAVA_LONG, 0));
            } catch (LoomException e) {
                throw e;
            } catch (Throwable t) {
                throw new RuntimeException("loom_sql_batch_exec failed", t);
            }
        }

        /**
         * Make the batch's changes durable with one atomic save (no history entry). Rejected while an SQL
         * transaction is open. The batch stays open.
         */
        public void commit() {
            try {
                int status = (int) LOOM_SQL_BATCH_COMMIT.invokeExact(batch);
                if (status != 0) {
                    throw lastError("loom_sql_batch_commit");
                }
            } catch (LoomException e) {
                throw e;
            } catch (Throwable t) {
                throw new RuntimeException("loom_sql_batch_commit failed", t);
            }
        }

        /**
         * Like {@link #commit}, but also records a VCS commit; returns its content address. Distinct from
         * a SQL {@code COMMIT}. Rejected while an SQL transaction is open.
         */
        public String commitVcs(String message, String author) {
            try (Arena arena = Arena.ofConfined()) {
                MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
                int status = (int) LOOM_SQL_BATCH_COMMIT_VCS.invokeExact(
                        batch, arena.allocateFrom(message), arena.allocateFrom(author), out);
                if (status != 0) {
                    throw lastError("loom_sql_batch_commit_vcs");
                }
                return takeOwnedString(out.get(ValueLayout.ADDRESS, 0));
            } catch (LoomException e) {
                throw e;
            } catch (Throwable t) {
                throw new RuntimeException("loom_sql_batch_commit_vcs failed", t);
            }
        }

        /** Discard un-persisted in-memory changes (and any open SQL transaction); the batch stays open. */
        public void abort() {
            try {
                int status = (int) LOOM_SQL_BATCH_ABORT.invokeExact(batch);
                if (status != 0) {
                    throw lastError("loom_sql_batch_abort");
                }
            } catch (LoomException e) {
                throw e;
            } catch (Throwable t) {
                throw new RuntimeException("loom_sql_batch_abort failed", t);
            }
        }

        @Override
        public void close() {
            if (batch != null && !batch.equals(MemorySegment.NULL)) {
                try {
                    LOOM_SQL_BATCH_CLOSE.invokeExact(batch);
                } catch (Throwable t) {
                    throw new RuntimeException("loom_sql_batch_close failed", t);
                }
                batch = MemorySegment.NULL;
            }
        }
    }

    /**
     * One decoded result cell. Only the accessors the {@link #tag} selects are meaningful
     * ({@code LOOM_VALUE_*}). 128-bit ints, UUIDs, the decimal mantissa, and INET octets arrive in
     * {@link #bytes16} (little-endian); floats carry both {@link #doubleValue} and the raw {@link #bits}.
     */
    public record LoomCell(int tag, int scale, long int64, long int64Secondary, long uint64,
            double doubleValue, double doubleSecondary, long bits, long bitsSecondary,
            byte[] bytes16, byte[] data) {
        public boolean isNull() {
            return tag == 0;
        }

        /** UTF-8 text payload (Text), or empty. */
        public String text() {
            return data == null ? "" : new String(data, java.nio.charset.StandardCharsets.UTF_8);
        }

        /** Raw byte payload (Bytes), or the canonical CBOR of a LIST/MAP cell, or empty. */
        public byte[] bytes() {
            return data == null ? new byte[0] : data;
        }
    }

    /**
     * A decoded, immutable, indexed result (the typed {@code exec} return). Navigate it with the indexed
     * accessors (mirroring the C result-view ABI) and read cells as {@link LoomCell}; close it
     * (try-with-resources). One shared decoder backs every C-ABI binding, so no CBOR is parsed here.
     */
    public static final class LoomResult implements AutoCloseable {
        private MemorySegment view;

        LoomResult(MemorySegment view) {
            this.view = view;
        }

        /** Number of items (SQL statements, or 1 for a reader result). */
        public long len() {
            try {
                return (long) LOOM_RESULT_LEN.invokeExact(view);
            } catch (Throwable t) {
                throw new RuntimeException("loom_result_len failed", t);
            }
        }

        /** True if this result is a list of SQL statements (vs a single reader result). */
        public boolean isStatements() {
            try {
                return (int) LOOM_RESULT_IS_STATEMENTS.invokeExact(view) == 1;
            } catch (Throwable t) {
                throw new RuntimeException("loom_result_is_statements failed", t);
            }
        }

        /** The kind of item {@code item} (a {@code LOOM_RESULT_*} value). */
        public int itemKind(long item) {
            try {
                return (int) LOOM_RESULT_ITEM_KIND.invokeExact(view, item);
            } catch (Throwable t) {
                throw new RuntimeException("loom_result_item_kind failed", t);
            }
        }

        public long columnCount(long item) {
            return longCall(LOOM_RESULT_COLUMN_COUNT, item, "column_count");
        }

        public long rowCount(long item) {
            return longCall(LOOM_RESULT_ROW_COUNT, item, "row_count");
        }

        public long stringCount(long item) {
            return longCall(LOOM_RESULT_STRING_COUNT, item, "string_count");
        }

        public long diffCount(long item) {
            return longCall(LOOM_RESULT_DIFF_COUNT, item, "diff_count");
        }

        public long rowLen(long item, long row) {
            return longCall2(LOOM_RESULT_ROW_LEN, item, row, "row_len");
        }

        public long mapLen(long item, long row) {
            return longCall2(LOOM_RESULT_MAP_LEN, item, row, "map_len");
        }

        public String columnName(long item, long col) {
            return borrowed(LOOM_RESULT_COLUMN_NAME, item, col, "column_name");
        }

        public String columnType(long item, long col) {
            return borrowed(LOOM_RESULT_COLUMN_TYPE, item, col, "column_type");
        }

        public String string(long item, long index) {
            return borrowed(LOOM_RESULT_STRING, item, index, "string");
        }

        public String rowCommit(long item, long row) {
            return borrowed(LOOM_RESULT_ROW_COMMIT, item, row, "row_commit");
        }

        public LoomCell cell(long item, long row, long col) {
            try (Arena arena = Arena.ofConfined()) {
                MemorySegment out = arena.allocate(LV_SIZE, 8);
                int st = (int) LOOM_RESULT_CELL.invokeExact(view, item, row, col, out);
                if (st != 0) {
                    throw lastError("loom_result_cell");
                }
                return readCell(out);
            } catch (LoomException e) {
                throw e;
            } catch (Throwable t) {
                throw new RuntimeException("loom_result_cell failed", t);
            }
        }

        /**
         * The rows of item {@code item} as lists of typed {@link LoomCell}s - the idiomatic
         * {@code for (var row : result.rows(0)) ...} form (over the
         * already-decoded typed result).
         */
        public java.util.List<java.util.List<LoomCell>> rows(long item) {
            long rc = rowCount(item);
            java.util.List<java.util.List<LoomCell>> out = new java.util.ArrayList<>((int) rc);
            for (long r = 0; r < rc; r++) {
                long n = rowLen(item, r);
                java.util.List<LoomCell> row = new java.util.ArrayList<>((int) n);
                for (long c = 0; c < n; c++) {
                    row.add(cell(item, r, c));
                }
                out.add(row);
            }
            return out;
        }

        /** Row count of an Insert/Delete/Update/DropTable item. */
        public long rowsAffected(long item) {
            try (Arena arena = Arena.ofConfined()) {
                MemorySegment out = arena.allocate(ValueLayout.JAVA_LONG);
                int st = (int) LOOM_RESULT_COUNT.invokeExact(view, item, out);
                if (st != 0) {
                    throw lastError("loom_result_count");
                }
                return out.get(ValueLayout.JAVA_LONG, 0);
            } catch (LoomException e) {
                throw e;
            } catch (Throwable t) {
                throw new RuntimeException("loom_result_count failed", t);
            }
        }

        /** ShowVariable variable kind ({@code LOOM_VARIABLE_*}). */
        public int variableKind(long item) {
            return intOut(LOOM_RESULT_VARIABLE_KIND, item, "variable_kind");
        }

        /** Merge outcome ({@code LOOM_MERGE_*}). */
        public int mergeOutcome(long item) {
            return intOut(LOOM_RESULT_MERGE_OUTCOME, item, "merge_outcome");
        }

        /** Diff change kind ({@code LOOM_DIFF_*}). */
        public int diffChange(long item, long entry) {
            try (Arena arena = Arena.ofConfined()) {
                MemorySegment out = arena.allocate(ValueLayout.JAVA_INT);
                int st = (int) LOOM_RESULT_DIFF_CHANGE.invokeExact(view, item, entry, out);
                if (st != 0) {
                    throw lastError("loom_result_diff_change");
                }
                return out.get(ValueLayout.JAVA_INT, 0);
            } catch (LoomException e) {
                throw e;
            } catch (Throwable t) {
                throw new RuntimeException("loom_result_diff_change failed", t);
            }
        }

        public long diffLen(long item, long entry, int side) {
            try {
                return (long) LOOM_RESULT_DIFF_LEN.invokeExact(view, item, entry, side);
            } catch (Throwable t) {
                throw new RuntimeException("loom_result_diff_len failed", t);
            }
        }

        public LoomCell diffCell(long item, long entry, int side, long col) {
            try (Arena arena = Arena.ofConfined()) {
                MemorySegment out = arena.allocate(LV_SIZE, 8);
                int st = (int) LOOM_RESULT_DIFF_CELL.invokeExact(view, item, entry, side, col, out);
                if (st != 0) {
                    throw lastError("loom_result_diff_cell");
                }
                return readCell(out);
            } catch (LoomException e) {
                throw e;
            } catch (Throwable t) {
                throw new RuntimeException("loom_result_diff_cell failed", t);
            }
        }

        public java.util.Map.Entry<String, LoomCell> mapEntry(long item, long row, long idx) {
            try (Arena arena = Arena.ofConfined()) {
                MemorySegment keyPtr = arena.allocate(ValueLayout.ADDRESS);
                MemorySegment keyLen = arena.allocate(ValueLayout.JAVA_LONG);
                MemorySegment out = arena.allocate(LV_SIZE, 8);
                int st = (int) LOOM_RESULT_MAP_ENTRY.invokeExact(view, item, row, idx, keyPtr, keyLen, out);
                if (st != 0) {
                    throw lastError("loom_result_map_entry");
                }
                String key = readBorrowed(keyPtr.get(ValueLayout.ADDRESS, 0),
                        keyLen.get(ValueLayout.JAVA_LONG, 0));
                return java.util.Map.entry(key, readCell(out));
            } catch (LoomException e) {
                throw e;
            } catch (Throwable t) {
                throw new RuntimeException("loom_result_map_entry failed", t);
            }
        }

        @Override
        public void close() {
            if (view != null && !view.equals(MemorySegment.NULL)) {
                try {
                    LOOM_RESULT_CLOSE.invokeExact(view);
                } catch (Throwable t) {
                    throw new RuntimeException("loom_result_close failed", t);
                }
                view = MemorySegment.NULL;
            }
        }

        private long longCall(MethodHandle fn, long item, String op) {
            try {
                return (long) fn.invokeExact(view, item);
            } catch (Throwable t) {
                throw new RuntimeException("loom_result_" + op + " failed", t);
            }
        }

        private long longCall2(MethodHandle fn, long a, long b, String op) {
            try {
                return (long) fn.invokeExact(view, a, b);
            } catch (Throwable t) {
                throw new RuntimeException("loom_result_" + op + " failed", t);
            }
        }

        private int intOut(MethodHandle fn, long item, String op) {
            try (Arena arena = Arena.ofConfined()) {
                MemorySegment out = arena.allocate(ValueLayout.JAVA_INT);
                int st = (int) fn.invokeExact(view, item, out);
                if (st != 0) {
                    throw lastError("loom_result_" + op);
                }
                return out.get(ValueLayout.JAVA_INT, 0);
            } catch (LoomException e) {
                throw e;
            } catch (Throwable t) {
                throw new RuntimeException("loom_result_" + op + " failed", t);
            }
        }

        private String borrowed(MethodHandle fn, long a, long b, String op) {
            try (Arena arena = Arena.ofConfined()) {
                MemorySegment ptr = arena.allocate(ValueLayout.ADDRESS);
                MemorySegment len = arena.allocate(ValueLayout.JAVA_LONG);
                int st = (int) fn.invokeExact(view, a, b, ptr, len);
                if (st != 0) {
                    throw lastError("loom_result_" + op);
                }
                return readBorrowed(ptr.get(ValueLayout.ADDRESS, 0), len.get(ValueLayout.JAVA_LONG, 0));
            } catch (LoomException e) {
                throw e;
            } catch (Throwable t) {
                throw new RuntimeException("loom_result_" + op + " failed", t);
            }
        }

        private static String readBorrowed(MemorySegment ptr, long len) {
            if (ptr.address() == 0L || len == 0) {
                return "";
            }
            byte[] b = ptr.reinterpret(len).toArray(ValueLayout.JAVA_BYTE);
            return new String(b, java.nio.charset.StandardCharsets.UTF_8);
        }

        private static LoomCell readCell(MemorySegment v) {
            int tag = v.get(ValueLayout.JAVA_INT, LV_TAG);
            int scale = v.get(ValueLayout.JAVA_INT, LV_SCALE);
            long i64 = v.get(ValueLayout.JAVA_LONG, LV_INT);
            long i64b = v.get(ValueLayout.JAVA_LONG, LV_INT2);
            long u64 = v.get(ValueLayout.JAVA_LONG, LV_UINT);
            double d = v.get(ValueLayout.JAVA_DOUBLE, LV_FLOAT);
            double d2 = v.get(ValueLayout.JAVA_DOUBLE, LV_FLOAT2);
            long bits = v.get(ValueLayout.JAVA_LONG, LV_BITS);
            long bits2 = v.get(ValueLayout.JAVA_LONG, LV_BITS2);
            byte[] b16 = new byte[16];
            MemorySegment.copy(v, ValueLayout.JAVA_BYTE, LV_BYTES16, b16, 0, 16);
            MemorySegment dataPtr = v.get(ValueLayout.ADDRESS, LV_DATA);
            long dataLen = v.get(ValueLayout.JAVA_LONG, LV_DATA_LEN);
            byte[] data = (dataPtr.address() == 0L || dataLen == 0)
                    ? null
                    : dataPtr.reinterpret(dataLen).toArray(ValueLayout.JAVA_BYTE);
            return new LoomCell(tag, scale, i64, i64b, u64, d, d2, bits, bits2, b16, data);
        }
    }
}
