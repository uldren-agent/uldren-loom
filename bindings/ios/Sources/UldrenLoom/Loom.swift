import CUldrenLoom
import Foundation

/// Swift binding over the Uldren Loom C ABI (`libuldren_loom`).
///
/// Every string returned by the C ABI is owned by the library and freed here with
/// `loom_string_free`, matching the ownership contract in `include/loom.h`.
public final class Loom {
    /// The engine version (the crate's `CARGO_PKG_VERSION`).
    public static func version() -> String {
        guard let ptr = loom_version() else { return "" }
        defer { loom_string_free(ptr) }
        return String(cString: ptr)
    }

    /// The content address (`"algo:hex"`, e.g. `blake3:...`) of `data` as an Uldren Loom blob.
    public static func blobDigest(_ data: Data) -> String {
        data.withUnsafeBytes { raw -> String in
            let base = raw.bindMemory(to: UInt8.self).baseAddress
            // The C ABI accepts a null pointer when len == 0.
            guard let ptr = loom_blob_digest(base, UInt(raw.count)) else { return "" }
            defer { loom_string_free(ptr) }
            return String(cString: ptr)
        }
    }

    /// The build capability report (0010 section 5) as canonical CBOR: a `CapabilitySet` map with
    /// `schema_version` and `records`. Build-aware: capabilities owned by the linked crates are
    /// reported with operational state `supported`. Mirrors the C ABI `loom_capabilities`.
    public static func capabilities() throws -> Data {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = loom_capabilities(&ptr, &len)
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return Data() }
        return Data(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }

    /// The runtime provider/profile report as canonical CBOR.
    public static func runtimeProfile() throws -> Data {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = loom_runtime_profile(&ptr, &len)
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return Data() }
        return Data(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }

