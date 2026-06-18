import CUldrenLoom
import Foundation

extension Loom {
    /// Create columnar dataset `name` in `workspace` (created with the `columnar` facet if absent). `columns`
    /// is a Loom Canonical CBOR array of `[name, type_tag]`; `targetSegmentRows` of 0 uses the default.
    public func columnarCreate(workspace: String, name: String, columns: Data,
                               targetSegmentRows: Int = 0) throws {
        let status = columns.withUnsafeBytes { raw -> Int32 in
            let base = raw.bindMemory(to: UInt8.self).baseAddress
            return loom_columnar_create(session, workspace, name, base, UInt(raw.count),
                                        UInt(targetSegmentRows))
        }
        guard status == 0 else { throw LoomSql.lastError() }
    }

    /// Append `row` (a Loom Canonical CBOR cell array) to dataset `name`, validating arity and column types.
    public func columnarAppend(workspace: String, name: String, row: Data) throws {
        let status = row.withUnsafeBytes { raw -> Int32 in
            let base = raw.bindMemory(to: UInt8.self).baseAddress
            return loom_columnar_append(session, workspace, name, base, UInt(raw.count))
        }
        guard status == 0 else { throw LoomSql.lastError() }
    }

    /// All rows of dataset `name` in append order as the Loom Canonical CBOR array of cell arrays.
    public func columnarScan(workspace: String, name: String) throws -> Data {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = loom_columnar_scan_cbor(session, workspace, name, &ptr, &len)
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return Data() }
        return Data(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }

    /// The `(name, type_tag)` columns of dataset `name` as the Loom Canonical CBOR array of `[name, type_tag]`.
    public func columnarColumns(workspace: String, name: String) throws -> Data {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = loom_columnar_columns_cbor(session, workspace, name, &ptr, &len)
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return Data() }
        return Data(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }

    /// The total row count of dataset `name`.
    public func columnarRows(workspace: String, name: String) throws -> UInt64 {
        var count: UInt64 = 0
        let status = loom_columnar_rows(session, workspace, name, &count)
        guard status == 0 else { throw LoomSql.lastError() }
        return count
    }

    /// Compact dataset `name` at its target segment size.
    public func columnarCompact(workspace: String, name: String) throws {
        let status = loom_columnar_compact(session, workspace, name)
        guard status == 0 else { throw LoomSql.lastError() }
    }

    /// Inspect dataset metadata as the Loom Canonical CBOR array
    /// `[columns, rows, segment_count, target_segment_rows, source_digest]`.
    public func columnarInspect(workspace: String, name: String) throws -> Data {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = loom_columnar_inspect_cbor(session, workspace, name, &ptr, &len)
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return Data() }
        return Data(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }

    /// Source digest used by derived columnar projections as CBOR text.
    public func columnarSourceDigest(workspace: String, name: String) throws -> Data {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = loom_columnar_source_digest_cbor(session, workspace, name, &ptr, &len)
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return Data() }
        return Data(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }

    /// Project `columns` (a CBOR array of text) from the rows of dataset `name` matching `filter` as the Loom
    /// Canonical CBOR array of cell arrays. `filter` is the CBOR array `[column, op, value_cell]`; an empty
    /// `filter` scans all rows.
    public func columnarSelect(workspace: String, name: String, columns: Data,
                               filter: Data = Data()) throws -> Data {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = columns.withUnsafeBytes { craw -> Int32 in
            let cbase = craw.bindMemory(to: UInt8.self).baseAddress
            return filter.withUnsafeBytes { fraw -> Int32 in
                let fbase = fraw.bindMemory(to: UInt8.self).baseAddress
                return loom_columnar_select_cbor(session, workspace, name, cbase, UInt(craw.count), fbase,
                                                 UInt(fraw.count), &ptr, &len)
            }
        }
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return Data() }
        return Data(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }

    /// Evaluate aggregate expressions from CBOR `[[op, column?] ...]`, with optional select filter.
    public func columnarAggregate(workspace: String, name: String, aggregates: Data,
                                  filter: Data = Data()) throws -> Data {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = aggregates.withUnsafeBytes { araw -> Int32 in
            let abase = araw.bindMemory(to: UInt8.self).baseAddress
            return filter.withUnsafeBytes { fraw -> Int32 in
                let fbase = fraw.bindMemory(to: UInt8.self).baseAddress
                return loom_columnar_aggregate_cbor(session, workspace, name, abase, UInt(araw.count),
                                                    fbase, UInt(fraw.count), &ptr, &len)
            }
        }
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return Data() }
        return Data(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }
}
