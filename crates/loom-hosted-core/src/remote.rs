//! Remote Loom server runtime.
//!
//! A `RemoteRuntime` owns exactly one Loom (through a [`LocalLoomClient`] engine binding) and one writer
//! authority, plus the runtime state: connection, session, handle, task, stream, and watch
//! registries with leases, cancellation, and disconnect cleanup. It authenticates protocol sessions
//!, dispatches unary calls into the local engine while serializing mutations through the
//! single writer authority, drives credit-based streams, and answers
//! credential-free discovery routes. This runtime is transport-agnostic: the HTTP/2 over TLS
//! carrier wraps it in `loom-hosted`, but every rule here is exercised without a socket.
//!
//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use std::collections::HashMap;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::generated_dispatch::{self, Dispatched};
use loom_client::LocalLoomClient;
use loom_client::types::{HandleId, LoomSession, RowIter, SqlBatch, SqlSession, Task};
use loom_codec::Value;
use loom_core::WorkspaceId;
use loom_remote_protocol::RemoteError;
use loom_remote_protocol::codec::{FromValue, ToValue};
use loom_remote_protocol::discovery::{Discovery, DiscoveryRoutes};
use loom_remote_protocol::envelope::{Request, Response, ResponsePayload};
use loom_remote_protocol::frame::Frame;
use loom_remote_protocol::generated::METHODS;
use loom_remote_protocol::generated::MethodSig;
use loom_store::{FileStore, ServedListenerRecord};
use loom_types::{Code, LoomError};

/// The `remote-loom` protocol surface name for served-listener records.
pub const REMOTE_SURFACE: &str = "remote-loom";
/// The `cbor-h2` transport name for remote served-listener records.
pub const REMOTE_TRANSPORT: &str = "cbor-h2";

/// Current wall-clock milliseconds since the Unix epoch.
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ---- configuration -------------------------------------------------------------------------------

/// A remote protocol authentication mode advertised by the endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteAuthMode {
    /// The client prompts or delegates to a local credential provider.
    Interactive,
    /// The client retrieves a named token.
    Token,
    /// The client presents a configured certificate identity.
    Mtls,
    /// The client presents an already established local principal assertion.
    Principal,
    /// The client delegates proof to an external verifier.
    External,
}

impl RemoteAuthMode {
    /// The wire spelling advertised in discovery.
    pub const fn as_str(self) -> &'static str {
        match self {
            RemoteAuthMode::Interactive => "interactive",
            RemoteAuthMode::Token => "token",
            RemoteAuthMode::Mtls => "mtls",
            RemoteAuthMode::Principal => "principal",
            RemoteAuthMode::External => "external",
        }
    }
}

/// A TLS trust selector advertised by the endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteTlsTrust {
    /// Platform trust.
    System,
    /// A named certificate bundle or host-provided trust anchor.
    Bundle(String),
    /// Certificate verification disabled, permitted only for loopback development.
    InsecureDev,
}

impl RemoteTlsTrust {
    /// The wire spelling advertised in discovery.
    pub fn as_str(&self) -> String {
        match self {
            RemoteTlsTrust::System => "system".to_string(),
            RemoteTlsTrust::Bundle(name) => format!("bundle:{name}"),
            RemoteTlsTrust::InsecureDev => "insecure-dev".to_string(),
        }
    }
}

/// Whether `host` is a loopback address for which plain HTTP is permitted.
fn is_loopback(host: &str) -> bool {
    let host = host.split(':').next().unwrap_or(host);
    matches!(host, "localhost" | "127.0.0.1" | "::1" | "[::1]")
}

/// Validate an endpoint scheme and trust against the endpoint policy: plain HTTP is rejected unless the host is
/// loopback or an explicit insecure-dev trust is configured.
///
/// # Errors
/// Returns [`LoomError`] (`INVALID_ARGUMENT`) for a rejected scheme or an unsafe plain-HTTP endpoint.
pub fn validate_endpoint(
    scheme: &str,
    host: &str,
    trust: &RemoteTlsTrust,
) -> Result<(), LoomError> {
    match scheme {
        "https" => Ok(()),
        "http" => {
            if is_loopback(host) || matches!(trust, RemoteTlsTrust::InsecureDev) {
                Ok(())
            } else {
                Err(LoomError::new(
                    Code::InvalidArgument,
                    "plain http is allowed only for loopback or an explicit insecure-dev trust",
                ))
            }
        }
        other => Err(LoomError::new(
            Code::InvalidArgument,
            format!("unsupported endpoint scheme `{other}` (expected https or http)"),
        )),
    }
}

/// Static configuration for one served endpoint.
#[derive(Debug, Clone)]
pub struct RemoteServerConfig {
    /// The service-root URL clients treat as the endpoint base.
    pub service_root: String,
    /// The concrete `cbor-h2` call endpoint URL.
    pub call_endpoint: String,
    /// Accepted auth modes.
    pub auth_modes: Vec<RemoteAuthMode>,
    /// Advertised TLS trust selectors.
    pub tls: Vec<RemoteTlsTrust>,
    /// Discovery route resolution for this deployment.
    pub discovery: DiscoveryRoutes,
    /// Session lease in milliseconds.
    pub session_lease_ms: u64,
}

impl RemoteServerConfig {
    /// Validate the configuration.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`INVALID_ARGUMENT`) for an empty auth list or a zero session lease.
    pub fn validate(&self) -> Result<(), LoomError> {
        if self.auth_modes.is_empty() {
            return Err(LoomError::new(
                Code::InvalidArgument,
                "at least one auth mode must be configured",
            ));
        }
        if self.session_lease_ms == 0 {
            return Err(LoomError::new(
                Code::InvalidArgument,
                "session lease must be greater than zero",
            ));
        }
        Ok(())
    }

    /// The public discovery document this endpoint publishes.
    pub fn discovery_document(&self) -> Discovery {
        Discovery::v1(
            self.service_root.clone(),
            self.call_endpoint.clone(),
            self.auth_modes
                .iter()
                .map(|m| m.as_str().to_string())
                .collect(),
            self.tls.iter().map(RemoteTlsTrust::as_str).collect(),
        )
    }
}

// ---- serve options and durable listener records --------------------------------------------------

/// Foreground options for serving one remote endpoint (`loom serve remote`): the bind address,
/// how the endpoint advertises itself, and the auth, TLS, discovery, and limit policy. Building the
/// server config from these validates the bind and the advertised endpoint.
#[derive(Debug, Clone)]
pub struct RemoteServeOptions {
    /// The `host:port` address to bind the listener to.
    pub bind: String,
    /// The advertised service-root URL.
    pub service_root: String,
    /// The concrete `cbor-h2` call endpoint URL.
    pub call_endpoint: String,
    /// Accepted auth modes.
    pub auth_modes: Vec<RemoteAuthMode>,
    /// Advertised TLS trust selectors.
    pub tls: Vec<RemoteTlsTrust>,
    /// Discovery route resolution.
    pub discovery: DiscoveryRoutes,
    /// Session lease in milliseconds.
    pub session_lease_ms: u64,
    /// Optional network access policy name (0066) gating the listener.
    pub network_access_policy: Option<String>,
    /// Maximum accepted request body size in bytes.
    pub max_request_bytes: u64,
}

impl RemoteServeOptions {
    /// Validate the bind address, the advertised endpoint scheme/trust, the request limit,
    /// and the derived server config.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`INVALID_ARGUMENT`) for a malformed bind, an unsafe plain-HTTP endpoint, a
    /// zero limit, or invalid server config.
    pub fn validate(&self) -> Result<(), LoomError> {
        parse_bind(&self.bind)?;
        if self.max_request_bytes == 0 {
            return Err(LoomError::new(
                Code::InvalidArgument,
                "max request bytes must be greater than zero",
            ));
        }
        let (scheme, host) = endpoint_scheme_and_host(&self.call_endpoint)?;
        let trust = self.tls.first().cloned().unwrap_or(RemoteTlsTrust::System);
        validate_endpoint(&scheme, &host, &trust)?;
        self.to_config().validate()
    }

    /// Build serve options from scalar inputs (the `loom serve remote` CLI shape): the call endpoint
    /// defaults to `<service-root>/v1/call`, and the discovery routes are derived from the service-root
    /// URL path. Keeps the discovery-route wire types internal to the hosted layer.
    #[allow(clippy::too_many_arguments)]
    pub fn from_cli(
        bind: String,
        service_root: String,
        call_endpoint: Option<String>,
        auth_modes: Vec<RemoteAuthMode>,
        tls: Vec<RemoteTlsTrust>,
        session_lease_ms: u64,
        max_request_bytes: u64,
        network_access_policy: Option<String>,
    ) -> Self {
        let call_endpoint = call_endpoint
            .unwrap_or_else(|| format!("{}/v1/call", service_root.trim_end_matches('/')));
        let service_root_path = url_path(&service_root);
        Self {
            bind,
            service_root,
            call_endpoint,
            auth_modes,
            tls,
            discovery: DiscoveryRoutes {
                mode: loom_remote_protocol::discovery::DiscoveryMode::Default,
                service_root_path,
                custom_path: None,
            },
            session_lease_ms,
            network_access_policy,
            max_request_bytes,
        }
    }

    /// The path portion of the configured call endpoint URL, used to route the carrier's call handler.
    pub fn call_path(&self) -> String {
        url_path(&self.call_endpoint)
    }

    /// The server configuration these options describe.
    pub fn to_config(&self) -> RemoteServerConfig {
        RemoteServerConfig {
            service_root: self.service_root.clone(),
            call_endpoint: self.call_endpoint.clone(),
            auth_modes: self.auth_modes.clone(),
            tls: self.tls.clone(),
            discovery: self.discovery.clone(),
            session_lease_ms: self.session_lease_ms,
        }
    }

    /// The durable served-listener record that persists this endpoint's intent for daemon-managed
    /// startup: the `remote-loom` surface over the `cbor-h2` transport at `bind`.
    ///
    /// # Errors
    /// Returns [`LoomError`] when a field fails served-listener validation.
    pub fn listener_record(&self) -> Result<ServedListenerRecord, LoomError> {
        let mut record = FileStore::served_listener_record(
            REMOTE_SURFACE,
            vec![self.service_root.clone()],
            REMOTE_TRANSPORT,
            &self.bind,
            true,
        )?;
        record.network_access_policy_ref = self.network_access_policy.clone();
        Ok(record)
    }
}

/// Parse a `host:port` bind address into its host and port.
///
/// # Errors
/// Returns [`LoomError`] (`INVALID_ARGUMENT`) for a missing port, an empty host, or a port outside
/// `1..=65535`.
pub fn parse_bind(bind: &str) -> Result<(String, u16), LoomError> {
    let (host, port) = bind
        .rsplit_once(':')
        .ok_or_else(|| LoomError::new(Code::InvalidArgument, "bind must be host:port"))?;
    if host.is_empty() {
        return Err(LoomError::new(
            Code::InvalidArgument,
            "bind host must not be empty",
        ));
    }
    let port: u16 = port
        .parse()
        .map_err(|_| LoomError::new(Code::InvalidArgument, "bind port must be 1..=65535"))?;
    if port == 0 {
        return Err(LoomError::new(
            Code::InvalidArgument,
            "bind port must be 1..=65535",
        ));
    }
    Ok((host.to_string(), port))
}

/// The path portion of a `scheme://host[:port]/path` URL (or the input itself, made root-absolute, when
/// it has no scheme). A URL with no path maps to `/`.
fn url_path(url: &str) -> String {
    match url.split_once("://") {
        Some((_, rest)) => match rest.find('/') {
            Some(idx) => rest[idx..].to_string(),
            None => "/".to_string(),
        },
        None if url.starts_with('/') => url.to_string(),
        None => format!("/{url}"),
    }
}

fn endpoint_scheme_and_host(url: &str) -> Result<(String, String), LoomError> {
    let (scheme, rest) = url
        .split_once("://")
        .ok_or_else(|| LoomError::new(Code::InvalidArgument, "endpoint must be a URL"))?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    let host = authority.split('@').next_back().unwrap_or(authority);
    Ok((scheme.to_string(), host.to_string()))
}

// ---- authentication request ----------------------------------------------------------------------

/// How a client authenticates a new protocol session.
#[derive(Debug, Clone)]
pub enum RemoteAuth {
    /// No authentication; the session operates in the store's current (unauthenticated-root) mode.
    Unauthenticated,
    /// Authenticate a principal by passphrase.
    Passphrase {
        /// The principal id to authenticate.
        principal: WorkspaceId,
        /// The passphrase bytes.
        passphrase: Vec<u8>,
    },
}

// ---- runtime state -------------------------------------------------------------------------------

struct SessionEntry {
    /// The credential replayed on each short-lived engine open, so a protocol session is a lifetime/auth
    /// record rather than a held OS writer lock on the bound `.loom`.
    auth: RemoteAuth,
    principal: Option<String>,
    connection: u64,
    lease_expires_ms: u64,
    /// A lazily-opened engine session, held only while the session has a stateful resource that needs an
    /// in-memory engine session to survive across calls (an open `FileHandle` or a `LoomSession`-capturing
    /// async `Task`). Ref-counted by `writer_refs`; released when the last such resource is freed, so the
    /// store write lock is not held between calls and the SQL path family can reopen by path.
    writer: Option<LoomSession>,
    writer_refs: u64,
}

struct ConnectionEntry {
    peer: String,
    opened_ms: u64,
}

/// The kind of a remote handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HandleKind {
    /// A SQL session handle.
    SqlSession,
    /// A SQL transaction batch handle.
    SqlBatch,
    /// A forward-only row iterator handle.
    RowIter,
    /// An asynchronous task handle.
    Task,
    /// An open file handle.
    File,
}

/// The local `LocalLoomClient` handle behind a registered remote handle, retained so session and
/// runtime teardown can close/free the underlying engine handle without leaking it.
enum HandleBacking {
    Sql(SqlSession),
    Batch(SqlBatch),
    Row(RowIter),
    Task(Task),
    File { engine: LoomSession, file: u64 },
}

impl HandleBacking {
    fn kind(&self) -> HandleKind {
        match self {
            HandleBacking::Sql(_) => HandleKind::SqlSession,
            HandleBacking::Batch(_) => HandleKind::SqlBatch,
            HandleBacking::Row(_) => HandleKind::RowIter,
            HandleBacking::Task(_) => HandleKind::Task,
            HandleBacking::File { .. } => HandleKind::File,
        }
    }

    fn local_id(&self) -> u64 {
        match self {
            HandleBacking::Sql(h) => handle_local_id(&h.0),
            HandleBacking::Batch(h) => handle_local_id(&h.0),
            HandleBacking::Row(h) => handle_local_id(&h.0),
            HandleBacking::Task(h) => handle_local_id(&h.0),
            HandleBacking::File { file, .. } => *file,
        }
    }
}

struct HandleEntry {
    generation: u64,
    owner_session: u64,
    connection: u64,
    last_use_ms: u64,
    backing: HandleBacking,
    /// Whether this handle holds a ref on its owner session's engine writer (an open `FileHandle` or a
    /// `LoomSession`-capturing async `Task`); freeing it releases that ref.
    pins_writer: bool,
}

/// The lifecycle state of a runtime task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    /// The task is running.
    Running,
    /// The task finished and its result is retained until taken.
    Done,
    /// The task was cancelled.
    Cancelled,
}

struct TaskEntry {
    owner_session: u64,
    state: TaskState,
    result: Option<Vec<u8>>,
}

struct StreamEntry {
    session: u64,
    connection: u64,
    credit: u32,
    delivered: u64,
    items: VecDeque<Vec<u8>>,
    closed: bool,
}

struct WatchEntry {
    owner_session: u64,
    cursor: String,
}

/// The remote server runtime: one Loom, one writer authority, and the registries.
/// Context for a server-side MCP tool execution: the resolved session principal, the caller's
/// idempotency key (if any), and the request deadline. Threaded to the executor so a server-executed
/// tool runs under the same authority and request-identity the unary dispatch path uses.
#[derive(Debug, Clone, Default)]
pub struct McpToolContext {
    /// The authenticated session principal (a `WorkspaceId` string), if the session carries one.
    pub session_principal: Option<String>,
    /// The caller's idempotency key for a mutating tool, or `None`.
    pub idempotency_key: Option<Vec<u8>>,
    /// The request deadline in wall-clock milliseconds since the Unix epoch, or `None`.
    pub deadline_ms: Option<u64>,
}

/// The seam that runs an MCP tool server-side, beside the served `Loom<FileStore>`, under server
/// authority. Implemented by the host that owns the MCP tool catalog and shared domain code (the CLI's
/// `loom serve remote`), and injected into the runtime with [`RemoteRuntime::set_mcp_executor`]. The
/// runtime stays decoupled: it ferries the tool name plus opaque argument/result bytes (the MCP host
/// owns their encoding) and never reconstructs tool semantics itself. `args` is the opaque tool
/// arguments; the returned bytes are the tool's result value (the same value the local MCP path
/// returns), or a [`LoomError`]. A tool that is not server-promoted returns [`Code::Unsupported`].
pub trait McpToolExecutor: Send + Sync {
    /// Execute promoted MCP tool `name` with `args` against the served store; return the result bytes.
    ///
    /// # Errors
    /// Returns [`LoomError`] for a rejected/unpromoted tool, an authorization failure, or a domain error.
    fn call_tool(
        &self,
        ctx: &McpToolContext,
        name: &str,
        args: &[u8],
    ) -> Result<Vec<u8>, LoomError>;
}

