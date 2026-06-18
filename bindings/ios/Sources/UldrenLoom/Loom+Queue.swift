import CUldrenLoom
import Foundation

extension Loom {
    /// Append `entry` to `stream` in `workspace` (UUID or name, created with the queue facet if absent);
    /// returns the assigned zero-based sequence.
    public func queueAppend(workspace: String, stream: String, entry: Data) throws -> UInt64 {
        var seq: UInt64 = 0
        let status = entry.withUnsafeBytes { raw -> Int32 in
            let base = raw.bindMemory(to: UInt8.self).baseAddress
            return loom_queue_append(session, workspace, stream, base, UInt(raw.count), &seq)
        }
        guard status == 0 else { throw LoomSql.lastError() }
        return seq
    }

    /// Fetch the entry at `seq` in `stream`, or nil if out of range.
    public func queueGet(workspace: String, stream: String, seq: UInt64) throws -> Data? {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        var found: Int32 = 0
        let status = loom_queue_get(session, workspace, stream, seq, &ptr, &len, &found)
        guard status == 0 else { throw LoomSql.lastError() }
        guard found != 0 else { return nil }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return Data() }
        return Data(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }

    /// The half-open range `[lo, hi)` of `stream` as raw Loom Canonical CBOR (an array of byte strings).
    public func queueRangeCbor(workspace: String, stream: String, lo: UInt64, hi: UInt64) throws -> [UInt8] {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = loom_queue_range(session, workspace, stream, lo, hi, &ptr, &len)
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return [] }
        return Array(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }

    /// The number of entries in `stream`.
    public func queueLen(workspace: String, stream: String) throws -> UInt64 {
        var len: UInt64 = 0
        guard loom_queue_len(session, workspace, stream, &len) == 0 else { throw LoomSql.lastError() }
        return len
    }

    /// The named consumer's next sequence for `stream`; 0 when none is stored.
    public func queueConsumerPosition(workspace: String, stream: String, consumerId: String) throws -> UInt64 {
        var seq: UInt64 = 0
        guard loom_queue_consumer_position(session, workspace, stream, consumerId, &seq) == 0 else {
            throw LoomSql.lastError()
        }
        return seq
    }

    /// Up to `max` entries from the consumer's stored next sequence as raw Loom Canonical CBOR; does not
    /// advance the consumer.
    public func queueConsumerReadCbor(workspace: String, stream: String, consumerId: String,
                                      max: UInt32) throws -> [UInt8] {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = loom_queue_consumer_read(session, workspace, stream, consumerId, max, &ptr, &len)
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return [] }
        return Array(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }

    /// Advance the named consumer's next sequence for `stream` to `nextSeq` (monotonic).
    public func queueConsumerAdvance(workspace: String, stream: String, consumerId: String,
                                     nextSeq: UInt64) throws {
        guard loom_queue_consumer_advance(session, workspace, stream, consumerId, nextSeq) == 0 else {
            throw LoomSql.lastError()
        }
    }

    /// Set the named consumer's next sequence for `stream` to `nextSeq` (may move backward).
    public func queueConsumerReset(workspace: String, stream: String, consumerId: String,
                                   nextSeq: UInt64) throws {
        guard loom_queue_consumer_reset(session, workspace, stream, consumerId, nextSeq) == 0 else {
            throw LoomSql.lastError()
        }
    }
}
