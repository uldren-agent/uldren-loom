//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;
#[cfg(feature = "serve")]
use loom_hosted_core::remote::{RemoteAuthMode, RemoteServeOptions, RemoteTlsTrust};

#[derive(Default)]
pub(crate) struct ServePolicyArgs {
    pub tls_certificate_bundle: Option<String>,
    pub auth_mode: Option<String>,
    pub exposure: Option<String>,
    pub audit_mode: Option<String>,
    pub request_size_limit: Option<u64>,
    pub idle_timeout_ms: Option<u64>,
    pub session_timeout_ms: Option<u64>,
    pub network_access_policy: Option<String>,
}

struct ServeConfigureRequest {
    store: String,
    surface: String,
    selector: Vec<String>,
    bind: String,
    transport: Option<String>,
    profile: Option<String>,
    mode: Option<String>,
    disabled: bool,
    policy: ServePolicyArgs,
}

pub(crate) fn run_serve(action: ServeCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        ServeCmd::Configure(args) => run_serve_configure(
            ServeConfigureRequest {
                store: args.store,
                surface: args.surface,
                selector: args.selector,
                bind: args.bind,
                transport: args.transport,
                profile: args.profile,
                mode: args.mode,
                disabled: args.disabled,
                policy: ServePolicyArgs {
                    tls_certificate_bundle: args.tls_certificate_bundle,
                    auth_mode: args.auth_mode,
                    exposure: args.exposure,
                    audit_mode: args.audit_mode,
                    request_size_limit: args.request_size_limit,
                    idle_timeout_ms: args.idle_timeout_ms,
                    session_timeout_ms: args.session_timeout_ms,
                    network_access_policy: args.network_access_policy,
                },
            },
            keys,
        ),
        ServeCmd::List { store } => run_serve_list(&store, keys),
        ServeCmd::Enable { store, id } => run_serve_set_enabled(&store, &id, true, keys),
        ServeCmd::Disable { store, id } => run_serve_set_enabled(&store, &id, false, keys),
        ServeCmd::Remove { store, id } => run_serve_remove(&store, &id, keys),
        ServeCmd::Route { action } => run_serve_route(action, keys),
        #[cfg(feature = "serve")]
        ServeCmd::Remote(args) => run_serve_remote(*args),
    }
}

/// Parse a `--auth-mode` value into a [`RemoteAuthMode`].
#[cfg(feature = "serve")]
fn parse_remote_auth_mode(value: &str) -> Result<RemoteAuthMode, String> {
    match value {
        "interactive" => Ok(RemoteAuthMode::Interactive),
        "token" => Ok(RemoteAuthMode::Token),
        "mtls" => Ok(RemoteAuthMode::Mtls),
        "principal" => Ok(RemoteAuthMode::Principal),
        "external" => Ok(RemoteAuthMode::External),
        other => Err(format!(
            "unsupported --auth-mode {other:?} (expected interactive, token, mtls, principal, or external)"
        )),
    }
}

/// Parse a `--tls-trust` value into a [`RemoteTlsTrust`] (`system`, `insecure-dev`, or `bundle:NAME`).
#[cfg(feature = "serve")]
fn parse_remote_tls_trust(value: &str) -> Result<RemoteTlsTrust, String> {
    match value {
        "system" => Ok(RemoteTlsTrust::System),
        "insecure-dev" => Ok(RemoteTlsTrust::InsecureDev),
        other => other
            .strip_prefix("bundle:")
            .filter(|name| !name.is_empty())
            .map(|name| RemoteTlsTrust::Bundle(name.to_string()))
            .ok_or_else(|| {
                format!("unsupported --tls-trust {other:?} (expected system, insecure-dev, or bundle:NAME)")
            }),
    }
}

/// Build the serve options from the parsed CLI arguments, defaulting auth to `interactive`, advertised
/// trust to `system`, the session lease to one hour, and the request limit to 16 MiB.
#[cfg(feature = "serve")]
fn serve_remote_options(args: &ServeRemoteArgs) -> Result<RemoteServeOptions, String> {
    let auth_modes = if args.auth_modes.is_empty() {
        vec![RemoteAuthMode::Interactive]
    } else {
        args.auth_modes
            .iter()
            .map(|value| parse_remote_auth_mode(value))
            .collect::<Result<Vec<_>, _>>()?
    };
    let tls = if args.tls_trust.is_empty() {
        vec![RemoteTlsTrust::System]
    } else {
        args.tls_trust
            .iter()
            .map(|value| parse_remote_tls_trust(value))
            .collect::<Result<Vec<_>, _>>()?
    };
    Ok(RemoteServeOptions::from_cli(
        args.bind.clone(),
        args.service_root.clone(),
        args.call_endpoint.clone(),
        auth_modes,
        tls,
        args.session_lease_ms.unwrap_or(3_600_000),
        args.max_request_bytes.unwrap_or(16 * 1024 * 1024),
        args.network_access_policy.clone(),
    ))
}

/// Run MCP tools server-side, beside the served store, through the shared `LoomMcp` domain seam. The
/// hosted `RemoteRuntime` calls this for an `Mcp.call_tool` request.
#[cfg(all(feature = "serve", feature = "mcp"))]
struct ServedMcpExecutor {
    store: String,
}

#[cfg(all(feature = "serve", feature = "mcp"))]
impl ServedMcpExecutor {
    fn new(store: &str) -> Result<Self, String> {
        Ok(Self {
            store: store.to_string(),
        })
    }
}

