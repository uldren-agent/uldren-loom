package ai.uldren.loom;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.ValueLayout;

/**
 * Columnar facet operations for a {@link LoomSession}: typed columns with append and scan/select, one
 * dataset per named table, in a workspace. Reached via {@link LoomSession#columnar()}. Columns cross as
 * a Loom Canonical CBOR array of {@code [name, type_tag]}; a row crosses as a CBOR cell array;
 * scan/select/columns cross as a CBOR array; the select filter is the CBOR array
 * {@code [column, op, value_cell]} (op: 0 eq, 1 ne, 2 lt, 3 le, 4 gt, 5 ge; empty = scan all). Owns the
 * FFM downcalls directly via {@link Loom#onHandle}.
 *
 * <p>Licensed under BUSL-1.1 (see the workspace {@code LICENSE}). (c) Uldren Technologies LLC.
 */
public final class ColumnarOps {
    private final LoomSession session;

    ColumnarOps(LoomSession session) {
        this.session = session;
    }

    /**
     * Create dataset {@code name} with {@code columns} (CBOR array of {@code [name, type_tag]}).
     * {@code targetSegmentRows} is the per-segment row target, or 0 for the engine default.
     */
    public void create(String workspace, String name, byte[] columns, long targetSegmentRows) {
        session.onHandle("loom_columnar_create",
                (arena, handle) -> {
                    int status = (int) Loom.LOOM_COLUMNAR_CREATE.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(name),
                            Loom.bytesOrNull(arena, columns),
                            (long) (columns != null ? columns.length : 0), targetSegmentRows);
                    if (status != 0) {
                        throw Loom.lastError("loom_columnar_create");
                    }
                    return null;
                });
    }

    /** Append {@code row} (a CBOR cell array) to dataset {@code name}, validating arity and column types. */
    public void append(String workspace, String name, byte[] row) {
        session.onHandle("loom_columnar_append",
                (arena, handle) -> {
                    int status = (int) Loom.LOOM_COLUMNAR_APPEND.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(name),
                            Loom.bytesOrNull(arena, row), (long) (row != null ? row.length : 0));
                    if (status != 0) {
                        throw Loom.lastError("loom_columnar_append");
                    }
                    return null;
                });
    }

    /** All rows of dataset {@code name} in append order as a CBOR array of cell arrays. */
    public byte[] scan(String workspace, String name) {
        return session.onHandle("loom_columnar_scan_cbor",
                (arena, handle) -> {
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    int status = (int) Loom.LOOM_COLUMNAR_SCAN_CBOR.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(name), outPtr, outLen);
                    if (status != 0) {
                        throw Loom.lastError("loom_columnar_scan_cbor");
                    }
                    return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0));
                });
    }

    /** The {@code (name, type_tag)} columns of dataset {@code name} as a CBOR array of {@code [name, type_tag]}. */
    public byte[] columns(String workspace, String name) {
        return session.onHandle("loom_columnar_columns_cbor", (arena, handle) -> {
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    int status = (int) Loom.LOOM_COLUMNAR_COLUMNS_CBOR.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(name), outPtr, outLen);
                    if (status != 0) {
                        throw Loom.lastError("loom_columnar_columns_cbor");
                    }
                    return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0));
                });
    }

    /** The total row count of dataset {@code name}. */
    public long rows(String workspace, String name) {
        return session.onHandle("loom_columnar_rows",
                (arena, handle) -> {
                    MemorySegment outCount = arena.allocate(ValueLayout.JAVA_LONG);
                    int status = (int) Loom.LOOM_COLUMNAR_ROWS.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(name), outCount);
                    if (status != 0) {
                        throw Loom.lastError("loom_columnar_rows");
                    }
                    return outCount.get(ValueLayout.JAVA_LONG, 0);
                });
    }

    /** Compact dataset {@code name} at its target segment size. */
    public void compact(String workspace, String name) {
        session.onHandle("loom_columnar_compact",
                (arena, handle) -> {
                    int status = (int) Loom.LOOM_COLUMNAR_COMPACT.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(name));
                    if (status != 0) {
                        throw Loom.lastError("loom_columnar_compact");
                    }
                    return null;
                });
    }

    /** Inspect dataset metadata as CBOR {@code [columns, rows, segment_count, target_segment_rows, source_digest]}. */
    public byte[] inspect(String workspace, String name) {
        return session.onHandle("loom_columnar_inspect_cbor", (arena, handle) -> {
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    int status = (int) Loom.LOOM_COLUMNAR_INSPECT_CBOR.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(name), outPtr, outLen);
                    if (status != 0) {
                        throw Loom.lastError("loom_columnar_inspect_cbor");
                    }
                    return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0));
                });
    }

    /** Source digest used by derived columnar projections as CBOR text. */
    public byte[] sourceDigest(String workspace, String name) {
        return session.onHandle("loom_columnar_source_digest_cbor", (arena, handle) -> {
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    int status = (int) Loom.LOOM_COLUMNAR_SOURCE_DIGEST_CBOR.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(name), outPtr, outLen);
                    if (status != 0) {
                        throw Loom.lastError("loom_columnar_source_digest_cbor");
                    }
                    return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0));
                });
    }

    /**
     * Project {@code columns} (CBOR array of text) from rows of dataset {@code name} matching
     * {@code filter} (CBOR {@code [column, op, value_cell]}; empty or {@code null} = all) as a CBOR
     * array of cell arrays.
     */
    public byte[] select(String workspace, String name, byte[] columns, byte[] filter) {
        return session.onHandle("loom_columnar_select_cbor",
                (arena, handle) -> {
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    int status = (int) Loom.LOOM_COLUMNAR_SELECT_CBOR.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(name),
                            Loom.bytesOrNull(arena, columns),
                            (long) (columns != null ? columns.length : 0),
                            Loom.bytesOrNull(arena, filter),
                            (long) (filter != null ? filter.length : 0), outPtr, outLen);
                    if (status != 0) {
                        throw Loom.lastError("loom_columnar_select_cbor");
                    }
                    return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0));
                });
    }

    /** Evaluate aggregate expressions from CBOR {@code [[op, column?] ...]}, with optional select filter. */
    public byte[] aggregate(String workspace, String name, byte[] aggregates, byte[] filter) {
        return session.onHandle("loom_columnar_aggregate_cbor",
                (arena, handle) -> {
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    int status = (int) Loom.LOOM_COLUMNAR_AGGREGATE_CBOR.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(name),
                            Loom.bytesOrNull(arena, aggregates),
                            (long) (aggregates != null ? aggregates.length : 0),
                            Loom.bytesOrNull(arena, filter),
                            (long) (filter != null ? filter.length : 0), outPtr, outLen);
                    if (status != 0) {
                        throw Loom.lastError("loom_columnar_aggregate_cbor");
                    }
                    return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0));
                });
    }
}
