//! Encryption key layer for at-rest encryption.
//!
//! This module is **pure crypto**: a key-derivation function, data-encryption-key (DEK) wrap/unwrap,
//! suite-aware content-encryption-key (CEK) derivation, AEAD seal/unseal, and the `encryption_meta`
//! codec. It performs **no randomness generation and no I/O**; every random input (KDF salt, the DEK
//! itself, and AEAD nonces) is supplied by the caller.
//!
//! **Crypto-suite agility lives here; object-digest agility does not.** The store identity profile
//! selects the object digest. This module selects the encryption suite and KDF. The two suites are:
//!
//! - [`Suite::XChaCha20Poly1305`] (default, id `0x01`): XChaCha20-Poly1305 AEAD; CEK derived with
//!   keyed-BLAKE3 (already a dependency, no new KDF primitive). Not a FIPS algorithm.
//! - [`Suite::Aes256Gcm`] (id `0x02`): AES-256-GCM AEAD; CEK derived with **HKDF-SHA-256** - a
//!   NIST-approved derivation, with **no BLAKE3 in its cryptographic path** - so the suite is
//!   NIST-oriented end to end.

use crate::digest::Digest;
use crate::error::{Code, LoomError, Result};
use zeroize::Zeroizing;

/// Symmetric key length (256-bit) for the master key, DEK, and every CEK.
pub const KEY_LEN: usize = 32;
/// Argon2id memory cost in KiB (RFC 9106 profile: 64 MiB).
const ARGON2_M_KIB: u32 = 65_536;
/// Argon2id time cost (iterations).
const ARGON2_T: u32 = 3;
/// Argon2id parallelism.
const ARGON2_P: u32 = 4;
/// PBKDF2-HMAC-SHA-256 iteration count for the FIPS profile's passphrase KDF.
/// OWASP-2023-grade for PBKDF2-HMAC-SHA-256; recorded implicitly by the FIPS wrap-alg.
const PBKDF2_ITERS: u32 = 600_000;
/// DEK-wrap algorithm id for the **default** profile: XChaCha20-Poly1305, with an Argon2id passphrase
/// KDF. This is the identity-profile key-management pairing for `blake3` stores.
pub const WRAP_ALG_XCHACHA20POLY1305: u8 = 0x01;
/// DEK-wrap algorithm id for the **FIPS** profile: AES-256-GCM DEK wrap with a
/// PBKDF2-HMAC-SHA-256 passphrase KDF, so a FIPS store's key-management path has no XChaCha/Argon2id
/// (non-FIPS) primitive. The `wrap_alg` byte selects the whole key-management pairing (KDF + wrap AEAD).
pub const WRAP_ALG_AES256GCM: u8 = 0x02;
/// The passphrase key-derivation function. Selected by the identity profile and recorded via `wrap_alg`
/// (default uses Argon2id, FIPS uses PBKDF2-HMAC-SHA-256); never mixed within one store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kdf {
    Argon2id,
    Pbkdf2Sha256,
}
/// Domain-separation associated data for the DEK wrap (distinguishes it from object frames).
const WRAP_AAD: &[u8] = b"uldren-loom/dek-wrap/v1";
/// Domain-separation context mixed into every CEK derivation.
const CEK_CONTEXT: &[u8] = b"uldren-loom/cek/v1";
/// `encryption_meta` container magic + version for the source-tagged multi-wrap descriptor.
const META_MAGIC: &[u8; 4] = b"LKM1";
const META_VERSION: u8 = 2;

/// The AEAD + CEK-derivation suite. The suite id is carried in each sealed frame **and** in the
/// `encryption_meta`, so a `rekey` can change the active suite while previously-sealed frames keep
/// deriving and opening under their own recorded suite.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Suite {
    /// Default, non-FIPS: XChaCha20-Poly1305 with a keyed-BLAKE3 CEK.
    XChaCha20Poly1305,
    /// NIST/FIPS-oriented: AES-256-GCM with an HKDF-SHA-256 CEK (no BLAKE3).
    Aes256Gcm,
}

impl Suite {
    /// The stable 1-byte id stored in frames and metadata.
    pub const fn id(self) -> u8 {
        match self {
            Suite::XChaCha20Poly1305 => 0x01,
            Suite::Aes256Gcm => 0x02,
        }
    }