    public static func studioSurfaceCatalogJson(workspace: String, set: String = "all") throws -> String {
        var out: UnsafeMutablePointer<CChar>?
        let status = loom_studio_surface_catalog_json(workspace, set, &out)
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_string_free(out) }
        guard let out else { return "" }
        return String(cString: out)
    }

    /// Create a fresh `.loom` at `path` under an identity `profile` (`"default"`/`"blake3"` or
    /// `"fips"`/`"sha256"`), optionally encrypted - the binding counterpart of `loom init`.
    /// A non-nil/non-empty `passphrase` encrypts the store; the DEK is wrapped under it with `suite`,
    /// or the profile default when `suite` is nil; otherwise the store is
    /// unencrypted. Throws on failure (e.g. `ALREADY_EXISTS` if a non-empty file is already there).
    public static func create(path: String, profile: String, suite: String? = nil,
                              passphrase: String? = nil) throws {
        let pass = passphrase.map { Array($0.utf8) } ?? []
        let status = pass.withUnsafeBufferPointer { buf in
            loom_create(path, profile, suite, pass.isEmpty ? nil : buf.baseAddress, UInt(buf.count))
        }
        guard status == 0 else { throw LoomSql.lastError() }
    }

    /// Create a fresh **encrypted** `.loom` whose DEK is wrapped under a host-supplied 256-bit `kek`.
    /// `profile` selects the content-address algorithm and `suite` the object AEAD (profile default
    /// when nil). `kek` must be 32 bytes.
    public static func createWithKek(path: String, profile: String, kek: [UInt8],
                                     suite: String? = nil) throws {
        let status = kek.withUnsafeBufferPointer { buf in
            loom_create_with_kek(path, profile, suite, buf.baseAddress, UInt(buf.count))
        }
        guard status == 0 else { throw LoomSql.lastError() }
    }

    let session: OpaquePointer

    private init(session: OpaquePointer) {
        self.session = session
    }

    public static func open(path: String) throws -> Loom {
        var out: OpaquePointer?
        let status = loom_open(path, &out)
        guard status == 0, let opened = out else { throw LoomSql.lastError() }
        return Loom(session: opened)
    }

    public static func open(path: String, passphrase: String) throws -> Loom {
        var out: OpaquePointer?
        let pass = Array(passphrase.utf8)
        let status = pass.withUnsafeBufferPointer { buf in
            loom_open_keyed(path, buf.baseAddress, UInt(buf.count), &out)
        }
        guard status == 0, let opened = out else { throw LoomSql.lastError() }
        return Loom(session: opened)
    }

    public static func open(path: String, kek: [UInt8]) throws -> Loom {
        var out: OpaquePointer?
        let status = kek.withUnsafeBufferPointer { buf in
            loom_open_with_kek(path, buf.baseAddress, UInt(buf.count), &out)
        }
        guard status == 0, let opened = out else { throw LoomSql.lastError() }
        return Loom(session: opened)
    }

    deinit { loom_close(session) }

    static func takeBytes(_ ptr: UnsafeMutablePointer<UInt8>?, _ len: UInt) -> [UInt8] {
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return [] }
        return Array(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }

    static func openResult(_ bytes: [UInt8]) throws -> LoomResult {
        var view: OpaquePointer?
        let status = bytes.withUnsafeBufferPointer { buf in
            loom_result_open(buf.baseAddress, UInt(buf.count), &view)
        }
        guard status == 0, let view else { throw LoomSql.lastError() }
        return LoomResult(view: view)
    }

    /// Execute canonical `loom.exec.request.v1` bytes and return canonical `loom.exec.result.v1`.
    public func execCbor(_ request: [UInt8]) throws -> [UInt8] {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = request.withUnsafeBufferPointer { buf in
            loom_exec_cbor(session, buf.baseAddress, UInt(buf.count), &ptr, &len)
        }
        guard status == 0 else { throw LoomSql.lastError() }
        return Loom.takeBytes(ptr, len)
    }

    public func meetingsImportSnapshot(workspace: String, inputProfile: String,
                                       snapshot: [UInt8], dryRun: Bool = false) throws -> String {
        var out: UnsafeMutablePointer<CChar>?
        let status = snapshot.withUnsafeBufferPointer { buf in
            loom_meetings_import_snapshot(session, workspace, inputProfile, buf.baseAddress,
                                          UInt(buf.count), dryRun ? 1 : 0, &out)
        }
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_string_free(out) }
        guard let out else { return "" }
        return String(cString: out)
    }

    public func meetingsSourceRead(workspace: String, sourceId: String, leaf: String) throws -> [UInt8] {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = loom_meetings_source_read(session, workspace, sourceId, leaf, &ptr, &len)
        guard status == 0 else { throw LoomSql.lastError() }
        return Loom.takeBytes(ptr, len)
    }

    public func fsImport(workspace: String, srcPath: String,
                         commit: Bool = false, dryRun: Bool = false) throws -> [UInt8] {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = loom_fs_import(session, workspace, srcPath, commit ? 1 : 0,
                                    dryRun ? 1 : 0, &ptr, &len)
        guard status == 0 else { throw LoomSql.lastError() }
        return Loom.takeBytes(ptr, len)
    }

    public func fsExport(workspace: String, dstPath: String,
                         revision: String? = nil, dryRun: Bool = false) throws -> [UInt8] {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status: Int32
        if let revision {
            status = loom_fs_export(session, workspace, dstPath, revision, dryRun ? 1 : 0,
                                    &ptr, &len)
        } else {
            status = loom_fs_export(session, workspace, dstPath, nil, dryRun ? 1 : 0,
                                    &ptr, &len)
        }
        guard status == 0 else { throw LoomSql.lastError() }
        return Loom.takeBytes(ptr, len)
    }

    public func archiveImport(workspace: String, srcPath: String, kind: String,
                              dryRun: Bool = false) throws -> [UInt8] {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = loom_archive_import(session, workspace, srcPath, kind, dryRun ? 1 : 0,
                                         &ptr, &len)
        guard status == 0 else { throw LoomSql.lastError() }
        return Loom.takeBytes(ptr, len)
    }

    public func archiveExport(workspace: String, dstPath: String, kind: String,
                              revision: String? = nil, dryRun: Bool = false) throws -> [UInt8] {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status: Int32
        if let revision {
            status = loom_archive_export(session, workspace, dstPath, kind, revision,
                                         dryRun ? 1 : 0, &ptr, &len)
        } else {
            status = loom_archive_export(session, workspace, dstPath, kind, nil,
                                         dryRun ? 1 : 0, &ptr, &len)
        }
        guard status == 0 else { throw LoomSql.lastError() }
        return Loom.takeBytes(ptr, len)
    }

    public func carImport(srcPath: String, dryRun: Bool = false) throws -> [UInt8] {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = loom_car_import(session, srcPath, dryRun ? 1 : 0, &ptr, &len)
        guard status == 0 else { throw LoomSql.lastError() }
        return Loom.takeBytes(ptr, len)
    }

    public func carExport(workspace: String, dstPath: String, dryRun: Bool = false) throws -> [UInt8] {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = loom_car_export(session, workspace, dstPath, dryRun ? 1 : 0, &ptr, &len)
        guard status == 0 else { throw LoomSql.lastError() }
        return Loom.takeBytes(ptr, len)
    }

    public func fsImportAsync(workspace: String, srcPath: String,
                              commit: Bool = false, dryRun: Bool = false) async throws -> [UInt8] {
        let session = SendableSession(ptr: self.session)
        return try await withCheckedThrowingContinuation { (cont: CheckedContinuation<[UInt8], Error>) in
            DispatchQueue.global().async {
                var task: OpaquePointer?
                if loom_fs_import_async(session.ptr, workspace, srcPath, commit ? 1 : 0,
                                        dryRun ? 1 : 0, &task) != 0 {
                    cont.resume(throwing: LoomSql.lastError())
                    return
                }
                cont.resume(with: Result { try Loom.waitTaskBytes(task) })
            }
        }
    }

    public func fsExportAsync(workspace: String, dstPath: String,
                              revision: String? = nil, dryRun: Bool = false) async throws -> [UInt8] {
        let session = SendableSession(ptr: self.session)
        return try await withCheckedThrowingContinuation { (cont: CheckedContinuation<[UInt8], Error>) in
            DispatchQueue.global().async {
                var task: OpaquePointer?
                let status: Int32
                if let revision {
                    status = loom_fs_export_async(session.ptr, workspace, dstPath, revision,
                                                  dryRun ? 1 : 0, &task)
                } else {
                    status = loom_fs_export_async(session.ptr, workspace, dstPath, nil,
                                                  dryRun ? 1 : 0, &task)
                }
                guard status == 0 else {
                    cont.resume(throwing: LoomSql.lastError())
                    return
                }
                cont.resume(with: Result { try Loom.waitTaskBytes(task) })
            }
        }
    }

    public func archiveImportAsync(workspace: String, srcPath: String, kind: String,
                                   dryRun: Bool = false) async throws -> [UInt8] {
        let session = SendableSession(ptr: self.session)
        return try await withCheckedThrowingContinuation { (cont: CheckedContinuation<[UInt8], Error>) in
            DispatchQueue.global().async {
                var task: OpaquePointer?
                if loom_archive_import_async(session.ptr, workspace, srcPath, kind,
                                             dryRun ? 1 : 0, &task) != 0 {
                    cont.resume(throwing: LoomSql.lastError())
                    return
                }
                cont.resume(with: Result { try Loom.waitTaskBytes(task) })
            }
        }
    }

    public func archiveExportAsync(workspace: String, dstPath: String, kind: String,
                                   revision: String? = nil, dryRun: Bool = false) async throws -> [UInt8] {
        let session = SendableSession(ptr: self.session)
        return try await withCheckedThrowingContinuation { (cont: CheckedContinuation<[UInt8], Error>) in
            DispatchQueue.global().async {
                var task: OpaquePointer?
                let status: Int32
                if let revision {
                    status = loom_archive_export_async(session.ptr, workspace, dstPath, kind, revision,
                                                       dryRun ? 1 : 0, &task)
                } else {
                    status = loom_archive_export_async(session.ptr, workspace, dstPath, kind, nil,
                                                       dryRun ? 1 : 0, &task)
                }
                guard status == 0 else {
                    cont.resume(throwing: LoomSql.lastError())
                    return
                }
                cont.resume(with: Result { try Loom.waitTaskBytes(task) })
            }
        }
    }

    public func carImportAsync(srcPath: String, dryRun: Bool = false) async throws -> [UInt8] {
        let session = SendableSession(ptr: self.session)
        return try await withCheckedThrowingContinuation { (cont: CheckedContinuation<[UInt8], Error>) in
            DispatchQueue.global().async {
                var task: OpaquePointer?
                if loom_car_import_async(session.ptr, srcPath, dryRun ? 1 : 0, &task) != 0 {
                    cont.resume(throwing: LoomSql.lastError())
                    return
                }
                cont.resume(with: Result { try Loom.waitTaskBytes(task) })
            }
        }
    }

    public func carExportAsync(workspace: String, dstPath: String,
                               dryRun: Bool = false) async throws -> [UInt8] {
        let session = SendableSession(ptr: self.session)
        return try await withCheckedThrowingContinuation { (cont: CheckedContinuation<[UInt8], Error>) in
            DispatchQueue.global().async {
                var task: OpaquePointer?
                if loom_car_export_async(session.ptr, workspace, dstPath, dryRun ? 1 : 0, &task) != 0 {
                    cont.resume(throwing: LoomSql.lastError())
                    return
                }
                cont.resume(with: Result { try Loom.waitTaskBytes(task) })
            }
        }
    }

    private static func waitTaskBytes(_ task: OpaquePointer?) throws -> [UInt8] {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = loom_task_wait(task, &ptr, &len)
        loom_task_free(task)
        guard status == 0 else { throw LoomSql.lastError() }
        return Loom.takeBytes(ptr, len)
    }

}

