//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;
#[cfg(feature = "serve")]
use loom_hosted_core::remote::{RemoteAuthMode, RemoteServeOptions, RemoteTlsTrust};

#[derive(Default)]
pub(crate) struct ServePolicyArgs {
    pub tls_certificate_bundle: Option<String>,
    pub tls_mode: Option<String>,
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

struct ServedSurfaceSpec {
    surface: &'static str,
    aliases: &'static [&'static str],
    min_selectors: usize,
    max_selectors: usize,
    default_transport: Option<&'static str>,
    transports: &'static [&'static str],
}

const SERVED_SURFACES: &[ServedSurfaceSpec] = &[
    ServedSurfaceSpec {
        surface: "admin",
        aliases: &[],
        min_selectors: 0,
        max_selectors: 0,
        default_transport: Some("rest"),
        transports: &["rest", "json_rpc"],
    },
    ServedSurfaceSpec {
        surface: "mcp",
        aliases: &[],
        min_selectors: 0,
        max_selectors: 0,
        default_transport: Some("mcp_http"),
        transports: &["mcp_http"],
    },
    ServedSurfaceSpec {
        surface: "exec",
        aliases: &[],
        min_selectors: 0,
        max_selectors: 0,
        default_transport: None,
        transports: &["rest", "json_rpc", "grpc"],
    },
    ServedSurfaceSpec {
        surface: "cas",
        aliases: &[],
        min_selectors: 1,
        max_selectors: 1,
        default_transport: Some("rest"),
        transports: &["rest", "json_rpc", "grpc"],
    },
    ServedSurfaceSpec {
        surface: "oci",
        aliases: &[],
        min_selectors: 1,
        max_selectors: 1,
        default_transport: Some("rest"),
        transports: &["rest"],
    },
    ServedSurfaceSpec {
        surface: "s3",
        aliases: &[],
        min_selectors: 1,
        max_selectors: 2,
        default_transport: Some("rest"),
        transports: &["rest"],
    },
    ServedSurfaceSpec {
        surface: "files",
        aliases: &[],
        min_selectors: 1,
        max_selectors: 1,
        default_transport: None,
        transports: &["rest", "json_rpc", "grpc"],
    },
    ServedSurfaceSpec {
        surface: "web",
        aliases: &[],
        min_selectors: 1,
        max_selectors: 1,
        default_transport: Some("rest"),
        transports: &["rest"],
    },
    ServedSurfaceSpec {
        surface: "vcs",
        aliases: &[],
        min_selectors: 1,
        max_selectors: 1,
        default_transport: None,
        transports: &["rest", "json_rpc", "grpc"],
    },
    ServedSurfaceSpec {
        surface: "sql",
        aliases: &[],
        min_selectors: 2,
        max_selectors: 2,
        default_transport: None,
        transports: &["rest", "json_rpc", "grpc"],
    },
    ServedSurfaceSpec {
        surface: "postgres",
        aliases: &[],
        min_selectors: 2,
        max_selectors: 2,
        default_transport: Some("tcp"),
        transports: &["tcp"],
    },
    ServedSurfaceSpec {
        surface: "mysql",
        aliases: &[],
        min_selectors: 2,
        max_selectors: 2,
        default_transport: Some("tcp"),
        transports: &["tcp"],
    },
    ServedSurfaceSpec {
        surface: "kv",
        aliases: &[],
        min_selectors: 2,
        max_selectors: 2,
        default_transport: None,
        transports: &["rest", "json_rpc", "grpc", "couchbase_kv"],
    },
    ServedSurfaceSpec {
        surface: "etcd",
        aliases: &[],
        min_selectors: 2,
        max_selectors: 2,
        default_transport: Some("tcp"),
        transports: &["tcp"],
    },
    ServedSurfaceSpec {
        surface: "redis",
        aliases: &[],
        min_selectors: 2,
        max_selectors: 2,
        default_transport: Some("resp"),
        transports: &["resp"],
    },
    ServedSurfaceSpec {
        surface: "memcached",
        aliases: &[],
        min_selectors: 2,
        max_selectors: 2,
        default_transport: Some("text"),
        transports: &["text"],
    },
    ServedSurfaceSpec {
        surface: "document",
        aliases: &[],
        min_selectors: 2,
        max_selectors: 2,
        default_transport: None,
        transports: &[
            "rest",
            "json_rpc",
            "grpc",
            "mongodb_wire",
            "couchdb_rest",
            "couchbase_document",
        ],
    },
    ServedSurfaceSpec {
        surface: "drive",
        aliases: &[],
        min_selectors: 1,
        max_selectors: 1,
        default_transport: None,
        transports: &["rest", "json_rpc"],
    },
    ServedSurfaceSpec {
        surface: "chat",
        aliases: &[],
        min_selectors: 2,
        max_selectors: 2,
        default_transport: None,
        transports: &["rest", "json_rpc"],
    },
    ServedSurfaceSpec {
        surface: "meetings",
        aliases: &[],
        min_selectors: 1,
        max_selectors: 1,
        default_transport: None,
        transports: &["rest", "json_rpc"],
    },
    ServedSurfaceSpec {
        surface: "queue",
        aliases: &[],
        min_selectors: 2,
        max_selectors: 2,
        default_transport: None,
        transports: &["rest", "json_rpc", "grpc"],
    },
    ServedSurfaceSpec {
        surface: "kafka",
        aliases: &[],
        min_selectors: 1,
        max_selectors: 1,
        default_transport: Some("tcp"),
        transports: &["tcp"],
    },
    ServedSurfaceSpec {
        surface: "mqtt",
        aliases: &[],
        min_selectors: 1,
        max_selectors: 1,
        default_transport: Some("tcp"),
        transports: &["tcp"],
    },
    ServedSurfaceSpec {
        surface: "nats",
        aliases: &[],
        min_selectors: 1,
        max_selectors: 1,
        default_transport: Some("tcp"),
        transports: &["tcp"],
    },
    ServedSurfaceSpec {
        surface: "time-series",
        aliases: &["timeseries", "time_series"],
        min_selectors: 2,
        max_selectors: 2,
        default_transport: None,
        transports: &["rest", "json_rpc", "grpc"],
    },
    ServedSurfaceSpec {
        surface: "influx",
        aliases: &[],
        min_selectors: 1,
        max_selectors: 1,
        default_transport: Some("http"),
        transports: &["http"],
    },
    ServedSurfaceSpec {
        surface: "prometheus",
        aliases: &[],
        min_selectors: 1,
        max_selectors: 1,
        default_transport: Some("http"),
        transports: &["http"],
    },
    ServedSurfaceSpec {
        surface: "grafana",
        aliases: &[],
        min_selectors: 1,
        max_selectors: 2,
        default_transport: Some("http"),
        transports: &["http"],
    },
    ServedSurfaceSpec {
        surface: "otlp",
        aliases: &[],
        min_selectors: 1,
        max_selectors: 1,
        default_transport: None,
        transports: &["grpc", "http"],
    },
    ServedSurfaceSpec {
        surface: "columnar",
        aliases: &[],
        min_selectors: 2,
        max_selectors: 2,
        default_transport: None,
        transports: &[
            "rest",
            "json_rpc",
            "grpc",
            "arrow_flight",
            "parquet",
            "duckdb_like",
            "snowflake_like",
            "spark_like",
            "bigquery_like",
        ],
    },
    ServedSurfaceSpec {
        surface: "dataframe",
        aliases: &[],
        min_selectors: 2,
        max_selectors: 2,
        default_transport: None,
        transports: &["rest"],
    },
    ServedSurfaceSpec {
        surface: "vector",
        aliases: &[],
        min_selectors: 2,
        max_selectors: 2,
        default_transport: None,
        transports: &["rest", "json_rpc", "grpc"],
    },
    ServedSurfaceSpec {
        surface: "fts",
        aliases: &[],
        min_selectors: 2,
        max_selectors: 2,
        default_transport: None,
        transports: &["rest", "json_rpc", "grpc", "ndjson"],
    },
    ServedSurfaceSpec {
        surface: "graph",
        aliases: &[],
        min_selectors: 2,
        max_selectors: 2,
        default_transport: None,
        transports: &["rest", "json_rpc", "grpc"],
    },
    ServedSurfaceSpec {
        surface: "neo4j",
        aliases: &[],
        min_selectors: 2,
        max_selectors: 2,
        default_transport: Some("tcp"),
        transports: &["tcp"],
    },
    ServedSurfaceSpec {
        surface: "ledger",
        aliases: &[],
        min_selectors: 2,
        max_selectors: 2,
        default_transport: None,
        transports: &[
            "rest",
            "json_rpc",
            "grpc",
            "immudb_grpc",
            "transparency_log",
        ],
    },
    ServedSurfaceSpec {
        surface: "calendar",
        aliases: &[],
        min_selectors: 1,
        max_selectors: 1,
        default_transport: Some("caldav"),
        transports: &["rest", "json_rpc", "caldav"],
    },
    ServedSurfaceSpec {
        surface: "contacts",
        aliases: &[],
        min_selectors: 1,
        max_selectors: 1,
        default_transport: Some("carddav"),
        transports: &["rest", "json_rpc", "carddav"],
    },
    ServedSurfaceSpec {
        surface: "mail",
        aliases: &[],
        min_selectors: 1,
        max_selectors: 1,
        default_transport: None,
        transports: &["rest", "json_rpc", "imap", "jmap", "smtp"],
    },
];

