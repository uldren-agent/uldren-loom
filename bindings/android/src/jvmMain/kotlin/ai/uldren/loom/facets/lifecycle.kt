package ai.uldren.loom

actual fun Loom.create(path: String, profile: String, suite: String?, passphrase: String?) =
        LoomNative.nativeCreate(path, profile, suite, passphrase?.encodeToByteArray())


actual fun Loom.createWithKek(path: String, profile: String, kek: ByteArray, suite: String?) =
        LoomNative.nativeCreateWithKek(path, profile, suite, kek)
