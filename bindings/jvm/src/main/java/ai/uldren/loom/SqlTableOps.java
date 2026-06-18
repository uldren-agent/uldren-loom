package ai.uldren.loom;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.ValueLayout;

/**
 * SQL table inspection for a {@link LoomSession}: the read-only direct readers over the versioned
 * tabular store. Reached via {@link LoomSession#tables()}. Each returns a {@link Loom.LoomResult} (an
 * {@link AutoCloseable} decoded result view) - close it (try-with-resources) when done. For running
 * SQL (exec/commit), use {@link LoomSession#sql(String, String)}. Owns the FFM downcalls directly
 * via {@link Loom#onHandle}; the raw result buffer is decoded with {@link Loom#openResult}.
 *
 * <p>Licensed under BUSL-1.1 (see the workspace {@code LICENSE}). (c) Uldren Technologies LLC.
 */
public final class SqlTableOps {
    private final LoomSession session;

    SqlTableOps(LoomSession session) {
        this.session = session;
    }

    /** Read all rows of {@code table} in {@code workspace} as a decoded result view. */
    public Loom.LoomResult readTable(String workspace, String table) {
        return session.onHandle("loom_sql_read_table",
                (arena, handle) -> {
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    int status = (int) Loom.LOOM_SQL_READ_TABLE.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(table), outPtr, outLen);
                    if (status != 0) {
                        throw Loom.lastError("loom_sql_read_table");
                    }
                    return Loom.openResult(Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0)));
                });
    }

    /** Scan {@code index} on {@code table} for keys with the given {@code prefix}. */
    public Loom.LoomResult indexScan(String workspace, String table, String index, byte[] prefix) {
        return session.onHandle("loom_sql_index_scan",
                (arena, handle) -> {
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    int status = (int) Loom.LOOM_SQL_INDEX_SCAN.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(table),
                            arena.allocateFrom(index), Loom.bytesOrNull(arena, prefix),
                            (long) (prefix != null ? prefix.length : 0), outPtr, outLen);
                    if (status != 0) {
                        throw Loom.lastError("loom_sql_index_scan");
                    }
                    return Loom.openResult(Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0)));
                });
    }

    /** Blame for {@code table} on {@code branch}: the commit that last set each row. */
    public Loom.LoomResult blame(String workspace, String branch, String table) {
        return session.onHandle("loom_sql_blame",
                (arena, handle) -> {
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    int status = (int) Loom.LOOM_SQL_BLAME.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(branch),
                            arena.allocateFrom(table), outPtr, outLen);
                    if (status != 0) {
                        throw Loom.lastError("loom_sql_blame");
                    }
                    return Loom.openResult(Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0)));
                });
    }

    /** Diff of {@code table} between commits {@code fromCommit} and {@code toCommit}. */
    public Loom.LoomResult diff(String workspace, String table, String fromCommit, String toCommit) {
        return session.onHandle("loom_sql_diff",
                (arena, handle) -> {
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    int status = (int) Loom.LOOM_SQL_DIFF.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(table),
                            arena.allocateFrom(fromCommit), arena.allocateFrom(toCommit), outPtr, outLen);
                    if (status != 0) {
                        throw Loom.lastError("loom_sql_diff");
                    }
                    return Loom.openResult(Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0)));
                });
    }
}
