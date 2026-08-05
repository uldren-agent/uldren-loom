package ai.uldren.loom;

import java.lang.foreign.MemorySegment;
import java.lang.foreign.ValueLayout;

public final class RefsOps {
    private final LoomSession session;

    RefsOps(LoomSession session) {
        this.session = session;
    }

    public String reconcileJson(String workspace, long max) {
        return session.onHandle("loom_refs_reconcile_json", (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) Loom.LOOM_REFS_RECONCILE_JSON.invokeExact(
                    handle, arena.allocateFrom(workspace), max, out);
            if (status != 0) {
                throw Loom.lastError("loom_refs_reconcile_json");
            }
            return Loom.takeOwnedString(out.get(ValueLayout.ADDRESS, 0));
        });
    }
}
