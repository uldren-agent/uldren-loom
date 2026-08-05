package ai.uldren.loom;

import java.lang.foreign.MemorySegment;
import java.lang.foreign.ValueLayout;

public final class StudioMaintenanceOps {
    private final LoomSession session;

    StudioMaintenanceOps(LoomSession session) {
        this.session = session;
    }

    public String reindexJson(String workspace, String profile) {
        return session.onHandle("loom_studio_reindex_json", (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) Loom.LOOM_STUDIO_REINDEX_JSON.invokeExact(
                    handle, arena.allocateFrom(workspace), arena.allocateFrom(profile), out);
            if (status != 0) {
                throw Loom.lastError("loom_studio_reindex_json");
            }
            return Loom.takeOwnedString(out.get(ValueLayout.ADDRESS, 0));
        });
    }

    public String revisionsRebuildJson(String workspace, String profile, boolean dryRun) {
        return session.onHandle("loom_studio_revisions_rebuild_json", (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) Loom.LOOM_STUDIO_REVISIONS_REBUILD_JSON.invokeExact(
                    handle, arena.allocateFrom(workspace), arena.allocateFrom(profile),
                    dryRun ? 1 : 0, out);
            if (status != 0) {
                throw Loom.lastError("loom_studio_revisions_rebuild_json");
            }
            return Loom.takeOwnedString(out.get(ValueLayout.ADDRESS, 0));
        });
    }
}
