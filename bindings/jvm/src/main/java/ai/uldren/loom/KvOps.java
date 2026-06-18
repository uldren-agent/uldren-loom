package ai.uldren.loom;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.ValueLayout;

/**
 * Key-value facet operations for a {@link LoomSession}: typed key/value maps, one per collection, in a
 * workspace. Reached via {@link LoomSession#kv()}. Keys and values cross as Loom Canonical CBOR typed
 * cells (the SQL cell codec); {@code list}/{@code range} return the canonical-CBOR array of
 * {@code [key, value]} pairs in key order. An absent key or collection reads as absent. Owns the FFM
 * downcalls directly via {@link Loom#onHandle}.
 *
 * <p>Licensed under BUSL-1.1 (see the workspace {@code LICENSE}). (c) Uldren Technologies LLC.
 */
public final class KvOps {
    private final LoomSession session;

    KvOps(LoomSession session) {
        this.session = session;
    }

    /** Put {@code value} at the typed {@code key} in map {@code collection} (created if absent). */
    public void put(String workspace, String collection, byte[] key, byte[] value) {
        session.onHandle("loom_kv_put", (arena, handle) -> {
            int status = (int) Loom.LOOM_KV_PUT.invokeExact(handle, arena.allocateFrom(workspace),
                    arena.allocateFrom(collection), Loom.bytesOrNull(arena, key),
                    (long) (key != null ? key.length : 0), Loom.bytesOrNull(arena, value),
                    (long) (value != null ? value.length : 0));
            if (status != 0) {
                throw Loom.lastError("loom_kv_put");
            }
            return null;
        });
    }

    /** Fetch the value at typed {@code key} in map {@code collection}, or {@code null} if absent. */
    public byte[] get(String workspace, String collection, byte[] key) {
        return session.onHandle("loom_kv_get",
                (arena, handle) -> {
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    MemorySegment outFound = arena.allocate(ValueLayout.JAVA_INT);
                    int status = (int) Loom.LOOM_KV_GET.invokeExact(handle, arena.allocateFrom(workspace),
                            arena.allocateFrom(collection), Loom.bytesOrNull(arena, key),
                            (long) (key != null ? key.length : 0), outPtr, outLen, outFound);
                    if (status != 0) {
                        throw Loom.lastError("loom_kv_get");
                    }
                    if (outFound.get(ValueLayout.JAVA_INT, 0) == 0) {
                        return null;
                    }
                    return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0));
                });
    }

    /** Remove the typed {@code key} from map {@code collection}; returns whether it was present. */
    public boolean delete(String workspace, String collection, byte[] key) {
        return session.onHandle("loom_kv_delete",
                (arena, handle) -> {
                    MemorySegment outFound = arena.allocate(ValueLayout.JAVA_INT);
                    int status = (int) Loom.LOOM_KV_DELETE.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(collection),
                            Loom.bytesOrNull(arena, key), (long) (key != null ? key.length : 0), outFound);
                    if (status != 0) {
                        throw Loom.lastError("loom_kv_delete");
                    }
                    return outFound.get(ValueLayout.JAVA_INT, 0) != 0;
                });
    }

    /** List map {@code collection} as Loom Canonical CBOR {@code [key, value]} pairs in key order. */
    public byte[] list(String workspace, String collection) {
        return session.onHandle("loom_kv_list_cbor",
                (arena, handle) -> {
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    int status = (int) Loom.LOOM_KV_LIST_CBOR.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(collection), outPtr, outLen);
                    if (status != 0) {
                        throw Loom.lastError("loom_kv_list_cbor");
                    }
                    return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0));
                });
    }

    /** Entries of map {@code collection} with {@code lo <= key < hi} (half-open) as CBOR pairs. */
    public byte[] range(String workspace, String collection, byte[] lo, byte[] hi) {
        return session.onHandle("loom_kv_range_cbor",
                (arena, handle) -> {
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    int status = (int) Loom.LOOM_KV_RANGE_CBOR.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(collection),
                            Loom.bytesOrNull(arena, lo), (long) (lo != null ? lo.length : 0),
                            Loom.bytesOrNull(arena, hi), (long) (hi != null ? hi.length : 0), outPtr, outLen);
                    if (status != 0) {
                        throw Loom.lastError("loom_kv_range_cbor");
                    }
                    return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0));
                });
    }
}