pub struct RemoteRuntime {
    client: LocalLoomClient,
    config: RemoteServerConfig,
    /// Optional server-side MCP tool executor. When unset, `Mcp.call_tool` requests are rejected with
    /// `Code::Unsupported`.
    mcp_executor: Option<Arc<dyn McpToolExecutor>>,
    sessions: Mutex<HashMap<u64, SessionEntry>>,
    connections: Mutex<HashMap<u64, ConnectionEntry>>,
    handles: Mutex<HashMap<(HandleKind, u64), HandleEntry>>,
    tasks: Mutex<HashMap<u64, TaskEntry>>,
    streams: Mutex<HashMap<u64, StreamEntry>>,
    watches: Mutex<HashMap<u64, WatchEntry>>,
    next_id: AtomicU64,
    write_lock: Mutex<()>,
    draining: AtomicBool,
    /// Server-side idempotency dedup (specs/0067 §6): a caller that supplies an `idempotency_key` for a
    /// mutating, non-naturally-idempotent method gets exactly-once semantics — a replay with the same
    /// request fingerprint returns the stored terminal result without re-applying the effect, and a replay
    /// of the same key with a different fingerprint is rejected. Scoped by `(session, interface, method,
    /// key)` (the session encodes the endpoint + principal). Bounded per session and dropped on session
    /// close/expiry.
    idempotency: Mutex<HashMap<IdemKey, IdemEntry>>,
}

/// Idempotency dedup key: `(session, interface, method, idempotency_key)`.
type IdemKey = (u64, String, String, Vec<u8>);

/// A remembered terminal outcome for one idempotency key.
struct IdemEntry {
    /// Canonical fingerprint of the originating request `(interface, method, args)`.
    fingerprint: Vec<u8>,
    /// Insertion order, used to evict the oldest entry for a session when the per-session cap is exceeded.
    seq: u64,
    /// The terminal response body to replay on an exact-fingerprint retry.
    payload: ResponsePayload,
}

/// Per-session cap on remembered idempotency entries; the oldest is evicted past this bound. A hard ceiling
/// so a long-lived session cannot grow the dedup table without limit (entries are also dropped wholesale on
/// session close/expiry).
const IDEMPOTENCY_PER_SESSION_CAP: usize = 1024;

/// A protocol session returned to a caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteSession {
    /// The opaque session id bytes.
    pub id: Vec<u8>,
    /// The wall-clock millisecond deadline when the lease expires without renewal.
    pub lease_expires_ms: u64,
}

/// A server-side incremental source of encoded stream frames, pulled one at a time by the carrier so a
/// long or unbounded stream is never materialized whole and the client's flow-control window bounds how
/// far the server runs ahead. Dropping it (e.g. when the client resets the stream) stops production.
pub struct ServerFrameStream {
    inner: ServerFrameKind,
}

enum ServerFrameKind {
    /// A fixed set of already-encoded frames (a generated streaming method's buffered output), still
    /// pushed to the wire one frame at a time.
    Buffered(VecDeque<Vec<u8>>),
    /// A lazy producer of encoded frames; `None` ends the stream. Used for unbounded server-driven tails.
    Lazy(Box<dyn FnMut() -> Option<Vec<u8>> + Send>),
}

impl ServerFrameStream {
    /// A stream over a fixed set of already-encoded frames.
    pub fn buffered(frames: Vec<Vec<u8>>) -> Self {
        Self {
            inner: ServerFrameKind::Buffered(frames.into()),
        }
    }

    /// A stream over a lazy frame producer (`None` ends it).
    pub fn lazy(producer: impl FnMut() -> Option<Vec<u8>> + Send + 'static) -> Self {
        Self {
            inner: ServerFrameKind::Lazy(Box::new(producer)),
        }
    }

    /// The next encoded frame, or `None` at end of stream. Called by the carrier only when it can send.
    pub fn next_frame(&mut self) -> Option<Vec<u8>> {
        match &mut self.inner {
            ServerFrameKind::Buffered(frames) => frames.pop_front(),
            ServerFrameKind::Lazy(producer) => producer(),
        }
    }
}

impl RemoteRuntime {
    /// Start a runtime bound to the `.loom` at `path`. The store must already exist; a probe session
    /// validates that it opens before the runtime accepts connections.
    ///
    /// # Errors
    /// Returns [`LoomError`] for invalid configuration or a store that cannot be opened.
    pub fn start(path: impl Into<PathBuf>, config: RemoteServerConfig) -> Result<Self, LoomError> {
        config.validate()?;
        let client = LocalLoomClient::new(path);
        let probe = client.open()?;
        client.close(&probe);
        Ok(Self {
            client,
            config,
            mcp_executor: None,
            sessions: Mutex::new(HashMap::new()),
            connections: Mutex::new(HashMap::new()),
            handles: Mutex::new(HashMap::new()),
            tasks: Mutex::new(HashMap::new()),
            streams: Mutex::new(HashMap::new()),
            watches: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            write_lock: Mutex::new(()),
            draining: AtomicBool::new(false),
            idempotency: Mutex::new(HashMap::new()),
        })
    }

    /// Install the server-side MCP tool executor. Called once during endpoint assembly, before the
    /// runtime is shared, by the host that owns the MCP catalog and shared domain code.
    pub fn set_mcp_executor(&mut self, executor: Arc<dyn McpToolExecutor>) {
        self.mcp_executor = Some(executor);
    }

    fn mint_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    /// The canonical fingerprint of a request `(interface, method, args)` for idempotency dedup, or `None`
    /// if the args cannot be re-encoded (which never happens for an already-decoded request, in which case
    /// dedup is safely skipped rather than risking a false fingerprint match).
    fn idem_fingerprint(interface: &str, method: &str, args: &[Value]) -> Option<Vec<u8>> {
        loom_codec::encode(&Value::Array(vec![
            Value::Text(interface.to_string()),
            Value::Text(method.to_string()),
            Value::Array(args.to_vec()),
        ]))
        .ok()
    }

    /// Idempotency precheck (specs/0067 §6): `Some(payload)` means return that body now — either the stored
    /// terminal result on an exact-fingerprint replay, or a `Code::Aborted` conflict when the same key is
    /// reused with a different request fingerprint. `None` means no prior record: proceed to execute.
    fn idempotency_precheck(
        &self,
        session_key: u64,
        interface: &str,
        method: &str,
        key: &[u8],
        fingerprint: &[u8],
    ) -> Option<ResponsePayload> {
        let map = self.idempotency.lock().expect("idempotency registry");
        match map.get(&(
            session_key,
            interface.to_string(),
            method.to_string(),
            key.to_vec(),
        )) {
            Some(entry) if entry.fingerprint == fingerprint => Some(entry.payload.clone()),
            Some(_) => Some(ResponsePayload::Err(RemoteError::from_loom_error(
                &LoomError::new(
                    Code::Conflict,
                    "idempotency key reused with a different request fingerprint",
                ),
            ))),
            None => None,
        }
    }

    /// Remember a terminal outcome for a supplied idempotency key so an exact-fingerprint retry replays it
    /// without re-applying the effect. Bounded per session (oldest-by-`seq` eviction past the cap).
    fn idempotency_remember(
        &self,
        session_key: u64,
        interface: &str,
        method: &str,
        key: &[u8],
        fingerprint: Vec<u8>,
        payload: &ResponsePayload,
    ) {
        let seq = self.mint_id();
        let mut map = self.idempotency.lock().expect("idempotency registry");
        map.insert(
            (
                session_key,
                interface.to_string(),
                method.to_string(),
                key.to_vec(),
            ),
            IdemEntry {
                fingerprint,
                seq,
                payload: payload.clone(),
            },
        );
        if map.keys().filter(|k| k.0 == session_key).count() > IDEMPOTENCY_PER_SESSION_CAP
            && let Some(oldest) = map
                .iter()
                .filter(|(k, _)| k.0 == session_key)
                .min_by_key(|(_, e)| e.seq)
                .map(|(k, _)| k.clone())
        {
            map.remove(&oldest);
        }
    }

    /// Drop every remembered idempotency entry for a session (called on session close/expiry).
    fn forget_idempotency(&self, session_key: u64) {
        self.idempotency
            .lock()
            .expect("idempotency registry")
            .retain(|k, _| k.0 != session_key);
    }

    /// The configuration this runtime serves.
    pub fn config(&self) -> &RemoteServerConfig {
        &self.config
    }

    /// The number of live protocol sessions.
    pub fn session_count(&self) -> usize {
        self.sessions.lock().expect("session registry").len()
    }

    /// Whether the runtime is draining and rejecting new sessions.
    pub fn is_draining(&self) -> bool {
        self.draining.load(Ordering::Relaxed)
    }

    /// Begin draining: reject new sessions and streams while letting existing sessions run down.
    pub fn drain(&self) {
        self.draining.store(true, Ordering::Relaxed);
    }

    /// Shut down: free every registered handle, close every session, and clear the registries.
    pub fn shutdown(&self) {
        self.draining.store(true, Ordering::Relaxed);
        // Free the backing engine handles first (file handles need their engine session still open).
        let backings: Vec<HandleEntry> = self
            .handles
            .lock()
            .expect("handle registry")
            .drain()
            .map(|(_, entry)| entry)
            .collect();
        for entry in &backings {
            self.close_backing(&entry.backing);
        }
        let mut sessions = self.sessions.lock().expect("session registry");
        for (_, entry) in sessions.drain() {
            if let Some(writer) = entry.writer {
                self.client.close(&writer);
            }
        }
        self.connections
            .lock()
            .expect("connection registry")
            .clear();
        self.tasks.lock().expect("task registry").clear();
        self.streams.lock().expect("stream registry").clear();
        self.watches.lock().expect("watch registry").clear();
    }

    // ---- connections -----------------------------------------------------------------------------

    /// Register a client connection, returning its id.
    pub fn register_connection(&self, peer: impl Into<String>) -> u64 {
        let id = self.mint_id();
        self.connections
            .lock()
            .expect("connection registry")
            .insert(
                id,
                ConnectionEntry {
                    peer: peer.into(),
                    opened_ms: now_ms(),
                },
            );
        id
    }

    /// Drop a connection: close its streams immediately. Session-bound handles survive
    /// until their lease expires. Returns the number of streams closed.
    pub fn drop_connection(&self, connection: u64) -> usize {
        self.connections
            .lock()
            .expect("connection registry")
            .remove(&connection);
        let mut streams = self.streams.lock().expect("stream registry");
        let ids: Vec<u64> = streams
            .iter()
            .filter(|(_, s)| s.connection == connection)
            .map(|(id, _)| *id)
            .collect();
        for id in &ids {
            streams.remove(id);
        }
        ids.len()
    }

    // ---- sessions and auth -----------------------------------------------------------------------

    /// Open and authenticate a protocol session on `connection`, minting a lease.
    ///
    /// # Errors
    /// Returns [`LoomError`] when the runtime is draining, the store cannot be opened, or authentication
    /// fails.
    pub fn open_session(
        &self,
        connection: u64,
        auth: RemoteAuth,
    ) -> Result<RemoteSession, LoomError> {
        if self.is_draining() {
            return Err(LoomError::new(
                Code::Unsupported,
                "runtime is draining and not accepting new sessions",
            ));
        }
        // A protocol session is a lifetime/auth record, not a held writer: the store binding is validated
        // once at `start`. Verify a passphrase credential now against a short-lived engine open (released
        // immediately), so a bad credential fails fast without holding the write lock; an unauthenticated
        // session opens no engine at all, leaving the store lock free for concurrent sessions and the SQL
        // path family.
        let principal = match &auth {
            RemoteAuth::Unauthenticated => None,
            RemoteAuth::Passphrase {
                principal,
                passphrase,
            } => {
                let probe = self.client.open()?;
                if let Err(err) = self
                    .client
                    .authenticate_passphrase(&probe, *principal, passphrase)
                {
                    self.client.close(&probe);
                    return Err(err);
                }
                self.client.close(&probe);
                Some(principal.to_string())
            }
        };
        let id = self.mint_id();
        let lease_expires_ms = now_ms().saturating_add(self.config.session_lease_ms);
        self.sessions.lock().expect("session registry").insert(
            id,
            SessionEntry {
                auth,
                principal,
                connection,
                lease_expires_ms,
                writer: None,
                writer_refs: 0,
            },
        );
        Ok(RemoteSession {
            id: id.to_be_bytes().to_vec(),
            lease_expires_ms,
        })
    }

    /// Renew a session lease, returning the new expiry. An expired or unknown session is not renewable.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND`) for an unknown session or (`LOCK_LEASE_EXPIRED`) for an
    /// already expired lease.
    pub fn renew_session(&self, session_id: &[u8], at_ms: u64) -> Result<u64, LoomError> {
        let key = session_key(session_id)?;
        let mut sessions = self.sessions.lock().expect("session registry");
        let entry = sessions
            .get_mut(&key)
            .ok_or_else(|| LoomError::new(Code::NotFound, "unknown session"))?;
        if entry.lease_expires_ms <= at_ms {
            return Err(LoomError::new(
                Code::LockLeaseExpired,
                "session lease already expired",
            ));
        }
        entry.lease_expires_ms = at_ms.saturating_add(self.config.session_lease_ms);
        Ok(entry.lease_expires_ms)
    }

    /// Sweep expired sessions at `at_ms`, freeing their handles and closing their engine sessions.
    /// Returns the number expired.
    pub fn expire_sessions(&self, at_ms: u64) -> usize {
        let expired: Vec<(u64, Option<LoomSession>)> = {
            let mut sessions = self.sessions.lock().expect("session registry");
            let keys: Vec<u64> = sessions
                .iter()
                .filter(|(_, s)| s.lease_expires_ms <= at_ms)
                .map(|(id, _)| *id)
                .collect();
            keys.into_iter()
                .filter_map(|id| sessions.remove(&id).map(|entry| (id, entry.writer)))
                .collect()
        };
        for (key, writer) in &expired {
            // Free registered handles while the engine writer is still open (file handles need it), then
            // release the writer if one was held.
            self.close_session_handles(*key);
            self.close_session_runtime_resources(*key);
            self.forget_idempotency(*key);
            if let Some(writer) = writer {
                self.client.close(writer);
            }
        }
        expired.len()
    }

    /// Close a session explicitly, freeing its registered handles first, returning whether one was open.
    pub fn close_session(&self, session_id: &[u8]) -> bool {
        let Ok(key) = session_key(session_id) else {
            return false;
        };
        let entry = {
            let mut sessions = self.sessions.lock().expect("session registry");
            sessions.remove(&key)
        };
        match entry {
            Some(entry) => {
                // Free registered handles while the writer is still open (file handles need it), then
                // release the writer if one was held.
                self.close_session_handles(key);
                self.close_session_runtime_resources(key);
                self.forget_idempotency(key);
                if let Some(writer) = entry.writer {
                    self.client.close(&writer);
                }
                true
            }
            None => false,
        }
    }

    /// The authenticated principal bound to a session, if any.
    pub fn session_principal(&self, session_id: &[u8]) -> Option<String> {
        let key = session_key(session_id).ok()?;
        self.sessions
            .lock()
            .expect("session registry")
            .get(&key)
            .and_then(|entry| entry.principal.clone())
    }

    /// The connection a session was opened on, if it is live.
    pub fn session_connection(&self, session_id: &[u8]) -> Option<u64> {
        let key = session_key(session_id).ok()?;
        self.sessions
            .lock()
            .expect("session registry")
            .get(&key)
            .map(|entry| entry.connection)
    }

    /// The `(peer, opened_ms)` record for a live connection.
    pub fn connection_info(&self, connection: u64) -> Option<(String, u64)> {
        self.connections
            .lock()
            .expect("connection registry")
            .get(&connection)
            .map(|entry| (entry.peer.clone(), entry.opened_ms))
    }

    // ---- discovery and health --------------------------------------------------------------------

    /// Answer a discovery request without authentication. Returns the encoded document when `path` is a
    /// configured discovery route, or `None` otherwise.
    pub fn discovery_response(&self, path: &str) -> Option<Vec<u8>> {
        if self.config.discovery.serves(path) {
            self.config.discovery_document().encode().ok()
        } else {
            None
        }
    }

    /// A credential-free health snapshot: whether the runtime is draining and its live session count.
    pub fn health(&self) -> (bool, usize) {
        (self.is_draining(), self.session_count())
    }

    // ---- unary dispatch --------------------------------------------------------------------------

