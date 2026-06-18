package ai.uldren.loom;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.ValueLayout;

/**
 * Ledger facet operations for a {@link LoomSession}: an append-only hash-chained log, per collection,
 * in a workspace. Reached via {@link LoomSession#ledger()}. Owns the FFM downcalls directly
 * via {@link Loom#onHandle}.
 *
 * <p>Licensed under BUSL-1.1 (see the workspace {@code LICENSE}). (c) Uldren Technologies LLC.
 */
public final class LedgerOps {
    private final LoomSession session;

    LedgerOps(LoomSession session) {
        this.session = session;
    }

    /** Append {@code payload} to ledger {@code collection}; returns the new entry's zero-based sequence. */
    public long append(String workspace, String collection, byte[] payload) {
        return session.onHandle("loom_ledger_append",
                (arena, handle) -> {
                    MemorySegment outSeq = arena.allocate(ValueLayout.JAVA_LONG);
                    int status = (int) Loom.LOOM_LEDGER_APPEND.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(collection),
                            Loom.bytesOrNull(arena, payload), (long) (payload != null ? payload.length : 0),
                            outSeq);
                    if (status != 0) {
                        throw Loom.lastError("loom_ledger_append");
                    }
                    return outSeq.get(ValueLayout.JAVA_LONG, 0);
                });
    }

    /** Fetch the payload at {@code seq} in ledger {@code collection}, or {@code null} if absent. */
    public byte[] get(String workspace, String collection, long seq) {
        return session.onHandle("loom_ledger_get",
                (arena, handle) -> {
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    MemorySegment outFound = arena.allocate(ValueLayout.JAVA_INT);
                    int status = (int) Loom.LOOM_LEDGER_GET.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(collection), seq, outPtr,
                            outLen, outFound);
                    if (status != 0) {
                        throw Loom.lastError("loom_ledger_get");
                    }
                    if (outFound.get(ValueLayout.JAVA_INT, 0) == 0) {
                        return null;
                    }
                    return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0));
                });
    }

    /** The head chain hash of ledger {@code collection} as {@code "algo:hex"}, or {@code null} if empty. */
    public String head(String workspace, String collection) {
        return session.onHandle("loom_ledger_head",
                (arena, handle) -> {
                    MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outFound = arena.allocate(ValueLayout.JAVA_INT);
                    int status = (int) Loom.LOOM_LEDGER_HEAD.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(collection), out, outFound);
                    if (status != 0) {
                        throw Loom.lastError("loom_ledger_head");
                    }
                    if (outFound.get(ValueLayout.JAVA_INT, 0) == 0) {
                        return null;
                    }
                    return Loom.takeOwnedString(out.get(ValueLayout.ADDRESS, 0));
                });
    }

    /** The number of entries in ledger {@code collection} (0 when absent). */
    public long len(String workspace, String collection) {
        return session.onHandle("loom_ledger_len",
                (arena, handle) -> {
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    int status = (int) Loom.LOOM_LEDGER_LEN.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(collection), outLen);
                    if (status != 0) {
                        throw Loom.lastError("loom_ledger_len");
                    }
                    return outLen.get(ValueLayout.JAVA_LONG, 0);
                });
    }

    /** Recompute the chain and confirm every stored hash matches; throws if broken. */
    public void verify(String workspace, String collection) {
        session.onHandle("loom_ledger_verify",
                (arena, handle) -> {
                    int status = (int) Loom.LOOM_LEDGER_VERIFY.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(collection));
                    if (status != 0) {
                        throw Loom.lastError("loom_ledger_verify");
                    }
                    return null;
                });
    }
}
