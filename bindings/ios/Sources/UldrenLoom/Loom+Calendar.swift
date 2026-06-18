import CUldrenLoom
import Foundation

extension Loom {
    /// Create (or replace the metadata of) calendar collection `collection` under `principal` in
    /// `workspace` (UUID or name, created with the `calendar` facet if absent). `displayName` is the
    /// collection's display name; `components` is a comma-separated component set ("event,todo"; "" is
    /// the empty set).
    public func calCreateCollection(workspace: String, principal: String, collection: String,
                                    displayName: String, components: String) throws {
        let status = loom_cal_create_collection(session, workspace, principal, collection, displayName,
                                                components)
        guard status == 0 else { throw LoomSql.lastError() }
    }

    /// Delete calendar collection `collection` under `principal` and every entry in it; returns whether
    /// it existed.
    public func calDeleteCollection(workspace: String, principal: String,
                                    collection: String) throws -> Bool {
        var found: Int32 = 0
        let status = loom_cal_delete_collection(session, workspace, principal, collection, &found)
        guard status == 0 else { throw LoomSql.lastError() }
        return found != 0
    }

    /// List the calendar collection ids under `principal` as the Loom Canonical CBOR array of text
    /// strings (sorted; an absent principal is the empty array).
    public func calListCollections(workspace: String, principal: String) throws -> Data {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = loom_cal_list_collections(session, workspace, principal, &ptr, &len)
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return Data() }
        return Data(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }

    /// Put the calendar `entry` (its `CalendarEntry` canonical CBOR) into the existing collection
    /// `collection` under `principal`, keyed by its UID. A later put at the same UID replaces it.
    public func calPutEntry(workspace: String, principal: String, collection: String,
                            entry: Data) throws {
        let status = entry.withUnsafeBytes { raw -> Int32 in
            let base = raw.bindMemory(to: UInt8.self).baseAddress
            return loom_cal_put_entry(session, workspace, principal, collection, base, UInt(raw.count))
        }
        guard status == 0 else { throw LoomSql.lastError() }
    }

    /// Fetch the calendar entry at `uid` in collection `collection` as its `CalendarEntry` canonical
    /// CBOR, or nil if absent.
    public func calGetEntry(workspace: String, principal: String, collection: String,
                            uid: String) throws -> Data? {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        var found: Int32 = 0
        let status = loom_cal_get_entry(session, workspace, principal, collection, uid, &ptr, &len,
                                        &found)
        guard status == 0 else { throw LoomSql.lastError() }
        guard found != 0 else { return nil }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return Data() }
        return Data(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }

    /// Remove the calendar entry at `uid` in collection `collection`; returns whether it was present.
    public func calDeleteEntry(workspace: String, principal: String, collection: String,
                               uid: String) throws -> Bool {
        var found: Int32 = 0
        let status = loom_cal_delete_entry(session, workspace, principal, collection, uid, &found)
        guard status == 0 else { throw LoomSql.lastError() }
        return found != 0
    }

    /// List collection `collection` as the Loom Canonical CBOR array of per-entry `CalendarEntry`
    /// canonical CBOR byte strings (UID order; an absent collection is the empty array).
    public func calListEntries(workspace: String, principal: String, collection: String) throws -> Data {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = loom_cal_list_entries(session, workspace, principal, collection, &ptr, &len)
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return Data() }
        return Data(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }

    /// Expand collection `collection` into occurrences within the half-open wall-clock window
    /// `[from, to)` (both `YYYYMMDDTHHMMSS`) as the Loom Canonical CBOR array of
    /// `[uid, "YYYYMMDDTHHMMSS"]` pairs.
    public func calRange(workspace: String, principal: String, collection: String, from: String,
                         to: String) throws -> Data {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = loom_cal_range(session, workspace, principal, collection, from, to, &ptr, &len)
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return Data() }
        return Data(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }

    /// Search collection `collection` by component filter (`""`/`"event"`/`"todo"`) and a
    /// case-insensitive summary substring `text` as the Loom Canonical CBOR array of per-entry
    /// `CalendarEntry` canonical CBOR byte strings.
    public func calSearch(workspace: String, principal: String, collection: String, component: String,
                          text: String) throws -> Data {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = loom_cal_search(session, workspace, principal, collection, component, text, &ptr,
                                     &len)
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return Data() }
        return Data(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }

    /// The on-demand iCalendar (`.ics`) projection of the entry at `uid`, or nil if absent.
    public func calEntryIcs(workspace: String, principal: String, collection: String,
                            uid: String) throws -> String? {
        var out: UnsafeMutablePointer<CChar>?
        var found: Int32 = 0
        let status = loom_cal_entry_ics(session, workspace, principal, collection, uid, &out, &found)
        guard status == 0 else { throw LoomSql.lastError() }
        guard found != 0 else { return nil }
        defer { loom_string_free(out) }
        return out.map { String(cString: $0) } ?? ""
    }

    /// Parse iCalendar document `ics` and store it as a record in collection `collection`; returns the
    /// new ETag as a `"algo:hex"` string.
    public func calPutIcs(workspace: String, principal: String, collection: String,
                          ics: String) throws -> String {
        var out: UnsafeMutablePointer<CChar>?
        let status = loom_cal_put_ics(session, workspace, principal, collection, ics, &out)
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_string_free(out) }
        return out.map { String(cString: $0) } ?? ""
    }
}