    /// Parse a suite id; `0x03`-`0xff` are reserved.
    pub fn from_id(id: u8) -> Result<Self> {
        match id {
            0x01 => Ok(Suite::XChaCha20Poly1305),
            0x02 => Ok(Suite::Aes256Gcm),
            other => Err(LoomError::invalid(format!(
                "unknown AEAD suite id {other:#04x}"
            ))),
        }
    }

    /// AEAD nonce length in bytes: XChaCha20 uses a 192-bit (24-byte) nonce, AES-256-GCM a 96-bit
    /// (12-byte) nonce. The caller MUST supply a nonce of exactly this length.
    pub const fn nonce_len(self) -> usize {
        match self {
            Suite::XChaCha20Poly1305 => 24,
            Suite::Aes256Gcm => 12,
        }
    }

    /// Human-readable name, also the CLI `--suite` spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Suite::XChaCha20Poly1305 => "xchacha20-poly1305",
            Suite::Aes256Gcm => "aes-256-gcm",
        }
    }

    /// Parse the CLI `--suite` spelling.
    pub fn parse(name: &str) -> Result<Self> {
        match name {
            "xchacha20-poly1305" | "xchacha" => Ok(Suite::XChaCha20Poly1305),
            "aes-256-gcm" | "aes" => Ok(Suite::Aes256Gcm),
            other => Err(LoomError::invalid(format!("unknown AEAD suite {other:?}"))),
        }
    }
}

/// How the key-encrypting key (KEK) that wraps the DEK is obtained. This KDF is
/// **separate** from any principal-auth KDF; never conflate them.
#[derive(Clone)]
pub enum KeySpec {
    /// A passphrase, stretched by the profile KDF (Argon2id default / PBKDF2 FIPS) over the metadata's
    /// salt.
    Passphrase(Zeroizing<String>),
    /// A caller-supplied 256-bit KEK. The host computed the KEK before calling; loom-core wraps/unwraps
    /// the DEK under it directly, with no passphrase KDF.
    RawKek(Zeroizing<[u8; KEY_LEN]>),
}

impl KeySpec {
    /// Build a passphrase credential, wrapping it in a zeroizing buffer so callers (loom-store, the CLI)
    /// need no direct `zeroize` dependency.
    pub fn passphrase(passphrase: impl Into<String>) -> Self {
        KeySpec::Passphrase(Zeroizing::new(passphrase.into()))
    }

    /// Build a raw-KEK credential from a caller-supplied 256-bit key.
    pub fn raw_kek(kek: [u8; KEY_LEN]) -> Self {
        KeySpec::RawKek(Zeroizing::new(kek))
    }

    /// The [`WrapSource`] a *newly created* wrap of this credential records. A raw KEK is tagged
    /// `RawKek`; a host that knows the concrete provider can relabel the entry's source afterward.
    fn wrap_source(&self) -> WrapSource {
        match self {
            KeySpec::Passphrase(_) => WrapSource::Passphrase,
            KeySpec::RawKek(_) => WrapSource::RawKek,
        }
    }

    /// Whether this credential can attempt to unwrap a wrap entry from `source`. A passphrase only
    /// unlocks passphrase entries; a host-supplied KEK unlocks any **external** (non-passphrase) entry,
    /// since the host computed the KEK from that provider before calling.
    fn unlocks(&self, source: WrapSource) -> bool {
        match self {
            KeySpec::Passphrase(_) => source == WrapSource::Passphrase,
            KeySpec::RawKek(_) => source != WrapSource::Passphrase,
        }
    }
}

/// Derive the 256-bit master key from a passphrase + salt via Argon2id (RFC 9106 profile: 64 MiB,
/// t=3, p=4). The salt MUST be at least 8 bytes.
fn argon2id_master(passphrase: &[u8], salt: &[u8]) -> Result<Zeroizing<[u8; KEY_LEN]>> {
    use argon2::{Algorithm, Argon2, Params, Version};
    let params = Params::new(ARGON2_M_KIB, ARGON2_T, ARGON2_P, Some(KEY_LEN))
        .map_err(|e| LoomError::new(Code::Internal, format!("argon2 params: {e}")))?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut out = Zeroizing::new([0u8; KEY_LEN]);
    argon2
        .hash_password_into(passphrase, salt, out.as_mut_slice())
        .map_err(|e| LoomError::new(Code::E2eKeyInvalid, format!("argon2 kdf: {e}")))?;
    Ok(out)
}