    /// Dispatch a unary request into the local engine, serializing mutations through the single writer
    /// authority. The generated dispatch covers the whole IDL surface; an unknown method returns
    /// `NOT_FOUND`, and `Daemon` is the only generated interface that answers `UNSUPPORTED`. Engine errors
    /// ride the response error envelope.
    pub fn dispatch(&self, request: &Request) -> Response {
        let request_id = request.request_id.clone();
        let session_id = request.session_id.clone();
        let reply_ok = |value: Value| Response::ok(request_id.clone(), session_id.clone(), value);
        let reply_err = |err: &LoomError| {
            Response::err(
                request_id.clone(),
                session_id.clone(),
                RemoteError::from_loom_error(err),
            )
        };

        // The `Mcp` interface is not a generated IDL interface: the local MCP host sends the whole tool
        // operation here, so the tool runs beside the served store under server authority. Routed before
        // the generated method-existence check.
        if request.interface == "Mcp" {
            return self.dispatch_mcp_tool(request);
        }

        if !method_exists(&request.interface, &request.method) {
            return reply_err(&LoomError::new(
                Code::NotFound,
                format!("unknown method {}.{}", request.interface, request.method),
            ));
        }
        // Store lifecycle is owned by the runtime, not the generated engine dispatch: the remote store
        // binding and its sessions are established out of band (`open_session`), so the open family
        // resolves to the request's live session and `close` releases it.
        if request.interface == "Store" {
            match request.method.as_str() {
                "open" | "open_keyed" | "open_with_kek" => {
                    return match self.session_handle(&request.session_id) {
                        Ok(value) => reply_ok(value),
                        Err(err) => reply_err(&err),
                    };
                }
                "close" => {
                    if let Some(bytes) = &request.session_id {
                        self.close_session(bytes);
                    }
                    return reply_ok(Value::Null);
                }
                _ => {}
            }
        }
        let Some(session_bytes) = request.session_id.as_ref() else {
            return reply_err(&LoomError::new(
                Code::PermissionDenied,
                "method requires a session",
            ));
        };
        let session_key = match session_key(session_bytes) {
            Ok(key) => key,
            Err(err) => return reply_err(&err),
        };
        if !self.session_exists(session_key) {
            return reply_err(&LoomError::new(
                Code::NotFound,
                "unknown or expired session",
            ));
        }
        let connection = self.session_connection(session_bytes).unwrap_or_default();
        // Validate consumed sub-handles (SqlSession/SqlBatch/RowIter/Task/File) before the engine call.
        if let Err(err) = self.validate_consumed_handles(
            session_key,
            &request.interface,
            &request.method,
            &request.args,
        ) {
            return reply_err(&err);
        }
        // Serialize every generated engine call through the single write authority, and resolve the engine
        // session the call runs against under that lock (short-lived, borrowed, pinned, or a placeholder
        // for the path-bound SQL family - never a per-session long-lived writer lock).
        let _guard = self.write_lock.lock().expect("write authority");
        // Idempotency dedup under the single write authority (specs/0067 §6): checking, executing, and
        // remembering atomically under this lock makes an in-flight duplicate deterministic — a second
        // request with the same key replays the first's terminal result instead of re-applying the effect.
        // Only mutating, non-naturally-idempotent methods carry a key (the caller's §6 contract); a replay
        // with a different request fingerprint on the same key is rejected inside `idempotency_precheck`.
        let idem_key = request.idempotency_key.as_deref().filter(|k| !k.is_empty());
        let idem_fingerprint = idem_key.and_then(|_| {
            Self::idem_fingerprint(&request.interface, &request.method, &request.args)
        });
        if let (Some(key), Some(fingerprint)) = (idem_key, idem_fingerprint.as_deref())
            && let Some(payload) = self.idempotency_precheck(
                session_key,
                &request.interface,
                &request.method,
                key,
                fingerprint,
            )
        {
            return Response {
                request_id: request_id.clone(),
                session_id: session_id.clone(),
                payload,
            };
        }
        let lease = match self.plan_engine(session_key, &request.interface, &request.method) {
            Ok(lease) => lease,
            Err(err) => return reply_err(&err),
        };
        let dispatched = generated_dispatch::dispatch(
            &self.client,
            lease.session(),
            &request.interface,
            &request.method,
            &request.args,
        );
        let response = match dispatched {
            Ok(Dispatched::Unary(value)) => {
                // Register a minted sub-handle and forget a freed one, keeping the runtime registry the
                // owner of every engine handle it hands out.
                let registered = match self.register_minted_handle(
                    session_key,
                    connection,
                    lease.session(),
                    &request.interface,
                    &request.method,
                    &value,
                    lease.is_pinned(),
                ) {
                    Ok(registered) => registered,
                    Err(err) => {
                        self.release_engine(session_key, lease, false);
                        return reply_err(&err);
                    }
                };
                if let Err(err) =
                    self.forget_freed_handle(&request.interface, &request.method, &request.args)
                {
                    self.release_engine(session_key, lease, registered);
                    return reply_err(&err);
                }
                self.release_engine(session_key, lease, registered);
                reply_ok(value)
            }
            Ok(Dispatched::Stream(_)) => {
                self.release_engine(session_key, lease, false);
                reply_err(&LoomError::new(
                    Code::InvalidArgument,
                    format!(
                        "method {}.{} is a streaming method; use the streaming route",
                        request.interface, request.method
                    ),
                ))
            }
            Err(err) => {
                self.release_engine(session_key, lease, false);
                reply_err(&err)
            }
        };
        // Remember the terminal outcome for an idempotency retry (exact-fingerprint replay). Still under the
        // write authority, so the store is atomic with the effect above.
        if let (Some(key), Some(fingerprint)) = (idem_key, idem_fingerprint) {
            self.idempotency_remember(
                session_key,
                &request.interface,
                &request.method,
                key,
                fingerprint,
                &response.payload,
            );
        }
        response
    }

    /// Server-side MCP tool execution route. The local MCP host forwards a whole tool operation as an
    /// `Mcp.call_tool` request carrying `[Text(tool_name), Bytes(args)]` (opaque argument bytes). The
    /// runtime resolves the session, runs the injected [`McpToolExecutor`] under the single write
    /// authority (so a server-executed write shares the store's single-writer discipline and idempotency
    /// dedup), and returns the opaque tool-result bytes. Requests are rejected precisely when no executor
    /// is installed or the executor declines the tool; the runtime never reconstructs tool semantics from
    /// low-level primitives.
    fn dispatch_mcp_tool(&self, request: &Request) -> Response {
        let request_id = request.request_id.clone();
        let session_id = request.session_id.clone();
        let reply_ok = |value: Value| Response::ok(request_id.clone(), session_id.clone(), value);
        let reply_err = |err: &LoomError| {
            Response::err(
                request_id.clone(),
                session_id.clone(),
                RemoteError::from_loom_error(err),
            )
        };

        if request.method != "call_tool" {
            return reply_err(&LoomError::new(
                Code::NotFound,
                format!("unknown MCP operation Mcp.{}", request.method),
            ));
        }
        let (name, args) = match request.args.as_slice() {
            [Value::Text(name), Value::Bytes(args)] => (name.clone(), args.clone()),
            _ => {
                return reply_err(&LoomError::new(
                    Code::InvalidArgument,
                    "Mcp.call_tool expects [tool_name, args]",
                ));
            }
        };
        let Some(session_bytes) = request.session_id.as_ref() else {
            return reply_err(&LoomError::new(
                Code::PermissionDenied,
                "Mcp.call_tool requires a session",
            ));
        };
        let session_key = match session_key(session_bytes) {
            Ok(key) => key,
            Err(err) => return reply_err(&err),
        };
        if !self.session_exists(session_key) {
            return reply_err(&LoomError::new(
                Code::NotFound,
                "unknown or expired session",
            ));
        }
        let Some(executor) = self.mcp_executor.clone() else {
            return reply_err(&LoomError::new(
                Code::Unsupported,
                format!(
                    "MCP tool {name} is not available against this remote endpoint: server-side MCP tool execution is not configured"
                ),
            ));
        };
        let idem_key = request.idempotency_key.as_deref().filter(|k| !k.is_empty());
        let idem_fingerprint =
            idem_key.and_then(|_| Self::idem_fingerprint("Mcp", "call_tool", &request.args));
        // Single write authority: a server-executed tool shares the store's single-writer discipline, and
        // idempotency dedup is checked/remembered atomically under this lock (specs/0067 §6).
        let _guard = self.write_lock.lock().expect("write authority");
        if let (Some(key), Some(fingerprint)) = (idem_key, idem_fingerprint.as_deref())
            && let Some(payload) =
                self.idempotency_precheck(session_key, "Mcp", "call_tool", key, fingerprint)
        {
            return Response {
                request_id,
                session_id,
                payload,
            };
        }
        let ctx = McpToolContext {
            session_principal: self.session_principal(session_bytes),
            idempotency_key: idem_key.map(<[u8]>::to_vec),
            deadline_ms: (request.deadline_ms != 0).then_some(request.deadline_ms),
        };
        let response = match executor.call_tool(&ctx, &name, &args) {
            Ok(bytes) => reply_ok(Value::Bytes(bytes)),
            Err(err) => reply_err(&err),
        };
        if let (Some(key), Some(fingerprint)) = (idem_key, idem_fingerprint) {
            self.idempotency_remember(
                session_key,
                "Mcp",
                "call_tool",
                key,
                fingerprint,
                &response.payload,
            );
        }
        response
    }

    /// Whether a protocol session is live.
    fn session_exists(&self, session_key: u64) -> bool {
        self.sessions
            .lock()
            .expect("session registry")
            .contains_key(&session_key)
    }

    /// Resolve the engine session a `(interface, method)` call runs against, honoring the store's
    /// single-writer / lock-free-reader model:
    /// - the path-bound SQL family (`loom_path`/`SqlSession`/`SqlBatch`) needs no engine session, so it
    ///   gets a non-opening placeholder and reopens the bound path itself;
    /// - `FileHandle.open` and `LoomSession`-capturing async `Task` mints pin a per-session engine writer
    ///   whose in-memory state must survive across calls;
    /// - other `FileHandle` calls borrow that pinned writer;
    /// - every other engine method reuses a pinned writer if one is held, else opens a short-lived writer
    ///   that is closed the moment the call returns, so no lock is held between calls.
    ///
    /// Caller must hold `write_lock`.
    fn plan_engine(
        &self,
        session_key: u64,
        interface: &str,
        method: &str,
    ) -> Result<EngineLease, LoomError> {
        if !method_takes_engine(interface, method) {
            return Ok(EngineLease::Placeholder(placeholder_session()));
        }
        if interface == "FileHandle" && method == "open" {
            return Ok(EngineLease::Pinned(self.pin_writer(session_key)?));
        }
        if interface == "FileHandle" {
            let writer = self
                .held_writer(session_key)
                .ok_or_else(|| LoomError::new(Code::NotFound, "file handle session is not open"))?;
            return Ok(EngineLease::Borrowed(writer));
        }
        // An async op whose first argument is the engine session captures it by value and reruns on poll,
        // so the session must outlive the minting call: pin it until the task is freed.
        if method_sig(interface, method).is_some_and(|sig| sig.ret == "Task") {
            return Ok(EngineLease::Pinned(self.pin_writer(session_key)?));
        }
        if let Some(writer) = self.held_writer(session_key) {
            return Ok(EngineLease::Borrowed(writer));
        }
        Ok(EngineLease::ShortLived(self.open_engine(session_key)?))
    }

    /// Dispose of a resolved engine lease after a call: close a short-lived writer, release a pin that did
    /// not become a live handle, and leave borrowed/placeholder/kept-pin leases untouched.
    /// Caller must hold `write_lock`.
    fn release_engine(&self, session_key: u64, lease: EngineLease, kept_pin: bool) {
        match lease {
            EngineLease::ShortLived(engine) => {
                self.client.close(&engine);
            }
            EngineLease::Pinned(_) => {
                if !kept_pin {
                    self.unpin_writer(session_key);
                }
            }
            EngineLease::Borrowed(_) | EngineLease::Placeholder(_) => {}
        }
    }

    /// Open a short-lived engine session for `session_key`, replaying its stored credential. The returned
    /// session must be closed by the caller (via [`RemoteRuntime::release_engine`]). Caller holds
    /// `write_lock`.
    fn open_engine(&self, session_key: u64) -> Result<LoomSession, LoomError> {
        let auth = {
            let sessions = self.sessions.lock().expect("session registry");
            sessions
                .get(&session_key)
                .ok_or_else(|| LoomError::new(Code::NotFound, "unknown or expired session"))?
                .auth
                .clone()
        };
        let engine = self.client.open()?;
        if let RemoteAuth::Passphrase {
            principal,
            passphrase,
        } = &auth
            && let Err(err) = self
                .client
                .authenticate_passphrase(&engine, *principal, passphrase)
        {
            self.client.close(&engine);
            return Err(err);
        }
        Ok(engine)
    }

    /// The session's pinned engine writer, if one is currently held. Caller holds `write_lock`.
    fn held_writer(&self, session_key: u64) -> Option<LoomSession> {
        self.sessions
            .lock()
            .expect("session registry")
            .get(&session_key)
            .and_then(|entry| entry.writer.clone())
    }

    /// Ensure `session_key` holds an engine writer and add a ref to it, returning the writer. Opens the
    /// writer lazily on the first pin. Caller holds `write_lock` (so pins do not race).
    fn pin_writer(&self, session_key: u64) -> Result<LoomSession, LoomError> {
        if self.held_writer(session_key).is_some() {
            let mut sessions = self.sessions.lock().expect("session registry");
            let entry = sessions
                .get_mut(&session_key)
                .ok_or_else(|| LoomError::new(Code::NotFound, "unknown or expired session"))?;
            entry.writer_refs = entry.writer_refs.saturating_add(1);
            return Ok(entry.writer.clone().expect("writer present"));
        }
        let engine = self.open_engine(session_key)?;
        let mut sessions = self.sessions.lock().expect("session registry");
        match sessions.get_mut(&session_key) {
            Some(entry) => {
                entry.writer = Some(engine.clone());
                entry.writer_refs = entry.writer_refs.saturating_add(1);
                Ok(engine)
            }
            None => {
                drop(sessions);
                self.client.close(&engine);
                Err(LoomError::new(Code::NotFound, "unknown or expired session"))
            }
        }
    }

    /// Drop a ref on `session_key`'s engine writer, closing it when the last ref is released. Caller holds
    /// `write_lock`.
    fn unpin_writer(&self, session_key: u64) {
        let to_close = {
            let mut sessions = self.sessions.lock().expect("session registry");
            match sessions.get_mut(&session_key) {
                Some(entry) => {
                    entry.writer_refs = entry.writer_refs.saturating_sub(1);
                    if entry.writer_refs == 0 {
                        entry.writer.take()
                    } else {
                        None
                    }
                }
                None => None,
            }
        };
        if let Some(writer) = to_close {
            self.client.close(&writer);
        }
    }

    /// The `LoomSession` handle value for the request's live session, which answers the runtime-owned
    /// `Store.open*` family (the session itself is established by `open_session`).
    fn session_handle(&self, session_id: &Option<Vec<u8>>) -> Result<Value, LoomError> {
        let bytes = session_id
            .as_ref()
            .ok_or_else(|| LoomError::new(Code::PermissionDenied, "method requires a session"))?;
        let key = session_key(bytes)?;
        if !self
            .sessions
            .lock()
            .expect("session registry")
            .contains_key(&key)
        {
            return Err(LoomError::new(Code::NotFound, "unknown or expired session"));
        }
        Ok(LoomSession(HandleId {
            kind: "session".to_string(),
            id: bytes.clone(),
            generation: 1,
            owner_session: bytes.clone(),
        })
        .to_value())
    }

    /// Validate every consumed sub-handle argument of `(interface, method)` against the registry before
    /// dispatch, so a stale or cross-session handle is rejected up front.
    fn validate_consumed_handles(
        &self,
        session_key: u64,
        interface: &str,
        method: &str,
        args: &[Value],
    ) -> Result<(), LoomError> {
        let Some(sig) = method_sig(interface, method) else {
            return Ok(());
        };
        let mut wire_idx = 0usize;
        for (ty, name) in sig.args {
            if *name == "loom_path" {
                continue;
            }
            // The engine session (`LoomSession handle`) is resolved by the runtime, not registry-tracked.
            if *name == "handle" && *ty == "LoomSession" {
                wire_idx += 1;
                continue;
            }
            if let Some(kind) = managed_kind(ty) {
                let value = args.get(wire_idx).ok_or_else(|| {
                    LoomError::new(Code::InvalidArgument, "missing handle argument")
                })?;
                self.validate_handle(session_key, kind, handle_id_of(value)?)?;
            } else if interface == "FileHandle" && *name == "file" {
                let value = args.get(wire_idx).ok_or_else(|| {
                    LoomError::new(Code::InvalidArgument, "missing file handle argument")
                })?;
                self.validate_handle(session_key, HandleKind::File, file_id_of(value)?)?;
            }
            wire_idx += 1;
        }
        Ok(())
    }

    /// Register a sub-handle minted by a unary generated dispatch (its returned `value`), owned by the
    /// request session, so later calls validate it and teardown frees it. `pinned` is true when the
    /// minting call ran against a pinned engine writer (`FileHandle.open` or a `LoomSession`-capturing
    /// async `Task`); the registered handle then holds a ref on that writer. Returns whether a
    /// writer-pinning handle was registered, so the caller keeps the pin alive.
    fn register_minted_handle(
        &self,
        session_key: u64,
        connection: u64,
        engine: &LoomSession,
        interface: &str,
        method: &str,
        value: &Value,
        pinned: bool,
    ) -> Result<bool, LoomError> {
        let reg_err = |err: loom_remote_protocol::codec::ArgError| {
            LoomError::new(Code::Internal, format!("register minted handle: {err}"))
        };
        let Some(sig) = method_sig(interface, method) else {
            return Ok(false);
        };
        if let Some(kind) = managed_kind(sig.ret) {
            let backing = match kind {
                HandleKind::SqlSession => {
                    HandleBacking::Sql(SqlSession::from_value(value).map_err(reg_err)?)
                }
                HandleKind::SqlBatch => {
                    HandleBacking::Batch(SqlBatch::from_value(value).map_err(reg_err)?)
                }
                HandleKind::RowIter => {
                    HandleBacking::Row(RowIter::from_value(value).map_err(reg_err)?)
                }
                HandleKind::Task => HandleBacking::Task(Task::from_value(value).map_err(reg_err)?),
                HandleKind::File => return Ok(false),
            };
            // Only a `LoomSession`-capturing async `Task` pins the writer; SQL sub-handles are path-bound.
            let pins_writer = pinned && kind == HandleKind::Task;
            self.register_backing(session_key, connection, backing, pins_writer);
            return Ok(pins_writer);
        } else if interface == "FileHandle" && method == "open" {
            let file = file_id_of(value)?;
            self.register_backing(
                session_key,
                connection,
                HandleBacking::File {
                    engine: engine.clone(),
                    file,
                },
                true,
            );
            return Ok(true);
        }
        Ok(false)
    }