fn served_surface_spec(surface: &str) -> Option<&'static ServedSurfaceSpec> {
    SERVED_SURFACES
        .iter()
        .find(|spec| spec.surface == surface || spec.aliases.contains(&surface))
}

fn apply_serve_policy(record: &mut ServedListenerRecord, policy: ServePolicyArgs) {
    if let Some(value) = policy.tls_certificate_bundle {
        record.tls.mode = policy.tls_mode.unwrap_or_else(|| "direct".to_string());
        record.tls.certificate_bundle_ref = Some(value);
    } else if let Some(value) = policy.tls_mode {
        record.tls.mode = value;
    }
    if let Some(value) = policy.auth_mode {
        record.auth.mode = value;
    }
    if let Some(value) = policy.exposure {
        record.exposure = value;
    }
    if let Some(value) = policy.audit_mode {
        record.audit.mode = value;
    }
    if let Some(value) = policy.request_size_limit {
        record.limits.request_size_limit = value;
    }
    if let Some(value) = policy.idle_timeout_ms {
        record.limits.idle_timeout_ms = value;
    }
    if let Some(value) = policy.session_timeout_ms {
        record.limits.session_timeout_ms = value;
    }
    if let Some(value) = policy.network_access_policy {
        record.network_access_policy_ref = Some(value);
    }
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
                    tls_mode: args.tls_mode,
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
/// hosted `RemoteRuntime` calls this for an `Mcp.call_tool` request. The server-promoted families and the
/// single-shot `ask_answers` poll (whose bounded wait is driven client-side) execute here; every other
/// tool rejects. Opens the served store per request (the runtime serializes the call under its write
/// authority).
#[cfg(all(feature = "serve", feature = "mcp"))]
struct ServedMcpExecutor {
    mcp: uldren_loom_mcp::LoomMcp,
}

#[cfg(all(feature = "serve", feature = "mcp"))]
impl ServedMcpExecutor {
    fn new(store: &str) -> Self {
        Self {
            mcp: uldren_loom_mcp::LoomMcp::new(uldren_loom_mcp::StoreAccess::per_request(
                store, None,
            )),
        }
    }
}

#[cfg(all(feature = "serve", feature = "mcp"))]
impl loom_hosted_core::remote::McpToolExecutor for ServedMcpExecutor {
    fn call_tool(
        &self,
        _ctx: &loom_hosted_core::remote::McpToolContext,
        name: &str,
        args: &[u8],
    ) -> Result<Vec<u8>, loom_types::LoomError> {
        uldren_loom_mcp::server::execute_promoted_tool(&self.mcp, name, args)
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
    // Install the server-side MCP tool executor so promoted families run beside the served store.
    // Without the `mcp` feature the executor is absent and promoted tools reject.
    #[cfg(feature = "mcp")]
    runtime.set_mcp_executor(std::sync::Arc::new(ServedMcpExecutor::new(store)));
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
    let loom = cli_open_loom(store, keys)?;
    let actor = require_global_admin_actor(&loom)?;
    let record = require_web_listener_record(&loom, listener)?;
    let web_listener = web_listener_from_record(&loom, &record)?;
    let seq = loom
        .store()
        .audit_append(
            Some(actor),
            "serve.web.route.list",
            Some(&format!("listener={listener}")),
        )
        .map_err(|e| e.to_string())?;
    println!("{}", web_listener_json(&web_listener, Some(seq)));
    Ok(())
}

fn run_serve_route_set(args: ServeRouteSetArgs, keys: &KeyOpts) -> Result<(), String> {
    let loom = cli_open_loom(&args.store, keys)?;
    let actor = require_global_admin_actor(&loom)?;
    let record = require_web_listener_record(&loom, &args.listener)?;
    let mut web_listener = web_listener_from_record(&loom, &record)?;
    let workspace = args
        .workspace
        .as_deref()
        .map(|workspace| resolve_ns(&loom, workspace))
        .transpose()?;
    let mut route = loom_substrate::web::WebRoute::new(
        args.route.clone(),
        vec![
            loom_substrate::web::WebMethod::Get,
            loom_substrate::web::WebMethod::Head,
        ],
        args.host.clone(),
        &args.prefix,
        &args.root,
        loom_substrate::web::WebRouteMode::StaticFile,
    )
    .map_err(|e| e.to_string())?;
    route.workspace = workspace;
    web_listener
        .routes
        .routes
        .retain(|existing| existing.route_id != route.route_id);
    web_listener.routes.routes.push(route);
    web_listener.routes = loom_substrate::web::WebRouteTable::new(web_listener.routes.routes)
        .map_err(|e| e.to_string())?;
    let seq = save_web_listener_config(
        &loom,
        actor,
        &web_listener,
        "serve.web.route.set",
        &format!("listener={};route={}", args.listener, args.route),
    )?;
    println!("{}", web_listener_json(&web_listener, Some(seq)));
    Ok(())
}

fn run_serve_route_remove(
    store: &str,
    listener: &str,
    route: &str,
    keys: &KeyOpts,
) -> Result<(), String> {
    let loom = cli_open_loom(store, keys)?;
    let actor = require_global_admin_actor(&loom)?;
    let record = require_web_listener_record(&loom, listener)?;
    let mut web_listener = web_listener_from_record(&loom, &record)?;
    let before = web_listener.routes.routes.len();
    web_listener
        .routes
        .routes
        .retain(|existing| existing.route_id != route);
    if web_listener.routes.routes.len() == before {
        return Err(format!("web route {route:?} not found"));
    }
    web_listener.routes = loom_substrate::web::WebRouteTable::new(web_listener.routes.routes)
        .map_err(|e| e.to_string())?;
    let seq = save_web_listener_config(
        &loom,
        actor,
        &web_listener,
        "serve.web.route.remove",
        &format!("listener={listener};route={route}"),
    )?;
    println!("{}", web_listener_json(&web_listener, Some(seq)));
    Ok(())
}

fn require_web_listener_record(
    loom: &Loom<FileStore>,
    listener: &str,
) -> Result<ServedListenerRecord, String> {
    let record = loom
        .store()
        .served_listener(listener)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("served listener {listener:?} not found"))?;
    if record.surface != "web" || record.transport != "rest" {
        return Err(format!(
            "served listener {listener:?} is not a web rest listener"
        ));
    }
    if record.selectors.len() != 1 {
        return Err(format!(
            "served listener {listener:?} must have exactly one workspace selector"
        ));
    }
    Ok(record)
}

fn web_listener_from_record(
    loom: &Loom<FileStore>,
    record: &ServedListenerRecord,
) -> Result<loom_substrate::web::WebListener, String> {
    let key =
        loom_substrate::web::web_profile_listener_key(&record.id).map_err(|e| e.to_string())?;
    if let Some(bytes) = loom.store().control_get(&key).map_err(|e| e.to_string())? {
        return loom_substrate::web::WebListener::decode(&bytes).map_err(|e| e.to_string());
    }
    let workspace = resolve_ns(loom, &record.selectors[0])?;
    let addr = record
        .bind
        .parse::<std::net::SocketAddr>()
        .map_err(|e| format!("invalid listener bind address {:?}: {e}", record.bind))?;
    loom_substrate::web::WebListener::new(
        &record.id,
        addr.ip().to_string(),
        addr.port(),
        loom_substrate::web::WebProtocol::Http,
        workspace,
        "/",
    )
    .map_err(|e| e.to_string())
}

fn save_web_listener_config(
    loom: &Loom<FileStore>,
    actor: WorkspaceId,
    listener: &loom_substrate::web::WebListener,
    action: &str,
    target: &str,
) -> Result<u64, String> {
    let key =
        loom_substrate::web::web_profile_listener_key(&listener.listener_id).map_err(|e| {
            format!(
                "build Webish listener config key for served listener {}: {e}",
                listener.listener_id
            )
        })?;
    let value = listener.encode().map_err(|e| e.to_string())?;
    loom.store()
        .control_set_audited(&key, value, Some(actor), action, Some(target))
        .map_err(|e| e.to_string())
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
    validate_bind(&bind)?;
    let surface = normalize_surface(&surface)?;
    validate_selector_shape(surface, &selector)?;
    let transport = normalize_transport(surface, transport.as_deref())?;
    validate_transport(surface, transport)?;
    let profile = normalize_profile(surface, transport, profile.as_deref(), mode.as_deref())?;
    let mut loom = cli_open_loom(&store, keys)?;
    let actor = require_global_admin_actor(&loom)?;
    let drive_policy_target = if surface == "drive" {
        let workspace = resolve_ns(&loom, &selector[0])?;
        Some((workspace, workspace.to_string()))
    } else {
        None
    };
    let mut record = FileStore::served_listener_record_with_profile(
        surface, selector, transport, profile, &bind, !disabled,
    )
    .map_err(|e| e.to_string())?;
    apply_serve_policy(&mut record, policy);
    validate_served_listener_certificate_bundle(&loom, &record)?;
    validate_served_listener_network_access_policy(&loom, &record)?;
    if configure_memcached_cache_mode(
        &mut loom,
        surface,
        record.profile.as_deref(),
        &record.selectors,
    )? {
        save_loom(&mut loom).map_err(|e| e.to_string())?;
    }
    let target = served_listener_target(&record);
    let seq = loom
        .store()
        .save_served_listener_audited(
            &record,
            Some(actor),
            "serve.listener.configure",
            Some(&target),
        )
        .map_err(|e| e.to_string())?;
    record.last_modified_audit_seq = Some(seq);
    if let Some((workspace, workspace_id)) = drive_policy_target {
        register_drive_policy_target(&loom, Some(actor), workspace, &workspace_id)?;
    }
    println!("{}", served_listener_json(&record, seq));
    Ok(())
}

fn validate_served_listener_network_access_policy(
    loom: &Loom<FileStore>,
    record: &ServedListenerRecord,
) -> Result<(), String> {
    let Some(name) = record.network_access_policy_ref.as_deref() else {
        return Ok(());
    };
    let Some(policy) = loom
        .store()
        .network_access_policy(name)
        .map_err(|e| e.to_string())?
    else {
        return Err(format!("network access policy {name:?} not found"));
    };
    if !network_access_policy_requires_mtls(&policy) {
        return Ok(());
    }
    if record.tls.mode != "direct" {
        return Err(format!(
            "network access policy {name:?} requires mTLS but listener TLS is not direct"
        ));
    }
    let bundle_name = record
        .tls
        .certificate_bundle_ref
        .as_deref()
        .ok_or_else(|| {
            format!("network access policy {name:?} requires a TLS certificate bundle")
        })?;
    let bundle = loom
        .store()
        .certificate_bundle(bundle_name)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("certificate bundle {bundle_name:?} not found"))?;
    if bundle.trust_bundle_pem.is_none() {
        return Err(format!(
            "network access policy {name:?} requires mTLS but certificate bundle {bundle_name:?} has no trust bundle"
        ));
    }
    Ok(())
}

