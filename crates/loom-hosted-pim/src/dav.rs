use std::collections::HashMap;
use std::future::Future;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::Router;
use axum::body::{Body, Bytes, to_bytes};
use axum::extract::{DefaultBodyLimit, Path, Request, State};
use axum::http::header::{ALLOW, CONTENT_TYPE, WWW_AUTHENTICATE};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use base64::Engine as _;
use loom_core::{Code, Digest, FacetKind, LoomError, WorkspaceId, WsSelector, calendar, contacts};
use quick_xml::Reader as XmlReader;
use quick_xml::XmlVersion;
use quick_xml::encoding::Decoder as XmlDecoder;
use quick_xml::events::{BytesStart as XmlBytesStart, Event as XmlEvent};
use tokio::net::TcpListener;
#[cfg(feature = "tls")]
use tokio::net::TcpStream;

#[cfg(feature = "tls")]
use loom_hosted_core::HostedTlsConfig;
use loom_hosted_core::http::serve_router;
use loom_hosted_core::{HostedAuth, HostedAuthPolicy, HostedError, HostedHttpLimits, HostedKernel};

#[cfg(feature = "tls")]
struct DavTlsTcpListener {
    listener: TcpListener,
    acceptor: tokio_rustls::TlsAcceptor,
}

#[cfg(feature = "tls")]
impl DavTlsTcpListener {
    fn new(listener: TcpListener, tls: HostedTlsConfig) -> Self {
        Self {
            listener,
            acceptor: tls.acceptor(),
        }
    }
}

#[cfg(feature = "tls")]
impl axum::serve::Listener for DavTlsTcpListener {
    type Io = tokio_rustls::server::TlsStream<TcpStream>;
    type Addr = std::net::SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            match self.listener.accept().await {
                Ok((stream, addr)) => match self.acceptor.accept(stream).await {
                    Ok(stream) => return (stream, addr),
                    Err(_) => continue,
                },
                Err(e) if is_connection_error(&e) => continue,
                Err(_) => tokio::time::sleep(std::time::Duration::from_secs(1)).await,
            }
        }
    }

    fn local_addr(&self) -> std::io::Result<Self::Addr> {
        self.listener.local_addr()
    }
}

#[cfg(feature = "tls")]
fn is_connection_error(e: &std::io::Error) -> bool {
    matches!(
        e.kind(),
        std::io::ErrorKind::ConnectionRefused
            | std::io::ErrorKind::ConnectionAborted
            | std::io::ErrorKind::ConnectionReset
    )
}

#[derive(Clone)]
struct PimWebDavState {
    kernel: HostedKernel,
    workspace: String,
    request_size_limit: usize,
    auth_policy: HostedAuthPolicy,
    basic_auth_cache: Arc<Mutex<DavBasicAuthCache>>,
}

const DAV_BASIC_AUTH_CACHE_TTL: Duration = Duration::from_secs(300);
const DAV_BASIC_AUTH_CACHE_MAX_ENTRIES: usize = 128;

#[derive(Default)]
struct DavBasicAuthCache {
    entries: HashMap<DavBasicAuthCacheKey, DavBasicAuthCacheEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct DavBasicAuthCacheKey {
    username: String,
    password_digest: String,
}

#[derive(Clone)]
struct DavBasicAuthCacheEntry {
    principal: WorkspaceId,
    principal_name: String,
    expires_at: Instant,
}

impl DavBasicAuthCache {
    fn get(
        &mut self,
        username: &str,
        password: &str,
        now: Instant,
    ) -> Option<DavBasicAuthCacheEntry> {
        self.prune(now);
        self.entries
            .get(&dav_basic_auth_cache_key(username, password))
            .filter(|entry| entry.expires_at > now)
            .cloned()
    }

    fn insert(
        &mut self,
        username: &str,
        password: &str,
        principal: WorkspaceId,
        principal_name: String,
        now: Instant,
    ) {
        self.prune(now);
        if self.entries.len() >= DAV_BASIC_AUTH_CACHE_MAX_ENTRIES
            && let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.expires_at)
                .map(|(key, _)| key.clone())
        {
            self.entries.remove(&oldest);
        }
        self.entries.insert(
            dav_basic_auth_cache_key(username, password),
            DavBasicAuthCacheEntry {
                principal,
                principal_name,
                expires_at: now + DAV_BASIC_AUTH_CACHE_TTL,
            },
        );
    }

    fn prune(&mut self, now: Instant) {
        self.entries.retain(|_, entry| entry.expires_at > now);
    }
}

fn dav_basic_auth_cache_key(username: &str, password: &str) -> DavBasicAuthCacheKey {
    DavBasicAuthCacheKey {
        username: username.to_string(),
        password_digest: Digest::blake3(password.as_bytes()).to_string(),
    }
}

struct HostedHttpError(Box<Response>);

type HostedHttpResult<T> = Result<T, HostedHttpError>;

impl HostedHttpError {
    fn into_response(self) -> Response {
        *self.0
    }
}

impl From<Response> for HostedHttpError {
    fn from(response: Response) -> Self {
        Self(Box::new(response))
    }
}

fn hosted_auth(headers: &HeaderMap, policy: HostedAuthPolicy) -> HostedHttpResult<HostedAuth> {
    let principal = header(headers, "x-loom-principal")?;
    let passphrase = header(headers, "x-loom-passphrase")?;
    let session = header(headers, "x-loom-session")?.unwrap_or_else(|| "http".to_string());
    match (principal, passphrase) {
        (None, None) if policy == HostedAuthPolicy::OwnerOrPassphrase => {
            Ok(HostedAuth::unauthenticated())
        }
        (None, None) => Err(error_response(
            StatusCode::UNAUTHORIZED,
            Code::AuthenticationFailed,
            "hosted listener requires authentication",
        )
        .into()),
        (Some(principal), Some(passphrase)) => {
            let principal = WorkspaceId::parse(&principal)
                .map_err(|err| loom_error_response(StatusCode::BAD_REQUEST, err))?;
            Ok(HostedAuth::passphrase(principal, passphrase, session))
        }
        _ => Err(error_response(
            StatusCode::BAD_REQUEST,
            Code::InvalidArgument,
            "supply both x-loom-principal and x-loom-passphrase, or neither",
        )
        .into()),
    }
}
fn authorization_basic(headers: &HeaderMap) -> HostedHttpResult<Option<(String, String)>> {
    let Some(value) = header(headers, "authorization")? else {
        return Ok(None);
    };
    let Some((scheme, token)) = value.split_once(' ') else {
        return Ok(None);
    };
    if !scheme.eq_ignore_ascii_case("Basic") {
        return Ok(None);
    }
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(token)
        .map_err(|_| {
            HostedHttpError::from(error_response(
                StatusCode::BAD_REQUEST,
                Code::InvalidArgument,
                "invalid HTTP Basic credential encoding",
            ))
        })?;
    let decoded = String::from_utf8(decoded).map_err(|_| {
        HostedHttpError::from(error_response(
            StatusCode::BAD_REQUEST,
            Code::InvalidArgument,
            "HTTP Basic credentials must be UTF-8",
        ))
    })?;
    let Some((username, password)) = decoded.split_once(':') else {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            Code::InvalidArgument,
            "HTTP Basic credentials must contain a username and password",
        )
        .into());
    };
    if username.is_empty() || password.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            Code::InvalidArgument,
            "HTTP Basic username and password must be non-empty",
        )
        .into());
    }
    if username.chars().any(char::is_control) || password.chars().any(char::is_control) {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            Code::InvalidArgument,
            "HTTP Basic username and password must not contain control characters",
        )
        .into());
    }
    Ok(Some((username.to_string(), password.to_string())))
}

fn header(headers: &HeaderMap, name: &str) -> HostedHttpResult<Option<String>> {
    headers
        .get(name)
        .map(|value| {
            value.to_str().map(|value| value.to_string()).map_err(|_| {
                HostedHttpError::from(error_response(
                    StatusCode::BAD_REQUEST,
                    Code::InvalidArgument,
                    "invalid non-utf8 hosted auth header",
                ))
            })
        })
        .transpose()
}

fn loom_error_response(status: StatusCode, err: LoomError) -> Response {
    hosted_error_response(status, HostedError::from_error(err))
}

fn hosted_error_response(status: StatusCode, err: HostedError) -> Response {
    let body = format!(
        "{{\"code\":{},\"code_number\":{},\"message\":{}}}",
        json_string(err.code_name),
        err.code_number,
        json_string(&err.message)
    );
    json_response(status, &body)
}

fn error_response(status: StatusCode, code: Code, message: &str) -> Response {
    let body = format!(
        "{{\"code\":{},\"code_number\":{},\"message\":{}}}",
        json_string(code.as_str()),
        code.as_i32(),
        json_string(message)
    );
    json_response(status, &body)
}

fn basic_challenge_response(mut response: Response) -> Response {
    response.headers_mut().insert(
        WWW_AUTHENTICATE,
        HeaderValue::from_static("Basic realm=\"Uldren Loom DAV\", charset=\"UTF-8\""),
    );
    response
}

fn json_response(status: StatusCode, body: &str) -> Response {
    response(status, "application/json", Body::from(body.to_string()))
}

fn response(status: StatusCode, content_type: &'static str, body: Body) -> Response {
    (status, [(CONTENT_TYPE, content_type)], body).into_response()
}

fn json_string(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

fn hex_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

pub fn caldav_router(kernel: HostedKernel, workspace: impl Into<String>) -> Router {
    caldav_router_with_limit(kernel, workspace, 16 * 1024 * 1024)
}

pub fn caldav_router_with_limit(
    kernel: HostedKernel,
    workspace: impl Into<String>,
    request_size_limit: usize,
) -> Router {
    caldav_router_with_policy(
        kernel,
        workspace,
        request_size_limit,
        HostedAuthPolicy::OwnerOrPassphrase,
    )
}

pub fn caldav_router_with_policy(
    kernel: HostedKernel,
    workspace: impl Into<String>,
    request_size_limit: usize,
    auth_policy: HostedAuthPolicy,
) -> Router {
    caldav_router_with_policy_and_cache(
        kernel,
        workspace,
        request_size_limit,
        auth_policy,
        Arc::new(Mutex::new(DavBasicAuthCache::default())),
    )
}

fn caldav_router_with_policy_and_cache(
    kernel: HostedKernel,
    workspace: impl Into<String>,
    request_size_limit: usize,
    auth_policy: HostedAuthPolicy,
    basic_auth_cache: Arc<Mutex<DavBasicAuthCache>>,
) -> Router {
    let state = PimWebDavState {
        kernel,
        workspace: workspace.into(),
        request_size_limit,
        auth_policy,
        basic_auth_cache,
    };
    Router::new()
        .route("/.well-known/caldav", any(caldav_well_known))
        .route("/.well-known/caldav/", any(caldav_well_known))
        .route("/caldav", any(caldav_root))
        .route("/caldav/", any(caldav_root))
        .route("/caldav/{*path}", any(caldav_resource))
        .layer(DefaultBodyLimit::max(request_size_limit))
        .with_state(state)
}

pub fn carddav_router(kernel: HostedKernel, workspace: impl Into<String>) -> Router {
    carddav_router_with_limit(kernel, workspace, 16 * 1024 * 1024)
}

pub fn carddav_router_with_limit(
    kernel: HostedKernel,
    workspace: impl Into<String>,
    request_size_limit: usize,
) -> Router {
    carddav_router_with_policy(
        kernel,
        workspace,
        request_size_limit,
        HostedAuthPolicy::OwnerOrPassphrase,
    )
}

pub fn carddav_router_with_policy(
    kernel: HostedKernel,
    workspace: impl Into<String>,
    request_size_limit: usize,
    auth_policy: HostedAuthPolicy,
) -> Router {
    carddav_router_with_policy_and_cache(
        kernel,
        workspace,
        request_size_limit,
        auth_policy,
        Arc::new(Mutex::new(DavBasicAuthCache::default())),
    )
}

fn carddav_router_with_policy_and_cache(
    kernel: HostedKernel,
    workspace: impl Into<String>,
    request_size_limit: usize,
    auth_policy: HostedAuthPolicy,
    basic_auth_cache: Arc<Mutex<DavBasicAuthCache>>,
) -> Router {
    let state = PimWebDavState {
        kernel,
        workspace: workspace.into(),
        request_size_limit,
        auth_policy,
        basic_auth_cache,
    };
    Router::new()
        .route("/.well-known/carddav", any(carddav_well_known))
        .route("/.well-known/carddav/", any(carddav_well_known))
        .route("/carddav", any(carddav_root))
        .route("/carddav/", any(carddav_root))
        .route("/carddav/{*path}", any(carddav_resource))
        .layer(DefaultBodyLimit::max(request_size_limit))
        .with_state(state)
}

pub fn dav_router_with_policy(
    kernel: HostedKernel,
    caldav_workspace: Option<String>,
    carddav_workspace: Option<String>,
    request_size_limit: usize,
    auth_policy: HostedAuthPolicy,
) -> Router {
    let mut router = Router::new();
    let basic_auth_cache = Arc::new(Mutex::new(DavBasicAuthCache::default()));
    if let Some(workspace) = caldav_workspace {
        router = router.merge(caldav_router_with_policy_and_cache(
            kernel.clone(),
            workspace,
            request_size_limit,
            auth_policy,
            basic_auth_cache.clone(),
        ));
    }
    if let Some(workspace) = carddav_workspace {
        router = router.merge(carddav_router_with_policy_and_cache(
            kernel,
            workspace,
            request_size_limit,
            auth_policy,
            basic_auth_cache,
        ));
    }
    router
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HostedDavWorkspaces {
    pub caldav: Option<String>,
    pub carddav: Option<String>,
}

pub async fn serve_caldav_with_limits<S>(
    listener: TcpListener,
    kernel: HostedKernel,
    workspace: impl Into<String>,
    limits: HostedHttpLimits,
    auth_policy: HostedAuthPolicy,
    shutdown: S,
) -> std::io::Result<()>
where
    S: Future<Output = ()> + Send + 'static,
{
    serve_router(
        listener,
        caldav_router_with_policy(kernel, workspace, limits.request_size_limit, auth_policy),
        limits,
        shutdown,
    )
    .await
}

#[cfg(feature = "tls")]
pub async fn serve_caldav_tls_with_limits<S>(
    listener: TcpListener,
    tls: HostedTlsConfig,
    kernel: HostedKernel,
    workspace: impl Into<String>,
    limits: HostedHttpLimits,
    auth_policy: HostedAuthPolicy,
    shutdown: S,
) -> std::io::Result<()>
where
    S: Future<Output = ()> + Send + 'static,
{
    serve_router(
        DavTlsTcpListener::new(listener, tls),
        caldav_router_with_policy(kernel, workspace, limits.request_size_limit, auth_policy),
        limits,
        shutdown,
    )
    .await
}

pub async fn serve_carddav_with_limits<S>(
    listener: TcpListener,
    kernel: HostedKernel,
    workspace: impl Into<String>,
    limits: HostedHttpLimits,
    auth_policy: HostedAuthPolicy,
    shutdown: S,
) -> std::io::Result<()>
where
    S: Future<Output = ()> + Send + 'static,
{
    serve_router(
        listener,
        carddav_router_with_policy(kernel, workspace, limits.request_size_limit, auth_policy),
        limits,
        shutdown,
    )
    .await
}

#[cfg(feature = "tls")]
pub async fn serve_carddav_tls_with_limits<S>(
    listener: TcpListener,
    tls: HostedTlsConfig,
    kernel: HostedKernel,
    workspace: impl Into<String>,
    limits: HostedHttpLimits,
    auth_policy: HostedAuthPolicy,
    shutdown: S,
) -> std::io::Result<()>
where
    S: Future<Output = ()> + Send + 'static,
{
    serve_router(
        DavTlsTcpListener::new(listener, tls),
        carddav_router_with_policy(kernel, workspace, limits.request_size_limit, auth_policy),
        limits,
        shutdown,
    )
    .await
}

pub async fn serve_dav_with_limits<S>(
    listener: TcpListener,
    kernel: HostedKernel,
    workspaces: HostedDavWorkspaces,
    limits: HostedHttpLimits,
    auth_policy: HostedAuthPolicy,
    shutdown: S,
) -> std::io::Result<()>
where
    S: Future<Output = ()> + Send + 'static,
{
    serve_router(
        listener,
        dav_router_with_policy(
            kernel,
            workspaces.caldav,
            workspaces.carddav,
            limits.request_size_limit,
            auth_policy,
        ),
        limits,
        shutdown,
    )
    .await
}

#[cfg(feature = "tls")]
pub async fn serve_dav_tls_with_limits<S>(
    listener: TcpListener,
    tls: HostedTlsConfig,
    kernel: HostedKernel,
    workspaces: HostedDavWorkspaces,
    limits: HostedHttpLimits,
    auth_policy: HostedAuthPolicy,
    shutdown: S,
) -> std::io::Result<()>
where
    S: Future<Output = ()> + Send + 'static,
{
    serve_router(
        DavTlsTcpListener::new(listener, tls),
        dav_router_with_policy(
            kernel,
            workspaces.caldav,
            workspaces.carddav,
            limits.request_size_limit,
            auth_policy,
        ),
        limits,
        shutdown,
    )
    .await
}

async fn caldav_well_known() -> Response {
    (
        StatusCode::TEMPORARY_REDIRECT,
        [("location", "/caldav/")],
        Body::empty(),
    )
        .into_response()
}

async fn caldav_root(
    State(state): State<PimWebDavState>,
    headers: HeaderMap,
    req: Request,
) -> Response {
    caldav_dispatch(state, headers, "", req).await
}

async fn caldav_resource(
    State(state): State<PimWebDavState>,
    headers: HeaderMap,
    Path(path): Path<String>,
    req: Request,
) -> Response {
    caldav_dispatch(state, headers, &path, req).await
}

async fn caldav_dispatch(
    state: PimWebDavState,
    headers: HeaderMap,
    path: &str,
    req: Request,
) -> Response {
    caldav_dispatch_inner(state, headers, path, req).await
}

async fn caldav_dispatch_inner(
    state: PimWebDavState,
    headers: HeaderMap,
    path: &str,
    req: Request,
) -> Response {
    let dav_auth = match hosted_dav_auth(&state, &headers) {
        Ok(auth) => auth,
        Err(response) => return response.into_response(),
    };
    let auth = dav_auth.auth;
    let principal = dav_auth.principal;
    match req.method().as_str() {
        "OPTIONS" => webdav_empty_response(StatusCode::NO_CONTENT, "1, 2, 3, calendar-access"),
        "PROPFIND" => {
            let depth = match webdav_depth(&headers) {
                Ok(depth) => depth,
                Err(response) => return response.into_response(),
            };
            let body = match webdav_request_body(req, state.request_size_limit).await {
                Ok(body) => body,
                Err(response) => return response.into_response(),
            };
            if let Err(response) = webdav_propfind_request(&body) {
                return response.into_response();
            }
            caldav_propfind(&state, &auth, &principal, path, depth)
        }
        "PROPPATCH" => {
            let body = match webdav_request_body(req, state.request_size_limit).await {
                Ok(body) => body,
                Err(response) => return response.into_response(),
            };
            caldav_proppatch(&state, &auth, &principal, path, &body)
        }
        "MKCALENDAR" => {
            let body = match webdav_request_body(req, state.request_size_limit).await {
                Ok(body) => body,
                Err(response) => return response.into_response(),
            };
            caldav_mkcalendar(&state, &auth, &principal, path, &body)
        }
        "REPORT" => {
            let body = match webdav_request_body(req, state.request_size_limit).await {
                Ok(body) => body,
                Err(response) => return response.into_response(),
            };
            caldav_report(&state, &auth, &principal, path, &body)
        }
        "GET" => caldav_get(&state, &auth, &principal, path),
        "HEAD" => webdav_head_response(caldav_get(&state, &auth, &principal, path)),
        "PUT" => {
            let body = match to_bytes(req.into_body(), state.request_size_limit).await {
                Ok(body) => body,
                Err(_) => {
                    return error_response(
                        StatusCode::BAD_REQUEST,
                        Code::InvalidArgument,
                        "invalid CalDAV request body",
                    );
                }
            };
            caldav_put(&state, &auth, &principal, &headers, path, &body)
        }
        "DELETE" => caldav_delete(&state, &auth, &principal, &headers, path),
        "MOVE" => caldav_move(&state, &auth, &principal, &headers, path),
        _ => webdav_method_not_allowed(
            "OPTIONS, PROPFIND, PROPPATCH, MKCALENDAR, REPORT, GET, HEAD, PUT, DELETE, MOVE",
            "unsupported CalDAV method",
        ),
    }
}

fn caldav_propfind(
    state: &PimWebDavState,
    auth: &HostedAuth,
    principal: &str,
    path: &str,
    depth: WebDavDepth,
) -> Response {
    let segments = match webdav_segments(path) {
        Ok(segments) => segments,
        Err(()) => return webdav_invalid_path_response(),
    };
    match segments.as_slice() {
        [] => match calendar_collections(state, auth, principal) {
            Ok(collections) => {
                let mut responses = vec![caldav_home_response(principal)];
                if depth == WebDavDepth::One {
                    responses.extend(collections.into_iter().map(|collection| {
                        caldav_collection_response(
                            &format!("/caldav/{}/", url_segment(&collection.name)),
                            principal,
                            &collection.display_name,
                            &collection.components,
                            &collection.sync_token,
                        )
                    }));
                }
                webdav_multistatus(responses)
            }
            Err(err) => loom_error_response(StatusCode::FORBIDDEN, err),
        },
        [prefix, _principal] if prefix == "principals" => {
            webdav_multistatus(vec![caldav_principal_response(principal)])
        }
        [collection] => match calendar_collection(state, auth, principal, collection) {
            Ok(Some(collection_meta)) => match calendar_entries(state, auth, principal, collection)
            {
                Ok(entries) => {
                    let mut responses = vec![caldav_collection_response(
                        &format!("/caldav/{}/", url_segment(collection)),
                        principal,
                        &collection_meta.display_name,
                        &collection_meta.components,
                        &collection_meta.sync_token,
                    )];
                    if depth == WebDavDepth::One {
                        responses.extend(entries.into_iter().map(|entry| {
                            webdav_resource_response(
                                &format!(
                                    "/caldav/{}/{}.ics",
                                    url_segment(collection),
                                    url_segment(&entry.uid)
                                ),
                                "text/calendar; charset=utf-8",
                                &entry.etag,
                            )
                        }));
                    }
                    webdav_multistatus(responses)
                }
                Err(err) => loom_error_response(StatusCode::FORBIDDEN, err),
            },
            Ok(None) => error_response(
                StatusCode::NOT_FOUND,
                Code::NotFound,
                "calendar collection not found",
            ),
            Err(err) => loom_error_response(StatusCode::FORBIDDEN, err),
        },
        [collection, resource] => {
            let Some(uid) = resource.strip_suffix(".ics") else {
                return webdav_invalid_path_response();
            };
            match caldav_resource_body(state, auth, principal, collection, uid) {
                Ok(Some(resource)) => webdav_multistatus(vec![webdav_resource_response(
                    &format!(
                        "/caldav/{}/{}.ics",
                        url_segment(collection),
                        url_segment(&resource.uid)
                    ),
                    "text/calendar; charset=utf-8",
                    &resource.etag,
                )]),
                Ok(None) => error_response(
                    StatusCode::NOT_FOUND,
                    Code::NotFound,
                    "calendar resource not found",
                ),
                Err(err) => loom_error_response(StatusCode::FORBIDDEN, err),
            }
        }
        _ => error_response(
            StatusCode::BAD_REQUEST,
            Code::InvalidArgument,
            "CalDAV PROPFIND expects a collection path",
        ),
    }
}

fn caldav_report(
    state: &PimWebDavState,
    auth: &HostedAuth,
    principal: &str,
    path: &str,
    body: &[u8],
) -> Response {
    let report = match webdav_report_request(body) {
        Ok(report) => report,
        Err(response) => return response.into_response(),
    };
    match report {
        WebDavReportRequest::CalendarMultiget { hrefs } => {
            caldav_calendar_multiget(state, auth, principal, hrefs)
        }
        WebDavReportRequest::AddressbookMultiget { hrefs, .. } => {
            let message = if hrefs.is_empty() {
                "addressbook-multiget requires at least one href"
            } else {
                "addressbook-multiget is not a CalDAV REPORT"
            };
            error_response(StatusCode::BAD_REQUEST, Code::InvalidArgument, message)
        }
        WebDavReportRequest::AddressbookQuery(_) => error_response(
            StatusCode::BAD_REQUEST,
            Code::InvalidArgument,
            "addressbook-query is not a CalDAV REPORT",
        ),
        WebDavReportRequest::CalendarQuery(query) => {
            caldav_calendar_query(state, auth, principal, path, query)
        }
        WebDavReportRequest::SyncCollection { sync_token } => {
            caldav_sync_collection(state, auth, principal, path, sync_token)
        }
        WebDavReportRequest::Unsupported(name) => {
            let message = format!("unsupported CalDAV REPORT {name}");
            error_response(StatusCode::NOT_IMPLEMENTED, Code::Unsupported, &message)
        }
    }
}

fn caldav_calendar_multiget(
    state: &PimWebDavState,
    auth: &HostedAuth,
    principal: &str,
    hrefs: Vec<String>,
) -> Response {
    if hrefs.is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            Code::InvalidArgument,
            "calendar-multiget requires at least one href",
        );
    }
    let mut responses = Vec::with_capacity(hrefs.len());
    for href in hrefs {
        let (collection, uid) = match webdav_href_resource_path(&href, "caldav", ".ics") {
            Ok(Some(path)) => path,
            Ok(None) => {
                responses.push(webdav_not_found_response(&href));
                continue;
            }
            Err(()) => return webdav_invalid_path_response(),
        };
        match caldav_resource_body(state, auth, principal, &collection, &uid) {
            Ok(Some(resource)) => responses.push(caldav_resource_report_response(
                &href,
                &resource.etag,
                &resource.body,
            )),
            Ok(None) => responses.push(webdav_not_found_response(&href)),
            Err(err) => return loom_error_response(StatusCode::FORBIDDEN, err),
        }
    }
    webdav_multistatus(responses)
}

