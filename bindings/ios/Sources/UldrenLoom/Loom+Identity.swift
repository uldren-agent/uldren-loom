import CUldrenLoom
import Foundation

extension Loom {
    public func authenticatePassphrase(principal: String, passphrase: String) throws {
        let pass = Array(passphrase.utf8)
        let status = pass.withUnsafeBufferPointer { buf in
            loom_authenticate_passphrase(session, principal, buf.baseAddress, UInt(buf.count))
        }
        guard status == 0 else { throw LoomSql.lastError() }
    }

    public func clearAuthentication() throws {
        guard loom_clear_authentication(session) == 0 else { throw LoomSql.lastError() }
    }

    public func identityListJson() throws -> String {
        var out: UnsafeMutablePointer<CChar>?
        let status = loom_identity_list_json(session, &out)
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_string_free(out) }
        return out.map { String(cString: $0) } ?? "{}"
    }

    public func identityAddPrincipal(handle: String, name: String, kind: String = "user") throws -> String {
        var out: UnsafeMutablePointer<CChar>?
        let status = loom_identity_add_principal(session, handle, name, kind, &out)
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_string_free(out) }
        return out.map { String(cString: $0) } ?? ""
    }

    public func identityRenamePrincipalHandle(principal: String, handle: String) throws {
        guard loom_identity_rename_principal_handle(session, principal, handle) == 0 else {
            throw LoomSql.lastError()
        }
    }

    public func identitySetPassphrase(principal: String, passphrase: String) throws {
        let pass = Array(passphrase.utf8)
        let status = pass.withUnsafeBufferPointer { buf in
            loom_identity_set_passphrase(session, principal, buf.baseAddress, UInt(buf.count))
        }
        guard status == 0 else { throw LoomSql.lastError() }
    }

    public func identityRemovePrincipal(_ principal: String) throws {
        guard loom_identity_remove_principal(session, principal) == 0 else {
            throw LoomSql.lastError()
        }
    }

    public func identityAssignRole(principal: String, role: String) throws {
        guard loom_identity_assign_role(session, principal, role) == 0 else {
            throw LoomSql.lastError()
        }
    }

    public func identityRevokeRole(principal: String, role: String) throws -> Bool {
        var removed: Int32 = 0
        guard loom_identity_revoke_role(session, principal, role, &removed) == 0 else {
            throw LoomSql.lastError()
        }
        return removed != 0
    }

    public func identityCreateExternalCredential(
        principal: String,
        kind: String,
        label: String,
        issuer: String,
        subject: String,
        materialDigest: String? = nil
    ) throws -> String {
        var out: UnsafeMutablePointer<CChar>?
        let status: Int32
        if let materialDigest {
            status = loom_identity_create_external_credential(
                session,
                principal,
                kind,
                label,
                issuer,
                subject,
                materialDigest,
                &out
            )
        } else {
            status = loom_identity_create_external_credential(
                session,
                principal,
                kind,
                label,
                issuer,
                subject,
                nil,
                &out
            )
        }
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_string_free(out) }
        return out.map { String(cString: $0) } ?? ""
    }

    public func identityRevokeExternalCredential(_ credential: String) throws {
        guard loom_identity_revoke_external_credential(session, credential) == 0 else {
            throw LoomSql.lastError()
        }
    }

    public func identityAddPublicKey(
        principal: String,
        label: String,
        algorithm: String,
        publicKeyHex: String
    ) throws -> String {
        var out: UnsafeMutablePointer<CChar>?
        let status = loom_identity_add_public_key(session, principal, label, algorithm, publicKeyHex, &out)
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_string_free(out) }
        return out.map { String(cString: $0) } ?? ""
    }

    public func identityRevokePublicKey(_ key: String) throws {
        guard loom_identity_revoke_public_key(session, key) == 0 else {
            throw LoomSql.lastError()
        }
    }
}
