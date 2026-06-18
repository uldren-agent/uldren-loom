use std::collections::BTreeMap;

use loom_codec::Value;

use crate::error::{Code, LoomError, Result};
use crate::provider::ObjectStore;
use crate::vcs::Loom;
use crate::workspace::{FacetKind, WorkspaceId, facet_path};
use crate::{AclRight, CapabilityOperationalState};

const CONFIG_DIR: &str = "optional-runtimes";
const CONFIG_SCHEMA: &str = "loom.optional-runtime.config.v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum OptionalRuntimeKind {
    Fuse,
    Tor,
    Ipfs,
    HeavyEngine,
}

impl OptionalRuntimeKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Fuse => "fuse",
            Self::Tor => "tor",
            Self::Ipfs => "ipfs",
            Self::HeavyEngine => "heavy-engine",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "fuse" => Ok(Self::Fuse),
            "tor" => Ok(Self::Tor),
            "ipfs" => Ok(Self::Ipfs),
            "heavy-engine" => Ok(Self::HeavyEngine),
            _ => Err(LoomError::invalid("unsupported optional runtime kind")),
        }
    }

    pub const fn activation_capability(self) -> &'static str {
        match self {
            Self::Fuse => "runtime-fuse-activation",
            Self::Tor => "runtime-tor-activation",
            Self::Ipfs => "runtime-ipfs-activation",
            Self::HeavyEngine => "runtime-heavy-engine-activation",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OptionalRuntimeConfig {
    pub kind: OptionalRuntimeKind,
    pub enabled: bool,
    pub settings: BTreeMap<String, String>,
}

impl OptionalRuntimeConfig {
    pub fn new(
        kind: OptionalRuntimeKind,
        enabled: bool,
        settings: BTreeMap<String, String>,
    ) -> Result<Self> {
        let config = Self {
            kind,
            enabled,
            settings,
        };
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        for (key, value) in &self.settings {
            validate_text("optional runtime setting key", key)?;
            validate_text("optional runtime setting value", value)?;
        }
        Ok(())
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        loom_codec::encode(&Value::Array(vec![
            Value::Text(CONFIG_SCHEMA.to_string()),
            Value::Text(self.kind.as_str().to_string()),
            Value::Bool(self.enabled),
            Value::Map(
                self.settings
                    .iter()
                    .map(|(key, value)| (Value::Text(key.clone()), Value::Text(value.clone())))
                    .collect(),
            ),
        ]))
        .map_err(|error| LoomError::invalid(format!("optional runtime config encode: {error:?}")))
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let Value::Array(items) = loom_codec::decode(bytes).map_err(|error| {
            LoomError::corrupt(format!("optional runtime config decode: {error:?}"))
        })?
        else {
            return Err(LoomError::corrupt(
                "optional runtime config must be an array",
            ));
        };
        let [schema, kind, enabled, settings]: [Value; 4] = items
            .try_into()
            .map_err(|_| LoomError::corrupt("optional runtime config field count is invalid"))?;
        let Value::Text(schema) = schema else {
            return Err(LoomError::corrupt(
                "optional runtime config schema is invalid",
            ));
        };
        if schema != CONFIG_SCHEMA {
            return Err(LoomError::corrupt("unknown optional runtime config schema"));
        }
        let Value::Text(kind) = kind else {
            return Err(LoomError::corrupt("optional runtime kind is invalid"));
        };
        let Value::Bool(enabled) = enabled else {
            return Err(LoomError::corrupt(
                "optional runtime enabled flag is invalid",
            ));
        };
        let Value::Map(settings) = settings else {
            return Err(LoomError::corrupt("optional runtime settings are invalid"));
        };
        let mut decoded = BTreeMap::new();
        for (key, value) in settings {
            let Value::Text(key) = key else {
                return Err(LoomError::corrupt(
                    "optional runtime setting key is invalid",
                ));
            };
            let Value::Text(value) = value else {
                return Err(LoomError::corrupt(
                    "optional runtime setting value is invalid",
                ));
            };
            decoded.insert(key, value);
        }
        Self::new(OptionalRuntimeKind::parse(&kind)?, enabled, decoded)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OptionalRuntimeCapability {
    pub kind: OptionalRuntimeKind,
    pub configuration_state: CapabilityOperationalState,
    pub activation_state: CapabilityOperationalState,
    pub reason_code: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IpfsGatewayCacheConfig {
    pub enabled: bool,
    pub gateway_url: String,
    pub cache_policy: String,
    pub verify_blocks: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TorOnionServiceConfig {
    pub enabled: bool,
    pub socks_proxy: String,
    pub onion_service: String,
    pub target_addr: String,
    pub route_policy: String,
}

impl TorOnionServiceConfig {
    pub const DEFAULT_ROUTE_POLICY: &'static str = "configured-only";

    pub fn new(
        socks_proxy: String,
        onion_service: String,
        target_addr: String,
        route_policy: String,
    ) -> Result<Self> {
        validate_text("tor socks proxy", &socks_proxy)?;
        validate_text("tor onion service", &onion_service)?;
        validate_text("tor target address", &target_addr)?;
        validate_text("tor route policy", &route_policy)?;
        if !socks_proxy.starts_with("socks5://") {
            return Err(LoomError::invalid("tor socks proxy must use socks5"));
        }
        if !onion_service.ends_with(".onion") {
            return Err(LoomError::invalid(
                "tor onion service must be an onion host",
            ));
        }
        if !target_addr.contains(':') {
            return Err(LoomError::invalid("tor target address must include a port"));
        }
        if route_policy != Self::DEFAULT_ROUTE_POLICY {
            return Err(LoomError::invalid("unsupported tor route policy"));
        }
        Ok(Self {
            enabled: true,
            socks_proxy,
            onion_service,
            target_addr,
            route_policy,
        })
    }

    pub fn to_optional_runtime_config(&self) -> Result<OptionalRuntimeConfig> {
        let mut settings = BTreeMap::new();
        settings.insert("profile".to_string(), "onion-service".to_string());
        settings.insert("socks_proxy".to_string(), self.socks_proxy.clone());
        settings.insert("onion_service".to_string(), self.onion_service.clone());
        settings.insert("target_addr".to_string(), self.target_addr.clone());
        settings.insert("route_policy".to_string(), self.route_policy.clone());
        OptionalRuntimeConfig::new(OptionalRuntimeKind::Tor, self.enabled, settings)
    }

    pub fn from_optional_runtime_config(config: &OptionalRuntimeConfig) -> Result<Self> {
        if config.kind != OptionalRuntimeKind::Tor {
            return Err(LoomError::invalid(
                "tor onion service config requires tor runtime kind",
            ));
        }
        let profile = config
            .settings
            .get("profile")
            .ok_or_else(|| LoomError::corrupt("tor profile setting is missing"))?;
        if profile != "onion-service" {
            return Err(LoomError::invalid(
                "unsupported tor optional runtime profile",
            ));
        }
        let socks_proxy = config
            .settings
            .get("socks_proxy")
            .ok_or_else(|| LoomError::corrupt("tor socks proxy setting is missing"))?
            .clone();
        let onion_service = config
            .settings
            .get("onion_service")
            .ok_or_else(|| LoomError::corrupt("tor onion service setting is missing"))?
            .clone();
        let target_addr = config
            .settings
            .get("target_addr")
            .ok_or_else(|| LoomError::corrupt("tor target address setting is missing"))?
            .clone();
        let route_policy = config
            .settings
            .get("route_policy")
            .ok_or_else(|| LoomError::corrupt("tor route policy setting is missing"))?
            .clone();
        let mut profile = Self::new(socks_proxy, onion_service, target_addr, route_policy)?;
        profile.enabled = config.enabled;
        Ok(profile)
    }
}

impl IpfsGatewayCacheConfig {
    pub const DEFAULT_CACHE_POLICY: &'static str = "verified-cache-only";

    pub fn new(gateway_url: String, cache_policy: String, verify_blocks: bool) -> Result<Self> {
        validate_text("ipfs gateway url", &gateway_url)?;
        validate_text("ipfs cache policy", &cache_policy)?;
        if !(gateway_url.starts_with("https://") || gateway_url.starts_with("http://")) {
            return Err(LoomError::invalid(
                "ipfs gateway url must use http or https",
            ));
        }
        if cache_policy != Self::DEFAULT_CACHE_POLICY {
            return Err(LoomError::invalid("unsupported ipfs cache policy"));
        }
        if !verify_blocks {
            return Err(LoomError::invalid("ipfs verified caching is required"));
        }
        Ok(Self {
            enabled: true,
            gateway_url,
            cache_policy,
            verify_blocks,
        })
    }

    pub fn to_optional_runtime_config(&self) -> Result<OptionalRuntimeConfig> {
        let mut settings = BTreeMap::new();
        settings.insert("profile".to_string(), "gateway-cache".to_string());
        settings.insert("gateway_url".to_string(), self.gateway_url.clone());
        settings.insert("cache_policy".to_string(), self.cache_policy.clone());
        settings.insert("verify_blocks".to_string(), self.verify_blocks.to_string());
        OptionalRuntimeConfig::new(OptionalRuntimeKind::Ipfs, self.enabled, settings)
    }

    pub fn from_optional_runtime_config(config: &OptionalRuntimeConfig) -> Result<Self> {
        if config.kind != OptionalRuntimeKind::Ipfs {
            return Err(LoomError::invalid(
                "ipfs gateway cache config requires ipfs runtime kind",
            ));
        }
        let profile = config
            .settings
            .get("profile")
            .ok_or_else(|| LoomError::corrupt("ipfs profile setting is missing"))?;
        if profile != "gateway-cache" {
            return Err(LoomError::invalid(
                "unsupported ipfs optional runtime profile",
            ));
        }
        let gateway_url = config
            .settings
            .get("gateway_url")
            .ok_or_else(|| LoomError::corrupt("ipfs gateway url setting is missing"))?
            .clone();
        let cache_policy = config
            .settings
            .get("cache_policy")
            .ok_or_else(|| LoomError::corrupt("ipfs cache policy setting is missing"))?
            .clone();
        let verify_blocks = config
            .settings
            .get("verify_blocks")
            .ok_or_else(|| LoomError::corrupt("ipfs verify blocks setting is missing"))?
            == "true";
        let mut profile = Self::new(gateway_url, cache_policy, verify_blocks)?;
        profile.enabled = config.enabled;
        Ok(profile)
    }
}

pub fn set_ipfs_gateway_cache_config<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    config: &IpfsGatewayCacheConfig,
) -> Result<()> {
    set_optional_runtime_config(loom, ns, &config.to_optional_runtime_config()?)
}

pub fn get_ipfs_gateway_cache_config<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
) -> Result<Option<IpfsGatewayCacheConfig>> {
    get_optional_runtime_config(loom, ns, OptionalRuntimeKind::Ipfs)?
        .map(|config| IpfsGatewayCacheConfig::from_optional_runtime_config(&config))
        .transpose()
}

pub fn set_tor_onion_service_config<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    config: &TorOnionServiceConfig,
) -> Result<()> {
    set_optional_runtime_config(loom, ns, &config.to_optional_runtime_config()?)
}

pub fn get_tor_onion_service_config<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
) -> Result<Option<TorOnionServiceConfig>> {
    get_optional_runtime_config(loom, ns, OptionalRuntimeKind::Tor)?
        .map(|config| TorOnionServiceConfig::from_optional_runtime_config(&config))
        .transpose()
}

pub fn optional_runtime_capability(
    config: Option<&OptionalRuntimeConfig>,
    kind: OptionalRuntimeKind,
) -> OptionalRuntimeCapability {
    OptionalRuntimeCapability {
        kind,
        configuration_state: CapabilityOperationalState::Supported,
        activation_state: if config.is_some_and(|config| config.enabled) {
            CapabilityOperationalState::Unavailable
        } else {
            CapabilityOperationalState::Disabled
        },
        reason_code: if config.is_some_and(|config| config.enabled) {
            "feature_not_compiled"
        } else {
            "configured_disabled"
        },
    }
}

pub fn set_optional_runtime_config<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    config: &OptionalRuntimeConfig,
) -> Result<()> {
    loom.authorize(ns, FacetKind::Vcs, AclRight::Admin)?;
    config.validate()?;
    loom.create_directory_reserved(ns, &config_dir(), true)?;
    loom.write_file_reserved(ns, &config_path(config.kind), &config.encode()?, 0o100644)
}

