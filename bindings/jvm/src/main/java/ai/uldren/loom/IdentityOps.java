package ai.uldren.loom;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.ValueLayout;
import java.nio.charset.StandardCharsets;

/** Identity and ACL administration for a {@link LoomSession}. */
public final class IdentityOps {
    private final LoomSession session;

    public record AclScope(int kind, byte[] prefix) {
        public static AclScope ref(String prefix) {
            return new AclScope(0, prefix.getBytes(StandardCharsets.UTF_8));
        }

        public static AclScope collection(String prefix) {
            return new AclScope(1, prefix.getBytes(StandardCharsets.UTF_8));
        }

        public static AclScope path(String prefix) {
            return new AclScope(2, prefix.getBytes(StandardCharsets.UTF_8));
        }

        public static AclScope key(byte[] prefix) {
            return new AclScope(3, prefix);
        }

        public static AclScope table(String prefix) {
            return new AclScope(4, prefix.getBytes(StandardCharsets.UTF_8));
        }

        public static AclScope exec(String prefix) {
            return new AclScope(5, prefix.getBytes(StandardCharsets.UTF_8));
        }
    }

    IdentityOps(LoomSession session) {
        this.session = session;
    }

    public void authenticatePassphrase(String principal, String principalPassphrase) {
        byte[] pass = principalPassphrase.getBytes(StandardCharsets.UTF_8);
        Loom.onHandle(session.path, session.passphraseBytes(), null, "loom_authenticate_passphrase",
                (arena, handle) -> {
                    MemorySegment passSeg = arena.allocate(Math.max(pass.length, 1));
                    MemorySegment.copy(pass, 0, passSeg, ValueLayout.JAVA_BYTE, 0, pass.length);
                    int status = (int) Loom.LOOM_AUTHENTICATE_PASSPHRASE.invokeExact(handle,
                            arena.allocateFrom(principal), passSeg, (long) pass.length);
                    if (status != 0) {
                        throw Loom.lastError("loom_authenticate_passphrase");
                    }
                    session.setAuthentication(principal, principalPassphrase);
                    return null;
                });
    }

    public String listJson() {
        return session.onHandle("loom_identity_list_json",
                (arena, handle) -> {
                    MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
                    int status = (int) Loom.LOOM_IDENTITY_LIST_JSON.invokeExact(handle, out);
                    if (status != 0) {
                        throw Loom.lastError("loom_identity_list_json");
                    }
                    return Loom.takeOwnedString(out.get(ValueLayout.ADDRESS, 0));
                });
    }

    public String addPrincipal(String principalHandle, String name, String kind) {
        return session.onHandle("loom_identity_add_principal", (arena, sessionHandle) -> {
                    MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
                    int status = (int) Loom.LOOM_IDENTITY_ADD_PRINCIPAL.invokeExact(sessionHandle,
                            arena.allocateFrom(principalHandle), arena.allocateFrom(name),
                            arena.allocateFrom(kind), out);
                    if (status != 0) {
                        throw Loom.lastError("loom_identity_add_principal");
                    }
                    return Loom.takeOwnedString(out.get(ValueLayout.ADDRESS, 0));
                });
    }

    public void renamePrincipalHandle(String principal, String handle) {
        session.onHandle("loom_identity_rename_principal_handle", (arena, sessionHandle) -> {
            int status = (int) Loom.LOOM_IDENTITY_RENAME_PRINCIPAL_HANDLE.invokeExact(
                    sessionHandle, arena.allocateFrom(principal), arena.allocateFrom(handle));
            if (status != 0) {
                throw Loom.lastError("loom_identity_rename_principal_handle");
            }
            return null;
        });
    }