/// A failure from the C ABI: the stable numeric `code` plus a message.
public struct LoomError: Error, CustomStringConvertible {
    public let code: Int32
    public let message: String
    public var description: String { "loom error \(code): \(message)" }
}

/// Carries a raw session pointer across a dispatch boundary for `execAsync`. `@unchecked Sendable` is
/// sound here: the session outlives the async call (documented) and the engine reopens the loom per op.
private struct SendableSession: @unchecked Sendable {
    let ptr: OpaquePointer
}

/// A SQL session over a workspace SQL facet in a `.loom`. A reopenable session: each `exec` / `commit` opens
/// the loom for its duration and releases it, so sessions are cheap and coexist. Throws `LoomError`.
public final class LoomSql {
    private let session: OpaquePointer

    /// Open `path` and start a SQL session over `workspace`'s SQL facet (created if absent),
    /// database `db`.
    public init(path: String, workspace ns: String, db: String) throws {
        var out: OpaquePointer?
        let status = loom_sql_open(path, ns, db, &out)
        guard status == 0, let opened = out else { throw LoomSql.lastError() }
        session = opened
    }

    /// Open a session over an **encrypted** loom, unlocking it with `passphrase`.
    /// The host acquires the passphrase securely; the FFI never reads an environment variable.
    public init(path: String, workspace ns: String, db: String, passphrase: String) throws {
        var out: OpaquePointer?
        let pass = Array(passphrase.utf8)
        let status = pass.withUnsafeBufferPointer { buf in
            loom_sql_open_keyed(path, ns, db, buf.baseAddress, UInt(buf.count), &out)
        }
        guard status == 0, let opened = out else { throw LoomSql.lastError() }
        session = opened
    }