/// Derive the 256-bit master key from a passphrase + salt via PBKDF2-HMAC-SHA-256 (the FIPS-profile
/// passphrase KDF). The salt MUST be at least 8 bytes.
fn pbkdf2_master(passphrase: &[u8], salt: &[u8]) -> Zeroizing<[u8; KEY_LEN]> {
    let mut out = Zeroizing::new([0u8; KEY_LEN]);
    pbkdf2::pbkdf2_hmac::<sha2::Sha256>(passphrase, salt, PBKDF2_ITERS, out.as_mut_slice());
    out
}

/// Resolve a [`KeySpec`] to the master key over `salt` under the profile's [`Kdf`]. A raw KEK *is* the
/// master key: it bypasses the KDF and ignores the salt.
fn master_key(spec: &KeySpec, salt: &[u8], kdf: Kdf) -> Result<Zeroizing<[u8; KEY_LEN]>> {
    match spec {
        KeySpec::Passphrase(p) => match kdf {
            Kdf::Argon2id => argon2id_master(p.as_bytes(), salt),
            Kdf::Pbkdf2Sha256 => Ok(pbkdf2_master(p.as_bytes(), salt)),
        },
        KeySpec::RawKek(k) => Ok(k.clone()),
    }
}

/// The key-management pairing (DEK-wrap AEAD + passphrase KDF) for an identity profile, selected by the
/// active object suite: the FIPS profile (`Aes256Gcm` objects) wraps the DEK with
/// AES-256-GCM under a PBKDF2 master key, so its key path has no XChaCha/Argon2id; the default profile
/// uses XChaCha20-Poly1305 + Argon2id.
fn wrap_pairing(active_suite: Suite) -> (u8, Suite, Kdf) {
    match active_suite {
        Suite::Aes256Gcm => (WRAP_ALG_AES256GCM, Suite::Aes256Gcm, Kdf::Pbkdf2Sha256),
        Suite::XChaCha20Poly1305 => (
            WRAP_ALG_XCHACHA20POLY1305,
            Suite::XChaCha20Poly1305,
            Kdf::Argon2id,
        ),
    }
}

/// Reverse of [`wrap_pairing`]: recover the wrap AEAD + KDF recorded in `encryption_meta` from the
/// stored `wrap_alg` byte. An unknown byte is a forward-version / corrupt store.
fn wrap_pairing_from_alg(wrap_alg: u8) -> Result<(Suite, Kdf)> {
    match wrap_alg {
        WRAP_ALG_XCHACHA20POLY1305 => Ok((Suite::XChaCha20Poly1305, Kdf::Argon2id)),
        WRAP_ALG_AES256GCM => Ok((Suite::Aes256Gcm, Kdf::Pbkdf2Sha256)),
        other => Err(LoomError::new(
            Code::Unsupported,
            format!("unknown DEK-wrap algorithm {other:#04x}"),
        )),
    }
}

/// Validate that `nonce` matches the suite's required length before handing it to a fixed-size AEAD
/// (which would otherwise panic on a wrong-length nonce).
fn check_nonce(suite: Suite, nonce: &[u8]) -> Result<()> {
    if nonce.len() == suite.nonce_len() {
        Ok(())
    } else {
        Err(LoomError::invalid(format!(
            "{} nonce must be {} bytes, got {}",
            suite.as_str(),
            suite.nonce_len(),
            nonce.len()
        )))
    }
}

/// AEAD-seal `plaintext` under `key` and `nonce`, binding `aad`, returning `ciphertext || tag`. The
/// suite selects the AEAD; the caller derives `key` (a CEK or the master key) and supplies a fresh
/// `nonce` of [`Suite::nonce_len`] bytes.
pub fn seal(
    suite: Suite,
    key: &[u8; KEY_LEN],
    nonce: &[u8],
    aad: &[u8],
    plaintext: &[u8],
) -> Result<Vec<u8>> {
    check_nonce(suite, nonce)?;
    let sealed = match suite {
        Suite::XChaCha20Poly1305 => {
            use chacha20poly1305::aead::{Aead, KeyInit, Payload};
            use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
            let cipher = XChaCha20Poly1305::new(Key::from_slice(key));
            cipher.encrypt(
                XNonce::from_slice(nonce),
                Payload {
                    msg: plaintext,
                    aad,
                },
            )
        }
        Suite::Aes256Gcm => {
            use aes_gcm::aead::{Aead, KeyInit, Payload};
            use aes_gcm::{Aes256Gcm, Key, Nonce};
            let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
            cipher.encrypt(
                Nonce::from_slice(nonce),
                Payload {
                    msg: plaintext,
                    aad,
                },
            )
        }
    };
    sealed.map_err(|_| LoomError::new(Code::Internal, "AEAD seal failed"))
}

