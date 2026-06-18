//! Connection and discovery layer.
//!
//! A [`RemoteConnection`] resolves a locator to a remote endpoint (via `loom-locator`), performs endpoint
//! discovery over the transport, validates that the endpoint advertises `remote-loom` and a compatible
//! protocol version, caches the service root, and fails fast when discovery is unavailable or
//! incompatible. Clients never save requests for later replay.
//!
//! Licensed under BUSL-1.1.

use crate::transport::Transport;
use loom_locator::{ContextResolver, Target};
use loom_remote_protocol::discovery::{Discovery, DiscoveryMode, WELL_KNOWN_PATH};
use loom_types::{Code, LoomError};

/// The client's supported protocol version range.
pub const CLIENT_MIN_VERSION: u64 = 1;
/// The client's maximum supported protocol version.
pub const CLIENT_MAX_VERSION: u64 = 1;

/// A discovered, version-negotiated connection to one remote endpoint.
pub struct RemoteConnection<T: Transport> {
    transport: T,
    service_root: String,
    discovery: Discovery,
    version: u64,
}

impl<T: Transport> RemoteConnection<T> {
    /// Resolve `locator` against `resolver`, discover the endpoint over `transport` using `mode`,
    /// validate the protocol version and capability, and cache the service root.
    ///
    /// # Errors
    /// Returns [`LoomError`]: `INVALID_ARGUMENT` when the locator is not a remote endpoint or discovery is
    /// disabled, `UNSUPPORTED` when the endpoint version/capability is incompatible, `CORRUPT_OBJECT` for
    /// a malformed discovery document, or the transport's fail-fast error when no route is reachable.
    pub async fn connect(
        transport: T,
        locator: &str,
        resolver: &ContextResolver,
        mode: DiscoveryMode,
    ) -> Result<Self, LoomError> {
        let target = if resolver.has_context(locator) {
            resolver.resolve_context(locator)
        } else {
            resolver.resolve_str(locator)
        }
        .map_err(|err| LoomError::new(Code::InvalidArgument, err.to_string()))?;
        let url = match target {
            Target::Remote(remote) => remote.url,
            Target::Local(_) => {
                return Err(LoomError::new(
                    Code::InvalidArgument,
                    "locator resolves to a local path; RemoteLoomClient requires a remote endpoint",
                ));
            }
        };
        let candidates = discovery_candidates(&url, mode);
        if candidates.is_empty() {
            return Err(LoomError::new(
                Code::InvalidArgument,
                "discovery is disabled; construct the connection against an explicit endpoint",
            ));
        }
        let mut last_err = LoomError::new(Code::NotFound, "no discovery route returned a document");
        for path in candidates {
            match transport.discover(&path).await {
                Ok(bytes) => {
                    let doc = Discovery::decode(&bytes).map_err(|err| {
                        LoomError::new(Code::CorruptObject, format!("discovery decode: {err}"))
                    })?;
                    if !doc.is_compatible(CLIENT_MIN_VERSION, CLIENT_MAX_VERSION) {
                        return Err(LoomError::new(
                            Code::Unsupported,
                            "endpoint does not advertise remote-loom at a compatible protocol version",
                        ));
                    }
                    let version = doc.max_version.min(CLIENT_MAX_VERSION);
                    return Ok(Self {
                        transport,
                        service_root: doc.service_root.clone(),
                        discovery: doc,
                        version,
                    });
                }
                Err(err) => last_err = err,
            }
        }
        Err(last_err)
    }

    /// The underlying transport.
    pub fn transport(&self) -> &T {
        &self.transport
    }

    /// The negotiated protocol version.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// The cached service-root URL.
    pub fn service_root(&self) -> &str {
        &self.service_root
    }

    /// The discovery document the endpoint published.
    pub fn discovery(&self) -> &Discovery {
        &self.discovery
    }
}

