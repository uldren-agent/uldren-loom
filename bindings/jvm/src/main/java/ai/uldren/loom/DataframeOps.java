package ai.uldren.loom;

import java.lang.foreign.MemorySegment;
import java.lang.foreign.ValueLayout;
import java.util.Optional;

/**
 * Dataframe facet operations for a {@link LoomSession}. Plans cross as canonical DataframePlan CBOR;
 * collect and preview return canonical CBOR {@code [columns, rows]}.
 *
 * <p>Licensed under BUSL-1.1 (see the workspace {@code LICENSE}). (c) Uldren Technologies LLC.
 */
public final class DataframeOps {
    private final LoomSession session;

    DataframeOps(LoomSession session) {
        this.session = session;
    }

    /** Create frame {@code name} from canonical DataframePlan CBOR. */
    public void create(String workspace, String name, byte[] plan) {
        session.onHandle("loom_dataframe_create",
                (arena, handle) -> {
                    int status = (int) Loom.LOOM_DATAFRAME_CREATE.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(name),
                            Loom.bytesOrNull(arena, plan), (long) (plan != null ? plan.length : 0));
                    if (status != 0) {
                        throw Loom.lastError("loom_dataframe_create");
                    }
                    return null;
                });
    }

    /** Execute frame {@code name} as canonical CBOR {@code [columns, rows]}. */
    public byte[] collect(String workspace, String name) {
        return session.onHandle("loom_dataframe_collect_cbor",
                (arena, handle) -> {
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    int status = (int) Loom.LOOM_DATAFRAME_COLLECT_CBOR.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(name), outPtr, outLen);
                    if (status != 0) {
                        throw Loom.lastError("loom_dataframe_collect_cbor");
                    }
                    return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0));
                });
    }

    /** Execute frame {@code name} and return at most {@code rows} rows as canonical CBOR {@code [columns, rows]}. */
    public byte[] preview(String workspace, String name, long rows) {
        return session.onHandle("loom_dataframe_preview_cbor",
                (arena, handle) -> {
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    int status = (int) Loom.LOOM_DATAFRAME_PREVIEW_CBOR.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(name), rows, outPtr,
                            outLen);
                    if (status != 0) {
                        throw Loom.lastError("loom_dataframe_preview_cbor");
                    }
                    return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0));
                });
    }

    /** Materialize frame {@code name}; returns a CAS digest when the materialization target emits one. */
    public Optional<String> materialize(String workspace, String name) {
        return session.onHandle("loom_dataframe_materialize",
                (arena, handle) -> {
                    MemorySegment outDigest = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outHasDigest = arena.allocate(ValueLayout.JAVA_INT);
                    int status = (int) Loom.LOOM_DATAFRAME_MATERIALIZE.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(name), outDigest,
                            outHasDigest);
                    if (status != 0) {
                        throw Loom.lastError("loom_dataframe_materialize");
                    }
                    if (outHasDigest.get(ValueLayout.JAVA_INT, 0) == 0) {
                        return Optional.empty();
                    }
                    return Optional.of(Loom.takeOwnedString(outDigest.get(ValueLayout.ADDRESS, 0)));
                });
    }

    /** Canonical dataframe plan digest as {@code algo:hex}. */
    public String planDigest(String workspace, String name) {
        return session.onHandle("loom_dataframe_plan_digest", (arena, handle) -> {
                    MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
                    int status = (int) Loom.LOOM_DATAFRAME_PLAN_DIGEST.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(name), out);
                    if (status != 0) {
                        throw Loom.lastError("loom_dataframe_plan_digest");
                    }
                    return Loom.takeOwnedString(out.get(ValueLayout.ADDRESS, 0));
                });
    }

    /** Source digests pinned in the dataframe plan as canonical CBOR array of {@code algo:hex} strings. */
    public byte[] sourceDigests(String workspace, String name) {
        return session.onHandle("loom_dataframe_source_digests_cbor", (arena, handle) -> {
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    int status = (int) Loom.LOOM_DATAFRAME_SOURCE_DIGESTS_CBOR.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(name), outPtr, outLen);
                    if (status != 0) {
                        throw Loom.lastError("loom_dataframe_source_digests_cbor");
                    }
                    return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0));
                });
    }
}
