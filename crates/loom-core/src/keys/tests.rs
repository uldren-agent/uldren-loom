use super::*;

fn pass(s: &str) -> KeySpec {
    KeySpec::Passphrase(Zeroizing::new(s.to_string()))
}

// Deterministic, fixed test inputs (no RNG in the key layer - randomness is the caller's job).
const SALT: [u8; 16] = [7u8; 16];
const DEK: [u8; KEY_LEN] = [0x42; KEY_LEN];
const WRAP_NONCE: [u8; 24] = [9u8; 24];

#[test]
fn argon2_kdf_is_deterministic_and_salt_sensitive() {
    let a = argon2id_master(b"correct horse", &SALT).unwrap();
    let b = argon2id_master(b"correct horse", &SALT).unwrap();
    assert_eq!(
        a.as_slice(),
        b.as_slice(),
        "same passphrase+salt -> same key"
    );
    let c = argon2id_master(b"correct horse", &[8u8; 16]).unwrap();
    assert_ne!(
        a.as_slice(),
        c.as_slice(),
        "different salt -> different key"
    );
    let d = argon2id_master(b"wrong horse", &SALT).unwrap();
    assert_ne!(
        a.as_slice(),
        d.as_slice(),
        "different passphrase -> different key"
    );
}

#[test]
fn dek_wraps_and_unlocks_round_trip() {
    let (meta, session) = EncryptionMeta::create(
        &pass("pw"),
        Suite::XChaCha20Poly1305,
        SALT.to_vec(),
        DEK,
        WRAP_NONCE.to_vec(),
    )
    .unwrap();
    assert_eq!(session.active_suite(), Suite::XChaCha20Poly1305);
    let unlocked = meta.unlock(&pass("pw")).unwrap();
    // The unlocked DEK derives identical CEKs to the original session.
    let digest = Digest::blake3(b"obj");
    assert_eq!(
        session
            .derive_cek(Suite::XChaCha20Poly1305, &digest)
            .as_slice(),
        unlocked
            .derive_cek(Suite::XChaCha20Poly1305, &digest)
            .as_slice()
    );
}

#[test]
fn wrong_passphrase_is_e2e_key_invalid() {
    let (meta, _) = EncryptionMeta::create(
        &pass("right"),
        Suite::XChaCha20Poly1305,
        SALT.to_vec(),
        DEK,
        WRAP_NONCE.to_vec(),
    )
    .unwrap();
    let err = meta.unlock(&pass("wrong")).unwrap_err();
    assert_eq!(
        err.code,
        Code::E2eKeyInvalid,
        "wrong passphrase must not unwrap"
    );
}

#[test]
fn corrupted_metadata_fails_cleanly() {
    let (meta, _) = EncryptionMeta::create(
        &pass("pw"),
        Suite::XChaCha20Poly1305,
        SALT.to_vec(),
        DEK,
        WRAP_NONCE.to_vec(),
    )
    .unwrap();
    // A flipped byte in the wrapped DEK fails the wrap AEAD, not a panic.
    let mut m = meta.clone();
    *m.wraps[0].wrapped_dek.last_mut().unwrap() ^= 0x01;
    assert_eq!(m.unlock(&pass("pw")).unwrap_err().code, Code::E2eKeyInvalid);
    // A truncated/garbled record decodes to a clean corruption error.
    assert_eq!(
        EncryptionMeta::decode(b"nope").unwrap_err().code,
        Code::CorruptObject
    );
}

#[test]
fn encryption_meta_round_trips_through_its_codec() {
    let fips_kek = KeySpec::raw_kek([0x11; KEY_LEN]);
    for (spec, suite) in [
        (pass("pw"), Suite::XChaCha20Poly1305),
        (fips_kek, Suite::Aes256Gcm),
    ] {
        let (meta, _) =
            EncryptionMeta::create(&spec, suite, SALT.to_vec(), DEK, WRAP_NONCE.to_vec()).unwrap();
        let decoded = EncryptionMeta::decode(&meta.encode()).unwrap();
        assert_eq!(decoded, meta);
        assert_eq!(decoded.active_suite, suite);
    }
}

