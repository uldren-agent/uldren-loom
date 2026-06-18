use std::future::Future;
#[cfg(feature = "tls")]
use std::io::Cursor;
use std::pin::Pin;
#[cfg(feature = "tls")]
use std::task::{Context, Poll};

use base64::Engine;
use loom_core::{Code, LoomError, WorkspaceId};
#[cfg(feature = "tls")]
use tokio::io::ReadBuf;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::task::JoinSet;

use loom_hosted_core::{
    HostedAuth, HostedAuthPolicy, HostedHttpLimits, HostedKernel,
    network_access::{current_hosted_network_access_policy, network_access_allows},
};

#[cfg(feature = "tls")]
use loom_hosted_core::HostedTlsConfig;

const SMTP_SHUTDOWN_GRACE: std::time::Duration = std::time::Duration::from_secs(30);
const SMTP_MAX_LINE: usize = 1000;

trait SmtpIo: AsyncRead + AsyncWrite + Unpin + Send {}

impl<T> SmtpIo for T where T: AsyncRead + AsyncWrite + Unpin + Send {}

#[cfg(feature = "tls")]
struct BufferedSmtpIo {
    pending: Cursor<Vec<u8>>,
    inner: Box<dyn SmtpIo>,
}

#[cfg(feature = "tls")]
impl BufferedSmtpIo {
    fn new(pending: Vec<u8>, inner: Box<dyn SmtpIo>) -> Self {
        Self {
            pending: Cursor::new(pending),
            inner,
        }
    }
}

#[cfg(feature = "tls")]
impl AsyncRead for BufferedSmtpIo {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let start = self.pending.position() as usize;
        let pending_len = self.pending.get_ref().len();
        if start < pending_len {
            let available = pending_len - start;
            let to_copy = available.min(buf.remaining());
            buf.put_slice(&self.pending.get_ref()[start..start + to_copy]);
            self.pending.set_position((start + to_copy) as u64);
            return Poll::Ready(Ok(()));
        }
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

#[cfg(feature = "tls")]
impl AsyncWrite for BufferedSmtpIo {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        data: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, data)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

#[derive(Clone)]
struct MailSmtpState {
    kernel: HostedKernel,
    auth_policy: HostedAuthPolicy,
    limits: HostedHttpLimits,
    implicit_tls: bool,
    #[cfg(feature = "tls")]
    tls: Option<HostedTlsConfig>,
}

#[derive(Default)]
struct MailSmtpSession {
    tls_active: bool,
    authenticated: bool,
    mail_from: Option<String>,
    rcpt_to: Vec<String>,
}

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
    let state = MailSmtpState {
        kernel,
        auth_policy,
        limits,
        implicit_tls: false,
        #[cfg(feature = "tls")]
        tls: None,
    };
    serve_mail_smtp_accept(listener, state, Box::pin(shutdown)).await
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
    let state = MailSmtpState {
        kernel,
        auth_policy,
        limits,
        implicit_tls: true,
        tls: Some(tls),
    };
    serve_mail_smtp_accept(listener, state, Box::pin(shutdown)).await
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
    let state = MailSmtpState {
        kernel,
        auth_policy,
        limits,
        implicit_tls: false,
        tls: Some(tls),
    };
    serve_mail_smtp_accept(listener, state, Box::pin(shutdown)).await
}

async fn serve_mail_smtp_accept(
    listener: TcpListener,
    state: MailSmtpState,
    mut shutdown: Pin<Box<dyn Future<Output = ()> + Send>>,
) -> std::io::Result<()> {
    let network_access_policy = current_hosted_network_access_policy();
    let mut tasks = JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, addr) = accepted?;
                if !network_access_allows(network_access_policy.as_ref(), addr, None, None, None) {
                    continue;
                }
                let state = state.clone();
                tasks.spawn(async move {
                    let _ = handle_smtp_connection(stream, state).await;
                });
            }
            joined = tasks.join_next(), if !tasks.is_empty() => {
                let _ = joined;
            }
            _ = &mut shutdown => break,
        }
    }
    drain_smtp_connections(tasks).await;
    Ok(())
}

async fn drain_smtp_connections(mut tasks: JoinSet<()>) {
    let drained = async { while tasks.join_next().await.is_some() {} };
    if tokio::time::timeout(SMTP_SHUTDOWN_GRACE, drained)
        .await
        .is_err()
    {
        tasks.abort_all();
        while tasks.join_next().await.is_some() {}
    }
}