pub fn get_optional_runtime_config<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    kind: OptionalRuntimeKind,
) -> Result<Option<OptionalRuntimeConfig>> {
    loom.authorize(ns, FacetKind::Vcs, AclRight::Read)?;
    match loom.read_file_reserved(ns, &config_path(kind)) {
        Ok(bytes) => OptionalRuntimeConfig::decode(&bytes).map(Some),
        Err(error) if error.code == Code::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

pub fn optional_runtime_capabilities<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
) -> Result<Vec<OptionalRuntimeCapability>> {
    let mut capabilities = Vec::new();
    for kind in [
        OptionalRuntimeKind::Fuse,
        OptionalRuntimeKind::Tor,
        OptionalRuntimeKind::Ipfs,
        OptionalRuntimeKind::HeavyEngine,
    ] {
        let config = get_optional_runtime_config(loom, ns, kind)?;
        capabilities.push(optional_runtime_capability(config.as_ref(), kind));
    }
    Ok(capabilities)
}

pub fn activate_optional_runtime<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    kind: OptionalRuntimeKind,
) -> Result<()> {
    let config = get_optional_runtime_config(loom, ns, kind)?;
    let capability = optional_runtime_capability(config.as_ref(), kind);
    Err(LoomError::new(
        capability
            .activation_state
            .stable_error()
            .unwrap_or(Code::Unsupported),
        format!(
            "optional runtime {} activation is unavailable: {}",
            kind.as_str(),
            capability.reason_code
        ),
    ))
}

