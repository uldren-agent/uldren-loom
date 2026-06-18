package ai.uldren.loom;

import java.lang.foreign.MemorySegment;
import java.lang.foreign.ValueLayout;

public final class ArchiveOps {
    private final LoomSession session;

    ArchiveOps(LoomSession session) {
        this.session = session;
    }

    public byte[] importFs(String workspace, String srcPath, boolean commit, boolean dryRun) {
        return session.onHandle("loom_fs_import", (arena, handle) -> {
            MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
            MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
            int status = (int) Loom.LOOM_FS_IMPORT.invokeExact(
                    handle, arena.allocateFrom(workspace), arena.allocateFrom(srcPath),
                    commit ? 1 : 0, dryRun ? 1 : 0, outPtr, outLen);
            if (status != 0) {
                throw Loom.lastError("loom_fs_import");
            }
            return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                    outLen.get(ValueLayout.JAVA_LONG, 0));
        });
    }

    public byte[] exportFs(String workspace, String dstPath, String revision, boolean dryRun) {
        return session.onHandle("loom_fs_export", (arena, handle) -> {
            MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
            MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
            MemorySegment revisionArg = revision != null ? arena.allocateFrom(revision) : MemorySegment.NULL;
            int status = (int) Loom.LOOM_FS_EXPORT.invokeExact(
                    handle, arena.allocateFrom(workspace), arena.allocateFrom(dstPath),
                    revisionArg, dryRun ? 1 : 0, outPtr, outLen);
            if (status != 0) {
                throw Loom.lastError("loom_fs_export");
            }
            return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                    outLen.get(ValueLayout.JAVA_LONG, 0));
        });
    }

    public byte[] importArchive(String workspace, String srcPath, String kind, boolean dryRun) {
        return session.onHandle("loom_archive_import", (arena, handle) -> {
            MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
            MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
            int status = (int) Loom.LOOM_ARCHIVE_IMPORT.invokeExact(
                    handle, arena.allocateFrom(workspace), arena.allocateFrom(srcPath),
                    arena.allocateFrom(kind), dryRun ? 1 : 0, outPtr, outLen);
            if (status != 0) {
                throw Loom.lastError("loom_archive_import");
            }
            return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                    outLen.get(ValueLayout.JAVA_LONG, 0));
        });
    }

    public byte[] exportArchive(String workspace, String dstPath, String kind, String revision,
            boolean dryRun) {
        return session.onHandle("loom_archive_export", (arena, handle) -> {
            MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
            MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
            MemorySegment revisionArg = revision != null ? arena.allocateFrom(revision) : MemorySegment.NULL;
            int status = (int) Loom.LOOM_ARCHIVE_EXPORT.invokeExact(
                    handle, arena.allocateFrom(workspace), arena.allocateFrom(dstPath),
                    arena.allocateFrom(kind), revisionArg, dryRun ? 1 : 0, outPtr, outLen);
            if (status != 0) {
                throw Loom.lastError("loom_archive_export");
            }
            return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                    outLen.get(ValueLayout.JAVA_LONG, 0));
        });
    }

    public byte[] importCar(String srcPath, boolean dryRun) {
        return session.onHandle("loom_car_import", (arena, handle) -> {
            MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
            MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
            int status = (int) Loom.LOOM_CAR_IMPORT.invokeExact(
                    handle, arena.allocateFrom(srcPath), dryRun ? 1 : 0, outPtr, outLen);
            if (status != 0) {
                throw Loom.lastError("loom_car_import");
            }
            return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                    outLen.get(ValueLayout.JAVA_LONG, 0));
        });
    }

    public byte[] exportCar(String workspace, String dstPath, boolean dryRun) {
        return session.onHandle("loom_car_export", (arena, handle) -> {
            MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
            MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
            int status = (int) Loom.LOOM_CAR_EXPORT.invokeExact(
                    handle, arena.allocateFrom(workspace), arena.allocateFrom(dstPath),
                    dryRun ? 1 : 0, outPtr, outLen);
            if (status != 0) {
                throw Loom.lastError("loom_car_export");
            }
            return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                    outLen.get(ValueLayout.JAVA_LONG, 0));
        });
    }
}
