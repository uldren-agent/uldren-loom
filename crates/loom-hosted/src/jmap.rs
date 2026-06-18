use std::future::Future;

use axum::Router;
use tokio::net::TcpListener;

use crate::{HostedAuthPolicy, HostedHttpLimits, HostedKernel};

pub fn mail_jmap_router(kernel: HostedKernel, workspace: impl Into<String>) -> Router {
    loom_hosted_pim::mail_jmap_router(kernel.into_core(), workspace)
}

pub fn mail_jmap_router_with_limit(
    kernel: HostedKernel,
    workspace: impl Into<String>,
    request_size_limit: usize,
) -> Router {
    loom_hosted_pim::mail_jmap_router_with_limit(kernel.into_core(), workspace, request_size_limit)
}

pub fn mail_jmap_router_with_policy(
    kernel: HostedKernel,
    workspace: impl Into<String>,
    request_size_limit: usize,
    auth_policy: HostedAuthPolicy,
) -> Router {
    loom_hosted_pim::mail_jmap_router_with_policy(
        kernel.into_core(),
        workspace,
        request_size_limit,
        auth_policy,
    )
}

pub async fn serve_mail_jmap_with_limits<S>(
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
    loom_hosted_pim::serve_mail_jmap_with_limits(
        listener,
        kernel.into_core(),
        workspace,
        limits,
        auth_policy,
        shutdown,
    )
    .await
}