    /// Open a session over an **encrypted** loom with a host-supplied 256-bit `kek` that directly
    /// unwraps the DEK. `kek` may come from a keychain, Secure Enclave, passkey-PRF, or KMS. `kek`
    /// must be 32 bytes.
    public init(path: String, workspace ns: String, db: String, kek: [UInt8]) throws {
        var out: OpaquePointer?
        let status = kek.withUnsafeBufferPointer { buf in
            loom_sql_open_with_kek(path, ns, db, buf.baseAddress, UInt(buf.count), &out)
        }
        guard status == 0, let opened = out else { throw LoomSql.lastError() }
        session = opened
    }

    public init(path: String, workspace ns: String, db: String,
                authPrincipal: String, authPassphrase: String) throws {
        var out: OpaquePointer?
        let auth = Array(authPassphrase.utf8)
        let status = auth.withUnsafeBufferPointer { authBuf in
            loom_sql_open_authenticated(path, ns, db, authPrincipal, authBuf.baseAddress,
                                        UInt(authBuf.count), &out)
        }
        guard status == 0, let opened = out else { throw LoomSql.lastError() }
        session = opened
    }

    public init(path: String, workspace ns: String, db: String, passphrase: String,
                authPrincipal: String, authPassphrase: String) throws {
        var out: OpaquePointer?
        let pass = Array(passphrase.utf8)
        let auth = Array(authPassphrase.utf8)
        let status = pass.withUnsafeBufferPointer { passBuf in
            auth.withUnsafeBufferPointer { authBuf in
                loom_sql_open_keyed_authenticated(path, ns, db, passBuf.baseAddress,
                                                  UInt(passBuf.count), authPrincipal,
                                                  authBuf.baseAddress, UInt(authBuf.count), &out)
            }
        }
        guard status == 0, let opened = out else { throw LoomSql.lastError() }
        session = opened
    }

    public init(path: String, workspace ns: String, db: String, kek: [UInt8],
                authPrincipal: String, authPassphrase: String) throws {
        var out: OpaquePointer?
        let auth = Array(authPassphrase.utf8)
        let status = kek.withUnsafeBufferPointer { kekBuf in
            auth.withUnsafeBufferPointer { authBuf in
                loom_sql_open_with_kek_authenticated(path, ns, db, kekBuf.baseAddress,
                                                     UInt(kekBuf.count), authPrincipal,
                                                     authBuf.baseAddress, UInt(authBuf.count), &out)
            }
        }
        guard status == 0, let opened = out else { throw LoomSql.lastError() }
        session = opened
    }

    deinit { loom_sql_close(session) }

    /// Run SQL and return a **typed**, indexed `LoomResult` (decoded once via the shared result-view; no
    /// CBOR is parsed in Swift). Read cells back as faithful `LoomCell`s. For raw bytes use `execBytes`;
    /// for the JSON debug form use `execJson`.
    public func exec(_ sql: String) throws -> LoomResult {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = loom_sql_exec(session, sql, &ptr, &len)
        guard status == 0 else { throw LoomSql.lastError() }
        var view: OpaquePointer?
        let opened = loom_result_open(ptr, len, &view)
        loom_bytes_free(ptr, len)  // result_open decodes into an owned view; the bytes are done.
        guard opened == 0, let view else { throw LoomSql.lastError() }
        return LoomResult(view: view)
    }

    /// Run SQL; returns a JSON array of the result payloads (debug/admin form, rendered from the
    /// canonical-CBOR result - not the type-faithful API; use `exec`).
    public func execJson(_ sql: String) throws -> String {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = loom_sql_exec(session, sql, &ptr, &len)
        guard status == 0 else { throw LoomSql.lastError() }
        return try LoomSql.renderResult(ptr, len)
    }