fn network_access_policy_requires_mtls(policy: &loom_store::NetworkAccessPolicyRecord) -> bool {
    policy.rules.iter().any(|rule| {
        rule.require_mtls
            || rule.client_cert_subject.is_some()
            || rule.client_cert_san.is_some()
            || rule.client_cert_issuer.is_some()
    })
}

fn validate_served_listener_certificate_bundle(
    loom: &Loom<FileStore>,
    record: &ServedListenerRecord,
) -> Result<(), String> {
    let Some(name) = record.tls.certificate_bundle_ref.as_deref() else {
        return Ok(());
    };
    if loom
        .store()
        .certificate_bundle(name)
        .map_err(|e| e.to_string())?
        .is_none()
    {
        return Err(format!("certificate bundle {name:?} not found"));
    }
    Ok(())
}

fn run_serve_list(store: &str, keys: &KeyOpts) -> Result<(), String> {
    let loom = cli_open_loom(store, keys)?;
    let actor = require_global_admin_actor(&loom)?;
    let listeners = loom.store().served_listeners().map_err(|e| e.to_string())?;
    let seq = loom
        .store()
        .audit_append(Some(actor), "serve.listener.list", Some("listeners"))
        .map_err(|e| e.to_string())?;
    let mut out = format!("{{\"seq\":{seq},\"listeners\":[");
    for (idx, listener) in listeners.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&served_listener_record_json(listener));
    }
    out.push_str("]}");
    println!("{out}");
    Ok(())
}

