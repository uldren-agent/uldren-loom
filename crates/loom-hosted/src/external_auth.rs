use std::time::SystemTime;

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use loom_core::error::{Code, LoomError, Result};
use loom_core::{Digest, ExternalCredentialKind};
use loom_store::VerifiedExternalCredential;
use serde::Deserialize;
use serde_json::Value;

pub fn verify_direct_external_credential(
    kind: ExternalCredentialKind,
    proof: &str,
    challenge: Option<&str>,
    peer_certificate_der: Option<&[u8]>,
) -> Result<VerifiedExternalCredential> {
    match kind {
        ExternalCredentialKind::PublicKey => verify_public_key(proof, challenge),
        ExternalCredentialKind::OidcSubject => verify_oidc(proof, challenge),
        ExternalCredentialKind::SamlSubject => verify_saml(proof, challenge),
        ExternalCredentialKind::Passkey => verify_passkey(proof, challenge),
        ExternalCredentialKind::MtlsCertificate => verify_mtls(proof, peer_certificate_der),
    }
}

fn verify_public_key(proof: &str, challenge: Option<&str>) -> Result<VerifiedExternalCredential> {
    let proof: PublicKeyProof = parse_proof(proof)?;
    let challenge = required_challenge(challenge)?;
    let public_key = decode_b64(&proof.public_key, "public_key")?;
    let signature = decode_b64(&proof.signature, "signature")?;
    verify_material_digest(&proof.material_digest, &public_key)?;
    let algorithm = proof.algorithm.as_deref().unwrap_or("ed25519");
    match algorithm {
        "ed25519" | "Ed25519" => {}
        other => {
            return Err(unsupported_direct(
                ExternalCredentialKind::PublicKey,
                format!("unsupported public-key algorithm {other}"),
            ));
        }
    }
    verify_ed25519_public_key(challenge.as_bytes(), &public_key, &signature)?;
    Ok(VerifiedExternalCredential {
        kind: ExternalCredentialKind::PublicKey,
        issuer: proof.issuer,
        subject: proof.subject,
        material_digest: Some(proof.material_digest),
        challenge_id: Some(parse_challenge_id(&proof.challenge_id)?),
    })
}

fn verify_oidc(proof: &str, challenge: Option<&str>) -> Result<VerifiedExternalCredential> {
    if cfg!(feature = "fips") {
        return Err(unsupported_direct(
            ExternalCredentialKind::OidcSubject,
            "FIPS direct OIDC proof requires a provider-backed verifier",
        ));
    }
    use openidconnect::core::{CoreIdToken, CoreIdTokenVerifier, CoreJsonWebKeySet};
    use openidconnect::{ClientId, IssuerUrl, Nonce};

    let proof: OidcProof = parse_proof(proof)?;
    verify_material_digest(
        &proof.material_digest,
        serde_json::to_string(&proof.jwks)
            .map_err(json_error)?
            .as_bytes(),
    )?;
    let issuer = IssuerUrl::new(proof.issuer.clone())
        .map_err(|err| LoomError::invalid(format!("oidc issuer must be an absolute URL: {err}")))?;
    let jwks: CoreJsonWebKeySet =
        serde_json::from_value(proof.jwks).map_err(|err| LoomError::invalid(err.to_string()))?;
    let verifier =
        CoreIdTokenVerifier::new_public_client(ClientId::new(proof.client_id), issuer, jwks);
    let id_token: CoreIdToken = serde_json::from_value(Value::String(proof.token))
        .map_err(|err| LoomError::invalid(err.to_string()))?;
    let nonce = required_challenge(challenge)?.to_string();
    let claims = id_token
        .into_claims(&verifier, &Nonce::new(nonce))
        .map_err(|err| {
            LoomError::new(
                Code::AuthenticationFailed,
                format!("oidc proof failed: {err}"),
            )
        })?;
    Ok(VerifiedExternalCredential {
        kind: ExternalCredentialKind::OidcSubject,
        issuer: proof.issuer,
        subject: claims.subject().to_string(),
        material_digest: Some(proof.material_digest),
        challenge_id: Some(parse_challenge_id(&proof.challenge_id)?),
    })
}