    /// Run SQL; returns the result payloads as canonical CBOR bytes.
    public func execBytes(_ sql: String) throws -> [UInt8] {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = loom_sql_exec(session, sql, &ptr, &len)
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return [] }
        return Array(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }

    /// Run a `SELECT` and return a lazy `LoomRowStream` over its rows (the streaming form):
    /// pull rows one at a time with `next()`, never materializing the whole result.
    public func query(_ sql: String) throws -> LoomRowStream {
        var it: OpaquePointer?
        guard loom_sql_query(session, sql, &it) == 0, let it else { throw LoomSql.lastError() }
        return LoomRowStream(iter: it)
    }

    /// Run SQL asynchronously (the poll/session form); returns the canonical-CBOR result
    /// bytes. The blocking wait runs on a background queue, off the caller's thread. The session must
    /// outlive the call.
    public func execAsync(_ sql: String) async throws -> [UInt8] {
        // The session pointer outlives the call (documented), and the C ABI reopens the loom per op,
        // so carrying it across the dispatch boundary is safe.
        let session = SendableSession(ptr: self.session)
        return try await withCheckedThrowingContinuation { (cont: CheckedContinuation<[UInt8], Error>) in
            DispatchQueue.global().async {
                var task: OpaquePointer?
                if loom_sql_exec_async(session.ptr, sql, &task) != 0 {
                    cont.resume(throwing: LoomSql.lastError())
                    return
                }
                var ptr: UnsafeMutablePointer<UInt8>?
                var len: UInt = 0
                let status = loom_task_wait(task, &ptr, &len)
                loom_task_free(task)
                guard status == 0 else {
                    cont.resume(throwing: LoomSql.lastError())
                    return
                }
                let bytes: [UInt8] =
                    (ptr != nil && len > 0)
                    ? Array(UnsafeBufferPointer(start: ptr, count: Int(len))) : []
                loom_bytes_free(ptr, len)
                cont.resume(returning: bytes)
            }
        }
    }

    /// Commit the staged database state; returns the new commit's content address.
    public func commit(message: String, author: String) throws -> String {
        var out: UnsafeMutablePointer<CChar>?
        let status = loom_sql_commit(session, message, author, &out)
        guard status == 0 else { throw LoomSql.lastError() }
        return LoomSql.take(out)
    }

    private static func take(_ ptr: UnsafeMutablePointer<CChar>?) -> String {
        guard let ptr else { return "" }
        defer { loom_string_free(ptr) }
        return String(cString: ptr)
    }

    /// Render a canonical-CBOR result buffer to JSON (debug form) and free it.
    private static func renderResult(_ ptr: UnsafeMutablePointer<UInt8>?, _ len: UInt) throws -> String {
        var json: UnsafeMutablePointer<CChar>?
        let status = loom_result_to_json(ptr, len, &json)
        loom_bytes_free(ptr, len)
        guard status == 0 else { throw LoomSql.lastError() }
        return LoomSql.take(json)
    }

    static func lastError() -> LoomError {
        var code: Int32 = 0
        var msg: UnsafeMutablePointer<CChar>?
        var len: UInt = 0
        loom_last_error(&code, &msg, &len)
        let message = msg.map { String(cString: $0) } ?? "loom error"
        if let msg { loom_string_free(msg) }
        return LoomError(code: code, message: message)
    }
}

/// An explicit transaction/batch scope. Unlike `LoomSql`, a batch holds the `.loom` open -
/// and its exclusive write lock - for its whole lifetime, so an SQL transaction (`BEGIN`/`COMMIT`/
/// `ROLLBACK`) can span `exec` calls; changes become durable through a single atomic save at `commit`
/// (or `commitVcs`). The SQL `COMMIT` is distinct from the VCS commit. Call `close()` (or let the
/// instance deinit) to release the lock; closing without a commit discards un-persisted changes.
public final class LoomSqlBatch {
    private var batch: OpaquePointer?

    /// Begin a batch over `workspace`'s SQL facet (created if absent), database `db`, in `path`.
    public init(path: String, workspace ns: String, db: String) throws {
        var out: OpaquePointer?
        let status = loom_sql_batch_begin(path, ns, db, &out)
        guard status == 0, let opened = out else { throw LoomSql.lastError() }
        batch = opened
    }

    /// Begin a batch over an **encrypted** loom, unlocking it with `passphrase` for the batch's lifetime.
    public init(path: String, workspace ns: String, db: String, passphrase: String) throws {
        var out: OpaquePointer?
        let pass = Array(passphrase.utf8)
        let status = pass.withUnsafeBufferPointer { buf in
            loom_sql_batch_begin_keyed(path, ns, db, buf.baseAddress, UInt(buf.count), &out)
        }
        guard status == 0, let opened = out else { throw LoomSql.lastError() }
        batch = opened
    }