/// AEAD-open `ciphertext` (which is `ciphertext || tag`) under `key` and `nonce`, checking `aad`. A
/// failed authentication tag (tamper, wrong key, wrong suite, or altered associated data) returns
/// [`Code::E2eKeyInvalid`] and **never** yields plaintext. The result is zeroized on drop.
pub fn unseal(
    suite: Suite,
    key: &[u8; KEY_LEN],
    nonce: &[u8],
    aad: &[u8],
    ciphertext: &[u8],
) -> Result<Zeroizing<Vec<u8>>> {
    check_nonce(suite, nonce)?;
    let opened = match suite {
        Suite::XChaCha20Poly1305 => {
            use chacha20poly1305::aead::{Aead, KeyInit, Payload};
            use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
            let cipher = XChaCha20Poly1305::new(Key::from_slice(key));
            cipher.decrypt(
                XNonce::from_slice(nonce),
                Payload {
                    msg: ciphertext,
                    aad,
                },
            )
        }
        Suite::Aes256Gcm => {
            use aes_gcm::aead::{Aead, KeyInit, Payload};
            use aes_gcm::{Aes256Gcm, Key, Nonce};
            let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
            cipher.decrypt(
                Nonce::from_slice(nonce),
                Payload {
                    msg: ciphertext,
                    aad,
                },
            )
        }
    };
    opened
        .map(Zeroizing::new)
        .map_err(|_| LoomError::new(Code::E2eKeyInvalid, "AEAD authentication failed"))
}

/// Derive the per-object content-encryption key from the DEK and the object's content address, under
/// `suite`. The suite picks the KDF: keyed-BLAKE3 for XChaCha20-Poly1305, HKDF-SHA-256 for AES-256-GCM
/// (the NIST path - no BLAKE3). No `workspace_id` is mixed in: the v1 encryption
/// boundary is the whole Loom and the store is workspace-blind, so the CEK binds the DEK plus object
/// identity only; frame associated data additionally binds the suite and digest against relocation.
pub fn derive_cek(suite: Suite, dek: &[u8; KEY_LEN], digest: &Digest) -> Zeroizing<[u8; KEY_LEN]> {
    match suite {
        Suite::XChaCha20Poly1305 => {
            let mut hasher = blake3::Hasher::new_keyed(dek);
            hasher.update(CEK_CONTEXT);
            hasher.update(digest.bytes());
            Zeroizing::new(*hasher.finalize().as_bytes())
        }
        Suite::Aes256Gcm => {
            let hk = hkdf::Hkdf::<sha2::Sha256>::new(None, dek);
            let mut info = Vec::with_capacity(CEK_CONTEXT.len() + 32);
            info.extend_from_slice(CEK_CONTEXT);
            info.extend_from_slice(digest.bytes());
            let mut okm = Zeroizing::new([0u8; KEY_LEN]);
            // HKDF-expand into 32 bytes is far below the 255*HashLen ceiling, so it cannot fail.
            hk.expand(&info, okm.as_mut_slice())
                .expect("hkdf-sha256 expand of 32 bytes is infallible");
            okm
        }
    }
}

/// An unlocked data-encryption-key session: the cleartext DEK (zeroized on drop) plus the active suite
/// used for **new** writes. Reads derive their CEK under the suite recorded in the frame, which may
/// differ from `active_suite` after a suite-changing rekey.
pub struct DekSession {
    dek: Zeroizing<[u8; KEY_LEN]>,
    active_suite: Suite,
}

impl std::fmt::Debug for DekSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print key material.
        f.debug_struct("DekSession")
            .field("active_suite", &self.active_suite)
            .field("dek", &"<redacted>")
            .finish()
    }
}

impl DekSession {
    /// The suite to seal new objects under.
    pub fn active_suite(&self) -> Suite {
        self.active_suite
    }

    /// Derive the CEK for `digest` under `suite` (the active suite on write, the frame's suite on read).
    pub fn derive_cek(&self, suite: Suite, digest: &Digest) -> Zeroizing<[u8; KEY_LEN]> {
        derive_cek(suite, &self.dek, digest)
    }

    /// Build a session directly from a raw DEK (test/helper use; normal callers go through
    /// [`unlock`](EncryptionMeta::unlock) or [`create`](EncryptionMeta::create)).
    pub fn from_raw(dek: [u8; KEY_LEN], active_suite: Suite) -> Self {
        Self {
            dek: Zeroizing::new(dek),
            active_suite,
        }
    }
}

