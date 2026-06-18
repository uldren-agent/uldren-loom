//! Endpoint discovery document and server route resolution.
//!
//! A `remote-loom` endpoint publishes a public discovery document that advertises the protocol id,
//! capabilities, versions, auth and TLS selectors, and the concrete call endpoint. The document is
//! served at the host well-known path and/or under the service root depending on the deployment mode.
//! This module owns the document codec and the server-side route resolution; it holds no secret
//! material.
//!
//! Licensed under BUSL-1.1.

use crate::codec::ArgError;
use crate::envelope::{PROTOCOL_ID, as_map, bool_field, field, text, text_field, u64_field};
use loom_codec::{CodecError, Value, decode, encode};

/// The host-level well-known discovery path.
pub const WELL_KNOWN_PATH: &str = "/.well-known/loom";

/// How the server exposes its discovery document (the deployment mode).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DiscoveryMode {
    /// Serve at the service-root well-known path (when the service root is not the host root) and at the
    /// host well-known path.
    #[default]
    Default,
    /// Serve only under the service root.
    ServiceRoot,
    /// Serve only at the host well-known path.
    WellKnown,
    /// Serve no discovery document; the locator URL is the exact protocol endpoint.
    Disabled,
}

/// The published discovery document.
#[derive(Debug, Clone, PartialEq)]
pub struct Discovery {
    /// The protocol id, always `loom.remote.v1` for v1.
    pub protocol: String,
    /// Advertised capabilities; a `remote-loom` endpoint lists `remote-loom`.
    pub capabilities: Vec<String>,
    /// The service-root URL clients treat as the endpoint base.
    pub service_root: String,
    /// Named concrete endpoints, for example `cbor-h2` to the call URL.
    pub endpoints: Vec<(String, String)>,
    /// Minimum supported protocol version.
    pub min_version: u64,
    /// Maximum supported protocol version.
    pub max_version: u64,
    /// Supported auth selectors (never secret material).
    pub auth: Vec<String>,
    /// Supported TLS trust selectors (never key material).
    pub tls: Vec<String>,
    /// Whether the endpoint supports streams.
    pub streams: bool,
    /// Supported payload compressions.
    pub compression: Vec<String>,
    /// Whether the endpoint serves exactly one Loom.
    pub single_loom: bool,
}

impl Discovery {
    /// A v1 discovery document for `service_root` whose `cbor-h2` endpoint is `call_endpoint`, with the
    /// default capability, version, stream, and compression advertisements.
    pub fn v1(
        service_root: impl Into<String>,
        call_endpoint: impl Into<String>,
        auth: Vec<String>,
        tls: Vec<String>,
    ) -> Self {
        Self {
            protocol: PROTOCOL_ID.to_string(),
            capabilities: vec!["remote-loom".to_string()],
            service_root: service_root.into(),
            endpoints: vec![("cbor-h2".to_string(), call_endpoint.into())],
            min_version: 1,
            max_version: 1,
            auth,
            tls,
            streams: true,
            compression: vec!["zstd".to_string(), "none".to_string()],
            single_loom: true,
        }
    }

    /// Whether this document advertises the `remote-loom` capability and a version overlapping `[lo, hi]`.
    pub fn is_compatible(&self, lo: u64, hi: u64) -> bool {
        self.protocol == PROTOCOL_ID
            && self.capabilities.iter().any(|c| c == "remote-loom")
            && self.min_version <= hi
            && self.max_version >= lo
    }

    /// Encode to a canonical CBOR map.
    ///
    /// # Errors
    /// Returns [`CodecError`] only for a non-finite float, which this document never carries.
    pub fn encode(&self) -> Result<Vec<u8>, CodecError> {
        let entries = vec![
            (text("protocol"), text(&self.protocol)),
            (text("capabilities"), text_array(&self.capabilities)),
            (text("service_root"), text(&self.service_root)),
            (
                text("endpoints"),
                Value::Map(
                    self.endpoints
                        .iter()
                        .map(|(k, v)| (text(k), text(v)))
                        .collect(),
                ),
            ),
            (text("min_version"), Value::Uint(self.min_version)),
            (text("max_version"), Value::Uint(self.max_version)),
            (text("auth"), text_array(&self.auth)),
            (text("tls"), text_array(&self.tls)),
            (text("streams"), Value::Bool(self.streams)),
            (text("compression"), text_array(&self.compression)),
            (text("single_loom"), Value::Bool(self.single_loom)),
        ];
        encode(&Value::Map(entries))
    }

    /// Decode from a canonical CBOR map.
    ///
    /// # Errors
    /// Returns [`ArgError`] for a non-map buffer or a mistyped field.
    pub fn decode(bytes: &[u8]) -> Result<Self, ArgError> {
        let map = as_map(&decode(bytes)?)?;
        Ok(Self {
            protocol: text_field(&map, "protocol")?,
            capabilities: text_array_field(&map, "capabilities")?,
            service_root: text_field(&map, "service_root")?,
            endpoints: endpoints_field(&map)?,
            min_version: u64_field(&map, "min_version")?,
            max_version: u64_field(&map, "max_version")?,
            auth: text_array_field(&map, "auth")?,
            tls: text_array_field(&map, "tls")?,
            streams: bool_field(&map, "streams")?,
            compression: text_array_field(&map, "compression")?,
            single_loom: bool_field(&map, "single_loom")?,
        })
    }
}

/// The server-side discovery route resolution for one deployment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryRoutes {
    /// The deployment mode.
    pub mode: DiscoveryMode,
    /// The service-root path (for example `/apps/loom`, or `/` at the host root).
    pub service_root_path: String,
    /// An explicit absolute discovery path that, when set, is the sole route.
    pub custom_path: Option<String>,
}

