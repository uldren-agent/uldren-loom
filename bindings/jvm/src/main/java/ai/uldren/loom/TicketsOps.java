package ai.uldren.loom;

import java.lang.foreign.FunctionDescriptor;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.ValueLayout;
import java.lang.invoke.MethodHandle;

public final class TicketsOps {
    private static final MethodHandle PROJECT_CREATE_JSON = down("loom_tickets_project_create_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));
    private static final MethodHandle PROJECT_REKEY_JSON = down("loom_tickets_project_rekey_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));
    private static final MethodHandle PROJECT_SETTINGS_GET_JSON = down("loom_tickets_project_settings_get_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));
    private static final MethodHandle PROJECT_SETTINGS_SET_JSON = down("loom_tickets_project_settings_set_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.JAVA_BOOLEAN, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));
    private static final MethodHandle FIELDS_JSON = down("loom_tickets_fields_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));
    private static final MethodHandle FIELD_PUT_JSON = down("loom_tickets_field_put_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.JAVA_INT, ValueLayout.JAVA_BOOLEAN, ValueLayout.JAVA_BOOLEAN,
                    ValueLayout.JAVA_BOOLEAN, ValueLayout.JAVA_BOOLEAN, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));
    private static final MethodHandle FIELD_RETIRE_JSON = down("loom_tickets_field_retire_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));
    private static final MethodHandle CREATE_JSON = down("loom_tickets_create_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));
    private static final MethodHandle UPDATE_JSON = down("loom_tickets_update_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));
    private static final MethodHandle DELETE_JSON = down("loom_tickets_delete_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS));
    private static final MethodHandle COMMENTS_JSON = down("loom_tickets_comments_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));
    private static final MethodHandle COMMENT_ADD_JSON = down("loom_tickets_comment_add_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS));
    private static final MethodHandle COMMENT_UPDATE_JSON = down("loom_tickets_comment_update_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS));
    private static final MethodHandle COMMENT_DELETE_JSON = down("loom_tickets_comment_delete_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));
    private static final MethodHandle RELATION_SET_JSON = down("loom_tickets_relation_set_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS));
    private static final MethodHandle RELATION_REMOVE_JSON = down("loom_tickets_relation_remove_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));
    private static final MethodHandle GET_JSON = down("loom_tickets_get_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS));
    private static final MethodHandle LIST_JSON = down("loom_tickets_list_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));
    private static final MethodHandle HISTORY_JSON = down("loom_tickets_history_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    private final LoomSession session;

    TicketsOps(LoomSession session) {
        this.session = session;
    }

    private static MethodHandle down(String symbol, FunctionDescriptor descriptor) {
        return Loom.LINKER.downcallHandle(Loom.LOOKUP.find(symbol).orElseThrow(), descriptor);
    }

    public String projectCreateJson(String workspace, String ticketWorkspaceId, String projectId,
            String keyPrefix, String name, String expectedRoot) {
        return session.onHandle("loom_tickets_project_create_json", (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) PROJECT_CREATE_JSON.invokeExact(handle, arena.allocateFrom(workspace),
                    arena.allocateFrom(ticketWorkspaceId), arena.allocateFrom(projectId),
                    arena.allocateFrom(keyPrefix), arena.allocateFrom(name),
                    arena.allocateFrom(expectedRoot), out);
            return takeString("loom_tickets_project_create_json", status, out);
        });
    }

    public String projectRekeyJson(String workspace, String ticketWorkspaceId, String projectId,
            String keyPrefix, String expectedRoot) {
        return session.onHandle("loom_tickets_project_rekey_json", (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) PROJECT_REKEY_JSON.invokeExact(handle, arena.allocateFrom(workspace),
                    arena.allocateFrom(ticketWorkspaceId), arena.allocateFrom(projectId),
                    arena.allocateFrom(keyPrefix), arena.allocateFrom(expectedRoot), out);
            return takeString("loom_tickets_project_rekey_json", status, out);
        });
    }

    public String projectSettingsGetJson(String workspace, String ticketWorkspaceId,
            String projectId) {
        return string3("loom_tickets_project_settings_get_json", PROJECT_SETTINGS_GET_JSON,
                workspace, ticketWorkspaceId, projectId);
    }

    public String projectSettingsSetJson(String workspace, String ticketWorkspaceId,
            String projectId, String defaultProjection, String enableProjectionsJson,
            String disableProjectionsJson, String actorEnforcement, String projectOwnerPrincipal,
            boolean clearProjectOwnerPrincipal, String acceptanceAuthoritiesJson,
            String expectedRoot) {
        return session.onHandle("loom_tickets_project_settings_set_json", (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) PROJECT_SETTINGS_SET_JSON.invokeExact(handle,
                    arena.allocateFrom(workspace), arena.allocateFrom(ticketWorkspaceId),
                    arena.allocateFrom(projectId), nullable(arena, defaultProjection),
                    arena.allocateFrom(enableProjectionsJson),
                    arena.allocateFrom(disableProjectionsJson), nullable(arena, actorEnforcement),
                    nullable(arena, projectOwnerPrincipal), clearProjectOwnerPrincipal,
                    nullable(arena, acceptanceAuthoritiesJson), arena.allocateFrom(expectedRoot),
                    out);
            return takeString("loom_tickets_project_settings_set_json", status, out);
        });
    }

    public String fieldsJson(String workspace, String ticketWorkspaceId, String projectId,
            String projection, String operation) {
        return session.onHandle("loom_tickets_fields_json", (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) FIELDS_JSON.invokeExact(handle, arena.allocateFrom(workspace),
                    arena.allocateFrom(ticketWorkspaceId), arena.allocateFrom(projectId),
                    arena.allocateFrom(projection), arena.allocateFrom(operation), out);
            return takeString("loom_tickets_fields_json", status, out);
        });
    }

    public String fieldPutJson(String workspace, String ticketWorkspaceId, String projectId,
            String fieldId, String key, String name, String description, String fieldType,
            String optionSet, int maxLength, boolean hasMaxLength, boolean required,
            boolean searchable, boolean orderable, String cardinality,
            String applicableTypeIdsJson, String expectedRoot) {
        return session.onHandle("loom_tickets_field_put_json", (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) FIELD_PUT_JSON.invokeExact(handle, arena.allocateFrom(workspace),
                    arena.allocateFrom(ticketWorkspaceId), arena.allocateFrom(projectId),
                    arena.allocateFrom(fieldId), arena.allocateFrom(key), arena.allocateFrom(name),
                    nullable(arena, description), arena.allocateFrom(fieldType),
                    nullable(arena, optionSet), maxLength, hasMaxLength, required, searchable,
                    orderable, arena.allocateFrom(cardinality),
                    arena.allocateFrom(applicableTypeIdsJson), arena.allocateFrom(expectedRoot),
                    out);
            return takeString("loom_tickets_field_put_json", status, out);
        });
    }

    public String fieldRetireJson(String workspace, String ticketWorkspaceId, String projectId,
            String fieldId, String expectedRoot) {
        return session.onHandle("loom_tickets_field_retire_json", (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) FIELD_RETIRE_JSON.invokeExact(handle, arena.allocateFrom(workspace),
                    arena.allocateFrom(ticketWorkspaceId), arena.allocateFrom(projectId),
                    arena.allocateFrom(fieldId), arena.allocateFrom(expectedRoot), out);
            return takeString("loom_tickets_field_retire_json", status, out);
        });
    }

    public String createJson(String workspace, String ticketWorkspaceId, String projectId,
            String ticketType, String externalSource, String externalId, String fieldsJson,
            String policyLabelsJson, String expectedRoot) {
        return session.onHandle("loom_tickets_create_json", (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) CREATE_JSON.invokeExact(handle, arena.allocateFrom(workspace),
                    arena.allocateFrom(ticketWorkspaceId), arena.allocateFrom(projectId),
                    arena.allocateFrom(ticketType), nullable(arena, externalSource),
                    nullable(arena, externalId), arena.allocateFrom(fieldsJson),
                    arena.allocateFrom(policyLabelsJson), arena.allocateFrom(expectedRoot), out);
            return takeString("loom_tickets_create_json", status, out);
        });
    }

    public String updateJson(String workspace, String ticketWorkspaceId, String ticketId,
            String setFieldsJson, String deleteFieldsJson, String action, String targetStatus,
            String observedSourceStatus, String observedWorkflowVersion, String assignee,
            String expectedRoot) {
        return updateJson(workspace, ticketWorkspaceId, ticketId, setFieldsJson, deleteFieldsJson,
                action, targetStatus, observedSourceStatus, observedWorkflowVersion, assignee, null,
                null, null, expectedRoot, null, null, null);
    }

    public String updateJson(String workspace, String ticketWorkspaceId, String ticketId,
            String setFieldsJson, String deleteFieldsJson, String action, String targetStatus,
            String observedSourceStatus, String observedWorkflowVersion, String assignee,
            String commentId, String commentType, String commentBody, String expectedRoot) {
        return updateJson(workspace, ticketWorkspaceId, ticketId, setFieldsJson, deleteFieldsJson,
                action, targetStatus, observedSourceStatus, observedWorkflowVersion, assignee,
                commentId, commentType, commentBody, expectedRoot, null, null, null);
    }

    public String updateJson(String workspace, String ticketWorkspaceId, String ticketId,
            String setFieldsJson, String deleteFieldsJson, String action, String targetStatus,
            String observedSourceStatus, String observedWorkflowVersion, String assignee,
            String commentId, String commentType, String commentBody, String expectedRoot,
            String commentsJson, String relationSetsJson, String relationRemovesJson) {
        return session.onHandle("loom_tickets_update_json", (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) UPDATE_JSON.invokeExact(handle, arena.allocateFrom(workspace),
                    arena.allocateFrom(ticketWorkspaceId), arena.allocateFrom(ticketId),
                    arena.allocateFrom(setFieldsJson), arena.allocateFrom(deleteFieldsJson),
                    nullable(arena, action), nullable(arena, targetStatus),
                    nullable(arena, observedSourceStatus), nullable(arena, observedWorkflowVersion),
                    nullable(arena, assignee), nullable(arena, commentId),
                    nullable(arena, commentType), nullable(arena, commentBody),
                    arena.allocateFrom(expectedRoot), nullable(arena, commentsJson),
                    nullable(arena, relationSetsJson), nullable(arena, relationRemovesJson), out);
            return takeString("loom_tickets_update_json", status, out);
        });
    }

    public String deleteJson(String workspace, String ticketWorkspaceId, String ticketId,
            String expectedRoot) {
        return session.onHandle("loom_tickets_delete_json", (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) DELETE_JSON.invokeExact(handle, arena.allocateFrom(workspace),
                    arena.allocateFrom(ticketWorkspaceId), arena.allocateFrom(ticketId),
                    arena.allocateFrom(expectedRoot), out);
            return takeString("loom_tickets_delete_json", status, out);
        });
    }

    public String commentsJson(String workspace, String ticketWorkspaceId, String ticketId) {
        return session.onHandle("loom_tickets_comments_json", (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) COMMENTS_JSON.invokeExact(handle, arena.allocateFrom(workspace),
                    arena.allocateFrom(ticketWorkspaceId), arena.allocateFrom(ticketId), out);
            return takeString("loom_tickets_comments_json", status, out);
        });
    }

    public String commentAddJson(String workspace, String ticketWorkspaceId, String ticketId,
            String commentId, String commentType, String body, String expectedRoot) {
        return session.onHandle("loom_tickets_comment_add_json", (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) COMMENT_ADD_JSON.invokeExact(handle, arena.allocateFrom(workspace),
                    arena.allocateFrom(ticketWorkspaceId), arena.allocateFrom(ticketId),
                    nullable(arena, commentId), nullable(arena, commentType),
                    arena.allocateFrom(body), arena.allocateFrom(expectedRoot), out);
            return takeString("loom_tickets_comment_add_json", status, out);
        });
    }

    public String commentUpdateJson(String workspace, String ticketWorkspaceId, String ticketId,
            String commentId, String commentType, String body, String expectedRoot) {
        return session.onHandle("loom_tickets_comment_update_json", (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) COMMENT_UPDATE_JSON.invokeExact(handle, arena.allocateFrom(workspace),
                    arena.allocateFrom(ticketWorkspaceId), arena.allocateFrom(ticketId),
                    arena.allocateFrom(commentId), nullable(arena, commentType),
                    nullable(arena, body), arena.allocateFrom(expectedRoot), out);
            return takeString("loom_tickets_comment_update_json", status, out);
        });
    }

    public String commentDeleteJson(String workspace, String ticketWorkspaceId, String ticketId,
            String commentId, String expectedRoot) {
        return session.onHandle("loom_tickets_comment_delete_json", (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) COMMENT_DELETE_JSON.invokeExact(handle, arena.allocateFrom(workspace),
                    arena.allocateFrom(ticketWorkspaceId), arena.allocateFrom(ticketId),
                    arena.allocateFrom(commentId), arena.allocateFrom(expectedRoot), out);
            return takeString("loom_tickets_comment_delete_json", status, out);
        });
    }

    public String relationSetJson(String workspace, String ticketWorkspaceId, String ticketId,
            String relationId, String kind, String targetId, String expectedRoot) {
        return session.onHandle("loom_tickets_relation_set_json", (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) RELATION_SET_JSON.invokeExact(handle, arena.allocateFrom(workspace),
                    arena.allocateFrom(ticketWorkspaceId), arena.allocateFrom(ticketId),
                    arena.allocateFrom(relationId), arena.allocateFrom(kind),
                    arena.allocateFrom(targetId), arena.allocateFrom(expectedRoot), out);
            return takeString("loom_tickets_relation_set_json", status, out);
        });
    }

    public String relationRemoveJson(String workspace, String ticketWorkspaceId,
            String ticketId, String relationId, String expectedRoot) {
        return session.onHandle("loom_tickets_relation_remove_json", (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) RELATION_REMOVE_JSON.invokeExact(handle, arena.allocateFrom(workspace),
                    arena.allocateFrom(ticketWorkspaceId), arena.allocateFrom(ticketId),
                    arena.allocateFrom(relationId), arena.allocateFrom(expectedRoot), out);
            return takeString("loom_tickets_relation_remove_json", status, out);
        });
    }

    public String getJson(String workspace, String ticketWorkspaceId, String ticketId,
            String projection) {
        return session.onHandle("loom_tickets_get_json", (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) GET_JSON.invokeExact(handle, arena.allocateFrom(workspace),
                    arena.allocateFrom(ticketWorkspaceId), arena.allocateFrom(ticketId),
                    arena.allocateFrom(projection), out);
            return takeString("loom_tickets_get_json", status, out);
        });
    }

    public String listJson(String workspace, String ticketWorkspaceId, String projection) {
        return session.onHandle("loom_tickets_list_json", (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) LIST_JSON.invokeExact(handle, arena.allocateFrom(workspace),
                    arena.allocateFrom(ticketWorkspaceId), arena.allocateFrom(projection), out);
            return takeString("loom_tickets_list_json", status, out);
        });
    }

    public String historyJson(String workspace, String ticketWorkspaceId, String ticketId) {
        return string3("loom_tickets_history_json", HISTORY_JSON, workspace, ticketWorkspaceId, ticketId);
    }

    private String string3(String symbol, MethodHandle handle, String workspace,
            String ticketWorkspaceId, String value) {
        return session.onHandle(symbol, (arena, h) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) handle.invokeExact(h, arena.allocateFrom(workspace),
                    arena.allocateFrom(ticketWorkspaceId), arena.allocateFrom(value), out);
            return takeString(symbol, status, out);
        });
    }

    private static MemorySegment nullable(java.lang.foreign.Arena arena, String value) {
        return value != null ? arena.allocateFrom(value) : MemorySegment.NULL;
    }

    private static String takeString(String symbol, int status, MemorySegment out) throws Throwable {
        if (status != 0) {
            throw Loom.lastError(symbol);
        }
        return Loom.takeOwnedString(out.get(ValueLayout.ADDRESS, 0));
    }
}
