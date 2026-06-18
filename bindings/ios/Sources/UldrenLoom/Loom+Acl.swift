import CUldrenLoom
import Foundation

public struct LoomAclScope {
    public let kind: Int32
    public let prefix: Data

    public init(kind: Int32, prefix: Data) {
        self.kind = kind
        self.prefix = prefix
    }

    public static func ref(_ prefix: String) -> LoomAclScope {
        LoomAclScope(kind: 0, prefix: Data(prefix.utf8))
    }

    public static func collection(_ prefix: String) -> LoomAclScope {
        LoomAclScope(kind: 1, prefix: Data(prefix.utf8))
    }

    public static func path(_ prefix: String) -> LoomAclScope {
        LoomAclScope(kind: 2, prefix: Data(prefix.utf8))
    }

    public static func key(_ prefix: Data) -> LoomAclScope {
        LoomAclScope(kind: 3, prefix: prefix)
    }

    public static func table(_ prefix: String) -> LoomAclScope {
        LoomAclScope(kind: 4, prefix: Data(prefix.utf8))
    }

    public static func exec(_ prefix: String) -> LoomAclScope {
        LoomAclScope(kind: 5, prefix: Data(prefix.utf8))
    }
}

extension Loom {
    public func aclListJson() throws -> String {
        var out: UnsafeMutablePointer<CChar>?
        let status = loom_acl_list_json(session, &out)
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_string_free(out) }
        return out.map { String(cString: $0) } ?? "[]"
    }

    public func aclGrant(effect: Int32, subject: String, rightsMask: UInt32,
                         workspace: String? = nil, domain: String? = nil) throws {
        let status = loom_acl_grant(session, effect, subject, workspace, domain, rightsMask)
        guard status == 0 else { throw LoomSql.lastError() }
    }

    public func aclGrantScoped(effect: Int32, subject: String, rightsMask: UInt32,
                               workspace: String? = nil, domain: String? = nil,
                               refGlob: String? = nil, scopes: [LoomAclScope] = []) throws {
        let status = withAclScopeArrays(scopes) { kinds, prefixes, lengths in
            loom_acl_grant_scoped(
                session,
                effect,
                subject,
                workspace,
                domain,
                rightsMask,
                refGlob,
                UInt(scopes.count),
                kinds,
                prefixes,
                lengths)
        }
        guard status == 0 else { throw LoomSql.lastError() }
    }

    public func aclGrantScopedPredicate(effect: Int32, subject: String, rightsMask: UInt32,
                                        workspace: String? = nil, domain: String? = nil,
                                        refGlob: String? = nil, scopes: [LoomAclScope] = [],
                                        predicateCel: String? = nil) throws {
        let status = withAclScopeArrays(scopes) { kinds, prefixes, lengths in
            loom_acl_grant_scoped_predicate(
                session,
                effect,
                subject,
                workspace,
                domain,
                rightsMask,
                refGlob,
                UInt(scopes.count),
                kinds,
                prefixes,
                lengths,
                predicateCel == nil ? nil : "cel",
                predicateCel)
        }
        guard status == 0 else { throw LoomSql.lastError() }
    }

    public func aclRevoke(effect: Int32, subject: String, rightsMask: UInt32,
                          workspace: String? = nil, domain: String? = nil) throws -> Bool {
        var removed: Int32 = 0
        let status = loom_acl_revoke(session, effect, subject, workspace, domain, rightsMask, &removed)
        guard status == 0 else { throw LoomSql.lastError() }
        return removed != 0
    }

    public func aclRevokeScoped(effect: Int32, subject: String, rightsMask: UInt32,
                                workspace: String? = nil, domain: String? = nil,
                                refGlob: String? = nil, scopes: [LoomAclScope] = []) throws -> Bool {
        var removed: Int32 = 0
        let status = withAclScopeArrays(scopes) { kinds, prefixes, lengths in
            loom_acl_revoke_scoped(
                session,
                effect,
                subject,
                workspace,
                domain,
                rightsMask,
                refGlob,
                UInt(scopes.count),
                kinds,
                prefixes,
                lengths,
                &removed)
        }
        guard status == 0 else { throw LoomSql.lastError() }
        return removed != 0
    }

    public func aclRevokeScopedPredicate(effect: Int32, subject: String, rightsMask: UInt32,
                                         workspace: String? = nil, domain: String? = nil,
                                         refGlob: String? = nil, scopes: [LoomAclScope] = [],
                                         predicateCel: String? = nil) throws -> Bool {
        var removed: Int32 = 0
        let status = withAclScopeArrays(scopes) { kinds, prefixes, lengths in
            loom_acl_revoke_scoped_predicate(
                session,
                effect,
                subject,
                workspace,
                domain,
                rightsMask,
                refGlob,
                UInt(scopes.count),
                kinds,
                prefixes,
                lengths,
                predicateCel == nil ? nil : "cel",
                predicateCel,
                &removed)
        }
        guard status == 0 else { throw LoomSql.lastError() }
        return removed != 0
    }

    public func protectedRefListJson(workspace: String) throws -> String {
        var out: UnsafeMutablePointer<CChar>?
        let status = loom_protected_ref_list_json(session, workspace, &out)
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_string_free(out) }
        return out.map { String(cString: $0) } ?? "[]"
    }

    public func protectedRefGetJson(workspace: String, refName: String) throws -> String {
        var out: UnsafeMutablePointer<CChar>?
        let status = loom_protected_ref_get_json(session, workspace, refName, &out)
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_string_free(out) }
        return out.map { String(cString: $0) } ?? "null"
    }

    public func protectedRefSet(workspace: String, refName: String,
                                fastForwardOnly: Bool,
                                signedCommitsRequired: Bool,
                                signedRefAdvanceRequired: Bool,
                                requiredReviewCount: UInt32,
                                retentionLock: Bool,
                                governanceLock: Bool) throws {
        let status = loom_protected_ref_set(
            session,
            workspace,
            refName,
            fastForwardOnly,
            signedCommitsRequired,
            signedRefAdvanceRequired,
            requiredReviewCount,
            retentionLock,
            governanceLock)
        guard status == 0 else { throw LoomSql.lastError() }
    }

    public func protectedRefRemove(workspace: String, refName: String) throws -> Bool {
        var removed: Int32 = 0
        let status = loom_protected_ref_remove(session, workspace, refName, &removed)
        guard status == 0 else { throw LoomSql.lastError() }
        return removed != 0
    }
}

