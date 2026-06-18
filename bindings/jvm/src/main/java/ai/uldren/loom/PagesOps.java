package ai.uldren.loom;

import java.lang.foreign.FunctionDescriptor;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.ValueLayout;
import java.lang.invoke.MethodHandle;

public final class PagesOps {
    private static final MethodHandle SPACES_CREATE_JSON = down("loom_spaces_create_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));
    private static final MethodHandle SPACES_LIST_JSON = down("loom_spaces_list_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));
    private static final MethodHandle SPACES_GET_JSON = down("loom_spaces_get_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));
    private static final MethodHandle PAGES_CREATE_JSON = down("loom_pages_create_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS));
    private static final MethodHandle PAGES_UPDATE_JSON = down("loom_pages_update_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));
    private static final MethodHandle PAGES_PUBLISH_JSON = down("loom_pages_publish_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));
    private static final MethodHandle PAGES_GET_JSON = down("loom_pages_get_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));
    private static final MethodHandle PAGES_LIST_JSON = down("loom_pages_list_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));
    private static final MethodHandle PAGES_HISTORY_JSON = down("loom_pages_history_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));
    private static final MethodHandle STRUCTURES_CREATE_JSON = down("loom_structures_create_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS));
    private static final MethodHandle STRUCTURES_ADD_NODE_JSON = down("loom_structures_add_node_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));
    private static final MethodHandle STRUCTURES_UPDATE_NODE_JSON = down("loom_structures_update_node_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));
    private static final MethodHandle STRUCTURES_BIND_JSON = down("loom_structures_bind_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));
    private static final MethodHandle STRUCTURES_MOVE_NODE_JSON = down("loom_structures_move_node_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));
    private static final MethodHandle STRUCTURES_LINK_NODE_JSON = down("loom_structures_link_node_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));
    private static final MethodHandle STRUCTURES_DECOMPOSE_TO_TICKETS_JSON =
            down("loom_structures_decompose_to_tickets_json",
                    FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS,
                            ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                            ValueLayout.ADDRESS, ValueLayout.ADDRESS));
    private static final MethodHandle STRUCTURES_GET_JSON = down("loom_structures_get_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));
    private static final MethodHandle STRUCTURES_LIST_JSON = down("loom_structures_list_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));

    private final LoomSession session;

    PagesOps(LoomSession session) {
        this.session = session;
    }

    private static MethodHandle down(String symbol, FunctionDescriptor descriptor) {
        return Loom.LINKER.downcallHandle(Loom.LOOKUP.find(symbol).orElseThrow(), descriptor);
    }

    public String spacesCreateJson(String workspace, String pageWorkspaceId, String spaceId,
            String title, String expectedRoot) {
        return session.onHandle("loom_spaces_create_json", (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) SPACES_CREATE_JSON.invokeExact(handle, arena.allocateFrom(workspace),
                    arena.allocateFrom(pageWorkspaceId), arena.allocateFrom(spaceId),
                    arena.allocateFrom(title), nullable(arena, expectedRoot), out);
            return takeString("loom_spaces_create_json", status, out);
        });
    }

    public String spacesListJson(String workspace, String pageWorkspaceId) {
        return string2("loom_spaces_list_json", SPACES_LIST_JSON, workspace, pageWorkspaceId);
    }

    public String spacesGetJson(String workspace, String pageWorkspaceId, String spaceId) {
        return string3("loom_spaces_get_json", SPACES_GET_JSON, workspace, pageWorkspaceId, spaceId);
    }

    public String pagesCreateJson(String workspace, String pageWorkspaceId, String pageId,
            String spaceId, String parentPageId, String title, String expectedRoot) {
        return session.onHandle("loom_pages_create_json", (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) PAGES_CREATE_JSON.invokeExact(handle, arena.allocateFrom(workspace),
                    arena.allocateFrom(pageWorkspaceId), arena.allocateFrom(pageId),
                    arena.allocateFrom(spaceId), nullable(arena, parentPageId),
                    arena.allocateFrom(title), nullable(arena, expectedRoot), out);
            return takeString("loom_pages_create_json", status, out);
        });
    }

    public String pagesUpdateJson(String workspace, String pageWorkspaceId, String pageId,
            String bodyText, String expectedRoot) {
        return session.onHandle("loom_pages_update_json", (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) PAGES_UPDATE_JSON.invokeExact(handle, arena.allocateFrom(workspace),
                    arena.allocateFrom(pageWorkspaceId), arena.allocateFrom(pageId),
                    arena.allocateFrom(bodyText), nullable(arena, expectedRoot), out);
            return takeString("loom_pages_update_json", status, out);
        });
    }

    public String pagesPublishJson(String workspace, String pageWorkspaceId, String pageId,
            String expectedRoot) {
        return session.onHandle("loom_pages_publish_json", (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) PAGES_PUBLISH_JSON.invokeExact(handle, arena.allocateFrom(workspace),
                    arena.allocateFrom(pageWorkspaceId), arena.allocateFrom(pageId),
                    nullable(arena, expectedRoot), out);
            return takeString("loom_pages_publish_json", status, out);
        });
    }

    public String pagesGetJson(String workspace, String pageWorkspaceId, String pageId) {
        return string3("loom_pages_get_json", PAGES_GET_JSON, workspace, pageWorkspaceId, pageId);
    }

    public String pagesListJson(String workspace, String pageWorkspaceId) {
        return string2("loom_pages_list_json", PAGES_LIST_JSON, workspace, pageWorkspaceId);
    }

    public String pagesHistoryJson(String workspace, String pageWorkspaceId, String pageId) {
        return string3("loom_pages_history_json", PAGES_HISTORY_JSON, workspace, pageWorkspaceId, pageId);
    }

    public String structuresCreateJson(String workspace, String pageWorkspaceId,
            String structureId, String spaceId, String kind, String title, String expectedRoot) {
        return session.onHandle("loom_structures_create_json", (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) STRUCTURES_CREATE_JSON.invokeExact(handle,
                    arena.allocateFrom(workspace), arena.allocateFrom(pageWorkspaceId),
                    arena.allocateFrom(structureId), arena.allocateFrom(spaceId),
                    arena.allocateFrom(kind), arena.allocateFrom(title),
                    nullable(arena, expectedRoot), out);
            return takeString("loom_structures_create_json", status, out);
        });
    }

    public String structuresAddNodeJson(String workspace, String pageWorkspaceId,
            String structureId, String nodeId, String kind, String label, String bodyDigest,
            String entityRef, String expectedRoot) {
        return structureNode("loom_structures_add_node_json", STRUCTURES_ADD_NODE_JSON, workspace,
                pageWorkspaceId, structureId, nodeId, kind, label, bodyDigest, entityRef,
                expectedRoot);
    }

    public String structuresUpdateNodeJson(String workspace, String pageWorkspaceId,
            String structureId, String nodeId, String kind, String label, String bodyDigest,
            String entityRef, String expectedRoot) {
        return structureNode("loom_structures_update_node_json", STRUCTURES_UPDATE_NODE_JSON,
                workspace, pageWorkspaceId, structureId, nodeId, kind, label, bodyDigest,
                entityRef, expectedRoot);
    }

    public String structuresBindJson(String workspace, String pageWorkspaceId,
            String structureId, String nodeId, String entityRef, String expectedRoot) {
        return session.onHandle("loom_structures_bind_json", (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) STRUCTURES_BIND_JSON.invokeExact(handle,
                    arena.allocateFrom(workspace), arena.allocateFrom(pageWorkspaceId),
                    arena.allocateFrom(structureId), arena.allocateFrom(nodeId),
                    nullable(arena, entityRef), nullable(arena, expectedRoot), out);
            return takeString("loom_structures_bind_json", status, out);
        });
    }

    public String structuresMoveNodeJson(String workspace, String pageWorkspaceId,
            String structureId, String nodeId, String parentNodeId, String label,
            String expectedRoot) {
        return session.onHandle("loom_structures_move_node_json", (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) STRUCTURES_MOVE_NODE_JSON.invokeExact(handle,
                    arena.allocateFrom(workspace), arena.allocateFrom(pageWorkspaceId),
                    arena.allocateFrom(structureId), arena.allocateFrom(nodeId),
                    nullable(arena, parentNodeId), nullable(arena, label),
                    nullable(arena, expectedRoot), out);
            return takeString("loom_structures_move_node_json", status, out);
        });
    }

    public String structuresLinkNodeJson(String workspace, String pageWorkspaceId,
            String structureId, String edgeId, String srcNodeId, String dstNodeId, String label,
            String targetRef, String expectedRoot) {
        return session.onHandle("loom_structures_link_node_json", (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) STRUCTURES_LINK_NODE_JSON.invokeExact(handle,
                    arena.allocateFrom(workspace), arena.allocateFrom(pageWorkspaceId),
                    arena.allocateFrom(structureId), arena.allocateFrom(edgeId),
                    arena.allocateFrom(srcNodeId), arena.allocateFrom(dstNodeId),
                    arena.allocateFrom(label), nullable(arena, targetRef),
                    nullable(arena, expectedRoot), out);
            return takeString("loom_structures_link_node_json", status, out);
        });
    }

    public String structuresDecomposeToTicketsJson(String workspace, String pageWorkspaceId,
            String structureId, String itemsJson) {
        return session.onHandle("loom_structures_decompose_to_tickets_json", (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) STRUCTURES_DECOMPOSE_TO_TICKETS_JSON.invokeExact(handle,
                    arena.allocateFrom(workspace), arena.allocateFrom(pageWorkspaceId),
                    arena.allocateFrom(structureId), arena.allocateFrom(itemsJson), out);
            return takeString("loom_structures_decompose_to_tickets_json", status, out);
        });
    }

    public String structuresGetJson(String workspace, String pageWorkspaceId, String structureId) {
        return string3("loom_structures_get_json", STRUCTURES_GET_JSON, workspace, pageWorkspaceId,
                structureId);
    }

    public String structuresListJson(String workspace, String pageWorkspaceId) {
        return string2("loom_structures_list_json", STRUCTURES_LIST_JSON, workspace, pageWorkspaceId);
    }

    private String structureNode(String symbol, MethodHandle method, String workspace,
            String pageWorkspaceId, String structureId, String nodeId, String kind, String label,
            String bodyDigest, String entityRef, String expectedRoot) {
        return session.onHandle(symbol, (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) method.invokeExact(handle, arena.allocateFrom(workspace),
                    arena.allocateFrom(pageWorkspaceId), arena.allocateFrom(structureId),
                    arena.allocateFrom(nodeId), arena.allocateFrom(kind), arena.allocateFrom(label),
                    nullable(arena, bodyDigest), nullable(arena, entityRef),
                    nullable(arena, expectedRoot), out);
            return takeString(symbol, status, out);
        });
    }

    private String string2(String symbol, MethodHandle handle, String workspace, String profileId) {
        return session.onHandle(symbol, (arena, h) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) handle.invokeExact(h, arena.allocateFrom(workspace),
                    arena.allocateFrom(profileId), out);
            return takeString(symbol, status, out);
        });
    }

    private String string3(String symbol, MethodHandle handle, String workspace,
            String profileId, String value) {
        return session.onHandle(symbol, (arena, h) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) handle.invokeExact(h, arena.allocateFrom(workspace),
                    arena.allocateFrom(profileId), arena.allocateFrom(value), out);
            return takeString(symbol, status, out);
        });
    }

    private static MemorySegment nullable(java.lang.foreign.Arena arena, String value) {
        return value != null && !value.isEmpty() ? arena.allocateFrom(value) : MemorySegment.NULL;
    }

    private static String takeString(String symbol, int status, MemorySegment out) throws Throwable {
        if (status != 0) {
            throw Loom.lastError(symbol);
        }
        return Loom.takeOwnedString(out.get(ValueLayout.ADDRESS, 0));
    }
}