fn run_serve_set_enabled(
    store: &str,
    id: &str,
    enabled: bool,
    keys: &KeyOpts,
) -> Result<(), String> {
    let loom = cli_open_loom(store, keys)?;
    let actor = require_global_admin_actor(&loom)?;
    let mut record = loom
        .store()
        .served_listener(id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("served listener {id:?} not found"))?;
    record.enabled = enabled;
    let target = served_listener_target(&record);
    let audit_action = if enabled {
        "serve.listener.enable"
    } else {
        "serve.listener.disable"
    };
    let seq = loom
        .store()
        .save_served_listener_audited(&record, Some(actor), audit_action, Some(&target))
        .map_err(|e| e.to_string())?;
    record.last_modified_audit_seq = Some(seq);
    println!("{}", served_listener_json(&record, seq));
    Ok(())
}

fn run_serve_remove(store: &str, id: &str, keys: &KeyOpts) -> Result<(), String> {
    let loom = cli_open_loom(store, keys)?;
    let actor = require_global_admin_actor(&loom)?;
    let record = loom
        .store()
        .served_listener(id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("served listener {id:?} not found"))?;
    let target = served_listener_target(&record);
    let seq = loom
        .store()
        .remove_served_listener_audited(id, Some(actor), "serve.listener.remove", Some(&target))
        .map_err(|e| e.to_string())?;
    println!("{{\"seq\":{seq},\"id\":{}}}", json_string(id));
    Ok(())
}