    /// Begin a batch over an **encrypted** loom with a host-supplied 256-bit `kek`. `kek` must be 32
    /// bytes.
    public init(path: String, workspace ns: String, db: String, kek: [UInt8]) throws {
        var out: OpaquePointer?
        let status = kek.withUnsafeBufferPointer { buf in
            loom_sql_batch_begin_with_kek(path, ns, db, buf.baseAddress, UInt(buf.count), &out)
        }
        guard status == 0, let opened = out else { throw LoomSql.lastError() }
        batch = opened
    }

    public init(path: String, workspace ns: String, db: String,
                authPrincipal: String, authPassphrase: String) throws {
        var out: OpaquePointer?
        let auth = Array(authPassphrase.utf8)
        let status = auth.withUnsafeBufferPointer { authBuf in
            loom_sql_batch_begin_authenticated(path, ns, db, authPrincipal, authBuf.baseAddress,
                                               UInt(authBuf.count), &out)
        }
        guard status == 0, let opened = out else { throw LoomSql.lastError() }
        batch = opened
    }

    public init(path: String, workspace ns: String, db: String, passphrase: String,
                authPrincipal: String, authPassphrase: String) throws {
        var out: OpaquePointer?
        let pass = Array(passphrase.utf8)
        let auth = Array(authPassphrase.utf8)
        let status = pass.withUnsafeBufferPointer { passBuf in
            auth.withUnsafeBufferPointer { authBuf in
                loom_sql_batch_begin_keyed_authenticated(path, ns, db, passBuf.baseAddress,
                                                         UInt(passBuf.count), authPrincipal,
                                                         authBuf.baseAddress, UInt(authBuf.count), &out)
            }
        }
        guard status == 0, let opened = out else { throw LoomSql.lastError() }
        batch = opened
    }

    public init(path: String, workspace ns: String, db: String, kek: [UInt8],
                authPrincipal: String, authPassphrase: String) throws {
        var out: OpaquePointer?
        let auth = Array(authPassphrase.utf8)
        let status = kek.withUnsafeBufferPointer { kekBuf in
            auth.withUnsafeBufferPointer { authBuf in
                loom_sql_batch_begin_with_kek_authenticated(path, ns, db, kekBuf.baseAddress,
                                                            UInt(kekBuf.count), authPrincipal,
                                                            authBuf.baseAddress,
                                                            UInt(authBuf.count), &out)
            }
        }
        guard status == 0, let opened = out else { throw LoomSql.lastError() }
        batch = opened
    }

    deinit { loom_sql_batch_close(batch) }

    /// Run SQL in the batch (including `BEGIN`/`COMMIT`/`ROLLBACK`) and return a typed `LoomResult`.
    public func exec(_ sql: String) throws -> LoomResult {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        guard loom_sql_batch_exec(batch, sql, &ptr, &len) == 0 else { throw LoomSql.lastError() }
        var view: OpaquePointer?
        let opened = loom_result_open(ptr, len, &view)
        loom_bytes_free(ptr, len)
        guard opened == 0, let view else { throw LoomSql.lastError() }
        return LoomResult(view: view)
    }

    /// Run SQL in the batch; returns the result payloads as canonical CBOR bytes.
    public func execBytes(_ sql: String) throws -> [UInt8] {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        guard loom_sql_batch_exec(batch, sql, &ptr, &len) == 0 else { throw LoomSql.lastError() }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return [] }
        return Array(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }

    /// Make the batch's changes durable with one atomic save (no history entry). Rejected while an SQL
    /// transaction is open. The batch stays open.
    public func commit() throws {
        guard loom_sql_batch_commit(batch) == 0 else { throw LoomSql.lastError() }
    }

    /// Like `commit`, but also records a VCS commit; returns its content address. Distinct from a SQL
    /// `COMMIT`. Rejected while an SQL transaction is open.
    public func commitVcs(message: String, author: String) throws -> String {
        var out: UnsafeMutablePointer<CChar>?
        guard loom_sql_batch_commit_vcs(batch, message, author, &out) == 0 else {
            throw LoomSql.lastError()
        }
        guard let out else { return "" }
        defer { loom_string_free(out) }
        return String(cString: out)
    }

    /// Discard un-persisted in-memory changes (and any open SQL transaction); the batch stays open.
    public func abort() throws {
        guard loom_sql_batch_abort(batch) == 0 else { throw LoomSql.lastError() }
    }

    /// Release the write lock and free the batch. Closing without a commit discards un-persisted changes.
    public func close() {
        loom_sql_batch_close(batch)
        batch = nil
    }
}

/// One decoded result cell - a faithful view over the C `LoomValue`. Only the accessors the `tag`
/// selects are meaningful (`LOOM_VALUE_*`); `text`/`bytes` (and a `LIST`/`MAP` cell's canonical CBOR)
/// are copied out of the owning `LoomResult`.
public struct LoomCell {
    public let raw: LoomValue

