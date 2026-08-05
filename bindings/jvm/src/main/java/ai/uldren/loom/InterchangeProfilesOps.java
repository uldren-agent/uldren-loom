package ai.uldren.loom;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.ValueLayout;

public final class InterchangeProfilesOps {
    private final LoomSession session;

    InterchangeProfilesOps(LoomSession session) {
        this.session = session;
    }

    public byte[] importTableCsv(String workspace, String sourceScope, byte[] csvPayload,
                                 String database, String table, String schema, String primaryKey,
                                 String mode, boolean commit, String author, String message,
                                 boolean dryRun) {
        return session.onHandle("loom_import_table_csv", (arena, handle) -> {
            MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
            MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
            int status = (int) Loom.LOOM_IMPORT_TABLE_CSV.invokeExact(
                    handle, arena.allocateFrom(workspace), arena.allocateFrom(sourceScope),
                    Loom.bytesOrNull(arena, csvPayload), (long) (csvPayload != null ? csvPayload.length : 0),
                    arena.allocateFrom(database), arena.allocateFrom(table), arena.allocateFrom(schema),
                    arena.allocateFrom(primaryKey), arena.allocateFrom(mode), commit ? 1 : 0,
                    nullable(arena, author), nullable(arena, message), dryRun ? 1 : 0,
                    outPtr, outLen);
            if (status != 0) {
                throw Loom.lastError("loom_import_table_csv");
            }
            return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                    outLen.get(ValueLayout.JAVA_LONG, 0));
        });
    }

    public byte[] importRedmine(String workspace, String profile, String sourceScope,
                                byte[] snapshotPayload, String fieldPolicy, boolean dryRun) {
        return ticketImport("loom_import_redmine", Loom.LOOM_IMPORT_REDMINE, workspace, profile,
                sourceScope, snapshotPayload, fieldPolicy, dryRun);
    }

    public byte[] importAsana(String workspace, String profile, String sourceScope,
                              byte[] snapshotPayload, String fieldPolicy, boolean dryRun) {
        return ticketImport("loom_import_asana", Loom.LOOM_IMPORT_ASANA, workspace, profile,
                sourceScope, snapshotPayload, fieldPolicy, dryRun);
    }

    public byte[] importJira(String workspace, String profile, String sourceScope,
                             byte[] snapshotPayload, String fieldPolicy, boolean dryRun) {
        return ticketImport("loom_import_jira", Loom.LOOM_IMPORT_JIRA, workspace, profile,
                sourceScope, snapshotPayload, fieldPolicy, dryRun);
    }

    public byte[] importConfluence(String workspace, String profile, String sourceScope,
                                   byte[] snapshotPayload, String defaultSpace, boolean dryRun) {
        return stringPayloadImport("loom_import_confluence", Loom.LOOM_IMPORT_CONFLUENCE, workspace,
                profile, sourceScope, snapshotPayload, defaultSpace, dryRun);
    }

    public byte[] importSlack(String workspace, String profile, String sourceScope,
                              byte[] snapshotPayload, boolean dryRun) {
        return simpleImport("loom_import_slack", Loom.LOOM_IMPORT_SLACK, workspace, profile,
                sourceScope, snapshotPayload, dryRun);
    }

    public byte[] importDrive(String workspace, String profile, String sourceScope,
                              byte[] archivePayload, boolean dryRun) {
        return simpleImport("loom_import_drive", Loom.LOOM_IMPORT_DRIVE, workspace, profile,
                sourceScope, archivePayload, dryRun);
    }

    public byte[] importMarkdown(String workspace, String profile, String sourceScope,
                                 byte[] archivePayload, String space, boolean dryRun) {
        return stringPayloadImport("loom_import_markdown", Loom.LOOM_IMPORT_MARKDOWN, workspace,
                profile, sourceScope, archivePayload, space, dryRun);
    }

    public byte[] importNotion(String workspace, String profile, String sourceScope,
                               byte[] snapshotPayload, String defaultSpace, boolean dryRun) {
        return stringPayloadImport("loom_import_notion", Loom.LOOM_IMPORT_NOTION, workspace,
                profile, sourceScope, snapshotPayload, defaultSpace, dryRun);
    }

    private byte[] ticketImport(String symbol, java.lang.invoke.MethodHandle method,
                                String workspace, String profile, String sourceScope,
                                byte[] payload, String fieldPolicy, boolean dryRun) {
        return session.onHandle(symbol, (arena, handle) -> {
            MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
            MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
            int status = (int) method.invokeExact(
                    handle, arena.allocateFrom(workspace), arena.allocateFrom(profile),
                    arena.allocateFrom(sourceScope), Loom.bytesOrNull(arena, payload),
                    (long) (payload != null ? payload.length : 0), arena.allocateFrom(fieldPolicy),
                    dryRun ? 1 : 0, outPtr, outLen);
            if (status != 0) {
                throw Loom.lastError(symbol);
            }
            return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                    outLen.get(ValueLayout.JAVA_LONG, 0));
        });
    }

    private byte[] stringPayloadImport(String symbol, java.lang.invoke.MethodHandle method,
                                       String workspace, String profile, String sourceScope,
                                       byte[] payload, String value, boolean dryRun) {
        return session.onHandle(symbol, (arena, handle) -> {
            MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
            MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
            int status = (int) method.invokeExact(
                    handle, arena.allocateFrom(workspace), arena.allocateFrom(profile),
                    arena.allocateFrom(sourceScope), Loom.bytesOrNull(arena, payload),
                    (long) (payload != null ? payload.length : 0), arena.allocateFrom(value),
                    dryRun ? 1 : 0, outPtr, outLen);
            if (status != 0) {
                throw Loom.lastError(symbol);
            }
            return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                    outLen.get(ValueLayout.JAVA_LONG, 0));
        });
    }

    private byte[] simpleImport(String symbol, java.lang.invoke.MethodHandle method,
                                String workspace, String profile, String sourceScope,
                                byte[] payload, boolean dryRun) {
        return session.onHandle(symbol, (arena, handle) -> {
            MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
            MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
            int status = (int) method.invokeExact(
                    handle, arena.allocateFrom(workspace), arena.allocateFrom(profile),
                    arena.allocateFrom(sourceScope), Loom.bytesOrNull(arena, payload),
                    (long) (payload != null ? payload.length : 0), dryRun ? 1 : 0,
                    outPtr, outLen);
            if (status != 0) {
                throw Loom.lastError(symbol);
            }
            return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                    outLen.get(ValueLayout.JAVA_LONG, 0));
        });
    }

    private static MemorySegment nullable(Arena arena, String value) {
        return value != null ? arena.allocateFrom(value) : MemorySegment.NULL;
    }
}