fn validate_bind(bind: &str) -> Result<(), String> {
    let addr = bind
        .parse::<std::net::SocketAddr>()
        .map_err(|e| format!("invalid --bind address {bind:?}: {e}"))?;
    if addr.port() == 0 {
        return Err("--bind port must not be 0 for durable listener configuration".to_string());
    }
    Ok(())
}

fn normalize_surface(surface: &str) -> Result<&'static str, String> {
    served_surface_spec(surface)
        .map(|spec| spec.surface)
        .ok_or_else(|| format!("unsupported served surface {surface:?}"))
}

fn normalize_transport(surface: &str, transport: Option<&str>) -> Result<&'static str, String> {
    let spec = served_surface_spec(surface)
        .ok_or_else(|| format!("unsupported served surface {surface:?}"))?;
    match transport {
        Some(value) => normalize_transport_name(value),
        None => spec
            .default_transport
            .ok_or_else(|| format!("served surface {surface:?} requires an explicit --transport")),
    }
}

fn normalize_transport_name(transport: &str) -> Result<&'static str, String> {
    match transport {
        "rest" => Ok("rest"),
        "json-rpc" | "json_rpc" => Ok("json_rpc"),
        "mcp-http" | "mcp_http" => Ok("mcp_http"),
        "grpc" => Ok("grpc"),
        "tcp" => Ok("tcp"),
        "http" => Ok("http"),
        "resp" => Ok("resp"),
        "text" => Ok("text"),
        "s3" => Ok("s3"),
        "oci-distribution" | "oci_distribution" => Ok("oci_distribution"),
        "car" => Ok("car"),
        "pg-wire" | "pg_wire" => Ok("pg_wire"),
        "mysql-wire" | "mysql_wire" => Ok("mysql_wire"),
        "couchbase-kv" | "couchbase_kv" => Ok("couchbase_kv"),
        "mongodb-wire" | "mongodb_wire" => Ok("mongodb_wire"),
        "couchdb-rest" | "couchdb_rest" => Ok("couchdb_rest"),
        "couchbase-document" | "couchbase_document" => Ok("couchbase_document"),
        "kafka" => Ok("kafka"),
        "nats" => Ok("nats"),
        "amqp" => Ok("amqp"),
        "arrow-flight" | "arrow_flight" => Ok("arrow_flight"),
        "parquet" => Ok("parquet"),
        "duckdb-like" | "duckdb_like" => Ok("duckdb_like"),
        "snowflake-like" | "snowflake_like" => Ok("snowflake_like"),
        "spark-like" | "spark_like" => Ok("spark_like"),
        "bigquery-like" | "bigquery_like" => Ok("bigquery_like"),
        "ndjson" => Ok("ndjson"),
        "bolt" => Ok("bolt"),
        "gremlin" => Ok("gremlin"),
        "immudb-grpc" | "immudb_grpc" => Ok("immudb_grpc"),
        "transparency-log" | "transparency_log" => Ok("transparency_log"),
        "caldav" => Ok("caldav"),
        "carddav" => Ok("carddav"),
        "imap" => Ok("imap"),
        "jmap" => Ok("jmap"),
        "smtp" => Ok("smtp"),
        other => Err(format!("unsupported served transport {other:?}")),
    }
}