fn verify_saml(proof: &str, challenge: Option<&str>) -> Result<VerifiedExternalCredential> {
    if cfg!(feature = "fips") {
        return Err(unsupported_direct(
            ExternalCredentialKind::SamlSubject,
            "FIPS direct SAML proof requires a provider-backed verifier",
        ));
    }
    use opensaml::constants::{Binding, ParserType};
    use opensaml::flow::{FlowOptions, HttpRequest, flow};

    let proof: SamlProof = parse_proof(proof)?;
    let cert_material = proof.signing_certs.join("\n");
    verify_material_digest(&proof.material_digest, cert_material.as_bytes())?;
    let challenge_value = required_challenge(challenge)?;
    let mut options = FlowOptions::default();
    options.binding = Some(Binding::Post);
    options.parser_type = Some(ParserType::SamlResponse);
    options.check_signature = true;
    options.from_issuer = Some(&proof.issuer);
    options.signing_certs = &proof.signing_certs;
    options.clock_drifts = (300_000, 300_000);
    options.expected_audience = Some(&proof.audience);
    options.expected_in_response_to = Some(challenge_value);
    let response = flow(
        &options,
        &HttpRequest::post(vec![("SAMLResponse".to_string(), proof.saml_response)]),
    )
    .map_err(|err| {
        LoomError::new(
            Code::AuthenticationFailed,
            format!("saml proof failed: {err}"),
        )
    })?;
    let subject = response
        .extract
        .get_str("nameID")
        .ok_or_else(|| LoomError::new(Code::AuthenticationFailed, "saml response missing subject"))?
        .to_string();
    Ok(VerifiedExternalCredential {
        kind: ExternalCredentialKind::SamlSubject,
        issuer: proof.issuer,
        subject,
        material_digest: Some(proof.material_digest),
        challenge_id: Some(parse_challenge_id(&proof.challenge_id)?),
    })
}

fn verify_passkey(proof: &str, challenge: Option<&str>) -> Result<VerifiedExternalCredential> {
    if cfg!(feature = "fips") {
        return Err(unsupported_direct(
            ExternalCredentialKind::Passkey,
            "FIPS direct passkey proof requires a provider-backed verifier",
        ));
    }
    use webauthn::credential::Challenge;
    use webauthn::{AuthenticatorAssertionResponse, RelyingParty};

    let proof: PasskeyProof = parse_proof(proof)?;
    let challenge = required_challenge(challenge)?;
    let credential_json = serde_json::to_vec(&proof.credential).map_err(json_error)?;
    verify_material_digest(&proof.material_digest, &credential_json)?;
    let credential = serde_json::from_value(proof.credential)
        .map_err(|err| LoomError::invalid(format!("bad passkey credential: {err}")))?;
    let response = AuthenticatorAssertionResponse {
        client_data_json: decode_b64(&proof.client_data_json, "client_data_json")?,
        authenticator_data: decode_b64(&proof.authenticator_data, "authenticator_data")?,
        signature: decode_b64(&proof.signature, "signature")?,
        user_handle: proof
            .user_handle
            .as_deref()
            .map(|value| decode_b64(value, "user_handle"))
            .transpose()?,
    };
    let challenge = Challenge {
        bytes: challenge.as_bytes().to_vec(),
        created_at: SystemTime::now(),
    };
    let rp = RelyingParty::new(&proof.rp_id, &proof.origin, &proof.rp_name);
    rp.verify_authentication(&credential, &challenge, &response)
        .map_err(|err| {
            LoomError::new(
                Code::AuthenticationFailed,
                format!("passkey proof failed: {err}"),
            )
        })?;
    Ok(VerifiedExternalCredential {
        kind: ExternalCredentialKind::Passkey,
        issuer: proof.issuer,
        subject: proof.subject,
        material_digest: Some(proof.material_digest),
        challenge_id: Some(parse_challenge_id(&proof.challenge_id)?),
    })
}

