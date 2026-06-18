package ai.uldren.loom

/** The most recent point of a time series: its timestamp and value bytes. */
data class TsPoint(val ts: Long, val value: ByteArray)

/**
 * Kotlin Multiplatform binding over the Uldren Loom C ABI.
 *
 * The same API is provided on the JVM (`jvm` target, off Android) and on Android (`androidTarget`),
 * both through the JNI shim `libuldren_loom_jni`.
 */
expect object Loom {
    /** The engine version. */
    fun version(): String

    /** The content address (`"algo:hex"`, e.g. `blake3:...`) of [data] as an Uldren Loom blob. */
    fun blobDigest(data: ByteArray): String

    /**
     * Build capability report (0010 section 5) as canonical CBOR: a `CapabilitySet` map with
     * `schema_version` and `records`. Build-aware: capabilities owned by the linked crates are
     * reported with operational state `supported`.
     */
    fun capabilities(): ByteArray

    /** Runtime provider/profile report as canonical CBOR. */
    fun runtimeProfile(): ByteArray

    fun studioSurfaceCatalogJson(workspace: String, set: String = "all"): String

    fun execCbor(
        path: String,
        request: ByteArray,
        passphrase: ByteArray? = null,
        kek: ByteArray? = null,
        authPrincipal: String? = null,
        authPassphrase: ByteArray? = null,
    ): ByteArray

}

/**
 * A SQL session over a workspace SQL facet in a `.loom`, through the same JNI shim.
 *
 * A reopenable handle: each [exec] / [commit] opens the loom for its duration and releases it, so
 * sessions are cheap and coexist. Call [close] when done. A non-zero status from the C ABI is raised
 * as a `RuntimeException` carrying the engine's error message.
 */
expect class LoomSql {
    /** Open `path` and start a SQL session over `workspace`'s SQL facet (created if absent). */
    constructor(path: String, workspace: String, db: String)

    /**
     * Open an **encrypted** loom, unlocking it with [passphrase]. The host
     * acquires the passphrase securely; no environment variable is read.
     */
    constructor(path: String, workspace: String, db: String, passphrase: String)

    /**
     * Open an **encrypted** loom with a host-supplied 256-bit [kek] that directly unwraps the DEK.
     * [kek] can come from a keychain, Secure Enclave, passkey-PRF, or KMS. [kek] must be 32 bytes.
     */
    constructor(path: String, workspace: String, db: String, kek: ByteArray)

    constructor(
        path: String,
        workspace: String,
        db: String,
        passphrase: String?,
        kek: ByteArray?,
        authPrincipal: String?,
        authPassphrase: String?,
    )

    /**
     * Run SQL and return a **typed**, indexed [LoomResult] (decoded once via the shared result-view; no
     * CBOR is parsed in Kotlin). Read cells back as faithful [LoomCell]s and [LoomResult.close] it when
     * done. For raw bytes use [execBytes]; for the JSON debug form use [execJson].
     */
    fun exec(sql: String): LoomResult

    /**
     * Run SQL; returns a JSON array of the result payloads (debug/admin form, rendered from canonical
     * CBOR - not the type-faithful API; use [exec]).
     */
    fun execJson(sql: String): String

    /**
     * Run SQL; returns the result payloads as canonical CBOR bytes.
     *
     * `exec` / `execBytes` / `commit` block; for async, wrap them in the caller's coroutine context,
     * e.g. `withContext(Dispatchers.IO) { session.execBytes(sql) }` (the engine has no worker pool of
     * its own, so off-thread execution is the host runtime's job).
     */
    fun execBytes(sql: String): ByteArray

    /**
     * Run a `SELECT` and return a lazy [LoomRowStream] over its rows (the streaming form):
     * pull rows one at a time, never materializing the whole result.
     */
    fun query(sql: String): LoomRowStream

    /** Commit the staged database state; returns the new commit's content address. */
    fun commit(message: String, author: String): String

    /** Release the session. */
    fun close()
}

/**
 * A lazy, forward stream of a `SELECT`'s rows, through the JNI shim:
 * each [next] pulls one row and decodes it into a one-row [LoomResult] (its cells at item 0, row 0), so
 * a large result is never materialized. Obtained from [LoomSql.query]; [close] it when done. The
 * constructor takes an engine iterator handle and is for `query`'s use only.
 */
expect class LoomRowStream(handle: Long) {
    /** The next row as a one-row [LoomResult] (read cells with `cell(0, 0, col)`), or `null` at the end. */
    fun next(): LoomResult?

    /** Release the iterator. */
    fun close()
}

/**
 * An explicit transaction/batch scope, through the same JNI shim. Unlike [LoomSql], a
 * batch holds the `.loom` open - and its exclusive write lock - for its whole lifetime, so an SQL
 * transaction (`BEGIN`/`COMMIT`/`ROLLBACK`) can span [exec] calls; changes become durable through a
 * single atomic save at [commit] (or [commitVcs]). The SQL `COMMIT` is distinct from the VCS commit.
 * Call [close] to release the lock; closing without a commit discards un-persisted changes.
 */