fn caldav_calendar_query(
    state: &PimWebDavState,
    auth: &HostedAuth,
    principal: &str,
    path: &str,
    query: CalDavCalendarQuery,
) -> Response {
    let segments = match webdav_segments(path) {
        Ok(segments) => segments,
        Err(()) => return webdav_invalid_path_response(),
    };
    let [collection] = segments.as_slice() else {
        return error_response(
            StatusCode::BAD_REQUEST,
            Code::InvalidArgument,
            "calendar-query expects a collection path",
        );
    };
    match caldav_query_resources(state, auth, principal, collection, &query) {
        Ok(resources) => webdav_multistatus(
            resources
                .into_iter()
                .map(|resource| {
                    caldav_resource_report_response(
                        &format!(
                            "/caldav/{}/{}.ics",
                            url_segment(collection),
                            url_segment(&resource.uid)
                        ),
                        &resource.etag,
                        &resource.body,
                    )
                })
                .collect(),
        ),
        Err(err) => loom_error_response(StatusCode::FORBIDDEN, err),
    }
}

fn caldav_sync_collection(
    state: &PimWebDavState,
    auth: &HostedAuth,
    principal: &str,
    path: &str,
    sync_token: Option<String>,
) -> Response {
    let segments = match webdav_segments(path) {
        Ok(segments) => segments,
        Err(()) => return webdav_invalid_path_response(),
    };
    let [collection] = segments.as_slice() else {
        return error_response(
            StatusCode::BAD_REQUEST,
            Code::InvalidArgument,
            "sync-collection expects a collection path",
        );
    };
    let requested_token = sync_token.filter(|token| !token.is_empty());
    match caldav_sync_resources(
        state,
        auth,
        principal,
        collection,
        requested_token.as_deref(),
    ) {
        Ok((current_token, rows)) => {
            let responses = rows
                .into_iter()
                .map(|row| match row {
                    CalDavSyncRow::Present(resource) => caldav_resource_report_response(
                        &format!(
                            "/caldav/{}/{}.ics",
                            url_segment(collection),
                            url_segment(&resource.uid)
                        ),
                        &resource.etag,
                        &resource.body,
                    ),
                    CalDavSyncRow::Removed(uid) => webdav_not_found_response(&format!(
                        "/caldav/{}/{}.ics",
                        url_segment(collection),
                        url_segment(&uid)
                    )),
                })
                .collect();
            webdav_sync_multistatus(responses, &current_token)
        }
        Err(err) if err.code == Code::CursorInvalid => {
            loom_error_response(StatusCode::CONFLICT, err)
        }
        Err(err) => loom_error_response(StatusCode::FORBIDDEN, err),
    }
}

fn caldav_mkcalendar(
    state: &PimWebDavState,
    auth: &HostedAuth,
    principal: &str,
    path: &str,
    body: &[u8],
) -> Response {
    let segments = match webdav_segments(path) {
        Ok(segments) => segments,
        Err(()) => return webdav_invalid_path_response(),
    };
    let [collection] = segments.as_slice() else {
        return error_response(
            StatusCode::BAD_REQUEST,
            Code::InvalidArgument,
            "CalDAV MKCALENDAR expects a collection path",
        );
    };
    let properties = match caldav_mkcalendar_properties(body) {
        Ok(properties) => properties,
        Err(response) => return response.into_response(),
    };
    match state.kernel.write(auth, |loom| {
        let ns = resolve_hosted_pim_workspace(loom, &state.workspace, FacetKind::Calendar)?;
        calendar::create_collection(
            loom,
            ns,
            principal,
            collection,
            &calendar::CollectionMeta {
                display_name: properties
                    .display_name
                    .unwrap_or_else(|| collection.to_string()),
                component_set: if properties.component_set.is_empty() {
                    vec![calendar::Component::Event, calendar::Component::Todo]
                } else {
                    properties.component_set
                },
            },
        )?;
        loom.commit(ns, "loom-hosted", "caldav mkcalendar", 0)?;
        Ok(())
    }) {
        Ok(()) => webdav_empty_response(StatusCode::CREATED, "1, 2, 3, calendar-access"),
        Err(err) => loom_error_response(StatusCode::FORBIDDEN, err),
    }
}

fn caldav_proppatch(
    state: &PimWebDavState,
    auth: &HostedAuth,
    principal: &str,
    path: &str,
    body: &[u8],
) -> Response {
    let segments = match webdav_segments(path) {
        Ok(segments) => segments,
        Err(()) => return webdav_invalid_path_response(),
    };
    let [collection] = segments.as_slice() else {
        return error_response(
            StatusCode::BAD_REQUEST,
            Code::InvalidArgument,
            "CalDAV PROPPATCH expects a collection path",
        );
    };
    let update = match webdav_property_update(body, WebDavPropertyProfile::Calendar) {
        Ok(update) => update,
        Err(response) => return response.into_response(),
    };
    match state.kernel.write(auth, |loom| {
        let ns = resolve_hosted_pim_workspace(loom, &state.workspace, FacetKind::Calendar)?;
        if let Some(display_name) = update.display_name.as_ref() {
            let mut meta = calendar::get_collection(loom, ns, principal, collection)?
                .ok_or_else(|| LoomError::not_found("calendar collection not found"))?;
            meta.display_name = display_name.clone();
            calendar::create_collection(loom, ns, principal, collection, &meta)?;
            loom.commit(ns, "loom-hosted", "caldav proppatch", 0)?;
        }
        Ok(())
    }) {
        Ok(()) => webdav_proppatch_response(
            &format!("/caldav/{}/", url_segment(collection)),
            update.properties,
        ),
        Err(err) => loom_error_response(StatusCode::FORBIDDEN, err),
    }
}

fn caldav_get(state: &PimWebDavState, auth: &HostedAuth, principal: &str, path: &str) -> Response {
    let (collection, uid) = match webdav_resource_path(path, ".ics") {
        Ok(Some(path)) => path,
        Ok(None) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                Code::InvalidArgument,
                "CalDAV GET expects /caldav/{collection}/{uid}.ics",
            );
        }
        Err(()) => return webdav_invalid_path_response(),
    };
    match caldav_resource_body(state, auth, principal, &collection, &uid) {
        Ok(Some(resource)) => Response::builder()
            .status(StatusCode::OK)
            .header(CONTENT_TYPE, "text/calendar; charset=utf-8")
            .header("etag", format!("\"{}\"", resource.etag))
            .body(Body::from(resource.body))
            .unwrap_or_else(|_| {
                response(
                    StatusCode::OK,
                    "text/calendar; charset=utf-8",
                    Body::empty(),
                )
            }),
        Ok(None) => error_response(
            StatusCode::NOT_FOUND,
            Code::NotFound,
            "calendar resource not found",
        ),
        Err(err) => loom_error_response(StatusCode::FORBIDDEN, err),
    }
}

fn caldav_put(
    state: &PimWebDavState,
    auth: &HostedAuth,
    principal: &str,
    headers: &HeaderMap,
    path: &str,
    body: &[u8],
) -> Response {
    let (collection, uid) = match webdav_resource_path(path, ".ics") {
        Ok(Some(path)) => path,
        Ok(None) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                Code::InvalidArgument,
                "CalDAV PUT expects /caldav/{collection}/{uid}.ics",
            );
        }
        Err(()) => return webdav_invalid_path_response(),
    };
    let ics = match std::str::from_utf8(body) {
        Ok(ics) => ics,
        Err(_) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                Code::InvalidArgument,
                "CalDAV resource body must be UTF-8 iCalendar",
            );
        }
    };
    let entry = match calendar::CalendarEntry::from_ics(ics) {
        Ok(entry) => entry,
        Err(err) => return loom_error_response(StatusCode::BAD_REQUEST, err),
    };
    if entry.uid != uid {
        return error_response(
            StatusCode::BAD_REQUEST,
            Code::InvalidArgument,
            "CalDAV resource UID must match the path",
        );
    }
    match state.kernel.write(auth, |loom| {
        let ns = resolve_hosted_pim_workspace(loom, &state.workspace, FacetKind::Calendar)?;
        let current = calendar_current_etag(loom, ns, principal, &collection, &uid)?;
        webdav_check_preconditions(headers, current.as_deref())?;
        let etag = calendar::put_ics(loom, ns, principal, &collection, ics)?;
        loom.commit(ns, "loom-hosted", "caldav put", 0)?;
        Ok(etag)
    }) {
        Ok(etag) => {
            let etag = etag.to_hex();
            Response::builder()
                .status(StatusCode::CREATED)
                .header(CONTENT_TYPE, "application/json")
                .header("etag", format!("\"{etag}\""))
                .body(Body::from(format!("{{\"etag\":{}}}", json_string(&etag))))
                .unwrap_or_else(|_| {
                    response(StatusCode::CREATED, "application/json", Body::empty())
                })
        }
        Err(err) => webdav_write_error_response(err),
    }
}

fn caldav_delete(
    state: &PimWebDavState,
    auth: &HostedAuth,
    principal: &str,
    headers: &HeaderMap,
    path: &str,
) -> Response {
    let segments = match webdav_segments(path) {
        Ok(segments) => segments,
        Err(()) => return webdav_invalid_path_response(),
    };
    if let [collection] = segments.as_slice() {
        return match state.kernel.write(auth, |loom| {
            let ns = resolve_hosted_pim_workspace(loom, &state.workspace, FacetKind::Calendar)?;
            let deleted = calendar::delete_collection(loom, ns, principal, collection)?;
            if deleted {
                loom.commit(ns, "loom-hosted", "caldav delete collection", 0)?;
            }
            Ok(deleted)
        }) {
            Ok(true) => webdav_empty_response(StatusCode::NO_CONTENT, "1, 2, 3, calendar-access"),
            Ok(false) => error_response(
                StatusCode::NOT_FOUND,
                Code::NotFound,
                "calendar collection not found",
            ),
            Err(err) => webdav_write_error_response(err),
        };
    }
    let (collection, uid) = match webdav_resource_path(path, ".ics") {
        Ok(Some(path)) => path,
        Ok(None) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                Code::InvalidArgument,
                "CalDAV DELETE expects /caldav/{collection}/{uid}.ics",
            );
        }
        Err(()) => return webdav_invalid_path_response(),
    };
    match state.kernel.write(auth, |loom| {
        let ns = resolve_hosted_pim_workspace(loom, &state.workspace, FacetKind::Calendar)?;
        let current = calendar_current_etag(loom, ns, principal, &collection, &uid)?;
        webdav_check_preconditions(headers, current.as_deref())?;
        let deleted = calendar::delete_entry(loom, ns, principal, &collection, &uid)?;
        if deleted {
            loom.commit(ns, "loom-hosted", "caldav delete", 0)?;
        }
        Ok(deleted)
    }) {
        Ok(true) => webdav_empty_response(StatusCode::NO_CONTENT, "1, 2, 3, calendar-access"),
        Ok(false) => error_response(
            StatusCode::NOT_FOUND,
            Code::NotFound,
            "calendar resource not found",
        ),
        Err(err) => webdav_write_error_response(err),
    }
}

fn caldav_move(
    state: &PimWebDavState,
    auth: &HostedAuth,
    principal: &str,
    headers: &HeaderMap,
    path: &str,
) -> Response {
    let (source_collection, source_uid) = match webdav_resource_path(path, ".ics") {
        Ok(Some(path)) => path,
        Ok(None) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                Code::InvalidArgument,
                "CalDAV MOVE expects /caldav/{collection}/{uid}.ics",
            );
        }
        Err(()) => return webdav_invalid_path_response(),
    };
    let destination = match webdav_destination_resource_path(headers, "caldav", ".ics") {
        Ok(destination) => destination,
        Err(response) => return response.into_response(),
    };
    let (destination_collection, destination_uid) = destination;
    if source_uid != destination_uid {
        return error_response(
            StatusCode::BAD_REQUEST,
            Code::InvalidArgument,
            "CalDAV MOVE destination UID must match the source resource UID",
        );
    }
    if source_collection == destination_collection {
        return webdav_empty_response(StatusCode::NO_CONTENT, "1, 2, 3, calendar-access");
    }
    let overwrite = match webdav_overwrite(headers) {
        Ok(overwrite) => overwrite,
        Err(response) => return response.into_response(),
    };
    match state.kernel.write(auth, |loom| {
        let ns = resolve_hosted_pim_workspace(loom, &state.workspace, FacetKind::Calendar)?;
        let source_current =
            calendar_current_etag(loom, ns, principal, &source_collection, &source_uid)?;
        webdav_check_preconditions(headers, source_current.as_deref())?;
        let destination_current = calendar_current_etag(
            loom,
            ns,
            principal,
            &destination_collection,
            &destination_uid,
        )?;
        if !overwrite && destination_current.is_some() {
            return Err(LoomError::new(
                Code::CasMismatch,
                "WebDAV Overwrite precondition failed",
            ));
        }
        let entry = calendar::get_entry(loom, ns, principal, &source_collection, &source_uid)?
            .ok_or_else(|| LoomError::not_found("calendar resource not found"))?;
        calendar::put_entry(loom, ns, principal, &destination_collection, &entry)?;
        calendar::delete_entry(loom, ns, principal, &source_collection, &source_uid)?;
        loom.commit(ns, "loom-hosted", "caldav move", 0)?;
        Ok(destination_current.is_some())
    }) {
        Ok(true) => webdav_empty_response(StatusCode::NO_CONTENT, "1, 2, 3, calendar-access"),
        Ok(false) => webdav_empty_response(StatusCode::CREATED, "1, 2, 3, calendar-access"),
        Err(err) => webdav_write_error_response(err),
    }
}

async fn carddav_well_known() -> Response {
    (
        StatusCode::TEMPORARY_REDIRECT,
        [("location", "/carddav/")],
        Body::empty(),
    )
        .into_response()
}

async fn carddav_root(
    State(state): State<PimWebDavState>,
    headers: HeaderMap,
    req: Request,
) -> Response {
    carddav_dispatch(state, headers, "", req).await
}

async fn carddav_resource(
    State(state): State<PimWebDavState>,
    headers: HeaderMap,
    Path(path): Path<String>,
    req: Request,
) -> Response {
    carddav_dispatch(state, headers, &path, req).await
}

