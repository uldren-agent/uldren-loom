package ai.uldren.loom;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.ValueLayout;

/**
 * Workspace administration for a {@link LoomSession}: create, list, rename, and delete the workspaces
 * within the loom. Reached via {@link LoomSession#workspaces()}. Owns the FFM downcalls directly
 * via {@link Loom#onHandle}.
 *
 * <p>Licensed under BUSL-1.1 (see the workspace {@code LICENSE}). (c) Uldren Technologies LLC.
 */
public final class WorkspaceOps {
    private final LoomSession session;

    WorkspaceOps(LoomSession session) {
        this.session = session;
    }

    /** Create a workspace named {@code name} with the given {@code facet}; returns its new UUID. */
    public String create(String name, String facet) {
        return session.onHandle("loom_workspace_create",
                (arena, handle) -> {
                    MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment nameSeg = name != null ? arena.allocateFrom(name) : MemorySegment.NULL;
                    MemorySegment facetSeg = facet != null ? arena.allocateFrom(facet) : MemorySegment.NULL;
                    int status = (int) Loom.LOOM_WORKSPACE_CREATE.invokeExact(handle, nameSeg, facetSeg,
                            out);
                    if (status != 0) {
                        throw Loom.lastError("loom_workspace_create");
                    }
                    return Loom.takeOwnedString(out.get(ValueLayout.ADDRESS, 0));
                });
    }

    /** The workspaces in this loom as a debug JSON array. */
    public String listJson() {
        return session.onHandle("loom_workspace_list_json",
                (arena, handle) -> {
                    MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
                    int status = (int) Loom.LOOM_WORKSPACE_LIST_JSON.invokeExact(handle, out);
                    if (status != 0) {
                        throw Loom.lastError("loom_workspace_list_json");
                    }
                    return Loom.takeOwnedString(out.get(ValueLayout.ADDRESS, 0));
                });
    }

    /** Rename {@code workspace} (UUID or name) to {@code newName}. */
    public void rename(String workspace, String newName) {
        session.onHandle("loom_workspace_rename",
                (arena, handle) -> {
                    int status = (int) Loom.LOOM_WORKSPACE_RENAME.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(newName));
                    if (status != 0) {
                        throw Loom.lastError("loom_workspace_rename");
                    }
                    return null;
                });
    }

    /** Delete {@code workspace} (UUID or name) and its facets. */
    public void delete(String workspace) {
        session.onHandle("loom_workspace_delete",
                (arena, handle) -> {
                    int status = (int) Loom.LOOM_WORKSPACE_DELETE.invokeExact(handle,
                            arena.allocateFrom(workspace));
                    if (status != 0) {
                        throw Loom.lastError("loom_workspace_delete");
                    }
                    return null;
                });
    }
}