/// The source that holds or derives the key-encrypting key (KEK) for one wrap of the DEK.
/// Current create paths write [`WrapSource::Passphrase`] or [`WrapSource::RawKek`]. The other variants
/// reserve stable on-disk codes for host-managed unlock sources.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum WrapSource {
    /// A passphrase, stretched by the profile KDF (Argon2id or PBKDF2) - the only v1 source.
    Passphrase,
    /// An OS keychain / secret-service item that releases or wraps a KEK.
    Keystore,
    /// A secure-enclave non-exportable key that wraps or unwraps the DEK.
    SecureEnclave,
    /// A WebAuthn passkey PRF output used as the KEK (`params` carries credential id + PRF salt).
    Passkey,
    /// A cloud KMS envelope (`params` carries the key id or ARN).
    Kms,
    /// A caller-supplied raw 256-bit KEK (advanced / testing).
    RawKek,
    /// A TPM-backed key that wraps or unwraps the DEK.
    Tpm,
    /// An HSM-backed envelope (`params` carries the key id or PKCS#11 locator).
    Hsm,
}

impl WrapSource {
    /// The stable 1-byte on-disk code.
    pub const fn code(self) -> u8 {
        match self {
            WrapSource::Passphrase => 0x01,
            WrapSource::Keystore => 0x02,
            WrapSource::SecureEnclave => 0x03,
            WrapSource::Passkey => 0x04,
            WrapSource::Kms => 0x05,
            WrapSource::RawKek => 0x06,
            WrapSource::Tpm => 0x07,
            WrapSource::Hsm => 0x08,
        }
    }
    /// Parse the on-disk code; unknown codes are a forward-version / corrupt descriptor.
    pub fn from_code(code: u8) -> Result<Self> {
        match code {
            0x01 => Ok(WrapSource::Passphrase),
            0x02 => Ok(WrapSource::Keystore),
            0x03 => Ok(WrapSource::SecureEnclave),
            0x04 => Ok(WrapSource::Passkey),
            0x05 => Ok(WrapSource::Kms),
            0x06 => Ok(WrapSource::RawKek),
            0x07 => Ok(WrapSource::Tpm),
            0x08 => Ok(WrapSource::Hsm),
            other => Err(LoomError::corrupt(format!(
                "unknown wrap source {other:#04x}"
            ))),
        }
    }
}

/// One way to unwrap the DEK: a [`WrapSource`], the DEK-wrap pairing (`wrap_alg` selects the wrap AEAD +
/// KDF), the KDF salt + wrap nonce + wrapped DEK, and an opaque per-source `params`
/// blob (e.g. a key id, a passkey credential-id + PRF salt, a KMS ARN). Several entries can unwrap the
/// same DEK; current create paths write one entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrapEntry {
    /// Which source holds/derives this entry's KEK.
    pub source: WrapSource,
    /// DEK-wrap algorithm id, which also selects the passphrase KDF:
    /// [`WRAP_ALG_XCHACHA20POLY1305`] is XChaCha wrap + Argon2id, [`WRAP_ALG_AES256GCM`] is AES-GCM + PBKDF2.
    pub wrap_alg: u8,
    /// Salt for the master/passphrase KDF (empty for sources that supply a KEK directly).
    pub kdf_salt: Vec<u8>,
    /// Nonce used to wrap the DEK under this entry's KEK.
    pub wrap_nonce: Vec<u8>,
    /// The wrapped DEK: `ciphertext || tag` of the 32-byte DEK under this entry's KEK.
    pub wrapped_dek: Vec<u8>,
    /// Opaque source-specific parameters needed to recover the KEK (key id, passkey credential-id + PRF
    /// salt, KMS ARN, ...). Empty for a passphrase entry.
    pub params: Vec<u8>,
}

/// The per-Loom encryption metadata stored in the superblock: the active object suite
/// plus one or more [`WrapEntry`]s, any of which can unlock the DEK. The master key is never stored;
/// only the **wrapped** DEK is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncryptionMeta {
    /// Active object AEAD suite for new writes.
    pub active_suite: Suite,
    /// The wraps of the DEK; at least one.
    pub wraps: Vec<WrapEntry>,
}