fn verify_mtls(
    proof: &str,
    peer_certificate_der: Option<&[u8]>,
) -> Result<VerifiedExternalCredential> {
    let leaf = peer_certificate_der.ok_or_else(|| {
        unsupported_direct(
            ExternalCredentialKind::MtlsCertificate,
            "mTLS direct proof requires TLS peer certificate binding",
        )
    })?;
    let proof: MtlsProof = parse_proof(proof)?;
    let (_, cert) = x509_parser::parse_x509_certificate(leaf)
        .map_err(|err| LoomError::invalid(format!("bad mTLS peer certificate: {err}")))?;
    let issuer = proof.issuer.unwrap_or_else(|| cert.issuer().to_string());
    let subject = proof.subject.unwrap_or_else(|| cert.subject().to_string());
    let expected = proof.material_digest;
    verify_material_digest(&expected, leaf)?;
    Ok(VerifiedExternalCredential {
        kind: ExternalCredentialKind::MtlsCertificate,
        issuer,
        subject,
        material_digest: Some(expected),
        challenge_id: proof
            .challenge_id
            .as_deref()
            .map(parse_challenge_id)
            .transpose()?,
    })
}

fn verify_ed25519_public_key(challenge: &[u8], public_key: &[u8], signature: &[u8]) -> Result<()> {
    #[cfg(feature = "fips")]
    {
        let verifier = aws_lc_rs::signature::UnparsedPublicKey::new(
            &aws_lc_rs::signature::ED25519,
            public_key,
        );
        return verifier
            .verify(challenge, signature)
            .map_err(|_| LoomError::new(Code::AuthenticationFailed, "public-key proof failed"));
    }
    #[cfg(not(feature = "fips"))]
    {
        let verifier =
            ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, public_key);
        verifier
            .verify(challenge, signature)
            .map_err(|_| LoomError::new(Code::AuthenticationFailed, "public-key proof failed"))
    }
}

#[derive(Deserialize)]
struct PublicKeyProof {
    issuer: String,
    subject: String,
    material_digest: String,
    public_key: String,
    signature: String,
    challenge_id: String,
    algorithm: Option<String>,
}

#[derive(Deserialize)]
struct OidcProof {
    issuer: String,
    client_id: String,
    token: String,
    jwks: Value,
    material_digest: String,
    challenge_id: String,
}

#[derive(Deserialize)]
struct SamlProof {
    issuer: String,
    audience: String,
    saml_response: String,
    signing_certs: Vec<String>,
    material_digest: String,
    challenge_id: String,
}

#[derive(Deserialize)]
struct PasskeyProof {
    issuer: String,
    subject: String,
    material_digest: String,
    rp_id: String,
    origin: String,
    #[serde(default = "default_passkey_rp_name")]
    rp_name: String,
    credential: Value,
    client_data_json: String,
    authenticator_data: String,
    signature: String,
    user_handle: Option<String>,
    challenge_id: String,
}

#[derive(Deserialize)]
struct MtlsProof {
    issuer: Option<String>,
    subject: Option<String>,
    material_digest: String,
    challenge_id: Option<String>,
}

fn parse_proof<T: for<'de> Deserialize<'de>>(proof: &str) -> Result<T> {
    serde_json::from_str(proof).map_err(|err| LoomError::invalid(format!("bad proof JSON: {err}")))
}

fn decode_b64(value: &str, field: &str) -> Result<Vec<u8>> {
    URL_SAFE_NO_PAD
        .decode(value)
        .or_else(|_| STANDARD.decode(value))
        .map_err(|err| LoomError::invalid(format!("{field} must be base64: {err}")))
}

fn required_challenge(challenge: Option<&str>) -> Result<&str> {
    challenge.ok_or_else(|| {
        LoomError::invalid("direct external proof requires x-loom-external-challenge")
    })
}

fn parse_challenge_id(challenge_id: &str) -> Result<loom_core::WorkspaceId> {
    loom_core::WorkspaceId::parse(challenge_id)
}

fn verify_material_digest(expected: &str, material: &[u8]) -> Result<()> {
    let expected_digest = Digest::parse(expected)?;
    let actual = Digest::hash(expected_digest.algo(), material).to_string();
    if actual != expected {
        return Err(LoomError::new(
            Code::AuthenticationFailed,
            "external credential material digest mismatch",
        ));
    }
    Ok(())
}

fn unsupported_direct(kind: ExternalCredentialKind, reason: impl AsRef<str>) -> LoomError {
    LoomError::new(
        Code::Unsupported,
        format!(
            "direct {} verifier is not available: {}",
            kind.as_str(),
            reason.as_ref()
        ),
    )
}

fn json_error(err: serde_json::Error) -> LoomError {
    LoomError::invalid(err.to_string())
}

fn default_passkey_rp_name() -> String {
    "Uldren Loom".to_string()
}