impl CapabilityOperationalState {
    fn stable_error(self) -> Option<Code> {
        match self {
            Self::Supported | Self::Degraded | Self::Disabled => None,
            Self::Unsupported | Self::Target => Some(Code::Unsupported),
            Self::Denied => Some(Code::PermissionDenied),
            Self::Unavailable => Some(Code::Unsupported),
        }
    }
}

fn config_dir() -> String {
    facet_path(FacetKind::Program, CONFIG_DIR)
}

fn config_path(kind: OptionalRuntimeKind) -> String {
    facet_path(
        FacetKind::Program,
        &format!("{CONFIG_DIR}/{}.cbor", kind.as_str()),
    )
}

fn validate_text(label: &str, value: &str) -> Result<()> {
    if value.is_empty() || value.len() > 1024 || value.chars().any(char::is_control) {
        return Err(LoomError::invalid(format!("{label} is invalid")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MemoryStore, WorkspaceId};

    fn ns(seed: u8) -> WorkspaceId {
        WorkspaceId::from_bytes([seed; 16])
    }

    #[test]
    fn optional_runtime_config_persists_without_activation() {
        let namespace = ns(1);
        let mut settings = BTreeMap::new();
        settings.insert("gateway".to_string(), "https://example.test".to_string());
        let config =
            OptionalRuntimeConfig::new(OptionalRuntimeKind::Ipfs, false, settings).unwrap();
        let mut loom = Loom::new(MemoryStore::new());
        set_optional_runtime_config(&mut loom, namespace, &config).unwrap();

        let stored = get_optional_runtime_config(&loom, namespace, OptionalRuntimeKind::Ipfs)
            .unwrap()
            .unwrap();
        assert_eq!(stored, config);
        let capability = optional_runtime_capability(Some(&stored), OptionalRuntimeKind::Ipfs);
        assert_eq!(
            capability.configuration_state,
            CapabilityOperationalState::Supported
        );
        assert_eq!(
            capability.activation_state,
            CapabilityOperationalState::Disabled
        );
        assert_eq!(
            activate_optional_runtime(&loom, namespace, OptionalRuntimeKind::Ipfs)
                .unwrap_err()
                .code,
            Code::Unsupported
        );
    }

    #[test]
    fn enabled_unsupported_activation_reports_unavailable() {
        let namespace = ns(2);
        let config =
            OptionalRuntimeConfig::new(OptionalRuntimeKind::Tor, true, BTreeMap::new()).unwrap();
        let mut loom = Loom::new(MemoryStore::new());
        set_optional_runtime_config(&mut loom, namespace, &config).unwrap();
        let report = optional_runtime_capabilities(&loom, namespace).unwrap();
        let tor = report
            .iter()
            .find(|capability| capability.kind == OptionalRuntimeKind::Tor)
            .unwrap();
        assert_eq!(
            tor.configuration_state,
            CapabilityOperationalState::Supported
        );
        assert_eq!(
            tor.activation_state,
            CapabilityOperationalState::Unavailable
        );
        assert_eq!(
            activate_optional_runtime(&loom, namespace, OptionalRuntimeKind::Tor)
                .unwrap_err()
                .code,
            Code::Unsupported
        );
    }

    #[test]
    fn ipfs_gateway_cache_config_round_trips_without_runtime_linkage() {
        let namespace = ns(3);
        let config = IpfsGatewayCacheConfig::new(
            "https://ipfs.example.test".to_string(),
            IpfsGatewayCacheConfig::DEFAULT_CACHE_POLICY.to_string(),
            true,
        )
        .unwrap();
        let runtime_config = config.to_optional_runtime_config().unwrap();
        assert_eq!(runtime_config.kind, OptionalRuntimeKind::Ipfs);
        assert!(runtime_config.enabled);
        assert_eq!(
            runtime_config.settings.get("profile").map(String::as_str),
            Some("gateway-cache")
        );

        let mut loom = Loom::new(MemoryStore::new());
        set_ipfs_gateway_cache_config(&mut loom, namespace, &config).unwrap();
        assert_eq!(
            get_optional_runtime_config(&loom, namespace, OptionalRuntimeKind::Ipfs)
                .unwrap()
                .unwrap(),
            runtime_config
        );
        assert_eq!(
            get_ipfs_gateway_cache_config(&loom, namespace).unwrap(),
            Some(config)
        );
        let capability = optional_runtime_capabilities(&loom, namespace)
            .unwrap()
            .into_iter()
            .find(|capability| capability.kind == OptionalRuntimeKind::Ipfs)
            .unwrap();
        assert_eq!(
            capability.configuration_state,
            CapabilityOperationalState::Supported
        );
        assert_eq!(
            capability.activation_state,
            CapabilityOperationalState::Unavailable
        );
        assert_eq!(capability.reason_code, "feature_not_compiled");
        assert_eq!(
            activate_optional_runtime(&loom, namespace, OptionalRuntimeKind::Ipfs)
                .unwrap_err()
                .code,
            Code::Unsupported
        );
    }

    #[test]
    fn ipfs_gateway_cache_config_rejects_unverified_or_unknown_profiles() {
        assert!(
            IpfsGatewayCacheConfig::new(
                "file:///tmp/ipfs".to_string(),
                IpfsGatewayCacheConfig::DEFAULT_CACHE_POLICY.to_string(),
                true,
            )
            .is_err()
        );
        assert!(
            IpfsGatewayCacheConfig::new(
                "https://ipfs.example.test".to_string(),
                "fetch-and-serve".to_string(),
                true,
            )
            .is_err()
        );
        assert!(
            IpfsGatewayCacheConfig::new(
                "https://ipfs.example.test".to_string(),
                IpfsGatewayCacheConfig::DEFAULT_CACHE_POLICY.to_string(),
                false,
            )
            .is_err()
        );

        let config = OptionalRuntimeConfig::new(
            OptionalRuntimeKind::Ipfs,
            true,
            BTreeMap::from([("profile".to_string(), "kubo-node".to_string())]),
        )
        .unwrap();
        assert!(IpfsGatewayCacheConfig::from_optional_runtime_config(&config).is_err());
    }

    #[test]
    fn tor_onion_service_config_round_trips_without_runtime_linkage() {
        let namespace = ns(4);
        let config = TorOnionServiceConfig::new(
            "socks5://127.0.0.1:9050".to_string(),
            "exampleabcdefghijklmnop.onion".to_string(),
            "127.0.0.1:8080".to_string(),
            TorOnionServiceConfig::DEFAULT_ROUTE_POLICY.to_string(),
        )
        .unwrap();
        let runtime_config = config.to_optional_runtime_config().unwrap();
        assert_eq!(runtime_config.kind, OptionalRuntimeKind::Tor);
        assert!(runtime_config.enabled);
        assert_eq!(
            runtime_config.settings.get("profile").map(String::as_str),
            Some("onion-service")
        );

        let mut loom = Loom::new(MemoryStore::new());
        set_tor_onion_service_config(&mut loom, namespace, &config).unwrap();
        assert_eq!(
            get_optional_runtime_config(&loom, namespace, OptionalRuntimeKind::Tor)
                .unwrap()
                .unwrap(),
            runtime_config
        );
        assert_eq!(
            get_tor_onion_service_config(&loom, namespace).unwrap(),
            Some(config)
        );
        let capability = optional_runtime_capabilities(&loom, namespace)
            .unwrap()
            .into_iter()
            .find(|capability| capability.kind == OptionalRuntimeKind::Tor)
            .unwrap();
        assert_eq!(
            capability.configuration_state,
            CapabilityOperationalState::Supported
        );
        assert_eq!(
            capability.activation_state,
            CapabilityOperationalState::Unavailable
        );
        assert_eq!(capability.reason_code, "feature_not_compiled");
        assert_eq!(
            activate_optional_runtime(&loom, namespace, OptionalRuntimeKind::Tor)
                .unwrap_err()
                .code,
            Code::Unsupported
        );
    }

    #[test]
    fn tor_onion_service_config_rejects_runtime_or_route_claims() {
        assert!(
            TorOnionServiceConfig::new(
                "http://127.0.0.1:9050".to_string(),
                "exampleabcdefghijklmnop.onion".to_string(),
                "127.0.0.1:8080".to_string(),
                TorOnionServiceConfig::DEFAULT_ROUTE_POLICY.to_string(),
            )
            .is_err()
        );
        assert!(
            TorOnionServiceConfig::new(
                "socks5://127.0.0.1:9050".to_string(),
                "example.invalid".to_string(),
                "127.0.0.1:8080".to_string(),
                TorOnionServiceConfig::DEFAULT_ROUTE_POLICY.to_string(),
            )
            .is_err()
        );
        assert!(
            TorOnionServiceConfig::new(
                "socks5://127.0.0.1:9050".to_string(),
                "exampleabcdefghijklmnop.onion".to_string(),
                "127.0.0.1".to_string(),
                TorOnionServiceConfig::DEFAULT_ROUTE_POLICY.to_string(),
            )
            .is_err()
        );
        assert!(
            TorOnionServiceConfig::new(
                "socks5://127.0.0.1:9050".to_string(),
                "exampleabcdefghijklmnop.onion".to_string(),
                "127.0.0.1:8080".to_string(),
                "route-traffic".to_string(),
            )
            .is_err()
        );

        let config = OptionalRuntimeConfig::new(
            OptionalRuntimeKind::Tor,
            true,
            BTreeMap::from([("profile".to_string(), "arti-client".to_string())]),
        )
        .unwrap();
        assert!(TorOnionServiceConfig::from_optional_runtime_config(&config).is_err());
    }
}