    public void setPassphrase(String principal, String principalPassphrase) {
        byte[] pass = principalPassphrase.getBytes(StandardCharsets.UTF_8);
        session.onHandle("loom_identity_set_passphrase",
                (arena, handle) -> {
                    MemorySegment passSeg = arena.allocate(Math.max(pass.length, 1));
                    MemorySegment.copy(pass, 0, passSeg, ValueLayout.JAVA_BYTE, 0, pass.length);
                    int status = (int) Loom.LOOM_IDENTITY_SET_PASSPHRASE.invokeExact(handle,
                            arena.allocateFrom(principal), passSeg, (long) pass.length);
                    if (status != 0) {
                        throw Loom.lastError("loom_identity_set_passphrase");
                    }
                    return null;
                });
    }

    public void removePrincipal(String principal) {
        session.onHandle("loom_identity_remove_principal", (arena, handle) -> {
                    int status = (int) Loom.LOOM_IDENTITY_REMOVE_PRINCIPAL.invokeExact(handle,
                            arena.allocateFrom(principal));
                    if (status != 0) {
                        throw Loom.lastError("loom_identity_remove_principal");
                    }
                    return null;
                });
    }

    public void assignRole(String principal, String role) {
        session.onHandle("loom_identity_assign_role", (arena, handle) -> {
                    int status = (int) Loom.LOOM_IDENTITY_ASSIGN_ROLE.invokeExact(handle,
                            arena.allocateFrom(principal), arena.allocateFrom(role));
                    if (status != 0) {
                        throw Loom.lastError("loom_identity_assign_role");
                    }
                    return null;
                });
    }

    public boolean revokeRole(String principal, String role) {
        return session.onHandle("loom_identity_revoke_role", (arena, handle) -> {
                    MemorySegment removed = arena.allocate(ValueLayout.JAVA_INT);
                    int status = (int) Loom.LOOM_IDENTITY_REVOKE_ROLE.invokeExact(handle,
                            arena.allocateFrom(principal), arena.allocateFrom(role), removed);
                    if (status != 0) {
                        throw Loom.lastError("loom_identity_revoke_role");
                    }
                    return removed.get(ValueLayout.JAVA_INT, 0) != 0;
                });
    }

    public String createExternalCredential(String principal, String kind, String label,
                                           String issuer, String subject, String materialDigest) {
        return session.onHandle("loom_identity_create_external_credential", (arena, handle) -> {
                    MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment digest = materialDigest == null
                            ? MemorySegment.NULL
                            : arena.allocateFrom(materialDigest);
                    int status = (int) Loom.LOOM_IDENTITY_CREATE_EXTERNAL_CREDENTIAL.invokeExact(
                            handle, arena.allocateFrom(principal), arena.allocateFrom(kind),
                            arena.allocateFrom(label), arena.allocateFrom(issuer),
                            arena.allocateFrom(subject), digest, out);
                    if (status != 0) {
                        throw Loom.lastError("loom_identity_create_external_credential");
                    }
                    return Loom.takeOwnedString(out.get(ValueLayout.ADDRESS, 0));
                });
    }

    public void revokeExternalCredential(String credential) {
        session.onHandle("loom_identity_revoke_external_credential", (arena, handle) -> {
                    int status = (int) Loom.LOOM_IDENTITY_REVOKE_EXTERNAL_CREDENTIAL.invokeExact(
                            handle, arena.allocateFrom(credential));
                    if (status != 0) {
                        throw Loom.lastError("loom_identity_revoke_external_credential");
                    }
                    return null;
                });
    }

    public String addPublicKey(String principal, String label, String algorithm, String publicKeyHex) {
        return session.onHandle("loom_identity_add_public_key", (arena, handle) -> {
                    MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
                    int status = (int) Loom.LOOM_IDENTITY_ADD_PUBLIC_KEY.invokeExact(
                            handle, arena.allocateFrom(principal), arena.allocateFrom(label),
                            arena.allocateFrom(algorithm), arena.allocateFrom(publicKeyHex), out);
                    if (status != 0) {
                        throw Loom.lastError("loom_identity_add_public_key");
                    }
                    return Loom.takeOwnedString(out.get(ValueLayout.ADDRESS, 0));
                });
    }