async fn handle_smtp_connection<T>(stream: T, state: MailSmtpState) -> std::io::Result<()>
where
    T: SmtpIo + 'static,
{
    let mut reader = Some(BufReader::new(Box::new(stream) as Box<dyn SmtpIo>));
    let mut session = MailSmtpSession {
        tls_active: state.implicit_tls,
        ..Default::default()
    };
    write_smtp(
        reader.as_mut().unwrap(),
        "220 uldrentest.com Loom SMTP setup compatibility ready",
    )
    .await?;
    loop {
        let mut current = reader.take().unwrap();
        let mut line = String::new();
        let read = current.read_line(&mut line).await?;
        if read == 0 {
            break;
        }
        if line.len() > SMTP_MAX_LINE {
            write_smtp(&mut current, "500 Line too long").await?;
            reader = Some(current);
            continue;
        }
        trim_eol(&mut line);
        if line.is_empty() {
            write_smtp(&mut current, "500 Empty command").await?;
            reader = Some(current);
            continue;
        }
        let (command, argument) = smtp_command(&line);
        match command.as_str() {
            "EHLO" | "HELO" => {
                handle_ehlo(&mut current, &session, &state, &command, argument).await?;
                reader = Some(current);
            }
            "STARTTLS" => {
                if !argument.is_empty() {
                    write_smtp(
                        &mut current,
                        "501 Syntax error: STARTTLS takes no arguments",
                    )
                    .await?;
                    reader = Some(current);
                } else if session.tls_active {
                    write_smtp(&mut current, "503 TLS already active").await?;
                    reader = Some(current);
                } else {
                    #[cfg(feature = "tls")]
                    {
                        if let Some(tls) = state.tls.clone() {
                            write_smtp(&mut current, "220 Ready to start TLS").await?;
                            let pending = current.buffer().to_vec();
                            let io = current.into_inner();
                            let io: Box<dyn SmtpIo> = if pending.is_empty() {
                                io
                            } else {
                                Box::new(BufferedSmtpIo::new(pending, io))
                            };
                            let tls_stream = tls.acceptor().accept(io).await?;
                            session.tls_active = true;
                            reader = Some(BufReader::new(Box::new(tls_stream) as Box<dyn SmtpIo>));
                        } else {
                            write_smtp(&mut current, "454 TLS not available").await?;
                            reader = Some(current);
                        }
                    }
                    #[cfg(not(feature = "tls"))]
                    {
                        write_smtp(&mut current, "454 TLS not available").await?;
                        reader = Some(current);
                    }
                }
            }
            "AUTH" => {
                handle_auth(&mut current, &state, &mut session, argument).await?;
                reader = Some(current);
            }
            "MAIL" => {
                handle_mail(&mut current, &state, &mut session, argument).await?;
                reader = Some(current);
            }
            "RCPT" => {
                handle_rcpt(&mut current, &mut session, argument).await?;
                reader = Some(current);
            }
            "DATA" => {
                handle_data(&mut current, &state, &mut session).await?;
                reader = Some(current);
            }
            "RSET" => {
                session.mail_from = None;
                session.rcpt_to.clear();
                write_smtp(&mut current, "250 Reset").await?;
                reader = Some(current);
            }
            "NOOP" => {
                write_smtp(&mut current, "250 OK").await?;
                reader = Some(current);
            }
            "QUIT" => {
                write_smtp(&mut current, "221 Bye").await?;
                current.get_mut().shutdown().await?;
                break;
            }
            _ => {
                write_smtp(&mut current, "502 Command not implemented").await?;
                reader = Some(current);
            }
        }
    }
    Ok(())
}

fn smtp_tls_configured(state: &MailSmtpState) -> bool {
    #[cfg(feature = "tls")]
    {
        state.tls.is_some()
    }
    #[cfg(not(feature = "tls"))]
    {
        let _ = state;
        false
    }
}

