import CUldrenLoom
import Foundation

extension Loom {
    public func sqlReadTable(workspace: String, table: String) throws -> LoomResult {
        try Loom.openResult(sqlReadTableBytes(workspace: workspace, table: table))
    }

    public func sqlReadTableBytes(workspace: String, table: String) throws -> [UInt8] {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = loom_sql_read_table(session, workspace, table, &ptr, &len)
        guard status == 0 else { throw LoomSql.lastError() }
        return Loom.takeBytes(ptr, len)
    }

    public func sqlReadTableAt(workspace: String, table: String,
                               commit: String) throws -> LoomResult {
        try Loom.openResult(
            sqlReadTableAtBytes(workspace: workspace, table: table, commit: commit)
        )
    }

    public func sqlReadTableAtBytes(workspace: String, table: String,
                                    commit: String) throws -> [UInt8] {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = loom_sql_read_table_at(session, workspace, table, commit, &ptr, &len)
        guard status == 0 else { throw LoomSql.lastError() }
        return Loom.takeBytes(ptr, len)
    }

    public func sqlIndexScan(workspace: String, table: String, index: String,
                             prefix: Data) throws -> LoomResult {
        try Loom.openResult(
            sqlIndexScanBytes(workspace: workspace, table: table, index: index, prefix: prefix)
        )
    }

    public func sqlIndexScanBytes(workspace: String, table: String, index: String,
                                  prefix: Data) throws -> [UInt8] {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = prefix.withUnsafeBytes { raw -> Int32 in
            let base = raw.bindMemory(to: UInt8.self).baseAddress
            return loom_sql_index_scan(session, workspace, table, index, base, UInt(raw.count), &ptr, &len)
        }
        guard status == 0 else { throw LoomSql.lastError() }
        return Loom.takeBytes(ptr, len)
    }

    public func sqlIndexScanAt(workspace: String, table: String, index: String,
                               prefix: Data, commit: String) throws -> LoomResult {
        try Loom.openResult(
            sqlIndexScanAtBytes(workspace: workspace, table: table, index: index,
                                prefix: prefix, commit: commit)
        )
    }

    public func sqlIndexScanAtBytes(workspace: String, table: String, index: String,
                                    prefix: Data, commit: String) throws -> [UInt8] {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = prefix.withUnsafeBytes { raw -> Int32 in
            let base = raw.bindMemory(to: UInt8.self).baseAddress
            return loom_sql_index_scan_at(session, workspace, table, index, base, UInt(raw.count),
                                          commit, &ptr, &len)
        }
        guard status == 0 else { throw LoomSql.lastError() }
        return Loom.takeBytes(ptr, len)
    }

    public func sqlBlame(workspace: String, branch: String,
                         table: String) throws -> LoomResult {
        try Loom.openResult(
            sqlBlameBytes(workspace: workspace, branch: branch, table: table)
        )
    }

    public func sqlBlameBytes(workspace: String, branch: String,
                              table: String) throws -> [UInt8] {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = loom_sql_blame(session, workspace, branch, table, &ptr, &len)
        guard status == 0 else { throw LoomSql.lastError() }
        return Loom.takeBytes(ptr, len)
    }

    public func sqlDiff(workspace: String, table: String, fromCommit: String,
                        toCommit: String) throws -> LoomResult {
        try Loom.openResult(
            sqlDiffBytes(workspace: workspace, table: table, fromCommit: fromCommit,
                         toCommit: toCommit)
        )
    }

    public func sqlDiffBytes(workspace: String, table: String, fromCommit: String,
                             toCommit: String) throws -> [UInt8] {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = loom_sql_diff(session, workspace, table, fromCommit, toCommit, &ptr, &len)
        guard status == 0 else { throw LoomSql.lastError() }
        return Loom.takeBytes(ptr, len)
    }

    public func sqlTableDiffBytes(workspace: String, table: String, fromCommit: String,
                                  toCommit: String) throws -> [UInt8] {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = loom_sql_table_diff(session, workspace, table, fromCommit, toCommit, &ptr, &len)
        guard status == 0 else { throw LoomSql.lastError() }
        return Loom.takeBytes(ptr, len)
    }
}