expect class LoomSqlBatch {
    /** Begin a batch over `workspace`'s SQL facet (created if absent), database `db`, in `path`. */
    constructor(path: String, workspace: String, db: String)

    /**
     * Begin a batch over an **encrypted** loom, unlocking it with [passphrase] for the batch's lifetime.
     */
    constructor(path: String, workspace: String, db: String, passphrase: String)

    /**
     * Begin a batch over an **encrypted** loom with a host-supplied 256-bit [kek]. [kek] must be 32
     * bytes.
     */
    constructor(path: String, workspace: String, db: String, kek: ByteArray)

    /**
     * Run SQL in the batch (including `BEGIN`/`COMMIT`/`ROLLBACK`) and return a typed [LoomResult].
     * Changes accumulate until [commit].
     */
    fun exec(sql: String): LoomResult

    /** Run SQL in the batch; returns the result payloads as canonical CBOR bytes. */
    fun execBytes(sql: String): ByteArray

    /**
     * Make the batch's changes durable with one atomic save (no history entry). Rejected while an SQL
     * transaction is open. The batch stays open.
     */
    fun commit()

    /**
     * Like [commit], but also records a VCS commit; returns its content address. Distinct from a SQL
     * `COMMIT`. Rejected while an SQL transaction is open.
     */
    fun commitVcs(message: String, author: String): String

    /** Discard un-persisted in-memory changes (and any open SQL transaction); the batch stays open. */
    fun abort()

    /** Release the write lock and free the batch. Closing without a commit discards un-persisted changes. */
    fun close()
}

/**
 * The rows of item [item] (default 0) as lists of typed [LoomCell]s - the idiomatic
 * `for (row in result.rows()) { ... }` form (over the already-decoded
 * typed result). An extension over the public accessors, so it is shared by both targets.
 */
fun LoomResult.rows(item: Long = 0L): List<List<LoomCell>> {
    val rowCount = rowCount(item)
    val out = ArrayList<List<LoomCell>>(rowCount.toInt())
    var r = 0L
    while (r < rowCount) {
        val n = rowLen(item, r)
        val row = ArrayList<LoomCell>(n.toInt())
        var c = 0L
        while (c < n) {
            row.add(cell(item, r, c))
            c++
        }
        out.add(row)
        r++
    }
    return out
}

/**
 * One decoded result cell. Only the accessors the [tag] selects are meaningful (the engine's
 * `LOOM_VALUE_*` tags). 128-bit ints, UUIDs, the decimal mantissa, and INET octets arrive in [bytes16]
 * (little-endian); floats carry both [doubleValue] and the raw IEEE-754 [bits]. [uint64] holds the raw
 * bits of an unsigned value (read it with `.toULong()`).
 */
data class LoomCell(
    val tag: Int,
    val scale: Int,
    val int64: Long,
    val int64Secondary: Long,
    val uint64: Long,
    val doubleValue: Double,
    val doubleSecondary: Double,
    val bits: Long,
    val bitsSecondary: Long,
    val bytes16: ByteArray,
    val data: ByteArray?,
) {
    /** True for a NULL cell. */
    val isNull: Boolean get() = tag == 0

    /** UTF-8 text payload (Text), or empty. */
    fun text(): String = data?.decodeToString() ?: ""

    /** Raw byte payload (Bytes), or the canonical CBOR of a LIST/MAP cell, or empty. */
    fun bytes(): ByteArray = data ?: ByteArray(0)
}

/**
 * A decoded, immutable, indexed SQL result (the typed [LoomSql.exec] return). Navigate it with the
 * indexed accessors (mirroring the C result-view ABI) and read cells as [LoomCell]; [close] it when
 * done. One shared decoder backs every C-ABI binding, so no CBOR is parsed in Kotlin. The constructor
 * takes an engine result handle and is for [LoomSql.exec]'s use only.
 */
expect class LoomResult(handle: Long) {
    /** Number of statement results. */
    fun len(): Long

    /** The kind of item [item] (a `LOOM_RESULT_*` value). */
    fun itemKind(item: Long): Int

    fun columnCount(item: Long): Long
    fun columnName(item: Long, col: Long): String
    fun columnType(item: Long, col: Long): String
    fun rowCount(item: Long): Long
    fun rowLen(item: Long, row: Long): Long
    fun cell(item: Long, row: Long, col: Long): LoomCell

    /** Row count of an Insert/Delete/Update/DropTable statement. */
    fun rowsAffected(item: Long): Long

    fun stringCount(item: Long): Long
    fun string(item: Long, index: Long): String

    /** ShowVariable variable kind (`LOOM_VARIABLE_*`). */
    fun variableKind(item: Long): Int

    fun mapLen(item: Long, row: Long): Long
    fun mapKey(item: Long, row: Long, idx: Long): String
    fun mapValue(item: Long, row: Long, idx: Long): LoomCell

    /** Release the result view. */
    fun close()
}
