package ai.uldren.loom;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.ValueLayout;
import java.util.List;

/**
 * Version-control inspection for a {@link LoomSession}: workspace blame and structural diff over the
 * versioned working tree. Reached via {@link LoomSession#vcs()}. Each returns a {@link Loom.LoomResult}
 * (an {@link AutoCloseable} decoded result view); close it (try-with-resources) when done. Owns the FFM
 * downcalls directly via {@link Loom#onHandle}.
 *
 * <p>Licensed under BUSL-1.1 (see the workspace {@code LICENSE}). (c) Uldren Technologies LLC.
 */
public final class VcsOps {
    private final LoomSession session;

    VcsOps(LoomSession session) {
        this.session = session;
    }

    /** Blame for {@code branch} in {@code workspace}: each current path with the commit that last set it. */
    public Loom.LoomResult blame(String workspace, String branch) {
        return session.onHandle("loom_vcs_blame",
                (arena, handle) -> {
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    int status = (int) Loom.LOOM_VCS_BLAME.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(branch), outPtr, outLen);
                    if (status != 0) {
                        throw Loom.lastError("loom_vcs_blame");
                    }
                    return Loom.openResult(Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0)));
                });
    }

    /** Structural diff between commits {@code fromCommit} and {@code toCommit} in {@code workspace}. */
    public Loom.LoomResult diff(String workspace, String fromCommit, String toCommit) {
        return session.onHandle("loom_vcs_diff",
                (arena, handle) -> {
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    int status = (int) Loom.LOOM_VCS_DIFF.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(fromCommit),
                            arena.allocateFrom(toCommit), outPtr, outLen);
                    if (status != 0) {
                        throw Loom.lastError("loom_vcs_diff");
                    }
                    return Loom.openResult(Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0)));
                });
    }

    /** Subscribe to workspace history changes and return an opaque watch cursor string. */
    public String watchSubscribe(String workspace, String branch, String facet, String pathPrefix,
            List<String> changeKinds, String fromCommit) {
        return session.onHandle("loom_watch_subscribe",
                (arena, handle) -> {
                    MemorySegment facetSeg = present(arena, facet);
                    MemorySegment pathSeg = present(arena, pathPrefix);
                    String kinds = changeKinds == null ? "" : String.join(",", changeKinds);
                    MemorySegment kindsSeg = present(arena, kinds);
                    MemorySegment fromSeg = present(arena, fromCommit);
                    MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
                    int status = (int) Loom.LOOM_WATCH_SUBSCRIBE.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(branch), facetSeg,
                            pathSeg, kindsSeg, fromSeg, out);
                    if (status != 0) {
                        throw Loom.lastError("loom_watch_subscribe");
                    }
                    return Loom.takeOwnedString(out.get(ValueLayout.ADDRESS, 0));
                });
    }

    /** Poll an opaque watch cursor and return a canonical-CBOR {@code loom.watch.batch.v1} batch. */
    public byte[] watchPollBytes(String cursor, int max) {
        return session.onHandle("loom_watch_poll",
                (arena, handle) -> {
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    int status = (int) Loom.LOOM_WATCH_POLL.invokeExact(handle,
                            arena.allocateFrom(cursor), max, outPtr, outLen);
                    if (status != 0) {
                        throw Loom.lastError("loom_watch_poll");
                    }
                    return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0));
                });
    }

    private static MemorySegment present(Arena arena, String value) {
        return value != null && !value.isEmpty() ? arena.allocateFrom(value) : MemorySegment.NULL;
    }
}