async fn handle_ehlo(
    reader: &mut BufReader<Box<dyn SmtpIo>>,
    session: &MailSmtpSession,
    state: &MailSmtpState,
    command: &str,
    argument: &str,
) -> std::io::Result<()> {
    let name = if argument.is_empty() {
        "client"
    } else {
        argument
    };
    if command == "HELO" {
        write_smtp(reader, &format!("250 uldrentest.com greets {name}")).await?;
        return Ok(());
    }
    write_smtp_dash(reader, &format!("250-uldrentest.com greets {name}")).await?;
    if smtp_tls_configured(state) && !session.tls_active {
        write_smtp_dash(reader, "250-STARTTLS").await?;
    }
    write_smtp_dash(reader, "250-AUTH PLAIN LOGIN").await?;
    write_smtp_dash(reader, "250-8BITMIME").await?;
    write_smtp(
        reader,
        &format!("250 SIZE {}", state.limits.request_size_limit),
    )
    .await
}

async fn handle_auth(
    reader: &mut BufReader<Box<dyn SmtpIo>>,
    state: &MailSmtpState,
    session: &mut MailSmtpSession,
    argument: &str,
) -> std::io::Result<()> {
    if session.authenticated {
        write_smtp(reader, "503 Already authenticated").await?;
        return Ok(());
    }
    if session.mail_from.is_some() {
        write_smtp(reader, "503 AUTH not permitted during mail transaction").await?;
        return Ok(());
    }
    if smtp_tls_configured(state) && !session.tls_active {
        write_smtp(reader, "530 Must ticket STARTTLS first").await?;
        return Ok(());
    }
    let (mechanism, payload) = smtp_command(argument);
    let result = match mechanism.as_str() {
        "PLAIN" => authenticate_plain(reader, state, payload).await,
        "LOGIN" => authenticate_login(reader, state, payload).await,
        _ => {
            write_smtp(reader, "504 Authentication mechanism unavailable").await?;
            return Ok(());
        }
    };
    match result {
        Ok((_username, _principal)) => {
            session.authenticated = true;
            write_smtp(reader, "235 Authentication successful").await
        }
        Err(err) => {
            if err.code == Code::InvalidArgument {
                write_smtp(reader, "501 Authentication exchange invalid").await
            } else {
                state
                    .kernel
                    .audit_security_failure(&HostedAuth::unauthenticated(), &err);
                write_smtp(reader, "535 Authentication failed").await
            }
        }
    }
}

async fn authenticate_login(
    reader: &mut BufReader<Box<dyn SmtpIo>>,
    state: &MailSmtpState,
    payload: &str,
) -> Result<(String, String), LoomError> {
    let username = if payload.is_empty() {
        write_smtp(reader, "334 VXNlcm5hbWU6")
            .await
            .map_err(|e| LoomError::new(Code::Io, e.to_string()))?;
        read_smtp_auth_field(reader).await?
    } else {
        decode_base64_text(payload)?
    };
    write_smtp(reader, "334 UGFzc3dvcmQ6")
        .await
        .map_err(|e| LoomError::new(Code::Io, e.to_string()))?;
    let password = read_smtp_auth_field(reader).await?;
    login(state, &username, &password).map(|(_, principal)| (username, principal))
}

async fn read_smtp_auth_field(
    reader: &mut BufReader<Box<dyn SmtpIo>>,
) -> Result<String, LoomError> {
    let mut line = String::new();
    let read = reader
        .read_line(&mut line)
        .await
        .map_err(|e| LoomError::new(Code::Io, e.to_string()))?;
    if read == 0 {
        return Err(LoomError::invalid("SMTP AUTH LOGIN cancelled"));
    }
    trim_eol(&mut line);
    decode_base64_text(&line)
}

