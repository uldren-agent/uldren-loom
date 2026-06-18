use std::future::Future;

use tokio::net::TcpListener;

#[cfg(feature = "tls")]
use crate::HostedTlsConfig;
use crate::{HostedAuthPolicy, HostedKernel};

pub async fn serve_mail_imap<S>(
    listener: TcpListener,
    kernel: HostedKernel,
    workspace: impl Into<String>,
    auth_policy: HostedAuthPolicy,
    shutdown: S,
) -> std::io::Result<()>
where
    S: Future<Output = ()> + Send + 'static,
{
    loom_hosted_pim::serve_mail_imap(
        listener,
        kernel.into_core(),
        workspace,
        auth_policy,
        shutdown,
    )
    .await
}

#[cfg(feature = "tls")]
pub async fn serve_mail_imap_tls<S>(
    listener: TcpListener,
    tls: HostedTlsConfig,
    kernel: HostedKernel,
    workspace: impl Into<String>,
    auth_policy: HostedAuthPolicy,
    shutdown: S,
) -> std::io::Result<()>
where
    S: Future<Output = ()> + Send + 'static,
{
    loom_hosted_pim::serve_mail_imap_tls(
        listener,
        tls,
        kernel.into_core(),
        workspace,
        auth_policy,
        shutdown,
    )
    .await
}
