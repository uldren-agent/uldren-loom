package ai.uldren.loom;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.ValueLayout;

/**
 * Content-addressed store (CAS) operations for a {@link LoomSession}: immutable blobs addressed by
 * their {@code "algo:hex"} digest, per workspace. Reached via {@link LoomSession#cas()}. Owns the
 * FFM downcalls directly; the shared open/close/error dance is {@link Loom#onHandle}.
 *
 * <p>Licensed under BUSL-1.1 (see the workspace {@code LICENSE}). (c) Uldren Technologies LLC.
 */
public final class CasOps {
    private final LoomSession session;

    CasOps(LoomSession session) {
        this.session = session;
    }

    /** Store {@code content}; returns its {@code "algo:hex"} content address. Idempotent. */
    public String put(String workspace, byte[] content) {
        return session.onHandle("loom_cas_put",
                (arena, handle) -> {
                    MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
                    int status = (int) Loom.LOOM_CAS_PUT.invokeExact(handle, arena.allocateFrom(workspace),
                            Loom.bytesOrNull(arena, content), (long) (content != null ? content.length : 0),
                            out);
                    if (status != 0) {
                        throw Loom.lastError("loom_cas_put");
                    }
                    return Loom.takeOwnedString(out.get(ValueLayout.ADDRESS, 0));
                });
    }

    /** Fetch the blob at {@code digest}, or {@code null} if absent. */
    public byte[] get(String workspace, String digest) {
        return session.onHandle("loom_cas_get",
                (arena, handle) -> {
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    MemorySegment outFound = arena.allocate(ValueLayout.JAVA_INT);
                    int status = (int) Loom.LOOM_CAS_GET.invokeExact(handle, arena.allocateFrom(workspace),
                            arena.allocateFrom(digest), outPtr, outLen, outFound);
                    if (status != 0) {
                        throw Loom.lastError("loom_cas_get");
                    }
                    if (outFound.get(ValueLayout.JAVA_INT, 0) == 0) {
                        return null;
                    }
                    return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0));
                });
    }

    /** Whether {@code digest} is present. */
    public boolean has(String workspace, String digest) {
        return session.onHandle("loom_cas_has",
                (arena, handle) -> {
                    MemorySegment outFound = arena.allocate(ValueLayout.JAVA_INT);
                    int status = (int) Loom.LOOM_CAS_HAS.invokeExact(handle, arena.allocateFrom(workspace),
                            arena.allocateFrom(digest), outFound);
                    if (status != 0) {
                        throw Loom.lastError("loom_cas_has");
                    }
                    return outFound.get(ValueLayout.JAVA_INT, 0) != 0;
                });
    }

    /** Remove {@code digest}; returns whether it was present. */
    public boolean delete(String workspace, String digest) {
        return session.onHandle("loom_cas_delete",
                (arena, handle) -> {
                    MemorySegment outFound = arena.allocate(ValueLayout.JAVA_INT);
                    int status = (int) Loom.LOOM_CAS_DELETE.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(digest), outFound);
                    if (status != 0) {
                        throw Loom.lastError("loom_cas_delete");
                    }
                    return outFound.get(ValueLayout.JAVA_INT, 0) != 0;
                });
    }

    /** The reachable blob addresses as a debug JSON array. */
    public String listJson(String workspace) {
        return session.onHandle("loom_cas_list_json",
                (arena, handle) -> {
                    MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
                    int status = (int) Loom.LOOM_CAS_LIST_JSON.invokeExact(handle,
                            arena.allocateFrom(workspace), out);
                    if (status != 0) {
                        throw Loom.lastError("loom_cas_list_json");
                    }
                    return Loom.takeOwnedString(out.get(ValueLayout.ADDRESS, 0));
                });
    }
}