#[cfg(all(feature = "serve", feature = "mcp"))]
impl loom_hosted_core::remote::McpToolExecutor for ServedMcpExecutor {
    fn call_tool(
        &self,
        ctx: &loom_hosted_core::remote::McpToolContext,
        name: &str,
        args: &[u8],
    ) -> Result<Vec<u8>, loom_types::LoomError> {
        let principal = ctx
            .session_principal
            .as_deref()
            .map(loom_core::WorkspaceId::parse)
            .transpose()?;
        let access = uldren_loom_mcp::StoreAccess::per_request_auth(
            &self.store,
            loom_store::LocalOpenAuth {
                preauthenticated_principal: principal,
                session_id: ctx
                    .session_principal
                    .as_ref()
                    .map(|principal| format!("remote-mcp:{principal}")),
                ..Default::default()
            },
        );
        let mcp = uldren_loom_mcp::LoomMcp::new(access);
        uldren_loom_mcp::server::execute_promoted_tool(&mcp, name, args)
    }
}

/// Bind a foreground remote endpoint: start a runtime over `store`, wrap it in the HTTP service, and bind
/// the HTTP/2-over-TLS carrier. Returns the running server (the accept loop runs on the current runtime).
#[cfg(feature = "serve")]
pub(crate) async fn bind_remote_endpoint(
    store: &str,
    options: &RemoteServeOptions,
    server_config: std::sync::Arc<rustls::ServerConfig>,
) -> Result<loom_hosted_core::remote_carrier::RemoteHttpServer, String> {
    use loom_hosted_core::remote::RemoteRuntime;
    use loom_hosted_core::remote_carrier::RemoteHttpServer;
    use loom_hosted_core::remote_http::RemoteHttpService;

    let addr: std::net::SocketAddr = options
        .bind
        .parse()
        .map_err(|e| format!("invalid --bind {:?}: {e}", options.bind))?;
    #[cfg_attr(not(feature = "mcp"), allow(unused_mut))]
    let mut runtime =
        RemoteRuntime::start(store, options.to_config()).map_err(|e| e.to_string())?;
    // Install the server-side MCP tool executor so manifest-routed families run beside the served store.
    // Without the `mcp` feature the executor is absent and manifest-routed tools reject.
    #[cfg(feature = "mcp")]
    runtime.set_mcp_executor(std::sync::Arc::new(ServedMcpExecutor::new(store)?));
    let service = std::sync::Arc::new(RemoteHttpService::new(
        std::sync::Arc::new(runtime),
        options.call_path(),
    ));
    RemoteHttpServer::bind(addr, server_config, service)
        .await
        .map_err(|e| format!("bind remote endpoint on {addr}: {e}"))
}

/// Run `loom serve remote`: validate options, load the TLS material, bind the HTTP/2-over-TLS carrier,
/// and serve until interrupted (SIGINT/SIGTERM), then shut the listener down.
#[cfg(feature = "serve")]
pub(crate) fn run_serve_remote(args: ServeRemoteArgs) -> Result<(), String> {
    let options = serve_remote_options(&args)?;
    options.validate().map_err(|e| e.to_string())?;
    let tls = loom_hosted_core::HostedTlsConfig::from_pem_files_with_client_trust(
        &args.tls_cert,
        &args.tls_key,
        args.tls_client_trust.as_deref(),
    )
    .map_err(|e| format!("load TLS material: {e}"))?;
    let server_config = tls.server_config();
    let store = args.store.clone();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("build async runtime: {e}"))?;
    runtime.block_on(async move {
        let server = bind_remote_endpoint(&store, &options, server_config).await?;
        println!(
            "{{\"listening\":{},\"service_root\":{},\"call_endpoint\":{}}}",
            json_string(&server.local_addr().to_string()),
            json_string(&options.service_root),
            json_string(&options.call_endpoint)
        );
        // Serve until an interrupt signal arrives, then release the socket.
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        ctrlc::set_handler(move || {
            let _ = tx.send(());
        })
        .map_err(|e| format!("install signal handler: {e}"))?;
        let _ = tokio::task::spawn_blocking(move || {
            let _ = rx.recv();
        })
        .await;
        server.shutdown();
        Ok::<(), String>(())
    })
}

fn run_serve_route(action: ServeRouteCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        ServeRouteCmd::List { store, listener } => run_serve_route_list(&store, &listener, keys),
        ServeRouteCmd::Set(args) => run_serve_route_set(*args, keys),
        ServeRouteCmd::Remove {
            store,
            listener,
            route,
        } => run_serve_route_remove(&store, &listener, &route, keys),
    }
}

fn run_serve_route_list(store: &str, listener: &str, keys: &KeyOpts) -> Result<(), String> {
    let client = remote::open_cli_generated_client(store, keys)?;
    let raw = execute_generated_string(
        &client,
        "ServeConfig",
        "serve_web_route_list_json",
        vec![listener.to_value()],
    )?;
    println!("{raw}");
    Ok(())
}

fn run_serve_route_set(args: ServeRouteSetArgs, keys: &KeyOpts) -> Result<(), String> {
    let request = serde_json::json!({
        "listener": args.listener,
        "route": args.route,
        "host": args.host,
        "prefix": args.prefix,
        "workspace": args.workspace,
        "root": args.root
    })
    .to_string();
    let client = remote::open_cli_generated_client(&args.store, keys)?;
    let raw = execute_generated_string(
        &client,
        "ServeConfig",
        "serve_web_route_set_json",
        vec![request.to_value()],
    )?;
    println!("{raw}");
    Ok(())
}

fn run_serve_route_remove(
    store: &str,
    listener: &str,
    route: &str,
    keys: &KeyOpts,
) -> Result<(), String> {
    let client = remote::open_cli_generated_client(store, keys)?;
    let raw = execute_generated_string(
        &client,
        "ServeConfig",
        "serve_web_route_remove_json",
        vec![listener.to_value(), route.to_value()],
    )?;
    println!("{raw}");
    Ok(())
}

