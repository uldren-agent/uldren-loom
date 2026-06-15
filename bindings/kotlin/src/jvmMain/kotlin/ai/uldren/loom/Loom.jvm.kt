package ai.uldren.loom

// Desktop/server JVM (off Android). Ensure libuldren_loom_jni.{so,dylib,dll} (and the
// libuldren_loom it links) are on java.library.path, e.g. -Djava.library.path=../../target/release.
actual object Loom {
    init {
        System.loadLibrary("uldren_loom_jni")
    }

    actual external fun version(): String

    actual external fun blobDigest(data: ByteArray): String
}