fn normalize_profile(
    surface: &str,
    transport: &str,
    profile: Option<&str>,
    mode: Option<&str>,
) -> Result<Option<&'static str>, String> {
    if surface == "memcached" {
        if profile.is_some() {
            return Err("served surface \"memcached\" uses --mode, not --profile".to_string());
        }
        let Some(mode) = mode else {
            return Ok(None);
        };
        return match mode {
            "volatile" => Ok(None),
            "versioned" => Ok(Some("versioned")),
            "read-through" => Ok(Some("read-through")),
            "write-through" => Ok(Some("write-through")),
            "write-around" => Ok(Some("write-around")),
            "write-behind" => Ok(Some("write-behind")),
            other => Err(format!(
                "unsupported memcached --mode {other:?} (expected `volatile`, `versioned`, `read-through`, `write-through`, `write-around`, or `write-behind`)"
            )),
        };
    }
    if mode.is_some() {
        return Err(format!("served surface {surface:?} does not accept --mode"));
    }
    if surface != "vector" {
        return match profile {
            Some(value) => Err(format!(
                "served surface {surface:?} does not accept --profile {value:?}"
            )),
            None => Ok(None),
        };
    }
    let Some(profile) = profile else {
        return Err(
            "served surface \"vector\" requires explicit --profile for rest, json-rpc, or grpc"
                .to_string(),
        );
    };
    match (profile, transport) {
        ("generic", "rest" | "json_rpc" | "grpc") => Ok(Some("generic")),
        ("qdrant", "rest" | "grpc") => Ok(Some("qdrant")),
        ("pinecone", "rest") => Ok(Some("pinecone")),
        ("qdrant", _) => Err(format!(
            "vector profile \"qdrant\" supports --transport rest or grpc, not {transport:?}"
        )),
        ("pinecone", _) => Err(format!(
            "vector profile \"pinecone\" supports --transport rest, not {transport:?}"
        )),
        ("generic", _) => Err(format!(
            "vector profile \"generic\" supports --transport rest, json-rpc, or grpc, not {transport:?}"
        )),
        other => Err(format!(
            "unsupported vector --profile {other:?} (expected `generic`, `qdrant`, or `pinecone`)"
        )),
    }
}