async fn carddav_dispatch(
    state: PimWebDavState,
    headers: HeaderMap,
    path: &str,
    req: Request,
) -> Response {
    carddav_dispatch_inner(state, headers, path, req).await
}

async fn carddav_dispatch_inner(
    state: PimWebDavState,
    headers: HeaderMap,
    path: &str,
    req: Request,
) -> Response {
    let dav_auth = match hosted_dav_auth(&state, &headers) {
        Ok(auth) => auth,
        Err(response) => return response.into_response(),
    };
    let auth = dav_auth.auth;
    let principal = dav_auth.principal;
    match req.method().as_str() {
        "OPTIONS" => webdav_empty_response(StatusCode::NO_CONTENT, "1, 2, 3, addressbook"),
        "PROPFIND" => {
            let depth = match webdav_depth(&headers) {
                Ok(depth) => depth,
                Err(response) => return response.into_response(),
            };
            let body = match webdav_request_body(req, state.request_size_limit).await {
                Ok(body) => body,
                Err(response) => return response.into_response(),
            };
            if let Err(response) = webdav_propfind_request(&body) {
                return response.into_response();
            }
            carddav_propfind(&state, &auth, &principal, path, depth)
        }
        "PROPPATCH" => {
            let body = match webdav_request_body(req, state.request_size_limit).await {
                Ok(body) => body,
                Err(response) => return response.into_response(),
            };
            carddav_proppatch(&state, &auth, &principal, path, &body)
        }
        "MKCOL" => {
            let body = match webdav_request_body(req, state.request_size_limit).await {
                Ok(body) => body,
                Err(response) => return response.into_response(),
            };
            if !body.is_empty() {
                return error_response(
                    StatusCode::NOT_IMPLEMENTED,
                    Code::Unsupported,
                    "Extended MKCOL request bodies are not supported",
                );
            }
            carddav_mkcol(&state, &auth, &principal, path)
        }
        "REPORT" => {
            let body = match webdav_request_body(req, state.request_size_limit).await {
                Ok(body) => body,
                Err(response) => return response.into_response(),
            };
            carddav_report(&state, &auth, &principal, path, &body)
        }
        "GET" => carddav_get(&state, &auth, &principal, path),
        "HEAD" => webdav_head_response(carddav_get(&state, &auth, &principal, path)),
        "PUT" => {
            let body = match to_bytes(req.into_body(), state.request_size_limit).await {
                Ok(body) => body,
                Err(_) => {
                    return error_response(
                        StatusCode::BAD_REQUEST,
                        Code::InvalidArgument,
                        "invalid CardDAV request body",
                    );
                }
            };
            carddav_put(&state, &auth, &principal, &headers, path, &body)
        }
        "DELETE" => carddav_delete(&state, &auth, &principal, &headers, path),
        _ => webdav_method_not_allowed(
            "OPTIONS, PROPFIND, PROPPATCH, MKCOL, REPORT, GET, HEAD, PUT, DELETE",
            "unsupported CardDAV method",
        ),
    }
}

fn carddav_propfind(
    state: &PimWebDavState,
    auth: &HostedAuth,
    principal: &str,
    path: &str,
    depth: WebDavDepth,
) -> Response {
    let segments = match webdav_segments(path) {
        Ok(segments) => segments,
        Err(()) => return webdav_invalid_path_response(),
    };
    match segments.as_slice() {
        [] => match contact_books(state, auth, principal) {
            Ok(books) => {
                let mut responses = vec![carddav_home_response(principal)];
                if depth == WebDavDepth::One {
                    responses.extend(books.into_iter().map(|book| {
                        carddav_addressbook_response(
                            &format!("/carddav/{}/", url_segment(&book.name)),
                            principal,
                            &book.display_name,
                            &book.sync_token,
                        )
                    }));
                }
                webdav_multistatus(responses)
            }
            Err(err) => loom_error_response(StatusCode::FORBIDDEN, err),
        },
        [prefix, _principal] if prefix == "principals" => {
            webdav_multistatus(vec![carddav_principal_response(principal)])
        }
        [book] => match contact_book(state, auth, principal, book) {
            Ok(Some(book_meta)) => match contact_entries(state, auth, principal, book) {
                Ok(entries) => {
                    let mut responses = vec![carddav_addressbook_response(
                        &format!("/carddav/{}/", url_segment(book)),
                        principal,
                        &book_meta.display_name,
                        &book_meta.sync_token,
                    )];
                    if depth == WebDavDepth::One {
                        responses.extend(entries.into_iter().map(|entry| {
                            webdav_resource_response(
                                &format!(
                                    "/carddav/{}/{}.vcf",
                                    url_segment(book),
                                    url_segment(&entry.uid)
                                ),
                                "text/vcard; charset=utf-8",
                                &entry.etag,
                            )
                        }));
                    }
                    webdav_multistatus(responses)
                }
                Err(err) => loom_error_response(StatusCode::FORBIDDEN, err),
            },
            Ok(None) => error_response(
                StatusCode::NOT_FOUND,
                Code::NotFound,
                "address book not found",
            ),
            Err(err) => loom_error_response(StatusCode::FORBIDDEN, err),
        },
        [book, resource] => {
            let Some(uid) = resource.strip_suffix(".vcf") else {
                return webdav_invalid_path_response();
            };
            match carddav_resource_body(state, auth, principal, book, uid, CardDavDataVersion::V3) {
                Ok(Some(resource)) => webdav_multistatus(vec![webdav_resource_response(
                    &format!(
                        "/carddav/{}/{}.vcf",
                        url_segment(book),
                        url_segment(&resource.uid)
                    ),
                    "text/vcard; charset=utf-8",
                    &resource.etag,
                )]),
                Ok(None) => error_response(
                    StatusCode::NOT_FOUND,
                    Code::NotFound,
                    "contact resource not found",
                ),
                Err(err) => loom_error_response(StatusCode::FORBIDDEN, err),
            }
        }
        _ => error_response(
            StatusCode::BAD_REQUEST,
            Code::InvalidArgument,
            "CardDAV PROPFIND expects an address-book path",
        ),
    }
}

fn carddav_report(
    state: &PimWebDavState,
    auth: &HostedAuth,
    principal: &str,
    path: &str,
    body: &[u8],
) -> Response {
    let report = match webdav_report_request(body) {
        Ok(report) => report,
        Err(response) => return response.into_response(),
    };
    match report {
        WebDavReportRequest::AddressbookMultiget { hrefs, version } => {
            carddav_addressbook_multiget(state, auth, principal, hrefs, version)
        }
        WebDavReportRequest::AddressbookQuery(query) => {
            carddav_addressbook_query(state, auth, principal, path, query)
        }
        WebDavReportRequest::SyncCollection { sync_token } => {
            carddav_sync_collection(state, auth, principal, path, sync_token)
        }
        WebDavReportRequest::Unsupported(name) => {
            let message = format!("unsupported CardDAV REPORT {name}");
            error_response(StatusCode::NOT_IMPLEMENTED, Code::Unsupported, &message)
        }
        _ => error_response(
            StatusCode::NOT_IMPLEMENTED,
            Code::Unsupported,
            "unsupported CardDAV REPORT",
        ),
    }
}

fn carddav_addressbook_multiget(
    state: &PimWebDavState,
    auth: &HostedAuth,
    principal: &str,
    hrefs: Vec<String>,
    version: CardDavDataVersion,
) -> Response {
    if hrefs.is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            Code::InvalidArgument,
            "addressbook-multiget requires at least one href",
        );
    }
    let mut responses = Vec::with_capacity(hrefs.len());
    for href in hrefs {
        let (book, uid) = match webdav_href_resource_path(&href, "carddav", ".vcf") {
            Ok(Some(path)) => path,
            Ok(None) => {
                responses.push(webdav_not_found_response(&href));
                continue;
            }
            Err(()) => return webdav_invalid_path_response(),
        };
        match carddav_resource_body(state, auth, principal, &book, &uid, version) {
            Ok(Some(resource)) => {
                responses.push(carddav_resource_report_response(
                    &href,
                    &resource.etag,
                    &resource.body,
                ));
            }
            Ok(None) => responses.push(webdav_not_found_response(&href)),
            Err(err) => return loom_error_response(StatusCode::FORBIDDEN, err),
        }
    }
    webdav_multistatus(responses)
}

fn carddav_addressbook_query(
    state: &PimWebDavState,
    auth: &HostedAuth,
    principal: &str,
    path: &str,
    query: CardDavAddressbookQuery,
) -> Response {
    let segments = match webdav_segments(path) {
        Ok(segments) => segments,
        Err(()) => return webdav_invalid_path_response(),
    };
    let [book] = segments.as_slice() else {
        return error_response(
            StatusCode::BAD_REQUEST,
            Code::InvalidArgument,
            "addressbook-query expects an address-book path",
        );
    };
    match carddav_query_resources(state, auth, principal, book, &query) {
        Ok(resources) => webdav_multistatus(
            resources
                .into_iter()
                .map(|resource| {
                    carddav_resource_report_response(
                        &format!(
                            "/carddav/{}/{}.vcf",
                            url_segment(book),
                            url_segment(&resource.uid)
                        ),
                        &resource.etag,
                        &resource.body,
                    )
                })
                .collect(),
        ),
        Err(err) => loom_error_response(StatusCode::FORBIDDEN, err),
    }
}

fn carddav_sync_collection(
    state: &PimWebDavState,
    auth: &HostedAuth,
    principal: &str,
    path: &str,
    sync_token: Option<String>,
) -> Response {
    let segments = match webdav_segments(path) {
        Ok(segments) => segments,
        Err(()) => return webdav_invalid_path_response(),
    };
    let [book] = segments.as_slice() else {
        return error_response(
            StatusCode::BAD_REQUEST,
            Code::InvalidArgument,
            "sync-collection expects an address-book path",
        );
    };
    let requested_token = sync_token.filter(|token| !token.is_empty());
    match carddav_sync_resources(state, auth, principal, book, requested_token.as_deref()) {
        Ok((current_token, rows)) => {
            let responses = rows
                .into_iter()
                .map(|row| match row {
                    CardDavSyncRow::Present(resource) => carddav_resource_report_response(
                        &format!(
                            "/carddav/{}/{}.vcf",
                            url_segment(book),
                            url_segment(&resource.uid)
                        ),
                        &resource.etag,
                        &resource.body,
                    ),
                    CardDavSyncRow::Removed(uid) => webdav_not_found_response(&format!(
                        "/carddav/{}/{}.vcf",
                        url_segment(book),
                        url_segment(&uid)
                    )),
                })
                .collect();
            webdav_sync_multistatus(responses, &current_token)
        }
        Err(err) if err.code == Code::CursorInvalid => {
            loom_error_response(StatusCode::CONFLICT, err)
        }
        Err(err) => loom_error_response(StatusCode::FORBIDDEN, err),
    }
}

fn carddav_mkcol(
    state: &PimWebDavState,
    auth: &HostedAuth,
    principal: &str,
    path: &str,
) -> Response {
    let segments = match webdav_segments(path) {
        Ok(segments) => segments,
        Err(()) => return webdav_invalid_path_response(),
    };
    let [book] = segments.as_slice() else {
        return error_response(
            StatusCode::BAD_REQUEST,
            Code::InvalidArgument,
            "CardDAV MKCOL expects an address-book path",
        );
    };
    match state.kernel.write(auth, |loom| {
        let ns = resolve_hosted_pim_workspace(loom, &state.workspace, FacetKind::Contacts)?;
        contacts::create_book(
            loom,
            ns,
            principal,
            book,
            &contacts::BookMeta {
                display_name: book.to_string(),
            },
        )?;
        loom.commit(ns, "loom-hosted", "carddav mkcol", 0)?;
        Ok(())
    }) {
        Ok(()) => webdav_empty_response(StatusCode::CREATED, "1, 2, 3, addressbook"),
        Err(err) => loom_error_response(StatusCode::FORBIDDEN, err),
    }
}

fn carddav_proppatch(
    state: &PimWebDavState,
    auth: &HostedAuth,
    principal: &str,
    path: &str,
    body: &[u8],
) -> Response {
    let segments = match webdav_segments(path) {
        Ok(segments) => segments,
        Err(()) => return webdav_invalid_path_response(),
    };
    let [book] = segments.as_slice() else {
        return error_response(
            StatusCode::BAD_REQUEST,
            Code::InvalidArgument,
            "CardDAV PROPPATCH expects an address-book path",
        );
    };
    let update = match webdav_property_update(body, WebDavPropertyProfile::AddressBook) {
        Ok(update) => update,
        Err(response) => return response.into_response(),
    };
    match state.kernel.write(auth, |loom| {
        let ns = resolve_hosted_pim_workspace(loom, &state.workspace, FacetKind::Contacts)?;
        if let Some(display_name) = update.display_name.as_ref() {
            let mut meta = contacts::get_book(loom, ns, principal, book)?
                .ok_or_else(|| LoomError::not_found("address book not found"))?;
            meta.display_name = display_name.clone();
            contacts::create_book(loom, ns, principal, book, &meta)?;
            loom.commit(ns, "loom-hosted", "carddav proppatch", 0)?;
        }
        Ok(())
    }) {
        Ok(()) => webdav_proppatch_response(
            &format!("/carddav/{}/", url_segment(book)),
            update.properties,
        ),
        Err(err) => loom_error_response(StatusCode::FORBIDDEN, err),
    }
}

fn carddav_get(state: &PimWebDavState, auth: &HostedAuth, principal: &str, path: &str) -> Response {
    let (book, uid) = match webdav_resource_path(path, ".vcf") {
        Ok(Some(path)) => path,
        Ok(None) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                Code::InvalidArgument,
                "CardDAV GET expects /carddav/{book}/{uid}.vcf",
            );
        }
        Err(()) => return webdav_invalid_path_response(),
    };
    match carddav_resource_body(
        state,
        auth,
        principal,
        &book,
        &uid,
        CardDavDataVersion::default(),
    ) {
        Ok(Some(resource)) => Response::builder()
            .status(StatusCode::OK)
            .header(CONTENT_TYPE, "text/vcard; charset=utf-8")
            .header("etag", format!("\"{}\"", resource.etag))
            .body(Body::from(resource.body))
            .unwrap_or_else(|_| {
                response(StatusCode::OK, "text/vcard; charset=utf-8", Body::empty())
            }),
        Ok(None) => error_response(
            StatusCode::NOT_FOUND,
            Code::NotFound,
            "contact resource not found",
        ),
        Err(err) => loom_error_response(StatusCode::FORBIDDEN, err),
    }
}

fn carddav_put(
    state: &PimWebDavState,
    auth: &HostedAuth,
    principal: &str,
    headers: &HeaderMap,
    path: &str,
    body: &[u8],
) -> Response {
    let (book, uid) = match webdav_resource_path(path, ".vcf") {
        Ok(Some(path)) => path,
        Ok(None) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                Code::InvalidArgument,
                "CardDAV PUT expects /carddav/{book}/{uid}.vcf",
            );
        }
        Err(()) => return webdav_invalid_path_response(),
    };
    let vcf = match std::str::from_utf8(body) {
        Ok(vcf) => vcf,
        Err(_) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                Code::InvalidArgument,
                "CardDAV resource body must be UTF-8 vCard",
            );
        }
    };
    let entry = match contacts::ContactEntry::from_vcard(vcf) {
        Ok(entry) => entry,
        Err(err) => return loom_error_response(StatusCode::BAD_REQUEST, err),
    };
    if entry.uid != uid {
        return error_response(
            StatusCode::BAD_REQUEST,
            Code::InvalidArgument,
            "CardDAV resource UID must match the path",
        );
    }
    match state.kernel.write(auth, |loom| {
        let ns = resolve_hosted_pim_workspace(loom, &state.workspace, FacetKind::Contacts)?;
        let current = contact_current_etag(loom, ns, principal, &book, &uid)?;
        let created = current.is_none();
        webdav_check_preconditions(headers, current.as_deref())?;
        let etag = contacts::put_entry(loom, ns, principal, &book, &entry)?;
        loom.commit(ns, "loom-hosted", "carddav put", 0)?;
        Ok((etag, created))
    }) {
        Ok((etag, created)) => {
            let etag = etag.to_hex();
            if created {
                Response::builder()
                    .status(StatusCode::CREATED)
                    .header(CONTENT_TYPE, "application/json")
                    .header("etag", format!("\"{etag}\""))
                    .body(Body::from(format!("{{\"etag\":{}}}", json_string(&etag))))
                    .unwrap_or_else(|_| {
                        response(StatusCode::CREATED, "application/json", Body::empty())
                    })
            } else {
                Response::builder()
                    .status(StatusCode::NO_CONTENT)
                    .header("etag", format!("\"{etag}\""))
                    .body(Body::empty())
                    .unwrap_or_else(|_| response(StatusCode::NO_CONTENT, "", Body::empty()))
            }
        }
        Err(err) => webdav_write_error_response(err),
    }
}

fn carddav_delete(
    state: &PimWebDavState,
    auth: &HostedAuth,
    principal: &str,
    headers: &HeaderMap,
    path: &str,
) -> Response {
    let (book, uid) = match webdav_resource_path(path, ".vcf") {
        Ok(Some(path)) => path,
        Ok(None) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                Code::InvalidArgument,
                "CardDAV DELETE expects /carddav/{book}/{uid}.vcf",
            );
        }
        Err(()) => return webdav_invalid_path_response(),
    };
    match state.kernel.write(auth, |loom| {
        let ns = resolve_hosted_pim_workspace(loom, &state.workspace, FacetKind::Contacts)?;
        let current = contact_current_etag(loom, ns, principal, &book, &uid)?;
        webdav_check_preconditions(headers, current.as_deref())?;
        let deleted = contacts::delete_entry(loom, ns, principal, &book, &uid)?;
        if deleted {
            loom.commit(ns, "loom-hosted", "carddav delete", 0)?;
        }
        Ok(deleted)
    }) {
        Ok(deleted) => {
            if deleted {
                webdav_empty_response(StatusCode::NO_CONTENT, "1, 2, 3, addressbook")
            } else {
                error_response(
                    StatusCode::NOT_FOUND,
                    Code::NotFound,
                    "contact resource not found",
                )
            }
        }
        Err(err) => webdav_write_error_response(err),
    }
}

#[derive(Clone, Debug)]
struct CalDavCollectionMeta {
    name: String,
    display_name: String,
    components: Vec<calendar::Component>,
    sync_token: String,
}

#[derive(Clone, Debug)]
struct WebDavResourceMeta {
    uid: String,
    etag: String,
}

#[derive(Clone, Debug)]
struct CardDavBookMeta {
    name: String,
    display_name: String,
    sync_token: String,
}

#[derive(Clone, Debug)]
struct CalDavResourceBody {
    uid: String,
    etag: String,
    body: String,
}

#[derive(Clone, Debug)]
struct CardDavResourceBody {
    uid: String,
    etag: String,
    body: String,
}

enum CalDavSyncRow {
    Present(CalDavResourceBody),
    Removed(String),
}

enum CardDavSyncRow {
    Present(CardDavResourceBody),
    Removed(String),
}

fn calendar_collections(
    state: &PimWebDavState,
    auth: &HostedAuth,
    principal: &str,
) -> Result<Vec<CalDavCollectionMeta>, LoomError> {
    state.kernel.read(auth, |loom| {
        let ns = resolve_hosted_pim_workspace(loom, &state.workspace, FacetKind::Calendar)?;
        let collections = calendar::list_collections(loom, ns, principal)?;
        collections
            .into_iter()
            .map(|name| {
                let meta =
                    calendar::get_collection(loom, ns, principal, &name)?.ok_or_else(|| {
                        LoomError::not_found(format!("calendar collection {principal}/{name}"))
                    })?;
                let entries = calendar::list_entries(loom, ns, principal, &name)?;
                Ok(CalDavCollectionMeta {
                    display_name: if meta.display_name.is_empty() {
                        name.clone()
                    } else {
                        meta.display_name
                    },
                    components: if meta.component_set.is_empty() {
                        vec![calendar::Component::Event, calendar::Component::Todo]
                    } else {
                        meta.component_set
                    },
                    sync_token: caldav_collection_sync_token(loom, ns, &entries),
                    name,
                })
            })
            .collect()
    })
}

