package ai.uldren.loom;

import java.lang.foreign.FunctionDescriptor;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.ValueLayout;
import java.lang.invoke.MethodHandle;

public final class DriveOps {
    private static final MethodHandle LIST_JSON = down("loom_drive_list_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));
    private static final MethodHandle STAT_JSON = down("loom_drive_stat_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS));
    private static final MethodHandle READ = down("loom_drive_read",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS));
    private static final MethodHandle LIST_VERSIONS_JSON = down("loom_drive_list_versions_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));
    private static final MethodHandle LIST_CONFLICTS_JSON = down("loom_drive_list_conflicts_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));
    private static final MethodHandle LIST_SHARES_JSON = down("loom_drive_list_shares_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));
    private static final MethodHandle LIST_RETENTION_JSON = down("loom_drive_list_retention_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));
    private static final MethodHandle CREATE_FOLDER_JSON = down("loom_drive_create_folder_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));
    private static final MethodHandle CREATE_UPLOAD_JSON = down("loom_drive_create_upload_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.JAVA_LONG, ValueLayout.JAVA_INT, ValueLayout.ADDRESS));
    private static final MethodHandle UPLOAD_CHUNK_JSON = down("loom_drive_upload_chunk_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.JAVA_LONG, ValueLayout.ADDRESS));
    private static final MethodHandle COMMIT_UPLOAD_JSON = down("loom_drive_commit_upload_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));
    private static final MethodHandle RENAME_JSON = down("loom_drive_rename_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));
    private static final MethodHandle MOVE_JSON = down("loom_drive_move_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));
    private static final MethodHandle DELETE_JSON = down("loom_drive_delete_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS));
    private static final MethodHandle RESOLVE_CONFLICT_JSON = down("loom_drive_resolve_conflict_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS));
    private static final MethodHandle GRANT_SHARE_JSON = down("loom_drive_grant_share_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.JAVA_LONG, ValueLayout.JAVA_LONG, ValueLayout.JAVA_INT,
                    ValueLayout.ADDRESS));
    private static final MethodHandle REVOKE_SHARE_JSON = down("loom_drive_revoke_share_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));
    private static final MethodHandle APPLY_SHARE_EXPIRY_JSON = down("loom_drive_apply_share_expiry_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.JAVA_LONG, ValueLayout.ADDRESS));
    private static final MethodHandle PIN_RETENTION_JSON = down("loom_drive_pin_retention_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG,
                    ValueLayout.JAVA_LONG, ValueLayout.JAVA_INT, ValueLayout.ADDRESS));
    private static final MethodHandle UNPIN_RETENTION_JSON = down("loom_drive_unpin_retention_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));
    private static final MethodHandle APPLY_RETENTION_JSON = down("loom_drive_apply_retention_json",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.JAVA_LONG, ValueLayout.ADDRESS));

    private final LoomSession session;

    DriveOps(LoomSession session) {
        this.session = session;
    }

    private static MethodHandle down(String symbol, FunctionDescriptor descriptor) {
        return Loom.LINKER.downcallHandle(Loom.LOOKUP.find(symbol).orElseThrow(), descriptor);
    }

    public String listJson(String workspace, String driveWorkspaceId, String folderId) {
        return string("loom_drive_list_json", LIST_JSON, workspace, driveWorkspaceId, folderId);
    }

    public String statJson(String workspace, String driveWorkspaceId, String folderId, String name) {
        return session.onHandle("loom_drive_stat_json", (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) STAT_JSON.invokeExact(handle, arena.allocateFrom(workspace),
                    arena.allocateFrom(driveWorkspaceId), arena.allocateFrom(folderId),
                    arena.allocateFrom(name), out);
            return takeString("loom_drive_stat_json", status, out);
        });
    }

    public byte[] readFile(String workspace, String driveWorkspaceId, String fileId) {
        return session.onHandle("loom_drive_read", (arena, handle) -> {
            MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
            MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
            int status = (int) READ.invokeExact(handle, arena.allocateFrom(workspace),
                    arena.allocateFrom(driveWorkspaceId), arena.allocateFrom(fileId), outPtr, outLen);
            if (status != 0) {
                throw Loom.lastError("loom_drive_read");
            }
            return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                    outLen.get(ValueLayout.JAVA_LONG, 0));
        });
    }

    public String listVersionsJson(String workspace, String driveWorkspaceId, String fileId) {
        return string("loom_drive_list_versions_json", LIST_VERSIONS_JSON, workspace, driveWorkspaceId, fileId);
    }

    public String listConflictsJson(String workspace, String driveWorkspaceId) {
        return string2("loom_drive_list_conflicts_json", LIST_CONFLICTS_JSON, workspace, driveWorkspaceId);
    }

    public String listSharesJson(String workspace, String driveWorkspaceId) {
        return string2("loom_drive_list_shares_json", LIST_SHARES_JSON, workspace, driveWorkspaceId);
    }

    public String listRetentionJson(String workspace, String driveWorkspaceId) {
        return string2("loom_drive_list_retention_json", LIST_RETENTION_JSON, workspace, driveWorkspaceId);
    }

    public String createFolderJson(String workspace, String driveWorkspaceId, String parentFolderId,
            String folderId, String name, String expectedRoot) {
        return session.onHandle("loom_drive_create_folder_json", (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) CREATE_FOLDER_JSON.invokeExact(handle, arena.allocateFrom(workspace),
                    arena.allocateFrom(driveWorkspaceId), arena.allocateFrom(parentFolderId),
                    arena.allocateFrom(folderId), arena.allocateFrom(name), arena.allocateFrom(expectedRoot),
                    out);
            return takeString("loom_drive_create_folder_json", status, out);
        });
    }

    public String createUploadJson(String workspace, String driveWorkspaceId, String uploadId,
            String parentFolderId, String name, String fileId, String expectedRoot, long createdAtMs,
            boolean replaceFile) {
        return session.onHandle("loom_drive_create_upload_json", (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) CREATE_UPLOAD_JSON.invokeExact(handle, arena.allocateFrom(workspace),
                    arena.allocateFrom(driveWorkspaceId), arena.allocateFrom(uploadId),
                    arena.allocateFrom(parentFolderId), arena.allocateFrom(name), arena.allocateFrom(fileId),
                    arena.allocateFrom(expectedRoot), createdAtMs, replaceFile ? 1 : 0, out);
            return takeString("loom_drive_create_upload_json", status, out);
        });
    }

    public String uploadChunkJson(String workspace, String driveWorkspaceId, String uploadId, byte[] chunk) {
        return session.onHandle("loom_drive_upload_chunk_json", (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) UPLOAD_CHUNK_JSON.invokeExact(handle, arena.allocateFrom(workspace),
                    arena.allocateFrom(driveWorkspaceId), arena.allocateFrom(uploadId),
                    Loom.bytesOrNull(arena, chunk), (long) (chunk != null ? chunk.length : 0), out);
            return takeString("loom_drive_upload_chunk_json", status, out);
        });
    }

    public String commitUploadJson(String workspace, String driveWorkspaceId, String uploadId) {
        return string("loom_drive_commit_upload_json", COMMIT_UPLOAD_JSON, workspace, driveWorkspaceId, uploadId);
    }

    public String renameJson(String workspace, String driveWorkspaceId, String folderId,
            String nodeId, String newName, String expectedRoot) {
        return session.onHandle("loom_drive_rename_json", (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) RENAME_JSON.invokeExact(handle, arena.allocateFrom(workspace),
                    arena.allocateFrom(driveWorkspaceId), arena.allocateFrom(folderId),
                    arena.allocateFrom(nodeId), arena.allocateFrom(newName), arena.allocateFrom(expectedRoot),
                    out);
            return takeString("loom_drive_rename_json", status, out);
        });
    }

    public String moveJson(String workspace, String driveWorkspaceId, String sourceFolderId,
            String targetFolderId, String nodeId, String expectedRoot) {
        return session.onHandle("loom_drive_move_json", (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) MOVE_JSON.invokeExact(handle, arena.allocateFrom(workspace),
                    arena.allocateFrom(driveWorkspaceId), arena.allocateFrom(sourceFolderId),
                    arena.allocateFrom(targetFolderId), arena.allocateFrom(nodeId), arena.allocateFrom(expectedRoot),
                    out);
            return takeString("loom_drive_move_json", status, out);
        });
    }

    public String deleteJson(String workspace, String driveWorkspaceId, String folderId,
            String nodeId, String expectedRoot) {
        return session.onHandle("loom_drive_delete_json", (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) DELETE_JSON.invokeExact(handle, arena.allocateFrom(workspace),
                    arena.allocateFrom(driveWorkspaceId), arena.allocateFrom(folderId),
                    arena.allocateFrom(nodeId), arena.allocateFrom(expectedRoot), out);
            return takeString("loom_drive_delete_json", status, out);
        });
    }

    public String resolveConflictJson(String workspace, String driveWorkspaceId,
            String conflictId, String resolution) {
        return session.onHandle("loom_drive_resolve_conflict_json", (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) RESOLVE_CONFLICT_JSON.invokeExact(handle, arena.allocateFrom(workspace),
                    arena.allocateFrom(driveWorkspaceId), arena.allocateFrom(conflictId),
                    arena.allocateFrom(resolution), out);
            return takeString("loom_drive_resolve_conflict_json", status, out);
        });
    }

    public String grantShareJson(String workspace, String driveWorkspaceId, String grantId,
            String targetKind, String targetId, String principal, String role, long grantedAtMs,
            Long expiresAtMs) {
        return session.onHandle("loom_drive_grant_share_json", (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) GRANT_SHARE_JSON.invokeExact(handle, arena.allocateFrom(workspace),
                    arena.allocateFrom(driveWorkspaceId), arena.allocateFrom(grantId),
                    arena.allocateFrom(targetKind), arena.allocateFrom(targetId), arena.allocateFrom(principal),
                    arena.allocateFrom(role), grantedAtMs, expiresAtMs != null ? expiresAtMs : 0L,
                    expiresAtMs != null ? 1 : 0, out);
            return takeString("loom_drive_grant_share_json", status, out);
        });
    }

    public String revokeShareJson(String workspace, String driveWorkspaceId, String grantId) {
        return string("loom_drive_revoke_share_json", REVOKE_SHARE_JSON, workspace, driveWorkspaceId, grantId);
    }

    public String applyShareExpiryJson(String workspace, String driveWorkspaceId, long nowMs) {
        return session.onHandle("loom_drive_apply_share_expiry_json", (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) APPLY_SHARE_EXPIRY_JSON.invokeExact(handle,
                    arena.allocateFrom(workspace), arena.allocateFrom(driveWorkspaceId), nowMs, out);
            return takeString("loom_drive_apply_share_expiry_json", status, out);
        });
    }

    public String pinRetentionJson(String workspace, String driveWorkspaceId, String pinId,
            String kind, String root, String targetEntityId, long addedAtMs, Long expiresAtMs) {
        return session.onHandle("loom_drive_pin_retention_json", (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) PIN_RETENTION_JSON.invokeExact(handle, arena.allocateFrom(workspace),
                    arena.allocateFrom(driveWorkspaceId), arena.allocateFrom(pinId), arena.allocateFrom(kind),
                    arena.allocateFrom(root), targetEntityId != null ? arena.allocateFrom(targetEntityId) : MemorySegment.NULL,
                    addedAtMs, expiresAtMs != null ? expiresAtMs : 0L, expiresAtMs != null ? 1 : 0, out);
            return takeString("loom_drive_pin_retention_json", status, out);
        });
    }

    public String unpinRetentionJson(String workspace, String driveWorkspaceId, String pinId) {
        return string("loom_drive_unpin_retention_json", UNPIN_RETENTION_JSON, workspace, driveWorkspaceId, pinId);
    }

    public String applyRetentionJson(String workspace, String driveWorkspaceId, long nowMs) {
        return session.onHandle("loom_drive_apply_retention_json", (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) APPLY_RETENTION_JSON.invokeExact(handle,
                    arena.allocateFrom(workspace), arena.allocateFrom(driveWorkspaceId), nowMs, out);
            return takeString("loom_drive_apply_retention_json", status, out);
        });
    }

    private String string2(String symbol, MethodHandle handle, String workspace, String driveWorkspaceId) {
        return session.onHandle(symbol, (arena, h) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) handle.invokeExact(h, arena.allocateFrom(workspace),
                    arena.allocateFrom(driveWorkspaceId), out);
            return takeString(symbol, status, out);
        });
    }

    private String string(String symbol, MethodHandle handle, String workspace, String driveWorkspaceId,
            String value) {
        return session.onHandle(symbol, (arena, h) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) handle.invokeExact(h, arena.allocateFrom(workspace),
                    arena.allocateFrom(driveWorkspaceId), arena.allocateFrom(value), out);
            return takeString(symbol, status, out);
        });
    }

    private static String takeString(String symbol, int status, MemorySegment out) throws Throwable {
        if (status != 0) {
            throw Loom.lastError(symbol);
        }
        return Loom.takeOwnedString(out.get(ValueLayout.ADDRESS, 0));
    }
}