fn run_serve_configure(request: ServeConfigureRequest, keys: &KeyOpts) -> Result<(), String> {
    let ServeConfigureRequest {
        store,
        surface,
        selector,
        bind,
        transport,
        profile,
        mode,
        disabled,
        policy,
    } = request;
    let request_json = serde_json::json!({
        "surface": surface,
        "selectors": selector,
        "bind": bind,
        "transport": transport,
        "profile": profile,
        "mode": mode,
        "enabled": !disabled,
        "tls_certificate_bundle": policy.tls_certificate_bundle,
        "auth_mode": policy.auth_mode,
        "exposure": policy.exposure,
        "audit_mode": policy.audit_mode,
        "request_size_limit": policy.request_size_limit,
        "idle_timeout_ms": policy.idle_timeout_ms,
        "session_timeout_ms": policy.session_timeout_ms,
        "network_access_policy": policy.network_access_policy
    })
    .to_string();
    let client = remote::open_cli_generated_client(&store, keys)?;
    let raw = execute_generated_string(
        &client,
        "ServeConfig",
        "serve_listener_configure_json",
        vec![request_json.to_value()],
    )?;
    println!("{raw}");
    Ok(())
}

fn run_serve_list(store: &str, keys: &KeyOpts) -> Result<(), String> {
    let client = remote::open_cli_generated_client(store, keys)?;
    let raw = execute_generated_string(&client, "ServeConfig", "serve_listener_list_json", vec![])?;
    println!("{raw}");
    Ok(())
}

fn run_serve_set_enabled(
    store: &str,
    id: &str,
    enabled: bool,
    keys: &KeyOpts,
) -> Result<(), String> {
    let client = remote::open_cli_generated_client(store, keys)?;
    let raw = execute_generated_string(
        &client,
        "ServeConfig",
        "serve_listener_set_enabled_json",
        vec![id.to_value(), enabled.to_value()],
    )?;
    println!("{raw}");
    Ok(())
}

fn run_serve_remove(store: &str, id: &str, keys: &KeyOpts) -> Result<(), String> {
    let client = remote::open_cli_generated_client(store, keys)?;
    let raw = execute_generated_string(
        &client,
        "ServeConfig",
        "serve_listener_remove_json",
        vec![id.to_value()],
    )?;
    println!("{raw}");
    Ok(())
}

#[cfg(all(test, feature = "integration-tests"))]
fn validate_bind(bind: &str) -> Result<(), String> {
    loom_client::serve_config::validate_bind(bind).map_err(loom_client::serve_config::cli_error)
}

#[cfg(all(test, feature = "integration-tests"))]
fn normalize_surface(surface: &str) -> Result<&'static str, String> {
    loom_client::serve_config::normalize_surface(surface)
        .map_err(loom_client::serve_config::cli_error)
}

#[cfg(all(test, feature = "integration-tests"))]
fn normalize_transport(surface: &str, transport: Option<&str>) -> Result<&'static str, String> {
    loom_client::serve_config::normalize_transport(surface, transport)
        .map_err(loom_client::serve_config::cli_error)
}

#[cfg(all(test, feature = "integration-tests"))]
fn normalize_transport_name(transport: &str) -> Result<&'static str, String> {
    loom_client::serve_config::normalize_transport_name(transport)
        .map_err(loom_client::serve_config::cli_error)
}

#[cfg(all(test, feature = "integration-tests"))]
fn validate_transport(surface: &str, transport: &str) -> Result<(), String> {
    loom_client::serve_config::validate_transport(surface, transport)
        .map_err(loom_client::serve_config::cli_error)
}

#[cfg(all(test, feature = "integration-tests"))]
fn validate_selector_shape(surface: &str, selectors: &[String]) -> Result<(), String> {
    loom_client::serve_config::validate_selector_shape(surface, selectors)
        .map_err(loom_client::serve_config::cli_error)
}

pub(crate) fn served_listener_target(record: &ServedListenerRecord) -> String {
    loom_client::serve_config::listener_target(record)
}

#[cfg(all(test, feature = "integration-tests"))]
mod tests {
    use super::*;