#[test]
#[ignore = "manual production-cost PBKDF2 validation"]
fn pbkdf2_kdf_is_deterministic_and_salt_sensitive() {
    let a = pbkdf2_master(b"correct horse", &SALT);
    let b = pbkdf2_master(b"correct horse", &SALT);
    assert_eq!(
        a.as_slice(),
        b.as_slice(),
        "same passphrase+salt -> same key"
    );
    assert_ne!(
        a.as_slice(),
        pbkdf2_master(b"correct horse", &[8u8; 16]).as_slice(),
        "different salt -> different key"
    );
    assert_ne!(
        a.as_slice(),
        argon2id_master(b"correct horse", &SALT).unwrap().as_slice(),
        "PBKDF2 and Argon2id produce different keys from the same input"
    );
}

#[test]
fn fips_wrap_pairing_records_the_pbkdf2_contract() {
    assert_eq!(PBKDF2_ITERS, 600_000);
    assert_eq!(
        wrap_pairing(Suite::Aes256Gcm),
        (WRAP_ALG_AES256GCM, Suite::Aes256Gcm, Kdf::Pbkdf2Sha256)
    );
    assert_eq!(
        wrap_pairing_from_alg(WRAP_ALG_AES256GCM).unwrap(),
        (Suite::Aes256Gcm, Kdf::Pbkdf2Sha256)
    );
}

/// The FIPS profile (AES-256-GCM objects) wraps the DEK with AES-256-GCM under a PBKDF2 master key,
/// recording `wrap_alg = 0x02` and a 12-byte AES nonce - no XChaCha/Argon2id in the key path.
/// The default profile uses `wrap_alg = 0x01` + a 24-byte XChaCha nonce.
#[test]
#[ignore = "manual production-cost PBKDF2 validation"]
fn fips_profile_uses_aes_wrap_and_pbkdf2() {
    let (fips, _) = EncryptionMeta::create(
        &pass("pw"),
        Suite::Aes256Gcm,
        SALT.to_vec(),
        DEK,
        WRAP_NONCE.to_vec(),
    )
    .unwrap();
    assert_eq!(fips.wraps[0].source, WrapSource::Passphrase);
    assert_eq!(fips.wraps[0].wrap_alg, WRAP_ALG_AES256GCM);
    assert_eq!(
        fips.wraps[0].wrap_nonce.len(),
        Suite::Aes256Gcm.nonce_len(),
        "12-byte AES nonce"
    );
    assert_eq!(
        fips.unlock(&pass("pw")).unwrap().active_suite(),
        Suite::Aes256Gcm
    );

    let (default, _) = EncryptionMeta::create(
        &pass("pw"),
        Suite::XChaCha20Poly1305,
        SALT.to_vec(),
        DEK,
        WRAP_NONCE.to_vec(),
    )
    .unwrap();
    assert_eq!(default.wraps[0].wrap_alg, WRAP_ALG_XCHACHA20POLY1305);
    assert_eq!(
        default.wraps[0].wrap_nonce.len(),
        24,
        "default wrap nonce unchanged (24-byte XChaCha)"
    );
    default.unlock(&pass("pw")).unwrap();

    // A rekey (re-wrap) of a FIPS store keeps the AES wrap + PBKDF2 pairing.
    let session = fips.unlock(&pass("pw")).unwrap();
    let rewrapped = EncryptionMeta::rewrap(
        &session,
        &pass("new"),
        [5u8; 16].to_vec(),
        WRAP_NONCE.to_vec(),
    )
    .unwrap();
    assert_eq!(rewrapped.wraps[0].wrap_alg, WRAP_ALG_AES256GCM);
    assert_eq!(
        rewrapped.unlock(&pass("new")).unwrap().active_suite(),
        Suite::Aes256Gcm
    );
}