async fn authenticate_plain(
    reader: &mut BufReader<Box<dyn SmtpIo>>,
    state: &MailSmtpState,
    payload: &str,
) -> Result<(String, String), LoomError> {
    let payload = if payload.is_empty() {
        write_smtp(reader, "334 ")
            .await
            .map_err(|e| LoomError::new(Code::Io, e.to_string()))?;
        let mut line = String::new();
        let read = reader
            .read_line(&mut line)
            .await
            .map_err(|e| LoomError::new(Code::Io, e.to_string()))?;
        if read == 0 {
            return Err(LoomError::invalid("SMTP AUTH PLAIN cancelled"));
        }
        trim_eol(&mut line);
        line
    } else {
        payload.to_string()
    };
    let decoded = decode_base64(&payload)?;
    let fields = decoded.split(|byte| *byte == 0).collect::<Vec<_>>();
    if fields.len() != 3 || fields[1].is_empty() || fields[2].is_empty() {
        return Err(LoomError::invalid("SMTP AUTH PLAIN expects credentials"));
    }
    let authzid = std::str::from_utf8(fields[0])
        .map_err(|_| LoomError::invalid("SMTP AUTH PLAIN authzid is not UTF-8"))?;
    let username = std::str::from_utf8(fields[1])
        .map_err(|_| LoomError::invalid("SMTP AUTH PLAIN username is not UTF-8"))?;
    let password = std::str::from_utf8(fields[2])
        .map_err(|_| LoomError::invalid("SMTP AUTH PLAIN password is not UTF-8"))?;
    if !authzid.is_empty() && authzid != username {
        return Err(LoomError::new(
            Code::AuthenticationFailed,
            "SMTP AUTH PLAIN authzid is not authorized",
        ));
    }
    login(state, username, password).map(|(_, principal)| (username.to_string(), principal))
}

fn login(
    state: &MailSmtpState,
    username: &str,
    password: &str,
) -> Result<(HostedAuth, String), LoomError> {
    let (auth, principal) = state.kernel.read(&HostedAuth::unauthenticated(), |loom| {
        let Some(identity) = loom.identity_store() else {
            if state.auth_policy == HostedAuthPolicy::OwnerOrPassphrase {
                return Ok((HostedAuth::unauthenticated(), username.to_string()));
            }
            return Err(LoomError::new(
                Code::AuthenticationFailed,
                "SMTP login requires an authenticated principal",
            ));
        };
        if !identity.authenticated_mode()
            && state.auth_policy == HostedAuthPolicy::OwnerOrPassphrase
        {
            return Ok((HostedAuth::unauthenticated(), username.to_string()));
        }
        let (principal, mail_principal) = resolve_login_principal(identity, username)?;
        Ok((
            HostedAuth::passphrase(principal, password, format!("smtp-{principal}")),
            mail_principal,
        ))
    })?;
    state.kernel.read(&auth, |_| Ok(()))?;
    Ok((auth, principal))
}

fn resolve_login_principal(
    identity: &loom_core::IdentityStore,
    username: &str,
) -> Result<(WorkspaceId, String), LoomError> {
    if let Some((principal, mail_principal)) = username.split_once(':') {
        let principal = WorkspaceId::parse(principal)?;
        let mail_principal = if mail_principal.is_empty() {
            identity.principal(principal)?.name.clone()
        } else {
            mail_principal.to_string()
        };
        return Ok((principal, mail_principal));
    }
    if let Ok(principal) = WorkspaceId::parse(username) {
        return Ok((principal, identity.principal(principal)?.name.clone()));
    }
    identity
        .principals()
        .find(|principal| principal.name == username)
        .map(|principal| (principal.id, principal.name.clone()))
        .ok_or_else(|| LoomError::new(Code::AuthenticationFailed, "unknown SMTP principal"))
}

async fn handle_mail(
    reader: &mut BufReader<Box<dyn SmtpIo>>,
    state: &MailSmtpState,
    session: &mut MailSmtpSession,
    argument: &str,
) -> std::io::Result<()> {
    if !session.authenticated {
        write_smtp(reader, "530 Authentication required").await?;
    } else if !argument.to_ascii_uppercase().starts_with("FROM:") {
        write_smtp(reader, "501 MAIL requires FROM").await?;
    } else {
        match parse_mail_from_argument(argument, state.limits.request_size_limit) {
            Ok(sender) => {
                session.mail_from = Some(sender);
                session.rcpt_to.clear();
                write_smtp(reader, "250 Sender accepted").await?;
            }
            Err(SmtpMailError::Syntax) => {
                write_smtp(reader, "501 MAIL FROM syntax error").await?;
            }
            Err(SmtpMailError::TooLarge) => {
                write_smtp(
                    reader,
                    "552 Message size exceeds fixed maximum message size",
                )
                .await?;
            }
        }
    }
    Ok(())
}

enum SmtpMailError {
    Syntax,
    TooLarge,
}

