//! Shared Serve configuration authority.
//!
//! Licensed under BUSL-1.1.

use loom_core::{KvMapConfig, Loom, ObjectStore, WorkspaceId};
use loom_store::{FileStore, ServedListenerRecord};
use loom_types::{Code, LoomError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServeWebRouteSetRequest {
    pub listener: String,
    pub route: String,
    pub host: Option<String>,
    pub prefix: String,
    pub workspace: Option<String>,
    pub root: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServeListenerConfigureRequest {
    pub surface: String,
    #[serde(default)]
    pub selectors: Vec<String>,
    pub bind: String,
    pub transport: Option<String>,
    pub profile: Option<String>,
    pub mode: Option<String>,
    pub enabled: bool,
    pub tls_certificate_bundle: Option<String>,
    pub auth_mode: Option<String>,
    pub exposure: Option<String>,
    pub audit_mode: Option<String>,
    pub request_size_limit: Option<u64>,
    pub idle_timeout_ms: Option<u64>,
    pub session_timeout_ms: Option<u64>,
    pub network_access_policy: Option<String>,
}

#[derive(Clone, Copy)]
pub struct ServedSurfaceSpec {
    pub surface: &'static str,
    pub aliases: &'static [&'static str],
    pub min_selectors: usize,
    pub max_selectors: usize,
    pub default_transport: Option<&'static str>,
    pub transports: &'static [&'static str],
}

pub const SERVED_SURFACES: &[ServedSurfaceSpec] = &[
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

pub fn served_surface_spec(surface: &str) -> Option<&'static ServedSurfaceSpec> {
    SERVED_SURFACES
        .iter()
        .find(|spec| spec.surface == surface || spec.aliases.contains(&surface))
}

pub fn normalize_surface(surface: &str) -> Result<&'static str, LoomError> {
    served_surface_spec(surface)
        .map(|spec| spec.surface)
        .ok_or_else(|| LoomError::invalid(format!("unsupported served surface {surface:?}")))
}

pub fn normalize_transport(
    surface: &str,
    transport: Option<&str>,
) -> Result<&'static str, LoomError> {
    let spec = served_surface_spec(surface)
        .ok_or_else(|| LoomError::invalid(format!("unsupported served surface {surface:?}")))?;
    match transport {
        Some(value) => normalize_transport_name(value),
        None => spec.default_transport.ok_or_else(|| {
            LoomError::invalid(format!(
                "served surface {surface:?} requires an explicit --transport"
            ))
        }),
    }
}

pub fn normalize_transport_name(transport: &str) -> Result<&'static str, LoomError> {
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
        other => Err(LoomError::invalid(format!(
            "unsupported served transport {other:?}"
        ))),
    }
}

pub fn validate_transport(surface: &str, transport: &str) -> Result<(), LoomError> {
    let spec = served_surface_spec(surface)
        .ok_or_else(|| LoomError::invalid(format!("unsupported served surface {surface:?}")))?;
    if spec.transports.contains(&transport) {
        Ok(())
    } else {
        Err(LoomError::invalid(format!(
            "transport {transport:?} is not valid for served surface {surface:?}"
        )))
    }
}

pub fn validate_selector_shape(surface: &str, selectors: &[String]) -> Result<(), LoomError> {
    let spec = served_surface_spec(surface)
        .ok_or_else(|| LoomError::invalid(format!("unsupported served surface {surface:?}")))?;
    let count = selectors.len();
    if (spec.min_selectors..=spec.max_selectors).contains(&count) {
        Ok(())
    } else if spec.min_selectors == spec.max_selectors {
        Err(LoomError::invalid(format!(
            "served surface {surface:?} expects {} selector(s), got {}",
            spec.min_selectors, count
        )))
    } else {
        Err(LoomError::invalid(format!(
            "served surface {surface:?} expects {} to {} selector(s), got {}",
            spec.min_selectors, spec.max_selectors, count
        )))
    }
}