fn calendar_collection(
    state: &PimWebDavState,
    auth: &HostedAuth,
    principal: &str,
    collection: &str,
) -> Result<Option<CalDavCollectionMeta>, LoomError> {
    state.kernel.read(auth, |loom| {
        let ns = resolve_hosted_pim_workspace(loom, &state.workspace, FacetKind::Calendar)?;
        let Some(meta) = calendar::get_collection(loom, ns, principal, collection)? else {
            return Ok(None);
        };
        let entries = calendar::list_entries(loom, ns, principal, collection)?;
        Ok(Some(CalDavCollectionMeta {
            name: collection.to_string(),
            display_name: if meta.display_name.is_empty() {
                collection.to_string()
            } else {
                meta.display_name
            },
            components: if meta.component_set.is_empty() {
                vec![calendar::Component::Event, calendar::Component::Todo]
            } else {
                meta.component_set
            },
            sync_token: caldav_collection_sync_token(loom, ns, &entries),
        }))
    })
}

fn calendar_entries(
    state: &PimWebDavState,
    auth: &HostedAuth,
    principal: &str,
    collection: &str,
) -> Result<Vec<WebDavResourceMeta>, LoomError> {
    state.kernel.read(auth, |loom| {
        let ns = resolve_hosted_pim_workspace(loom, &state.workspace, FacetKind::Calendar)?;
        calendar::list_entries(loom, ns, principal, collection).map(|entries| {
            entries
                .into_iter()
                .map(|entry| WebDavResourceMeta {
                    etag: calendar::entry_etag(loom, &entry).to_hex(),
                    uid: entry.uid,
                })
                .collect()
        })
    })
}

fn caldav_resource_body(
    state: &PimWebDavState,
    auth: &HostedAuth,
    principal: &str,
    collection: &str,
    uid: &str,
) -> Result<Option<CalDavResourceBody>, LoomError> {
    state.kernel.read(auth, |loom| {
        let ns = resolve_hosted_pim_workspace(loom, &state.workspace, FacetKind::Calendar)?;
        calendar::get_entry(loom, ns, principal, collection, uid).map(|entry| {
            entry.map(|entry| CalDavResourceBody {
                uid: entry.uid.clone(),
                etag: calendar::entry_etag(loom, &entry).to_hex(),
                body: entry.to_ics(),
            })
        })
    })
}

fn caldav_query_resources(
    state: &PimWebDavState,
    auth: &HostedAuth,
    principal: &str,
    collection: &str,
    query: &CalDavCalendarQuery,
) -> Result<Vec<CalDavResourceBody>, LoomError> {
    state.kernel.read(auth, |loom| {
        let ns = resolve_hosted_pim_workspace(loom, &state.workspace, FacetKind::Calendar)?;
        calendar::get_collection(loom, ns, principal, collection)?.ok_or_else(|| {
            LoomError::not_found(format!(
                "calendar: collection {principal}/{collection} does not exist"
            ))
        })?;
        calendar::list_entries(loom, ns, principal, collection)?
            .into_iter()
            .filter_map(|entry| match caldav_entry_matches(&entry, query) {
                Ok(true) => Some(Ok(CalDavResourceBody {
                    uid: entry.uid.clone(),
                    etag: calendar::entry_etag(loom, &entry).to_hex(),
                    body: entry.to_ics(),
                })),
                Ok(false) => None,
                Err(err) => Some(Err(err)),
            })
            .collect()
    })
}

fn caldav_sync_resources(
    state: &PimWebDavState,
    auth: &HostedAuth,
    principal: &str,
    collection: &str,
    requested_token: Option<&str>,
) -> Result<(String, Vec<CalDavSyncRow>), LoomError> {
    state.kernel.read(auth, |loom| {
        let ns = resolve_hosted_pim_workspace(loom, &state.workspace, FacetKind::Calendar)?;
        calendar::get_collection(loom, ns, principal, collection)?.ok_or_else(|| {
            LoomError::not_found(format!(
                "calendar: collection {principal}/{collection} does not exist"
            ))
        })?;
        let entries = calendar::list_entries(loom, ns, principal, collection)?;
        let sync_token = caldav_collection_sync_token(loom, ns, &entries);
        if requested_token.is_some_and(|token| token == sync_token) {
            return Ok((sync_token, Vec::new()));
        }
        let rows = match requested_token {
            None => entries
                .into_iter()
                .map(|entry| caldav_sync_present_row(loom, entry))
                .collect::<Vec<_>>(),
            Some(token) => {
                let old = caldav_sync_token_digest(token)?;
                let old_entries =
                    calendar::list_entries_at_commit(loom, ns, old, principal, collection)
                        .map_err(|_| {
                            LoomError::new(Code::CursorInvalid, "invalid CalDAV sync-token")
                        })?;
                let changes = calendar::diff_entries(loom, &old_entries, &entries);
                let mut rows = Vec::new();
                for change in changes {
                    match change.kind {
                        calendar::ChangeKind::Added | calendar::ChangeKind::Updated => {
                            let Some(entry) = entries.iter().find(|entry| entry.uid == change.uid)
                            else {
                                return Err(LoomError::new(
                                    Code::Internal,
                                    "calendar sync diff referenced absent current entry",
                                ));
                            };
                            rows.push(caldav_sync_present_row(loom, entry.clone()));
                        }
                        calendar::ChangeKind::Removed => {
                            rows.push(CalDavSyncRow::Removed(change.uid))
                        }
                    }
                }
                rows
            }
        };
        Ok((sync_token, rows))
    })
}

fn caldav_sync_present_row<S: loom_core::provider::ObjectStore>(
    loom: &loom_core::Loom<S>,
    entry: calendar::CalendarEntry,
) -> CalDavSyncRow {
    CalDavSyncRow::Present(CalDavResourceBody {
        uid: entry.uid.clone(),
        etag: calendar::entry_etag(loom, &entry).to_hex(),
        body: entry.to_ics(),
    })
}

fn caldav_sync_token_digest(token: &str) -> Result<Digest, LoomError> {
    let Some(raw) = token.strip_prefix("loom-caldav-sync-v1:") else {
        return Err(LoomError::new(
            Code::CursorInvalid,
            "invalid CalDAV sync-token",
        ));
    };
    Digest::parse(raw).map_err(|_| LoomError::new(Code::CursorInvalid, "invalid CalDAV sync-token"))
}

fn caldav_entry_matches(
    entry: &calendar::CalendarEntry,
    query: &CalDavCalendarQuery,
) -> Result<bool, LoomError> {
    if query
        .component
        .is_some_and(|component| component != entry.component.0)
    {
        return Ok(false);
    }
    if let Some(uid_text) = &query.uid_text {
        let uid = entry.uid.to_lowercase();
        if !uid.contains(uid_text) {
            return Ok(false);
        }
    }
    if let Some(summary_text) = &query.summary_text {
        let summary = entry.summary.to_lowercase();
        if !summary.contains(summary_text) {
            return Ok(false);
        }
    }
    if let Some((from, to)) = query.time_range
        && entry.occurrence_starts(from, to)?.is_empty()
    {
        return Ok(false);
    }
    Ok(true)
}

fn caldav_collection_sync_token<S: loom_core::provider::ObjectStore>(
    loom: &loom_core::Loom<S>,
    ns: WorkspaceId,
    entries: &[calendar::CalendarEntry],
) -> String {
    if let Ok(head) = loom.registry().head_branch(ns)
        && let Ok(Some(tip)) = loom.registry().branch_tip(ns, &head)
    {
        return format!("loom-caldav-sync-v1:{tip}");
    }
    let mut bytes = b"loom-caldav-sync-v1\n".to_vec();
    for entry in entries {
        bytes.extend_from_slice(entry.uid.as_bytes());
        bytes.push(0);
        bytes.extend_from_slice(calendar::entry_etag(loom, entry).to_string().as_bytes());
        bytes.push(b'\n');
    }
    format!(
        "loom-caldav-sync-v1:{}",
        Digest::hash(loom.store().digest_algo(), &bytes)
    )
}

enum WebDavReportRequest {
    CalendarMultiget {
        hrefs: Vec<String>,
    },
    AddressbookMultiget {
        hrefs: Vec<String>,
        version: CardDavDataVersion,
    },
    AddressbookQuery(CardDavAddressbookQuery),
    CalendarQuery(CalDavCalendarQuery),
    SyncCollection {
        sync_token: Option<String>,
    },
    Unsupported(String),
}

#[derive(Clone, Debug, Default)]
struct CalDavCalendarQuery {
    component: Option<calendar::Component>,
    uid_text: Option<String>,
    summary_text: Option<String>,
    time_range: Option<(calendar::DateTime, calendar::DateTime)>,
}

#[derive(Clone, Debug, Default)]
struct CardDavAddressbookQuery {
    filters: Vec<CardDavPropFilter>,
    version: CardDavDataVersion,
}

#[derive(Clone, Copy, Debug, Default)]
enum CardDavDataVersion {
    #[default]
    V3,
    V4,
}

#[derive(Clone, Debug)]
struct CardDavPropFilter {
    name: String,
    text: Option<String>,
    param_filters: Vec<CardDavParamFilter>,
}

#[derive(Clone, Debug)]
struct CardDavParamFilter {
    name: String,
    text: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CardDavTextTarget {
    Prop,
    Param,
}

fn contact_books(
    state: &PimWebDavState,
    auth: &HostedAuth,
    principal: &str,
) -> Result<Vec<CardDavBookMeta>, LoomError> {
    state.kernel.read(auth, |loom| {
        let ns = resolve_hosted_pim_workspace(loom, &state.workspace, FacetKind::Contacts)?;
        contacts::list_books(loom, ns, principal)?
            .into_iter()
            .map(|name| {
                let meta = contacts::get_book(loom, ns, principal, &name)?.ok_or_else(|| {
                    LoomError::not_found(format!("address book {principal}/{name}"))
                })?;
                let entries = contacts::list_entries(loom, ns, principal, &name)?;
                Ok(CardDavBookMeta {
                    display_name: if meta.display_name.is_empty() {
                        name.clone()
                    } else {
                        meta.display_name
                    },
                    sync_token: carddav_book_sync_token(loom, ns, &entries),
                    name,
                })
            })
            .collect()
    })
}

fn contact_book(
    state: &PimWebDavState,
    auth: &HostedAuth,
    principal: &str,
    book: &str,
) -> Result<Option<CardDavBookMeta>, LoomError> {
    state.kernel.read(auth, |loom| {
        let ns = resolve_hosted_pim_workspace(loom, &state.workspace, FacetKind::Contacts)?;
        let Some(meta) = contacts::get_book(loom, ns, principal, book)? else {
            return Ok(None);
        };
        let entries = contacts::list_entries(loom, ns, principal, book)?;
        Ok(Some(CardDavBookMeta {
            name: book.to_string(),
            display_name: if meta.display_name.is_empty() {
                book.to_string()
            } else {
                meta.display_name
            },
            sync_token: carddav_book_sync_token(loom, ns, &entries),
        }))
    })
}

fn contact_entries(
    state: &PimWebDavState,
    auth: &HostedAuth,
    principal: &str,
    book: &str,
) -> Result<Vec<WebDavResourceMeta>, LoomError> {
    state.kernel.read(auth, |loom| {
        let ns = resolve_hosted_pim_workspace(loom, &state.workspace, FacetKind::Contacts)?;
        contacts::list_entries(loom, ns, principal, book).map(|entries| {
            entries
                .into_iter()
                .map(|entry| WebDavResourceMeta {
                    etag: contacts::entry_etag(loom, &entry).to_hex(),
                    uid: entry.uid,
                })
                .collect()
        })
    })
}

fn carddav_resource_body(
    state: &PimWebDavState,
    auth: &HostedAuth,
    principal: &str,
    book: &str,
    uid: &str,
    version: CardDavDataVersion,
) -> Result<Option<CardDavResourceBody>, LoomError> {
    state.kernel.read(auth, |loom| {
        let ns = resolve_hosted_pim_workspace(loom, &state.workspace, FacetKind::Contacts)?;
        contacts::get_entry(loom, ns, principal, book, uid).map(|entry| {
            entry.map(|entry| CardDavResourceBody {
                uid: entry.uid.clone(),
                etag: contacts::entry_etag(loom, &entry).to_hex(),
                body: carddav_entry_vcard(&entry, version),
            })
        })
    })
}

fn carddav_sync_resources(
    state: &PimWebDavState,
    auth: &HostedAuth,
    principal: &str,
    book: &str,
    requested_token: Option<&str>,
) -> Result<(String, Vec<CardDavSyncRow>), LoomError> {
    state.kernel.read(auth, |loom| {
        let ns = resolve_hosted_pim_workspace(loom, &state.workspace, FacetKind::Contacts)?;
        contacts::get_book(loom, ns, principal, book)?.ok_or_else(|| {
            LoomError::not_found(format!("address book {principal}/{book} does not exist"))
        })?;
        let entries = contacts::list_entries(loom, ns, principal, book)?;
        let sync_token = carddav_book_sync_token(loom, ns, &entries);
        if requested_token.is_some_and(|token| token == sync_token) {
            return Ok((sync_token, Vec::new()));
        }
        let rows = match requested_token {
            None => entries
                .into_iter()
                .map(|entry| carddav_sync_present_row(loom, entry))
                .collect::<Vec<_>>(),
            Some(token) => {
                let old = carddav_sync_token_digest(token)?;
                let old_entries =
                    match contacts::list_entries_at_commit(loom, ns, old, principal, book) {
                        Ok(old_entries) => old_entries,
                        Err(_) => {
                            return Ok((
                                sync_token,
                                entries
                                    .into_iter()
                                    .map(|entry| carddav_sync_present_row(loom, entry))
                                    .collect::<Vec<_>>(),
                            ));
                        }
                    };
                let changes = contacts::diff_entries(loom, &old_entries, &entries);
                let mut rows = Vec::new();
                for change in changes {
                    match change.kind {
                        contacts::ChangeKind::Added | contacts::ChangeKind::Updated => {
                            let Some(entry) = entries.iter().find(|entry| entry.uid == change.uid)
                            else {
                                return Err(LoomError::new(
                                    Code::Internal,
                                    "contact sync diff referenced absent current entry",
                                ));
                            };
                            rows.push(carddav_sync_present_row(loom, entry.clone()));
                        }
                        contacts::ChangeKind::Removed => {
                            rows.push(CardDavSyncRow::Removed(change.uid))
                        }
                    }
                }
                rows
            }
        };
        Ok((sync_token, rows))
    })
}

fn carddav_sync_present_row<S: loom_core::provider::ObjectStore>(
    loom: &loom_core::Loom<S>,
    entry: contacts::ContactEntry,
) -> CardDavSyncRow {
    CardDavSyncRow::Present(CardDavResourceBody {
        uid: entry.uid.clone(),
        etag: contacts::entry_etag(loom, &entry).to_hex(),
        body: entry.to_vcard3(),
    })
}

fn carddav_sync_token_digest(token: &str) -> Result<Digest, LoomError> {
    let Some(raw) = token.strip_prefix("loom-carddav-sync-v1:") else {
        return Err(LoomError::new(
            Code::CursorInvalid,
            "invalid CardDAV sync-token",
        ));
    };
    Digest::parse(raw)
        .map_err(|_| LoomError::new(Code::CursorInvalid, "invalid CardDAV sync-token"))
}

fn carddav_book_sync_token<S: loom_core::provider::ObjectStore>(
    loom: &loom_core::Loom<S>,
    ns: WorkspaceId,
    entries: &[contacts::ContactEntry],
) -> String {
    if let Ok(head) = loom.registry().head_branch(ns)
        && let Ok(Some(tip)) = loom.registry().branch_tip(ns, &head)
    {
        return format!("loom-carddav-sync-v1:{tip}");
    }
    let mut bytes = b"loom-carddav-sync-v1\n".to_vec();
    for entry in entries {
        bytes.extend_from_slice(entry.uid.as_bytes());
        bytes.push(0);
        bytes.extend_from_slice(contacts::entry_etag(loom, entry).to_string().as_bytes());
        bytes.push(b'\n');
    }
    format!(
        "loom-carddav-sync-v1:{}",
        Digest::hash(loom.store().digest_algo(), &bytes)
    )
}

fn carddav_query_resources(
    state: &PimWebDavState,
    auth: &HostedAuth,
    principal: &str,
    book: &str,
    query: &CardDavAddressbookQuery,
) -> Result<Vec<CardDavResourceBody>, LoomError> {
    state.kernel.read(auth, |loom| {
        let ns = resolve_hosted_pim_workspace(loom, &state.workspace, FacetKind::Contacts)?;
        contacts::get_book(loom, ns, principal, book)?.ok_or_else(|| {
            LoomError::not_found(format!("address book {principal}/{book} does not exist"))
        })?;
        contacts::list_entries(loom, ns, principal, book).map(|entries| {
            entries
                .into_iter()
                .filter(|entry| carddav_entry_matches(entry, query))
                .map(|entry| CardDavResourceBody {
                    uid: entry.uid.clone(),
                    etag: contacts::entry_etag(loom, &entry).to_hex(),
                    body: carddav_entry_vcard(&entry, query.version),
                })
                .collect()
        })
    })
}

fn carddav_entry_vcard(entry: &contacts::ContactEntry, version: CardDavDataVersion) -> String {
    match version {
        CardDavDataVersion::V3 => entry.to_vcard3(),
        CardDavDataVersion::V4 => entry.to_vcard(),
    }
}

fn carddav_entry_matches(entry: &contacts::ContactEntry, query: &CardDavAddressbookQuery) -> bool {
    query
        .filters
        .iter()
        .all(|filter| carddav_prop_filter_matches(entry, filter))
}

fn carddav_prop_filter_matches(entry: &contacts::ContactEntry, filter: &CardDavPropFilter) -> bool {
    match filter.name.as_str() {
        "UID" => carddav_text_matches(&entry.uid, filter.text.as_deref()),
        "FN" => carddav_text_matches(&entry.full_name, filter.text.as_deref()),
        "N" => entry
            .n
            .as_deref()
            .is_some_and(|value| carddav_text_matches(value, filter.text.as_deref())),
        "ORG" => entry
            .org
            .as_deref()
            .is_some_and(|value| carddav_text_matches(value, filter.text.as_deref())),
        "TITLE" => entry
            .title
            .as_deref()
            .is_some_and(|value| carddav_text_matches(value, filter.text.as_deref())),
        "EMAIL" => carddav_typed_values_match(&entry.emails, filter),
        "TEL" => carddav_typed_values_match(&entry.tels, filter),
        _ => false,
    }
}

fn carddav_typed_values_match(values: &[contacts::TypedValue], filter: &CardDavPropFilter) -> bool {
    values.iter().any(|value| {
        carddav_text_matches(&value.value, filter.text.as_deref())
            && filter
                .param_filters
                .iter()
                .all(|param| carddav_param_filter_matches(value, param))
    })
}

fn carddav_param_filter_matches(value: &contacts::TypedValue, filter: &CardDavParamFilter) -> bool {
    match filter.name.as_str() {
        "TYPE" => value
            .kind
            .as_deref()
            .is_some_and(|kind| carddav_text_matches(kind, filter.text.as_deref())),
        _ => false,
    }
}

fn carddav_text_matches(value: &str, needle: Option<&str>) -> bool {
    match needle {
        Some(needle) => value.to_lowercase().contains(needle),
        None => !value.is_empty(),
    }
}

fn hosted_principal_name(
    kernel: &HostedKernel,
    auth: &HostedAuth,
    headers: &HeaderMap,
) -> HostedHttpResult<String> {
    let principal = match header(headers, "x-loom-principal") {
        Ok(Some(principal)) => principal,
        Ok(None) => return Ok("root".to_string()),
        Err(err) => return Err(err),
    };
    let principal_id = WorkspaceId::parse(&principal)
        .map_err(|err| HostedHttpError::from(loom_error_response(StatusCode::BAD_REQUEST, err)))?;
    kernel
        .read(auth, |loom| {
            let Some(identity) = loom.identity_store() else {
                return Ok(principal);
            };
            identity
                .principal(principal_id)
                .map(|principal| principal.name.clone())
        })
        .map_err(|err| HostedHttpError::from(loom_error_response(StatusCode::FORBIDDEN, err)))
}

struct HostedDavAuth {
    auth: HostedAuth,
    principal: String,
}

fn hosted_dav_auth(state: &PimWebDavState, headers: &HeaderMap) -> HostedHttpResult<HostedDavAuth> {
    let basic = authorization_basic(headers)?;
    let has_loom_passphrase = header(headers, "x-loom-principal")?.is_some()
        || header(headers, "x-loom-passphrase")?.is_some();
    if basic.is_some() && has_loom_passphrase {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            Code::InvalidArgument,
            "supply either HTTP Basic auth or Loom passphrase headers",
        )
        .into());
    }
    if let Some((username, password)) = basic {
        let now = Instant::now();
        let cached = state
            .basic_auth_cache
            .lock()
            .ok()
            .and_then(|mut cache| cache.get(&username, &password, now));
        if let Some(cached) = cached {
            return Ok(HostedDavAuth {
                auth: HostedAuth::preauthenticated(
                    cached.principal,
                    format!("dav-{}", cached.principal),
                ),
                principal: cached.principal_name,
            });
        }
        let (principal, principal_name) =
            resolve_http_basic_principal(&state.kernel, &username, headers)?;
        let auth = HostedAuth::passphrase(principal, password, format!("dav-{principal}"));
        state.kernel.read(&auth, |_| Ok(())).map_err(|err| {
            HostedHttpError::from(loom_error_response(StatusCode::FORBIDDEN, err))
        })?;
        if let Ok(mut cache) = state.basic_auth_cache.lock() {
            cache.insert(
                &username,
                auth.passphrase.as_deref().unwrap_or_default(),
                principal,
                principal_name.clone(),
                now,
            );
        }
        return Ok(HostedDavAuth {
            auth,
            principal: principal_name,
        });
    }
    let auth = match hosted_auth(headers, state.auth_policy) {
        Ok(auth) => auth,
        Err(err) if state.auth_policy == HostedAuthPolicy::Passphrase => {
            return Err(basic_challenge_response(err.into_response()).into());
        }
        Err(err) => return Err(err),
    };
    let principal = hosted_principal_name(&state.kernel, &auth, headers)?;
    Ok(HostedDavAuth { auth, principal })
}

