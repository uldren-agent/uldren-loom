package ai.uldren.loom

    /**
     * Create a fresh `.loom` at [path] under an identity [profile] (`"default"`/`"blake3"` or
     * `"fips"`/`"sha256"`), optionally encrypted - the binding counterpart of `loom init`.
     * A non-null/non-empty [passphrase] encrypts the store; the DEK is wrapped
     * under it with [suite], or the profile default when [suite] is null); otherwise unencrypted.
     * Throws on failure (e.g. `ALREADY_EXISTS`).
     */
expect fun Loom.create(path: String, profile: String, suite: String? = null, passphrase: String? = null)


    /**
     * Create a fresh **encrypted** `.loom` whose DEK is wrapped under a host-supplied 256-bit [kek].
     * [profile] selects the content-address algorithm and [suite] the object AEAD (profile default
     * when null). [kek] must be 32 bytes.
     */
expect fun Loom.createWithKek(path: String, profile: String, kek: ByteArray, suite: String? = null)
