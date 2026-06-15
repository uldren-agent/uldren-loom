package ai.uldren.loom

actual object Loom {
    init {
        System.loadLibrary("uldren_loom_jni")
    }

    actual external fun version(): String

    actual external fun blobDigest(data: ByteArray): String
}