pub fn normalize_profile(
    surface: &str,
    transport: &str,
    profile: Option<&str>,
    mode: Option<&str>,
) -> Result<Option<&'static str>, LoomError> {
    if surface == "memcached" {
        if profile.is_some() {
            return Err(LoomError::invalid(
                "served surface \"memcached\" uses --mode, not --profile",
            ));
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
            other => Err(LoomError::invalid(format!(
                "unsupported memcached --mode {other:?} (expected `volatile`, `versioned`, `read-through`, `write-through`, `write-around`, or `write-behind`)"
            ))),
        };
    }
    if mode.is_some() {
        return Err(LoomError::invalid(format!(
            "served surface {surface:?} does not accept --mode"
        )));
    }
    if surface != "vector" {
        return match profile {
            Some(value) => Err(LoomError::invalid(format!(
                "served surface {surface:?} does not accept --profile {value:?}"
            ))),
            None => Ok(None),
        };
    }
    let Some(profile) = profile else {
        return Err(LoomError::invalid(
            "served surface \"vector\" requires explicit --profile for rest, json-rpc, or grpc",
        ));
    };
    match (profile, transport) {
        ("generic", "rest" | "json_rpc" | "grpc") => Ok(Some("generic")),
        ("qdrant", "rest" | "grpc") => Ok(Some("qdrant")),
        ("pinecone", "rest") => Ok(Some("pinecone")),
        ("qdrant", _) => Err(LoomError::invalid(format!(
            "vector profile \"qdrant\" supports --transport rest or grpc, not {transport:?}"
        ))),
        ("pinecone", _) => Err(LoomError::invalid(format!(
            "vector profile \"pinecone\" supports --transport rest, not {transport:?}"
        ))),
        ("generic", _) => Err(LoomError::invalid(format!(
            "vector profile \"generic\" supports --transport rest, json-rpc, or grpc, not {transport:?}"
        ))),
        other => Err(LoomError::invalid(format!(
            "unsupported vector --profile {other:?} (expected `generic`, `qdrant`, or `pinecone`)"
        ))),
    }
}

pub fn validate_bind(bind: &str) -> Result<(), LoomError> {
    let addr = bind
        .parse::<std::net::SocketAddr>()
        .map_err(|err| LoomError::invalid(format!("invalid --bind address {bind:?}: {err}")))?;
    if addr.port() == 0 {
        return Err(LoomError::invalid(
            "--bind port must not be 0 for durable listener configuration",
        ));
    }
    Ok(())
}

pub fn apply_listener_policy(
    record: &mut ServedListenerRecord,
    request: ServeListenerConfigureRequest,
) {
    if let Some(value) = request.tls_certificate_bundle {
        record.tls.mode = "direct".to_string();
        record.tls.certificate_bundle_ref = Some(value);
    }
    if let Some(value) = request.auth_mode {
        record.auth.mode = value;
    }
    if let Some(value) = request.exposure {
        record.exposure = value;
    }
    if let Some(value) = request.audit_mode {
        record.audit.mode = value;
    }
    if let Some(value) = request.request_size_limit {
        record.limits.request_size_limit = value;
    }
    if let Some(value) = request.idle_timeout_ms {
        record.limits.idle_timeout_ms = value;
    }
    if let Some(value) = request.session_timeout_ms {
        record.limits.session_timeout_ms = value;
    }
    if let Some(value) = request.network_access_policy {
        record.network_access_policy_ref = Some(value);
    }
}

pub fn memcached_kv_config(mode: &str) -> Result<KvMapConfig, LoomError> {
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
            return Err(LoomError::invalid(format!(
                "unsupported memcached mode {other:?}"
            )));
        }
    })
}

pub fn configure_memcached_cache_mode<S: ObjectStore>(
    loom: &mut Loom<S>,
    workspace_id: impl FnOnce(&mut Loom<S>, &str) -> Result<WorkspaceId, LoomError>,
    surface: &str,
    profile: Option<&str>,
    selectors: &[String],
) -> Result<bool, LoomError> {
    if surface != "memcached" {
        return Ok(false);
    }
    let Some(mode) = profile else {
        return Ok(false);
    };
    let [workspace, cache] = selectors else {
        return Err(LoomError::invalid(
            "memcached listener expects workspace and cache selectors",
        ));
    };
    let ns = workspace_id(loom, workspace)?;
    loom.configure_kv_map(ns, cache, memcached_kv_config(mode)?)?;
    Ok(true)
}