    public var tag: Int32 { raw.tag }
    public var isNull: Bool { raw.tag == LOOM_VALUE_NULL }
    /// Signed payload: `Bool` (0/1), `Int`/`I8`/`I16`/`I32`, `Date`, `Timestamp`, `Interval.months`.
    public var int64: Int64 { raw.int_val }
    /// Secondary signed payload: `Interval.micros`.
    public var int64Secondary: Int64 { raw.int_val2 }
    /// Unsigned payload: `U8`/`U16`/`U32`/`U64`, `Time`, the `Inet` family (4 or 6).
    public var uint64: UInt64 { raw.uint_val }
    /// Float payload (convenience): `Float`, `F32`, `Point.x`. See `bits` for the exact IEEE-754.
    public var double: Double { raw.float_val }
    /// `Point.y` (convenience). See `bitsSecondary` for the exact IEEE-754.
    public var doubleSecondary: Double { raw.float_val2 }
    /// Raw IEEE-754 bits of the float payload (`Float`/`F32`/`Point.x`).
    public var bits: UInt64 { raw.bits }
    /// Raw IEEE-754 bits of `Point.y`.
    public var bitsSecondary: UInt64 { raw.bits2 }
    /// Decimal scale (with the 16-byte little-endian mantissa from `bytes16`).
    public var scale: UInt32 { raw.scale }
    /// 16-byte little-endian payload: `I128`/`U128`, `Uuid`, the decimal mantissa, or `Inet` octets.
    public var bytes16: [UInt8] { withUnsafeBytes(of: raw.bytes16) { Array($0) } }
    /// UTF-8 text payload (`Text`).
    public var text: String {
        guard let p = raw.data, raw.data_len > 0 else { return "" }
        return String(bytes: UnsafeBufferPointer(start: p, count: Int(raw.data_len)), encoding: .utf8)
            ?? ""
    }
    /// Raw byte payload: `Bytes`, or the canonical CBOR of a `LIST`/`MAP` cell.
    public var bytes: [UInt8] {
        guard let p = raw.data, raw.data_len > 0 else { return [] }
        return Array(UnsafeBufferPointer(start: p, count: Int(raw.data_len)))
    }
}

/// A decoded, immutable, indexed result (RAII over `LoomResultView`). Built by `LoomSql.exec`; navigate
/// with the indexed accessors (mirroring the C result-view ABI) and read cells as `LoomCell`. One
/// decoder backs every C-ABI binding, so no CBOR is parsed here.
public final class LoomResult {
    private let view: OpaquePointer

    init(view: OpaquePointer) { self.view = view }
    deinit { loom_result_close(view) }

    /// Number of items (SQL statements, or 1 for a reader result).
    public var count: Int { Int(loom_result_len(view)) }
    /// True if this result is a list of SQL statements (vs a single reader result).
    public var isStatements: Bool { loom_result_is_statements(view) == 1 }
    /// The kind of item `item` (a `LOOM_RESULT_*` value).
    public func itemKind(_ item: Int) -> Int32 { loom_result_item_kind(view, UInt(item)) }

    public func columnCount(_ item: Int) -> Int { Int(loom_result_column_count(view, UInt(item))) }
    public func columnName(_ item: Int, _ col: Int) throws -> String {
        var p: UnsafePointer<UInt8>?
        var l: UInt = 0
        guard loom_result_column_name(view, UInt(item), UInt(col), &p, &l) == 0 else {
            throw LoomSql.lastError()
        }
        return LoomResult.string(p, l)
    }
    public func columnType(_ item: Int, _ col: Int) throws -> String {
        var p: UnsafePointer<UInt8>?
        var l: UInt = 0
        guard loom_result_column_type(view, UInt(item), UInt(col), &p, &l) == 0 else {
            throw LoomSql.lastError()
        }
        return LoomResult.string(p, l)
    }

    public func rowCount(_ item: Int) -> Int { Int(loom_result_row_count(view, UInt(item))) }
    public func rowLen(_ item: Int, _ row: Int) -> Int {
        Int(loom_result_row_len(view, UInt(item), UInt(row)))
    }
    public func cell(_ item: Int, _ row: Int, _ col: Int) throws -> LoomCell {
        var v = LoomValue()
        guard loom_result_cell(view, UInt(item), UInt(row), UInt(col), &v) == 0 else {
            throw LoomSql.lastError()
        }
        return LoomCell(raw: v)
    }

    /// The rows of item `item` (default 0) as arrays of typed `LoomCell`s - the idiomatic
    /// `for row in try result.rows() { ... }` form (over the
    /// already-decoded typed result).
    public func rows(_ item: Int = 0) throws -> [[LoomCell]] {
        let count = rowCount(item)
        var out: [[LoomCell]] = []
        out.reserveCapacity(count)
        for r in 0..<count {
            let n = rowLen(item, r)
            var row: [LoomCell] = []
            row.reserveCapacity(n)
            for c in 0..<n {
                row.append(try cell(item, r, c))
            }
            out.append(row)
        }
        return out
    }

