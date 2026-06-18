import CUldrenLoom
import Foundation

extension Loom {
    /// Create (or replace the metadata of) mailbox `mailbox` under `principal` in `workspace` (UUID or
    /// name, created with the `mail` facet if absent). `displayName` is the mailbox's display name.
    public func mailCreateMailbox(workspace: String, principal: String, mailbox: String,
                                  displayName: String) throws {
        let status = loom_mail_create_mailbox(session, workspace, principal, mailbox, displayName)
        guard status == 0 else { throw LoomSql.lastError() }
    }

    /// Delete mailbox `mailbox` under `principal` and every message index and flag set in it (immutable
    /// bodies stay in the CAS until GC); returns whether it existed.
    public func mailDeleteMailbox(workspace: String, principal: String,
                                  mailbox: String) throws -> Bool {
        var found: Int32 = 0
        let status = loom_mail_delete_mailbox(session, workspace, principal, mailbox, &found)
        guard status == 0 else { throw LoomSql.lastError() }
        return found != 0
    }

    /// List the mailbox ids under `principal` as the Loom Canonical CBOR array of text strings (sorted;
    /// an absent principal is the empty array).
    public func mailListMailboxes(workspace: String, principal: String) throws -> Data {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = loom_mail_list_mailboxes(session, workspace, principal, &ptr, &len)
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return Data() }
        return Data(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }

    /// Ingest the raw RFC 5322 message `raw` into mailbox `mailbox` under `uid` (store the immutable
    /// body in the CAS, parse the headers into a structured index, write it); returns the body's content
    /// address as a `"algo:hex"` string.
    public func mailIngestMessage(workspace: String, principal: String, mailbox: String, uid: String,
                                  raw: Data) throws -> String {
        var out: UnsafeMutablePointer<CChar>?
        let status = raw.withUnsafeBytes { rraw -> Int32 in
            let base = rraw.bindMemory(to: UInt8.self).baseAddress
            return loom_mail_ingest_message(session, workspace, principal, mailbox, uid, base,
                                            UInt(rraw.count), &out)
        }
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_string_free(out) }
        return out.map { String(cString: $0) } ?? ""
    }

    /// Fetch the structured index of the message at `uid` in mailbox `mailbox` as its `MailMessage`
    /// canonical CBOR, or nil if absent.
    public func mailGetMessage(workspace: String, principal: String, mailbox: String,
                               uid: String) throws -> Data? {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        var found: Int32 = 0
        let status = loom_mail_get_message(session, workspace, principal, mailbox, uid, &ptr, &len,
                                           &found)
        guard status == 0 else { throw LoomSql.lastError() }
        guard found != 0 else { return nil }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return Data() }
        return Data(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }

    /// Fetch the raw RFC 5322 body (`.eml` bytes) of the message at `uid`, from the CAS and
    /// digest-verified, or nil if absent.
    public func mailToEml(workspace: String, principal: String, mailbox: String,
                            uid: String) throws -> Data? {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        var found: Int32 = 0
        let status = loom_mail_to_eml(session, workspace, principal, mailbox, uid, &ptr, &len, &found)
        guard status == 0 else { throw LoomSql.lastError() }
        guard found != 0 else { return nil }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return Data() }
        return Data(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }

    /// Remove the message index and its flags at `uid` (the immutable body stays in the CAS until GC);
    /// returns whether it was present.
    public func mailDeleteMessage(workspace: String, principal: String, mailbox: String,
                                  uid: String) throws -> Bool {
        var found: Int32 = 0
        let status = loom_mail_delete_message(session, workspace, principal, mailbox, uid, &found)
        guard status == 0 else { throw LoomSql.lastError() }
        return found != 0
    }

    /// List mailbox `mailbox` as the Loom Canonical CBOR array of per-message `MailMessage` canonical
    /// CBOR byte strings (UID order; an absent mailbox is the empty array).
    public func mailListMessages(workspace: String, principal: String, mailbox: String) throws -> Data {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = loom_mail_list_messages(session, workspace, principal, mailbox, &ptr, &len)
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return Data() }
        return Data(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }

    /// The flags/labels on the message at `uid` as the Loom Canonical CBOR array of text strings
    /// (sorted, deduplicated; an absent flag set is the empty array).
    public func mailGetFlags(workspace: String, principal: String, mailbox: String,
                             uid: String) throws -> Data {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = loom_mail_get_flags(session, workspace, principal, mailbox, uid, &ptr, &len)
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return Data() }
        return Data(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }

    /// Replace the flags/labels on the message at `uid` with `flags`, a Loom Canonical CBOR
    /// `Array(Text)` buffer. The message must exist.
    public func mailSetFlags(workspace: String, principal: String, mailbox: String, uid: String,
                             flags: Data) throws {
        let status = flags.withUnsafeBytes { fraw -> Int32 in
            let base = fraw.bindMemory(to: UInt8.self).baseAddress
            return loom_mail_set_flags(session, workspace, principal, mailbox, uid, base,
                                       UInt(fraw.count))
        }
        guard status == 0 else { throw LoomSql.lastError() }
    }

    /// Search mailbox `mailbox` by a case-insensitive substring `text` over the subject and from values
    /// as the Loom Canonical CBOR array of per-message `MailMessage` canonical CBOR byte strings.
    public func mailSearch(workspace: String, principal: String, mailbox: String,
                           text: String) throws -> Data {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = loom_mail_search(session, workspace, principal, mailbox, text, &ptr, &len)
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return Data() }
        return Data(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }
}