fn parse_mail_from_argument(argument: &str, max_size: usize) -> Result<String, SmtpMailError> {
    let (prefix, rest) = argument.split_once(':').ok_or(SmtpMailError::Syntax)?;
    if !prefix.eq_ignore_ascii_case("FROM") {
        return Err(SmtpMailError::Syntax);
    }
    let rest = rest.trim();
    if rest.is_empty() {
        return Err(SmtpMailError::Syntax);
    }
    let mut parts = rest.split_whitespace();
    let sender = parts.next().ok_or(SmtpMailError::Syntax)?.to_string();
    let mut declared_size = None;
    let mut declared_body = None;
    for param in parts {
        let Some((key, value)) = param.split_once('=') else {
            return Err(SmtpMailError::Syntax);
        };
        if key.eq_ignore_ascii_case("SIZE") {
            if value.is_empty() || declared_size.is_some() {
                return Err(SmtpMailError::Syntax);
            }
            let size = value.parse::<u128>().map_err(|_| SmtpMailError::Syntax)?;
            declared_size = Some(size);
        } else if key.eq_ignore_ascii_case("BODY") {
            if declared_body.is_some()
                || !(value.eq_ignore_ascii_case("7BIT") || value.eq_ignore_ascii_case("8BITMIME"))
            {
                return Err(SmtpMailError::Syntax);
            }
            declared_body = Some(());
        } else {
            return Err(SmtpMailError::Syntax);
        }
    }
    if declared_size.is_some_and(|size| size > max_size as u128) {
        return Err(SmtpMailError::TooLarge);
    }
    Ok(sender)
}

async fn handle_rcpt(
    reader: &mut BufReader<Box<dyn SmtpIo>>,
    session: &mut MailSmtpSession,
    argument: &str,
) -> std::io::Result<()> {
    if session.mail_from.is_none() {
        write_smtp(reader, "503 Need MAIL before RCPT").await?;
    } else if !argument.to_ascii_uppercase().starts_with("TO:") {
        write_smtp(reader, "501 RCPT requires TO").await?;
    } else {
        session.rcpt_to.push(argument[3..].trim().to_string());
        write_smtp(reader, "250 Recipient accepted").await?;
    }
    Ok(())
}

async fn handle_data(
    reader: &mut BufReader<Box<dyn SmtpIo>>,
    state: &MailSmtpState,
    session: &mut MailSmtpSession,
) -> std::io::Result<()> {
    if session.mail_from.is_none() || session.rcpt_to.is_empty() {
        write_smtp(reader, "503 Need MAIL and RCPT before DATA").await?;
        return Ok(());
    }
    write_smtp(reader, "354 End data with <CR><LF>.<CR><LF>").await?;
    let mut total = 0usize;
    loop {
        let mut line = Vec::new();
        let read = reader.read_until(b'\n', &mut line).await?;
        if read == 0 {
            return Ok(());
        }
        if line.len() > SMTP_MAX_LINE {
            write_smtp(reader, "500 Line too long").await?;
            session.mail_from = None;
            session.rcpt_to.clear();
            return Ok(());
        }
        if line == b".\r\n" || line == b".\n" {
            break;
        }
        total = total.saturating_add(line.len());
        if total > state.limits.request_size_limit {
            write_smtp(
                reader,
                "552 Message size exceeds fixed maximum message size",
            )
            .await?;
            session.mail_from = None;
            session.rcpt_to.clear();
            return Ok(());
        }
    }
    session.mail_from = None;
    session.rcpt_to.clear();
    write_smtp(reader, "250 Message accepted for setup compatibility").await
}

fn smtp_command(line: &str) -> (String, &str) {
    match line.split_once(' ') {
        Some((command, argument)) => (command.to_ascii_uppercase(), argument.trim()),
        None => (line.to_ascii_uppercase(), ""),
    }
}

async fn write_smtp(reader: &mut BufReader<Box<dyn SmtpIo>>, line: &str) -> std::io::Result<()> {
    let mut response = String::with_capacity(line.len() + 2);
    response.push_str(line);
    response.push_str("\r\n");
    reader.get_mut().write_all(response.as_bytes()).await?;
    reader.get_mut().flush().await
}

async fn write_smtp_dash(
    reader: &mut BufReader<Box<dyn SmtpIo>>,
    line: &str,
) -> std::io::Result<()> {
    let mut response = String::with_capacity(line.len() + 2);
    response.push_str(line);
    response.push_str("\r\n");
    reader.get_mut().write_all(response.as_bytes()).await
}