    public void revokePublicKey(String key) {
        session.onHandle("loom_identity_revoke_public_key", (arena, handle) -> {
                    int status = (int) Loom.LOOM_IDENTITY_REVOKE_PUBLIC_KEY.invokeExact(
                            handle, arena.allocateFrom(key));
                    if (status != 0) {
                        throw Loom.lastError("loom_identity_revoke_public_key");
                    }
                    return null;
                });
    }

    public String aclListJson() {
        return session.onHandle("loom_acl_list_json",
                (arena, handle) -> {
                    MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
                    int status = (int) Loom.LOOM_ACL_LIST_JSON.invokeExact(handle, out);
                    if (status != 0) {
                        throw Loom.lastError("loom_acl_list_json");
                    }
                    return Loom.takeOwnedString(out.get(ValueLayout.ADDRESS, 0));
                });
    }

    public void aclGrant(int effect, String subject, String workspace, String domain, int rightsMask) {
        session.onHandle("loom_acl_grant",
                (arena, handle) -> {
                    MemorySegment nsSeg = workspace != null ? arena.allocateFrom(workspace) : MemorySegment.NULL;
                    MemorySegment domainSeg = domain != null ? arena.allocateFrom(domain) : MemorySegment.NULL;
                    int status = (int) Loom.LOOM_ACL_GRANT.invokeExact(handle, effect,
                            arena.allocateFrom(subject), nsSeg, domainSeg, rightsMask);
                    if (status != 0) {
                        throw Loom.lastError("loom_acl_grant");
                    }
                    return null;
                });
    }

    public boolean aclRevoke(int effect, String subject, String workspace, String domain, int rightsMask) {
        return session.onHandle("loom_acl_revoke",
                (arena, handle) -> {
                    MemorySegment nsSeg = workspace != null ? arena.allocateFrom(workspace) : MemorySegment.NULL;
                    MemorySegment domainSeg = domain != null ? arena.allocateFrom(domain) : MemorySegment.NULL;
                    MemorySegment removed = arena.allocate(ValueLayout.JAVA_INT);
                    int status = (int) Loom.LOOM_ACL_REVOKE.invokeExact(handle, effect,
                            arena.allocateFrom(subject), nsSeg, domainSeg, rightsMask, removed);
                    if (status != 0) {
                        throw Loom.lastError("loom_acl_revoke");
                    }
                    return removed.get(ValueLayout.JAVA_INT, 0) != 0;
                });
    }

    public void aclGrantScoped(int effect, String subject, String workspace, String domain, int rightsMask,
            String refGlob, AclScope[] scopes) {
        session.onHandle("loom_acl_grant_scoped",
                (arena, handle) -> {
                    MemorySegment nsSeg = workspace != null ? arena.allocateFrom(workspace) : MemorySegment.NULL;
                    MemorySegment domainSeg = domain != null ? arena.allocateFrom(domain) : MemorySegment.NULL;
                    MemorySegment refGlobSeg = refGlob != null ? arena.allocateFrom(refGlob) : MemorySegment.NULL;
                    ScopeSegments scopeSegments = ScopeSegments.allocate(arena, scopes);
                    int status = (int) Loom.LOOM_ACL_GRANT_SCOPED.invokeExact(handle, effect,
                            arena.allocateFrom(subject), nsSeg, domainSeg, rightsMask, refGlobSeg,
                            (long) scopeSegments.count, scopeSegments.kinds, scopeSegments.prefixes,
                            scopeSegments.lengths);
                    if (status != 0) {
                        throw Loom.lastError("loom_acl_grant_scoped");
                    }
                    return null;
                });
    }