impl EncryptionMeta {
    /// Create a new encrypted Loom's metadata + an unlocked session, wrapping the caller-supplied random
    /// `dek` under the KEK that `spec` yields. With a **passphrase** the KEK is stretched from
    /// `salt` under the profile's KDF; with a **raw KEK** the 256-bit key is used
    /// directly (`salt` is ignored and the entry's `kdf_salt` is empty). The DEK-wrap AEAD + passphrase
    /// KDF follow the identity profile: FIPS uses AES-256-GCM + PBKDF2, the default
    /// XChaCha20-Poly1305 + Argon2id. The caller supplies all randomness; for a passphrase `salt` >= 8
    /// bytes, and `wrap_nonce` is at least the wrap AEAD's nonce length (24 XChaCha / 12 AES-GCM) - the
    /// leading bytes are used, so a caller can keep supplying 24 random bytes for either profile. The
    /// recorded [`WrapSource`] reflects the credential (passphrase -> `Passphrase`, raw KEK -> `RawKek`).
    pub fn create(
        spec: &KeySpec,
        active_suite: Suite,
        salt: Vec<u8>,
        dek: [u8; KEY_LEN],
        wrap_nonce: Vec<u8>,
    ) -> Result<(Self, DekSession)> {
        let source = spec.wrap_source();
        let (wrap_alg, wrap_suite, kdf) = wrap_pairing(active_suite);
        // A passphrase KDF needs a salt; a raw KEK ignores it (and stores an empty `kdf_salt`).
        let kdf_salt = if source == WrapSource::Passphrase {
            if salt.len() < 8 {
                return Err(LoomError::invalid("kdf salt must be at least 8 bytes"));
            }
            salt
        } else {
            Vec::new()
        };
        if wrap_nonce.len() < wrap_suite.nonce_len() {
            return Err(LoomError::invalid(format!(
                "wrap nonce must be at least {} bytes for {}",
                wrap_suite.nonce_len(),
                wrap_suite.as_str()
            )));
        }
        let wrap_nonce = wrap_nonce[..wrap_suite.nonce_len()].to_vec();
        let master = master_key(spec, &kdf_salt, kdf)?;
        let wrapped_dek = seal(wrap_suite, &master, &wrap_nonce, WRAP_AAD, &dek)?;
        let meta = Self {
            active_suite,
            wraps: vec![WrapEntry {
                source,
                wrap_alg,
                kdf_salt,
                wrap_nonce,
                wrapped_dek,
                params: Vec::new(),
            }],
        };
        Ok((meta, DekSession::from_raw(dek, active_suite)))
    }

    /// Unlock the DEK from a credential, trying each wrap entry the credential can open:
    /// a passphrase tries `Passphrase` entries; a raw KEK tries every external
    /// (non-passphrase) entry. The first that unwraps wins. A wrong credential (or tampered wrap), or no
    /// matching entry, returns [`Code::E2eKeyInvalid`].
    pub fn unlock(&self, spec: &KeySpec) -> Result<DekSession> {
        let mut last_err = LoomError::new(Code::E2eKeyInvalid, "no wrap entry unlocked the DEK");
        for entry in &self.wraps {
            if !spec.unlocks(entry.source) {
                continue;
            }
            match self.try_unwrap(entry, spec) {
                Ok(dek) => return Ok(DekSession::from_raw(dek, self.active_suite)),
                Err(e) => last_err = e,
            }
        }
        Err(last_err)
    }

    /// Attempt to unwrap one entry; the wrap AEAD + KDF come from its `wrap_alg` (a raw-KEK credential
    /// bypasses the KDF in [`master_key`]).
    fn try_unwrap(&self, entry: &WrapEntry, spec: &KeySpec) -> Result<[u8; KEY_LEN]> {
        let (wrap_suite, kdf) = wrap_pairing_from_alg(entry.wrap_alg)?;
        let master = master_key(spec, &entry.kdf_salt, kdf)?;
        let dek_bytes = unseal(
            wrap_suite,
            &master,
            &entry.wrap_nonce,
            WRAP_AAD,
            &entry.wrapped_dek,
        )?;
        if dek_bytes.len() != KEY_LEN {
            return Err(LoomError::new(
                Code::E2eKeyInvalid,
                "unwrapped DEK has wrong length",
            ));
        }
        let mut dek = [0u8; KEY_LEN];
        dek.copy_from_slice(&dek_bytes);
        Ok(dek)
    }