fn trim_eol(line: &mut String) {
    while line.ends_with('\n') || line.ends_with('\r') {
        line.pop();
    }
}

fn decode_base64_text(input: &str) -> Result<String, LoomError> {
    let decoded = decode_base64(input)?;
    String::from_utf8(decoded).map_err(|_| LoomError::invalid("SMTP AUTH field is not UTF-8"))
}

fn decode_base64(input: &str) -> Result<Vec<u8>, LoomError> {
    base64::engine::general_purpose::STANDARD
        .decode(input)
        .map_err(|_| LoomError::invalid("invalid base64"))
}

#[cfg(test)]
mod tests {
    use base64::Engine as _;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use loom_core::mail;

    use super::*;
    use loom_hosted_core::test_support::{init, nid, temp_path};

    fn smtp_state(label: &str, limit: usize) -> (std::path::PathBuf, WorkspaceId, MailSmtpState) {
        let path = temp_path(label);
        let ns = init(&path, None);
        let state = MailSmtpState {
            kernel: HostedKernel::new(&path),
            auth_policy: HostedAuthPolicy::Passphrase,
            limits: HostedHttpLimits::new(limit, 30_000, 30_000).unwrap(),
            implicit_tls: false,
            #[cfg(feature = "tls")]
            tls: None,
        };
        (path, ns, state)
    }