    fn selectors(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    fn temp_store(tag: &str) -> String {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("loomcli-{tag}-{}-{seq}.loom", std::process::id()));
        let _ = std::fs::remove_file(&path);
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn served_surface_registry_accepts_downstream_surfaces() {
        assert_eq!(normalize_surface("timeseries").unwrap(), "time-series");
        assert_eq!(
            normalize_transport("fts", Some("ndjson")).unwrap(),
            "ndjson"
        );
        assert_eq!(normalize_transport("redis", None).unwrap(), "resp");
        assert_eq!(normalize_transport("memcached", None).unwrap(), "text");
        assert_eq!(normalize_transport("etcd", None).unwrap(), "tcp");
        assert_eq!(normalize_transport("kafka", None).unwrap(), "tcp");
        assert_eq!(normalize_transport("mqtt", None).unwrap(), "tcp");
        assert_eq!(normalize_transport("nats", None).unwrap(), "tcp");
        assert_eq!(normalize_transport("postgres", None).unwrap(), "tcp");
        assert_eq!(normalize_transport("mysql", None).unwrap(), "tcp");
        assert_eq!(normalize_transport("neo4j", None).unwrap(), "tcp");
        assert_eq!(normalize_transport("influx", None).unwrap(), "http");
        assert_eq!(normalize_transport("prometheus", None).unwrap(), "http");
        assert_eq!(normalize_transport("grafana", None).unwrap(), "http");
        assert_eq!(normalize_transport("oci", None).unwrap(), "rest");
        assert_eq!(normalize_transport("s3", None).unwrap(), "rest");
        assert_eq!(normalize_transport("web", None).unwrap(), "rest");
        assert_eq!(normalize_transport("calendar", None).unwrap(), "caldav");
        validate_selector_shape("fts", &selectors(&["work", "docs"])).unwrap();
        validate_selector_shape("drive", &selectors(&["work"])).unwrap();
        validate_selector_shape("chat", &selectors(&["work", "general"])).unwrap();
        validate_selector_shape("meetings", &selectors(&["work"])).unwrap();
        validate_selector_shape("redis", &selectors(&["work", "default"])).unwrap();
        validate_selector_shape("memcached", &selectors(&["work", "sessions"])).unwrap();
        validate_selector_shape("etcd", &selectors(&["work", "config"])).unwrap();
        validate_selector_shape("kafka", &selectors(&["work"])).unwrap();
        validate_selector_shape("mqtt", &selectors(&["work"])).unwrap();
        validate_selector_shape("nats", &selectors(&["work"])).unwrap();
        validate_selector_shape("neo4j", &selectors(&["work", "people"])).unwrap();
        validate_selector_shape("influx", &selectors(&["work"])).unwrap();
        validate_selector_shape("prometheus", &selectors(&["work"])).unwrap();
        validate_selector_shape("grafana", &selectors(&["work"])).unwrap();
        validate_selector_shape("grafana", &selectors(&["work", "metrics"])).unwrap();
        validate_selector_shape("otlp", &selectors(&["work"])).unwrap();
        validate_selector_shape("oci", &selectors(&["work"])).unwrap();
        validate_selector_shape("s3", &selectors(&["work"])).unwrap();
        validate_selector_shape("s3", &selectors(&["work", "photos"])).unwrap();
        validate_selector_shape("dataframe", &selectors(&["work", "etl"])).unwrap();
        validate_selector_shape("files", &selectors(&["work"])).unwrap();
        validate_selector_shape("web", &selectors(&["work"])).unwrap();
        validate_selector_shape("exec", &selectors(&[])).unwrap();
        validate_transport("columnar", "arrow_flight").unwrap();
        validate_transport("dataframe", "rest").unwrap();
        validate_transport("document", "couchbase_document").unwrap();
        validate_transport("web", "rest").unwrap();
        validate_transport("drive", "rest").unwrap();
        validate_transport("drive", "json_rpc").unwrap();
        validate_transport("chat", "rest").unwrap();
        validate_transport("chat", "json_rpc").unwrap();
        validate_transport("exec", "grpc").unwrap();
        validate_transport("redis", "resp").unwrap();
        validate_transport("memcached", "text").unwrap();
        validate_transport("etcd", "tcp").unwrap();
        validate_transport("kafka", "tcp").unwrap();
        validate_transport("mqtt", "tcp").unwrap();
        validate_transport("nats", "tcp").unwrap();
        validate_transport("postgres", "tcp").unwrap();
        validate_transport("mysql", "tcp").unwrap();
        validate_transport("neo4j", "tcp").unwrap();
        validate_transport("influx", "http").unwrap();
        validate_transport("prometheus", "http").unwrap();
        validate_transport("grafana", "http").unwrap();
        validate_transport("otlp", "grpc").unwrap();
        validate_transport("otlp", "http").unwrap();
        validate_transport("oci", "rest").unwrap();
        validate_transport("s3", "rest").unwrap();
    }

    #[test]
    fn served_surface_registry_rejects_wrong_shape_or_transport() {
        assert!(normalize_surface("search").is_err());
        assert!(normalize_transport("mail", None).is_err());
        assert!(normalize_transport("kv", None).is_err());
        assert!(validate_selector_shape("graph", &selectors(&["work"])).is_err());
        assert!(validate_transport("files", "ndjson").is_err());
        assert!(validate_transport("web", "json_rpc").is_err());
        assert!(validate_selector_shape("web", &selectors(&[])).is_err());
        assert!(validate_selector_shape("web", &selectors(&["work", "extra"])).is_err());
        assert!(validate_transport("cas", "oci_distribution").is_err());
        assert!(validate_transport("files", "s3").is_err());
        assert!(validate_transport("s3", "json_rpc").is_err());
        assert!(validate_selector_shape("s3", &selectors(&[])).is_err());
        assert!(validate_selector_shape("s3", &selectors(&["work", "photos", "extra"])).is_err());
        assert!(validate_transport("dataframe", "json_rpc").is_err());
        assert!(validate_transport("kv", "mongodb_wire").is_err());
        assert!(normalize_transport("drive", None).is_err());
        assert!(validate_transport("drive", "grpc").is_err());
        assert!(validate_selector_shape("drive", &selectors(&[])).is_err());
        assert!(validate_selector_shape("drive", &selectors(&["work", "extra"])).is_err());
        assert!(normalize_transport("chat", None).is_err());
        assert!(validate_transport("chat", "grpc").is_err());
        assert!(validate_selector_shape("chat", &selectors(&["work"])).is_err());
        assert!(
            validate_selector_shape("chat", &selectors(&["work", "general", "extra"])).is_err()
        );
        assert!(validate_selector_shape("meetings", &selectors(&[])).is_err());
        assert!(validate_selector_shape("meetings", &selectors(&["work", "extra"])).is_err());
        assert!(validate_transport("kv", "resp").is_err());
        assert!(validate_transport("kv", "etcd_grpc").is_err());
        assert!(validate_transport("redis", "text").is_err());
        assert!(validate_transport("memcached", "resp").is_err());
        assert!(validate_transport("etcd", "grpc").is_err());
        assert!(validate_transport("queue", "kafka").is_err());
        assert!(validate_transport("queue", "nats").is_err());
        assert!(validate_transport("graph", "bolt").is_err());
        assert!(validate_transport("graph", "gremlin").is_err());
        assert!(validate_transport("neo4j", "rest").is_err());
        assert!(validate_transport("queue", "amqp").is_err());
        assert!(validate_transport("sql", "pg_wire").is_err());
        assert!(validate_transport("sql", "mysql_wire").is_err());
        assert!(validate_transport("vector", "pgvector_sql").is_err());
        assert!(normalize_transport_name("pgvector-sql").is_err());
        assert!(validate_transport("postgres", "rest").is_err());
        assert!(validate_transport("mysql", "grpc").is_err());
        assert!(validate_transport("influx", "rest").is_err());
        assert!(validate_transport("prometheus", "rest").is_err());
        assert!(validate_transport("grafana", "rest").is_err());
        assert!(validate_transport("time-series", "influx_rest").is_err());
        assert!(validate_transport("time-series", "prometheus_remote").is_err());
        assert!(validate_transport("time-series", "grafana_datasource").is_err());
        assert!(normalize_transport("otlp", None).is_err());
        assert!(validate_selector_shape("kafka", &selectors(&["work", "events"])).is_err());
        assert!(validate_selector_shape("influx", &selectors(&["work", "metrics"])).is_err());
    }

    #[test]
    fn serve_configure_admits_neo4j_tcp_listener_intent() {
        let store = temp_store("serve-neo4j-admission");
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        let root = WorkspaceId::v4_from_bytes([33; 16]);
        let workspace = WorkspaceId::v4_from_bytes([34; 16]);
        let mut loom = Loom::new(fs);
        loom.registry_mut()
            .create(FacetKind::Files, Some("work"), workspace)
            .unwrap();
        loom.registry_mut()
            .add_facet(workspace, FacetKind::Graph)
            .unwrap();
        let identity = IdentityStore::new(root);
        let mut acl = AclStore::new();
        acl.allow(AclSubject::Principal(root), None, None, [AclRight::Admin])
            .unwrap();
        loom.store().save_identity_store(&identity).unwrap();
        loom.store().save_acl_store(&acl).unwrap();
        save_loom(&mut loom).unwrap();
        drop(loom);

        run_serve_configure(
            ServeConfigureRequest {
                store: store.clone(),
                surface: "neo4j".to_string(),
                selector: selectors(&["work", "people"]),
                bind: "127.0.0.1:17687".to_string(),
                transport: None,
                profile: None,
                mode: None,
                disabled: true,
                policy: ServePolicyArgs::default(),
            },
            &KeyOpts::default(),
        )
        .unwrap();

        let fs = FileStore::open_read(&store).unwrap();
        let listener = fs.served_listeners().unwrap().remove(0);
        assert_eq!(listener.surface, "neo4j");
        assert_eq!(listener.transport, "tcp");
        assert_eq!(listener.selectors, vec!["work", "people"]);
        assert_eq!(listener.bind, "127.0.0.1:17687");
        assert!(!listener.enabled);
        let _ = std::fs::remove_file(&store);
    }

    #[test]
    fn serve_drive_configure_registers_policy_target() {
        let store = temp_store("serve-drive-policy-registry");
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        let root = WorkspaceId::v4_from_bytes([31; 16]);
        let workspace = WorkspaceId::v4_from_bytes([32; 16]);
        let mut loom = Loom::new(fs);
        loom.registry_mut()
            .create(FacetKind::Files, Some("work"), workspace)
            .unwrap();
        loom.registry_mut()
            .add_facet(workspace, FacetKind::Vcs)
            .unwrap();
        let identity = IdentityStore::new(root);
        let mut acl = AclStore::new();
        acl.allow(AclSubject::Principal(root), None, None, [AclRight::Admin])
            .unwrap();
        loom.store().save_identity_store(&identity).unwrap();
        loom.store().save_acl_store(&acl).unwrap();
        save_loom(&mut loom).unwrap();
        drop(loom);

        run_serve_configure(
            ServeConfigureRequest {
                store: store.clone(),
                surface: "drive".to_string(),
                selector: selectors(&["work"]),
                bind: "127.0.0.1:18080".to_string(),
                transport: Some("rest".to_string()),
                profile: None,
                mode: None,
                disabled: true,
                policy: ServePolicyArgs::default(),
            },
            &KeyOpts::default(),
        )
        .unwrap();

        let fs = FileStore::open_read(&store).unwrap();
        let registry = load_drive_policy_registry(&fs).unwrap();
        let target = registry.enabled_targets().next().unwrap();
        assert_eq!(target.workspace, workspace);
        assert_eq!(target.workspace_id, workspace.to_string());
        assert!(fs.audit_records().unwrap().iter().any(|record| {
            record.action == "drive.policy_registry.configure" && record.principal == Some(root)
        }));
        let _ = std::fs::remove_file(&store);
    }

    #[test]
    fn serve_route_set_and_remove_persist_web_listener_config() {
        let store = temp_store("serve-web-route");
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        let root = WorkspaceId::v4_from_bytes([41; 16]);
        let workspace = WorkspaceId::v4_from_bytes([42; 16]);
        let mut loom = Loom::new(fs);
        loom.registry_mut()
            .create(FacetKind::Files, Some("work"), workspace)
            .unwrap();
        loom.registry_mut()
            .add_facet(workspace, FacetKind::Vcs)
            .unwrap();
        let identity = IdentityStore::new(root);
        let mut acl = AclStore::new();
        acl.allow(AclSubject::Principal(root), None, None, [AclRight::Admin])
            .unwrap();
        loom.store().save_identity_store(&identity).unwrap();
        loom.store().save_acl_store(&acl).unwrap();
        save_loom(&mut loom).unwrap();
        drop(loom);

        run_serve_configure(
            ServeConfigureRequest {
                store: store.clone(),
                surface: "web".to_string(),
                selector: selectors(&["work"]),
                bind: "127.0.0.1:18081".to_string(),
                transport: None,
                profile: None,
                mode: None,
                disabled: true,
                policy: ServePolicyArgs::default(),
            },
            &KeyOpts::default(),
        )
        .unwrap();

        let fs = FileStore::open_read(&store).unwrap();
        let listener = fs.served_listeners().unwrap().remove(0);
        drop(fs);

        run_serve_route_set(
            ServeRouteSetArgs {
                store: store.clone(),
                listener: listener.id.clone(),
                route: "docs".to_string(),
                host: Some("docs.example.com".to_string()),
                prefix: "/docs".to_string(),
                workspace: Some("work".to_string()),
                root: "/site/docs".to_string(),
            },
            &KeyOpts::default(),
        )
        .unwrap();

        let fs = FileStore::open_read(&store).unwrap();
        let key = loom_substrate::web::web_profile_listener_key(&listener.id).unwrap();
        let web_listener =
            loom_substrate::web::WebListener::decode(&fs.control_get(&key).unwrap().unwrap())
                .unwrap();
        assert_eq!(web_listener.routes.routes.len(), 1);
        let route = &web_listener.routes.routes[0];
        assert_eq!(route.route_id, "docs");
        assert_eq!(route.host_pattern.as_deref(), Some("docs.example.com"));
        assert_eq!(route.path_prefix, "/docs");
        assert_eq!(route.workspace, Some(workspace));
        assert_eq!(route.root_path, "/site/docs");
        drop(fs);

        run_serve_route_remove(&store, &listener.id, "docs", &KeyOpts::default()).unwrap();

        let fs = FileStore::open_read(&store).unwrap();
        let web_listener =
            loom_substrate::web::WebListener::decode(&fs.control_get(&key).unwrap().unwrap())
                .unwrap();
        assert!(web_listener.routes.routes.is_empty());
        let actions = fs
            .audit_records()
            .unwrap()
            .into_iter()
            .map(|record| record.action)
            .collect::<Vec<_>>();
        assert!(actions.iter().any(|action| action == "serve.web.route.set"));
        assert!(
            actions
                .iter()
                .any(|action| action == "serve.web.route.remove")
        );
        let _ = std::fs::remove_file(&store);
    }

    #[test]
    fn mu_6h_h_d_serve_commands_delegate_to_generated_contracts() {
        let store = temp_store("serve-generated-delegation");
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        let root = WorkspaceId::v4_from_bytes([43; 16]);
        let workspace = WorkspaceId::v4_from_bytes([44; 16]);
        let mut loom = Loom::new(fs);
        loom.registry_mut()
            .create(FacetKind::Files, Some("work"), workspace)
            .unwrap();
        let identity = IdentityStore::new(root);
        let mut acl = AclStore::new();
        acl.allow(AclSubject::Principal(root), None, None, [AclRight::Admin])
            .unwrap();
        loom.store().save_identity_store(&identity).unwrap();
        loom.store().save_acl_store(&acl).unwrap();
        save_loom(&mut loom).unwrap();
        drop(loom);

        run_serve_configure(
            ServeConfigureRequest {
                store: store.clone(),
                surface: "web".to_string(),
                selector: selectors(&["work"]),
                bind: "127.0.0.1:18082".to_string(),
                transport: None,
                profile: None,
                mode: None,
                disabled: true,
                policy: ServePolicyArgs::default(),
            },
            &KeyOpts::default(),
        )
        .unwrap();
        let fs = FileStore::open_read(&store).unwrap();
        let listener = fs.served_listeners().unwrap().remove(0);
        drop(fs);

        run_serve_list(&store, &KeyOpts::default()).unwrap();
        run_serve_set_enabled(&store, &listener.id, true, &KeyOpts::default()).unwrap();
        run_serve_set_enabled(&store, &listener.id, false, &KeyOpts::default()).unwrap();
        run_serve_route_set(
            ServeRouteSetArgs {
                store: store.clone(),
                listener: listener.id.clone(),
                route: "docs".to_string(),
                host: None,
                prefix: "docs".to_string(),
                workspace: None,
                root: "site/docs".to_string(),
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run_serve_route_list(&store, &listener.id, &KeyOpts::default()).unwrap();
        run_serve_route_remove(&store, &listener.id, "docs", &KeyOpts::default()).unwrap();
        run_serve_remove(&store, &listener.id, &KeyOpts::default()).unwrap();

        let fs = FileStore::open_read(&store).unwrap();
        let actions = fs
            .audit_records()
            .unwrap()
            .into_iter()
            .map(|record| record.action)
            .collect::<Vec<_>>();
        for action in [
            "serve.listener.configure",
            "serve.listener.list",
            "serve.listener.enable",
            "serve.listener.disable",
            "serve.web.route.set",
            "serve.web.route.list",
            "serve.web.route.remove",
            "serve.listener.remove",
        ] {
            assert!(actions.iter().any(|seen| seen == action), "{action}");
        }
        assert!(fs.served_listeners().unwrap().is_empty());
        let _ = std::fs::remove_file(&store);
    }

    #[test]
    fn mu_6h_h_d_no_direct_serve_cli_mutation_paths_remain() {
        let source = include_str!("serve_cmd.rs");
        for forbidden in [
            concat!("save_served", "_listener_audited"),
            concat!("remove_served", "_listener_audited"),
            concat!("control_set", "_audited"),
            concat!("put_saved_state", "_served_listener"),
            concat!("audit", "_append"),
        ] {
            assert!(!source.contains(forbidden), "{forbidden}");
        }
    }

    #[cfg(feature = "serve")]
    fn serve_remote_args(store: String, bind: &str, cert: String, key: String) -> ServeRemoteArgs {
        ServeRemoteArgs {
            store,
            bind: bind.to_string(),
            service_root: "https://localhost/apps/loom".to_string(),
            call_endpoint: None,
            tls_cert: cert,
            tls_key: key,
            tls_client_trust: None,
            auth_modes: Vec::new(),
            tls_trust: Vec::new(),
            session_lease_ms: None,
            max_request_bytes: None,
            network_access_policy: None,
        }
    }

    #[cfg(feature = "serve")]
    #[test]
    fn serve_remote_options_apply_defaults_and_validate() {
        let args = serve_remote_args(
            "unused.loom".to_string(),
            "127.0.0.1:8443",
            "cert.pem".to_string(),
            "key.pem".to_string(),
        );
        let options = serve_remote_options(&args).expect("options");
        assert_eq!(options.call_endpoint, "https://localhost/apps/loom/v1/call");
        assert_eq!(options.call_path(), "/apps/loom/v1/call");
        assert_eq!(options.auth_modes, vec![RemoteAuthMode::Interactive]);
        assert_eq!(options.tls, vec![RemoteTlsTrust::System]);
        assert_eq!(options.session_lease_ms, 3_600_000);
        options.validate().expect("valid options");

        // Bad auth mode and bad trust selector are rejected.
        let mut bad = serve_remote_args(
            "unused.loom".to_string(),
            "127.0.0.1:8443",
            "cert.pem".to_string(),
            "key.pem".to_string(),
        );
        bad.auth_modes = vec!["nope".to_string()];
        assert!(serve_remote_options(&bad).is_err());
    }

    #[cfg(feature = "serve")]
    #[test]
    fn serve_remote_binds_over_tls_on_an_ephemeral_port() {
        let store = temp_store("serve-remote-bind");
        FileStore::create_with_profile(&store, Algo::Blake3).expect("create store");

        // A self-signed localhost cert written to temp PEM files, loaded through the same TLS path the
        // command uses.
        let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
        let dir = std::env::temp_dir();
        let cert_path = dir.join(format!("loom-serve-remote-{}.crt", std::process::id()));
        let key_path = dir.join(format!("loom-serve-remote-{}.key", std::process::id()));
        std::fs::write(&cert_path, cert.cert.pem()).unwrap();
        std::fs::write(&key_path, cert.signing_key.serialize_pem()).unwrap();

        let args = serve_remote_args(
            store.clone(),
            "127.0.0.1:0",
            cert_path.to_string_lossy().into_owned(),
            key_path.to_string_lossy().into_owned(),
        );
        let options = serve_remote_options(&args).expect("options");
        let tls = loom_hosted_core::HostedTlsConfig::from_pem_files(&args.tls_cert, &args.tls_key)
            .expect("tls material");

        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let server = bind_remote_endpoint(&store, &options, tls.server_config())
                .await
                .expect("bind");
            assert_ne!(
                server.local_addr().port(),
                0,
                "an ephemeral port was resolved and bound"
            );
            server.shutdown();
        });

        let _ = std::fs::remove_file(&store);
        let _ = std::fs::remove_file(&cert_path);
        let _ = std::fs::remove_file(&key_path);
    }
}

#[cfg(all(test, feature = "serve", feature = "mcp"))]
mod mcp_executor_tests {
    use super::*;
    use loom_codec::Value;
    use loom_core::identity::{IdentityStore, PrincipalKind};
    use loom_core::workspace::{FacetKind, WorkspaceId};
    use loom_core::{AclRight, AclStore, AclSubject, Algo, Digest, Loom};
    use loom_hosted_core::remote::{
        McpToolContext, McpToolExecutor, RemoteAuth, RemoteAuthMode, RemoteRuntime,
        RemoteServeOptions, RemoteTlsTrust,
    };
    use loom_remote_protocol::envelope::{Compression, Request, ResponsePayload};
    use loom_store::{FileStore, save_loom};
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_store(tag: &str) -> String {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "loomcli-mcp-{tag}-{}-{seq}.loom",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        path.to_string_lossy().into_owned()
    }

