package ai.uldren.loom

/**
 * A handle to one `.loom` file: it holds the path (and the unlock key for an encrypted store) so facet
 * operations need not repeat them on every call. Construct it directly, or via the companion
 * factories [open] / [openEncrypted] / [authenticate].
 *
 * Facet operations are reached through grouped accessors, mirroring the JVM binding:
 *
 * ```
 * val s = LoomSession("/path/app.loom")
 * s.kv().put("notes", "drafts", key, value)
 * val v = s.kv().get("notes", "drafts", key)
 * ```
 *
 * `workspace` stays a per-call argument (it matches the C ABI and lets one session address many
 * workspaces). The Kotlin facet functions are uniform - every call takes a trailing
 * `passphrase`/`kek` - so the session simply forwards [passphrase] and [kek].
 *
 * Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.
 */
class LoomSession(
    val path: String,
    val passphrase: String? = null,
    val kek: ByteArray? = null,
) {
    internal var authPrincipal: String? = null
    internal var authPassphrase: String? = null

    /** Key-value facet operations (typed key/value maps per collection). */
    fun kv(): KvOps = KvOps(this)

    /** Content-addressed store operations (immutable blobs by digest). */
    fun cas(): CasOps = CasOps(this)

    /** Document facet operations (opaque documents by string id, per collection). */
    fun document(): DocumentOps = DocumentOps(this)

    /** Time-series facet operations (points by i64 timestamp, per collection). */
    fun timeSeries(): TimeSeriesOps = TimeSeriesOps(this)

    /** Native telemetry facet operations for metrics, logs, and traces. */
    fun telemetry(): TelemetryOps = TelemetryOps(this)

    /** Ledger facet operations (append-only hash-chained log, per collection). */
    fun ledger(): LedgerOps = LedgerOps(this)

    /** Append-log queue operations (append/get/range/len plus per-consumer offsets, per stream). */
    fun queue(): QueueOps = QueueOps(this)

    /** Version-control inspection (workspace/entry-level blame and diff). */
    fun vcs(): VcsOps = VcsOps(this)

    /** Calendar facet operations (CalDAV collections + entries, per principal). */
    fun calendar(): CalendarOps = CalendarOps(this)

    /** Contacts facet operations (address books + vCard entries, per principal). */
    fun contacts(): ContactsOps = ContactsOps(this)

    /** Mail facet operations (mailboxes + RFC 5322 messages, per principal). */
    fun mail(): MailOps = MailOps(this)

    /** SQL table inspection (read-only direct readers over the versioned tabular store). */
    fun tables(): SqlTableOps = SqlTableOps(this)

    /** Workspace administration (create / list / rename / delete workspaces in this loom). */
    fun workspaces(): WorkspaceOps = WorkspaceOps(this)

    /** Identity and ACL administration for this loom. */
    fun identity(): IdentityOps = IdentityOps(this)

    /** Property-graph facet operations (nodes/edges + traversal, per graph). */
    fun graph(): GraphOps = GraphOps(this)

    /** Vector-set facet operations (embeddings + metadata + nearest-neighbour search, per set). */
    fun vector(): VectorOps = VectorOps(this)

    /** Columnar-dataset facet operations (typed columns + append/scan/select, per dataset). */
    fun columnar(): ColumnarOps = ColumnarOps(this)

    /** Dataframe facet operations (plans + collect/preview/materialize, per frame). */
    fun dataframe(): DataframeOps = DataframeOps(this)

    /** Search facet operations (mapped fields + index/get/delete/query, per collection). */
    fun search(): SearchOps = SearchOps(this)

    /**
     * Open a SQL session over [workspace]'s SQL facet (created if absent), database [db]. The returned
     * [LoomSql] runs `exec`/`query`/`commit`. Unlocked with this session's key when encrypted.
     */
    fun sql(workspace: String, db: String): LoomSql = when {
        authPrincipal != null -> LoomSql(
            path,
            workspace,
            db,
            passphrase,
            kek,
            authPrincipal,
            authPassphrase,
        )
        kek != null -> LoomSql(path, workspace, db, kek)
        passphrase != null -> LoomSql(path, workspace, db, passphrase)
        else -> LoomSql(path, workspace, db)
    }

    companion object {
        /** Open a session over an unencrypted `.loom` at [path]. */
        fun open(path: String): LoomSession = LoomSession(path)

        /** Open a session over the encrypted `.loom` at [path], unlocked with [passphrase]. */
        fun openEncrypted(path: String, passphrase: String): LoomSession =
            LoomSession(path, passphrase = passphrase)

        /** Open and unlock a session in one call ([passphrase] may be null for an unencrypted store). */
        fun authenticate(path: String, passphrase: String?): LoomSession =
            LoomSession(path, passphrase = passphrase)
    }
}