pub fn validate_listener_references(
    loom: &Loom<FileStore>,
    record: &ServedListenerRecord,
) -> Result<(), LoomError> {
    if let Some(name) = record.tls.certificate_bundle_ref.as_deref()
        && loom.store().certificate_bundle(name)?.is_none()
    {
        return Err(LoomError::not_found("certificate bundle not found"));
    }
    if let Some(name) = record.network_access_policy_ref.as_deref()
        && loom.store().network_access_policy(name)?.is_none()
    {
        return Err(LoomError::not_found("network access policy not found"));
    }
    Ok(())
}

pub fn require_web_listener_record(
    loom: &Loom<FileStore>,
    listener: &str,
) -> Result<ServedListenerRecord, LoomError> {
    let record = loom
        .store()
        .served_listener(listener)?
        .ok_or_else(|| LoomError::not_found("served listener not found"))?;
    if record.surface != "web" || record.transport != "rest" {
        return Err(LoomError::invalid(
            "served listener is not a web rest listener",
        ));
    }
    if record.selectors.len() != 1 {
        return Err(LoomError::invalid(
            "served listener must have exactly one workspace selector",
        ));
    }
    Ok(record)
}

pub fn web_listener_from_record(
    loom: &Loom<FileStore>,
    record: &ServedListenerRecord,
    resolve_workspace: impl FnOnce(&Loom<FileStore>, &str) -> Result<WorkspaceId, LoomError>,
) -> Result<loom_substrate::web::WebListener, LoomError> {
    let key = loom_substrate::web::web_profile_listener_key(&record.id)?;
    if let Some(bytes) = loom.store().control_get(&key)? {
        return loom_substrate::web::WebListener::decode(&bytes);
    }
    let workspace = resolve_workspace(loom, &record.selectors[0])?;
    let addr = record.bind.parse::<std::net::SocketAddr>().map_err(|err| {
        LoomError::new(
            Code::InvalidArgument,
            format!("invalid listener bind address {:?}: {err}", record.bind),
        )
    })?;
    loom_substrate::web::WebListener::new(
        &record.id,
        addr.ip().to_string(),
        addr.port(),
        loom_substrate::web::WebProtocol::Http,
        workspace,
        "/",
    )
}

pub fn web_listener_control_key(listener_id: &str) -> Result<Vec<u8>, LoomError> {
    loom_substrate::web::web_profile_listener_key(listener_id)
}

pub fn web_listener_json(
    listener: &loom_substrate::web::WebListener,
    seq: Option<u64>,
) -> Result<String, LoomError> {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"seq\":");
    push_json_u64_option(&mut out, seq);
    out.push_str(",\"listener\":");
    out.push_str(&json_string(&listener.listener_id)?);
    out.push_str(",\"default_workspace\":");
    out.push_str(&json_string(&listener.default_workspace.to_string())?);
    out.push_str(",\"root_path\":");
    out.push_str(&json_string(&listener.root_path)?);
    out.push_str(",\"routes\":[");
    for (idx, route) in listener.routes.routes.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&web_route_json(route)?);
    }
    out.push_str("]}");
    Ok(out)
}

fn web_route_json(route: &loom_substrate::web::WebRoute) -> Result<String, LoomError> {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"route_id\":");
    out.push_str(&json_string(&route.route_id)?);
    out.push_str(",\"methods\":[");
    for (idx, method) in route.methods.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&json_string(web_method_name(*method))?);
    }
    out.push_str("],\"host_pattern\":");
    push_json_string_option(&mut out, route.host_pattern.as_deref())?;
    out.push_str(",\"path_prefix\":");
    out.push_str(&json_string(&route.path_prefix)?);
    out.push_str(",\"workspace\":");
    match route.workspace {
        Some(workspace) => out.push_str(&json_string(&workspace.to_string())?),
        None => out.push_str("null"),
    }
    out.push_str(",\"root_path\":");
    out.push_str(&json_string(&route.root_path)?);
    out.push_str(",\"mode\":");
    out.push_str(&json_string(web_route_mode_name(route.mode))?);
    out.push('}');
    Ok(out)
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