    fn remote_config() -> loom_hosted_core::remote::RemoteServerConfig {
        RemoteServeOptions::from_cli(
            "127.0.0.1:0".to_string(),
            "https://localhost/apps/loom".to_string(),
            None,
            vec![RemoteAuthMode::Interactive],
            vec![RemoteTlsTrust::InsecureDev],
            60_000,
            1 << 20,
            None,
        )
        .to_config()
    }

    fn request(session: &[u8], interface: &str, method: &str, args: Vec<Value>) -> Request {
        Request {
            request_id: vec![1],
            session_id: Some(session.to_vec()),
            interface: interface.to_string(),
            method: method.to_string(),
            args,
            deadline_ms: 0,
            idempotency_key: None,
            principal_hint: None,
            compression: Compression::None,
            stream: false,
        }
    }

    fn mcp_request(session: &[u8], name: &str, args: serde_json::Value) -> Request {
        request(
            session,
            "Mcp",
            "call_tool",
            vec![
                Value::Text(name.to_string()),
                Value::Bytes(serde_json::to_vec(&args).expect("json args")),
            ],
        )
    }

    fn seed_app_acl_store(path: &str) -> (WorkspaceId, WorkspaceId) {
        let root = WorkspaceId::v4_from_bytes([0x21; 16]);
        let denied = WorkspaceId::v4_from_bytes([0x22; 16]);
        let workspace = WorkspaceId::v4_from_bytes([0x23; 16]);
        let store = FileStore::create_with_profile(path, Algo::Blake3).expect("create store");
        let mut loom = Loom::new(store);
        loom.registry_mut()
            .create(FacetKind::Files, Some("repo"), workspace)
            .expect("workspace");
        let mut identity = IdentityStore::new(root);
        identity
            .add_principal(denied, "denied", PrincipalKind::User)
            .expect("denied principal");
        loom.store()
            .save_identity_store(&identity)
            .expect("save identity");
        let mut acl = AclStore::new();
        acl.allow(
            AclSubject::Principal(root),
            Some(workspace),
            Some(FacetKind::Files),
            [AclRight::Read, AclRight::Write],
        )
        .expect("allow root");
        loom.store().save_acl_store(&acl).expect("save acl");
        save_loom(&mut loom).expect("save store");
        (root, denied)
    }