/** Key-value facet operations for a [LoomSession]. */
class KvOps(private val s: LoomSession) {
    fun put(workspace: String, collection: String, key: ByteArray, value: ByteArray) =
        Loom.kvPut(s.path, workspace, collection, key, value, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun get(workspace: String, collection: String, key: ByteArray): ByteArray? =
        Loom.kvGet(s.path, workspace, collection, key, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun delete(workspace: String, collection: String, key: ByteArray): Boolean =
        Loom.kvDelete(s.path, workspace, collection, key, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun list(workspace: String, collection: String): ByteArray =
        Loom.kvList(s.path, workspace, collection, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun range(workspace: String, collection: String, lo: ByteArray, hi: ByteArray): ByteArray =
        Loom.kvRange(s.path, workspace, collection, lo, hi, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)
}

/** Identity and ACL administration for a [LoomSession]. */
class IdentityOps(private val s: LoomSession) {
    fun authenticatePassphrase(principal: String, principalPassphrase: String) {
        Loom.authenticatePassphrase(s.path, principal, principalPassphrase, s.passphrase, s.kek)
        s.authPrincipal = principal
        s.authPassphrase = principalPassphrase
    }

    fun clearAuthentication() {
        s.authPrincipal = null
        s.authPassphrase = null
    }

    fun listJson(): String =
        Loom.identityListJson(s.path, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun addPrincipal(handle: String, name: String, kind: String = "user"): String =
        Loom.identityAddPrincipal(s.path, handle, name, kind, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun renamePrincipalHandle(principal: String, handle: String) =
        Loom.identityRenamePrincipalHandle(
            s.path,
            principal,
            handle,
            s.passphrase,
            s.kek,
            s.authPrincipal,
            s.authPassphrase,
        )

    fun setPassphrase(principal: String, principalPassphrase: String) =
        Loom.identitySetPassphrase(
            s.path,
            principal,
            principalPassphrase,
            s.passphrase,
            s.kek,
            s.authPrincipal,
            s.authPassphrase,
        )

    fun removePrincipal(principal: String) =
        Loom.identityRemovePrincipal(s.path, principal, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun assignRole(principal: String, role: String) =
        Loom.identityAssignRole(s.path, principal, role, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun revokeRole(principal: String, role: String): Boolean =
        Loom.identityRevokeRole(s.path, principal, role, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun createExternalCredential(
        principal: String,
        kind: String,
        label: String,
        issuer: String,
        subject: String,
        materialDigest: String? = null,
    ): String =
        Loom.identityCreateExternalCredential(
            s.path,
            principal,
            kind,
            label,
            issuer,
            subject,
            materialDigest,
            s.passphrase,
            s.kek,
            s.authPrincipal,
            s.authPassphrase,
        )

    fun revokeExternalCredential(credential: String) =
        Loom.identityRevokeExternalCredential(
            s.path,
            credential,
            s.passphrase,
            s.kek,
            s.authPrincipal,
            s.authPassphrase,
        )

    fun aclListJson(): String =
        Loom.aclListJson(s.path, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun aclGrant(effect: Int, subject: String, workspace: String? = null, domain: String? = null, rightsMask: Int) =
        Loom.aclGrant(
            s.path,
            effect,
            subject,
            workspace,
            domain,
            rightsMask,
            s.passphrase,
            s.kek,
            s.authPrincipal,
            s.authPassphrase,
        )

    fun aclRevoke(effect: Int, subject: String, workspace: String? = null, domain: String? = null, rightsMask: Int): Boolean =
        Loom.aclRevoke(
            s.path,
            effect,
            subject,
            workspace,
            domain,
            rightsMask,
            s.passphrase,
            s.kek,
            s.authPrincipal,
            s.authPassphrase,
        )

    fun aclGrantScoped(
        effect: Int,
        subject: String,
        workspace: String? = null,
        domain: String? = null,
        rightsMask: Int,
        refGlob: String? = null,
        scopes: List<AclScope> = emptyList(),
    ) = Loom.aclGrantScoped(
        s.path,
        effect,
        subject,
        workspace,
        domain,
        rightsMask,
        refGlob,
        scopes,
        s.passphrase,
        s.kek,
        s.authPrincipal,
        s.authPassphrase,
    )

    fun aclGrantScopedPredicate(
        effect: Int,
        subject: String,
        workspace: String? = null,
        domain: String? = null,
        rightsMask: Int,
        refGlob: String? = null,
        scopes: List<AclScope> = emptyList(),
        predicateCel: String? = null,
    ) = Loom.aclGrantScopedPredicate(
        s.path,
        effect,
        subject,
        workspace,
        domain,
        rightsMask,
        refGlob,
        scopes,
        predicateCel,
        s.passphrase,
        s.kek,
        s.authPrincipal,
        s.authPassphrase,
    )

    fun aclRevokeScoped(
        effect: Int,
        subject: String,
        workspace: String? = null,
        domain: String? = null,
        rightsMask: Int,
        refGlob: String? = null,
        scopes: List<AclScope> = emptyList(),
    ): Boolean = Loom.aclRevokeScoped(
        s.path,
        effect,
        subject,
        workspace,
        domain,
        rightsMask,
        refGlob,
        scopes,
        s.passphrase,
        s.kek,
        s.authPrincipal,
        s.authPassphrase,
    )

    fun aclRevokeScopedPredicate(
        effect: Int,
        subject: String,
        workspace: String? = null,
        domain: String? = null,
        rightsMask: Int,
        refGlob: String? = null,
        scopes: List<AclScope> = emptyList(),
        predicateCel: String? = null,
    ): Boolean = Loom.aclRevokeScopedPredicate(
        s.path,
        effect,
        subject,
        workspace,
        domain,
        rightsMask,
        refGlob,
        scopes,
        predicateCel,
        s.passphrase,
        s.kek,
        s.authPrincipal,
        s.authPassphrase,
    )

    fun protectedRefListJson(workspace: String): String =
        Loom.protectedRefListJson(s.path, workspace, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun protectedRefGetJson(workspace: String, refName: String): String =
        Loom.protectedRefGetJson(
            s.path,
            workspace,
            refName,
            s.passphrase,
            s.kek,
            s.authPrincipal,
            s.authPassphrase,
        )

    fun protectedRefSet(
        workspace: String,
        refName: String,
        fastForwardOnly: Boolean,
        signedCommitsRequired: Boolean,
        signedRefAdvanceRequired: Boolean,
        requiredReviewCount: Int,
        retentionLock: Boolean,
        governanceLock: Boolean,
    ) = Loom.protectedRefSet(
        s.path,
        workspace,
        refName,
        fastForwardOnly,
        signedCommitsRequired,
        signedRefAdvanceRequired,
        requiredReviewCount,
        retentionLock,
        governanceLock,
        s.passphrase,
        s.kek,
        s.authPrincipal,
        s.authPassphrase,
    )

    fun protectedRefRemove(workspace: String, refName: String): Boolean =
        Loom.protectedRefRemove(s.path, workspace, refName, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)
}

/** Content-addressed store operations for a [LoomSession]. */
class CasOps(private val s: LoomSession) {
    fun put(workspace: String, content: ByteArray): String =
        Loom.casPut(s.path, workspace, content, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun get(workspace: String, digest: String): ByteArray? =
        Loom.casGet(s.path, workspace, digest, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun has(workspace: String, digest: String): Boolean =
        Loom.casHas(s.path, workspace, digest, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun delete(workspace: String, digest: String): Boolean =
        Loom.casDelete(s.path, workspace, digest, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun listJson(workspace: String): String =
        Loom.casListJson(s.path, workspace, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)
}

/** Document facet operations for a [LoomSession]. */
data class DocumentText(val text: String, val digest: String, val entityTag: String)

data class DocumentBinary(val bytes: ByteArray, val digest: String, val entityTag: String)

data class DocumentPutResult(val digest: String, val entityTag: String)

class DocumentOps(private val s: LoomSession) {
    fun putText(workspace: String, collection: String, id: String, text: String, expectedEntityTag: String? = null): DocumentPutResult =
        Loom.docPutText(
            s.path, workspace, collection, id, text, expectedEntityTag,
            s.passphrase, s.kek, s.authPrincipal, s.authPassphrase,
        )

    fun getText(workspace: String, collection: String, id: String): DocumentText? =
        Loom.docGetText(s.path, workspace, collection, id, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun putBinary(workspace: String, collection: String, id: String, bytes: ByteArray, expectedEntityTag: String? = null): DocumentPutResult =
        Loom.docPutBinary(
            s.path, workspace, collection, id, bytes, expectedEntityTag,
            s.passphrase, s.kek, s.authPrincipal, s.authPassphrase,
        )

    fun getBinary(workspace: String, collection: String, id: String): DocumentBinary? =
        Loom.docGetBinary(s.path, workspace, collection, id, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun delete(workspace: String, collection: String, id: String): Boolean =
        Loom.docDelete(s.path, workspace, collection, id, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun listBinary(workspace: String, collection: String): ByteArray =
        Loom.docListBinary(s.path, workspace, collection, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun indexCreate(workspace: String, collection: String, name: String, fieldPath: String, unique: Boolean = false) =
        Loom.docIndexCreate(s.path, workspace, collection, name, fieldPath, unique, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun indexCreateJson(workspace: String, collection: String, declarationJson: ByteArray) =
        Loom.docIndexCreateJson(s.path, workspace, collection, declarationJson, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun indexDrop(workspace: String, collection: String, name: String): Boolean =
        Loom.docIndexDrop(s.path, workspace, collection, name, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun indexRebuild(workspace: String, collection: String, name: String) =
        Loom.docIndexRebuild(s.path, workspace, collection, name, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun indexListJson(workspace: String, collection: String): String =
        Loom.docIndexListJson(s.path, workspace, collection, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun indexStatusJson(workspace: String, collection: String): String =
        Loom.docIndexStatusJson(s.path, workspace, collection, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun findJson(workspace: String, collection: String, index: String, valueJson: String): String =
        Loom.docFindJson(s.path, workspace, collection, index, valueJson, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun queryJson(workspace: String, collection: String, queryJson: String): String =
        Loom.docQueryJson(s.path, workspace, collection, queryJson, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)
}

/** Time-series facet operations for a [LoomSession]. */
class TimeSeriesOps(private val s: LoomSession) {
    fun put(workspace: String, collection: String, ts: Long, value: ByteArray) =
        Loom.tsPut(s.path, workspace, collection, ts, value, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun get(workspace: String, collection: String, ts: Long): ByteArray? =
        Loom.tsGet(s.path, workspace, collection, ts, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun range(workspace: String, collection: String, from: Long, to: Long): ByteArray =
        Loom.tsRange(s.path, workspace, collection, from, to, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun latest(workspace: String, collection: String): TsPoint? =
        Loom.tsLatest(s.path, workspace, collection, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)
}

/** Ledger facet operations for a [LoomSession]. */
class LedgerOps(private val s: LoomSession) {
    fun append(workspace: String, collection: String, payload: ByteArray): Long =
        Loom.ledgerAppend(s.path, workspace, collection, payload, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun get(workspace: String, collection: String, seq: Long): ByteArray? =
        Loom.ledgerGet(s.path, workspace, collection, seq, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun head(workspace: String, collection: String): String? =
        Loom.ledgerHead(s.path, workspace, collection, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun len(workspace: String, collection: String): Long =
        Loom.ledgerLen(s.path, workspace, collection, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun verify(workspace: String, collection: String) =
        Loom.ledgerVerify(s.path, workspace, collection, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)
}

/** Append-log queue operations for a [LoomSession]. */
class QueueOps(private val s: LoomSession) {
    fun append(workspace: String, stream: String, entry: ByteArray): Long =
        Loom.queueAppend(s.path, workspace, stream, entry, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun get(workspace: String, stream: String, seq: Long): ByteArray? =
        Loom.queueGet(s.path, workspace, stream, seq, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun range(workspace: String, stream: String, lo: Long, hi: Long): ByteArray =
        Loom.queueRangeCbor(s.path, workspace, stream, lo, hi, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun len(workspace: String, stream: String): Long =
        Loom.queueLen(s.path, workspace, stream, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun consumerPosition(workspace: String, stream: String, consumerId: String): Long =
        Loom.queueConsumerPosition(
            s.path,
            workspace,
            stream,
            consumerId,
            s.passphrase,
            s.kek,
            s.authPrincipal,
            s.authPassphrase,
        )

    fun consumerRead(workspace: String, stream: String, consumerId: String, max: Int): ByteArray =
        Loom.queueConsumerReadCbor(
            s.path,
            workspace,
            stream,
            consumerId,
            max,
            s.passphrase,
            s.kek,
            s.authPrincipal,
            s.authPassphrase,
        )

    fun consumerAdvance(workspace: String, stream: String, consumerId: String, nextSeq: Long) =
        Loom.queueConsumerAdvance(
            s.path,
            workspace,
            stream,
            consumerId,
            nextSeq,
            s.passphrase,
            s.kek,
            s.authPrincipal,
            s.authPassphrase,
        )

    fun consumerReset(workspace: String, stream: String, consumerId: String, nextSeq: Long) =
        Loom.queueConsumerReset(
            s.path,
            workspace,
            stream,
            consumerId,
            nextSeq,
            s.passphrase,
            s.kek,
            s.authPrincipal,
            s.authPassphrase,
        )
}

/** Version-control inspection for a [LoomSession] (returns raw Loom Canonical CBOR). */
class VcsOps(private val s: LoomSession) {
    fun blame(workspace: String, branch: String): ByteArray =
        Loom.vcsBlameCbor(s.path, workspace, branch, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun diff(workspace: String, fromCommit: String, toCommit: String): ByteArray =
        Loom.vcsDiffCbor(
            s.path,
            workspace,
            fromCommit,
            toCommit,
            s.passphrase,
            s.kek,
            s.authPrincipal,
            s.authPassphrase,
        )

    fun watchSubscribe(
        workspace: String,
        branch: String,
        facet: String? = null,
        pathPrefix: String? = null,
        changeKinds: List<String> = emptyList(),
        fromCommit: String? = null,
    ): String =
        Loom.watchSubscribe(
            s.path,
            workspace,
            branch,
            facet,
            pathPrefix,
            changeKinds,
            fromCommit,
            s.passphrase,
            s.kek,
            s.authPrincipal,
            s.authPassphrase,
        )

    fun watchPoll(cursor: String, max: UInt): ByteArray =
        Loom.watchPollCbor(s.path, cursor, max, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)
}

/** Calendar facet operations for a [LoomSession] (per principal). */
class CalendarOps(private val s: LoomSession) {
    fun createCollection(
        workspace: String, principal: String, collection: String,
        displayName: String, components: String,
    ) = Loom.calCreateCollection(
        s.path, workspace, principal, collection, displayName, components, s.passphrase, s.kek,
        s.authPrincipal, s.authPassphrase,
    )

    fun deleteCollection(workspace: String, principal: String, collection: String): Boolean =
        Loom.calDeleteCollection(
            s.path, workspace, principal, collection, s.passphrase, s.kek,
            s.authPrincipal, s.authPassphrase,
        )

    fun listCollections(workspace: String, principal: String): ByteArray =
        Loom.calListCollections(s.path, workspace, principal, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun putEntry(workspace: String, principal: String, collection: String, entry: ByteArray) =
        Loom.calPutEntry(
            s.path, workspace, principal, collection, entry, s.passphrase, s.kek,
            s.authPrincipal, s.authPassphrase,
        )

    fun getEntry(workspace: String, principal: String, collection: String, uid: String): ByteArray? =
        Loom.calGetEntry(
            s.path, workspace, principal, collection, uid, s.passphrase, s.kek,
            s.authPrincipal, s.authPassphrase,
        )

    fun deleteEntry(workspace: String, principal: String, collection: String, uid: String): Boolean =
        Loom.calDeleteEntry(
            s.path, workspace, principal, collection, uid, s.passphrase, s.kek,
            s.authPrincipal, s.authPassphrase,
        )

    fun listEntries(workspace: String, principal: String, collection: String): ByteArray =
        Loom.calListEntries(
            s.path, workspace, principal, collection, s.passphrase, s.kek,
            s.authPrincipal, s.authPassphrase,
        )

    fun range(
        workspace: String, principal: String, collection: String, from: String, to: String,
    ): ByteArray =
        Loom.calRange(
            s.path, workspace, principal, collection, from, to, s.passphrase, s.kek,
            s.authPrincipal, s.authPassphrase,
        )

    fun search(
        workspace: String, principal: String, collection: String, component: String, text: String,
    ): ByteArray =
        Loom.calSearch(
            s.path, workspace, principal, collection, component, text, s.passphrase, s.kek,
            s.authPrincipal, s.authPassphrase,
        )

    fun entryIcs(workspace: String, principal: String, collection: String, uid: String): String? =
        Loom.calEntryIcs(
            s.path, workspace, principal, collection, uid, s.passphrase, s.kek,
            s.authPrincipal, s.authPassphrase,
        )

    fun putIcs(workspace: String, principal: String, collection: String, ics: String): String =
        Loom.calPutIcs(
            s.path, workspace, principal, collection, ics, s.passphrase, s.kek,
            s.authPrincipal, s.authPassphrase,
        )
}

/** Contacts facet operations for a [LoomSession] (per principal). */
class ContactsOps(private val s: LoomSession) {
    fun createBook(workspace: String, principal: String, book: String, displayName: String) =
        Loom.cardCreateBook(
            s.path, workspace, principal, book, displayName, s.passphrase, s.kek,
            s.authPrincipal, s.authPassphrase,
        )

    fun deleteBook(workspace: String, principal: String, book: String): Boolean =
        Loom.cardDeleteBook(s.path, workspace, principal, book, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun listBooks(workspace: String, principal: String): ByteArray =
        Loom.cardListBooks(s.path, workspace, principal, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun putEntry(workspace: String, principal: String, book: String, entry: ByteArray) =
        Loom.cardPutEntry(
            s.path, workspace, principal, book, entry, s.passphrase, s.kek,
            s.authPrincipal, s.authPassphrase,
        )

    fun getEntry(workspace: String, principal: String, book: String, uid: String): ByteArray? =
        Loom.cardGetEntry(
            s.path, workspace, principal, book, uid, s.passphrase, s.kek,
            s.authPrincipal, s.authPassphrase,
        )

    fun deleteEntry(workspace: String, principal: String, book: String, uid: String): Boolean =
        Loom.cardDeleteEntry(
            s.path, workspace, principal, book, uid, s.passphrase, s.kek,
            s.authPrincipal, s.authPassphrase,
        )

    fun listEntries(workspace: String, principal: String, book: String): ByteArray =
        Loom.cardListEntries(s.path, workspace, principal, book, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun search(workspace: String, principal: String, book: String, text: String): ByteArray =
        Loom.cardSearch(s.path, workspace, principal, book, text, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun entryVcard(workspace: String, principal: String, book: String, uid: String): String? =
        Loom.cardEntryVcard(
            s.path, workspace, principal, book, uid, s.passphrase, s.kek,
            s.authPrincipal, s.authPassphrase,
        )

    fun putVcard(workspace: String, principal: String, book: String, vcf: String): String =
        Loom.cardPutVcard(
            s.path, workspace, principal, book, vcf, s.passphrase, s.kek,
            s.authPrincipal, s.authPassphrase,
        )
}

/** Mail facet operations for a [LoomSession] (per principal). */
class MailOps(private val s: LoomSession) {
    fun createMailbox(workspace: String, principal: String, mailbox: String, displayName: String) =
        Loom.mailCreateMailbox(
            s.path, workspace, principal, mailbox, displayName, s.passphrase, s.kek,
            s.authPrincipal, s.authPassphrase,
        )

    fun deleteMailbox(workspace: String, principal: String, mailbox: String): Boolean =
        Loom.mailDeleteMailbox(
            s.path, workspace, principal, mailbox, s.passphrase, s.kek,
            s.authPrincipal, s.authPassphrase,
        )

    fun listMailboxes(workspace: String, principal: String): ByteArray =
        Loom.mailListMailboxes(s.path, workspace, principal, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun ingestMessage(
        workspace: String, principal: String, mailbox: String, uid: String, raw: ByteArray,
    ): String =
        Loom.mailIngestMessage(
            s.path, workspace, principal, mailbox, uid, raw, s.passphrase, s.kek,
            s.authPrincipal, s.authPassphrase,
        )

    fun getMessage(workspace: String, principal: String, mailbox: String, uid: String): ByteArray? =
        Loom.mailGetMessage(
            s.path, workspace, principal, mailbox, uid, s.passphrase, s.kek,
            s.authPrincipal, s.authPassphrase,
        )

    fun toEml(workspace: String, principal: String, mailbox: String, uid: String): ByteArray? =
        Loom.mailToEml(
            s.path, workspace, principal, mailbox, uid, s.passphrase, s.kek,
            s.authPrincipal, s.authPassphrase,
        )

    fun deleteMessage(workspace: String, principal: String, mailbox: String, uid: String): Boolean =
        Loom.mailDeleteMessage(
            s.path, workspace, principal, mailbox, uid, s.passphrase, s.kek,
            s.authPrincipal, s.authPassphrase,
        )

    fun listMessages(workspace: String, principal: String, mailbox: String): ByteArray =
        Loom.mailListMessages(
            s.path, workspace, principal, mailbox, s.passphrase, s.kek,
            s.authPrincipal, s.authPassphrase,
        )

    fun getFlags(workspace: String, principal: String, mailbox: String, uid: String): ByteArray =
        Loom.mailGetFlags(
            s.path, workspace, principal, mailbox, uid, s.passphrase, s.kek,
            s.authPrincipal, s.authPassphrase,
        )

    fun setFlags(
        workspace: String, principal: String, mailbox: String, uid: String, flags: ByteArray,
    ) = Loom.mailSetFlags(
        s.path, workspace, principal, mailbox, uid, flags, s.passphrase, s.kek,
        s.authPrincipal, s.authPassphrase,
    )

    fun search(workspace: String, principal: String, mailbox: String, text: String): ByteArray =
        Loom.mailSearch(s.path, workspace, principal, mailbox, text, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)
}

/** SQL table inspection for a [LoomSession] (read-only direct readers; raw Loom Canonical CBOR). */
class SqlTableOps(private val s: LoomSession) {
    fun readTable(workspace: String, table: String): ByteArray =
        Loom.sqlReadTableCbor(s.path, workspace, table, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun readTableAt(workspace: String, table: String, commit: String): ByteArray =
        Loom.sqlReadTableAtCbor(s.path, workspace, table, commit, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun indexScan(workspace: String, table: String, index: String, prefix: ByteArray): ByteArray =
        Loom.sqlIndexScanCbor(
            s.path, workspace, table, index, prefix, s.passphrase, s.kek,
            s.authPrincipal, s.authPassphrase,
        )

    fun indexScanAt(workspace: String, table: String, index: String, prefix: ByteArray, commit: String): ByteArray =
        Loom.sqlIndexScanAtCbor(
            s.path, workspace, table, index, prefix, commit, s.passphrase, s.kek,
            s.authPrincipal, s.authPassphrase,
        )

    fun blame(workspace: String, branch: String, table: String): ByteArray =
        Loom.sqlBlameCbor(s.path, workspace, branch, table, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun diff(workspace: String, table: String, fromCommit: String, toCommit: String): ByteArray =
        Loom.sqlDiffCbor(
            s.path, workspace, table, fromCommit, toCommit, s.passphrase, s.kek,
            s.authPrincipal, s.authPassphrase,
        )

    fun tableDiff(workspace: String, table: String, fromCommit: String, toCommit: String): ByteArray =
        Loom.sqlTableDiffCbor(
            s.path, workspace, table, fromCommit, toCommit, s.passphrase, s.kek,
            s.authPrincipal, s.authPassphrase,
        )
}

/** Workspace administration for a [LoomSession]. */
class WorkspaceOps(private val s: LoomSession) {
    fun create(name: String, facet: String): String =
        Loom.workspaceCreate(s.path, name, facet, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun listJson(): String =
        Loom.workspaceListJson(s.path, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun rename(workspace: String, newName: String) =
        Loom.workspaceRename(s.path, workspace, newName, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun delete(workspace: String) =
        Loom.workspaceDelete(s.path, workspace, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)
}

/** Property-graph facet operations for a [LoomSession] (per graph; returns raw Loom Canonical CBOR). */
class GraphOps(private val s: LoomSession) {
    fun upsertNode(workspace: String, name: String, id: String, props: ByteArray) =
        Loom.graphUpsertNode(s.path, workspace, name, id, props, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun getNode(workspace: String, name: String, id: String): ByteArray? =
        Loom.graphGetNode(s.path, workspace, name, id, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun removeNode(workspace: String, name: String, id: String, cascade: Boolean) =
        Loom.graphRemoveNode(s.path, workspace, name, id, cascade, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun upsertEdge(
        workspace: String, name: String, id: String, src: String, dst: String, label: String,
        props: ByteArray,
    ) = Loom.graphUpsertEdge(
        s.path,
        workspace,
        name,
        id,
        src,
        dst,
        label,
        props,
        s.passphrase,
        s.kek,
        s.authPrincipal,
        s.authPassphrase,
    )

    fun getEdge(workspace: String, name: String, id: String): ByteArray? =
        Loom.graphGetEdge(s.path, workspace, name, id, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun removeEdge(workspace: String, name: String, id: String): Boolean =
        Loom.graphRemoveEdge(s.path, workspace, name, id, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun neighbors(workspace: String, name: String, id: String): ByteArray =
        Loom.graphNeighborsCbor(s.path, workspace, name, id, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun outEdges(workspace: String, name: String, id: String): ByteArray =
        Loom.graphOutEdgesCbor(s.path, workspace, name, id, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun inEdges(workspace: String, name: String, id: String): ByteArray =
        Loom.graphInEdgesCbor(s.path, workspace, name, id, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun reachable(
        workspace: String, name: String, start: String, maxDepth: Long, viaLabel: String?,
    ): ByteArray =
        Loom.graphReachableCbor(
            s.path,
            workspace,
            name,
            start,
            maxDepth,
            viaLabel,
            s.passphrase,
            s.kek,
            s.authPrincipal,
            s.authPassphrase,
        )

    fun shortestPath(
        workspace: String, name: String, from: String, to: String, viaLabel: String?,
    ): ByteArray? =
        Loom.graphShortestPathCbor(
            s.path,
            workspace,
            name,
            from,
            to,
            viaLabel,
            s.passphrase,
            s.kek,
            s.authPrincipal,
            s.authPassphrase,
        )
}

/** Vector-set facet operations for a [LoomSession] (per set; returns raw Loom Canonical CBOR). */
class VectorOps(private val s: LoomSession) {
    fun create(workspace: String, name: String, dim: Long, metric: Int) =
        Loom.vectorCreate(
            s.path, workspace, name, dim, metric, s.passphrase, s.kek,
            s.authPrincipal, s.authPassphrase,
        )

    fun upsert(workspace: String, name: String, id: String, vector: ByteArray, metadata: ByteArray) =
        Loom.vectorUpsert(
            s.path, workspace, name, id, vector, metadata, s.passphrase, s.kek,
            s.authPrincipal, s.authPassphrase,
        )

    fun upsertSource(
        workspace: String,
        name: String,
        id: String,
        vector: ByteArray,
        metadata: ByteArray,
        sourceText: ByteArray,
        modelId: String? = null,
        weightsDigest: String? = null,
    ) =
        Loom.vectorUpsertSource(
            s.path, workspace, name, id, vector, metadata, sourceText, modelId, weightsDigest,
            s.passphrase, s.kek, s.authPrincipal, s.authPassphrase,
        )

    fun get(workspace: String, name: String, id: String): ByteArray? =
        Loom.vectorGet(s.path, workspace, name, id, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun sourceText(workspace: String, name: String, id: String): ByteArray? =
        Loom.vectorSourceText(s.path, workspace, name, id, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun embeddingModel(workspace: String, name: String): ByteArray? =
        Loom.vectorEmbeddingModelCbor(s.path, workspace, name, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun ids(workspace: String, name: String, prefix: String? = null): ByteArray =
        Loom.vectorIdsCbor(s.path, workspace, name, prefix, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun metadataIndexKeys(workspace: String, name: String): ByteArray =
        Loom.vectorMetadataIndexKeysCbor(s.path, workspace, name, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun createMetadataIndex(workspace: String, name: String, key: String): Boolean =
        Loom.vectorCreateMetadataIndex(
            s.path, workspace, name, key, s.passphrase, s.kek,
            s.authPrincipal, s.authPassphrase,
        )

    fun dropMetadataIndex(workspace: String, name: String, key: String): Boolean =
        Loom.vectorDropMetadataIndex(
            s.path, workspace, name, key, s.passphrase, s.kek,
            s.authPrincipal, s.authPassphrase,
        )

    fun delete(workspace: String, name: String, id: String): Boolean =
        Loom.vectorDelete(s.path, workspace, name, id, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun search(workspace: String, name: String, query: ByteArray, k: Long, filter: ByteArray): ByteArray =
        Loom.vectorSearchCbor(
            s.path, workspace, name, query, k, filter, s.passphrase, s.kek,
            s.authPrincipal, s.authPassphrase,
        )

    fun searchPolicy(
        workspace: String,
        name: String,
        query: ByteArray,
        k: Long,
        filter: ByteArray,
        policy: Int,
        threshold: Long,
        ef: Long,
        pqM: Long,
        pqK: Long,
        pqIters: Long,
    ): ByteArray =
        Loom.vectorSearchPolicyCbor(
            s.path, workspace, name, query, k, filter, policy, threshold, ef, pqM, pqK, pqIters,
            s.passphrase, s.kek, s.authPrincipal, s.authPassphrase,
        )
}

/** Columnar-dataset facet operations for a [LoomSession] (per dataset; returns raw Loom Canonical CBOR). */
class ColumnarOps(private val s: LoomSession) {
    fun create(workspace: String, name: String, columns: ByteArray, targetSegmentRows: Long) =
        Loom.columnarCreate(
            s.path,
            workspace,
            name,
            columns,
            targetSegmentRows,
            s.passphrase,
            s.kek,
            s.authPrincipal,
            s.authPassphrase,
        )

    fun append(workspace: String, name: String, row: ByteArray) =
        Loom.columnarAppend(s.path, workspace, name, row, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun scan(workspace: String, name: String): ByteArray =
        Loom.columnarScanCbor(s.path, workspace, name, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun columns(workspace: String, name: String): ByteArray =
        Loom.columnarColumnsCbor(s.path, workspace, name, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun rows(workspace: String, name: String): Long =
        Loom.columnarRows(s.path, workspace, name, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun compact(workspace: String, name: String) =
        Loom.columnarCompact(s.path, workspace, name, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun inspect(workspace: String, name: String): ByteArray =
        Loom.columnarInspectCbor(s.path, workspace, name, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun sourceDigest(workspace: String, name: String): ByteArray =
        Loom.columnarSourceDigestCbor(s.path, workspace, name, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun select(workspace: String, name: String, columns: ByteArray, filter: ByteArray): ByteArray =
        Loom.columnarSelectCbor(
            s.path,
            workspace,
            name,
            columns,
            filter,
            s.passphrase,
            s.kek,
            s.authPrincipal,
            s.authPassphrase,
        )

    fun aggregate(workspace: String, name: String, aggregates: ByteArray, filter: ByteArray): ByteArray =
        Loom.columnarAggregateCbor(
            s.path,
            workspace,
            name,
            aggregates,
            filter,
            s.passphrase,
            s.kek,
            s.authPrincipal,
            s.authPassphrase,
        )
}

/** Dataframe facet operations for a [LoomSession] (per frame; returns raw Loom Canonical CBOR). */
class DataframeOps(private val s: LoomSession) {
    fun create(workspace: String, name: String, plan: ByteArray) =
        Loom.dataframeCreate(s.path, workspace, name, plan, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun collect(workspace: String, name: String): ByteArray =
        Loom.dataframeCollectCbor(s.path, workspace, name, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun preview(workspace: String, name: String, rows: Long): ByteArray =
        Loom.dataframePreviewCbor(s.path, workspace, name, rows, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun materialize(workspace: String, name: String): String? =
        Loom.dataframeMaterialize(s.path, workspace, name, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun planDigest(workspace: String, name: String): String =
        Loom.dataframePlanDigest(s.path, workspace, name, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun sourceDigests(workspace: String, name: String): ByteArray =
        Loom.dataframeSourceDigestsCbor(s.path, workspace, name, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)
}

/** Search facet operations for a [LoomSession] (per collection; returns raw Loom Canonical CBOR). */
class SearchOps(private val s: LoomSession) {
    fun create(workspace: String, name: String, mapping: ByteArray) =
        Loom.searchCreate(s.path, workspace, name, mapping, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun index(workspace: String, name: String, id: ByteArray, doc: ByteArray) =
        Loom.searchIndex(s.path, workspace, name, id, doc, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun get(workspace: String, name: String, id: ByteArray): ByteArray? =
        Loom.searchGet(s.path, workspace, name, id, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun delete(workspace: String, name: String, id: ByteArray): Boolean =
        Loom.searchDelete(s.path, workspace, name, id, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun ids(workspace: String, name: String, prefix: ByteArray? = null): ByteArray =
        Loom.searchIdsCbor(s.path, workspace, name, prefix, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun remap(workspace: String, name: String, mapping: ByteArray) =
        Loom.searchRemap(s.path, workspace, name, mapping, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)

    fun query(workspace: String, name: String, request: ByteArray): ByteArray =
        Loom.searchQueryCbor(s.path, workspace, name, request, s.passphrase, s.kek, s.authPrincipal, s.authPassphrase)
}