/// The absolute discovery paths a client tries for `url` under `mode`, highest preference first.
fn discovery_candidates(url: &str, mode: DiscoveryMode) -> Vec<String> {
    let path = url_path(url);
    let at_root = path.trim_end_matches('/').is_empty();
    let host = WELL_KNOWN_PATH.to_string();
    let service_root = if at_root {
        host.clone()
    } else {
        format!("{}{}", path.trim_end_matches('/'), WELL_KNOWN_PATH)
    };
    match mode {
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

/// The path component of a URL (everything from the first `/` after the authority), or `/`.
fn url_path(url: &str) -> String {
    let authority_and_rest = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    match authority_and_rest.find('/') {
        Some(index) => authority_and_rest[index..].to_string(),
        None => "/".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::loopback::Loopback;
    use loom_locator::{ContextResolver, Layer};
    use loom_remote_protocol::discovery::{Discovery, DiscoveryRoutes};
    use loom_types::Code;

    fn resolver() -> ContextResolver {
        ContextResolver::from_layers(&[Layer::new(
            "test",
            "[contexts.prod]\ntarget = \"https://remote.host/apps/loom\"\n",
        )])
        .unwrap()
    }

    fn doc_bytes() -> Vec<u8> {
        Discovery::v1(
            "https://remote.host/apps/loom",
            "https://remote.host/apps/loom/v1/call",
            vec!["interactive".to_string()],
            vec!["system".to_string()],
        )
        .encode()
        .unwrap()
    }

    fn block<F: std::future::Future>(f: F) -> F::Output {
        futures::executor::block_on(f)
    }

    #[test]
    fn resolves_configured_context_name() {
        let routes = DiscoveryRoutes {
            mode: DiscoveryMode::Default,
            service_root_path: "/apps/loom".to_string(),
            custom_path: None,
        };
        let transport = Loopback::unary(
            Box::new(move |path| {
                if routes.serves(path) {
                    Ok(doc_bytes())
                } else {
                    Err(LoomError::new(Code::NotFound, "no doc here"))
                }
            }),
            Box::new(|_| Ok(Vec::new())),
        );
        let conn = block(RemoteConnection::connect(
            transport,
            "prod",
            &resolver(),
            DiscoveryMode::Default,
        ))
        .expect("connect");
        assert_eq!(conn.version(), 1);
        assert_eq!(conn.service_root(), "https://remote.host/apps/loom");
        assert!(conn.discovery().is_compatible(1, 1));
    }

    #[test]
    fn incompatible_protocol_is_rejected() {
        let transport = Loopback::unary(
            Box::new(|_| {
                let mut doc = Discovery::v1("https://h/x", "https://h/x/call", vec![], vec![]);
                doc.min_version = 2;
                doc.max_version = 2;
                Ok(doc.encode().unwrap())
            }),
            Box::new(|_| Ok(Vec::new())),
        );
        let result = block(RemoteConnection::connect(
            transport,
            "https://remote.host/apps/loom",
            &resolver(),
            DiscoveryMode::WellKnown,
        ));
        assert!(matches!(result, Err(e) if e.code == Code::Unsupported));
    }

    #[test]
    fn unavailable_endpoint_fails_fast() {
        let transport = Loopback::unary(
            Box::new(|_| Err(LoomError::new(Code::Io, "connection refused"))),
            Box::new(|_| Ok(Vec::new())),
        );
        let result = block(RemoteConnection::connect(
            transport,
            "prod",
            &resolver(),
            DiscoveryMode::Default,
        ));
        assert!(matches!(result, Err(e) if e.code == Code::Io));
    }

    #[test]
    fn local_locator_is_rejected() {
        let transport =
            Loopback::unary(Box::new(|_| Ok(doc_bytes())), Box::new(|_| Ok(Vec::new())));
        let result = block(RemoteConnection::connect(
            transport,
            "./local.loom",
            &ContextResolver::default(),
            DiscoveryMode::Default,
        ));
        assert!(matches!(result, Err(e) if e.code == Code::InvalidArgument));
    }
}