    fn executor_call(
        executor: &ServedMcpExecutor,
        principal: WorkspaceId,
        name: &str,
        args: serde_json::Value,
    ) -> Result<Vec<u8>, loom_types::LoomError> {
        let ctx = McpToolContext {
            session_principal: Some(principal.to_string()),
            idempotency_key: None,
            deadline_ms: None,
        };
        McpToolExecutor::call_tool(
            executor,
            &ctx,
            name,
            &serde_json::to_vec(&args).expect("json args"),
        )
    }

    #[test]
    fn served_mcp_executor_binds_principal_for_server_executed_read_and_write() {
        let store = temp_store("principal-pep");
        let (allowed, denied) = seed_app_acl_store(&store);
        let executor = ServedMcpExecutor::new(&store).expect("executor");

        executor_call(
            &executor,
            allowed,
            "apps_create",
            serde_json::json!({
                "workspace": "repo",
                "app": "panel",
                "index_html": b"<!doctype html><html><body>allowed</body></html>".to_vec(),
                "meta_md": b"---\nname: Panel\n---\n".to_vec()
            }),
        )
        .expect("allowed server-executed write");

        let denied_write = executor_call(
            &executor,
            denied,
            "apps_write_file",
            serde_json::json!({
                "workspace": "repo",
                "app": "panel",
                "path": "assets/data.json",
                "content": b"{\"mutated\":true}".to_vec(),
                "mode": 0o100644
            }),
        )
        .expect_err("denied server-executed write");
        assert_eq!(denied_write.code, loom_types::Code::PermissionDenied);

        let denied_read = executor_call(
            &executor,
            denied,
            "apps_read_file",
            serde_json::json!({
                "workspace": "repo",
                "app": "panel",
                "path": "index.html"
            }),
        )
        .expect_err("denied server-executed read");
        assert_eq!(denied_read.code, loom_types::Code::PermissionDenied);

        let read = executor_call(
            &executor,
            allowed,
            "apps_read_file",
            serde_json::json!({
                "workspace": "repo",
                "app": "panel",
                "path": "index.html"
            }),
        )
        .expect("allowed server-executed read");
        let value: serde_json::Value = serde_json::from_slice(&read).expect("json result");
        assert_eq!(
            value["value"],
            serde_json::json!(b"<!doctype html><html><body>allowed</body></html>".to_vec())
        );

        let _ = std::fs::remove_file(&store);
    }

