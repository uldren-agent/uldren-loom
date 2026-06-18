//! Runtime provider and profile reporting.
//!
//! This report describes the linked Loom artifact, not an open store. Bindings use it to expose
//! whether the loaded native artifact is standard or FIPS-capable without linking hosted server code.

use crate::Algo;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeProfile {
    pub binary_channel: &'static str,
    pub runtime_policy: &'static str,
    pub default_identity_profile: Algo,
    pub crypto_provider: &'static str,
    pub tls_provider: &'static str,
    pub fips_capable: bool,
    pub fips_tls_claim: bool,
}

pub const fn runtime_profile() -> RuntimeProfile {
    runtime_profile_with_tls("none", false)
}

pub const fn runtime_profile_with_tls(
    tls_provider: &'static str,
    fips_tls_claim: bool,
) -> RuntimeProfile {
    if cfg!(feature = "fips") {
        RuntimeProfile {
            binary_channel: "fips",
            runtime_policy: "strict",
            default_identity_profile: Algo::Sha256,
            crypto_provider: "rustcrypto-fips-profile",
            tls_provider,
            fips_capable: true,
            fips_tls_claim,
        }
    } else {
        RuntimeProfile {
            binary_channel: "standard",
            runtime_policy: "capable",
            default_identity_profile: Algo::Blake3,
            crypto_provider: "rustcrypto-default",
            tls_provider,
            fips_capable: false,
            fips_tls_claim: false,
        }
    }
}

impl RuntimeProfile {
    pub fn to_cbor(self) -> Vec<u8> {
        use loom_codec::Value;
        let value = Value::Map(vec![
            (
                Value::Text("binary_channel".into()),
                Value::Text(self.binary_channel.into()),
            ),
            (
                Value::Text("runtime_policy".into()),
                Value::Text(self.runtime_policy.into()),
            ),
            (
                Value::Text("default_identity_profile".into()),
                Value::Text(self.default_identity_profile.as_str().into()),
            ),
            (
                Value::Text("crypto_provider".into()),
                Value::Text(self.crypto_provider.into()),
            ),
            (
                Value::Text("tls_provider".into()),
                Value::Text(self.tls_provider.into()),
            ),
            (
                Value::Text("fips_capable".into()),
                Value::Bool(self.fips_capable),
            ),
            (
                Value::Text("fips_tls_claim".into()),
                Value::Bool(self.fips_tls_claim),
            ),
        ]);
        loom_codec::encode(&value).expect("the runtime profile always encodes")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_profile_reports_build_policy() {
        let profile = runtime_profile();
        if cfg!(feature = "fips") {
            assert_eq!(profile.binary_channel, "fips");
            assert_eq!(profile.runtime_policy, "strict");
            assert_eq!(profile.default_identity_profile, Algo::Sha256);
            assert!(profile.fips_capable);
        } else {
            assert_eq!(profile.binary_channel, "standard");
            assert_eq!(profile.runtime_policy, "capable");
            assert_eq!(profile.default_identity_profile, Algo::Blake3);
            assert!(!profile.fips_capable);
        }
        assert_eq!(profile.tls_provider, "none");
        assert!(!profile.fips_tls_claim);
        assert!(!profile.to_cbor().is_empty());
    }
}