fn resolve_http_basic_principal(
    kernel: &HostedKernel,
    username: &str,
    headers: &HeaderMap,
) -> HostedHttpResult<(WorkspaceId, String)> {
    kernel
        .read(&HostedAuth::unauthenticated(), |loom| {
            let candidates = http_basic_principal_candidates(username, headers);
            let Some(identity) = loom.identity_store() else {
                for candidate in candidates {
                    if let Ok(principal) = WorkspaceId::parse(&candidate) {
                        return Ok((principal, candidate));
                    }
                }
                return Err(LoomError::new(
                    Code::AuthenticationFailed,
                    "unknown HTTP Basic principal",
                ));
            };
            for candidate in candidates {
                if let Ok(principal) = WorkspaceId::parse(&candidate) {
                    let principal_record = identity.principal(principal)?;
                    return Ok((principal, principal_record.name.clone()));
                }
                if let Some(principal) = identity
                    .principals()
                    .find(|principal| principal_name_matches(&principal.name, &candidate))
                {
                    return Ok((principal.id, principal.name.clone()));
                }
            }
            Err(LoomError::new(
                Code::AuthenticationFailed,
                "unknown HTTP Basic principal",
            ))
        })
        .map_err(|err| HostedHttpError::from(loom_error_response(StatusCode::UNAUTHORIZED, err)))
}

fn http_basic_principal_candidates(username: &str, headers: &HeaderMap) -> Vec<String> {
    let mut candidates = Vec::new();
    push_unique_candidate(&mut candidates, username.to_string());
    if username.contains('%')
        && let Ok(decoded) = percent_decode_path(username)
        && decoded != username
    {
        push_unique_candidate(&mut candidates, decoded);
    }
    if !username.contains('@')
        && !username.contains('%')
        && let Some(domain) = dav_request_host_domain(headers)
    {
        push_unique_candidate(&mut candidates, format!("{username}@{domain}"));
    }
    candidates
}

fn push_unique_candidate(candidates: &mut Vec<String>, candidate: String) {
    if !candidates.iter().any(|existing| existing == &candidate) {
        candidates.push(candidate);
    }
}

fn dav_request_host_domain(headers: &HeaderMap) -> Option<String> {
    let host = headers.get("host")?.to_str().ok()?.trim();
    let host = if let Some(rest) = host.strip_prefix('[') {
        let (host, _) = rest.split_once(']')?;
        host
    } else {
        host.split_once(':').map_or(host, |(host, _)| host)
    };
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty()
        || host.parse::<IpAddr>().is_ok()
        || host
            .chars()
            .any(|ch| ch.is_control() || matches!(ch, '/' | '\\' | '@'))
    {
        return None;
    }
    Some(host)
}

fn principal_name_matches(stored: &str, candidate: &str) -> bool {
    if stored == candidate {
        return true;
    }
    let Some((stored_local, stored_domain)) = stored.rsplit_once('@') else {
        return false;
    };
    let Some((candidate_local, candidate_domain)) = candidate.rsplit_once('@') else {
        return false;
    };
    stored_local == candidate_local && stored_domain.eq_ignore_ascii_case(candidate_domain)
}

fn resolve_hosted_pim_workspace<S: loom_core::provider::ObjectStore>(
    loom: &loom_core::Loom<S>,
    workspace: &str,
    facet: FacetKind,
) -> Result<WorkspaceId, LoomError> {
    let selector = match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Typed {
            ty: facet,
            name: workspace.to_string(),
        },
    };
    loom.registry().open(&selector)
}

fn calendar_current_etag<S: loom_core::provider::ObjectStore>(
    loom: &loom_core::Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    collection: &str,
    uid: &str,
) -> Result<Option<String>, LoomError> {
    calendar::get_entry(loom, ns, principal, collection, uid)
        .map(|entry| entry.map(|entry| calendar::entry_etag(loom, &entry).to_hex()))
}

fn contact_current_etag<S: loom_core::provider::ObjectStore>(
    loom: &loom_core::Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    book: &str,
    uid: &str,
) -> Result<Option<String>, LoomError> {
    contacts::get_entry(loom, ns, principal, book, uid)
        .map(|entry| entry.map(|entry| contacts::entry_etag(loom, &entry).to_hex()))
}

fn webdav_segments(path: &str) -> Result<Vec<String>, ()> {
    let trimmed = path.trim_matches('/');
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    trimmed
        .split('/')
        .map(webdav_segment)
        .collect::<Result<Vec<_>, _>>()
}

fn webdav_segment(segment: &str) -> Result<String, ()> {
    let segment = percent_decode_path(segment)?;
    if segment.is_empty() || segment == "." || segment == ".." {
        return Err(());
    }
    if segment
        .chars()
        .any(|ch| ch.is_control() || matches!(ch, '/' | '\\'))
    {
        return Err(());
    }
    Ok(segment)
}

fn webdav_resource_path(path: &str, suffix: &str) -> Result<Option<(String, String)>, ()> {
    let segments = webdav_segments(path)?;
    let [collection, resource] = segments.as_slice() else {
        return Ok(None);
    };
    let Some(uid) = resource.strip_suffix(suffix) else {
        return Ok(None);
    };
    if uid.is_empty() {
        return Err(());
    }
    Ok(Some((collection.clone(), uid.to_string())))
}

fn webdav_href_resource_path(
    href: &str,
    root: &str,
    suffix: &str,
) -> Result<Option<(String, String)>, ()> {
    let path = webdav_href_path(href);
    let path = path.trim_start_matches('/');
    let Some(rest) = path
        .strip_prefix(root)
        .and_then(|path| path.strip_prefix('/'))
    else {
        return Ok(None);
    };
    webdav_resource_path(rest, suffix)
}

fn webdav_destination_resource_path(
    headers: &HeaderMap,
    root: &'static str,
    suffix: &'static str,
) -> HostedHttpResult<(String, String)> {
    let Some(destination) = headers.get("destination") else {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            Code::InvalidArgument,
            "WebDAV MOVE requires a Destination header",
        )
        .into());
    };
    let destination = destination.to_str().map_err(|_| {
        HostedHttpError::from(error_response(
            StatusCode::BAD_REQUEST,
            Code::InvalidArgument,
            "invalid WebDAV Destination header",
        ))
    })?;
    match webdav_href_resource_path(destination, root, suffix) {
        Ok(Some(path)) => Ok(path),
        Ok(None) => Err(error_response(
            StatusCode::BAD_GATEWAY,
            Code::InvalidArgument,
            "WebDAV Destination must target the same hosted DAV service",
        )
        .into()),
        Err(()) => Err(webdav_invalid_path_response().into()),
    }
}

fn webdav_href_path(href: &str) -> &str {
    let href = href.split_once('?').map_or(href, |(path, _)| path);
    if let Some(after_scheme) = href.split_once("://").map(|(_, rest)| rest) {
        return after_scheme
            .find('/')
            .map_or("/", |slash| &after_scheme[slash..]);
    }
    href
}

fn webdav_overwrite(headers: &HeaderMap) -> HostedHttpResult<bool> {
    let Some(overwrite) = headers.get("overwrite") else {
        return Ok(true);
    };
    let overwrite = overwrite.to_str().map_err(|_| {
        HostedHttpError::from(error_response(
            StatusCode::BAD_REQUEST,
            Code::InvalidArgument,
            "invalid WebDAV Overwrite header",
        ))
    })?;
    match overwrite {
        value if value.eq_ignore_ascii_case("T") => Ok(true),
        value if value.eq_ignore_ascii_case("F") => Ok(false),
        _ => Err(error_response(
            StatusCode::BAD_REQUEST,
            Code::InvalidArgument,
            "invalid WebDAV Overwrite header",
        )
        .into()),
    }
}

fn percent_decode_path(value: &str) -> Result<String, ()> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                if i + 2 >= bytes.len() {
                    return Err(());
                }
                let hi = hex_nibble(bytes[i + 1]);
                let lo = hex_nibble(bytes[i + 2]);
                if let (Some(hi), Some(lo)) = (hi, lo) {
                    out.push((hi << 4) | lo);
                    i += 3;
                } else {
                    return Err(());
                }
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8(out).map_err(|_| ())
}

fn webdav_invalid_path_response() -> Response {
    error_response(
        StatusCode::BAD_REQUEST,
        Code::InvalidArgument,
        "invalid WebDAV path segment",
    )
}

fn webdav_head_response(response: Response) -> Response {
    let (parts, _) = response.into_parts();
    Response::from_parts(parts, Body::empty())
}

fn webdav_method_not_allowed(allow: &'static str, message: &str) -> Response {
    let mut response = error_response(
        StatusCode::METHOD_NOT_ALLOWED,
        Code::InvalidArgument,
        message,
    );
    response
        .headers_mut()
        .insert(ALLOW, HeaderValue::from_static(allow));
    response
}

async fn webdav_request_body(req: Request, limit: usize) -> HostedHttpResult<Bytes> {
    to_bytes(req.into_body(), limit).await.map_err(|_| {
        error_response(
            StatusCode::BAD_REQUEST,
            Code::InvalidArgument,
            "invalid WebDAV request body",
        )
        .into()
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WebDavDepth {
    Zero,
    One,
}

fn webdav_depth(headers: &HeaderMap) -> HostedHttpResult<WebDavDepth> {
    let Some(value) = headers.get("depth") else {
        return Ok(WebDavDepth::One);
    };
    let value = value
        .to_str()
        .map_err(|_| webdav_invalid_depth_response())?;
    match value.trim().to_ascii_lowercase().as_str() {
        "0" => Ok(WebDavDepth::Zero),
        "1" => Ok(WebDavDepth::One),
        "infinity" => Err(error_response(
            StatusCode::FORBIDDEN,
            Code::Unsupported,
            "WebDAV Depth infinity is not supported",
        )
        .into()),
        _ => Err(webdav_invalid_depth_response()),
    }
}

fn webdav_invalid_depth_response() -> HostedHttpError {
    error_response(
        StatusCode::BAD_REQUEST,
        Code::InvalidArgument,
        "invalid WebDAV Depth header",
    )
    .into()
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum WebDavEtagCondition {
    Any,
    NoEtag,
    Tag(String),
}

fn webdav_check_preconditions(
    headers: &HeaderMap,
    current_etag: Option<&str>,
) -> Result<(), LoomError> {
    let if_match = webdav_etag_conditions(headers, "if-match")?;
    if !if_match.is_empty() && !webdav_etag_matches(&if_match, current_etag) {
        return Err(LoomError::new(
            Code::CasMismatch,
            "WebDAV If-Match precondition failed",
        ));
    }

    let if_none_match = webdav_etag_conditions(headers, "if-none-match")?;
    if !if_none_match.is_empty() && webdav_etag_matches(&if_none_match, current_etag) {
        return Err(LoomError::new(
            Code::CasMismatch,
            "WebDAV If-None-Match precondition failed",
        ));
    }

    Ok(())
}

fn webdav_etag_conditions(
    headers: &HeaderMap,
    name: &'static str,
) -> Result<Vec<WebDavEtagCondition>, LoomError> {
    let mut conditions = Vec::new();
    for value in headers.get_all(name) {
        let value = value.to_str().map_err(|_| {
            LoomError::invalid(format!("invalid WebDAV precondition header {name}"))
        })?;
        for token in value.split(',') {
            conditions.push(webdav_etag_condition(name, token)?);
        }
    }
    Ok(conditions)
}

fn webdav_etag_condition(
    name: &'static str,
    token: &str,
) -> Result<WebDavEtagCondition, LoomError> {
    let token = token.trim();
    if token.is_empty() {
        return Err(LoomError::invalid(format!(
            "invalid WebDAV precondition header {name}"
        )));
    }
    if token == "*" {
        return Ok(WebDavEtagCondition::Any);
    }
    if token == "_NO_ETAG_" {
        return Ok(WebDavEtagCondition::NoEtag);
    }
    if token
        .get(..2)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("W/"))
    {
        return Err(LoomError::invalid(format!(
            "invalid WebDAV precondition header {name}: weak ETags are not accepted for writes"
        )));
    }
    let tag = if token.starts_with('"') || token.ends_with('"') {
        token
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
            .ok_or_else(|| {
                LoomError::invalid(format!("invalid WebDAV precondition header {name}"))
            })?
    } else {
        token
    };
    if tag.is_empty()
        || tag
            .bytes()
            .any(|byte| byte.is_ascii_control() || matches!(byte, b'"' | b','))
    {
        return Err(LoomError::invalid(format!(
            "invalid WebDAV precondition header {name}"
        )));
    }
    Ok(WebDavEtagCondition::Tag(tag.to_string()))
}

fn webdav_etag_matches(conditions: &[WebDavEtagCondition], current_etag: Option<&str>) -> bool {
    conditions.iter().any(|condition| match condition {
        WebDavEtagCondition::Any => current_etag.is_some(),
        WebDavEtagCondition::NoEtag => current_etag.is_none(),
        WebDavEtagCondition::Tag(tag) => current_etag.is_some_and(|etag| tag == etag),
    })
}

fn webdav_write_error_response(err: LoomError) -> Response {
    match err.code {
        Code::CasMismatch => loom_error_response(StatusCode::PRECONDITION_FAILED, err),
        Code::InvalidArgument
            if err
                .message
                .starts_with("invalid WebDAV precondition header") =>
        {
            loom_error_response(StatusCode::BAD_REQUEST, err)
        }
        _ => loom_error_response(StatusCode::FORBIDDEN, err),
    }
}

fn webdav_propfind_request(body: &[u8]) -> HostedHttpResult<()> {
    if body.is_empty() {
        return Ok(());
    }
    let xml = std::str::from_utf8(body).map_err(|_| {
        HostedHttpError::from(error_response(
            StatusCode::BAD_REQUEST,
            Code::InvalidArgument,
            "WebDAV request body must be UTF-8 XML",
        ))
    })?;
    if contains_ascii_case_insensitive(xml, "<!doctype")
        || contains_ascii_case_insensitive(xml, "<!entity")
    {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            Code::InvalidArgument,
            "WebDAV XML must not contain DTD or entity declarations",
        )
        .into());
    }
    if !contains_ascii_case_insensitive(xml, "propfind") {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            Code::InvalidArgument,
            "WebDAV PROPFIND body must contain propfind",
        )
        .into());
    }
    if webdav_has_element(xml, "allprop")
        || webdav_has_element(xml, "propname")
        || webdav_has_element(xml, "prop")
    {
        return Ok(());
    }
    Err(error_response(
        StatusCode::BAD_REQUEST,
        Code::InvalidArgument,
        "unsupported WebDAV PROPFIND body",
    )
    .into())
}

#[derive(Clone, Copy)]
enum WebDavPropertyProfile {
    Calendar,
    AddressBook,
}

struct WebDavPropertyUpdate {
    display_name: Option<String>,
    properties: Vec<WebDavUpdatedProperty>,
}

#[derive(Clone, Debug, Default)]
struct CalDavMkCalendarProperties {
    display_name: Option<String>,
    component_set: Vec<calendar::Component>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WebDavUpdatedProperty {
    DisplayName,
    CalendarColor,
    CalendarOrder,
}

fn caldav_mkcalendar_properties(body: &[u8]) -> HostedHttpResult<CalDavMkCalendarProperties> {
    if body.is_empty() {
        return Ok(CalDavMkCalendarProperties::default());
    }
    let xml = std::str::from_utf8(body).map_err(|_| {
        HostedHttpError::from(error_response(
            StatusCode::BAD_REQUEST,
            Code::InvalidArgument,
            "WebDAV request body must be UTF-8 XML",
        ))
    })?;
    if contains_ascii_case_insensitive(xml, "<!doctype")
        || contains_ascii_case_insensitive(xml, "<!entity")
    {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            Code::InvalidArgument,
            "WebDAV XML must not contain DTD or entity declarations",
        )
        .into());
    }
    let mut reader = XmlReader::from_str(xml);
    reader.config_mut().trim_text(false);
    let decoder = reader.decoder();
    let mut root = None;
    let mut in_set = false;
    let mut in_prop = false;
    let mut in_display_name = false;
    let mut display_name_text = String::new();
    let mut in_component_set = false;
    let mut properties = CalDavMkCalendarProperties::default();
    loop {
        match reader.read_event() {
            Ok(XmlEvent::Start(element)) => {
                let raw_name = element.name();
                let name = xml_local_name(raw_name.as_ref());
                if root.is_none() {
                    root = Some(xml_name_string(name)?);
                }
                if name.eq_ignore_ascii_case(b"set") {
                    in_set = true;
                } else if name.eq_ignore_ascii_case(b"prop") {
                    in_prop = true;
                } else if in_set && in_prop && name.eq_ignore_ascii_case(b"displayname") {
                    in_display_name = true;
                    display_name_text.clear();
                } else if in_set
                    && in_prop
                    && name.eq_ignore_ascii_case(b"supported-calendar-component-set")
                {
                    in_component_set = true;
                } else if in_component_set && name.eq_ignore_ascii_case(b"comp") {
                    push_mkcalendar_component(&mut properties.component_set, &element, decoder)?;
                }
            }
            Ok(XmlEvent::Empty(element)) => {
                let raw_name = element.name();
                let name = xml_local_name(raw_name.as_ref());
                if root.is_none() {
                    root = Some(xml_name_string(name)?);
                }
                if in_component_set && name.eq_ignore_ascii_case(b"comp") {
                    push_mkcalendar_component(&mut properties.component_set, &element, decoder)?;
                }
            }
            Ok(XmlEvent::End(element)) => {
                let raw_name = element.name();
                let name = xml_local_name(raw_name.as_ref());
                if name.eq_ignore_ascii_case(b"set") {
                    in_set = false;
                } else if name.eq_ignore_ascii_case(b"prop") {
                    in_prop = false;
                } else if name.eq_ignore_ascii_case(b"displayname") {
                    in_display_name = false;
                    properties.display_name = Some(display_name_text.clone());
                    display_name_text.clear();
                } else if name.eq_ignore_ascii_case(b"supported-calendar-component-set") {
                    in_component_set = false;
                }
            }
            Ok(XmlEvent::Text(text)) => {
                if in_display_name {
                    display_name_text.push_str(
                        &text
                            .decode()
                            .map_err(|_| mkcalendar_invalid_xml_response("text-decode"))?,
                    );
                }
            }
            Ok(XmlEvent::CData(text)) => {
                if in_display_name {
                    display_name_text.push_str(
                        &text
                            .decode()
                            .map_err(|_| mkcalendar_invalid_xml_response("cdata-decode"))?,
                    );
                }
            }
            Ok(XmlEvent::DocType(_)) => {
                return Err(mkcalendar_invalid_xml_response("doctype-or-reference"));
            }
            Ok(XmlEvent::GeneralRef(reference)) => {
                if in_display_name {
                    let text = xml_general_ref_text(reference.as_ref())?
                        .ok_or_else(|| mkcalendar_invalid_xml_response("unknown-reference"))?;
                    display_name_text.push_str(&text);
                }
            }
            Ok(XmlEvent::Eof) => break,
            Ok(_) => {}
            Err(_) => return Err(mkcalendar_invalid_xml_response("xml-reader")),
        }
    }
    match root.as_deref() {
        Some("mkcalendar" | "mkcol" | "propertyupdate") => Ok(properties),
        _ => Err(error_response(
            StatusCode::BAD_REQUEST,
            Code::InvalidArgument,
            "CalDAV MKCALENDAR body must contain mkcalendar-compatible creation properties",
        )
        .into()),
    }
}