#[test]
fn fips_profile_uses_aes_wrap_with_host_supplied_kek() {
    let kek = KeySpec::raw_kek([0x11; KEY_LEN]);
    let (fips, _) = EncryptionMeta::create(
        &kek,
        Suite::Aes256Gcm,
        SALT.to_vec(),
        DEK,
        WRAP_NONCE.to_vec(),
    )
    .unwrap();
    assert_eq!(fips.wraps[0].source, WrapSource::RawKek);
    assert_eq!(fips.wraps[0].wrap_alg, WRAP_ALG_AES256GCM);
    assert_eq!(
        fips.wraps[0].wrap_nonce.len(),
        Suite::Aes256Gcm.nonce_len(),
        "12-byte AES nonce"
    );
    assert_eq!(fips.unlock(&kek).unwrap().active_suite(), Suite::Aes256Gcm);

    let session = fips.unlock(&kek).unwrap();
    let new_kek = KeySpec::raw_kek([0x22; KEY_LEN]);
    let rewrapped =
        EncryptionMeta::rewrap(&session, &new_kek, [5u8; 16].to_vec(), WRAP_NONCE.to_vec())
            .unwrap();
    assert_eq!(rewrapped.wraps[0].wrap_alg, WRAP_ALG_AES256GCM);
    assert_eq!(
        rewrapped.unlock(&new_kek).unwrap().active_suite(),
        Suite::Aes256Gcm
    );
}

/// The descriptor round-trips a multi-wrap with source-tagged entries.
#[test]
fn encryption_meta_multiwrap_round_trips_and_legacy_version_is_rejected() {
    // Build a 2-entry descriptor: the real passphrase wrap plus a reserved passkey entry.
    let (mut meta, _) = EncryptionMeta::create(
        &pass("pw"),
        Suite::XChaCha20Poly1305,
        SALT.to_vec(),
        DEK,
        WRAP_NONCE.to_vec(),
    )
    .unwrap();
    meta.wraps.push(WrapEntry {
        source: WrapSource::Passkey,
        wrap_alg: WRAP_ALG_AES256GCM,
        kdf_salt: Vec::new(),
        wrap_nonce: vec![1u8; 12],
        wrapped_dek: vec![2u8; 48],
        params: b"cred-id|prf-salt".to_vec(),
    });
    let decoded = EncryptionMeta::decode(&meta.encode()).unwrap();
    assert_eq!(decoded, meta, "v2 multi-wrap round-trips");
    assert_eq!(decoded.wraps.len(), 2);
    assert_eq!(decoded.wraps[1].source, WrapSource::Passkey);
    assert_eq!(decoded.wraps[1].params, b"cred-id|prf-salt");
    // The passphrase still unlocks despite the extra (non-passphrase) entry.
    decoded.unlock(&pass("pw")).unwrap();

    // A hand-built legacy record (magic||1||suite||wrap_alg||u16{salt,nonce,wrapped}) is rejected.
    let mut v1 = Vec::new();
    v1.extend_from_slice(META_MAGIC);
    v1.push(1);
    v1.push(Suite::XChaCha20Poly1305.id());
    v1.push(WRAP_ALG_XCHACHA20POLY1305);
    for f in [&SALT[..], &WRAP_NONCE[..], &[0u8; 48][..]] {
        v1.extend_from_slice(&(f.len() as u16).to_be_bytes());
        v1.extend_from_slice(f);
    }
    assert_eq!(
        EncryptionMeta::decode(&v1).unwrap_err().code,
        Code::CorruptObject
    );
}

#[test]
fn wrap_source_provider_taxonomy_has_distinct_stable_codes() {
    let sources = [
        (WrapSource::Passphrase, 0x01),
        (WrapSource::Keystore, 0x02),
        (WrapSource::SecureEnclave, 0x03),
        (WrapSource::Passkey, 0x04),
        (WrapSource::Kms, 0x05),
        (WrapSource::RawKek, 0x06),
        (WrapSource::Tpm, 0x07),
        (WrapSource::Hsm, 0x08),
    ];
    for (source, code) in sources {
        assert_eq!(source.code(), code);
        assert_eq!(WrapSource::from_code(code).unwrap(), source);
    }

    let (mut meta, _) = EncryptionMeta::create(
        &pass("pw"),
        Suite::XChaCha20Poly1305,
        SALT.to_vec(),
        DEK,
        WRAP_NONCE.to_vec(),
    )
    .unwrap();
    meta.wraps.push(WrapEntry {
        source: WrapSource::Tpm,
        wrap_alg: WRAP_ALG_AES256GCM,
        kdf_salt: Vec::new(),
        wrap_nonce: vec![3u8; 12],
        wrapped_dek: vec![4u8; 48],
        params: b"tpm-key".to_vec(),
    });
    meta.wraps.push(WrapEntry {
        source: WrapSource::Hsm,
        wrap_alg: WRAP_ALG_AES256GCM,
        kdf_salt: Vec::new(),
        wrap_nonce: vec![5u8; 12],
        wrapped_dek: vec![6u8; 48],
        params: b"pkcs11:slot=1;label=loom".to_vec(),
    });

    let decoded = EncryptionMeta::decode(&meta.encode()).unwrap();
    assert_eq!(decoded.wraps[1].source, WrapSource::Tpm);
    assert_eq!(decoded.wraps[2].source, WrapSource::Hsm);
    assert_eq!(decoded, meta);
}

