import CUldrenLoom
import Foundation

extension Loom {
    private func driveString(_ call: (UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>) -> Int32) throws -> String {
        var out: UnsafeMutablePointer<CChar>?
        let status = call(&out)
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_string_free(out) }
        return out.map { String(cString: $0) } ?? ""
    }

    public func driveListJson(workspace: String, driveWorkspaceId: String, folderId: String) throws -> String {
        try driveString { loom_drive_list_json(session, workspace, driveWorkspaceId, folderId, $0) }
    }

    public func driveStatJson(workspace: String, driveWorkspaceId: String, folderId: String,
                              name: String) throws -> String {
        try driveString { loom_drive_stat_json(session, workspace, driveWorkspaceId, folderId, name, $0) }
    }

    public func driveReadFile(workspace: String, driveWorkspaceId: String, fileId: String) throws -> Data {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = loom_drive_read(session, workspace, driveWorkspaceId, fileId, &ptr, &len)
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return Data() }
        return Data(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }

    public func driveListVersionsJson(workspace: String, driveWorkspaceId: String,
                                      fileId: String) throws -> String {
        try driveString { loom_drive_list_versions_json(session, workspace, driveWorkspaceId, fileId, $0) }
    }

    public func driveListConflictsJson(workspace: String, driveWorkspaceId: String) throws -> String {
        try driveString { loom_drive_list_conflicts_json(session, workspace, driveWorkspaceId, $0) }
    }

    public func driveListSharesJson(workspace: String, driveWorkspaceId: String) throws -> String {
        try driveString { loom_drive_list_shares_json(session, workspace, driveWorkspaceId, $0) }
    }

    public func driveListRetentionJson(workspace: String, driveWorkspaceId: String) throws -> String {
        try driveString { loom_drive_list_retention_json(session, workspace, driveWorkspaceId, $0) }
    }

    public func driveCreateFolderJson(workspace: String, driveWorkspaceId: String,
                                      parentFolderId: String, folderId: String, name: String,
                                      expectedRoot: String) throws -> String {
        try driveString {
            loom_drive_create_folder_json(
                session, workspace, driveWorkspaceId, parentFolderId, folderId, name, expectedRoot, $0
            )
        }
    }

    public func driveCreateUploadJson(workspace: String, driveWorkspaceId: String,
                                      uploadId: String, parentFolderId: String, name: String,
                                      fileId: String, expectedRoot: String, createdAtMs: UInt64,
                                      replaceFile: Bool) throws -> String {
        try driveString {
            loom_drive_create_upload_json(
                session, workspace, driveWorkspaceId, uploadId, parentFolderId, name, fileId,
                expectedRoot, createdAtMs, replaceFile ? 1 : 0, $0
            )
        }
    }

    public func driveUploadChunkJson(workspace: String, driveWorkspaceId: String,
                                     uploadId: String, chunk: Data) throws -> String {
        try chunk.withUnsafeBytes { raw in
            let base = raw.bindMemory(to: UInt8.self).baseAddress
            return try driveString {
                loom_drive_upload_chunk_json(
                    session, workspace, driveWorkspaceId, uploadId, base, UInt(raw.count), $0
                )
            }
        }
    }

    public func driveCommitUploadJson(workspace: String, driveWorkspaceId: String,
                                      uploadId: String) throws -> String {
        try driveString { loom_drive_commit_upload_json(session, workspace, driveWorkspaceId, uploadId, $0) }
    }

    public func driveRenameJson(workspace: String, driveWorkspaceId: String, folderId: String,
                                nodeId: String, newName: String, expectedRoot: String) throws -> String {
        try driveString {
            loom_drive_rename_json(session, workspace, driveWorkspaceId, folderId, nodeId, newName, expectedRoot, $0)
        }
    }

    public func driveMoveJson(workspace: String, driveWorkspaceId: String,
                              sourceFolderId: String, targetFolderId: String,
                              nodeId: String, expectedRoot: String) throws -> String {
        try driveString {
            loom_drive_move_json(
                session, workspace, driveWorkspaceId, sourceFolderId, targetFolderId, nodeId, expectedRoot, $0
            )
        }
    }

    public func driveDeleteJson(workspace: String, driveWorkspaceId: String, folderId: String,
                                nodeId: String, expectedRoot: String) throws -> String {
        try driveString {
            loom_drive_delete_json(session, workspace, driveWorkspaceId, folderId, nodeId, expectedRoot, $0)
        }
    }

    public func driveResolveConflictJson(workspace: String, driveWorkspaceId: String,
                                         conflictId: String, resolution: String) throws -> String {
        try driveString {
            loom_drive_resolve_conflict_json(session, workspace, driveWorkspaceId, conflictId, resolution, $0)
        }
    }

    public func driveGrantShareJson(workspace: String, driveWorkspaceId: String, grantId: String,
                                    targetKind: String, targetId: String, principal: String,
                                    role: String, grantedAtMs: UInt64,
                                    expiresAtMs: UInt64? = nil) throws -> String {
        try driveString {
            loom_drive_grant_share_json(
                session, workspace, driveWorkspaceId, grantId, targetKind, targetId, principal, role,
                grantedAtMs, expiresAtMs ?? 0, expiresAtMs == nil ? 0 : 1, $0
            )
        }
    }

    public func driveRevokeShareJson(workspace: String, driveWorkspaceId: String,
                                     grantId: String) throws -> String {
        try driveString { loom_drive_revoke_share_json(session, workspace, driveWorkspaceId, grantId, $0) }
    }

    public func driveApplyShareExpiryJson(workspace: String, driveWorkspaceId: String,
                                          nowMs: UInt64) throws -> String {
        try driveString { loom_drive_apply_share_expiry_json(session, workspace, driveWorkspaceId, nowMs, $0) }
    }

    public func drivePinRetentionJson(workspace: String, driveWorkspaceId: String, pinId: String,
                                      kind: String, root: String, targetEntityId: String?,
                                      addedAtMs: UInt64, expiresAtMs: UInt64? = nil) throws -> String {
        try driveString {
            loom_drive_pin_retention_json(
                session, workspace, driveWorkspaceId, pinId, kind, root, targetEntityId,
                addedAtMs, expiresAtMs ?? 0, expiresAtMs == nil ? 0 : 1, $0
            )
        }
    }

    public func driveUnpinRetentionJson(workspace: String, driveWorkspaceId: String,
                                        pinId: String) throws -> String {
        try driveString { loom_drive_unpin_retention_json(session, workspace, driveWorkspaceId, pinId, $0) }
    }

    public func driveApplyRetentionJson(workspace: String, driveWorkspaceId: String,
                                        nowMs: UInt64) throws -> String {
        try driveString { loom_drive_apply_retention_json(session, workspace, driveWorkspaceId, nowMs, $0) }
    }
}