    /// Drop the registry record for a handle freed by a close/free method (its engine handle was freed by
    /// the dispatched trait call).
    fn forget_freed_handle(
        &self,
        interface: &str,
        method: &str,
        args: &[Value],
    ) -> Result<(), LoomError> {
        let Some((kind, idx)) = freed_handle_slot(interface, method) else {
            return Ok(());
        };
        if let Some(value) = args.get(idx) {
            let id = if kind == HandleKind::File {
                file_id_of(value)?
            } else {
                handle_id_of(value)?
            };
            self.forget_handle(kind, id);
        }
        Ok(())
    }

    /// Dispatch a streaming request into the local engine, returning the ordered encoded frames the
    /// server emits: one `item` frame per element, a `trailer` with the count, and a terminal
    /// `complete`; an engine or routing failure yields a single terminal `error` frame. Frame encoding
    /// never fails for these frames.
    pub fn dispatch_stream(&self, request: &Request) -> Vec<Vec<u8>> {
        let frames = match self.stream_items(request) {
            Ok(items) => {
                let count = items.len() as u64;
                let mut frames: Vec<Frame> = items.into_iter().map(Frame::Item).collect();
                frames.push(Frame::Trailer(count.to_be_bytes().to_vec()));
                frames.push(Frame::Complete);
                frames
            }
            Err(err) => vec![Frame::Error(RemoteError::from_loom_error(&err))],
        };
        frames
            .into_iter()
            .map(|frame| frame.encode().unwrap_or_default())
            .collect()
    }

    /// Open an incremental server-side frame stream for a streaming request, so the carrier can push
    /// frames one at a time (bounded memory, honoring the client's flow-control window) instead of
    /// buffering the whole response. `Diagnostics.event_tail` is an unbounded server-driven tick tail (a
    /// diagnostic push stream that never completes on its own); every other streaming method yields its
    /// generated frames, which the carrier still streams incrementally over the wire.
    pub fn open_frame_stream(&self, request: &Request) -> ServerFrameStream {
        if request.interface == "Diagnostics" && request.method == "event_tail" {
            let mut tick: u64 = 0;
            return ServerFrameStream::lazy(move || {
                let frame = Frame::Item(tick.to_be_bytes().to_vec());
                tick = tick.wrapping_add(1);
                frame.encode().ok()
            });
        }
        ServerFrameStream::buffered(self.dispatch_stream(request))
    }

    fn stream_items(&self, request: &Request) -> Result<Vec<Vec<u8>>, LoomError> {
        if !method_exists(&request.interface, &request.method) {
            return Err(LoomError::new(
                Code::NotFound,
                format!("unknown method {}.{}", request.interface, request.method),
            ));
        }
        let session_bytes = request
            .session_id
            .as_ref()
            .ok_or_else(|| LoomError::new(Code::PermissionDenied, "method requires a session"))?;
        let session_key = session_key(session_bytes)?;
        if !self.session_exists(session_key) {
            return Err(LoomError::new(Code::NotFound, "unknown or expired session"));
        }
        self.validate_consumed_handles(
            session_key,
            &request.interface,
            &request.method,
            &request.args,
        )?;
        // Serialize every generated engine call through the single write authority, resolving the engine
        // session under that lock (a streaming method mints no handle, so its lease is released here).
        let _guard = self.write_lock.lock().expect("write authority");
        let lease = self.plan_engine(session_key, &request.interface, &request.method)?;
        let dispatched = generated_dispatch::dispatch(
            &self.client,
            lease.session(),
            &request.interface,
            &request.method,
            &request.args,
        );
        let out = match dispatched {
            Ok(Dispatched::Stream(items)) => Ok(items),
            Ok(Dispatched::Unary(_)) => Err(LoomError::new(
                Code::InvalidArgument,
                format!(
                    "method {}.{} is not a streaming method; use the unary route",
                    request.interface, request.method
                ),
            )),
            Err(err) => Err(err),
        };
        self.release_engine(session_key, lease, false);
        out
    }

    // ---- streams and handles ---------------------------------------------------------------------

    /// Open a stream on `session`/`connection` with an initial client credit and a prepared item
    /// source. Returns the stream id.
    ///
    /// # Errors
    /// Returns [`LoomError`] when the runtime is draining.
    pub fn open_stream(
        &self,
        session: &[u8],
        connection: u64,
        initial_credit: u32,
        items: Vec<Vec<u8>>,
    ) -> Result<u64, LoomError> {
        if self.is_draining() {
            return Err(LoomError::new(Code::Unsupported, "runtime is draining"));
        }
        let session = session_key(session)?;
        let id = self.mint_id();
        self.streams.lock().expect("stream registry").insert(
            id,
            StreamEntry {
                session,
                connection,
                credit: initial_credit,
                delivered: 0,
                items: items.into_iter().collect(),
                closed: false,
            },
        );
        Ok(id)
    }

    /// Grant additional credit to a stream.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND`) for an unknown stream.
    pub fn grant_credit(&self, stream: u64, credit: u32) -> Result<(), LoomError> {
        let mut streams = self.streams.lock().expect("stream registry");
        let entry = streams
            .get_mut(&stream)
            .ok_or_else(|| LoomError::new(Code::NotFound, "unknown stream"))?;
        entry.credit = entry.credit.saturating_add(credit);
        Ok(())
    }

    /// Produce the next frame for a stream, honoring credit-based backpressure. Returns `None` when
    /// credit is exhausted but items remain (the client must grant more credit); an item frame when an
    /// item is delivered; and a terminal `complete` frame (closing the stream) when the source is empty.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND`) for an unknown or closed stream.
    pub fn stream_next(&self, stream: u64) -> Result<Option<Frame>, LoomError> {
        let mut streams = self.streams.lock().expect("stream registry");
        let entry = streams
            .get_mut(&stream)
            .filter(|s| !s.closed)
            .ok_or_else(|| LoomError::new(Code::NotFound, "unknown or closed stream"))?;
        if entry.items.is_empty() {
            entry.closed = true;
            streams.remove(&stream);
            return Ok(Some(Frame::Complete));
        }
        if entry.credit == 0 {
            return Ok(None);
        }
        let item = entry.items.pop_front().expect("non-empty items");
        entry.credit -= 1;
        entry.delivered += 1;
        Ok(Some(Frame::Item(item)))
    }

    /// Cancel a stream, closing it and releasing its resources. Returns whether a stream was open.
    pub fn cancel_stream(&self, stream: u64) -> bool {
        self.streams
            .lock()
            .expect("stream registry")
            .remove(&stream)
            .is_some()
    }

    /// The number of live streams.
    pub fn stream_count(&self) -> usize {
        self.streams.lock().expect("stream registry").len()
    }

    /// Register a live `LocalLoomClient` handle minted by a generated dispatch, owned by `owner_session`
    /// on `connection`, so later calls can validate it and teardown can free it.
    fn register_backing(
        &self,
        owner_session: u64,
        connection: u64,
        backing: HandleBacking,
        pins_writer: bool,
    ) {
        let key = (backing.kind(), backing.local_id());
        self.handles.lock().expect("handle registry").insert(
            key,
            HandleEntry {
                generation: 1,
                owner_session,
                connection,
                last_use_ms: now_ms(),
                backing,
                pins_writer,
            },
        );
    }

    /// Validate that handle `(kind, id)` is live and owned by `owner_session`, refreshing its last-use
    /// time. A stale/freed handle is `NOT_FOUND`; a cross-session handle is `PERMISSION_DENIED`.
    fn validate_handle(
        &self,
        owner_session: u64,
        kind: HandleKind,
        id: u64,
    ) -> Result<(), LoomError> {
        let mut handles = self.handles.lock().expect("handle registry");
        let entry = handles
            .get_mut(&(kind, id))
            .ok_or_else(|| LoomError::new(Code::NotFound, "unknown or freed handle"))?;
        if entry.owner_session != owner_session {
            return Err(LoomError::new(
                Code::PermissionDenied,
                "handle belongs to another session",
            ));
        }
        entry.last_use_ms = now_ms();
        Ok(())
    }

    /// Drop the registry record for handle `(kind, id)` (its engine handle was freed by the dispatched
    /// close/free call), releasing its engine-writer ref if it held one. Caller holds `write_lock`.
    fn forget_handle(&self, kind: HandleKind, id: u64) {
        let removed = self
            .handles
            .lock()
            .expect("handle registry")
            .remove(&(kind, id));
        if let Some(entry) = removed
            && entry.pins_writer
        {
            self.unpin_writer(entry.owner_session);
        }
    }

    /// The number of live registered handles (introspection/tests).
    pub fn handle_count(&self) -> usize {
        self.handles.lock().expect("handle registry").len()
    }

    /// The `(owner_session, connection, generation)` of a live registered handle (introspection/tests).
    pub fn managed_handle(&self, kind: HandleKind, id: u64) -> Option<(u64, u64, u64)> {
        self.handles
            .lock()
            .expect("handle registry")
            .get(&(kind, id))
            .map(|entry| (entry.owner_session, entry.connection, entry.generation))
    }

    /// Close/free the underlying `LocalLoomClient` handle behind a registry entry. Best-effort: a handle
    /// whose backing was already reclaimed (e.g. a file on an engine session being torn down) is ignored.
    fn close_backing(&self, backing: &HandleBacking) {
        match backing {
            HandleBacking::Sql(handle) => {
                self.client.sql_close(handle);
            }
            HandleBacking::Batch(handle) => {
                self.client.sql_batch_close(handle);
            }
            HandleBacking::Row(handle) => {
                self.client.iter_free(handle);
            }
            HandleBacking::Task(handle) => {
                self.client.task_free(handle);
            }
            HandleBacking::File { engine, file } => {
                let _ = self.client.file_close(engine, *file);
            }
        }
    }

    /// Free and forget every handle owned by `session_key`, closing the underlying engine handles so a
    /// session teardown never leaks them. Must run while the engine session is still open (file handles
    /// need it).
    fn close_session_handles(&self, session_key: u64) {
        let drained: Vec<HandleEntry> = {
            let mut handles = self.handles.lock().expect("handle registry");
            let keys: Vec<(HandleKind, u64)> = handles
                .iter()
                .filter(|(_, entry)| entry.owner_session == session_key)
                .map(|(key, _)| *key)
                .collect();
            keys.into_iter()
                .filter_map(|key| handles.remove(&key))
                .collect()
        };
        for entry in &drained {
            self.close_backing(&entry.backing);
        }
    }

    fn close_session_runtime_resources(&self, session_key: u64) {
        self.streams
            .lock()
            .expect("stream registry")
            .retain(|_, entry| entry.session != session_key);
        self.tasks
            .lock()
            .expect("task registry")
            .retain(|_, entry| entry.owner_session != session_key);
        self.watches
            .lock()
            .expect("watch registry")
            .retain(|_, entry| entry.owner_session != session_key);
    }

    /// The owning session key of a live stream.
    pub fn stream_owner_session(&self, stream: u64) -> Option<u64> {
        self.streams
            .lock()
            .expect("stream registry")
            .get(&stream)
            .map(|s| s.session)
    }

    // ---- tasks -----------------------------------------------------------------------------------

    /// Create a running task owned by `session`, returning its id.
    pub fn create_task(&self, session: &[u8]) -> Result<u64, LoomError> {
        let owner_session = session_key(session)?;
        let id = self.mint_id();
        self.tasks.lock().expect("task registry").insert(
            id,
            TaskEntry {
                owner_session,
                state: TaskState::Running,
                result: None,
            },
        );
        Ok(id)
    }

    /// Complete a task with its terminal result, retained until taken.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND`) for an unknown task.
    pub fn complete_task(&self, task: u64, result: Vec<u8>) -> Result<(), LoomError> {
        let mut tasks = self.tasks.lock().expect("task registry");
        let entry = tasks
            .get_mut(&task)
            .ok_or_else(|| LoomError::new(Code::NotFound, "unknown task"))?;
        entry.state = TaskState::Done;
        entry.result = Some(result);
        Ok(())
    }

    /// Cancel a task. Returns whether one was cancelled.
    pub fn cancel_task(&self, task: u64) -> bool {
        let mut tasks = self.tasks.lock().expect("task registry");
        match tasks.get_mut(&task) {
            Some(entry) => {
                entry.state = TaskState::Cancelled;
                entry.result = None;
                true
            }
            None => false,
        }
    }

    /// The state of a task, if it exists.
    pub fn task_state(&self, task: u64) -> Option<TaskState> {
        self.tasks
            .lock()
            .expect("task registry")
            .get(&task)
            .map(|t| t.state)
    }

    /// The owning session key of a task.
    pub fn task_owner_session(&self, task: u64) -> Option<u64> {
        self.tasks
            .lock()
            .expect("task registry")
            .get(&task)
            .map(|t| t.owner_session)
    }

    /// Take a completed task's result once (take-once retention).
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND`) for an unknown task or one whose result was already taken.
    pub fn take_task_result(&self, task: u64) -> Result<Vec<u8>, LoomError> {
        let mut tasks = self.tasks.lock().expect("task registry");
        let entry = tasks
            .get_mut(&task)
            .ok_or_else(|| LoomError::new(Code::NotFound, "unknown task"))?;
        entry
            .result
            .take()
            .ok_or_else(|| LoomError::new(Code::NotFound, "task result already taken or absent"))
    }

    // ---- watches ---------------------------------------------------------------------------------

    /// Open a watch owned by `session` starting at `cursor`, returning its id.
    pub fn open_watch(&self, session: &[u8], cursor: impl Into<String>) -> Result<u64, LoomError> {
        let owner_session = session_key(session)?;
        let id = self.mint_id();
        self.watches.lock().expect("watch registry").insert(
            id,
            WatchEntry {
                owner_session,
                cursor: cursor.into(),
            },
        );
        Ok(id)
    }

    /// Advance a watch cursor.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND`) for an unknown watch.
    pub fn advance_watch(&self, watch: u64, cursor: impl Into<String>) -> Result<(), LoomError> {
        let mut watches = self.watches.lock().expect("watch registry");
        let entry = watches
            .get_mut(&watch)
            .ok_or_else(|| LoomError::new(Code::NotFound, "unknown watch"))?;
        entry.cursor = cursor.into();
        Ok(())
    }

    /// Close a watch, returning whether one was open.
    pub fn close_watch(&self, watch: u64) -> bool {
        self.watches
            .lock()
            .expect("watch registry")
            .remove(&watch)
            .is_some()
    }

    /// The `(owner session key, cursor)` of a live watch.
    pub fn watch_state(&self, watch: u64) -> Option<(u64, String)> {
        self.watches
            .lock()
            .expect("watch registry")
            .get(&watch)
            .map(|w| (w.owner_session, w.cursor.clone()))
    }
}

/// The engine session a dispatch runs against, and how it must be disposed afterwards.
enum EngineLease {
    /// A freshly opened engine writer to close as soon as the call returns.
    ShortLived(LoomSession),
    /// A pinned engine writer borrowed for this call; the pin outlives the call.
    Borrowed(LoomSession),
    /// A newly pinned engine writer; the pin is kept only if the call registers a pinning handle.
    Pinned(LoomSession),
    /// A non-opening handle for the path-bound SQL family, which reopens the bound path itself.
    Placeholder(LoomSession),
}

impl EngineLease {
    fn session(&self) -> &LoomSession {
        match self {
            EngineLease::ShortLived(session)
            | EngineLease::Borrowed(session)
            | EngineLease::Pinned(session)
            | EngineLease::Placeholder(session) => session,
        }
    }

    fn is_pinned(&self) -> bool {
        matches!(self, EngineLease::Pinned(_))
    }
}

/// Whether `(interface, method)` takes the engine session as its first IDL argument
/// (`LoomSession handle`). The path-bound SQL family and pure sub-handle operations do not, so they need
/// no runtime-resolved engine session.
fn method_takes_engine(interface: &str, method: &str) -> bool {
    method_sig(interface, method)
        .and_then(|sig| sig.args.first())
        .is_some_and(|(ty, name)| *ty == "LoomSession" && *name == "handle")
}

/// A non-opening `LoomSession` value for dispatches that do not touch a runtime engine session (the
/// path-bound SQL family): the generated dispatch decodes its own handle argument and never reads this.
fn placeholder_session() -> LoomSession {
    LoomSession(HandleId {
        kind: "session".to_string(),
        id: Vec::new(),
        generation: 0,
        owner_session: Vec::new(),
    })
}

fn method_exists(interface: &str, method: &str) -> bool {
    METHODS
        .iter()
        .any(|m| m.interface == interface && m.method == method)
}

fn session_key(session_id: &[u8]) -> Result<u64, LoomError> {
    let bytes: [u8; 8] = session_id
        .try_into()
        .map_err(|_| LoomError::new(Code::InvalidArgument, "malformed session id"))?;
    Ok(u64::from_be_bytes(bytes))
}

/// The local `u64` id inside a `HandleId`; ids minted by `LocalLoomClient` are 8 big-endian bytes.
fn handle_local_id(id: &HandleId) -> u64 {
    match id.id.as_slice().try_into() {
        Ok(bytes) => u64::from_be_bytes(bytes),
        Err(_) => u64::MAX,
    }
}

/// The managed handle kind an IDL argument/return type names, if it is a runtime-tracked sub-handle.
fn managed_kind(ty: &str) -> Option<HandleKind> {
    match ty.trim() {
        "SqlSession" => Some(HandleKind::SqlSession),
        "SqlBatch" => Some(HandleKind::SqlBatch),
        "RowIter" => Some(HandleKind::RowIter),
        "Task" => Some(HandleKind::Task),
        _ => None,
    }
}

/// The `(kind, wire arg index)` of the handle a close/free method releases, if any.
fn freed_handle_slot(interface: &str, method: &str) -> Option<(HandleKind, usize)> {
    match (interface, method) {
        ("Sql", "sql_close") => Some((HandleKind::SqlSession, 0)),
        ("Sql", "sql_batch_close") => Some((HandleKind::SqlBatch, 0)),
        ("Tasks", "iter_free") => Some((HandleKind::RowIter, 0)),
        ("Tasks", "task_free") => Some((HandleKind::Task, 0)),
        // FileHandle.close(LoomSession handle, u64 file): the file id is the second wire arg.
        ("FileHandle", "close") => Some((HandleKind::File, 1)),
        _ => None,
    }
}

