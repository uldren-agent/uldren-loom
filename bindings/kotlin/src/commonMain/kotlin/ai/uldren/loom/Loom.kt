package ai.uldren.loom

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
}