fn configure_memcached_cache_mode(
    loom: &mut Loom<FileStore>,
    surface: &str,
    profile: Option<&str>,
    selector: &[String],
) -> Result<bool, String> {
    if surface != "memcached" {
        return Ok(false);
    }
    let Some(mode) = profile else {
        return Ok(false);
    };
    let [workspace, cache] = selector else {
        return Err("memcached listener expects workspace and cache selectors".to_string());
    };
    let ns = ensure_facet_workspace(loom, workspace, FacetKind::Kv)?;
    let config = memcached_kv_config(mode)?;
    loom.configure_kv_map(ns, cache, config)
        .map_err(|e| e.to_string())?;
    Ok(true)
}

fn memcached_kv_config(mode: &str) -> Result<KvMapConfig, String> {
    Ok(match mode {
        "versioned" => KvMapConfig::VERSIONED,
        "read-through" => KvMapConfig {
            read_through: true,
            ..KvMapConfig::EPHEMERAL
        },
        "write-through" => KvMapConfig {
            write_through: true,
            ..KvMapConfig::EPHEMERAL
        },
        "write-around" => KvMapConfig {
            write_around: true,
            ..KvMapConfig::EPHEMERAL
        },
        "write-behind" => KvMapConfig {
            write_behind: true,
            ..KvMapConfig::EPHEMERAL
        },
        other => {
            return Err(format!("unsupported memcached mode {other:?}"));
        }
    })
}

fn validate_transport(surface: &str, transport: &str) -> Result<(), String> {
    let spec = served_surface_spec(surface)
        .ok_or_else(|| format!("unsupported served surface {surface:?}"))?;
    if spec.transports.contains(&transport) {
        Ok(())
    } else {
        Err(format!(
            "transport {transport:?} is not valid for served surface {surface:?}"
        ))
    }
}

fn validate_selector_shape(surface: &str, selectors: &[String]) -> Result<(), String> {
    let spec = served_surface_spec(surface)
        .ok_or_else(|| format!("unsupported served surface {surface:?}"))?;
    let count = selectors.len();
    if (spec.min_selectors..=spec.max_selectors).contains(&count) {
        Ok(())
    } else if spec.min_selectors == spec.max_selectors {
        Err(format!(
            "served surface {surface:?} expects {} selector(s), got {}",
            spec.min_selectors, count
        ))
    } else {
        Err(format!(
            "served surface {surface:?} expects {} to {} selector(s), got {}",
            spec.min_selectors, spec.max_selectors, count
        ))
    }
}

pub(crate) fn served_listener_target(record: &ServedListenerRecord) -> String {
    let profile = record.profile.as_deref().unwrap_or("");
    format!(
        "id={};surface={};transport={};profile={};bind={};enabled={}",
        record.id, record.surface, record.transport, profile, record.bind, record.enabled
    )
}

fn served_listener_json(record: &ServedListenerRecord, seq: u64) -> String {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"seq\":");
    out.push_str(&seq.to_string());
    out.push(',');
    out.push_str(&served_listener_record_json(record)[1..]);
    out
}