    /// Re-wrap the **same** DEK under a new credential (the cheap rekey - it does not
    /// re-seal objects). `new_spec` may be a passphrase or a raw KEK; the
    /// returned descriptor has a single wrap whose source matches it (any prior wraps are dropped -
    /// re-attaching external wraps is an explicit, additive step). The caller supplies a fresh `salt`
    /// (used only for a passphrase) and `wrap_nonce`. The active object suite is unchanged, so a FIPS
    /// store keeps its AES-256-GCM wrap + PBKDF2 pairing.
    pub fn rewrap(
        session: &DekSession,
        new_spec: &KeySpec,
        salt: Vec<u8>,
        wrap_nonce: Vec<u8>,
    ) -> Result<Self> {
        let source = new_spec.wrap_source();
        let (wrap_alg, wrap_suite, kdf) = wrap_pairing(session.active_suite);
        let kdf_salt = if source == WrapSource::Passphrase {
            if salt.len() < 8 {
                return Err(LoomError::invalid("kdf salt must be at least 8 bytes"));
            }
            salt
        } else {
            Vec::new()
        };
        if wrap_nonce.len() < wrap_suite.nonce_len() {
            return Err(LoomError::invalid(format!(
                "wrap nonce must be at least {} bytes for {}",
                wrap_suite.nonce_len(),
                wrap_suite.as_str()
            )));
        }
        let wrap_nonce = wrap_nonce[..wrap_suite.nonce_len()].to_vec();
        let master = master_key(new_spec, &kdf_salt, kdf)?;
        let wrapped_dek = seal(wrap_suite, &master, &wrap_nonce, WRAP_AAD, &session.dek[..])?;
        Ok(Self {
            active_suite: session.active_suite,
            wraps: vec![WrapEntry {
                source,
                wrap_alg,
                kdf_salt,
                wrap_nonce,
                wrapped_dek,
                params: Vec::new(),
            }],
        })
    }

    fn wrap_for_session(
        session: &DekSession,
        spec: &KeySpec,
        salt: Vec<u8>,
        wrap_nonce: Vec<u8>,
    ) -> Result<WrapEntry> {
        let source = spec.wrap_source();
        let (wrap_alg, wrap_suite, kdf) = wrap_pairing(session.active_suite);
        let kdf_salt = if source == WrapSource::Passphrase {
            if salt.len() < 8 {
                return Err(LoomError::invalid("kdf salt must be at least 8 bytes"));
            }
            salt
        } else {
            Vec::new()
        };
        if wrap_nonce.len() < wrap_suite.nonce_len() {
            return Err(LoomError::invalid(format!(
                "wrap nonce must be at least {} bytes for {}",
                wrap_suite.nonce_len(),
                wrap_suite.as_str()
            )));
        }
        let wrap_nonce = wrap_nonce[..wrap_suite.nonce_len()].to_vec();
        let master = master_key(spec, &kdf_salt, kdf)?;
        let wrapped_dek = seal(wrap_suite, &master, &wrap_nonce, WRAP_AAD, &session.dek[..])?;
        Ok(WrapEntry {
            source,
            wrap_alg,
            kdf_salt,
            wrap_nonce,
            wrapped_dek,
            params: Vec::new(),
        })
    }

    fn has_passphrase_wrap(wraps: &[WrapEntry]) -> bool {
        wraps.iter().any(|w| w.source == WrapSource::Passphrase)
    }

    fn has_external_wrap(wraps: &[WrapEntry]) -> bool {
        wraps.iter().any(|w| w.source != WrapSource::Passphrase)
    }

    fn enforce_recovery_policy(wraps: &[WrapEntry], allow_no_recovery: bool) -> Result<()> {
        if !allow_no_recovery && Self::has_external_wrap(wraps) && !Self::has_passphrase_wrap(wraps)
        {
            return Err(LoomError::new(
                Code::InvalidArgument,
                "external key-source wraps require a passphrase recovery wrap unless no-recovery is explicitly allowed",
            ));
        }
        Ok(())
    }

    /// Append another credential that unwraps the same DEK. The existing session proves the caller has
    /// already unlocked the store. External credentials require a passphrase recovery wrap unless
    /// `allow_no_recovery` is set.
    pub fn add_wrap(
        &self,
        session: &DekSession,
        new_spec: &KeySpec,
        salt: Vec<u8>,
        wrap_nonce: Vec<u8>,
        allow_no_recovery: bool,
    ) -> Result<Self> {
        if session.active_suite != self.active_suite {
            return Err(LoomError::invalid(
                "active suite mismatch between encryption metadata and DEK session",
            ));
        }
        if self.unlock(new_spec).is_ok() {
            return Err(LoomError::new(
                Code::AlreadyExists,
                "wrap credential already exists",
            ));
        }
        let mut wraps = self.wraps.clone();
        let entry = Self::wrap_for_session(session, new_spec, salt, wrap_nonce)?;
        wraps.push(entry);
        Self::enforce_recovery_policy(&wraps, allow_no_recovery)?;
        Ok(Self {
            active_suite: self.active_suite,
            wraps,
        })
    }