    public void aclGrantScopedPredicate(int effect, String subject, String workspace, String domain, int rightsMask,
            String refGlob, AclScope[] scopes, String predicateCel) {
        session.onHandle("loom_acl_grant_scoped_predicate",
                (arena, handle) -> {
                    MemorySegment nsSeg = workspace != null ? arena.allocateFrom(workspace) : MemorySegment.NULL;
                    MemorySegment domainSeg = domain != null ? arena.allocateFrom(domain) : MemorySegment.NULL;
                    MemorySegment refGlobSeg = refGlob != null ? arena.allocateFrom(refGlob) : MemorySegment.NULL;
                    MemorySegment predicateLanguage = predicateCel != null ? arena.allocateFrom("cel") : MemorySegment.NULL;
                    MemorySegment predicateExpression = predicateCel != null ? arena.allocateFrom(predicateCel) : MemorySegment.NULL;
                    ScopeSegments scopeSegments = ScopeSegments.allocate(arena, scopes);
                    int status = (int) Loom.LOOM_ACL_GRANT_SCOPED_PREDICATE.invokeExact(handle, effect,
                            arena.allocateFrom(subject), nsSeg, domainSeg, rightsMask, refGlobSeg,
                            (long) scopeSegments.count, scopeSegments.kinds, scopeSegments.prefixes,
                            scopeSegments.lengths, predicateLanguage, predicateExpression);
                    if (status != 0) {
                        throw Loom.lastError("loom_acl_grant_scoped_predicate");
                    }
                    return null;
                });
    }

    public boolean aclRevokeScoped(int effect, String subject, String workspace, String domain, int rightsMask,
            String refGlob, AclScope[] scopes) {
        return session.onHandle("loom_acl_revoke_scoped",
                (arena, handle) -> {
                    MemorySegment nsSeg = workspace != null ? arena.allocateFrom(workspace) : MemorySegment.NULL;
                    MemorySegment domainSeg = domain != null ? arena.allocateFrom(domain) : MemorySegment.NULL;
                    MemorySegment refGlobSeg = refGlob != null ? arena.allocateFrom(refGlob) : MemorySegment.NULL;
                    ScopeSegments scopeSegments = ScopeSegments.allocate(arena, scopes);
                    MemorySegment removed = arena.allocate(ValueLayout.JAVA_INT);
                    int status = (int) Loom.LOOM_ACL_REVOKE_SCOPED.invokeExact(handle, effect,
                            arena.allocateFrom(subject), nsSeg, domainSeg, rightsMask, refGlobSeg,
                            (long) scopeSegments.count, scopeSegments.kinds, scopeSegments.prefixes,
                            scopeSegments.lengths, removed);
                    if (status != 0) {
                        throw Loom.lastError("loom_acl_revoke_scoped");
                    }
                    return removed.get(ValueLayout.JAVA_INT, 0) != 0;
                });
    }

    public boolean aclRevokeScopedPredicate(int effect, String subject, String workspace, String domain, int rightsMask,
            String refGlob, AclScope[] scopes, String predicateCel) {
        return session.onHandle("loom_acl_revoke_scoped_predicate",
                (arena, handle) -> {
                    MemorySegment nsSeg = workspace != null ? arena.allocateFrom(workspace) : MemorySegment.NULL;
                    MemorySegment domainSeg = domain != null ? arena.allocateFrom(domain) : MemorySegment.NULL;
                    MemorySegment refGlobSeg = refGlob != null ? arena.allocateFrom(refGlob) : MemorySegment.NULL;
                    MemorySegment predicateLanguage = predicateCel != null ? arena.allocateFrom("cel") : MemorySegment.NULL;
                    MemorySegment predicateExpression = predicateCel != null ? arena.allocateFrom(predicateCel) : MemorySegment.NULL;
                    ScopeSegments scopeSegments = ScopeSegments.allocate(arena, scopes);
                    MemorySegment removed = arena.allocate(ValueLayout.JAVA_INT);
                    int status = (int) Loom.LOOM_ACL_REVOKE_SCOPED_PREDICATE.invokeExact(handle, effect,
                            arena.allocateFrom(subject), nsSeg, domainSeg, rightsMask, refGlobSeg,
                            (long) scopeSegments.count, scopeSegments.kinds, scopeSegments.prefixes,
                            scopeSegments.lengths, predicateLanguage, predicateExpression, removed);
                    if (status != 0) {
                        throw Loom.lastError("loom_acl_revoke_scoped_predicate");
                    }
                    return removed.get(ValueLayout.JAVA_INT, 0) != 0;
                });
    }