/// A caller-supplied 256-bit KEK wraps the DEK directly (no KDF), recorded as a `RawKek` entry.
/// The same KEK unlocks; a wrong KEK or a passphrase does not; and a raw KEK does not unlock a
/// passphrase store. Works under the FIPS profile (AES-256-GCM wrap) too.
#[test]
fn raw_kek_wraps_and_unlocks_without_a_kdf() {
    let kek = [0x5au8; KEY_LEN];
    for suite in [Suite::XChaCha20Poly1305, Suite::Aes256Gcm] {
        let (meta, session) = EncryptionMeta::create(
            &KeySpec::raw_kek(kek),
            suite,
            Vec::new(), // salt ignored for a raw KEK
            DEK,
            WRAP_NONCE.to_vec(),
        )
        .unwrap();
        assert_eq!(meta.wraps[0].source, WrapSource::RawKek);
        assert!(
            meta.wraps[0].kdf_salt.is_empty(),
            "raw KEK stores no KDF salt"
        );
        // The right KEK unlocks and yields the same DEK.
        let unlocked = meta.unlock(&KeySpec::raw_kek(kek)).unwrap();
        let digest = Digest::blake3(b"obj");
        assert_eq!(
            unlocked.derive_cek(suite, &digest).as_slice(),
            session.derive_cek(suite, &digest).as_slice()
        );
        // A wrong KEK fails the AEAD.
        assert_eq!(
            meta.unlock(&KeySpec::raw_kek([0u8; KEY_LEN]))
                .unwrap_err()
                .code,
            Code::E2eKeyInvalid
        );
        // A passphrase does not unlock a raw-KEK store (no passphrase entry).
        assert_eq!(
            meta.unlock(&pass("pw")).unwrap_err().code,
            Code::E2eKeyInvalid
        );
        // The round-trip survives the codec (RawKek source + empty salt + params encode/decode).
        assert_eq!(EncryptionMeta::decode(&meta.encode()).unwrap(), meta);
    }
    // Symmetrically, a raw KEK does not unlock a passphrase store.
    let (pw_meta, _) = EncryptionMeta::create(
        &pass("pw"),
        Suite::XChaCha20Poly1305,
        SALT.to_vec(),
        DEK,
        WRAP_NONCE.to_vec(),
    )
    .unwrap();
    assert_eq!(
        pw_meta.unlock(&KeySpec::raw_kek(kek)).unwrap_err().code,
        Code::E2eKeyInvalid
    );
}

#[test]
fn seal_unseal_round_trip_and_tamper_detection() {
    for suite in [Suite::XChaCha20Poly1305, Suite::Aes256Gcm] {
        let digest = Digest::blake3(b"payload-object");
        let session = DekSession::from_raw(DEK, suite);
        let cek = session.derive_cek(suite, &digest);
        let nonce = vec![3u8; suite.nonce_len()];
        let aad = b"frame-metadata";
        let pt = b"the quick brown fox jumps over the lazy dog";
        let sealed = seal(suite, &cek, &nonce, aad, pt).unwrap();
        assert_ne!(&sealed[..], &pt[..], "ciphertext differs from plaintext");
        // Correct open.
        assert_eq!(
            unseal(suite, &cek, &nonce, aad, &sealed)
                .unwrap()
                .as_slice(),
            pt
        );
        // Tampered ciphertext fails.
        let mut bad = sealed.clone();
        bad[0] ^= 0x01;
        assert_eq!(
            unseal(suite, &cek, &nonce, aad, &bad).unwrap_err().code,
            Code::E2eKeyInvalid
        );
        // Altered associated data fails (binding holds).
        assert_eq!(
            unseal(suite, &cek, &nonce, b"other-aad", &sealed)
                .unwrap_err()
                .code,
            Code::E2eKeyInvalid
        );
        // Wrong nonce fails.
        let other_nonce = vec![4u8; suite.nonce_len()];
        assert_eq!(
            unseal(suite, &cek, &other_nonce, aad, &sealed)
                .unwrap_err()
                .code,
            Code::E2eKeyInvalid
        );
    }
}