    async fn run_smtp_script(state: MailSmtpState, commands: &[&[u8]]) -> String {
        let (mut client, server) = tokio::io::duplex(8192);
        let server = tokio::spawn(async move { handle_smtp_connection(server, state).await });
        let mut buf = vec![0u8; 8192];
        let read = client.read(&mut buf).await.unwrap();
        let greeting = String::from_utf8_lossy(&buf[..read]);
        assert!(
            greeting.contains("Loom SMTP setup compatibility ready"),
            "{greeting}"
        );

        for command in commands {
            client.write_all(command).await.unwrap();
        }
        client.shutdown().await.unwrap();

        let mut out = Vec::new();
        client.read_to_end(&mut out).await.unwrap();
        server.await.unwrap().unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn smtp_setup_listener_accepts_authenticated_probe_without_delivery() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let (path, ns, state) = smtp_state("smtp-setup-probe", 16 * 1024);
            let kernel = state.kernel.clone();
            let auth_plain = base64::engine::general_purpose::STANDARD.encode("\0root\0root-pass");
            let auth = format!("AUTH PLAIN {auth_plain}\r\n");
            let response = run_smtp_script(
                state,
                &[
                    b"EHLO client.example\r\n",
                    auth.as_bytes(),
                    b"MAIL FROM:<example@uldrentest.com>\r\n",
                    b"RCPT TO:<example@uldrentest.com>\r\n",
                    b"DATA\r\n",
                    b"Subject: Setup Probe\r\n",
                    b"\r\n",
                    b"body\r\n",
                    b".\r\n",
                    b"NOOP\r\n",
                    b"RSET\r\n",
                    b"QUIT\r\n",
                ],
            )
            .await;

            assert!(
                response.contains("250-uldrentest.com greets client.example"),
                "{response}"
            );
            assert!(response.contains("250-AUTH PLAIN LOGIN"), "{response}");
            assert!(
                response.contains("235 Authentication successful"),
                "{response}"
            );
            assert!(response.contains("250 Sender accepted"), "{response}");
            assert!(response.contains("250 Recipient accepted"), "{response}");
            assert!(response.contains("354 End data"), "{response}");
            assert!(
                response.contains("250 Message accepted for setup compatibility"),
                "{response}"
            );
            assert!(response.contains("250 OK"), "{response}");
            assert!(response.contains("250 Reset"), "{response}");
            assert!(response.contains("221 Bye"), "{response}");

            let auth = HostedAuth::passphrase(nid(1), "root-pass", "smtp-test");
            let mailboxes = kernel
                .read(&auth, |loom| mail::list_mailboxes(loom, ns, "root"))
                .unwrap();
            assert!(mailboxes.is_empty());
            let _ = std::fs::remove_file(path);
        });
    }

    #[test]
    fn smtp_setup_listener_rejects_overlong_lines() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let (path, _ns, state) = smtp_state("smtp-line-limit", 16 * 1024);
            let mut long_command = vec![b'X'; SMTP_MAX_LINE + 1];
            long_command.extend_from_slice(b"\r\nQUIT\r\n");
            let response = run_smtp_script(state, &[&long_command]).await;
            assert!(response.contains("500 Line too long"), "{response}");
            let _ = std::fs::remove_file(path);
        });
    }

    #[test]
    fn smtp_starttls_rejects_parameters_and_reports_unavailable_without_tls() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let (path, _ns, state) = smtp_state("smtp-starttls-syntax", 16 * 1024);
            let response =
                run_smtp_script(state, &[b"STARTTLS now\r\n", b"STARTTLS\r\n", b"QUIT\r\n"]).await;
            assert!(
                response.contains("501 Syntax error: STARTTLS takes no arguments"),
                "{response}"
            );
            assert!(response.contains("454 TLS not available"), "{response}");
            assert!(response.contains("221 Bye"), "{response}");
            let _ = std::fs::remove_file(path);
        });
    }

    #[test]
    fn smtp_auth_covers_rfc4954_reply_semantics() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let (path, _ns, state) = smtp_state("smtp-auth-rfc4954", 16 * 1024);
            let auth_plain = base64::engine::general_purpose::STANDARD.encode("\0root\0root-pass");
            let bad_plain = base64::engine::general_purpose::STANDARD.encode("\0root\0bad-pass");
            let unauthorized_plain =
                base64::engine::general_purpose::STANDARD.encode("other\0root\0root-pass");
            let auth = format!("AUTH PLAIN {auth_plain}\r\n");
            let bad_auth = format!("AUTH PLAIN {bad_plain}\r\n");
            let unauthorized_auth = format!("AUTH PLAIN {unauthorized_plain}\r\n");
            let response = run_smtp_script(
                state,
                &[
                    b"EHLO client.example\r\n",
                    b"AUTH PLAIN !!!\r\n",
                    b"AUTH PLAIN\r\n",
                    b"*\r\n",
                    bad_auth.as_bytes(),
                    unauthorized_auth.as_bytes(),
                    auth.as_bytes(),
                    auth.as_bytes(),
                    b"MAIL FROM:<example@uldrentest.com>\r\n",
                    b"AUTH LOGIN\r\n",
                    b"RSET\r\n",
                    b"QUIT\r\n",
                ],
            )
            .await;

            assert!(response.contains("250-AUTH PLAIN LOGIN"), "{response}");
            assert!(
                response.contains("501 Authentication exchange invalid"),
                "{response}"
            );
            assert!(response.contains("535 Authentication failed"), "{response}");
            assert!(
                response.contains("235 Authentication successful"),
                "{response}"
            );
            assert!(response.contains("503 Already authenticated"), "{response}");
            assert!(
                response.contains("503 AUTH not permitted during mail transaction")
                    || response.matches("503 Already authenticated").count() >= 2,
                "{response}"
            );
            assert!(response.contains("250 Reset"), "{response}");
            assert!(response.contains("221 Bye"), "{response}");
            let _ = std::fs::remove_file(path);
        });
    }

    #[test]
    fn smtp_size_extension_covers_rfc1870_bounded_profile() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let (path, _ns, state) = smtp_state("smtp-size-rfc1870", 64);
            let auth_plain = base64::engine::general_purpose::STANDARD.encode("\0root\0root-pass");
            let auth = format!("AUTH PLAIN {auth_plain}\r\n");
            let response = run_smtp_script(
                state,
                &[
                    b"EHLO client.example\r\n",
                    auth.as_bytes(),
                    b"MAIL FROM:<example@uldrentest.com> SIZE=65\r\n",
                    b"MAIL FROM:<example@uldrentest.com> SIZE=not-a-number\r\n",
                    b"MAIL FROM:<example@uldrentest.com> SIZE=1 SIZE=1\r\n",
                    b"MAIL FROM:<example@uldrentest.com> SIZE=64\r\n",
                    b"RCPT TO:<example@uldrentest.com>\r\n",
                    b"DATA\r\n",
                    b"Subject: Size Probe\r\n",
                    b"\r\n",
                    b"body\r\n",
                    b".\r\n",
                    b"QUIT\r\n",
                ],
            )
            .await;

            assert!(response.contains("250 SIZE 64"), "{response}");
            assert!(
                response.contains("552 Message size exceeds fixed maximum message size"),
                "{response}"
            );
            assert!(
                response.matches("501 MAIL FROM syntax error").count() >= 2,
                "{response}"
            );
            assert!(response.contains("250 Sender accepted"), "{response}");
            assert!(
                response.contains("250 Message accepted for setup compatibility"),
                "{response}"
            );
            let _ = std::fs::remove_file(path);
        });
    }

    #[test]
    fn smtp_8bitmime_extension_covers_rfc6152_bounded_profile() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let (path, _ns, state) = smtp_state("smtp-8bitmime-rfc6152", 16 * 1024);
            let auth_plain = base64::engine::general_purpose::STANDARD.encode("\0root\0root-pass");
            let auth = format!("AUTH PLAIN {auth_plain}\r\n");
            let response = run_smtp_script(
                state,
                &[
                    b"EHLO client.example\r\n",
                    auth.as_bytes(),
                    b"MAIL FROM:<example@uldrentest.com> BODY=BINARYMIME\r\n",
                    b"MAIL FROM:<example@uldrentest.com> BODY=8BITMIME BODY=8BITMIME\r\n",
                    b"MAIL FROM:<example@uldrentest.com> BODY=8BITMIME\r\n",
                    b"RCPT TO:<example@uldrentest.com>\r\n",
                    b"DATA\r\n",
                    b"Subject: 8bit Probe\r\n",
                    b"\r\n",
                    b"caf\xc3\xa9\r\n",
                    b".\r\n",
                    b"QUIT\r\n",
                ],
            )
            .await;

            assert!(response.contains("250-8BITMIME"), "{response}");
            assert!(
                response.matches("501 MAIL FROM syntax error").count() >= 2,
                "{response}"
            );
            assert!(response.contains("250 Sender accepted"), "{response}");
            assert!(
                response.contains("250 Message accepted for setup compatibility"),
                "{response}"
            );
            let _ = std::fs::remove_file(path);
        });
    }

    #[test]
    fn smtp_setup_listener_covers_rfc5321_command_sequence_and_rejections() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let (path, _ns, state) = smtp_state("smtp-rfc5321-sequence", 16 * 1024);
            let auth_plain = base64::engine::general_purpose::STANDARD.encode("\0root\0root-pass");
            let auth = format!("AUTH PLAIN {auth_plain}\r\n");
            let response = run_smtp_script(
                state,
                &[
                    b"HELO legacy.example\r\n",
                    b"MAIL FROM:<example@uldrentest.com>\r\n",
                    b"EHLO client.example\r\n",
                    auth.as_bytes(),
                    b"RCPT TO:<example@uldrentest.com>\r\n",
                    b"MAIL bogus\r\n",
                    b"MAIL FROM:<example@uldrentest.com>\r\n",
                    b"DATA\r\n",
                    b"RCPT bogus\r\n",
                    b"RCPT TO:<example@uldrentest.com>\r\n",
                    b"VRFY example@uldrentest.com\r\n",
                    b"DATA\r\n",
                    b"Subject: Setup Probe\r\n",
                    b"\r\n",
                    b"body\r\n",
                    b".\r\n",
                    b"QUIT\r\n",
                ],
            )
            .await;

            assert!(
                response.contains("250 uldrentest.com greets legacy.example"),
                "{response}"
            );
            assert!(
                response.contains("530 Authentication required"),
                "{response}"
            );
            assert!(
                response.contains("235 Authentication successful"),
                "{response}"
            );
            assert!(response.contains("503 Need MAIL before RCPT"), "{response}");
            assert!(response.contains("501 MAIL requires FROM"), "{response}");
            assert!(response.contains("250 Sender accepted"), "{response}");
            assert!(
                response.contains("503 Need MAIL and RCPT before DATA"),
                "{response}"
            );
            assert!(response.contains("501 RCPT requires TO"), "{response}");
            assert!(response.contains("250 Recipient accepted"), "{response}");
            assert!(
                response.contains("502 Command not implemented"),
                "{response}"
            );
            assert!(
                response.contains("250 Message accepted for setup compatibility"),
                "{response}"
            );
            assert!(response.contains("221 Bye"), "{response}");
            let _ = std::fs::remove_file(path);
        });
    }
}