    public String protectedRefListJson(String workspace) {
        return session.onHandle("loom_protected_ref_list_json",
                (arena, handle) -> {
                    MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
                    int status = (int) Loom.LOOM_PROTECTED_REF_LIST_JSON.invokeExact(handle,
                            arena.allocateFrom(workspace), out);
                    if (status != 0) {
                        throw Loom.lastError("loom_protected_ref_list_json");
                    }
                    return Loom.takeOwnedString(out.get(ValueLayout.ADDRESS, 0));
                });
    }

    public String protectedRefGetJson(String workspace, String refName) {
        return session.onHandle("loom_protected_ref_get_json",
                (arena, handle) -> {
                    MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
                    int status = (int) Loom.LOOM_PROTECTED_REF_GET_JSON.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(refName), out);
                    if (status != 0) {
                        throw Loom.lastError("loom_protected_ref_get_json");
                    }
                    return Loom.takeOwnedString(out.get(ValueLayout.ADDRESS, 0));
                });
    }

    public void protectedRefSet(String workspace, String refName, boolean fastForwardOnly,
            boolean signedCommitsRequired, boolean signedRefAdvanceRequired, int requiredReviewCount,
            boolean retentionLock, boolean governanceLock) {
        session.onHandle("loom_protected_ref_set",
                (arena, handle) -> {
                    int status = (int) Loom.LOOM_PROTECTED_REF_SET.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(refName), fastForwardOnly,
                            signedCommitsRequired, signedRefAdvanceRequired, requiredReviewCount,
                            retentionLock, governanceLock);
                    if (status != 0) {
                        throw Loom.lastError("loom_protected_ref_set");
                    }
                    return null;
                });
    }

    public boolean protectedRefRemove(String workspace, String refName) {
        return session.onHandle("loom_protected_ref_remove",
                (arena, handle) -> {
                    MemorySegment removed = arena.allocate(ValueLayout.JAVA_INT);
                    int status = (int) Loom.LOOM_PROTECTED_REF_REMOVE.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(refName), removed);
                    if (status != 0) {
                        throw Loom.lastError("loom_protected_ref_remove");
                    }
                    return removed.get(ValueLayout.JAVA_INT, 0) != 0;
                });
    }

    private record ScopeSegments(int count, MemorySegment kinds, MemorySegment prefixes, MemorySegment lengths) {
        static ScopeSegments allocate(Arena arena, AclScope[] scopes) {
            int count = scopes != null ? scopes.length : 0;
            if (count == 0) {
                return new ScopeSegments(0, MemorySegment.NULL, MemorySegment.NULL, MemorySegment.NULL);
            }
            MemorySegment kinds = arena.allocate(ValueLayout.JAVA_INT, count);
            MemorySegment prefixes = arena.allocate(ValueLayout.ADDRESS, count);
            MemorySegment lengths = arena.allocate(ValueLayout.JAVA_LONG, count);
            for (int i = 0; i < count; i++) {
                AclScope scope = scopes[i];
                byte[] prefix = scope.prefix != null ? scope.prefix : new byte[0];
                MemorySegment prefixSeg = arena.allocate(Math.max(prefix.length, 1));
                MemorySegment.copy(prefix, 0, prefixSeg, ValueLayout.JAVA_BYTE, 0, prefix.length);
                kinds.setAtIndex(ValueLayout.JAVA_INT, i, scope.kind);
                prefixes.setAtIndex(ValueLayout.ADDRESS, i, prefixSeg);
                lengths.setAtIndex(ValueLayout.JAVA_LONG, i, prefix.length);
            }
            return new ScopeSegments(count, kinds, prefixes, lengths);
        }
    }
}