    /// Remove one wrap by zero-based index. At least one unlock path must remain, and external-only
    /// metadata is rejected unless `allow_no_recovery` is set.
    pub fn remove_wrap(&self, index: usize, allow_no_recovery: bool) -> Result<Self> {
        if index >= self.wraps.len() {
            return Err(LoomError::new(
                Code::NotFound,
                format!("wrap index {index} does not exist"),
            ));
        }
        if self.wraps.len() == 1 {
            return Err(LoomError::new(
                Code::InvalidArgument,
                "cannot remove the last encryption wrap",
            ));
        }
        let mut wraps = self.wraps.clone();
        wraps.remove(index);
        Self::enforce_recovery_policy(&wraps, allow_no_recovery)?;
        Ok(Self {
            active_suite: self.active_suite,
            wraps,
        })
    }

    /// Encode to the compact superblock record as a versioned, count-prefixed list of source-tagged wraps:
    /// `magic || 2 || suite || u16{n} || [ source || wrap_alg || u16-len{salt, nonce, wrapped, params} ]*`.
    /// Small enough for the superblock's reserved span; the caller places it under the slot CRC.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(96);
        out.extend_from_slice(META_MAGIC);
        out.push(META_VERSION);
        out.push(self.active_suite.id());
        out.extend_from_slice(&(self.wraps.len() as u16).to_be_bytes());
        for w in &self.wraps {
            out.push(w.source.code());
            out.push(w.wrap_alg);
            for field in [&w.kdf_salt, &w.wrap_nonce, &w.wrapped_dek, &w.params] {
                out.extend_from_slice(&(field.len() as u16).to_be_bytes());
                out.extend_from_slice(field);
            }
        }
        out
    }

    /// Decode [`encode`](Self::encode). A bad magic, unknown version, bad suite, or truncated buffer
    /// is a clean [`Code::CorruptObject`].
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let corrupt = || LoomError::corrupt("corrupt encryption_meta");
        if bytes.len() < 6 || &bytes[0..4] != META_MAGIC {
            return Err(corrupt());
        }
        let version = bytes[4];
        let active_suite = Suite::from_id(bytes[5]).map_err(|_| corrupt())?;
        let mut pos = 6;
        let take = |pos: &mut usize| -> Result<Vec<u8>> {
            if *pos + 2 > bytes.len() {
                return Err(corrupt());
            }
            let len = u16::from_be_bytes([bytes[*pos], bytes[*pos + 1]]) as usize;
            *pos += 2;
            if *pos + len > bytes.len() {
                return Err(corrupt());
            }
            let v = bytes[*pos..*pos + len].to_vec();
            *pos += len;
            Ok(v)
        };
        let wraps = match version {
            META_VERSION => {
                if pos + 2 > bytes.len() {
                    return Err(corrupt());
                }
                let n = u16::from_be_bytes([bytes[pos], bytes[pos + 1]]) as usize;
                pos += 2;
                let mut wraps = Vec::with_capacity(n);
                for _ in 0..n {
                    if pos + 2 > bytes.len() {
                        return Err(corrupt());
                    }
                    let source = WrapSource::from_code(bytes[pos])?;
                    let wrap_alg = bytes[pos + 1];
                    pos += 2;
                    let kdf_salt = take(&mut pos)?;
                    let wrap_nonce = take(&mut pos)?;
                    let wrapped_dek = take(&mut pos)?;
                    let params = take(&mut pos)?;
                    wraps.push(WrapEntry {
                        source,
                        wrap_alg,
                        kdf_salt,
                        wrap_nonce,
                        wrapped_dek,
                        params,
                    });
                }
                wraps
            }
            other => {
                return Err(LoomError::corrupt(format!(
                    "unsupported encryption_meta version {other}"
                )));
            }
        };
        if wraps.is_empty() {
            return Err(corrupt());
        }
        Ok(Self {
            active_suite,
            wraps,
        })
    }
}

#[cfg(test)]
mod tests;