impl DiscoveryRoutes {
    /// The absolute request paths that serve the discovery document for this deployment. Highest
    /// preference first; empty when discovery is disabled.
    pub fn discovery_paths(&self) -> Vec<String> {
        if let Some(custom) = &self.custom_path {
            return vec![normalize(custom)];
        }
        let host = WELL_KNOWN_PATH.to_string();
        let at_root = self.service_root_path.trim_end_matches('/').is_empty();
        let service_root = if at_root {
            host.clone()
        } else {
            format!(
                "{}{}",
                self.service_root_path.trim_end_matches('/'),
                WELL_KNOWN_PATH
            )
        };
        match self.mode {
            DiscoveryMode::Disabled => Vec::new(),
            DiscoveryMode::WellKnown => vec![host],
            DiscoveryMode::ServiceRoot => vec![service_root],
            DiscoveryMode::Default => {
                if at_root {
                    vec![host]
                } else {
                    vec![service_root, host]
                }
            }
        }
    }

    /// Whether `request_path` is a discovery route for this deployment.
    pub fn serves(&self, request_path: &str) -> bool {
        let normalized = normalize(request_path);
        self.discovery_paths().iter().any(|p| p == &normalized)
    }
}

fn normalize(path: &str) -> String {
    if path.len() > 1 {
        path.trim_end_matches('/').to_string()
    } else {
        path.to_string()
    }
}

fn text_array(values: &[String]) -> Value {
    Value::Array(values.iter().map(|v| text(v)).collect())
}

fn text_array_field(map: &[(Value, Value)], key: &str) -> Result<Vec<String>, ArgError> {
    match field(map, key) {
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| match item {
                Value::Text(t) => Ok(t.clone()),
                _ => Err(ArgError::TypeMismatch {
                    expected: "text array element",
                }),
            })
            .collect(),
        _ => Err(ArgError::TypeMismatch {
            expected: "text array",
        }),
    }
}

fn endpoints_field(map: &[(Value, Value)]) -> Result<Vec<(String, String)>, ArgError> {
    match field(map, "endpoints") {
        Some(Value::Map(entries)) => entries
            .iter()
            .map(|(k, v)| match (k, v) {
                (Value::Text(name), Value::Text(url)) => Ok((name.clone(), url.clone())),
                _ => Err(ArgError::TypeMismatch {
                    expected: "endpoint text pair",
                }),
            })
            .collect(),
        _ => Err(ArgError::TypeMismatch {
            expected: "endpoints map",
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_round_trips() {
        let doc = Discovery::v1(
            "https://remote.host/apps/loom",
            "https://remote.host/apps/loom/v1/call",
            vec!["interactive".to_string(), "token".to_string()],
            vec!["system".to_string()],
        );
        let decoded = Discovery::decode(&doc.encode().unwrap()).unwrap();
        assert_eq!(decoded, doc);
        assert!(decoded.is_compatible(1, 1));
    }

    #[test]
    fn default_mode_at_host_root_serves_well_known() {
        let routes = DiscoveryRoutes {
            mode: DiscoveryMode::Default,
            service_root_path: "/".to_string(),
            custom_path: None,
        };
        assert_eq!(routes.discovery_paths(), vec![WELL_KNOWN_PATH.to_string()]);
        assert!(routes.serves("/.well-known/loom"));
        assert!(!routes.serves("/other"));
    }

    #[test]
    fn default_mode_at_subpath_serves_service_root_then_host() {
        let routes = DiscoveryRoutes {
            mode: DiscoveryMode::Default,
            service_root_path: "/apps/loom".to_string(),
            custom_path: None,
        };
        assert_eq!(
            routes.discovery_paths(),
            vec![
                "/apps/loom/.well-known/loom".to_string(),
                "/.well-known/loom".to_string(),
            ]
        );
        assert!(routes.serves("/apps/loom/.well-known/loom"));
        assert!(routes.serves("/.well-known/loom"));
    }

    #[test]
    fn well_known_and_service_root_modes_are_exclusive() {
        let well_known = DiscoveryRoutes {
            mode: DiscoveryMode::WellKnown,
            service_root_path: "/apps/loom".to_string(),
            custom_path: None,
        };
        assert_eq!(
            well_known.discovery_paths(),
            vec![WELL_KNOWN_PATH.to_string()]
        );
        assert!(!well_known.serves("/apps/loom/.well-known/loom"));

        let service_root = DiscoveryRoutes {
            mode: DiscoveryMode::ServiceRoot,
            service_root_path: "/apps/loom".to_string(),
            custom_path: None,
        };
        assert_eq!(
            service_root.discovery_paths(),
            vec!["/apps/loom/.well-known/loom".to_string()]
        );
        assert!(!service_root.serves("/.well-known/loom"));
    }

    #[test]
    fn custom_path_overrides_and_disabled_serves_nothing() {
        let custom = DiscoveryRoutes {
            mode: DiscoveryMode::Default,
            service_root_path: "/apps/loom".to_string(),
            custom_path: Some("/internal/loom-discovery".to_string()),
        };
        assert_eq!(
            custom.discovery_paths(),
            vec!["/internal/loom-discovery".to_string()]
        );
        assert!(custom.serves("/internal/loom-discovery"));

        let disabled = DiscoveryRoutes {
            mode: DiscoveryMode::Disabled,
            service_root_path: "/".to_string(),
            custom_path: None,
        };
        assert!(disabled.discovery_paths().is_empty());
        assert!(!disabled.serves("/.well-known/loom"));
    }
}