    #[test]
    fn remote_runtime_alternates_generated_and_server_executed_document_paths() {
        let store = temp_store("alternating-coherence");
        FileStore::create_with_profile(&store, Algo::Blake3).expect("create store");
        let mut runtime = RemoteRuntime::start(&store, remote_config()).expect("start runtime");
        runtime.set_mcp_executor(std::sync::Arc::new(
            ServedMcpExecutor::new(&store).expect("executor"),
        ));
        let conn = runtime.register_connection("peer");
        let session = runtime
            .open_session(conn, RemoteAuth::Unauthenticated)
            .expect("session");

        let first = b"one alpha".to_vec();
        let first_digest = Digest::hash(Algo::Blake3, &first).to_string();
        match runtime
            .dispatch(&request(
                &session.id,
                "Document",
                "put_binary",
                vec![
                    Value::Null,
                    Value::Text("repo".to_string()),
                    Value::Text("notes".to_string()),
                    Value::Text("doc".to_string()),
                    Value::Bytes(first),
                    Value::Null,
                ],
            ))
            .payload
        {
            ResponsePayload::Ok(Value::Bytes(_)) => {}
            other => panic!("generated document put failed: {other:?}"),
        }

        match runtime
            .dispatch(&mcp_request(
                &session.id,
                "document_replace_text",
                serde_json::json!({
                    "workspace": "repo",
                    "collection": "notes",
                    "id": "doc",
                    "base_digest": first_digest,
                    "find": "one",
                    "replace": "two",
                    "replace_all": false
                }),
            ))
            .payload
        {
            ResponsePayload::Ok(Value::Bytes(bytes)) => {
                let value: serde_json::Value = serde_json::from_slice(&bytes).expect("json result");
                assert_eq!(value["value"]["replacements"], serde_json::json!(1));
            }
            other => panic!("server-executed replace failed: {other:?}"),
        }

        match runtime
            .dispatch(&request(
                &session.id,
                "Document",
                "get_binary",
                vec![
                    Value::Null,
                    Value::Text("repo".to_string()),
                    Value::Text("notes".to_string()),
                    Value::Text("doc".to_string()),
                ],
            ))
            .payload
        {
            ResponsePayload::Ok(Value::Bytes(bytes)) => {
                let decoded = loom_codec::decode(&bytes).expect("document cbor");
                let Value::Array(items) = decoded else {
                    panic!("document result shape: {decoded:?}");
                };
                assert_eq!(items.first(), Some(&Value::Bytes(b"two alpha".to_vec())));
            }
            other => panic!("generated document get failed: {other:?}"),
        }

        let second_base_digest = Digest::hash(Algo::Blake3, b"two alpha").to_string();
        match runtime
            .dispatch(&mcp_request(
                &session.id,
                "document_replace_text",
                serde_json::json!({
                    "workspace": "repo",
                    "collection": "notes",
                    "id": "doc",
                    "base_digest": second_base_digest,
                    "find": "two alpha",
                    "replace": "three beta",
                    "replace_all": false
                }),
            ))
            .payload
        {
            ResponsePayload::Ok(Value::Bytes(_)) => {}
            other => panic!("server-executed document write failed: {other:?}"),
        }

        match runtime
            .dispatch(&request(
                &session.id,
                "Document",
                "get_binary",
                vec![
                    Value::Null,
                    Value::Text("repo".to_string()),
                    Value::Text("notes".to_string()),
                    Value::Text("doc".to_string()),
                ],
            ))
            .payload
        {
            ResponsePayload::Ok(Value::Bytes(bytes)) => {
                let decoded = loom_codec::decode(&bytes).expect("document cbor");
                let Value::Array(items) = decoded else {
                    panic!("document result shape: {decoded:?}");
                };
                assert_eq!(items.first(), Some(&Value::Bytes(b"three beta".to_vec())));
            }
            other => panic!("generated document get after server write failed: {other:?}"),
        }

        runtime.shutdown();
        let _ = std::fs::remove_file(&store);
    }
}