#[test]
fn suite_chooses_the_cek_kdf_so_keys_differ() {
    // The same DEK + digest under different suites must derive different CEKs (different KDFs:
    // keyed-BLAKE3 vs HKDF-SHA-256), so the FIPS suite shares no key material derivation with the
    // default.
    let digest = Digest::blake3(b"x");
    let session = DekSession::from_raw(DEK, Suite::XChaCha20Poly1305);
    let xchacha = session.derive_cek(Suite::XChaCha20Poly1305, &digest);
    let aes = session.derive_cek(Suite::Aes256Gcm, &digest);
    assert_ne!(xchacha.as_slice(), aes.as_slice());
    // And a wrong-suite open of an XChaCha frame with an AES CEK fails (no cross-suite confusion).
    let nonce = vec![1u8; Suite::XChaCha20Poly1305.nonce_len()];
    let sealed = seal(Suite::XChaCha20Poly1305, &xchacha, &nonce, b"", b"hi").unwrap();
    // Opening under the wrong suite/nonce-length is rejected.
    assert!(unseal(Suite::Aes256Gcm, &aes, &[1u8; 12], b"", &sealed).is_err());
}

#[test]
fn rekey_rewraps_same_dek_under_new_passphrase() {
    let (_meta, session) = EncryptionMeta::create(
        &pass("old"),
        Suite::XChaCha20Poly1305,
        SALT.to_vec(),
        DEK,
        WRAP_NONCE.to_vec(),
    )
    .unwrap();
    let rewrapped = EncryptionMeta::rewrap(
        &session,
        &pass("new"),
        [1u8; 16].to_vec(),
        [2u8; 24].to_vec(),
    )
    .unwrap();
    // Old passphrase no longer unlocks the rewrapped meta; the new one does, yielding the same DEK
    // (objects are not re-sealed).
    assert_eq!(
        rewrapped.unlock(&pass("old")).unwrap_err().code,
        Code::E2eKeyInvalid
    );
    let unlocked = rewrapped.unlock(&pass("new")).unwrap();
    let digest = Digest::blake3(b"obj");
    assert_eq!(
        unlocked
            .derive_cek(Suite::XChaCha20Poly1305, &digest)
            .as_slice(),
        session
            .derive_cek(Suite::XChaCha20Poly1305, &digest)
            .as_slice(),
        "rekey must preserve the DEK so existing objects still open"
    );
}

#[test]
fn add_and_remove_wrap_enforces_recovery_policy() {
    let (meta, session) = EncryptionMeta::create(
        &pass("pw"),
        Suite::XChaCha20Poly1305,
        SALT.to_vec(),
        DEK,
        WRAP_NONCE.to_vec(),
    )
    .unwrap();
    let kek = [0x5au8; KEY_LEN];
    let multi = meta
        .add_wrap(
            &session,
            &KeySpec::raw_kek(kek),
            Vec::new(),
            [3u8; 24].to_vec(),
            false,
        )
        .unwrap();
    assert_eq!(multi.wraps.len(), 2);
    assert_eq!(multi.wraps[0].source, WrapSource::Passphrase);
    assert_eq!(multi.wraps[1].source, WrapSource::RawKek);
    multi.unlock(&pass("pw")).unwrap();
    multi.unlock(&KeySpec::raw_kek(kek)).unwrap();

    let external_only = multi.remove_wrap(0, false).unwrap_err();
    assert_eq!(external_only.code, Code::InvalidArgument);
    let external_only = multi.remove_wrap(0, true).unwrap();
    assert_eq!(external_only.wraps.len(), 1);
    assert_eq!(external_only.wraps[0].source, WrapSource::RawKek);
    external_only.unlock(&KeySpec::raw_kek(kek)).unwrap();
    assert_eq!(
        external_only.remove_wrap(0, true).unwrap_err().code,
        Code::InvalidArgument
    );
}