fn push_mkcalendar_component(
    components: &mut Vec<calendar::Component>,
    element: &XmlBytesStart<'_>,
    decoder: XmlDecoder,
) -> HostedHttpResult<()> {
    let Some(name) = xml_attr(element, decoder, b"name")? else {
        return Ok(());
    };
    let component = match name.as_str() {
        value if value.eq_ignore_ascii_case("VEVENT") => calendar::Component::Event,
        value if value.eq_ignore_ascii_case("VTODO") => calendar::Component::Todo,
        _ => return Ok(()),
    };
    if !components.contains(&component) {
        components.push(component);
    }
    Ok(())
}

fn mkcalendar_invalid_xml_response(_reason: &'static str) -> HostedHttpError {
    webdav_invalid_xml_response()
}

fn webdav_property_update(
    body: &[u8],
    profile: WebDavPropertyProfile,
) -> HostedHttpResult<WebDavPropertyUpdate> {
    if body.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            Code::InvalidArgument,
            "WebDAV PROPPATCH body is required",
        )
        .into());
    }
    let xml = std::str::from_utf8(body).map_err(|_| {
        HostedHttpError::from(error_response(
            StatusCode::BAD_REQUEST,
            Code::InvalidArgument,
            "WebDAV request body must be UTF-8 XML",
        ))
    })?;
    if contains_ascii_case_insensitive(xml, "<!doctype")
        || contains_ascii_case_insensitive(xml, "<!entity")
    {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            Code::InvalidArgument,
            "WebDAV XML must not contain DTD or entity declarations",
        )
        .into());
    }
    let mut reader = XmlReader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut root = None;
    let mut in_set = false;
    let mut in_prop = false;
    let mut current_property = None;
    let mut display_name = None;
    let mut properties = Vec::new();
    loop {
        match reader.read_event() {
            Ok(XmlEvent::Start(element)) => {
                let raw_name = element.name();
                let name = xml_local_name(raw_name.as_ref());
                if root.is_none() {
                    root = Some(xml_name_string(name)?);
                }
                if name.eq_ignore_ascii_case(b"set") {
                    in_set = true;
                } else if name.eq_ignore_ascii_case(b"prop") {
                    in_prop = true;
                } else if in_set && in_prop {
                    let property = webdav_updated_property(name, profile)?;
                    push_unique_property(&mut properties, property);
                    current_property = Some(property);
                }
            }
            Ok(XmlEvent::Empty(element)) => {
                let raw_name = element.name();
                let name = xml_local_name(raw_name.as_ref());
                if root.is_none() {
                    root = Some(xml_name_string(name)?);
                }
                if in_set && in_prop {
                    let property = webdav_updated_property(name, profile)?;
                    push_unique_property(&mut properties, property);
                }
            }
            Ok(XmlEvent::End(element)) => {
                let raw_name = element.name();
                let name = xml_local_name(raw_name.as_ref());
                if name.eq_ignore_ascii_case(b"set") {
                    in_set = false;
                } else if name.eq_ignore_ascii_case(b"prop") {
                    in_prop = false;
                } else if current_property.is_some() {
                    current_property = None;
                }
            }
            Ok(XmlEvent::Text(text)) => {
                if current_property == Some(WebDavUpdatedProperty::DisplayName) {
                    display_name = Some(
                        text.decode()
                            .map_err(|_| webdav_invalid_xml_response())?
                            .into_owned(),
                    );
                }
            }
            Ok(XmlEvent::CData(text)) => {
                if current_property == Some(WebDavUpdatedProperty::DisplayName) {
                    display_name = Some(
                        text.decode()
                            .map_err(|_| webdav_invalid_xml_response())?
                            .into_owned(),
                    );
                }
            }
            Ok(XmlEvent::DocType(_) | XmlEvent::GeneralRef(_)) => {
                return Err(webdav_invalid_xml_response());
            }
            Ok(XmlEvent::Eof) => break,
            Ok(_) => {}
            Err(_) => return Err(webdav_invalid_xml_response()),
        }
    }
    if root.as_deref() != Some("propertyupdate") || properties.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            Code::InvalidArgument,
            "WebDAV PROPPATCH body must contain propertyupdate set props",
        )
        .into());
    }
    Ok(WebDavPropertyUpdate {
        display_name,
        properties,
    })
}

fn webdav_updated_property(
    name: &[u8],
    profile: WebDavPropertyProfile,
) -> HostedHttpResult<WebDavUpdatedProperty> {
    if name.eq_ignore_ascii_case(b"displayname") {
        return Ok(WebDavUpdatedProperty::DisplayName);
    }
    if matches!(profile, WebDavPropertyProfile::Calendar)
        && name.eq_ignore_ascii_case(b"calendar-color")
    {
        return Ok(WebDavUpdatedProperty::CalendarColor);
    }
    if matches!(profile, WebDavPropertyProfile::Calendar)
        && name.eq_ignore_ascii_case(b"calendar-order")
    {
        return Ok(WebDavUpdatedProperty::CalendarOrder);
    }
    Err(error_response(
        StatusCode::FORBIDDEN,
        Code::Unsupported,
        "unsupported WebDAV PROPPATCH property",
    )
    .into())
}

fn push_unique_property(
    properties: &mut Vec<WebDavUpdatedProperty>,
    property: WebDavUpdatedProperty,
) {
    if !properties.contains(&property) {
        properties.push(property);
    }
}

fn webdav_report_request(body: &[u8]) -> HostedHttpResult<WebDavReportRequest> {
    if body.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            Code::InvalidArgument,
            "CalDAV REPORT body is required",
        )
        .into());
    }
    let xml = std::str::from_utf8(body).map_err(|_| {
        HostedHttpError::from(error_response(
            StatusCode::BAD_REQUEST,
            Code::InvalidArgument,
            "WebDAV request body must be UTF-8 XML",
        ))
    })?;
    if contains_ascii_case_insensitive(xml, "<!doctype")
        || contains_ascii_case_insensitive(xml, "<!entity")
    {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            Code::InvalidArgument,
            "WebDAV XML must not contain DTD or entity declarations",
        )
        .into());
    }

    let mut reader = XmlReader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut root = None;
    let mut in_href = false;
    let mut caldav_in_text_match = false;
    let mut in_sync_token = false;
    let mut caldav_prop_filter: Option<String> = None;
    let mut carddav_prop_filter: Option<CardDavPropFilter> = None;
    let mut carddav_param_filter: Option<CardDavParamFilter> = None;
    let mut carddav_text_target: Option<CardDavTextTarget> = None;
    let mut hrefs = Vec::new();
    let mut sync_token = None;
    let mut query = CalDavCalendarQuery::default();
    let mut addressbook_query = CardDavAddressbookQuery::default();
    let mut addressbook_data_version = CardDavDataVersion::default();
    loop {
        match reader.read_event() {
            Ok(XmlEvent::Start(element)) => {
                let raw_name = element.name();
                let name = xml_local_name(raw_name.as_ref());
                if root.is_none() {
                    root = Some(xml_name_string(name)?);
                }
                in_href = name.eq_ignore_ascii_case(b"href");
                in_sync_token = name.eq_ignore_ascii_case(b"sync-token");
                match root.as_deref() {
                    Some("calendar-query") => caldav_report_start(
                        name,
                        &element,
                        reader.decoder(),
                        &mut query,
                        &mut caldav_prop_filter,
                        &mut caldav_in_text_match,
                    )?,
                    Some("addressbook-query") => carddav_report_start(
                        name,
                        &element,
                        reader.decoder(),
                        &mut carddav_prop_filter,
                        &mut carddav_param_filter,
                        &mut carddav_text_target,
                        &mut addressbook_query.version,
                    )?,
                    Some("addressbook-multiget") if name.eq_ignore_ascii_case(b"address-data") => {
                        addressbook_data_version =
                            carddav_address_data_version(&element, reader.decoder())?;
                    }
                    _ => {}
                }
            }
            Ok(XmlEvent::Empty(element)) => {
                let raw_name = element.name();
                let name = xml_local_name(raw_name.as_ref());
                if root.is_none() {
                    root = Some(xml_name_string(name)?);
                }
                match root.as_deref() {
                    Some("calendar-query") => {
                        caldav_report_empty(name, &element, reader.decoder(), &mut query)?
                    }
                    Some("addressbook-query") => carddav_report_empty(
                        name,
                        &element,
                        reader.decoder(),
                        &mut addressbook_query,
                        &mut carddav_prop_filter,
                    )?,
                    Some("addressbook-multiget") if name.eq_ignore_ascii_case(b"address-data") => {
                        addressbook_data_version =
                            carddav_address_data_version(&element, reader.decoder())?;
                    }
                    _ => {}
                }
            }
            Ok(XmlEvent::End(element)) => {
                let raw_name = element.name();
                let name = xml_local_name(raw_name.as_ref());
                if name.eq_ignore_ascii_case(b"href") {
                    in_href = false;
                }
                if name.eq_ignore_ascii_case(b"text-match") {
                    caldav_in_text_match = false;
                    carddav_text_target = None;
                }
                if name.eq_ignore_ascii_case(b"sync-token") {
                    in_sync_token = false;
                }
                match root.as_deref() {
                    Some("calendar-query") if name.eq_ignore_ascii_case(b"prop-filter") => {
                        caldav_prop_filter = None;
                    }
                    Some("addressbook-query") => carddav_report_end(
                        name,
                        &mut addressbook_query,
                        &mut carddav_prop_filter,
                        &mut carddav_param_filter,
                    )?,
                    _ => {}
                }
            }
            Ok(XmlEvent::Text(text)) if in_href => {
                hrefs.push(
                    text.decode()
                        .map_err(|_| webdav_invalid_xml_response())?
                        .into_owned(),
                );
            }
            Ok(XmlEvent::Text(text)) if caldav_in_text_match => {
                caldav_apply_text_match(
                    caldav_prop_filter.as_deref(),
                    text.decode()
                        .map_err(|_| webdav_invalid_xml_response())?
                        .as_ref(),
                    &mut query,
                )?;
            }
            Ok(XmlEvent::Text(text)) if carddav_text_target.is_some() => {
                carddav_apply_text_match(
                    carddav_text_target,
                    text.decode()
                        .map_err(|_| webdav_invalid_xml_response())?
                        .as_ref(),
                    &mut carddav_prop_filter,
                    &mut carddav_param_filter,
                )?;
            }
            Ok(XmlEvent::Text(text)) if in_sync_token => {
                sync_token = Some(
                    text.decode()
                        .map_err(|_| webdav_invalid_xml_response())?
                        .into_owned(),
                );
            }
            Ok(XmlEvent::CData(text)) if in_href => {
                hrefs.push(
                    text.decode()
                        .map_err(|_| webdav_invalid_xml_response())?
                        .into_owned(),
                );
            }
            Ok(XmlEvent::CData(text)) if caldav_in_text_match => {
                caldav_apply_text_match(
                    caldav_prop_filter.as_deref(),
                    text.decode()
                        .map_err(|_| webdav_invalid_xml_response())?
                        .as_ref(),
                    &mut query,
                )?;
            }
            Ok(XmlEvent::CData(text)) if carddav_text_target.is_some() => {
                carddav_apply_text_match(
                    carddav_text_target,
                    text.decode()
                        .map_err(|_| webdav_invalid_xml_response())?
                        .as_ref(),
                    &mut carddav_prop_filter,
                    &mut carddav_param_filter,
                )?;
            }
            Ok(XmlEvent::CData(text)) if in_sync_token => {
                sync_token = Some(
                    text.decode()
                        .map_err(|_| webdav_invalid_xml_response())?
                        .into_owned(),
                );
            }
            Ok(XmlEvent::DocType(_) | XmlEvent::GeneralRef(_)) => {
                return Err(webdav_invalid_xml_response());
            }
            Ok(XmlEvent::Eof) => break,
            Ok(_) => {}
            Err(_) => return Err(webdav_invalid_xml_response()),
        }
    }

    match root.as_deref() {
        Some("calendar-multiget") => Ok(WebDavReportRequest::CalendarMultiget { hrefs }),
        Some("addressbook-multiget") => Ok(WebDavReportRequest::AddressbookMultiget {
            hrefs,
            version: addressbook_data_version,
        }),
        Some("addressbook-query") => Ok(WebDavReportRequest::AddressbookQuery(addressbook_query)),
        Some("calendar-query") => Ok(WebDavReportRequest::CalendarQuery(query)),
        Some("sync-collection") => Ok(WebDavReportRequest::SyncCollection { sync_token }),
        Some(name) => Ok(WebDavReportRequest::Unsupported(name.to_string())),
        None => Err(error_response(
            StatusCode::BAD_REQUEST,
            Code::InvalidArgument,
            "CalDAV REPORT body must contain a report element",
        )
        .into()),
    }
}

fn caldav_report_start(
    name: &[u8],
    element: &XmlBytesStart<'_>,
    decoder: XmlDecoder,
    query: &mut CalDavCalendarQuery,
    prop_filter: &mut Option<String>,
    in_text_match: &mut bool,
) -> HostedHttpResult<()> {
    if name.eq_ignore_ascii_case(b"comp-filter") {
        caldav_apply_comp_filter(xml_attr(element, decoder, b"name")?.as_deref(), query)?;
    } else if name.eq_ignore_ascii_case(b"time-range") {
        caldav_apply_time_range(element, decoder, query)?;
    } else if name.eq_ignore_ascii_case(b"prop-filter") {
        let Some(name) = xml_attr(element, decoder, b"name")? else {
            return Err(caldav_unsupported_filter_response().into());
        };
        let name = name.to_ascii_uppercase();
        if !matches!(name.as_str(), "UID" | "SUMMARY") {
            return Err(caldav_unsupported_filter_response().into());
        }
        *prop_filter = Some(name);
    } else if name.eq_ignore_ascii_case(b"text-match") {
        *in_text_match = true;
    }
    Ok(())
}

fn caldav_report_empty(
    name: &[u8],
    element: &XmlBytesStart<'_>,
    decoder: XmlDecoder,
    query: &mut CalDavCalendarQuery,
) -> HostedHttpResult<()> {
    if name.eq_ignore_ascii_case(b"comp-filter") {
        caldav_apply_comp_filter(xml_attr(element, decoder, b"name")?.as_deref(), query)?;
    } else if name.eq_ignore_ascii_case(b"time-range") {
        caldav_apply_time_range(element, decoder, query)?;
    } else if name.eq_ignore_ascii_case(b"prop-filter") {
        let Some(name) = xml_attr(element, decoder, b"name")? else {
            return Err(caldav_unsupported_filter_response().into());
        };
        if !matches!(name.to_ascii_uppercase().as_str(), "UID" | "SUMMARY") {
            return Err(caldav_unsupported_filter_response().into());
        }
    }
    Ok(())
}

fn carddav_report_start(
    name: &[u8],
    element: &XmlBytesStart<'_>,
    decoder: XmlDecoder,
    prop_filter: &mut Option<CardDavPropFilter>,
    param_filter: &mut Option<CardDavParamFilter>,
    text_target: &mut Option<CardDavTextTarget>,
    version: &mut CardDavDataVersion,
) -> HostedHttpResult<()> {
    if name.eq_ignore_ascii_case(b"address-data") {
        *version = carddav_address_data_version(element, decoder)?;
    } else if name.eq_ignore_ascii_case(b"prop-filter") {
        let Some(name) = xml_attr(element, decoder, b"name")? else {
            return Err(carddav_unsupported_filter_response().into());
        };
        let name = carddav_filter_name(&name)?;
        *prop_filter = Some(CardDavPropFilter {
            name,
            text: None,
            param_filters: Vec::new(),
        });
    } else if name.eq_ignore_ascii_case(b"param-filter") {
        if prop_filter.is_none() {
            return Err(carddav_unsupported_filter_response().into());
        }
        let Some(name) = xml_attr(element, decoder, b"name")? else {
            return Err(carddav_unsupported_filter_response().into());
        };
        let name = name.to_ascii_uppercase();
        if name != "TYPE" {
            return Err(carddav_unsupported_filter_response().into());
        }
        *param_filter = Some(CardDavParamFilter { name, text: None });
    } else if name.eq_ignore_ascii_case(b"text-match") {
        *text_target = Some(if param_filter.is_some() {
            CardDavTextTarget::Param
        } else if prop_filter.is_some() {
            CardDavTextTarget::Prop
        } else {
            return Err(carddav_unsupported_filter_response().into());
        });
    }
    Ok(())
}

fn carddav_report_empty(
    name: &[u8],
    element: &XmlBytesStart<'_>,
    decoder: XmlDecoder,
    query: &mut CardDavAddressbookQuery,
    prop_filter: &mut Option<CardDavPropFilter>,
) -> HostedHttpResult<()> {
    if name.eq_ignore_ascii_case(b"address-data") {
        query.version = carddav_address_data_version(element, decoder)?;
    } else if name.eq_ignore_ascii_case(b"prop-filter") {
        let Some(name) = xml_attr(element, decoder, b"name")? else {
            return Err(carddav_unsupported_filter_response().into());
        };
        query.filters.push(CardDavPropFilter {
            name: carddav_filter_name(&name)?,
            text: None,
            param_filters: Vec::new(),
        });
    } else if name.eq_ignore_ascii_case(b"param-filter") {
        let Some(filter) = prop_filter.as_mut() else {
            return Err(carddav_unsupported_filter_response().into());
        };
        let Some(name) = xml_attr(element, decoder, b"name")? else {
            return Err(carddav_unsupported_filter_response().into());
        };
        let name = name.to_ascii_uppercase();
        if name != "TYPE" {
            return Err(carddav_unsupported_filter_response().into());
        }
        filter
            .param_filters
            .push(CardDavParamFilter { name, text: None });
    }
    Ok(())
}

fn carddav_report_end(
    name: &[u8],
    query: &mut CardDavAddressbookQuery,
    prop_filter: &mut Option<CardDavPropFilter>,
    param_filter: &mut Option<CardDavParamFilter>,
) -> HostedHttpResult<()> {
    if name.eq_ignore_ascii_case(b"param-filter") {
        let Some(param) = param_filter.take() else {
            return Err(carddav_unsupported_filter_response().into());
        };
        let Some(filter) = prop_filter.as_mut() else {
            return Err(carddav_unsupported_filter_response().into());
        };
        filter.param_filters.push(param);
    } else if name.eq_ignore_ascii_case(b"prop-filter") {
        let Some(filter) = prop_filter.take() else {
            return Err(carddav_unsupported_filter_response().into());
        };
        query.filters.push(filter);
    }
    Ok(())
}