private func withAclScopeArrays<R>(
    _ scopes: [LoomAclScope],
    _ body: (
        UnsafePointer<Int32>?,
        UnsafePointer<UnsafePointer<UInt8>?>?,
        UnsafePointer<UInt>?
    ) -> R
) -> R {
    if scopes.isEmpty {
        return body(nil, nil, nil)
    }
    var buffers: [UnsafeMutablePointer<UInt8>] = []
    defer {
        for buffer in buffers {
            buffer.deallocate()
        }
    }
    var kinds: [Int32] = []
    var lengths: [UInt] = []
    var pointers: [UnsafePointer<UInt8>?] = []
    for scope in scopes {
        kinds.append(scope.kind)
        lengths.append(UInt(scope.prefix.count))
        let buffer = UnsafeMutablePointer<UInt8>.allocate(capacity: max(scope.prefix.count, 1))
        scope.prefix.withUnsafeBytes { raw in
            if let base = raw.bindMemory(to: UInt8.self).baseAddress, scope.prefix.count > 0 {
                buffer.initialize(from: base, count: scope.prefix.count)
            }
        }
        buffers.append(buffer)
        pointers.append(UnsafePointer(buffer))
    }
    return kinds.withUnsafeBufferPointer { kindPtr in
        pointers.withUnsafeBufferPointer { pointerPtr in
            lengths.withUnsafeBufferPointer { lengthPtr in
                body(kindPtr.baseAddress, pointerPtr.baseAddress, lengthPtr.baseAddress)
            }
        }
    }
}
