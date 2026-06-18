package ai.uldren.loom;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.ValueLayout;

/**
 * Time-series facet operations for a {@link LoomSession}: points by {@code i64} timestamp, per
 * collection, in a workspace. Reached via {@link LoomSession#timeSeries()}. Owns the FFM downcalls
 * directly via {@link Loom#onHandle}.
 *
 * <p>Licensed under BUSL-1.1 (see the workspace {@code LICENSE}). (c) Uldren Technologies LLC.
 */
public final class TimeSeriesOps {
    private final LoomSession session;

    TimeSeriesOps(LoomSession session) {
        this.session = session;
    }

    /** Record {@code value} at timestamp {@code ts} in series {@code collection} (created if absent). */
    public void put(String workspace, String collection, long ts, byte[] value) {
        session.onHandle("loom_ts_put", (arena, handle) -> {
            int status = (int) Loom.LOOM_TS_PUT.invokeExact(handle, arena.allocateFrom(workspace),
                    arena.allocateFrom(collection), ts, Loom.bytesOrNull(arena, value),
                    (long) (value != null ? value.length : 0));
            if (status != 0) {
                throw Loom.lastError("loom_ts_put");
            }
            return null;
        });
    }

    /** Fetch the point at timestamp {@code ts} in series {@code collection}, or {@code null} if absent. */
    public byte[] get(String workspace, String collection, long ts) {
        return session.onHandle("loom_ts_get",
                (arena, handle) -> {
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    MemorySegment outFound = arena.allocate(ValueLayout.JAVA_INT);
                    int status = (int) Loom.LOOM_TS_GET.invokeExact(handle, arena.allocateFrom(workspace),
                            arena.allocateFrom(collection), ts, outPtr, outLen, outFound);
                    if (status != 0) {
                        throw Loom.lastError("loom_ts_get");
                    }
                    if (outFound.get(ValueLayout.JAVA_INT, 0) == 0) {
                        return null;
                    }
                    return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0));
                });
    }

    /** Points of series {@code collection} with {@code from <= ts < to} (half-open) as CBOR pairs. */
    public byte[] range(String workspace, String collection, long from, long to) {
        return session.onHandle("loom_ts_range_cbor",
                (arena, handle) -> {
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    int status = (int) Loom.LOOM_TS_RANGE_CBOR.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(collection), from, to, outPtr,
                            outLen);
                    if (status != 0) {
                        throw Loom.lastError("loom_ts_range_cbor");
                    }
                    return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0));
                });
    }

    /** The most recent point of series {@code collection} as {@code (ts, value)}, or {@code null}. */
    public Loom.TsPoint latest(String workspace, String collection) {
        return session.onHandle("loom_ts_latest",
                (arena, handle) -> {
                    MemorySegment outTs = arena.allocate(ValueLayout.JAVA_LONG);
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    MemorySegment outFound = arena.allocate(ValueLayout.JAVA_INT);
                    int status = (int) Loom.LOOM_TS_LATEST.invokeExact(handle, arena.allocateFrom(workspace),
                            arena.allocateFrom(collection), outTs, outPtr, outLen, outFound);
                    if (status != 0) {
                        throw Loom.lastError("loom_ts_latest");
                    }
                    if (outFound.get(ValueLayout.JAVA_INT, 0) == 0) {
                        return null;
                    }
                    byte[] value = Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0));
                    return new Loom.TsPoint(outTs.get(ValueLayout.JAVA_LONG, 0), value);
                });
    }
}
