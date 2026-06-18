import CUldrenLoom
import Foundation

extension Loom {
    /// Create (or replace the metadata of) address book `book` under `principal` in `workspace` (UUID
    /// or name, created with the `contacts` facet if absent). `displayName` is the book's display name.
    public func cardCreateBook(workspace: String, principal: String, book: String,
                               displayName: String) throws {
        let status = loom_card_create_book(session, workspace, principal, book, displayName)
        guard status == 0 else { throw LoomSql.lastError() }
    }

    /// Delete address book `book` under `principal` and every contact in it; returns whether it existed.
    public func cardDeleteBook(workspace: String, principal: String, book: String) throws -> Bool {
        var found: Int32 = 0
        let status = loom_card_delete_book(session, workspace, principal, book, &found)
        guard status == 0 else { throw LoomSql.lastError() }
        return found != 0
    }

    /// List the address-book ids under `principal` as the Loom Canonical CBOR array of text strings
    /// (sorted; an absent principal is the empty array).
    public func cardListBooks(workspace: String, principal: String) throws -> Data {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = loom_card_list_books(session, workspace, principal, &ptr, &len)
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return Data() }
        return Data(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }

    /// Put the contact `entry` (its `ContactEntry` canonical CBOR) into the existing address book `book`
    /// under `principal`, keyed by its UID. A later put at the same UID replaces it.
    public func cardPutEntry(workspace: String, principal: String, book: String, entry: Data) throws {
        let status = entry.withUnsafeBytes { raw -> Int32 in
            let base = raw.bindMemory(to: UInt8.self).baseAddress
            return loom_card_put_entry(session, workspace, principal, book, base, UInt(raw.count))
        }
        guard status == 0 else { throw LoomSql.lastError() }
    }

    /// Fetch the contact at `uid` in address book `book` as its `ContactEntry` canonical CBOR, or nil if
    /// absent.
    public func cardGetEntry(workspace: String, principal: String, book: String,
                             uid: String) throws -> Data? {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        var found: Int32 = 0
        let status = loom_card_get_entry(session, workspace, principal, book, uid, &ptr, &len, &found)
        guard status == 0 else { throw LoomSql.lastError() }
        guard found != 0 else { return nil }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return Data() }
        return Data(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }

    /// Remove the contact at `uid` in address book `book`; returns whether it was present.
    public func cardDeleteEntry(workspace: String, principal: String, book: String,
                                uid: String) throws -> Bool {
        var found: Int32 = 0
        let status = loom_card_delete_entry(session, workspace, principal, book, uid, &found)
        guard status == 0 else { throw LoomSql.lastError() }
        return found != 0
    }

    /// List address book `book` as the Loom Canonical CBOR array of per-contact `ContactEntry` canonical
    /// CBOR byte strings (UID order; an absent book is the empty array).
    public func cardListEntries(workspace: String, principal: String, book: String) throws -> Data {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = loom_card_list_entries(session, workspace, principal, book, &ptr, &len)
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return Data() }
        return Data(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }

    /// Search address book `book` by a case-insensitive substring `text` over the formatted name,
    /// organization, and email values as the Loom Canonical CBOR array of per-contact `ContactEntry`
    /// canonical CBOR byte strings.
    public func cardSearch(workspace: String, principal: String, book: String,
                           text: String) throws -> Data {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = loom_card_search(session, workspace, principal, book, text, &ptr, &len)
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return Data() }
        return Data(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }

    /// The on-demand vCard (`.vcf`) projection of the contact at `uid`, or nil if absent.
    public func cardEntryVcard(workspace: String, principal: String, book: String,
                               uid: String) throws -> String? {
        var out: UnsafeMutablePointer<CChar>?
        var found: Int32 = 0
        let status = loom_card_entry_vcard(session, workspace, principal, book, uid, &out, &found)
        guard status == 0 else { throw LoomSql.lastError() }
        guard found != 0 else { return nil }
        defer { loom_string_free(out) }
        return out.map { String(cString: $0) } ?? ""
    }

    /// Parse vCard document `vcf` and store it as a record in address book `book`; returns the new ETag
    /// as a `"algo:hex"` string.
    public func cardPutVcard(workspace: String, principal: String, book: String,
                             vcf: String) throws -> String {
        var out: UnsafeMutablePointer<CChar>?
        let status = loom_card_put_vcard(session, workspace, principal, book, vcf, &out)
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_string_free(out) }
        return out.map { String(cString: $0) } ?? ""
    }
}