fn served_listener_record_json(record: &ServedListenerRecord) -> String {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"id\":");
    out.push_str(&json_string(&record.id));
    out.push_str(",\"schema_version\":");
    out.push_str(&record.schema_version.to_string());
    out.push_str(",\"surface\":");
    out.push_str(&json_string(&record.surface));
    out.push_str(",\"selectors\":[");
    for (idx, selector) in record.selectors.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&json_string(selector));
    }
    out.push_str("],\"transport\":");
    out.push_str(&json_string(&record.transport));
    out.push_str(",\"profile\":");
    push_json_option(&mut out, record.profile.as_deref());
    out.push_str(",\"bind\":");
    out.push_str(&json_string(&record.bind));
    out.push_str(",\"enabled\":");
    out.push_str(if record.enabled { "true" } else { "false" });
    out.push_str(",\"tls\":{\"mode\":");
    out.push_str(&json_string(&record.tls.mode));
    out.push_str(",\"certificate_bundle_ref\":");
    push_json_option(&mut out, record.tls.certificate_bundle_ref.as_deref());
    out.push('}');
    out.push_str(",\"auth\":{\"mode\":");
    out.push_str(&json_string(&record.auth.mode));
    out.push('}');
    out.push_str(",\"limits\":{\"request_size_limit\":");
    out.push_str(&record.limits.request_size_limit.to_string());
    out.push_str(",\"idle_timeout_ms\":");
    out.push_str(&record.limits.idle_timeout_ms.to_string());
    out.push_str(",\"session_timeout_ms\":");
    out.push_str(&record.limits.session_timeout_ms.to_string());
    out.push('}');
    out.push_str(",\"audit\":{\"mode\":");
    out.push_str(&json_string(&record.audit.mode));
    out.push('}');
    out.push_str(",\"route_scope\":");
    out.push_str(&json_string(&record.route_scope));
    out.push_str(",\"exposure\":");
    out.push_str(&json_string(&record.exposure));
    out.push_str(",\"network_access_policy_ref\":");
    push_json_option(&mut out, record.network_access_policy_ref.as_deref());
    out.push_str(",\"last_modified_audit_seq\":");
    push_json_u64_option(&mut out, record.last_modified_audit_seq);
    out.push('}');
    out
}

fn web_listener_json(listener: &loom_substrate::web::WebListener, seq: Option<u64>) -> String {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"seq\":");
    push_json_u64_option(&mut out, seq);
    out.push_str(",\"listener\":");
    out.push_str(&json_string(&listener.listener_id));
    out.push_str(",\"default_workspace\":");
    out.push_str(&json_string(&listener.default_workspace.to_string()));
    out.push_str(",\"root_path\":");
    out.push_str(&json_string(&listener.root_path));
    out.push_str(",\"routes\":[");
    for (idx, route) in listener.routes.routes.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&web_route_json(route));
    }
    out.push_str("]}");
    out
}

fn web_route_json(route: &loom_substrate::web::WebRoute) -> String {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"route_id\":");
    out.push_str(&json_string(&route.route_id));
    out.push_str(",\"methods\":[");
    for (idx, method) in route.methods.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&json_string(web_method_name(*method)));
    }
    out.push_str("],\"host_pattern\":");
    push_json_option(&mut out, route.host_pattern.as_deref());
    out.push_str(",\"path_prefix\":");
    out.push_str(&json_string(&route.path_prefix));
    out.push_str(",\"workspace\":");
    match route.workspace {
        Some(workspace) => out.push_str(&json_string(&workspace.to_string())),
        None => out.push_str("null"),
    }
    out.push_str(",\"root_path\":");
    out.push_str(&json_string(&route.root_path));
    out.push_str(",\"mode\":");
    out.push_str(&json_string(web_route_mode_name(route.mode)));
    out.push('}');
    out
}

fn web_method_name(method: loom_substrate::web::WebMethod) -> &'static str {
    match method {
        loom_substrate::web::WebMethod::Get => "GET",
        loom_substrate::web::WebMethod::Head => "HEAD",
        loom_substrate::web::WebMethod::Post => "POST",
        loom_substrate::web::WebMethod::Put => "PUT",
        loom_substrate::web::WebMethod::Patch => "PATCH",
        loom_substrate::web::WebMethod::Delete => "DELETE",
        loom_substrate::web::WebMethod::Options => "OPTIONS",
    }
}

fn web_route_mode_name(mode: loom_substrate::web::WebRouteMode) -> &'static str {
    match mode {
        loom_substrate::web::WebRouteMode::StaticFile => "static-file",
        loom_substrate::web::WebRouteMode::Presentation => "presentation",
        loom_substrate::web::WebRouteMode::Program => "program",
        loom_substrate::web::WebRouteMode::Redirect => "redirect",
        loom_substrate::web::WebRouteMode::ReverseProxy => "reverse-proxy",
        loom_substrate::web::WebRouteMode::Error => "error",
    }
}

fn push_json_u64_option(out: &mut String, value: Option<u64>) {
    match value {
        Some(value) => out.push_str(&value.to_string()),
        None => out.push_str("null"),
    }
}

fn push_json_option(out: &mut String, value: Option<&str>) {
    match value {
        Some(value) => out.push_str(&json_string(value)),
        None => out.push_str("null"),
    }
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