#[test]
fn adding_external_wrap_without_recovery_needs_override() {
    let kek_a = [0x11u8; KEY_LEN];
    let kek_b = [0x22u8; KEY_LEN];
    let (meta, session) = EncryptionMeta::create(
        &KeySpec::raw_kek(kek_a),
        Suite::XChaCha20Poly1305,
        Vec::new(),
        DEK,
        WRAP_NONCE.to_vec(),
    )
    .unwrap();
    assert_eq!(
        meta.add_wrap(
            &session,
            &KeySpec::raw_kek(kek_b),
            Vec::new(),
            [4u8; 24].to_vec(),
            false,
        )
        .unwrap_err()
        .code,
        Code::InvalidArgument
    );
    let multi = meta
        .add_wrap(
            &session,
            &KeySpec::raw_kek(kek_b),
            Vec::new(),
            [4u8; 24].to_vec(),
            true,
        )
        .unwrap();
    assert_eq!(multi.wraps.len(), 2);
    multi.unlock(&KeySpec::raw_kek(kek_a)).unwrap();
    multi.unlock(&KeySpec::raw_kek(kek_b)).unwrap();
}

#[test]
fn duplicate_passphrase_add_is_already_exists() {
    let (meta, session) = EncryptionMeta::create(
        &pass("pw"),
        Suite::XChaCha20Poly1305,
        SALT.to_vec(),
        DEK,
        WRAP_NONCE.to_vec(),
    )
    .unwrap();
    let err = meta
        .add_wrap(
            &session,
            &pass("pw"),
            SALT.to_vec(),
            [7u8; 24].to_vec(),
            false,
        )
        .unwrap_err();
    assert_eq!(err.code, Code::AlreadyExists);
}

#[test]
fn duplicate_raw_kek_add_is_already_exists() {
    let kek = [0x5au8; KEY_LEN];
    let (meta, session) = EncryptionMeta::create(
        &KeySpec::raw_kek(kek),
        Suite::XChaCha20Poly1305,
        Vec::new(),
        DEK,
        WRAP_NONCE.to_vec(),
    )
    .unwrap();
    let err = meta
        .add_wrap(
            &session,
            &KeySpec::raw_kek(kek),
            Vec::new(),
            [7u8; 24].to_vec(),
            true,
        )
        .unwrap_err();
    assert_eq!(err.code, Code::AlreadyExists);
}

#[test]
fn non_duplicate_credentials_still_add() {
    let (meta, session) = EncryptionMeta::create(
        &pass("pw"),
        Suite::XChaCha20Poly1305,
        SALT.to_vec(),
        DEK,
        WRAP_NONCE.to_vec(),
    )
    .unwrap();
    let two_pass = meta
        .add_wrap(
            &session,
            &pass("other"),
            SALT.to_vec(),
            [7u8; 24].to_vec(),
            false,
        )
        .unwrap();
    assert_eq!(two_pass.wraps.len(), 2);
    two_pass.unlock(&pass("pw")).unwrap();
    two_pass.unlock(&pass("other")).unwrap();

    let kek = [0x33u8; KEY_LEN];
    let with_kek = meta
        .add_wrap(
            &session,
            &KeySpec::raw_kek(kek),
            Vec::new(),
            [8u8; 24].to_vec(),
            false,
        )
        .unwrap();
    assert_eq!(with_kek.wraps.len(), 2);
    with_kek.unlock(&KeySpec::raw_kek(kek)).unwrap();
}

#[test]
fn recovery_policy_still_wins_for_non_duplicate_external_add() {
    let kek_a = [0x11u8; KEY_LEN];
    let kek_b = [0x22u8; KEY_LEN];
    let (meta, session) = EncryptionMeta::create(
        &KeySpec::raw_kek(kek_a),
        Suite::XChaCha20Poly1305,
        Vec::new(),
        DEK,
        WRAP_NONCE.to_vec(),
    )
    .unwrap();
    let err = meta
        .add_wrap(
            &session,
            &KeySpec::raw_kek(kek_b),
            Vec::new(),
            [9u8; 24].to_vec(),
            false,
        )
        .unwrap_err();
    assert_eq!(err.code, Code::InvalidArgument);
}