fn carddav_filter_name(name: &str) -> HostedHttpResult<String> {
    let name = name.to_ascii_uppercase();
    if matches!(
        name.as_str(),
        "UID" | "FN" | "N" | "ORG" | "TITLE" | "EMAIL" | "TEL"
    ) {
        Ok(name)
    } else {
        Err(carddav_unsupported_filter_response().into())
    }
}

fn carddav_address_data_version(
    element: &XmlBytesStart<'_>,
    decoder: XmlDecoder,
) -> HostedHttpResult<CardDavDataVersion> {
    if let Some(content_type) = xml_attr(element, decoder, b"content-type")?
        && !content_type.eq_ignore_ascii_case("text/vcard")
    {
        return Err(error_response(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Code::Unsupported,
            "unsupported CardDAV address-data content type",
        )
        .into());
    }
    match xml_attr(element, decoder, b"version")?.as_deref() {
        Some("4.0") => Ok(CardDavDataVersion::V4),
        Some("3.0") | None => Ok(CardDavDataVersion::V3),
        Some(_) => Err(error_response(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Code::Unsupported,
            "unsupported CardDAV address-data version",
        )
        .into()),
    }
}

fn carddav_apply_text_match(
    target: Option<CardDavTextTarget>,
    text: &str,
    prop_filter: &mut Option<CardDavPropFilter>,
    param_filter: &mut Option<CardDavParamFilter>,
) -> HostedHttpResult<()> {
    let text = text.to_lowercase();
    match target {
        Some(CardDavTextTarget::Prop) => {
            let Some(filter) = prop_filter.as_mut() else {
                return Err(carddav_unsupported_filter_response().into());
            };
            filter.text = Some(text);
        }
        Some(CardDavTextTarget::Param) => {
            let Some(filter) = param_filter.as_mut() else {
                return Err(carddav_unsupported_filter_response().into());
            };
            filter.text = Some(text);
        }
        None => return Err(carddav_unsupported_filter_response().into()),
    }
    Ok(())
}

fn caldav_apply_comp_filter(
    name: Option<&str>,
    query: &mut CalDavCalendarQuery,
) -> HostedHttpResult<()> {
    match name.map(str::to_ascii_uppercase).as_deref() {
        Some("VCALENDAR") | None => Ok(()),
        Some("VEVENT") => {
            query.component = Some(calendar::Component::Event);
            Ok(())
        }
        Some("VTODO") => {
            query.component = Some(calendar::Component::Todo);
            Ok(())
        }
        Some(_) => Err(caldav_unsupported_filter_response().into()),
    }
}

fn caldav_apply_time_range(
    element: &XmlBytesStart<'_>,
    decoder: XmlDecoder,
    query: &mut CalDavCalendarQuery,
) -> HostedHttpResult<()> {
    let Some(start) = xml_attr(element, decoder, b"start")? else {
        return Err(caldav_unsupported_filter_response().into());
    };
    let Some(end) = xml_attr(element, decoder, b"end")? else {
        return Err(caldav_unsupported_filter_response().into());
    };
    let start = parse_caldav_time(&start)
        .ok_or_else(|| HostedHttpError::from(caldav_unsupported_filter_response()))?;
    let end = parse_caldav_time(&end)
        .ok_or_else(|| HostedHttpError::from(caldav_unsupported_filter_response()))?;
    query.time_range = Some((start, end));
    Ok(())
}

fn caldav_apply_text_match(
    prop_filter: Option<&str>,
    text: &str,
    query: &mut CalDavCalendarQuery,
) -> HostedHttpResult<()> {
    let text = text.to_lowercase();
    match prop_filter {
        Some("UID") => query.uid_text = Some(text),
        Some("SUMMARY") => query.summary_text = Some(text),
        _ => return Err(caldav_unsupported_filter_response().into()),
    }
    Ok(())
}

fn xml_attr(
    element: &XmlBytesStart<'_>,
    decoder: XmlDecoder,
    name: &[u8],
) -> HostedHttpResult<Option<String>> {
    for attr in element.attributes().with_checks(false) {
        let attr = attr.map_err(|_| webdav_invalid_xml_response())?;
        if xml_local_name(attr.key.as_ref()).eq_ignore_ascii_case(name) {
            return attr
                .decoded_and_normalized_value(XmlVersion::Implicit1_0, decoder)
                .map(|value| Some(value.into_owned()))
                .map_err(|_| webdav_invalid_xml_response());
        }
    }
    Ok(None)
}

fn parse_caldav_time(value: &str) -> Option<calendar::DateTime> {
    let core = value.strip_suffix('Z').unwrap_or(value);
    let (date_part, time_part) = core.split_once('T')?;
    if date_part.len() != 8
        || time_part.len() != 6
        || !date_part.bytes().all(|byte| byte.is_ascii_digit())
        || !time_part.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let date = calendar::IcalDate::from_calendar_date(
        date_part[0..4].parse().ok()?,
        calendar::IcalMonth::try_from(date_part[4..6].parse::<u8>().ok()?).ok()?,
        date_part[6..8].parse().ok()?,
    )
    .ok()?;
    let time = calendar::IcalTime::from_hms(
        time_part[0..2].parse().ok()?,
        time_part[2..4].parse().ok()?,
        time_part[4..6].parse().ok()?,
    )
    .ok()?;
    Some(calendar::DateTime::new(date, time))
}

fn caldav_unsupported_filter_response() -> Response {
    error_response(
        StatusCode::NOT_IMPLEMENTED,
        Code::Unsupported,
        "unsupported CalDAV calendar-query filter",
    )
}

fn carddav_unsupported_filter_response() -> Response {
    error_response(
        StatusCode::NOT_IMPLEMENTED,
        Code::Unsupported,
        "unsupported CardDAV addressbook-query filter",
    )
}

fn xml_local_name(name: &[u8]) -> &[u8] {
    name.iter()
        .rposition(|byte| *byte == b':')
        .map_or(name, |colon| &name[colon + 1..])
}

fn xml_name_string(name: &[u8]) -> HostedHttpResult<String> {
    std::str::from_utf8(name)
        .map(str::to_ascii_lowercase)
        .map_err(|_| webdav_invalid_xml_response())
}

fn xml_general_ref_text(name: &[u8]) -> HostedHttpResult<Option<String>> {
    let name = std::str::from_utf8(name).map_err(|_| webdav_invalid_xml_response())?;
    let text = match name {
        "amp" => Some("&".to_string()),
        "lt" => Some("<".to_string()),
        "gt" => Some(">".to_string()),
        "apos" => Some("'".to_string()),
        "quot" => Some("\"".to_string()),
        value if value.starts_with("#x") || value.starts_with("#X") => {
            let parsed = u32::from_str_radix(&value[2..], 16).ok();
            parsed
                .and_then(char::from_u32)
                .map(|value| value.to_string())
        }
        value if value.starts_with('#') => {
            let parsed = value[1..].parse::<u32>().ok();
            parsed
                .and_then(char::from_u32)
                .map(|value| value.to_string())
        }
        _ => None,
    };
    Ok(text)
}

fn webdav_invalid_xml_response() -> HostedHttpError {
    error_response(
        StatusCode::BAD_REQUEST,
        Code::InvalidArgument,
        "invalid WebDAV XML body",
    )
    .into()
}

fn contains_ascii_case_insensitive(haystack: &str, needle: &str) -> bool {
    haystack
        .as_bytes()
        .windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
}

fn webdav_has_element(xml: &str, local_name: &str) -> bool {
    let bytes = xml.as_bytes();
    let name = local_name.as_bytes();
    let mut index = 0;
    while let Some(offset) = bytes[index..].iter().position(|byte| *byte == b'<') {
        index += offset + 1;
        if matches!(bytes.get(index), Some(b'/') | Some(b'!') | Some(b'?')) {
            continue;
        }
        let start = index;
        while matches!(
            bytes.get(index),
            Some(b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-' | b':')
        ) {
            index += 1;
        }
        let raw = &bytes[start..index];
        let local = raw
            .iter()
            .rposition(|byte| *byte == b':')
            .map_or(raw, |colon| &raw[colon + 1..]);
        if local.eq_ignore_ascii_case(name)
            && matches!(
                bytes.get(index),
                Some(b'/' | b'>' | b' ' | b'\t' | b'\r' | b'\n')
            )
        {
            return true;
        }
    }
    false
}

fn webdav_empty_response(status: StatusCode, dav: &'static str) -> Response {
    (status, [("dav", dav)], Body::empty()).into_response()
}

fn webdav_multistatus(responses: Vec<String>) -> Response {
    let body = format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?><D:multistatus xmlns:D=\"DAV:\" \
         xmlns:C=\"urn:ietf:params:xml:ns:caldav\" \
         xmlns:CS=\"http://calendarserver.org/ns/\" \
         xmlns:CARD=\"urn:ietf:params:xml:ns:carddav\">{}\
         </D:multistatus>",
        responses.join("")
    );
    response(
        StatusCode::MULTI_STATUS,
        "application/xml; charset=utf-8",
        Body::from(body),
    )
}

fn webdav_sync_multistatus(responses: Vec<String>, sync_token: &str) -> Response {
    let mut responses = responses;
    responses.push(format!(
        "<D:sync-token>{}</D:sync-token>",
        xml_escape(sync_token)
    ));
    webdav_multistatus(responses)
}

fn webdav_proppatch_response(href: &str, properties: Vec<WebDavUpdatedProperty>) -> Response {
    let props = properties
        .into_iter()
        .map(webdav_updated_property_xml)
        .collect::<Vec<_>>()
        .join("");
    webdav_multistatus(vec![format!(
        "<D:response><D:href>{}</D:href><D:propstat><D:prop>{}</D:prop>\
         <D:status>HTTP/1.1 200 OK</D:status></D:propstat></D:response>",
        xml_escape(href),
        props
    )])
}

fn webdav_updated_property_xml(property: WebDavUpdatedProperty) -> &'static str {
    match property {
        WebDavUpdatedProperty::DisplayName => "<D:displayname/>",
        WebDavUpdatedProperty::CalendarColor => "<C:calendar-color/>",
        WebDavUpdatedProperty::CalendarOrder => "<CS:calendar-order/>",
    }
}

fn caldav_principal_href(principal: &str) -> String {
    format!("/caldav/principals/{}/", url_segment(principal))
}

fn carddav_principal_href(principal: &str) -> String {
    format!("/carddav/principals/{}/", url_segment(principal))
}

fn webdav_current_user_privilege_set() -> &'static str {
    "<D:current-user-privilege-set><D:privilege><D:read/></D:privilege>\
     <D:privilege><D:write/></D:privilege></D:current-user-privilege-set>"
}

fn calendar_user_address_set(principal: &str) -> String {
    let href = if principal.contains('@') {
        format!("mailto:{principal}")
    } else {
        format!("urn:uuid:{principal}")
    };
    format!(
        "<C:calendar-user-address-set><D:href>{}</D:href></C:calendar-user-address-set>",
        xml_escape(&href)
    )
}

fn caldav_home_response(principal: &str) -> String {
    let principal_href = caldav_principal_href(principal);
    format!(
        "<D:response><D:href>{}</D:href><D:propstat><D:prop>\
         <D:resourcetype><D:collection/></D:resourcetype><D:displayname>Calendar Home</D:displayname>\
         <D:current-user-principal><D:href>{}</D:href></D:current-user-principal>\
         <D:principal-URL><D:href>{}</D:href></D:principal-URL>\
         <D:owner><D:href>{}</D:href></D:owner>{}\
         {}\
         <C:calendar-home-set><D:href>{}</D:href></C:calendar-home-set>\
         </D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat></D:response>",
        xml_escape("/caldav/"),
        xml_escape(&principal_href),
        xml_escape(&principal_href),
        xml_escape(&principal_href),
        webdav_current_user_privilege_set(),
        calendar_user_address_set(principal),
        xml_escape("/caldav/")
    )
}

fn caldav_principal_response(principal: &str) -> String {
    let principal_href = caldav_principal_href(principal);
    format!(
        "<D:response><D:href>{}</D:href><D:propstat><D:prop>\
         <D:resourcetype><D:principal/></D:resourcetype><D:displayname>{}</D:displayname>\
         <D:current-user-principal><D:href>{}</D:href></D:current-user-principal>\
         <D:principal-URL><D:href>{}</D:href></D:principal-URL>\
         <D:owner><D:href>{}</D:href></D:owner>{}{}\
         <C:calendar-home-set><D:href>/caldav/</D:href></C:calendar-home-set>\
         </D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat></D:response>",
        xml_escape(&principal_href),
        xml_escape(principal),
        xml_escape(&principal_href),
        xml_escape(&principal_href),
        xml_escape(&principal_href),
        webdav_current_user_privilege_set(),
        calendar_user_address_set(principal)
    )
}

fn carddav_home_response(principal: &str) -> String {
    let principal_href = carddav_principal_href(principal);
    format!(
        "<D:response><D:href>{}</D:href><D:propstat><D:prop>\
         <D:resourcetype><D:collection/></D:resourcetype><D:displayname>Address Books</D:displayname>\
         <D:current-user-principal><D:href>{}</D:href></D:current-user-principal>\
         <D:principal-URL><D:href>{}</D:href></D:principal-URL>\
         <D:owner><D:href>{}</D:href></D:owner>{}\
         <CARD:addressbook-home-set><D:href>{}</D:href></CARD:addressbook-home-set>\
         </D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat></D:response>",
        xml_escape("/carddav/"),
        xml_escape(&principal_href),
        xml_escape(&principal_href),
        xml_escape(&principal_href),
        webdav_current_user_privilege_set(),
        xml_escape("/carddav/")
    )
}

fn carddav_principal_response(principal: &str) -> String {
    let principal_href = carddav_principal_href(principal);
    format!(
        "<D:response><D:href>{}</D:href><D:propstat><D:prop>\
         <D:resourcetype><D:principal/></D:resourcetype><D:displayname>{}</D:displayname>\
         <D:current-user-principal><D:href>{}</D:href></D:current-user-principal>\
         <D:principal-URL><D:href>{}</D:href></D:principal-URL>\
         <D:owner><D:href>{}</D:href></D:owner>{}\
         <CARD:addressbook-home-set><D:href>/carddav/</D:href></CARD:addressbook-home-set>\
         </D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat></D:response>",
        xml_escape(&principal_href),
        xml_escape(principal),
        xml_escape(&principal_href),
        xml_escape(&principal_href),
        xml_escape(&principal_href),
        webdav_current_user_privilege_set()
    )
}

fn caldav_collection_response(
    href: &str,
    principal: &str,
    display_name: &str,
    components: &[calendar::Component],
    sync_token: &str,
) -> String {
    let principal_href = caldav_principal_href(principal);
    let components = components
        .iter()
        .map(|component| {
            format!(
                "<C:comp name=\"{}\"/>",
                match component {
                    calendar::Component::Event => "VEVENT",
                    calendar::Component::Todo => "VTODO",
                }
            )
        })
        .collect::<Vec<_>>()
        .join("");
    format!(
        "<D:response><D:href>{}</D:href><D:propstat><D:prop>\
         <D:resourcetype><D:collection/><C:calendar/></D:resourcetype>\
         <D:displayname>{}</D:displayname>\
         <D:owner><D:href>{}</D:href></D:owner>{}\
         <D:supported-report-set><D:supported-report><D:report><C:calendar-multiget/>\
         </D:report></D:supported-report><D:supported-report><D:report><C:calendar-query/>\
         </D:report></D:supported-report><D:supported-report><D:report><D:sync-collection/>\
         </D:report></D:supported-report></D:supported-report-set>\
         <C:supported-calendar-component-set>{}</C:supported-calendar-component-set>\
         <C:supported-calendar-data><C:calendar-data content-type=\"text/calendar\" version=\"2.0\"/>\
         </C:supported-calendar-data><C:calendar-color>#3b82f6</C:calendar-color>\
         <D:sync-token>{}</D:sync-token><CS:getctag>{}</CS:getctag>\
         </D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat></D:response>",
        xml_escape(href),
        xml_escape(display_name),
        xml_escape(&principal_href),
        webdav_current_user_privilege_set(),
        components,
        xml_escape(sync_token),
        xml_escape(sync_token)
    )
}

fn webdav_resource_response(href: &str, content_type: &str, etag: &str) -> String {
    format!(
        "<D:response><D:href>{}</D:href><D:propstat><D:prop>\
         <D:resourcetype/><D:getcontenttype>{}</D:getcontenttype><D:getetag>\"{}\"</D:getetag>\
         </D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat></D:response>",
        xml_escape(href),
        xml_escape(content_type),
        xml_escape(etag)
    )
}

fn caldav_resource_report_response(href: &str, etag: &str, calendar_data: &str) -> String {
    format!(
        "<D:response><D:href>{}</D:href><D:propstat><D:prop>\
         <D:getetag>\"{}\"</D:getetag><C:calendar-data>{}</C:calendar-data>\
         </D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat></D:response>",
        xml_escape(href),
        xml_escape(etag),
        xml_escape(calendar_data)
    )
}

fn carddav_addressbook_response(
    href: &str,
    principal: &str,
    display_name: &str,
    sync_token: &str,
) -> String {
    let principal_href = carddav_principal_href(principal);
    format!(
        "<D:response><D:href>{}</D:href><D:propstat><D:prop>\
         <D:resourcetype><D:collection/><CARD:addressbook/></D:resourcetype>\
         <D:displayname>{}</D:displayname>\
         <D:owner><D:href>{}</D:href></D:owner>{}\
         <D:supported-report-set><D:supported-report><D:report><CARD:addressbook-multiget/>\
         </D:report></D:supported-report><D:supported-report><D:report><CARD:addressbook-query/>\
         </D:report></D:supported-report><D:supported-report><D:report><D:sync-collection/>\
         </D:report></D:supported-report></D:supported-report-set>\
         <CARD:supported-address-data><CARD:address-data content-type=\"text/vcard\" version=\"3.0\"/>\
         <CARD:address-data content-type=\"text/vcard\" version=\"4.0\"/>\
         </CARD:supported-address-data>\
         <D:sync-token>{}</D:sync-token><CS:getctag>{}</CS:getctag>\
         </D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat></D:response>",
        xml_escape(href),
        xml_escape(display_name),
        xml_escape(&principal_href),
        webdav_current_user_privilege_set(),
        xml_escape(sync_token),
        xml_escape(sync_token)
    )
}

fn carddav_resource_report_response(href: &str, etag: &str, address_data: &str) -> String {
    format!(
        "<D:response><D:href>{}</D:href><D:propstat><D:prop>\
         <D:getetag>\"{}\"</D:getetag><CARD:address-data>{}</CARD:address-data>\
         </D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat></D:response>",
        xml_escape(href),
        xml_escape(etag),
        xml_escape(address_data)
    )
}

fn webdav_not_found_response(href: &str) -> String {
    format!(
        "<D:response><D:href>{}</D:href><D:status>HTTP/1.1 404 Not Found</D:status></D:response>",
        xml_escape(href)
    )
}

