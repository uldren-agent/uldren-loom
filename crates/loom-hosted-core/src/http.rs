use std::future::Future;
use std::net::SocketAddr;
use std::pin::pin;

use axum::Router;
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::serve::Listener;
use futures_util::FutureExt;
use hyper::body::Incoming;
use hyper_util::rt::{TokioExecutor, TokioIo, TokioTimer};
use hyper_util::{server::conn::auto::Builder, service::TowerToHyperService};
use tokio::io::{AsyncRead, AsyncWrite};
#[cfg(feature = "tls")]
use tokio::net::TcpStream;
use tokio::sync::watch;
use tower::ServiceExt as _;

use loom_core::Code;

use crate::HostedHttpLimits;
use crate::network_access::{
    HostedNetworkAccessPolicy, HostedPeerCertificate, current_hosted_network_access_policy,
    network_access_allows,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct HostedPeerAddr(SocketAddr);

pub async fn serve_router<L, S>(
    listener: L,
    router: Router,
    limits: HostedHttpLimits,
    shutdown: S,
) -> std::io::Result<()>
where
    L: Listener,
    L::Addr: std::fmt::Debug + Into<SocketAddr>,
    L::Io: AsyncRead + AsyncWrite + Unpin + Send + HostedPeerCertificateSource + 'static,
    S: Future<Output = ()> + Send + 'static,
{
    serve_router_with_network_access(
        listener,
        router,
        limits,
        current_hosted_network_access_policy(),
        shutdown,
    )
    .await
}

pub async fn serve_router_with_network_access<L, S>(
    mut listener: L,
    router: Router,
    limits: HostedHttpLimits,
    network_access_policy: Option<HostedNetworkAccessPolicy>,
    shutdown: S,
) -> std::io::Result<()>
where
    L: Listener,
    L::Addr: std::fmt::Debug + Into<SocketAddr>,
    L::Io: AsyncRead + AsyncWrite + Unpin + Send + HostedPeerCertificateSource + 'static,
    S: Future<Output = ()> + Send + 'static,
{
    let router = match network_access_policy.clone() {
        Some(policy) => router.layer(middleware::from_fn_with_state(
            policy,
            hosted_network_access_layer,
        )),
        None => router,
    };
    let (signal_tx, signal_rx) = watch::channel(());
    tokio::spawn(async move {
        shutdown.await;
        drop(signal_rx);
    });
    let (close_tx, close_rx) = watch::channel(());

    loop {
        let (io, addr) = tokio::select! {
            conn = listener.accept() => conn,
            _ = signal_tx.closed() => break,
        };
        let peer_certificate = io.hosted_peer_certificate();
        spawn_http_connection(
            router.clone(),
            io,
            addr.into(),
            peer_certificate,
            limits,
            signal_tx.clone(),
            close_rx.clone(),
        );
    }

    drop(close_rx);
    drop(listener);
    close_tx.closed().await;
    Ok(())
}

pub fn effective_hosted_network_access_policy(
    policy: Option<loom_store::NetworkAccessPolicyRecord>,
) -> Option<HostedNetworkAccessPolicy> {
    policy
        .map(HostedNetworkAccessPolicy::from_record)
        .or_else(current_hosted_network_access_policy)
}

fn spawn_http_connection<I>(
    router: Router,
    io: I,
    peer_addr: SocketAddr,
    peer_certificate: Option<HostedPeerCertificate>,
    limits: HostedHttpLimits,
    signal_tx: watch::Sender<()>,
    close_rx: watch::Receiver<()>,
) where
    I: AsyncRead + AsyncWrite + Unpin + Send + HostedPeerCertificateSource + 'static,
{
    let io = TokioIo::new(io);
    let tower_service = router.map_request(move |mut req: Request<Incoming>| {
        req.extensions_mut().insert(HostedPeerAddr(peer_addr));
        if let Some(peer_certificate) = peer_certificate.clone() {
            req.extensions_mut().insert(peer_certificate);
        }
        req.map(Body::new)
    });
    let hyper_service = TowerToHyperService::new(tower_service);
    tokio::spawn(async move {
        let mut builder = Builder::new(TokioExecutor::new());
        let timer = TokioTimer::new();
        builder
            .http1()
            .timer(timer.clone())
            .header_read_timeout(limits.idle_timeout);
        builder
            .http2()
            .timer(timer)
            .keep_alive_interval(Some(limits.idle_timeout))
            .keep_alive_timeout(limits.idle_timeout);

        let mut conn = pin!(builder.serve_connection_with_upgrades(io, hyper_service));
        let mut signal_closed = pin!(signal_tx.closed().fuse());
        let session_timeout = tokio::time::sleep(limits.session_timeout);
        tokio::pin!(session_timeout);

        loop {
            tokio::select! {
                _ = conn.as_mut() => break,
                _ = &mut signal_closed => {
                    conn.as_mut().graceful_shutdown();
                }
                _ = &mut session_timeout => break,
            }
        }
        drop(close_rx);
    });
}

async fn hosted_network_access_layer(
    State(policy): State<HostedNetworkAccessPolicy>,
    request: Request,
    next: Next,
) -> Response {
    let peer_addr = request
        .extensions()
        .get::<HostedPeerAddr>()
        .map(|addr| addr.0);
    let peer_certificate = request.extensions().get::<HostedPeerCertificate>();
    let forwarded_for = request
        .headers()
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok());
    let forwarded = request
        .headers()
        .get("forwarded")
        .and_then(|value| value.to_str().ok());
    let allowed = peer_addr.is_some_and(|addr| {
        network_access_allows(
            Some(&policy),
            addr,
            peer_certificate,
            forwarded_for,
            forwarded,
        )
    });
    if !allowed {
        return (
            StatusCode::FORBIDDEN,
            [("content-type", "application/json")],
            format!(
                "{{\"code\":\"{}\",\"code_number\":{},\"message\":\"network access denied\"}}",
                Code::PermissionDenied.as_str(),
                Code::PermissionDenied.as_i32()
            ),
        )
            .into_response();
    }
    next.run(request).await
}

pub trait HostedPeerCertificateSource {
    fn hosted_peer_certificate(&self) -> Option<HostedPeerCertificate>;
}

impl HostedPeerCertificateSource for tokio::net::TcpStream {
    fn hosted_peer_certificate(&self) -> Option<HostedPeerCertificate> {
        None
    }
}

#[cfg(feature = "tls")]
impl HostedPeerCertificateSource for tokio_rustls::server::TlsStream<TcpStream> {
    fn hosted_peer_certificate(&self) -> Option<HostedPeerCertificate> {
        self.get_ref()
            .1
            .peer_certificates()
            .and_then(|chain| chain.first())
            .map(|leaf| HostedPeerCertificate::from_leaf_der(leaf.as_ref().to_vec()))
    }
}