    /// Row count of an Insert/Delete/Update/DropTable item.
    public func count(_ item: Int) throws -> UInt64 {
        var n: UInt64 = 0
        guard loom_result_count(view, UInt(item), &n) == 0 else { throw LoomSql.lastError() }
        return n
    }

    public func stringCount(_ item: Int) -> Int { Int(loom_result_string_count(view, UInt(item))) }
    public func string(_ item: Int, _ i: Int) throws -> String {
        var p: UnsafePointer<UInt8>?
        var l: UInt = 0
        guard loom_result_string(view, UInt(item), UInt(i), &p, &l) == 0 else {
            throw LoomSql.lastError()
        }
        return LoomResult.string(p, l)
    }
    /// ShowVariable variable kind (`LOOM_VARIABLE_*`).
    public func variableKind(_ item: Int) throws -> Int32 {
        var k: Int32 = 0
        guard loom_result_variable_kind(view, UInt(item), &k) == 0 else { throw LoomSql.lastError() }
        return k
    }

    /// Commit address of blame row `row`.
    public func rowCommit(_ item: Int, _ row: Int) throws -> String {
        var p: UnsafePointer<UInt8>?
        var l: UInt = 0
        guard loom_result_row_commit(view, UInt(item), UInt(row), &p, &l) == 0 else {
            throw LoomSql.lastError()
        }
        return LoomResult.string(p, l)
    }

    public func diffCount(_ item: Int) -> Int { Int(loom_result_diff_count(view, UInt(item))) }
    /// Diff change kind (`LOOM_DIFF_*`).
    public func diffChange(_ item: Int, _ entry: Int) throws -> Int32 {
        var c: Int32 = 0
        guard loom_result_diff_change(view, UInt(item), UInt(entry), &c) == 0 else {
            throw LoomSql.lastError()
        }
        return c
    }
    public func diffLen(_ item: Int, _ entry: Int, _ side: Int32) -> Int {
        Int(loom_result_diff_len(view, UInt(item), UInt(entry), side))
    }
    public func diffCell(_ item: Int, _ entry: Int, _ side: Int32, _ col: Int) throws -> LoomCell {
        var v = LoomValue()
        guard loom_result_diff_cell(view, UInt(item), UInt(entry), side, UInt(col), &v) == 0 else {
            throw LoomSql.lastError()
        }
        return LoomCell(raw: v)
    }

    /// Merge outcome (`LOOM_MERGE_*`).
    public func mergeOutcome(_ item: Int) throws -> Int32 {
        var o: Int32 = 0
        guard loom_result_merge_outcome(view, UInt(item), &o) == 0 else { throw LoomSql.lastError() }
        return o
    }

    public func mapLen(_ item: Int, _ row: Int) -> Int {
        Int(loom_result_map_len(view, UInt(item), UInt(row)))
    }
    public func mapEntry(_ item: Int, _ row: Int, _ idx: Int) throws -> (key: String, value: LoomCell) {
        var p: UnsafePointer<UInt8>?
        var l: UInt = 0
        var v = LoomValue()
        guard loom_result_map_entry(view, UInt(item), UInt(row), UInt(idx), &p, &l, &v) == 0 else {
            throw LoomSql.lastError()
        }
        return (LoomResult.string(p, l), LoomCell(raw: v))
    }

    private static func string(_ p: UnsafePointer<UInt8>?, _ l: UInt) -> String {
        guard let p, l > 0 else { return "" }
        return String(bytes: UnsafeBufferPointer(start: p, count: Int(l)), encoding: .utf8) ?? ""
    }
}

/// A lazy, forward stream of a `SELECT`'s rows: RAII over the C
/// `LoomIter`, it pulls one row at a time and decodes it via `loom_row_open`, so a large result is
/// never materialized. Each `next()` yields a one-row `LoomResult` whose row (item 0, row 0) carries
/// the cells: `while let row = try stream.next() { let c = try row.cell(0, 0, 0) }`.
public final class LoomRowStream {
    private var iter: OpaquePointer?

    init(iter: OpaquePointer) { self.iter = iter }
    deinit { loom_iter_free(iter) }

    /// The next row as a one-row `LoomResult` (read cells with `cell(0, 0, col)`), or `nil` at the end.
    public func next() throws -> LoomResult? {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        var done: Int32 = 0
        guard loom_iter_next(iter, &ptr, &len, &done) == 0 else { throw LoomSql.lastError() }
        if done == 1 {
            return nil
        }
        var view: OpaquePointer?
        let status = loom_row_open(ptr, len, &view)
        loom_bytes_free(ptr, len)
        guard status == 0, let view else { throw LoomSql.lastError() }
        return LoomResult(view: view)
    }

    /// Release the iterator (also done at deinit).
    public func close() {
        loom_iter_free(iter)
        iter = nil
    }
}
