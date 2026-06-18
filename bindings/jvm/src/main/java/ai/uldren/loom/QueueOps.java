package ai.uldren.loom;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.ValueLayout;

/**
 * Append-log queue operations for a {@link LoomSession}: an ordered stream of entries per workspace,
 * plus durable per-consumer read offsets. Reached via {@link LoomSession#queue()}. {@code append}
 * returns the assigned zero-based sequence; {@code range} is half-open {@code [lo, hi)} and ordered by
 * sequence. Consumer methods track and advance a named consumer's position. Owns the FFM downcalls
 * directly via {@link Loom#onHandle}.
 *
 * <p>Licensed under BUSL-1.1 (see the workspace {@code LICENSE}). (c) Uldren Technologies LLC.
 */
public final class QueueOps {
    private final LoomSession session;

    QueueOps(LoomSession session) {
        this.session = session;
    }

    /** Append {@code entry} to {@code stream}; returns the assigned zero-based sequence. */
    public long append(String workspace, String stream, byte[] entry) {
        return session.onHandle("loom_queue_append",
                (arena, handle) -> {
                    MemorySegment outSeq = arena.allocate(ValueLayout.JAVA_LONG);
                    int status = (int) Loom.LOOM_QUEUE_APPEND.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(stream),
                            Loom.bytesOrNull(arena, entry), (long) (entry != null ? entry.length : 0),
                            outSeq);
                    if (status != 0) {
                        throw Loom.lastError("loom_queue_append");
                    }
                    return outSeq.get(ValueLayout.JAVA_LONG, 0);
                });
    }

    /** Fetch the entry at {@code seq} in {@code stream}, or {@code null} if out of range. */
    public byte[] get(String workspace, String stream, long seq) {
        return session.onHandle("loom_queue_get",
                (arena, handle) -> {
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    MemorySegment outFound = arena.allocate(ValueLayout.JAVA_INT);
                    int status = (int) Loom.LOOM_QUEUE_GET.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(stream), seq, outPtr, outLen,
                            outFound);
                    if (status != 0) {
                        throw Loom.lastError("loom_queue_get");
                    }
                    if (outFound.get(ValueLayout.JAVA_INT, 0) == 0) {
                        return null;
                    }
                    return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0));
                });
    }

    /** The half-open range {@code [lo, hi)} of {@code stream} as Loom Canonical CBOR, ordered by seq. */
    public byte[] range(String workspace, String stream, long lo, long hi) {
        return session.onHandle("loom_queue_range",
                (arena, handle) -> {
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    int status = (int) Loom.LOOM_QUEUE_RANGE.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(stream), lo, hi, outPtr,
                            outLen);
                    if (status != 0) {
                        throw Loom.lastError("loom_queue_range");
                    }
                    return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0));
                });
    }

    /** The number of entries appended to {@code stream}. */
    public long len(String workspace, String stream) {
        return session.onHandle("loom_queue_len",
                (arena, handle) -> {
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    int status = (int) Loom.LOOM_QUEUE_LEN.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(stream), outLen);
                    if (status != 0) {
                        throw Loom.lastError("loom_queue_len");
                    }
                    return outLen.get(ValueLayout.JAVA_LONG, 0);
                });
    }

    /** The next sequence {@code consumerId} will read from {@code stream}; 0 when none is stored. */
    public long consumerPosition(String workspace, String stream, String consumerId) {
        return session.onHandle("loom_queue_consumer_position", (arena, handle) -> {
                    MemorySegment outSeq = arena.allocate(ValueLayout.JAVA_LONG);
                    int status = (int) Loom.LOOM_QUEUE_CONSUMER_POSITION.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(stream),
                            arena.allocateFrom(consumerId), outSeq);
                    if (status != 0) {
                        throw Loom.lastError("loom_queue_consumer_position");
                    }
                    return outSeq.get(ValueLayout.JAVA_LONG, 0);
                });
    }

    /** Read up to {@code max} entries from {@code consumerId}'s position as Loom Canonical CBOR. */
    public byte[] consumerRead(String workspace, String stream, String consumerId, int max) {
        return session.onHandle("loom_queue_consumer_read",
                (arena, handle) -> {
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    int status = (int) Loom.LOOM_QUEUE_CONSUMER_READ.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(stream),
                            arena.allocateFrom(consumerId), max, outPtr, outLen);
                    if (status != 0) {
                        throw Loom.lastError("loom_queue_consumer_read");
                    }
                    return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0));
                });
    }

    /** Advance {@code consumerId}'s position in {@code stream} to {@code nextSeq} (monotonic). */
    public void consumerAdvance(String workspace, String stream, String consumerId, long nextSeq) {
        session.onHandle("loom_queue_consumer_advance",
                (arena, handle) -> {
                    int status = (int) Loom.LOOM_QUEUE_CONSUMER_ADVANCE.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(stream),
                            arena.allocateFrom(consumerId), nextSeq);
                    if (status != 0) {
                        throw Loom.lastError("loom_queue_consumer_advance");
                    }
                    return null;
                });
    }

    /** Set {@code consumerId}'s position in {@code stream} to {@code nextSeq} (may move backward). */
    public void consumerReset(String workspace, String stream, String consumerId, long nextSeq) {
        session.onHandle("loom_queue_consumer_reset",
                (arena, handle) -> {
                    int status = (int) Loom.LOOM_QUEUE_CONSUMER_RESET.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(stream),
                            arena.allocateFrom(consumerId), nextSeq);
                    if (status != 0) {
                        throw Loom.lastError("loom_queue_consumer_reset");
                    }
                    return null;
                });
    }
}