pub fn listener_target(record: &ServedListenerRecord) -> String {
    let profile = record.profile.as_deref().unwrap_or("");
    format!(
        "id={};surface={};transport={};profile={};bind={};enabled={}",
        record.id, record.surface, record.transport, profile, record.bind, record.enabled
    )
}

pub fn listener_json(record: &ServedListenerRecord, seq: u64) -> Result<String, LoomError> {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"seq\":");
    out.push_str(&seq.to_string());
    out.push(',');
    out.push_str(&listener_record_json(record)?[1..]);
    Ok(out)
}

pub fn listener_record_json(record: &ServedListenerRecord) -> Result<String, LoomError> {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"id\":");
    out.push_str(&json_string(&record.id)?);
    out.push_str(",\"schema_version\":");
    out.push_str(&record.schema_version.to_string());
    out.push_str(",\"surface\":");
    out.push_str(&json_string(&record.surface)?);
    out.push_str(",\"selectors\":[");
    for (idx, selector) in record.selectors.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&json_string(selector)?);
    }
    out.push_str("],\"transport\":");
    out.push_str(&json_string(&record.transport)?);
    out.push_str(",\"profile\":");
    push_json_string_option(&mut out, record.profile.as_deref())?;
    out.push_str(",\"bind\":");
    out.push_str(&json_string(&record.bind)?);
    out.push_str(",\"enabled\":");
    out.push_str(if record.enabled { "true" } else { "false" });
    out.push_str(",\"tls\":{\"mode\":");
    out.push_str(&json_string(&record.tls.mode)?);
    out.push_str(",\"certificate_bundle_ref\":");
    push_json_string_option(&mut out, record.tls.certificate_bundle_ref.as_deref())?;
    out.push('}');
    out.push_str(",\"auth\":{\"mode\":");
    out.push_str(&json_string(&record.auth.mode)?);
    out.push('}');
    out.push_str(",\"limits\":{\"request_size_limit\":");
    out.push_str(&record.limits.request_size_limit.to_string());
    out.push_str(",\"idle_timeout_ms\":");
    out.push_str(&record.limits.idle_timeout_ms.to_string());
    out.push_str(",\"session_timeout_ms\":");
    out.push_str(&record.limits.session_timeout_ms.to_string());
    out.push('}');
    out.push_str(",\"audit\":{\"mode\":");
    out.push_str(&json_string(&record.audit.mode)?);
    out.push('}');
    out.push_str(",\"route_scope\":");
    out.push_str(&json_string(&record.route_scope)?);
    out.push_str(",\"exposure\":");
    out.push_str(&json_string(&record.exposure)?);
    out.push_str(",\"network_access_policy_ref\":");
    push_json_string_option(&mut out, record.network_access_policy_ref.as_deref())?;
    out.push_str(",\"last_modified_audit_seq\":");
    push_json_u64_option(&mut out, record.last_modified_audit_seq);
    out.push('}');
    Ok(out)
}

fn json_string<T: Serialize + ?Sized>(value: &T) -> Result<String, LoomError> {
    serde_json::to_string(value)
        .map_err(|err| LoomError::new(Code::InvalidArgument, err.to_string()))
}

fn push_json_string_option(out: &mut String, value: Option<&str>) -> Result<(), LoomError> {
    match value {
        Some(value) => out.push_str(&json_string(&value)?),
        None => out.push_str("null"),
    }
    Ok(())
}

fn push_json_u64_option(out: &mut String, value: Option<u64>) {
    match value {
        Some(value) => out.push_str(&value.to_string()),
        None => out.push_str("null"),
    }
}

pub fn cli_error(error: LoomError) -> String {
    error.message
}
