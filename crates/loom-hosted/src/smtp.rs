use std::future::Future;

use tokio::net::TcpListener;

#[cfg(feature = "tls")]
use crate::HostedTlsConfig;
use crate::{HostedAuthPolicy, HostedHttpLimits, HostedKernel};

pub async fn serve_mail_smtp<S>(
    listener: TcpListener,
    kernel: HostedKernel,
    limits: HostedHttpLimits,
    auth_policy: HostedAuthPolicy,
    shutdown: S,
) -> std::io::Result<()>
where
    S: Future<Output = ()> + Send + 'static,
{
    loom_hosted_pim::serve_mail_smtp(listener, kernel.into_core(), limits, auth_policy, shutdown)
        .await
}

#[cfg(feature = "tls")]
pub async fn serve_mail_smtp_tls<S>(
    listener: TcpListener,
    tls: HostedTlsConfig,
    kernel: HostedKernel,
    limits: HostedHttpLimits,
    auth_policy: HostedAuthPolicy,
    shutdown: S,
) -> std::io::Result<()>
where
    S: Future<Output = ()> + Send + 'static,
{
    loom_hosted_pim::serve_mail_smtp_tls(
        listener,
        tls,
        kernel.into_core(),
        limits,
        auth_policy,
        shutdown,
    )
    .await
}

#[cfg(feature = "tls")]
pub async fn serve_mail_smtp_starttls<S>(
    listener: TcpListener,
    tls: HostedTlsConfig,
    kernel: HostedKernel,
    limits: HostedHttpLimits,
    auth_policy: HostedAuthPolicy,
    shutdown: S,
) -> std::io::Result<()>
where
    S: Future<Output = ()> + Send + 'static,
{
    loom_hosted_pim::serve_mail_smtp_starttls(
        listener,
        tls,
        kernel.into_core(),
        limits,
        auth_policy,
        shutdown,
    )
    .await
}