fn method_sig(interface: &str, method: &str) -> Option<&'static MethodSig> {
    METHODS
        .iter()
        .find(|m| m.interface == interface && m.method == method)
}

/// Decode the local `u64` id of a handle argument `Value` (a `HandleId` array).
fn handle_id_of(value: &Value) -> Result<u64, LoomError> {
    let handle = <HandleId as FromValue>::from_value(value)
        .map_err(|err| LoomError::new(Code::InvalidArgument, format!("handle arg: {err}")))?;
    Ok(handle_local_id(&handle))
}

/// Decode a `u64` file id argument `Value`.
fn file_id_of(value: &Value) -> Result<u64, LoomError> {
    <u64 as FromValue>::from_value(value)
        .map_err(|err| LoomError::new(Code::InvalidArgument, format!("file id arg: {err}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_compute::{GrantSet, Manifest};
    use loom_core::acl::{AclEffect, AclGrant, AclRight, AclScope, AclSubject};
    use loom_core::{
        FacetKind, IdentityPublicKeySpec, IdentityStore, LogRecord, LogSeverityNumber, LogValue,
        PrincipalKind,
    };
    use loom_remote_protocol::discovery::{DiscoveryMode, DiscoveryRoutes};
    use loom_remote_protocol::envelope::{Compression, ResponsePayload};
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::atomic::AtomicU64;

    fn temp_store(tag: &str) -> PathBuf {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let mut path = std::env::temp_dir();
        path.push(format!(
            "loom-remote-{tag}-{}-{}.loom",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::remove_dir_all(&path).ok();
        LocalLoomClient::new(&path).create().expect("create store");
        path
    }

    fn config() -> RemoteServerConfig {
        RemoteServerConfig {
            service_root: "https://host/apps/loom".to_string(),
            call_endpoint: "https://host/apps/loom/v1/call".to_string(),
            auth_modes: vec![RemoteAuthMode::Interactive, RemoteAuthMode::Principal],
            tls: vec![RemoteTlsTrust::System],
            discovery: DiscoveryRoutes {
                mode: DiscoveryMode::Default,
                service_root_path: "/apps/loom".to_string(),
                custom_path: None,
            },
            session_lease_ms: 60_000,
        }
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

    /// A fixture server-side MCP tool executor: `fixture_echo` runs server-side and echoes its argument
    /// bytes; any other tool is declined, standing in for a not-yet-promoted tool.
    struct FixtureExecutor;
    impl McpToolExecutor for FixtureExecutor {
        fn call_tool(
            &self,
            _ctx: &McpToolContext,
            name: &str,
            args: &[u8],
        ) -> Result<Vec<u8>, LoomError> {
            match name {
                "fixture_echo" => Ok(args.to_vec()),
                other => Err(LoomError::new(
                    Code::Unsupported,
                    format!("MCP tool {other} is not server-promoted"),
                )),
            }
        }
    }

    struct MutatingFixtureExecutor {
        applied: Arc<AtomicU64>,
    }

    impl McpToolExecutor for MutatingFixtureExecutor {
        fn call_tool(
            &self,
            _ctx: &McpToolContext,
            name: &str,
            args: &[u8],
        ) -> Result<Vec<u8>, LoomError> {
            match name {
                "fixture_mutate" => {
                    let count = self.applied.fetch_add(1, Ordering::SeqCst) + 1;
                    let mut out = count.to_be_bytes().to_vec();
                    out.extend_from_slice(args);
                    Ok(out)
                }
                other => Err(LoomError::new(
                    Code::Unsupported,
                    format!("MCP tool {other} is not server-promoted"),
                )),
            }
        }
    }

    #[test]
    fn mcp_tool_route_executes_server_side_and_rejects_precisely() {
        let path = temp_store("mcp-exec");
        let mut rt = RemoteRuntime::start(&path, config()).expect("start");
        let conn = rt.register_connection("peer");
        let session = rt
            .open_session(conn, RemoteAuth::Unauthenticated)
            .expect("open");
        let call = |rt: &RemoteRuntime, name: &str, args: Vec<u8>| {
            rt.dispatch(&request(
                &session.id,
                "Mcp",
                "call_tool",
                vec![Value::Text(name.to_string()), Value::Bytes(args)],
            ))
            .payload
        };

        // No executor installed: precise UNSUPPORTED, not a silent success.
        match call(&rt, "fixture_echo", b"x".to_vec()) {
            ResponsePayload::Err(e) => {
                assert_eq!(e.code, Code::Unsupported);
                assert!(e.message.contains("not configured"), "{}", e.message);
            }
            other => panic!("expected unsupported error, got {other:?}"),
        }

        rt.set_mcp_executor(Arc::new(FixtureExecutor));

        // A promoted fixture tool executes server-side and returns its result bytes verbatim.
        match call(&rt, "fixture_echo", b"payload".to_vec()) {
            ResponsePayload::Ok(Value::Bytes(bytes)) => assert_eq!(bytes, b"payload"),
            other => panic!("expected server-executed bytes, got {other:?}"),
        }
        // A tool the executor declines (a not-yet-promoted tool) rejects precisely.
        match call(&rt, "not_promoted", Vec::new()) {
            ResponsePayload::Err(e) => assert!(e.message.contains("not server-promoted")),
            other => panic!("expected decline error, got {other:?}"),
        }
        // An unknown MCP operation is NOT_FOUND.
        match rt
            .dispatch(&request(&session.id, "Mcp", "frobnicate", vec![]))
            .payload
        {
            ResponsePayload::Err(e) => assert_eq!(e.code, Code::NotFound),
            other => panic!("expected not-found, got {other:?}"),
        }
        // Malformed args are INVALID_ARGUMENT.
        match rt
            .dispatch(&request(
                &session.id,
                "Mcp",
                "call_tool",
                vec![Value::Text("only-name".to_string())],
            ))
            .payload
        {
            ResponsePayload::Err(e) => assert_eq!(e.code, Code::InvalidArgument),
            other => panic!("expected invalid-argument, got {other:?}"),
        }
        std::fs::remove_dir_all(&path).ok();
    }

    #[test]
    fn mcp_mutating_tool_is_server_side_and_idempotent() {
        let path = temp_store("mcp-mutate");
        let mut rt = RemoteRuntime::start(&path, config()).expect("start");
        let conn = rt.register_connection("peer");
        let session = rt
            .open_session(conn, RemoteAuth::Unauthenticated)
            .expect("open");
        let applied = Arc::new(AtomicU64::new(0));
        rt.set_mcp_executor(Arc::new(MutatingFixtureExecutor {
            applied: Arc::clone(&applied),
        }));

        let mut first = request(
            &session.id,
            "Mcp",
            "call_tool",
            vec![
                Value::Text("fixture_mutate".to_string()),
                Value::Bytes(b"payload".to_vec()),
            ],
        );
        first.idempotency_key = Some(b"same-key".to_vec());
        let replay = first.clone();

        match rt.dispatch(&first).payload {
            ResponsePayload::Ok(Value::Bytes(bytes)) => {
                assert_eq!(&bytes[..8], 1u64.to_be_bytes().as_slice());
                assert_eq!(&bytes[8..], b"payload");
            }
            other => panic!("expected mutating tool result, got {other:?}"),
        }
        match rt.dispatch(&replay).payload {
            ResponsePayload::Ok(Value::Bytes(bytes)) => {
                assert_eq!(&bytes[..8], 1u64.to_be_bytes().as_slice());
                assert_eq!(&bytes[8..], b"payload");
            }
            other => panic!("expected idempotent replay, got {other:?}"),
        }
        assert_eq!(applied.load(Ordering::SeqCst), 1);

        let mut conflict = request(
            &session.id,
            "Mcp",
            "call_tool",
            vec![
                Value::Text("fixture_mutate".to_string()),
                Value::Bytes(b"different".to_vec()),
            ],
        );
        conflict.idempotency_key = Some(b"same-key".to_vec());
        match rt.dispatch(&conflict).payload {
            ResponsePayload::Err(err) => assert_eq!(err.code, Code::Conflict),
            other => panic!("expected idempotency conflict, got {other:?}"),
        }
        assert_eq!(applied.load(Ordering::SeqCst), 1);
        std::fs::remove_dir_all(&path).ok();
    }

    #[test]
    fn starts_opens_a_session_and_shuts_down() {
        let path = temp_store("start");
        let rt = RemoteRuntime::start(&path, config()).expect("start");
        assert_eq!(rt.session_count(), 0);
        assert!(!rt.is_draining());
        let conn = rt.register_connection("peer-1");
        let session = rt
            .open_session(conn, RemoteAuth::Unauthenticated)
            .expect("open session");
        assert_eq!(rt.session_count(), 1);
        assert_eq!(rt.session_connection(&session.id), Some(conn));
        assert_eq!(
            rt.connection_info(conn).map(|(peer, _)| peer),
            Some("peer-1".to_string())
        );
        rt.shutdown();
        assert_eq!(rt.session_count(), 0);
        assert!(rt.is_draining());
        assert!(
            rt.open_session(conn, RemoteAuth::Unauthenticated).is_err(),
            "draining runtime rejects new sessions"
        );
        std::fs::remove_dir_all(&path).ok();
    }

    #[test]
    fn config_and_endpoint_validation() {
        let mut cfg = config();
        assert!(cfg.validate().is_ok());
        cfg.auth_modes.clear();
        assert!(cfg.validate().is_err());

        assert!(validate_endpoint("https", "example.com", &RemoteTlsTrust::System).is_ok());
        assert!(validate_endpoint("http", "example.com", &RemoteTlsTrust::System).is_err());
        assert!(validate_endpoint("http", "127.0.0.1", &RemoteTlsTrust::System).is_ok());
        assert!(validate_endpoint("http", "example.com", &RemoteTlsTrust::InsecureDev).is_ok());
        assert!(validate_endpoint("ftp", "example.com", &RemoteTlsTrust::System).is_err());
    }

    #[test]
    fn session_renewal_and_expiry() {
        let path = temp_store("sess");
        let rt = RemoteRuntime::start(&path, config()).expect("start");
        let conn = rt.register_connection("peer");
        let session = rt
            .open_session(conn, RemoteAuth::Unauthenticated)
            .expect("open");

        let renewed = rt.renew_session(&session.id, 10_000).expect("renew");
        assert_eq!(renewed, 70_000);
        assert_eq!(rt.expire_sessions(0), 0);
        assert_eq!(rt.expire_sessions(u64::MAX), 1);
        assert_eq!(rt.session_count(), 0);
        assert!(
            rt.renew_session(&session.id, 0).is_err(),
            "an expired/unknown session is not renewable"
        );
        std::fs::remove_dir_all(&path).ok();
    }

    #[test]
    fn session_expiry_reclaims_session_owned_runtime_resources() {
        let path = temp_store("sess-resources");
        let rt = RemoteRuntime::start(&path, config()).expect("start");
        let conn = rt.register_connection("peer");
        let session = rt
            .open_session(conn, RemoteAuth::Unauthenticated)
            .expect("open");

        let stream = rt
            .open_stream(&session.id, conn, 10, vec![b"x".to_vec()])
            .expect("open stream");
        let task = rt.create_task(&session.id).expect("task");
        let watch = rt.open_watch(&session.id, "cursor-0").expect("watch");
        assert_eq!(rt.stream_count(), 1);
        assert!(rt.task_owner_session(task).is_some());
        assert!(rt.watch_state(watch).is_some());

        assert_eq!(rt.expire_sessions(u64::MAX), 1);
        assert_eq!(rt.stream_count(), 0);
        assert!(rt.stream_next(stream).is_err());
        assert_eq!(rt.task_owner_session(task), None);
        assert!(rt.take_task_result(task).is_err());
        assert_eq!(rt.watch_state(watch), None);
        assert!(!rt.close_watch(watch));
        std::fs::remove_dir_all(&path).ok();
    }

    #[test]
    fn auth_success_and_failure() {
        let path = temp_store("auth");
        let seed = LocalLoomClient::new(&path);
        let engine = seed.open().expect("open seed");
        let root = WorkspaceId::from_bytes([1; 16]);
        let user = WorkspaceId::from_bytes([7; 16]);
        seed.with_session(&engine, |loom| {
            let mut identity = IdentityStore::new(root);
            identity.add_principal(user, "user", PrincipalKind::User)?;
            identity.set_passphrase(user, "s3cret", b"salt-bytes")?;
            loom.store().save_identity_store(&identity)
        })
        .expect("seed identity");
        seed.close(&engine);

        let rt = RemoteRuntime::start(&path, config()).expect("start");
        let conn = rt.register_connection("peer");
        rt.open_session(
            conn,
            RemoteAuth::Passphrase {
                principal: user,
                passphrase: b"s3cret".to_vec(),
            },
        )
        .expect("authenticated session");
        assert_eq!(rt.session_count(), 1);

        let before = rt.session_count();
        assert!(
            rt.open_session(
                conn,
                RemoteAuth::Passphrase {
                    principal: user,
                    passphrase: b"wrong".to_vec(),
                },
            )
            .is_err(),
            "a bad passphrase is rejected"
        );
        assert_eq!(rt.session_count(), before, "a failed auth leaks no session");
        std::fs::remove_dir_all(&path).ok();
    }

    fn seed_files_acl_fixture(path: &std::path::Path, grant_write: bool) {
        let seed = LocalLoomClient::new(path);
        let engine = seed.open().expect("open seed");
        let root = WorkspaceId::from_bytes([1; 16]);
        let user = WorkspaceId::from_bytes([7; 16]);
        let files = WorkspaceId::from_bytes([9; 16]);
        seed.with_session(&engine, |loom| {
            loom.registry_mut()
                .create(FacetKind::Files, Some("secure"), files)?;
            let mut identity = IdentityStore::new(root);
            identity.add_principal(user, "user", PrincipalKind::User)?;
            identity.set_passphrase(user, "s3cret", b"salt-bytes")?;
            loom.store().save_identity_store(&identity)?;

            let mut acl = loom.store().acl_store()?.unwrap_or_default();
            if grant_write {
                acl.grant(AclGrant {
                    subject: AclSubject::Principal(user),
                    workspace: Some(files),
                    domain: Some(FacetKind::Files.into()),
                    ref_glob: None,
                    scopes: vec![AclScope::All],
                    rights: [AclRight::Write].into_iter().collect(),
                    effect: AclEffect::Allow,
                    predicate: None,
                })?;
            }
            loom.store().save_acl_store(&acl)?;
            Ok(())
        })
        .expect("seed acl fixture");
        seed.save(&engine).expect("save seed");
        seed.close(&engine);
    }

    fn remote_write_file_payload(path: &std::path::Path) -> ResponsePayload {
        let rt = RemoteRuntime::start(path, config()).expect("start");
        let conn = rt.register_connection("peer");
        let session = rt
            .open_session(
                conn,
                RemoteAuth::Passphrase {
                    principal: WorkspaceId::from_bytes([7; 16]),
                    passphrase: b"s3cret".to_vec(),
                },
            )
            .expect("authenticated session");
        let payload = rt
            .dispatch(&request(
                &session.id,
                "FileSystem",
                "write_file",
                vec![
                    Value::Null,
                    Value::Text("secure".to_string()),
                    Value::Text("note.txt".to_string()),
                    Value::Bytes(b"remote".to_vec()),
                    Value::Uint(0),
                ],
            ))
            .payload;
        rt.shutdown();
        payload
    }

    #[test]
    fn remote_dispatch_uses_engine_acl_for_authenticated_files() {
        let denied_path = temp_store("files-acl-denied");
        seed_files_acl_fixture(&denied_path, false);
        match remote_write_file_payload(&denied_path) {
            ResponsePayload::Err(err) => assert_eq!(err.code, Code::PermissionDenied),
            other => panic!("write without an ACL grant must fail closed, got {other:?}"),
        }
        std::fs::remove_dir_all(&denied_path).ok();

        let allowed_path = temp_store("files-acl-allowed");
        seed_files_acl_fixture(&allowed_path, true);
        assert!(matches!(
            remote_write_file_payload(&allowed_path),
            ResponsePayload::Ok(Value::Null)
        ));
        std::fs::remove_dir_all(&allowed_path).ok();
    }

    fn seed_identity_audit_fixture(path: &std::path::Path) {
        let seed = LocalLoomClient::new(path);
        let engine = seed.open().expect("open seed");
        let root = WorkspaceId::from_bytes([1; 16]);
        let user = WorkspaceId::from_bytes([7; 16]);
        seed.with_session(&engine, |loom| {
            let mut identity = IdentityStore::new(root);
            identity.set_passphrase(root, "rootpw", b"root-salt-bytes")?;
            identity.add_principal(user, "user", PrincipalKind::User)?;
            loom.store().save_identity_store(&identity)
        })
        .expect("seed identity");
        seed.close(&engine);
    }

    fn decode_identity_audit_result(bytes: &[u8]) -> (u64, String, Option<String>) {
        let value = loom_codec::decode(bytes).expect("decode identity audit result");
        let Value::Array(fields) = value else {
            panic!("identity audit result is not an array");
        };
        let [seq, _id, action, target] = fields.as_slice() else {
            panic!("identity audit result has wrong arity");
        };
        let Value::Uint(seq) = seq else {
            panic!("identity audit seq is not an unsigned integer");
        };
        let Value::Text(action) = action else {
            panic!("identity audit action is not text");
        };
        let target = match target {
            Value::Null => None,
            Value::Text(value) => Some(value.clone()),
            _ => panic!("identity audit target is not text or null"),
        };
        (*seq, action.clone(), target)
    }

    #[test]
    fn remote_identity_audit_uses_authenticated_session_actor() {
        let root = WorkspaceId::from_bytes([1; 16]);
        let user = WorkspaceId::from_bytes([7; 16]);

        let local_path = temp_store("identity-audit-local");
        seed_identity_audit_fixture(&local_path);
        let local = LocalLoomClient::new(&local_path);
        let local_session = local.open().expect("open local");
        local
            .authenticate_passphrase(&local_session, root, b"rootpw")
            .expect("authenticate local root");
        let local_key = WorkspaceId::from_bytes([8; 16]);
        let local_result = local
            .identity_add_public_key(
                &local_session,
                user,
                IdentityPublicKeySpec {
                    id: local_key,
                    label: "laptop".to_string(),
                    algorithm: "Ed25519".to_string(),
                    public_key: vec![7u8; 32],
                },
            )
            .expect("local add public key");
        local.close(&local_session);
        let local_audit = FileStore::open_read(&local_path)
            .expect("open local audit")
            .audit_records()
            .expect("read local audit");
        assert_eq!(local_audit.len(), 1);
        assert_eq!(local_audit[0].seq, local_result.audit_seq);
        assert_eq!(local_audit[0].principal, Some(root));
        assert_eq!(local_audit[0].action, local_result.action);
        assert_eq!(local_audit[0].target, local_result.target);
        std::fs::remove_dir_all(&local_path).ok();

        let remote_path = temp_store("identity-audit-remote");
        seed_identity_audit_fixture(&remote_path);
        let rt = RemoteRuntime::start(&remote_path, config()).expect("start");
        let conn = rt.register_connection("peer");
        let session = rt
            .open_session(
                conn,
                RemoteAuth::Passphrase {
                    principal: root,
                    passphrase: b"rootpw".to_vec(),
                },
            )
            .expect("authenticated session");
        let remote_result = match rt
            .dispatch(&request(
                &session.id,
                "Identity",
                "identity_add_public_key",
                vec![
                    Value::Null,
                    Value::Bytes(user.as_bytes().to_vec()),
                    Value::Text("laptop".to_string()),
                    Value::Text("Ed25519".to_string()),
                    Value::Bytes(vec![7u8; 32]),
                ],
            ))
            .payload
        {
            ResponsePayload::Ok(Value::Bytes(bytes)) => decode_identity_audit_result(&bytes),
            other => panic!("remote add public key failed: {other:?}"),
        };
        rt.shutdown();
        let remote_audit = FileStore::open_read(&remote_path)
            .expect("open remote audit")
            .audit_records()
            .expect("read remote audit");
        assert_eq!(remote_audit.len(), 1);
        assert_eq!(remote_audit[0].seq, remote_result.0);
        assert_eq!(remote_audit[0].principal, Some(root));
        assert_eq!(remote_audit[0].action, remote_result.1);
        assert_eq!(remote_audit[0].target, remote_result.2);
        assert_eq!(remote_result.1, "identity.public_key.add");
        assert!(
            remote_result
                .2
                .as_deref()
                .is_some_and(|target| target.starts_with(&format!("principal={user};key=")))
        );
        std::fs::remove_dir_all(&remote_path).ok();
    }

    #[test]
    fn discovery_is_credential_free() {
        let path = temp_store("disco");
        let rt = RemoteRuntime::start(&path, config()).expect("start");
        let bytes = rt
            .discovery_response("/apps/loom/.well-known/loom")
            .expect("discovery route serves");
        let doc = Discovery::decode(&bytes).expect("decode");
        assert!(doc.is_compatible(1, 1));
        assert!(doc.auth.contains(&"interactive".to_string()));
        assert!(rt.discovery_response("/apps/loom/data").is_none());
        std::fs::remove_dir_all(&path).ok();
    }

    #[test]
    fn unary_dispatch_roundtrips_and_reports_unsupported() {
        let path = temp_store("dispatch");
        let rt = RemoteRuntime::start(&path, config()).expect("start");
        let conn = rt.register_connection("peer");
        let session = rt
            .open_session(conn, RemoteAuth::Unauthenticated)
            .expect("open");

        // Every generated method carries the IDL `LoomSession handle` as arg 0; the server decodes it and
        // substitutes its own resolved engine session, so a placeholder at index 0 suffices here.
        let handle = Value::Null;
        // KV keys cross the wire as canonical CBOR of the engine's typed cell.
        let key = loom_core::kv::key_to_cbor(&loom_core::tabular::Value::Text("k".to_string()));
        let put = request(
            &session.id,
            "Kv",
            "put",
            vec![
                handle.clone(),
                Value::Text("app".to_string()),
                Value::Text("c".to_string()),
                Value::Bytes(key.clone()),
                Value::Bytes(b"v".to_vec()),
            ],
        );
        assert!(matches!(rt.dispatch(&put).payload, ResponsePayload::Ok(_)));

        let get = request(
            &session.id,
            "Kv",
            "get",
            vec![
                handle.clone(),
                Value::Text("app".to_string()),
                Value::Text("c".to_string()),
                Value::Bytes(key.clone()),
            ],
        );
        match rt.dispatch(&get).payload {
            ResponsePayload::Ok(Value::Bytes(bytes)) => assert_eq!(bytes, b"v"),
            other => panic!("expected bytes value, got {other:?}"),
        }

        let version = request(&session.id, "Store", "version", vec![]);
        assert!(matches!(
            rt.dispatch(&version).payload,
            ResponsePayload::Ok(Value::Text(_))
        ));

        // A non-representative IDL method dispatches through the generated server dispatch rather than
        // reporting UNSUPPORTED.
        let listed = request(
            &session.id,
            "Kv",
            "list",
            vec![
                handle.clone(),
                Value::Text("app".to_string()),
                Value::Text("c".to_string()),
            ],
        );
        assert!(matches!(
            rt.dispatch(&listed).payload,
            ResponsePayload::Ok(_)
        ));

        // An unknown method reports NOT_FOUND.
        let unknown = request(&session.id, "Kv", "nope", vec![]);
        match rt.dispatch(&unknown).payload {
            ResponsePayload::Err(err) => assert_eq!(err.code, Code::NotFound),
            other => panic!("expected not-found error, got {other:?}"),
        }
        std::fs::remove_dir_all(&path).ok();
    }

    #[test]
    fn program_dispatch_binds_authenticated_principal_and_rechecks_acl() {
        let path = temp_store("program-auth");
        let seed = LocalLoomClient::new(&path);
        let engine = seed.open().expect("open seed");
        let root = WorkspaceId::from_bytes([1; 16]);
        let user = WorkspaceId::from_bytes([7; 16]);
        seed.with_session(&engine, |loom| {
            let mut identity = IdentityStore::new(root);
            identity.add_principal(user, "user", PrincipalKind::User)?;
            identity.set_passphrase(user, "s3cret", b"salt-bytes")?;
            loom.store().save_identity_store(&identity)?;
            loom.set_identity_store(identity);
            Ok(())
        })
        .expect("seed identity");

        let source = "Hello, {{ name }}";
        let manifest = Manifest::for_template("page-card", source, GrantSet::default());
        let manifest_bytes = manifest.encode();
        let put_args = || {
            vec![
                Value::Null,
                Value::Text("app".to_string()),
                Value::Text("page-card".to_string()),
                Value::Bytes(manifest_bytes.clone()),
                Value::Bytes(source.as_bytes().to_vec()),
            ]
        };
        let inspect_args = || {
            vec![
                Value::Null,
                Value::Text("app".to_string()),
                Value::Text("page-card".to_string()),
            ]
        };

        let grant = AclGrant {
            subject: AclSubject::Principal(user),
            workspace: None,
            domain: Some(FacetKind::Program.into()),
            ref_glob: None,
            scopes: vec![AclScope::All],
            rights: BTreeSet::from([AclRight::Read, AclRight::Write]),
            effect: AclEffect::Allow,
            predicate: None,
        };
        seed.acl_grant(&engine, grant.clone()).expect("grant");
        seed.close(&engine);

        let rt = RemoteRuntime::start(&path, config()).expect("start");
        let conn = rt.register_connection("peer");
        let unauth = rt
            .open_session(conn, RemoteAuth::Unauthenticated)
            .expect("open unauth");
        match rt
            .dispatch(&request(&unauth.id, "Program", "program_put", put_args()))
            .payload
        {
            ResponsePayload::Err(err) => {
                assert!(matches!(
                    err.code,
                    Code::AuthenticationFailed | Code::PermissionDenied
                ))
            }
            other => panic!("unauthenticated Program put must fail, got {other:?}"),
        }

        let authed = rt
            .open_session(
                conn,
                RemoteAuth::Passphrase {
                    principal: user,
                    passphrase: b"s3cret".to_vec(),
                },
            )
            .expect("open authenticated");
        match rt
            .dispatch(&request(&authed.id, "Program", "program_put", put_args()))
            .payload
        {
            ResponsePayload::Ok(Value::Bytes(record)) => {
                let value = loom_codec::decode(&record).expect("decode record");
                let Value::Array(fields) = value else {
                    panic!("program record must be an array");
                };
                assert_eq!(fields[0], Value::Text("page-card".to_string()));
            }
            other => panic!("authenticated Program put must succeed, got {other:?}"),
        }
        assert!(matches!(
            rt.dispatch(&request(
                &authed.id,
                "Program",
                "program_inspect",
                inspect_args()
            ))
            .payload,
            ResponsePayload::Ok(Value::Bytes(_))
        ));

        let revoke = LocalLoomClient::new(&path);
        let revoke_engine = revoke.open().expect("open revoke");
        revoke
            .acl_revoke(&revoke_engine, &grant)
            .expect("revoke grant");
        revoke.close(&revoke_engine);

        match rt
            .dispatch(&request(
                &authed.id,
                "Program",
                "program_inspect",
                inspect_args(),
            ))
            .payload
        {
            ResponsePayload::Err(err) => assert_eq!(err.code, Code::PermissionDenied),
            other => panic!("revoked Program inspect must fail, got {other:?}"),
        }
        std::fs::remove_dir_all(&path).ok();
    }

    #[test]
    fn logs_dispatch_binds_authenticated_principal_and_rechecks_acl() {
        let path = temp_store("logs-auth");
        let seed = LocalLoomClient::new(&path);
        let engine = seed.open().expect("open seed");
        let root = WorkspaceId::from_bytes([1; 16]);
        let user = WorkspaceId::from_bytes([7; 16]);
        seed.with_session(&engine, |loom| {
            let mut identity = IdentityStore::new(root);
            identity.add_principal(user, "user", PrincipalKind::User)?;
            identity.set_passphrase(user, "s3cret", b"salt-bytes")?;
            loom.store().save_identity_store(&identity)?;
            loom.set_identity_store(identity);
            Ok(())
        })
        .expect("seed identity");

        let record = LogRecord::new(
            100,
            Some(110),
            LogSeverityNumber::new(13).expect("severity"),
            "WARN".to_string(),
            LogValue::String("cache miss".to_string()),
        )
        .expect("record")
        .with_context(
            BTreeMap::from([("cache.hit".to_string(), LogValue::Bool(false))]),
            BTreeMap::from([(
                "service.name".to_string(),
                LogValue::String("api".to_string()),
            )]),
            BTreeMap::from([("name".to_string(), LogValue::String("loom".to_string()))]),
            None,
        )
        .expect("record context");
        let record_bytes = record.encode().expect("encode record");
        let put_args = || {
            vec![
                Value::Null,
                Value::Text("app".to_string()),
                Value::Bytes(record_bytes.clone()),
            ]
        };

        let grant = AclGrant {
            subject: AclSubject::Principal(user),
            workspace: None,
            domain: Some(FacetKind::Logs.into()),
            ref_glob: None,
            scopes: vec![AclScope::All],
            rights: BTreeSet::from([AclRight::Read, AclRight::Write]),
            effect: AclEffect::Allow,
            predicate: None,
        };
        seed.acl_grant(&engine, grant.clone()).expect("grant");
        seed.close(&engine);

        let rt = RemoteRuntime::start(&path, config()).expect("start");
        let conn = rt.register_connection("peer");
        let unauth = rt
            .open_session(conn, RemoteAuth::Unauthenticated)
            .expect("open unauth");
        match rt
            .dispatch(&request(&unauth.id, "Logs", "put_record", put_args()))
            .payload
        {
            ResponsePayload::Err(err) => {
                assert!(matches!(
                    err.code,
                    Code::AuthenticationFailed | Code::PermissionDenied
                ))
            }
            other => panic!("unauthenticated Logs put must fail, got {other:?}"),
        }

        let authed = rt
            .open_session(
                conn,
                RemoteAuth::Passphrase {
                    principal: user,
                    passphrase: b"s3cret".to_vec(),
                },
            )
            .expect("open authenticated");
        let record_id = match rt
            .dispatch(&request(&authed.id, "Logs", "put_record", put_args()))
            .payload
        {
            ResponsePayload::Ok(Value::Text(record_id)) => record_id,
            other => panic!("authenticated Logs put must succeed, got {other:?}"),
        };
        let get_args = || {
            vec![
                Value::Null,
                Value::Text("app".to_string()),
                Value::Text(record_id.clone()),
            ]
        };
        assert!(matches!(
            rt.dispatch(&request(&authed.id, "Logs", "get_record", get_args()))
                .payload,
            ResponsePayload::Ok(Value::Bytes(_))
        ));

        let revoke = LocalLoomClient::new(&path);
        let revoke_engine = revoke.open().expect("open revoke");
        revoke
            .acl_revoke(&revoke_engine, &grant)
            .expect("revoke grant");
        revoke.close(&revoke_engine);

        match rt
            .dispatch(&request(&authed.id, "Logs", "get_record", get_args()))
            .payload
        {
            ResponsePayload::Err(err) => assert_eq!(err.code, Code::PermissionDenied),
            other => panic!("revoked Logs get must fail, got {other:?}"),
        }
        std::fs::remove_dir_all(&path).ok();
    }

    #[test]
    fn every_generated_method_is_dispatchable_or_daemon_handled() {
        let path = temp_store("coverage");
        let seed = LocalLoomClient::new(&path);
        seed.create().expect("create store");
        let engine = seed.open().expect("open engine");
        // Every method in the generated registry must be recognized by the generated dispatch (or the
        // runtime-owned Store lifecycle): none may fall through to the generic "unknown method" arm.
        for sig in METHODS {
            let outcome =
                crate::generated_dispatch::dispatch(&seed, &engine, sig.interface, sig.method, &[]);
            if let Err(err) = outcome {
                assert!(
                    !(err.code == Code::NotFound && err.message.contains("unknown method")),
                    "generated dispatch has no arm for {}.{}",
                    sig.interface,
                    sig.method
                );
            }
        }
        seed.close(&engine);
        std::fs::remove_dir_all(&path).ok();
    }

    #[test]
    fn generated_dispatch_tracks_file_handle_lifecycle() {
        let path = temp_store("file-handle");
        let rt = RemoteRuntime::start(&path, config()).expect("start");
        let conn = rt.register_connection("peer");
        let session = rt
            .open_session(conn, RemoteAuth::Unauthenticated)
            .expect("open");

        // FileHandle.open(handle, workspace, path, mode) -> u64; mode is the one-byte OpenMode tag
        // (2 = ReadWrite). arg 0 is the LoomSession handle placeholder.
        let open = rt.dispatch(&request(
            &session.id,
            "FileHandle",
            "open",
            vec![
                Value::Null,
                Value::Text("app".to_string()),
                Value::Text("f.txt".to_string()),
                Value::Bytes(vec![2]),
            ],
        ));
        let ResponsePayload::Ok(Value::Uint(file)) = open.payload else {
            panic!("file open failed: {:?}", open.payload);
        };
        assert_eq!(rt.handle_count(), 1, "file handle registered");
        assert!(rt.managed_handle(HandleKind::File, file).is_some());

        // Write then read the same bytes back through the validated file handle (offset-addressed so the
        // read does not depend on the post-write cursor position).
        let write = rt.dispatch(&request(
            &session.id,
            "FileHandle",
            "write_at",
            vec![
                Value::Null,
                Value::Uint(file),
                Value::Uint(0),
                Value::Bytes(b"hi".to_vec()),
            ],
        ));
        assert!(
            matches!(write.payload, ResponsePayload::Ok(_)),
            "write_at: {:?}",
            write.payload
        );
        let read = rt.dispatch(&request(
            &session.id,
            "FileHandle",
            "read_at",
            vec![
                Value::Null,
                Value::Uint(file),
                Value::Uint(0),
                Value::Uint(2),
            ],
        ));
        match read.payload {
            ResponsePayload::Ok(Value::Bytes(bytes)) => assert_eq!(bytes, b"hi"),
            other => panic!("expected read bytes, got {other:?}"),
        }

        // Cross-session ownership: a second live protocol session on the same store must not consume the
        // first session's file handle. The runtime holds no exclusive per-session writer lock, so this is
        // exercised through a real concurrent session driving a dispatch.
        let other = rt
            .open_session(conn, RemoteAuth::Unauthenticated)
            .expect("second session");
        let foreign_read = rt.dispatch(&request(
            &other.id,
            "FileHandle",
            "read_at",
            vec![
                Value::Null,
                Value::Uint(file),
                Value::Uint(0),
                Value::Uint(2),
            ],
        ));
        match foreign_read.payload {
            ResponsePayload::Err(err) => assert_eq!(err.code, Code::PermissionDenied),
            other => {
                panic!("a foreign session must not use another session's handle, got {other:?}")
            }
        }
        assert!(rt.close_session(&other.id));

        // close frees the file handle (registry record removed, engine handle closed).
        let close = rt.dispatch(&request(
            &session.id,
            "FileHandle",
            "close",
            vec![Value::Null, Value::Uint(file)],
        ));
        assert!(matches!(close.payload, ResponsePayload::Ok(_)));
        assert_eq!(rt.handle_count(), 0, "file handle freed");

        // A freed handle is rejected before reaching the engine.
        let stale = rt.dispatch(&request(
            &session.id,
            "FileHandle",
            "read_at",
            vec![
                Value::Null,
                Value::Uint(file),
                Value::Uint(0),
                Value::Uint(2),
            ],
        ));
        match stale.payload {
            ResponsePayload::Err(err) => assert_eq!(err.code, Code::NotFound),
            other => panic!("expected not-found for freed handle, got {other:?}"),
        }

        rt.shutdown();
        std::fs::remove_dir_all(&path).ok();
    }

    #[test]
    fn session_close_frees_registered_handles() {
        let path = temp_store("handle-drop");
        let rt = RemoteRuntime::start(&path, config()).expect("start");
        let conn = rt.register_connection("peer");
        let session = rt
            .open_session(conn, RemoteAuth::Unauthenticated)
            .expect("open");

        // Open a file handle and leave it open.
        let open = rt.dispatch(&request(
            &session.id,
            "FileHandle",
            "open",
            vec![
                Value::Null,
                Value::Text("app".to_string()),
                Value::Text("f.txt".to_string()),
                Value::Bytes(vec![2]),
            ],
        ));
        assert!(matches!(open.payload, ResponsePayload::Ok(Value::Uint(_))));
        assert_eq!(rt.handle_count(), 1);

        // Closing the session reclaims the registered handle and its underlying engine handle.
        assert!(rt.close_session(&session.id));
        assert_eq!(rt.handle_count(), 0, "session close reclaimed the handle");

        rt.shutdown();
        std::fs::remove_dir_all(&path).ok();
    }

    #[test]
    fn streaming_backpressure_completion_and_disconnect_cleanup() {
        let path = temp_store("stream");
        let rt = RemoteRuntime::start(&path, config()).expect("start");
        let conn = rt.register_connection("peer");
        let session = rt
            .open_session(conn, RemoteAuth::Unauthenticated)
            .expect("open");

        let stream = rt
            .open_stream(&session.id, conn, 1, vec![b"a".to_vec(), b"b".to_vec()])
            .expect("open stream");
        assert!(rt.stream_owner_session(stream).is_some());
        match rt.stream_next(stream).expect("first") {
            Some(Frame::Item(item)) => assert_eq!(item, b"a"),
            other => panic!("expected first item, got {other:?}"),
        }
        assert!(
            rt.stream_next(stream).expect("credit exhausted").is_none(),
            "no credit means backpressure, not an item"
        );
        rt.grant_credit(stream, 4).expect("grant");
        match rt.stream_next(stream).expect("second") {
            Some(Frame::Item(item)) => assert_eq!(item, b"b"),
            other => panic!("expected second item, got {other:?}"),
        }
        assert!(matches!(
            rt.stream_next(stream).expect("complete"),
            Some(Frame::Complete)
        ));
        assert!(
            rt.stream_next(stream).is_err(),
            "a completed stream is closed"
        );

        // A dropped connection closes its streams immediately.
        let live = rt
            .open_stream(&session.id, conn, 10, vec![b"x".to_vec()])
            .expect("open");
        assert!(rt.cancel_stream(live) || rt.stream_count() == 0);
        let again = rt
            .open_stream(&session.id, conn, 10, vec![b"y".to_vec()])
            .expect("open");
        assert_eq!(rt.stream_count(), 1);
        assert_eq!(rt.drop_connection(conn), 1);
        assert_eq!(rt.stream_count(), 0);
        let _ = again;
        std::fs::remove_dir_all(&path).ok();
    }

    #[test]
    fn task_and_watch_registries_have_lifecycle() {
        let path = temp_store("handles");
        let rt = RemoteRuntime::start(&path, config()).expect("start");
        let conn = rt.register_connection("peer");
        let session = rt
            .open_session(conn, RemoteAuth::Unauthenticated)
            .expect("open");

        let task = rt.create_task(&session.id).expect("task");
        assert_eq!(rt.task_state(task), Some(TaskState::Running));
        rt.complete_task(task, b"done".to_vec()).expect("complete");
        assert_eq!(rt.task_state(task), Some(TaskState::Done));
        assert_eq!(rt.take_task_result(task).expect("take"), b"done");
        assert!(
            rt.take_task_result(task).is_err(),
            "task result is take-once"
        );
        assert!(rt.cancel_task(task));
        assert_eq!(rt.task_state(task), Some(TaskState::Cancelled));
        assert!(rt.task_owner_session(task).is_some());

        let watch = rt.open_watch(&session.id, "cursor-0").expect("watch");
        rt.advance_watch(watch, "cursor-1").expect("advance");
        assert_eq!(
            rt.watch_state(watch).map(|(_, cursor)| cursor),
            Some("cursor-1".to_string())
        );
        assert!(rt.close_watch(watch));
        assert!(!rt.close_watch(watch));
        std::fs::remove_dir_all(&path).ok();
    }

    /// Extract the `Ok` value of a unary dispatch, panicking with the error payload otherwise.
    fn ok_value(response: Response) -> Value {
        match response.payload {
            ResponsePayload::Ok(value) => value,
            other => panic!("expected ok, got {other:?}"),
        }
    }

    fn u64_field(value: &Value, label: &str) -> u64 {
        match value {
            Value::Uint(value) => *value,
            other => panic!("{label} is not an unsigned integer: {other:?}"),
        }
    }

    fn decode_lock_token(bytes: &[u8]) -> (String, String, String, Vec<u8>, u32, u32, u64, u64) {
        let value = loom_codec::decode(bytes).expect("decode lock token");
        let Value::Array(fields) = value else {
            panic!("lock token is not an array");
        };
        let [
            key,
            principal,
            session,
            mode,
            permits,
            capacity,
            authority,
            epoch,
            sequence,
            _deadline,
        ] = fields.as_slice()
        else {
            panic!("lock token has wrong arity");
        };
        let Value::Text(key) = key else {
            panic!("lock token key is not text");
        };
        let Value::Text(principal) = principal else {
            panic!("lock token principal is not text");
        };
        let Value::Text(session) = session else {
            panic!("lock token session is not text");
        };
        let mode = u64_field(mode, "mode");
        let permits = u64_field(permits, "permits");
        let capacity = u64_field(capacity, "capacity");
        let authority = u64_field(authority, "authority");
        let epoch = u64_field(epoch, "epoch");
        let sequence = u64_field(sequence, "sequence");
        assert!(authority <= u64::from(u32::MAX));
        assert!(epoch <= u64::from(u32::MAX));
        (
            key.clone(),
            principal.clone(),
            session.clone(),
            vec![u8::try_from(mode).expect("mode tag")],
            u32::try_from(permits).expect("permits"),
            u32::try_from(capacity).expect("capacity"),
            sequence,
            (authority << 32) | epoch,
        )
    }

    fn lock_acquire_request(session: &[u8], principal: &str, owner_session: &str) -> Request {
        request(
            session,
            "Locks",
            "lock_acquire",
            vec![
                Value::Text("resource".to_string()),
                Value::Text(principal.to_string()),
                Value::Text(owner_session.to_string()),
                Value::Bytes(vec![0]),
                Value::Uint(1),
                Value::Uint(1),
                Value::Uint(60_000),
                Value::Uint(0),
            ],
        )
    }

    #[test]
    fn remote_locks_use_store_coordinator_and_reject_stale_tokens() {
        let path = temp_store("locks");
        let rt = RemoteRuntime::start(&path, config()).expect("start");
        let conn = rt.register_connection("peer");
        let session = rt
            .open_session(conn, RemoteAuth::Unauthenticated)
            .expect("open");

        let first = match rt
            .dispatch(&lock_acquire_request(&session.id, "alice", "s1"))
            .payload
        {
            ResponsePayload::Ok(Value::Bytes(bytes)) => decode_lock_token(&bytes),
            other => panic!("first lock acquire failed: {other:?}"),
        };
        assert_eq!(first.0, "resource");
        assert_eq!(first.1, "alice");
        assert_eq!(first.2, "s1");
        assert_eq!(first.6, 1);

        match rt
            .dispatch(&lock_acquire_request(&session.id, "bob", "s2"))
            .payload
        {
            ResponsePayload::Err(err) => assert_eq!(err.code, Code::Locked),
            other => panic!("contended lock acquire must fail, got {other:?}"),
        }

        let refreshed = match rt
            .dispatch(&request(
                &session.id,
                "Locks",
                "lock_refresh",
                vec![
                    Value::Text(first.0.clone()),
                    Value::Text(first.1.clone()),
                    Value::Text(first.2.clone()),
                    Value::Bytes(first.3.clone()),
                    Value::Uint(u64::from(first.4)),
                    Value::Uint(u64::from(first.5)),
                    Value::Uint(first.6),
                    Value::Uint(first.7),
                    Value::Uint(60_000),
                ],
            ))
            .payload
        {
            ResponsePayload::Ok(Value::Bytes(bytes)) => decode_lock_token(&bytes),
            other => panic!("lock refresh failed: {other:?}"),
        };
        assert_eq!(refreshed.6, first.6);

        assert!(matches!(
            rt.dispatch(&request(
                &session.id,
                "Locks",
                "lock_release",
                vec![
                    Value::Text(refreshed.0.clone()),
                    Value::Text(refreshed.1.clone()),
                    Value::Text(refreshed.2.clone()),
                    Value::Bytes(refreshed.3.clone()),
                    Value::Uint(u64::from(refreshed.4)),
                    Value::Uint(u64::from(refreshed.5)),
                    Value::Uint(refreshed.6),
                    Value::Uint(refreshed.7),
                ],
            ))
            .payload,
            ResponsePayload::Ok(Value::Null)
        ));

        match rt
            .dispatch(&request(
                &session.id,
                "Locks",
                "lock_release",
                vec![
                    Value::Text(first.0),
                    Value::Text(first.1),
                    Value::Text(first.2),
                    Value::Bytes(first.3),
                    Value::Uint(u64::from(first.4)),
                    Value::Uint(u64::from(first.5)),
                    Value::Uint(first.6),
                    Value::Uint(first.7),
                ],
            ))
            .payload
        {
            ResponsePayload::Err(err) => assert_eq!(err.code, Code::LockNotHeld),
            other => panic!("stale lock token must fail, got {other:?}"),
        }

        let next = match rt
            .dispatch(&lock_acquire_request(&session.id, "bob", "s2"))
            .payload
        {
            ResponsePayload::Ok(Value::Bytes(bytes)) => decode_lock_token(&bytes),
            other => panic!("second lock acquire failed: {other:?}"),
        };
        assert_eq!(next.6, 2);
        std::fs::remove_dir_all(&path).ok();
    }

    #[test]
    fn generated_dispatch_runs_sql_session_lifecycle() {
        let path = temp_store("sql-session");
        let rt = RemoteRuntime::start(&path, config()).expect("start");
        let conn = rt.register_connection("peer");
        let session = rt
            .open_session(conn, RemoteAuth::Unauthenticated)
            .expect("open");

        // sql_open(loom_path, workspace, db) drops loom_path on the wire; the runtime binds its own store.
        let opened = rt.dispatch(&request(
            &session.id,
            "Sql",
            "sql_open",
            vec![
                Value::Text("app".to_string()),
                Value::Text("db".to_string()),
            ],
        ));
        let sql_session = ok_value(opened);
        let sql_id = handle_id_of(&sql_session).expect("sql session id");
        assert_eq!(rt.handle_count(), 1, "sql session registered");
        assert!(rt.managed_handle(HandleKind::SqlSession, sql_id).is_some());

        // Create + populate a table, then read it back, all through the generated server dispatch with no
        // runtime-held write lock to conflict with the SQL reopen-by-path.
        for stmt in [
            "CREATE TABLE t (x INTEGER)",
            "INSERT INTO t (x) VALUES (1)",
            "INSERT INTO t (x) VALUES (2)",
        ] {
            let exec = rt.dispatch(&request(
                &session.id,
                "Sql",
                "sql_exec",
                vec![sql_session.clone(), Value::Text(stmt.to_string())],
            ));
            assert!(
                matches!(exec.payload, ResponsePayload::Ok(_)),
                "sql_exec {stmt}: {:?}",
                exec.payload
            );
        }

        // sql_query is a streaming SELECT: the buffered dispatch yields one item frame per row.
        let rows = rt.dispatch_stream(&request(
            &session.id,
            "Sql",
            "sql_query",
            vec![
                sql_session.clone(),
                Value::Text("SELECT x FROM t ORDER BY x".to_string()),
            ],
        ));
        let items = rows
            .iter()
            .filter(|frame| matches!(Frame::decode(frame), Ok(Frame::Item(_))))
            .count();
        assert_eq!(items, 2, "two rows stream back through sql_query");

        // close frees the registered handle.
        let closed = rt.dispatch(&request(
            &session.id,
            "Sql",
            "sql_close",
            vec![sql_session.clone()],
        ));
        assert!(matches!(closed.payload, ResponsePayload::Ok(_)));
        assert_eq!(rt.handle_count(), 0, "sql session freed");

        // A stale sql session is rejected before dispatch.
        let stale = rt.dispatch(&request(
            &session.id,
            "Sql",
            "sql_exec",
            vec![sql_session, Value::Text("SELECT 1".to_string())],
        ));
        match stale.payload {
            ResponsePayload::Err(err) => assert_eq!(err.code, Code::NotFound),
            other => panic!("expected not-found for freed sql session, got {other:?}"),
        }

        rt.shutdown();
        std::fs::remove_dir_all(&path).ok();
    }

    #[test]
    fn generated_dispatch_runs_sql_batch_lifecycle() {
        let path = temp_store("sql-batch");
        let rt = RemoteRuntime::start(&path, config()).expect("start");
        let conn = rt.register_connection("peer");
        let session = rt
            .open_session(conn, RemoteAuth::Unauthenticated)
            .expect("open");

        let begun = rt.dispatch(&request(
            &session.id,
            "Sql",
            "sql_batch_begin",
            vec![
                Value::Text("app".to_string()),
                Value::Text("db".to_string()),
            ],
        ));
        let batch = ok_value(begun);
        let batch_id = handle_id_of(&batch).expect("batch id");
        assert_eq!(rt.handle_count(), 1, "sql batch registered");
        assert!(rt.managed_handle(HandleKind::SqlBatch, batch_id).is_some());

        for stmt in ["CREATE TABLE t (x INTEGER)", "INSERT INTO t (x) VALUES (7)"] {
            let exec = rt.dispatch(&request(
                &session.id,
                "Sql",
                "sql_batch_exec",
                vec![batch.clone(), Value::Text(stmt.to_string())],
            ));
            assert!(
                matches!(exec.payload, ResponsePayload::Ok(_)),
                "sql_batch_exec {stmt}: {:?}",
                exec.payload
            );
        }

        let committed = rt.dispatch(&request(
            &session.id,
            "Sql",
            "sql_batch_commit",
            vec![batch.clone()],
        ));
        assert!(matches!(committed.payload, ResponsePayload::Ok(_)));

        let closed = rt.dispatch(&request(&session.id, "Sql", "sql_batch_close", vec![batch]));
        assert!(matches!(closed.payload, ResponsePayload::Ok(_)));
        assert_eq!(rt.handle_count(), 0, "sql batch freed");

        // The committed row is durable: a fresh sql session reads it back.
        let opened = rt.dispatch(&request(
            &session.id,
            "Sql",
            "sql_open",
            vec![
                Value::Text("app".to_string()),
                Value::Text("db".to_string()),
            ],
        ));
        let sql_session = ok_value(opened);
        let rows = rt.dispatch_stream(&request(
            &session.id,
            "Sql",
            "sql_query",
            vec![
                sql_session.clone(),
                Value::Text("SELECT x FROM t".to_string()),
            ],
        ));
        let items = rows
            .iter()
            .filter(|frame| matches!(Frame::decode(frame), Ok(Frame::Item(_))))
            .count();
        assert_eq!(items, 1, "the batch-committed row is durable");
        rt.dispatch(&request(&session.id, "Sql", "sql_close", vec![sql_session]));

        rt.shutdown();
        std::fs::remove_dir_all(&path).ok();
    }

    #[test]
    fn generated_dispatch_runs_async_task_lifecycle() {
        let path = temp_store("async-task");
        let rt = RemoteRuntime::start(&path, config()).expect("start");
        let conn = rt.register_connection("peer");
        let session = rt
            .open_session(conn, RemoteAuth::Unauthenticated)
            .expect("open");

        // Seed a table via a sql session so the async exec has something to run against.
        let opened = rt.dispatch(&request(
            &session.id,
            "Sql",
            "sql_open",
            vec![
                Value::Text("app".to_string()),
                Value::Text("db".to_string()),
            ],
        ));
        let sql_session = ok_value(opened);
        rt.dispatch(&request(
            &session.id,
            "Sql",
            "sql_exec",
            vec![
                sql_session.clone(),
                Value::Text("CREATE TABLE t (x INTEGER)".to_string()),
            ],
        ));

        // sql_exec_async(SqlSession, sql) mints a Task; poll runs it, result takes the buffer, free drops
        // the handle. The SqlSession is path-bound, so no engine writer is pinned for the task.
        let spawned = rt.dispatch(&request(
            &session.id,
            "Tasks",
            "sql_exec_async",
            vec![
                sql_session.clone(),
                Value::Text("INSERT INTO t (x) VALUES (5)".to_string()),
            ],
        ));
        let task = ok_value(spawned);
        let task_id = handle_id_of(&task).expect("task id");
        assert!(rt.managed_handle(HandleKind::Task, task_id).is_some());

        let polled = rt.dispatch(&request(
            &session.id,
            "Tasks",
            "task_poll",
            vec![task.clone()],
        ));
        assert!(matches!(
            polled.payload,
            ResponsePayload::Ok(Value::Bool(_))
        ));
        let result = rt.dispatch(&request(
            &session.id,
            "Tasks",
            "task_result",
            vec![task.clone()],
        ));
        assert!(
            matches!(result.payload, ResponsePayload::Ok(_)),
            "task_result: {:?}",
            result.payload
        );

        let freed = rt.dispatch(&request(&session.id, "Tasks", "task_free", vec![task]));
        assert!(matches!(freed.payload, ResponsePayload::Ok(_)));
        assert!(
            rt.managed_handle(HandleKind::Task, task_id).is_none(),
            "task handle freed"
        );

        rt.dispatch(&request(&session.id, "Sql", "sql_close", vec![sql_session]));
        rt.shutdown();
        std::fs::remove_dir_all(&path).ok();
    }

    #[test]
    fn two_sessions_share_one_store_and_reject_cross_session_handles() {
        let path = temp_store("multi-session");
        let rt = RemoteRuntime::start(&path, config()).expect("start");
        let conn = rt.register_connection("peer");

        // Two protocol sessions open concurrently on the same bound store: neither holds an exclusive
        // writer lock, so a second `open_session` on the same store succeeds.
        let a = rt
            .open_session(conn, RemoteAuth::Unauthenticated)
            .expect("session a");
        let b = rt
            .open_session(conn, RemoteAuth::Unauthenticated)
            .expect("session b");
        assert_eq!(rt.session_count(), 2, "two live sessions on one store");

        // Both sessions execute engine writes (serialized by the runtime write authority), each through a
        // short-lived engine session.
        let key = loom_core::kv::key_to_cbor(&loom_core::tabular::Value::Text("k".to_string()));
        for session in [&a, &b] {
            let put = rt.dispatch(&request(
                &session.id,
                "Kv",
                "put",
                vec![
                    Value::Null,
                    Value::Text("app".to_string()),
                    Value::Text("c".to_string()),
                    Value::Bytes(key.clone()),
                    Value::Bytes(b"v".to_vec()),
                ],
            ));
            assert!(
                matches!(put.payload, ResponsePayload::Ok(_)),
                "put: {:?}",
                put.payload
            );
        }

        // A handle minted by session A must be rejected when session B tries to consume it.
        let opened = rt.dispatch(&request(
            &a.id,
            "Sql",
            "sql_open",
            vec![
                Value::Text("app".to_string()),
                Value::Text("db".to_string()),
            ],
        ));
        let sql_session = ok_value(opened);
        let cross = rt.dispatch(&request(
            &b.id,
            "Sql",
            "sql_exec",
            vec![sql_session.clone(), Value::Text("SELECT 1".to_string())],
        ));
        match cross.payload {
            ResponsePayload::Err(err) => assert_eq!(err.code, Code::PermissionDenied),
            other => panic!("cross-session handle must be permission-denied, got {other:?}"),
        }
        // Its owner still uses it.
        let own = rt.dispatch(&request(
            &a.id,
            "Sql",
            "sql_exec",
            vec![
                sql_session.clone(),
                Value::Text("CREATE TABLE t (x INTEGER)".to_string()),
            ],
        ));
        assert!(matches!(own.payload, ResponsePayload::Ok(_)));
        rt.dispatch(&request(&a.id, "Sql", "sql_close", vec![sql_session]));

        rt.shutdown();
        std::fs::remove_dir_all(&path).ok();
    }

    // Two connections/sessions on one bound store see each other's committed writes, and writes from both
    // serialize through the single writer authority. This is the read-side companion to
    // `two_sessions_share_one_store_and_reject_cross_session_handles` (which covers the write +
    // cross-session-handle-isolation side): protocol sessions do not take an exclusive writer lock, so
    // parallel readers on the shared store observe a consistent, up-to-date view.
    #[test]
    fn concurrent_sessions_see_each_others_committed_writes() {
        let path = temp_store("coord-reads");
        let rt = RemoteRuntime::start(&path, config()).expect("start");
        let conn = rt.register_connection("peer");
        let a = rt
            .open_session(conn, RemoteAuth::Unauthenticated)
            .expect("session a");
        let b = rt
            .open_session(conn, RemoteAuth::Unauthenticated)
            .expect("session b");

        let key =
            |k: &str| loom_core::kv::key_to_cbor(&loom_core::tabular::Value::Text(k.to_string()));
        let put = |sess: &[u8], k: &str, v: &[u8]| {
            request(
                sess,
                "Kv",
                "put",
                vec![
                    Value::Null,
                    Value::Text("app".to_string()),
                    Value::Text("c".to_string()),
                    Value::Bytes(key(k)),
                    Value::Bytes(v.to_vec()),
                ],
            )
        };
        let get = |sess: &[u8], k: &str| {
            request(
                sess,
                "Kv",
                "get",
                vec![
                    Value::Null,
                    Value::Text("app".to_string()),
                    Value::Text("c".to_string()),
                    Value::Bytes(key(k)),
                ],
            )
        };

        // Session A writes k1; session B reads A's committed write over the shared store.
        assert!(matches!(
            rt.dispatch(&put(&a.id, "k1", b"va")).payload,
            ResponsePayload::Ok(_)
        ));
        assert_eq!(
            ok_value(rt.dispatch(&get(&b.id, "k1"))),
            Value::Bytes(b"va".to_vec()),
            "session B reads session A's committed write"
        );
        // Session B writes k2; session A reads it. Writes from both connections serialize through the single
        // writer authority (each runs on a short-lived engine session; no cross-session lock is held).
        assert!(matches!(
            rt.dispatch(&put(&b.id, "k2", b"vb")).payload,
            ResponsePayload::Ok(_)
        ));
        assert_eq!(
            ok_value(rt.dispatch(&get(&a.id, "k2"))),
            Value::Bytes(b"vb".to_vec()),
            "session A reads session B's committed write"
        );

        rt.shutdown();
        std::fs::remove_dir_all(&path).ok();
    }

    // Draining rejects new sessions and new streams, but already-open sessions keep serving unary calls so
    // in-flight work runs down (the shutdown-drain contract). `starts_opens_a_session_and_shuts_down` covers
    // the new-session rejection; this adds the "existing session still served" and "new stream rejected"
    // halves.
    #[test]
    fn drain_rejects_new_sessions_and_streams_but_serves_existing() {
        let path = temp_store("coord-drain");
        let rt = RemoteRuntime::start(&path, config()).expect("start");
        let conn = rt.register_connection("peer");
        let a = rt
            .open_session(conn, RemoteAuth::Unauthenticated)
            .expect("session a");

        rt.drain();
        assert!(rt.is_draining(), "runtime reports draining");
        assert!(
            rt.open_session(conn, RemoteAuth::Unauthenticated).is_err(),
            "draining rejects new sessions"
        );
        assert!(
            rt.open_stream(&a.id, conn, 8, vec![b"x".to_vec()]).is_err(),
            "draining rejects new streams"
        );

        // The already-open session still serves a unary call while draining (existing work runs down).
        let key = loom_core::kv::key_to_cbor(&loom_core::tabular::Value::Text("k".to_string()));
        let put = rt.dispatch(&request(
            &a.id,
            "Kv",
            "put",
            vec![
                Value::Null,
                Value::Text("app".to_string()),
                Value::Text("c".to_string()),
                Value::Bytes(key),
                Value::Bytes(b"v".to_vec()),
            ],
        ));
        assert!(
            matches!(put.payload, ResponsePayload::Ok(_)),
            "existing session is served during drain: {:?}",
            put.payload
        );

        rt.shutdown();
        std::fs::remove_dir_all(&path).ok();
    }

    // Idempotency (specs/0067 §6): a keyed retry of a sequence-appending method replays the stored sequence
    // without appending a second entry — exactly-once. Queue.append is the visible-side-effect case (a
    // double-apply would leave two entries and advance the sequence).
    fn keyed_append(session: &[u8], stream: &str, entry: &[u8], key: &[u8]) -> Request {
        Request {
            request_id: vec![1],
            session_id: Some(session.to_vec()),
            interface: "Queue".to_string(),
            method: "append".to_string(),
            args: vec![
                Value::Null,
                Value::Text("jobs".to_string()),
                Value::Text(stream.to_string()),
                Value::Bytes(entry.to_vec()),
            ],
            deadline_ms: 0,
            idempotency_key: Some(key.to_vec()),
            principal_hint: None,
            compression: Compression::None,
            stream: false,
        }
    }

    #[test]
    fn idempotency_replays_a_keyed_append_without_reapplying() {
        let path = temp_store("idem-replay");
        let rt = RemoteRuntime::start(&path, config()).expect("start");
        let conn = rt.register_connection("peer");
        let s = rt
            .open_session(conn, RemoteAuth::Unauthenticated)
            .expect("session");

        // First append returns sequence 0; the identical keyed retry replays the same sequence.
        let first = ok_value(rt.dispatch(&keyed_append(&s.id, "in", b"a", b"k1")));
        assert_eq!(first, Value::Uint(0));
        let replay = ok_value(rt.dispatch(&keyed_append(&s.id, "in", b"a", b"k1")));
        assert_eq!(
            replay, first,
            "keyed retry replays the same terminal result"
        );

        // Exactly-once: the stream holds a single entry (no double-apply).
        let len = ok_value(rt.dispatch(&request(
            &s.id,
            "Queue",
            "len",
            vec![
                Value::Null,
                Value::Text("jobs".to_string()),
                Value::Text("in".to_string()),
            ],
        )));
        assert_eq!(len, Value::Uint(1), "keyed append applied exactly once");

        // A different key is a distinct effect: it appends and advances the sequence.
        let other = ok_value(rt.dispatch(&keyed_append(&s.id, "in", b"b", b"k2")));
        assert_eq!(other, Value::Uint(1), "a distinct key applies a new append");

        rt.shutdown();
        std::fs::remove_dir_all(&path).ok();
    }

    #[test]
    fn idempotency_rejects_same_key_with_a_different_request() {
        let path = temp_store("idem-conflict");
        let rt = RemoteRuntime::start(&path, config()).expect("start");
        let conn = rt.register_connection("peer");
        let s = rt
            .open_session(conn, RemoteAuth::Unauthenticated)
            .expect("session");

        assert!(matches!(
            rt.dispatch(&keyed_append(&s.id, "in", b"a", b"kx")).payload,
            ResponsePayload::Ok(_)
        ));
        // Same key, different payload (different fingerprint) is rejected per §6.
        match rt
            .dispatch(&keyed_append(&s.id, "in", b"DIFFERENT", b"kx"))
            .payload
        {
            ResponsePayload::Err(err) => assert_eq!(err.code, Code::Conflict),
            other => panic!("expected Conflict, got {other:?}"),
        }
        // The original effect applied exactly once and the conflict did not append.
        let len = ok_value(rt.dispatch(&request(
            &s.id,
            "Queue",
            "len",
            vec![
                Value::Null,
                Value::Text("jobs".to_string()),
                Value::Text("in".to_string()),
            ],
        )));
        assert_eq!(len, Value::Uint(1), "the rejected duplicate did not apply");

        rt.shutdown();
        std::fs::remove_dir_all(&path).ok();
    }

    #[test]
    fn idempotency_entries_are_dropped_on_session_close() {
        let path = temp_store("idem-cleanup");
        let rt = RemoteRuntime::start(&path, config()).expect("start");
        let conn = rt.register_connection("peer");
        let s = rt
            .open_session(conn, RemoteAuth::Unauthenticated)
            .expect("session");

        rt.dispatch(&keyed_append(&s.id, "in", b"a", b"k1"));
        assert_eq!(
            rt.idempotency.lock().expect("idem").len(),
            1,
            "one remembered idempotency entry"
        );
        assert!(rt.close_session(&s.id));
        assert_eq!(
            rt.idempotency.lock().expect("idem").len(),
            0,
            "session close drops its idempotency entries"
        );

        rt.shutdown();
        std::fs::remove_dir_all(&path).ok();
    }

    #[test]
    fn open_frame_stream_event_tail_is_lazy_and_unbounded() {
        let path = temp_store("event-tail");
        let rt = RemoteRuntime::start(&path, config()).expect("start");
        let conn = rt.register_connection("peer");
        let session = rt
            .open_session(conn, RemoteAuth::Unauthenticated)
            .expect("open");

        // The unbounded server-driven tail yields monotonic tick frames pulled one at a time; it is lazy
        // (a frame is produced only when pulled) so an infinite stream costs bounded memory.
        let mut stream =
            rt.open_frame_stream(&request(&session.id, "Diagnostics", "event_tail", vec![]));
        for expected in 0u64..6 {
            let encoded = stream.next_frame().expect("a tick frame");
            match Frame::decode(&encoded).expect("decode frame") {
                Frame::Item(bytes) => assert_eq!(
                    u64::from_be_bytes(bytes.try_into().expect("8-byte tick")),
                    expected
                ),
                other => panic!("expected an item frame, got {other:?}"),
            }
        }
        // The tail never completes on its own: the next pull still yields a frame.
        assert!(
            stream.next_frame().is_some(),
            "an unbounded tail keeps producing"
        );

        rt.shutdown();
        std::fs::remove_dir_all(&path).ok();
    }

    fn serve_options() -> RemoteServeOptions {
        RemoteServeOptions {
            bind: "127.0.0.1:9443".to_string(),
            service_root: "https://remote.host/apps/loom".to_string(),
            call_endpoint: "https://remote.host/apps/loom/v1/call".to_string(),
            auth_modes: vec![RemoteAuthMode::Interactive],
            tls: vec![RemoteTlsTrust::System],
            discovery: DiscoveryRoutes {
                mode: DiscoveryMode::Default,
                service_root_path: "/apps/loom".to_string(),
                custom_path: None,
            },
            session_lease_ms: 60_000,
            network_access_policy: None,
            max_request_bytes: 1_048_576,
        }
    }

    #[test]
    fn serve_options_from_cli_derive_endpoint_and_discovery() {
        // No explicit call endpoint: it defaults to `<service-root>/v1/call`, and the discovery routes
        // take the service-root URL path.
        let options = RemoteServeOptions::from_cli(
            "127.0.0.1:8443".to_string(),
            "https://host:8443/apps/loom".to_string(),
            None,
            vec![RemoteAuthMode::Interactive],
            vec![RemoteTlsTrust::System],
            60_000,
            4096,
            None,
        );
        assert_eq!(options.call_endpoint, "https://host:8443/apps/loom/v1/call");
        assert_eq!(options.call_path(), "/apps/loom/v1/call");
        assert_eq!(options.discovery.service_root_path, "/apps/loom");
        options.validate().expect("valid options");

        // An explicit call endpoint is honored and drives the routed call path.
        let custom = RemoteServeOptions::from_cli(
            "127.0.0.1:8443".to_string(),
            "https://host/base".to_string(),
            Some("https://host/base/rpc".to_string()),
            vec![RemoteAuthMode::Principal],
            vec![RemoteTlsTrust::InsecureDev],
            60_000,
            4096,
            None,
        );
        assert_eq!(custom.call_path(), "/base/rpc");
    }

    #[test]
    fn serve_options_validate_bind_and_endpoint() {
        serve_options().validate().expect("valid options");
        assert_eq!(
            parse_bind("127.0.0.1:9443").unwrap(),
            ("127.0.0.1".to_string(), 9443)
        );

        let mut no_port = serve_options();
        no_port.bind = "no-port".to_string();
        assert!(no_port.validate().is_err());

        let mut zero_port = serve_options();
        zero_port.bind = "host:0".to_string();
        assert!(zero_port.validate().is_err());

        let mut zero_limit = serve_options();
        zero_limit.max_request_bytes = 0;
        assert!(zero_limit.validate().is_err());

        // A public plain-HTTP endpoint is rejected; a loopback one is allowed.
        let mut public_http = serve_options();
        public_http.call_endpoint = "http://public.example/call".to_string();
        assert!(public_http.validate().is_err());
        let mut loopback_http = serve_options();
        loopback_http.call_endpoint = "http://127.0.0.1:9443/call".to_string();
        loopback_http.validate().expect("loopback http is allowed");
    }

    #[test]
    fn serve_options_build_a_running_runtime() {
        let path = temp_store("serve");
        let rt = RemoteRuntime::start(&path, serve_options().to_config()).expect("start");
        assert!(
            rt.discovery_response("/apps/loom/.well-known/loom")
                .is_some()
        );
        rt.shutdown();
        std::fs::remove_dir_all(&path).ok();
    }

    #[test]
    fn remote_listener_record_round_trips_durably() {
        let path = temp_store("listener");
        let record = serve_options().listener_record().expect("record");
        assert_eq!(record.surface, REMOTE_SURFACE);
        assert_eq!(record.transport, REMOTE_TRANSPORT);
        assert!(record.enabled);

        let store = FileStore::open(&path).expect("open store");
        store
            .save_served_listener_audited(&record, None, "configure", Some(&record.id))
            .expect("configure");
        assert!(
            store
                .served_listeners()
                .expect("list")
                .iter()
                .any(|r| r.surface == REMOTE_SURFACE)
        );

        let mut disabled = record.clone();
        disabled.enabled = false;
        store
            .save_served_listener_audited(&disabled, None, "disable", Some(&record.id))
            .expect("disable");
        assert!(
            !store
                .served_listener(&record.id)
                .expect("get")
                .expect("present")
                .enabled
        );

        store
            .remove_served_listener_audited(&record.id, None, "remove", Some(&record.id))
            .expect("remove");
        assert!(
            store
                .served_listener(&record.id)
                .expect("get gone")
                .is_none()
        );
        std::fs::remove_dir_all(&path).ok();
    }
}