fn url_segment(value: &str) -> String {
    let mut out = String::new();
    for byte in value.as_bytes() {
        match *byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(char::from(*byte));
            }
            _ => {
                out.push('%');
                out.push_str(&format!("{byte:02X}"));
            }
        }
    }
    out
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_core::{LoomError, PrincipalKind};
    use loom_hosted_core::test_support::{init, nid, temp_path};

    #[test]
    fn webdav_property_update_accepts_apple_calendar_properties() {
        let body = br#"<?xml version="1.0" encoding="utf-8"?>
            <D:propertyupdate xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav" xmlns:CS="http://calendarserver.org/ns/">
              <D:set>
                <D:prop>
                  <D:displayname>Work Calendar</D:displayname>
                  <C:calendar-color>#ff0000</C:calendar-color>
                  <CS:calendar-order>1</CS:calendar-order>
                </D:prop>
              </D:set>
            </D:propertyupdate>"#;
        let update =
            webdav_property_update(body, WebDavPropertyProfile::Calendar).unwrap_or_else(|err| {
                panic!(
                    "unexpected PROPPATCH parse error: {}",
                    err.into_response().status()
                )
            });
        assert_eq!(update.display_name.as_deref(), Some("Work Calendar"));
        assert_eq!(
            update.properties,
            vec![
                WebDavUpdatedProperty::DisplayName,
                WebDavUpdatedProperty::CalendarColor,
                WebDavUpdatedProperty::CalendarOrder,
            ]
        );
    }

    #[test]
    fn webdav_property_update_rejects_unknown_properties() {
        let body = br#"<?xml version="1.0" encoding="utf-8"?>
            <D:propertyupdate xmlns:D="DAV:">
              <D:set><D:prop><D:unknown>value</D:unknown></D:prop></D:set>
            </D:propertyupdate>"#;
        assert!(webdav_property_update(body, WebDavPropertyProfile::Calendar).is_err());
    }

    #[test]
    fn caldav_mkcalendar_body_sets_display_name_and_components() {
        let body = br#"<?xml version="1.0" encoding="utf-8"?>
            <C:mkcalendar xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">
              <D:set>
                <D:prop>
                  <D:displayname>Reminders</D:displayname>
                  <C:supported-calendar-component-set>
                    <C:comp name="VTODO"/>
                  </C:supported-calendar-component-set>
                </D:prop>
              </D:set>
            </C:mkcalendar>"#;

        let properties = caldav_mkcalendar_properties(body).unwrap_or_else(|err| {
            panic!(
                "unexpected MKCALENDAR parse error: {}",
                err.into_response().status()
            )
        });

        assert_eq!(properties.display_name.as_deref(), Some("Reminders"));
        assert_eq!(properties.component_set, vec![calendar::Component::Todo]);
    }

    #[test]
    fn caldav_mkcalendar_accepts_extended_mkcol_creation_body() {
        let body = br#"<?xml version="1.0" encoding="utf-8"?>
            <D:mkcol xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav" xmlns:CS="http://calendarserver.org/ns/">
              <D:set>
                <D:prop>
                  <D:resourcetype><D:collection/><C:calendar/></D:resourcetype>
                  <D:displayname>Tasks</D:displayname>
                  <C:supported-calendar-component-set>
                    <C:comp name="VTODO"/>
                  </C:supported-calendar-component-set>
                  <C:calendar-color>#ff0000</C:calendar-color>
                  <CS:calendar-order>1</CS:calendar-order>
                </D:prop>
              </D:set>
            </D:mkcol>"#;

        let properties = caldav_mkcalendar_properties(body).unwrap_or_else(|err| {
            panic!(
                "unexpected MKCALENDAR parse error: {}",
                err.into_response().status()
            )
        });

        assert_eq!(properties.display_name.as_deref(), Some("Tasks"));
        assert_eq!(properties.component_set, vec![calendar::Component::Todo]);
    }

    #[test]
    fn caldav_mkcalendar_ignores_unsupported_component_names() {
        let body = br#"<?xml version="1.0" encoding="utf-8"?>
            <C:mkcalendar xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">
              <D:set>
                <D:prop>
                  <D:displayname>Tasks</D:displayname>
                  <C:supported-calendar-component-set>
                    <C:comp name="VTODO"/>
                    <C:comp name="VJOURNAL"/>
                    <C:comp name="VFREEBUSY"/>
                  </C:supported-calendar-component-set>
                </D:prop>
              </D:set>
            </C:mkcalendar>"#;

        let properties = caldav_mkcalendar_properties(body).unwrap_or_else(|err| {
            panic!(
                "unexpected MKCALENDAR parse error: {}",
                err.into_response().status()
            )
        });

        assert_eq!(properties.display_name.as_deref(), Some("Tasks"));
        assert_eq!(properties.component_set, vec![calendar::Component::Todo]);
    }

    #[test]
    fn caldav_mkcalendar_accepts_escaped_display_name_text() {
        let body = br#"<?xml version="1.0" encoding="utf-8"?>
            <C:mkcalendar xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">
              <D:set>
                <D:prop>
                  <D:displayname>Tasks &amp; Reminders &#35;1</D:displayname>
                  <C:supported-calendar-component-set>
                    <C:comp name="VTODO"/>
                  </C:supported-calendar-component-set>
                </D:prop>
              </D:set>
            </C:mkcalendar>"#;

        let properties = caldav_mkcalendar_properties(body).unwrap_or_else(|err| {
            panic!(
                "unexpected MKCALENDAR parse error: {}",
                err.into_response().status()
            )
        });

        assert_eq!(
            properties.display_name.as_deref(),
            Some("Tasks & Reminders #1")
        );
        assert_eq!(properties.component_set, vec![calendar::Component::Todo]);
    }

    #[test]
    fn webdav_precondition_accepts_apple_no_etag_for_absent_resource() {
        let mut headers = HeaderMap::new();
        headers.insert("if-match", "_NO_ETAG_".parse().unwrap());

        assert!(webdav_check_preconditions(&headers, None).is_ok());
        assert!(webdav_check_preconditions(&headers, Some("current")).is_err());
    }

    #[test]
    fn carddav_advertises_vcard3_and_vcard4_address_data() {
        let response = carddav_addressbook_response(
            "/carddav/personal/",
            "example@uldrentest.com",
            "Personal Contacts",
            "sync-token",
        );
        assert!(
            response.contains("<CARD:address-data content-type=\"text/vcard\" version=\"3.0\"/>"),
            "{response}"
        );
        assert!(
            response.contains("<CARD:address-data content-type=\"text/vcard\" version=\"4.0\"/>"),
            "{response}"
        );
    }

    #[test]
    fn carddav_report_request_defaults_address_data_to_vcard3() {
        let body =
            br#"<CARD:addressbook-query xmlns:D="DAV:" xmlns:CARD="urn:ietf:params:xml:ns:carddav">
            <D:prop><D:getetag/><CARD:address-data/></D:prop>
        </CARD:addressbook-query>"#;
        let report = webdav_report_request(body).unwrap_or_else(|err| {
            panic!(
                "unexpected report parse error: {}",
                err.into_response().status()
            )
        });
        let WebDavReportRequest::AddressbookQuery(query) = report else {
            panic!("expected addressbook-query");
        };
        assert!(matches!(query.version, CardDavDataVersion::V3));
    }

    #[test]
    fn carddav_report_request_accepts_requested_address_data_versions() {
        let body = br#"<CARD:addressbook-query xmlns:D="DAV:" xmlns:CARD="urn:ietf:params:xml:ns:carddav">
            <D:prop><D:getetag/><CARD:address-data content-type="text/vcard" version="4.0"/></D:prop>
        </CARD:addressbook-query>"#;
        let report = webdav_report_request(body).unwrap_or_else(|err| {
            panic!(
                "unexpected report parse error: {}",
                err.into_response().status()
            )
        });
        let WebDavReportRequest::AddressbookQuery(query) = report else {
            panic!("expected addressbook-query");
        };
        assert!(matches!(query.version, CardDavDataVersion::V4));

        let body = br#"<CARD:addressbook-multiget xmlns:D="DAV:" xmlns:CARD="urn:ietf:params:xml:ns:carddav">
            <D:prop><D:getetag/><CARD:address-data content-type="text/vcard" version="3.0"/></D:prop>
            <D:href>/carddav/personal/pim-cert-contact-1.vcf</D:href>
        </CARD:addressbook-multiget>"#;
        let report = webdav_report_request(body).unwrap_or_else(|err| {
            panic!(
                "unexpected report parse error: {}",
                err.into_response().status()
            )
        });
        let WebDavReportRequest::AddressbookMultiget { hrefs, version } = report else {
            panic!("expected addressbook-multiget");
        };
        assert_eq!(hrefs, vec!["/carddav/personal/pim-cert-contact-1.vcf"]);
        assert!(matches!(version, CardDavDataVersion::V3));
    }

    #[test]
    fn carddav_report_request_rejects_unsupported_address_data_content_type() {
        let body = br#"<CARD:addressbook-query xmlns:D="DAV:" xmlns:CARD="urn:ietf:params:xml:ns:carddav">
            <D:prop><D:getetag/><CARD:address-data content-type="application/vcard+xml" version="4.0"/></D:prop>
        </CARD:addressbook-query>"#;
        let err = match webdav_report_request(body) {
            Ok(_) => panic!("xCard address-data must be rejected"),
            Err(err) => err,
        };
        assert_eq!(
            err.into_response().status(),
            StatusCode::UNSUPPORTED_MEDIA_TYPE
        );
    }

    #[test]
    fn caldav_propfind_accepts_resource_path() {
        let path = temp_path("caldav-resource-propfind");
        let ns = init(&path, None);
        let state = test_webdav_state(&path);
        let auth = HostedAuth::passphrase(nid(1), "root-pass", "caldav-resource-propfind");
        seed_caldav_move_collections(&state, &auth, ns);

        let response = caldav_propfind(
            &state,
            &auth,
            "root",
            "staging/event-1.ics",
            WebDavDepth::Zero,
        );

        assert_eq!(response.status(), StatusCode::MULTI_STATUS);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn carddav_propfind_accepts_resource_path() {
        let path = temp_path("carddav-resource-propfind");
        let ns = init(&path, None);
        let state = test_webdav_state(&path);
        let auth = HostedAuth::passphrase(nid(1), "root-pass", "carddav-resource-propfind");
        state
            .kernel
            .write(&auth, |loom| {
                contacts::create_book(
                    loom,
                    ns,
                    "root",
                    "personal",
                    &contacts::BookMeta {
                        display_name: "Personal".to_string(),
                    },
                )?;
                contacts::put_entry(
                    loom,
                    ns,
                    "root",
                    "personal",
                    &contacts::ContactEntry {
                        uid: "contact-1".to_string(),
                        full_name: "Ada Lovelace".to_string(),
                        emails: vec![contacts::TypedValue::typed("ada@example.test", "work")],
                        ..Default::default()
                    },
                )?;
                loom.commit(ns, "test", "seed contact", 0)?;
                Ok(())
            })
            .unwrap();

        let response = carddav_propfind(
            &state,
            &auth,
            "root",
            "personal/contact-1.vcf",
            WebDavDepth::Zero,
        );

        assert_eq!(response.status(), StatusCode::MULTI_STATUS);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn caldav_reports_reject_missing_collection() {
        let path = temp_path("caldav-missing-report-collection");
        init(&path, None);
        let state = test_webdav_state(&path);
        let auth = HostedAuth::passphrase(nid(1), "root-pass", "caldav-missing-report");

        let query_err = caldav_query_resources(
            &state,
            &auth,
            "root",
            "missing",
            &CalDavCalendarQuery::default(),
        )
        .unwrap_err();
        assert_eq!(query_err.code, Code::NotFound);

        let sync_err = match caldav_sync_resources(&state, &auth, "root", "missing", None) {
            Ok(_) => panic!("missing CalDAV collection must fail"),
            Err(err) => err,
        };
        assert_eq!(sync_err.code, Code::NotFound);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn caldav_move_resource_between_collections() {
        let path = temp_path("caldav-move-resource");
        let ns = init(&path, None);
        let state = test_webdav_state(&path);
        let auth = HostedAuth::passphrase(nid(1), "root-pass", "caldav-move");
        seed_caldav_move_collections(&state, &auth, ns);

        let mut headers = HeaderMap::new();
        headers.insert(
            "destination",
            "https://uldrentest.com/caldav/personal/event-1.ics"
                .parse()
                .unwrap(),
        );
        let response = caldav_move(&state, &auth, "root", &headers, "staging/event-1.ics");
        assert_eq!(response.status(), StatusCode::CREATED);

        state
            .kernel
            .read(&auth, |loom| {
                assert!(calendar::get_entry(loom, ns, "root", "staging", "event-1")?.is_none());
                assert!(calendar::get_entry(loom, ns, "root", "personal", "event-1")?.is_some());
                Ok(())
            })
            .unwrap();
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn caldav_move_honors_overwrite_false() {
        let path = temp_path("caldav-move-overwrite-false");
        let ns = init(&path, None);
        let state = test_webdav_state(&path);
        let auth = HostedAuth::passphrase(nid(1), "root-pass", "caldav-move-overwrite");
        seed_caldav_move_collections(&state, &auth, ns);
        state
            .kernel
            .write(&auth, |loom| {
                let entry = calendar::CalendarEntry::from_ics(
                    "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:event-1\r\nDTSTART:20260708T160000Z\r\nSUMMARY:Existing\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n",
                )?;
                calendar::put_entry(loom, ns, "root", "personal", &entry)?;
                loom.commit(ns, "test", "seed destination", 0)?;
                Ok(())
            })
            .unwrap();

        let mut headers = HeaderMap::new();
        headers.insert(
            "destination",
            "https://uldrentest.com/caldav/personal/event-1.ics"
                .parse()
                .unwrap(),
        );
        headers.insert("overwrite", "F".parse().unwrap());
        let response = caldav_move(&state, &auth, "root", &headers, "staging/event-1.ics");
        assert_eq!(response.status(), StatusCode::PRECONDITION_FAILED);

        state
            .kernel
            .read(&auth, |loom| {
                assert!(calendar::get_entry(loom, ns, "root", "staging", "event-1")?.is_some());
                Ok(())
            })
            .unwrap();
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn carddav_reports_reject_missing_book() {
        let path = temp_path("carddav-missing-report-book");
        init(&path, None);
        let state = test_webdav_state(&path);
        let auth = HostedAuth::passphrase(nid(1), "root-pass", "carddav-missing-report");

        let query_err = carddav_query_resources(
            &state,
            &auth,
            "root",
            "missing",
            &CardDavAddressbookQuery::default(),
        )
        .unwrap_err();
        assert_eq!(query_err.code, Code::NotFound);

        let sync_err = match carddav_sync_resources(&state, &auth, "root", "missing", None) {
            Ok(_) => panic!("missing CardDAV book must fail"),
            Err(err) => err,
        };
        assert_eq!(sync_err.code, Code::NotFound);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn carddav_sync_collection_resyncs_on_stale_client_token() {
        let path = temp_path("carddav-stale-sync-token");
        let ns = init(&path, None);
        let kernel = HostedKernel::new(&path);
        let auth = HostedAuth::passphrase(nid(1), "root-pass", "carddav-sync-test");
        kernel
            .write(&auth, |loom| {
                contacts::create_book(
                    loom,
                    ns,
                    "root",
                    "personal",
                    &contacts::BookMeta {
                        display_name: "Personal".to_string(),
                    },
                )?;
                contacts::put_entry(
                    loom,
                    ns,
                    "root",
                    "personal",
                    &contacts::ContactEntry {
                        uid: "contact-1".to_string(),
                        full_name: "Ada Lovelace".to_string(),
                        emails: vec![contacts::TypedValue::typed("ada@example.test", "work")],
                        ..Default::default()
                    },
                )?;
                loom.commit(ns, "test", "seed contacts", 0)?;
                Ok(())
            })
            .unwrap();
        let state = PimWebDavState {
            kernel,
            workspace: "main".to_string(),
            request_size_limit: 16 * 1024 * 1024,
            auth_policy: HostedAuthPolicy::Passphrase,
            basic_auth_cache: Arc::new(Mutex::new(DavBasicAuthCache::default())),
        };

        let (token, rows) = carddav_sync_resources(
            &state,
            &auth,
            "root",
            "personal",
            Some("loom-carddav-sync-v1:blake3:0000000000000000000000000000000000000000000000000000000000000000"),
        )
        .unwrap();

        assert!(token.starts_with("loom-carddav-sync-v1:"));
        assert_eq!(rows.len(), 1);
        let CardDavSyncRow::Present(resource) = &rows[0] else {
            panic!("stale sync-token should return present resources");
        };
        assert_eq!(resource.uid, "contact-1");
        assert!(resource.body.contains("VERSION:3.0"));
        std::fs::remove_file(path).unwrap();
    }

    fn test_webdav_state(path: &std::path::Path) -> PimWebDavState {
        PimWebDavState {
            kernel: HostedKernel::new(path),
            workspace: "main".to_string(),
            request_size_limit: 16 * 1024 * 1024,
            auth_policy: HostedAuthPolicy::Passphrase,
            basic_auth_cache: Arc::new(Mutex::new(DavBasicAuthCache::default())),
        }
    }

    fn seed_caldav_move_collections(state: &PimWebDavState, auth: &HostedAuth, ns: WorkspaceId) {
        state
            .kernel
            .write(auth, |loom| {
                calendar::create_collection(
                    loom,
                    ns,
                    "root",
                    "staging",
                    &calendar::CollectionMeta {
                        display_name: "Staging".to_string(),
                        component_set: vec![calendar::Component::Event],
                    },
                )?;
                calendar::create_collection(
                    loom,
                    ns,
                    "root",
                    "personal",
                    &calendar::CollectionMeta {
                        display_name: "Personal".to_string(),
                        component_set: vec![calendar::Component::Event],
                    },
                )?;
                let entry = calendar::CalendarEntry::from_ics(
                    "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:event-1\r\nDTSTART:20260708T150000Z\r\nSUMMARY:Move me\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n",
                )?;
                calendar::put_entry(loom, ns, "root", "staging", &entry)?;
                loom.commit(ns, "test", "seed move calendar", 0)?;
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn http_basic_principal_accepts_full_email_username() {
        let (kernel, path, principal) = basic_auth_identity_store();
        let headers = host_headers("uldrentest.com");
        let resolved = resolved_basic_principal(&kernel, "example@uldrentest.com", &headers);
        assert_eq!(resolved, (principal, "example@uldrentest.com".to_string()));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn http_basic_principal_accepts_one_pass_percent_encoded_email_username() {
        let (kernel, path, principal) = basic_auth_identity_store();
        let headers = host_headers("uldrentest.com");
        let resolved = resolved_basic_principal(&kernel, "example%40uldrentest.com", &headers);
        assert_eq!(resolved, (principal, "example@uldrentest.com".to_string()));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn http_basic_principal_accepts_same_domain_local_part_username() {
        let (kernel, path, principal) = basic_auth_identity_store();
        let headers = host_headers("uldrentest.com:443");
        let resolved = resolved_basic_principal(&kernel, "example", &headers);
        assert_eq!(resolved, (principal, "example@uldrentest.com".to_string()));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn http_basic_principal_rejects_local_part_for_wrong_domain() {
        let (kernel, path, _) = basic_auth_identity_store();
        let headers = host_headers("other.example");
        let err = resolve_http_basic_principal(&kernel, "example", &headers).unwrap_err();
        assert_eq!(err.into_response().status(), StatusCode::UNAUTHORIZED);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn http_basic_principal_does_not_decode_username_twice() {
        let (kernel, path, _) = basic_auth_identity_store();
        let headers = host_headers("uldrentest.com");
        let err = resolve_http_basic_principal(&kernel, "example%2540uldrentest.com", &headers)
            .unwrap_err();
        assert_eq!(err.into_response().status(), StatusCode::UNAUTHORIZED);
        std::fs::remove_file(path).unwrap();
    }

    fn basic_auth_identity_store() -> (HostedKernel, std::path::PathBuf, WorkspaceId) {
        let path = temp_path("dav-basic-auth");
        init(&path, None);
        let kernel = HostedKernel::new(&path);
        let principal = nid(8);
        kernel
            .write(
                &HostedAuth::passphrase(nid(1), "root-pass", "dav-basic-auth-setup"),
                |loom| {
                    {
                        let identity = loom.identity_store_mut().ok_or_else(|| {
                            LoomError::new(Code::NotFound, "identity store not found")
                        })?;
                        identity.add_principal(
                            principal,
                            "example@uldrentest.com",
                            PrincipalKind::User,
                        )?;
                        identity.set_passphrase(principal, "testpassword", b"example1")?;
                    }
                    let identity = loom.identity_store().ok_or_else(|| {
                        LoomError::new(Code::NotFound, "identity store not found")
                    })?;
                    loom.store().save_identity_store(identity)?;
                    Ok(())
                },
            )
            .unwrap();
        (kernel, path, principal)
    }

    fn host_headers(host: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("host", host.parse().unwrap());
        headers
    }

    fn resolved_basic_principal(
        kernel: &HostedKernel,
        username: &str,
        headers: &HeaderMap,
    ) -> (WorkspaceId, String) {
        match resolve_http_basic_principal(kernel, username, headers) {
            Ok(resolved) => resolved,
            Err(err) => panic!(
                "unexpected Basic principal resolution failure: {}",
                err.into_response().status()
            ),
        }
    }
}
