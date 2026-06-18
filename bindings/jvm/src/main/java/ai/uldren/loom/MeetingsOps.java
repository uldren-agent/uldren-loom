package ai.uldren.loom;

import java.lang.foreign.MemorySegment;
import java.lang.foreign.ValueLayout;

public final class MeetingsOps {
    private final LoomSession session;

    MeetingsOps(LoomSession session) {
        this.session = session;
    }

    public String importSnapshot(String workspace, String inputProfile, byte[] snapshot, boolean dryRun) {
        return session.onHandle("loom_meetings_import_snapshot", (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) Loom.LOOM_MEETINGS_IMPORT_SNAPSHOT.invokeExact(
                    handle, arena.allocateFrom(workspace), arena.allocateFrom(inputProfile),
                    Loom.bytesOrNull(arena, snapshot), (long) (snapshot != null ? snapshot.length : 0),
                    dryRun ? 1 : 0, out);
            if (status != 0) {
                throw Loom.lastError("loom_meetings_import_snapshot");
            }
            return Loom.takeOwnedString(out.get(ValueLayout.ADDRESS, 0));
        });
    }

    public byte[] sourceRead(String workspace, String sourceId, String leaf) {
        return session.onHandle("loom_meetings_source_read", (arena, handle) -> {
            MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
            MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
            int status = (int) Loom.LOOM_MEETINGS_SOURCE_READ.invokeExact(
                    handle, arena.allocateFrom(workspace), arena.allocateFrom(sourceId),
                    arena.allocateFrom(leaf), outPtr, outLen);
            if (status != 0) {
                throw Loom.lastError("loom_meetings_source_read");
            }
            return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                    outLen.get(ValueLayout.JAVA_LONG, 0));
        });
    }
}
