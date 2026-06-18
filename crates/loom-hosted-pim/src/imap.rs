use std::future::Future;
use std::pin::Pin;

use loom_core::mail::{self, MailMessage};
use loom_core::{Code, FacetKind, LoomError, WorkspaceId, WsSelector};
use tokio::io::{
    AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader,
};
use tokio::net::TcpListener;
use tokio::task::JoinSet;

const IMAP_SHUTDOWN_GRACE: std::time::Duration = std::time::Duration::from_secs(30);

#[cfg(test)]
use crate::pim::HostedPimKernelExt;
#[cfg(feature = "tls")]
use loom_hosted_core::HostedTlsConfig;
#[cfg(feature = "tls")]
use loom_hosted_core::network_access::HostedPeerCertificate;
use loom_hosted_core::{
    HostedAuth, HostedAuthPolicy, HostedKernel,
    network_access::{current_hosted_network_access_policy, network_access_allows},
};

#[derive(Clone)]
struct MailImapState {
    kernel: HostedKernel,
    workspace: String,
    auth_policy: HostedAuthPolicy,
}

struct MailSession {
    auth: Option<HostedAuth>,
    principal: Option<String>,
    selected_mailbox: Option<String>,
    read_only: bool,
}

impl MailSession {
    fn new() -> Self {
        Self {
            auth: None,
            principal: None,
            selected_mailbox: None,
            read_only: false,
        }
    }
}

struct ImapMailboxSnapshot {
    messages: Vec<MailMessage>,
    uid_state: mail::ImapUidState,
}

impl ImapMailboxSnapshot {
    fn new(mut messages: Vec<MailMessage>, uid_state: mail::ImapUidState) -> Self {
        messages.sort_by_key(|message| {
            uid_state
                .mappings
                .iter()
                .find(|mapping| mapping.uid == message.uid)
                .map(|mapping| mapping.imap_uid)
                .unwrap_or_else(|| fallback_imap_uid(&message.uid))
        });
        Self {
            messages,
            uid_state,
        }
    }

    fn imap_uid(&self, uid: &str) -> u32 {
        self.uid_state
            .mappings
            .iter()
            .find(|mapping| mapping.uid == uid)
            .map(|mapping| mapping.imap_uid)
            .unwrap_or_else(|| fallback_imap_uid(uid))
    }

    fn max_imap_uid(&self) -> u32 {
        self.uid_state
            .mappings
            .iter()
            .map(|mapping| mapping.imap_uid)
            .max()
            .unwrap_or(0)
    }
}

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
    let state = MailImapState {
        kernel,
        workspace: workspace.into(),
        auth_policy,
    };
    serve_mail_imap_accept(listener, state, Box::pin(shutdown)).await
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
    let state = MailImapState {
        kernel,
        workspace: workspace.into(),
        auth_policy,
    };
    serve_mail_imap_tls_accept(listener, tls, state, Box::pin(shutdown)).await
}

async fn serve_mail_imap_accept(
    listener: TcpListener,
    state: MailImapState,
    mut shutdown: Pin<Box<dyn Future<Output = ()> + Send>>,
) -> std::io::Result<()> {
    let network_access_policy = current_hosted_network_access_policy();
    let mut tasks = JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, addr) = accepted?;
                if !network_access_allows(
                    network_access_policy.as_ref(),
                    addr,
                    None,
                    None,
                    None,
                ) {
                    continue;
                }
                let state = state.clone();
                tasks.spawn(async move {
                    let _ = handle_imap_connection(stream, state).await;
                });
            }
            joined = tasks.join_next(), if !tasks.is_empty() => {
                let _ = joined;
            }
            _ = &mut shutdown => break,
        }
    }
    drain_imap_connections(tasks).await;
    Ok(())
}

#[cfg(feature = "tls")]
async fn serve_mail_imap_tls_accept(
    listener: TcpListener,
    tls: HostedTlsConfig,
    state: MailImapState,
    mut shutdown: Pin<Box<dyn Future<Output = ()> + Send>>,
) -> std::io::Result<()> {
    let acceptor = tls.acceptor();
    let network_access_policy = current_hosted_network_access_policy();
    let mut tasks = JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, addr) = accepted?;
                let acceptor = acceptor.clone();
                let state = state.clone();
                let network_access_policy = network_access_policy.clone();
                tasks.spawn(async move {
                    if let Ok(stream) = acceptor.accept(stream).await {
                        let peer_certificate = stream
                            .get_ref()
                            .1
                            .peer_certificates()
                            .and_then(|chain| chain.first())
                            .map(|leaf| HostedPeerCertificate::from_leaf_der(leaf.as_ref().to_vec()));
                        if network_access_allows(
                            network_access_policy.as_ref(),
                            addr,
                            peer_certificate.as_ref(),
                            None,
                            None,
                        ) {
                            let _ = handle_imap_connection(stream, state).await;
                        }
                    }
                });
            }
            joined = tasks.join_next(), if !tasks.is_empty() => {
                let _ = joined;
            }
            _ = &mut shutdown => break,
        }
    }
    drain_imap_connections(tasks).await;
    Ok(())
}

async fn drain_imap_connections(mut tasks: JoinSet<()>) {
    let drained = async { while tasks.join_next().await.is_some() {} };
    if tokio::time::timeout(IMAP_SHUTDOWN_GRACE, drained)
        .await
        .is_err()
    {
        tasks.abort_all();
        while tasks.join_next().await.is_some() {}
    }
}

async fn handle_imap_connection<T>(stream: T, state: MailImapState) -> std::io::Result<()>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    let (reader, writer) = tokio::io::split(stream);
    let mut writer = writer;
    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    let mut session = MailSession::new();
    writer.write_all(b"* OK Loom IMAP ready\r\n").await?;
    loop {
        line.clear();
        let read = reader.read_line(&mut line).await?;
        if read == 0 {
            break;
        }
        trim_eol(&mut line);
        if line.is_empty() {
            continue;
        }
        let close = handle_imap_line(&state, &mut session, &mut reader, &mut writer, &line).await?;
        if close {
            break;
        }
    }
    Ok(())
}

async fn handle_imap_line<R, W>(
    state: &MailImapState,
    session: &mut MailSession,
    reader: &mut R,
    writer: &mut W,
    line: &str,
) -> std::io::Result<bool>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut parts = split_imap_words(line);
    if parts.len() < 2 {
        writer
            .write_all(b"* BAD expected tagged command\r\n")
            .await?;
        return Ok(false);
    }
    let tag = parts.remove(0);
    let command = parts.remove(0).to_ascii_uppercase();
    match command.as_str() {
        "CAPABILITY" => {
            writer
                .write_all(
                    format!(
                        "* CAPABILITY IMAP4rev1 AUTH=PLAIN IDLE MOVE WORKSPACE LIST-EXTENDED SPECIAL-USE\r\n{tag} OK CAPABILITY completed\r\n"
                    )
                    .as_bytes(),
                )
                .await?;
        }
        "NOOP" => {
            writer
                .write_all(format!("{tag} OK NOOP completed\r\n").as_bytes())
                .await?;
        }
        "IDLE" => {
            idle(state, session, reader, writer, &tag).await?;
        }
        "LOGOUT" => {
            writer.write_all(b"* BYE Logging out\r\n").await?;
            writer
                .write_all(format!("{tag} OK LOGOUT completed\r\n").as_bytes())
                .await?;
            writer.shutdown().await?;
            return Ok(true);
        }
        "LOGIN" => {
            if parts.len() < 2 {
                writer
                    .write_all(
                        format!("{tag} BAD LOGIN expects username and password\r\n").as_bytes(),
                    )
                    .await?;
            } else {
                let username = unquote(&parts[0]);
                let password = unquote(&parts[1]);
                match login(state, &username, &password) {
                    Ok((auth, principal)) => {
                        session.auth = Some(auth);
                        session.principal = Some(principal);
                        writer
                            .write_all(format!("{tag} OK LOGIN completed\r\n").as_bytes())
                            .await?;
                    }
                    Err(err) => {
                        state
                            .kernel
                            .audit_security_failure(&HostedAuth::unauthenticated(), &err);
                        writer
                            .write_all(format!("{tag} NO LOGIN failed\r\n").as_bytes())
                            .await?;
                    }
                }
            }
        }
        "AUTHENTICATE" => {
            if parts.is_empty() || !parts[0].eq_ignore_ascii_case("PLAIN") {
                writer
                    .write_all(
                        format!("{tag} NO unsupported authentication mechanism\r\n").as_bytes(),
                    )
                    .await?;
            } else {
                let initial_response = if parts.len() >= 2 {
                    parts[1].clone()
                } else {
                    writer.write_all(b"+ \r\n").await?;
                    let mut response = String::new();
                    let read = reader.read_line(&mut response).await?;
                    if read == 0 {
                        writer
                            .write_all(
                                format!("{tag} BAD AUTHENTICATE PLAIN cancelled\r\n").as_bytes(),
                            )
                            .await?;
                        return Ok(false);
                    }
                    trim_eol(&mut response);
                    response
                };
                match authenticate_plain(state, &initial_response) {
                    Ok((auth, principal)) => {
                        session.auth = Some(auth);
                        session.principal = Some(principal);
                        writer
                            .write_all(format!("{tag} OK AUTHENTICATE completed\r\n").as_bytes())
                            .await?;
                    }
                    Err(err) => {
                        state
                            .kernel
                            .audit_security_failure(&HostedAuth::unauthenticated(), &err);
                        writer
                            .write_all(format!("{tag} NO AUTHENTICATE failed\r\n").as_bytes())
                            .await?;
                    }
                }
            }
        }
        "ENABLE" => {
            if session_auth(session).is_none() {
                writer
                    .write_all(format!("{tag} NO authenticate first\r\n").as_bytes())
                    .await?;
                return Ok(false);
            }
            if parts.is_empty() {
                writer
                    .write_all(format!("{tag} BAD ENABLE expects capabilities\r\n").as_bytes())
                    .await?;
            } else {
                writer
                    .write_all(format!("* ENABLED\r\n{tag} OK ENABLE completed\r\n").as_bytes())
                    .await?;
            }
        }
        "LIST" => {
            let Some((auth, principal)) = session_auth(session) else {
                writer
                    .write_all(format!("{tag} NO authenticate first\r\n").as_bytes())
                    .await?;
                return Ok(false);
            };
            let Some(list_request) = parse_list_request(&parts) else {
                writer
                    .write_all(
                        format!("{tag} BAD LIST expects reference and mailbox\r\n").as_bytes(),
                    )
                    .await?;
                return Ok(false);
            };
            let reference = list_request.reference;
            let mailbox_pattern = list_request.mailbox_pattern;
            let return_status_items = list_request.return_status_items;
            if mailbox_pattern.is_empty() {
                writer
                    .write_all(b"* LIST (\\Noselect) \"/\" \"\"\r\n")
                    .await?;
                writer
                    .write_all(format!("{tag} OK LIST completed\r\n").as_bytes())
                    .await?;
                return Ok(false);
            }
            let pattern = imap_list_pattern(&reference, &mailbox_pattern);
            match list_mailboxes(state, auth, principal) {
                Ok(mailboxes) => {
                    for mailbox in mailboxes
                        .into_iter()
                        .filter(|mailbox| imap_mailbox_matches_pattern(mailbox, &pattern))
                    {
                        let name = imap_mailbox_name(&mailbox);
                        let attributes = imap_mailbox_attributes(&mailbox);
                        writer
                            .write_all(
                                format!("* LIST ({attributes}) \"/\" \"{name}\"\r\n").as_bytes(),
                            )
                            .await?;
                        if !return_status_items.is_empty() {
                            let snapshot = mailbox_snapshot(state, auth, principal, &mailbox)
                                .map_err(|err| std::io::Error::other(err.message))?;
                            let unseen =
                                unseen_count(state, auth, principal, &mailbox, &snapshot.messages)?;
                            let values = status_values(&snapshot, unseen, &return_status_items);
                            writer
                                .write_all(format!("* STATUS \"{name}\" ({values})\r\n").as_bytes())
                                .await?;
                        }
                    }
                    writer
                        .write_all(format!("{tag} OK LIST completed\r\n").as_bytes())
                        .await?;
                }
                Err(err) => write_no(writer, &tag, err).await?,
            }
        }
        "WORKSPACE" => {
            if session_auth(session).is_none() {
                writer
                    .write_all(format!("{tag} NO authenticate first\r\n").as_bytes())
                    .await?;
                return Ok(false);
            }
            writer
                .write_all(
                    format!(
                        "* WORKSPACE ((\"\" \"/\")) NIL NIL\r\n{tag} OK WORKSPACE completed\r\n"
                    )
                    .as_bytes(),
                )
                .await?;
        }
        "LSUB" => {
            let Some((auth, principal)) = session_auth(session) else {
                writer
                    .write_all(format!("{tag} NO authenticate first\r\n").as_bytes())
                    .await?;
                return Ok(false);
            };
            let Some(list_request) = parse_list_request(&parts) else {
                writer
                    .write_all(
                        format!("{tag} BAD LSUB expects reference and mailbox\r\n").as_bytes(),
                    )
                    .await?;
                return Ok(false);
            };
            let reference = list_request.reference;
            let mailbox_pattern = list_request.mailbox_pattern;
            if mailbox_pattern.is_empty() {
                writer
                    .write_all(b"* LSUB (\\Noselect) \"/\" \"\"\r\n")
                    .await?;
                writer
                    .write_all(format!("{tag} OK LSUB completed\r\n").as_bytes())
                    .await?;
                return Ok(false);
            }
            let pattern = imap_list_pattern(&reference, &mailbox_pattern);
            match list_subscribed_mailboxes(state, auth, principal) {
                Ok(mailboxes) => {
                    for mailbox in mailboxes
                        .into_iter()
                        .filter(|mailbox| imap_mailbox_matches_pattern(mailbox, &pattern))
                    {
                        let name = imap_mailbox_name(&mailbox);
                        let attributes = imap_mailbox_attributes(&mailbox);
                        writer
                            .write_all(
                                format!("* LSUB ({attributes}) \"/\" \"{name}\"\r\n").as_bytes(),
                            )
                            .await?;
                    }
                    writer
                        .write_all(format!("{tag} OK LSUB completed\r\n").as_bytes())
                        .await?;
                }
                Err(err) => write_no(writer, &tag, err).await?,
            }
        }
        "SUBSCRIBE" | "UNSUBSCRIBE" => {
            let Some((auth, principal)) = session_auth(session) else {
                writer
                    .write_all(format!("{tag} NO authenticate first\r\n").as_bytes())
                    .await?;
                return Ok(false);
            };
            if parts.is_empty() {
                writer
                    .write_all(format!("{tag} BAD {command} expects mailbox\r\n").as_bytes())
                    .await?;
                return Ok(false);
            }
            let mailbox = normalize_mailbox(&unquote(&parts[0]));
            match set_mailbox_subscription(state, auth, principal, &mailbox, command == "SUBSCRIBE")
            {
                Ok(()) => {
                    writer
                        .write_all(format!("{tag} OK {command} completed\r\n").as_bytes())
                        .await?;
                }
                Err(err) => write_no(writer, &tag, err).await?,
            }
        }
        "CREATE" => {
            create_mailbox(state, session, writer, &tag, &parts).await?;
        }
        "DELETE" => {
            delete_mailbox(state, session, writer, &tag, &parts).await?;
        }
        "RENAME" => {
            rename_mailbox(state, session, writer, &tag, &parts).await?;
        }
        "SELECT" | "EXAMINE" => {
            let Some((auth, principal)) = session_auth(session) else {
                writer
                    .write_all(format!("{tag} NO authenticate first\r\n").as_bytes())
                    .await?;
                return Ok(false);
            };
            let auth = auth.clone();
            let principal = principal.to_string();
            if parts.is_empty() {
                writer
                    .write_all(format!("{tag} BAD {command} expects mailbox\r\n").as_bytes())
                    .await?;
                return Ok(false);
            }
            let mailbox = normalize_mailbox(&unquote(&parts[0]));
            if mailbox.is_empty() {
                writer
                    .write_all(
                        format!("{tag} NO [NONEXISTENT] mailbox does not exist\r\n").as_bytes(),
                    )
                    .await?;
                return Ok(false);
            }
            match mailbox_snapshot(state, &auth, &principal, &mailbox) {
                Ok(snapshot) => {
                    let first_unseen = first_unseen_sequence(
                        state,
                        &auth,
                        &principal,
                        &mailbox,
                        &snapshot.messages,
                    )
                    .map_err(|err| std::io::Error::other(err.message))?;
                    session.selected_mailbox = Some(mailbox);
                    session.read_only = command == "EXAMINE";
                    let uid_next = snapshot.uid_state.uid_next;
                    let uid_validity = snapshot.uid_state.uid_validity;
                    let exists = snapshot.messages.len();
                    let mode = if session.read_only {
                        "READ-ONLY"
                    } else {
                        "READ-WRITE"
                    };
                    writer
                        .write_all(
                            format!(
                                "* {} EXISTS\r\n* 0 RECENT\r\n* FLAGS (\\Seen \\Answered \\Flagged \\Deleted \\Draft)\r\n* OK [UIDVALIDITY {}] UIDs valid\r\n* OK [UIDNEXT {uid_next}] Predicted next UID\r\n* OK [PERMANENTFLAGS (\\Seen \\Answered \\Flagged \\Deleted \\Draft \\*)] Permanent flags\r\n{unseen_line}{tag} OK [{mode}] {command} completed\r\n",
                                exists,
                                uid_validity,
                                unseen_line = first_unseen
                                    .map(|seq| format!("* OK [UNSEEN {seq}] First unseen\r\n"))
                                    .unwrap_or_default(),
                            )
                            .as_bytes(),
                        )
                        .await?;
                }
                Err(err) => write_no(writer, &tag, err).await?,
            }
        }
        "STATUS" => {
            let Some((auth, principal)) = session_auth(session) else {
                writer
                    .write_all(format!("{tag} NO authenticate first\r\n").as_bytes())
                    .await?;
                return Ok(false);
            };
            if parts.is_empty() {
                writer
                    .write_all(format!("{tag} BAD STATUS expects mailbox\r\n").as_bytes())
                    .await?;
                return Ok(false);
            }
            let mailbox = normalize_mailbox(&unquote(&parts[0]));
            if mailbox.is_empty() {
                writer
                    .write_all(
                        format!("{tag} NO [NONEXISTENT] mailbox does not exist\r\n").as_bytes(),
                    )
                    .await?;
                return Ok(false);
            }
            let status_items = match parse_status_items(&parts[1..]) {
                Ok(items) => items,
                Err(err) => {
                    writer
                        .write_all(format!("{tag} BAD {err}\r\n").as_bytes())
                        .await?;
                    return Ok(false);
                }
            };
            match mailbox_snapshot(state, auth, principal, &mailbox) {
                Ok(snapshot) => {
                    let unseen =
                        unseen_count(state, auth, principal, &mailbox, &snapshot.messages)?;
                    let values = status_values(&snapshot, unseen, &status_items);
                    writer
                        .write_all(
                            format!(
                                "* STATUS \"{}\" ({values})\r\n{tag} OK STATUS completed\r\n",
                                imap_mailbox_name(&mailbox),
                            )
                            .as_bytes(),
                        )
                        .await?;
                }
                Err(err) => write_no(writer, &tag, err).await?,
            }
        }
        "FETCH" => {
            fetch_messages(state, session, writer, &tag, &parts, false).await?;
        }
        "SEARCH" => {
            search_messages(state, session, writer, &tag, &parts, false).await?;
        }
        "COPY" => {
            copy_or_move_messages(state, session, writer, &tag, &parts, false, false).await?;
        }
        "MOVE" => {
            copy_or_move_messages(state, session, writer, &tag, &parts, false, true).await?;
        }
        "APPEND" => {
            append_message(state, session, reader, writer, &tag, &parts).await?;
        }
        "CHECK" => {
            if !parts.is_empty() || session.selected_mailbox.is_none() {
                writer
                    .write_all(
                        format!("{tag} BAD CHECK expects a selected mailbox and no arguments\r\n")
                            .as_bytes(),
                    )
                    .await?;
            } else {
                writer
                    .write_all(format!("{tag} OK CHECK completed\r\n").as_bytes())
                    .await?;
            }
        }
        "EXPUNGE" => {
            expunge_messages(state, session, writer, &tag, true).await?;
        }
        "CLOSE" => {
            expunge_messages(state, session, writer, &tag, false).await?;
            session.selected_mailbox = None;
            session.read_only = false;
        }
        "UNSELECT" => {
            if !parts.is_empty() || session.selected_mailbox.is_none() {
                writer
                    .write_all(
                        format!(
                            "{tag} BAD UNSELECT expects a selected mailbox and no arguments\r\n"
                        )
                        .as_bytes(),
                    )
                    .await?;
            } else {
                session.selected_mailbox = None;
                session.read_only = false;
                writer
                    .write_all(format!("{tag} OK UNSELECT completed\r\n").as_bytes())
                    .await?;
            }
        }
        "UID" => {
            if parts.is_empty() {
                writer
                    .write_all(format!("{tag} BAD UID expects subcommand\r\n").as_bytes())
                    .await?;
                return Ok(false);
            }
            let subcommand = parts[0].to_ascii_uppercase();
            match subcommand.as_str() {
                "FETCH" => fetch_messages(state, session, writer, &tag, &parts[1..], true).await?,
                "STORE" => store_flags(state, session, writer, &tag, &parts[1..], true).await?,
                "SEARCH" => {
                    search_messages(state, session, writer, &tag, &parts[1..], true).await?
                }
                "COPY" => {
                    copy_or_move_messages(state, session, writer, &tag, &parts[1..], true, false)
                        .await?
                }
                "MOVE" => {
                    copy_or_move_messages(state, session, writer, &tag, &parts[1..], true, true)
                        .await?
                }
                _ => {
                    writer
                        .write_all(format!("{tag} BAD unsupported UID subcommand\r\n").as_bytes())
                        .await?;
                }
            }
        }
        "STORE" => {
            store_flags(state, session, writer, &tag, &parts, false).await?;
        }
        _ => {
            writer
                .write_all(format!("{tag} BAD unsupported command {command}\r\n").as_bytes())
                .await?;
        }
    }
    Ok(false)
}

async fn expunge_messages<W>(
    state: &MailImapState,
    session: &MailSession,
    writer: &mut W,
    tag: &str,
    emit_untagged: bool,
) -> std::io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    let Some((auth, principal)) = session_auth(session) else {
        writer
            .write_all(format!("{tag} NO authenticate first\r\n").as_bytes())
            .await?;
        return Ok(());
    };
    let Some(mailbox) = session.selected_mailbox.as_deref() else {
        writer
            .write_all(format!("{tag} NO select a mailbox first\r\n").as_bytes())
            .await?;
        return Ok(());
    };
    if session.read_only {
        writer
            .write_all(format!("{tag} NO selected mailbox is read-only\r\n").as_bytes())
            .await?;
        return Ok(());
    }
    match expunge_deleted(state, auth, principal, mailbox) {
        Ok(sequences) => {
            if emit_untagged {
                for sequence in &sequences {
                    writer
                        .write_all(format!("* {sequence} EXPUNGE\r\n").as_bytes())
                        .await?;
                }
                writer
                    .write_all(format!("{tag} OK EXPUNGE completed\r\n").as_bytes())
                    .await?;
            } else {
                writer
                    .write_all(format!("{tag} OK CLOSE completed\r\n").as_bytes())
                    .await?;
            }
        }
        Err(err) => write_no(writer, tag, err).await?,
    }
    Ok(())
}

async fn idle<R, W>(
    state: &MailImapState,
    session: &MailSession,
    reader: &mut R,
    writer: &mut W,
    tag: &str,
) -> std::io::Result<()>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    writer.write_all(b"+ idling\r\n").await?;
    let mut known = idle_message_uids(state, session);
    let mut interval = tokio::time::interval(std::time::Duration::from_millis(750));
    let mut line = String::new();
    loop {
        tokio::select! {
            read = reader.read_line(&mut line) => {
                let read = read?;
                if read == 0 {
                    return Ok(());
                }
                trim_eol(&mut line);
                if line.eq_ignore_ascii_case("DONE") {
                    writer
                        .write_all(format!("{tag} OK IDLE completed\r\n").as_bytes())
                        .await?;
                    return Ok(());
                }
                line.clear();
            }
            _ = interval.tick() => {
                if let Some(latest) = idle_message_uids(state, session) {
                    if let Some(current) = known.as_mut() {
                        idle_write_mailbox_updates(writer, current, &latest).await?;
                    }
                    known = Some(latest);
                }
            }
        }
    }
}

fn idle_message_uids(state: &MailImapState, session: &MailSession) -> Option<Vec<String>> {
    let (auth, principal) = session_auth(session)?;
    let mailbox = session.selected_mailbox.as_deref()?;
    mailbox_snapshot(state, auth, principal, mailbox)
        .ok()
        .map(|snapshot| {
            snapshot
                .messages
                .into_iter()
                .map(|message| message.uid)
                .collect()
        })
}

async fn idle_write_mailbox_updates<W>(
    writer: &mut W,
    current: &mut Vec<String>,
    latest: &[String],
) -> std::io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    for line in idle_mailbox_update_lines(current, latest) {
        writer.write_all(line.as_bytes()).await?;
    }
    Ok(())
}

fn idle_mailbox_update_lines(current: &mut Vec<String>, latest: &[String]) -> Vec<String> {
    let mut lines = Vec::new();
    let mut idx = 0usize;
    while idx < current.len() {
        if latest.iter().any(|uid| uid == &current[idx]) {
            idx += 1;
        } else {
            lines.push(format!("* {} EXPUNGE\r\n", idx + 1));
            current.remove(idx);
        }
    }
    if latest.len() != current.len() {
        lines.push(format!("* {} EXISTS\r\n", latest.len()));
    }
    lines
}

async fn create_mailbox<W>(
    state: &MailImapState,
    session: &MailSession,
    writer: &mut W,
    tag: &str,
    parts: &[String],
) -> std::io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    let Some((auth, principal)) = session_auth(session) else {
        writer
            .write_all(format!("{tag} NO authenticate first\r\n").as_bytes())
            .await?;
        return Ok(());
    };
    if parts.is_empty() {
        writer
            .write_all(format!("{tag} BAD CREATE expects mailbox\r\n").as_bytes())
            .await?;
        return Ok(());
    }
    let mailbox = normalize_mailbox(&unquote(&parts[0]));
    match create_mailbox_record(state, auth, principal, &mailbox) {
        Ok(()) => {
            writer
                .write_all(format!("{tag} OK CREATE completed\r\n").as_bytes())
                .await?;
        }
        Err(err) => write_no(writer, tag, err).await?,
    }
    Ok(())
}

async fn delete_mailbox<W>(
    state: &MailImapState,
    session: &MailSession,
    writer: &mut W,
    tag: &str,
    parts: &[String],
) -> std::io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    let Some((auth, principal)) = session_auth(session) else {
        writer
            .write_all(format!("{tag} NO authenticate first\r\n").as_bytes())
            .await?;
        return Ok(());
    };
    if parts.is_empty() {
        writer
            .write_all(format!("{tag} BAD DELETE expects mailbox\r\n").as_bytes())
            .await?;
        return Ok(());
    }
    let mailbox = normalize_mailbox(&unquote(&parts[0]));
    match delete_mailbox_record(state, auth, principal, &mailbox) {
        Ok(_) => {
            writer
                .write_all(format!("{tag} OK DELETE completed\r\n").as_bytes())
                .await?;
        }
        Err(err) => write_no(writer, tag, err).await?,
    }
    Ok(())
}

async fn rename_mailbox<W>(
    state: &MailImapState,
    session: &mut MailSession,
    writer: &mut W,
    tag: &str,
    parts: &[String],
) -> std::io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    let Some((auth, principal)) = session_auth(session) else {
        writer
            .write_all(format!("{tag} NO authenticate first\r\n").as_bytes())
            .await?;
        return Ok(());
    };
    if parts.len() != 2 {
        writer
            .write_all(format!("{tag} BAD RENAME expects source and target mailbox\r\n").as_bytes())
            .await?;
        return Ok(());
    }
    let source = normalize_mailbox(&unquote(&parts[0]));
    let target = normalize_mailbox(&unquote(&parts[1]));
    if source.is_empty() || target.is_empty() {
        writer
            .write_all(format!("{tag} NO [NONEXISTENT] mailbox does not exist\r\n").as_bytes())
            .await?;
        return Ok(());
    }
    if source.eq_ignore_ascii_case("inbox") {
        writer
            .write_all(format!("{tag} BAD RENAME of INBOX is not supported\r\n").as_bytes())
            .await?;
        return Ok(());
    }
    match rename_mailbox_record(state, auth, principal, &source, &target) {
        Ok(()) => {
            if session
                .selected_mailbox
                .as_deref()
                .is_some_and(|selected| selected.eq_ignore_ascii_case(&source))
            {
                session.selected_mailbox = None;
                session.read_only = false;
            }
            writer
                .write_all(format!("{tag} OK RENAME completed\r\n").as_bytes())
                .await?;
        }
        Err(err) => write_no(writer, tag, err).await?,
    }
    Ok(())
}

async fn fetch_messages<W>(
    state: &MailImapState,
    session: &MailSession,
    writer: &mut W,
    tag: &str,
    parts: &[String],
    by_uid: bool,
) -> std::io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    let Some((auth, principal)) = session_auth(session) else {
        writer
            .write_all(format!("{tag} NO authenticate first\r\n").as_bytes())
            .await?;
        return Ok(());
    };
    let Some(mailbox) = session.selected_mailbox.as_deref() else {
        writer
            .write_all(format!("{tag} NO select a mailbox first\r\n").as_bytes())
            .await?;
        return Ok(());
    };
    if parts.len() < 2 {
        writer
            .write_all(format!("{tag} BAD FETCH expects set and attributes\r\n").as_bytes())
            .await?;
        return Ok(());
    }
    let mut fetch_attributes = match parse_fetch_attributes(&parts[1..].join(" ")) {
        Ok(attrs) => attrs,
        Err(err) => {
            writer
                .write_all(format!("{tag} BAD {err}\r\n").as_bytes())
                .await?;
            return Ok(());
        }
    };
    if by_uid {
        fetch_attributes.uid = true;
    }
    if fetch_attributes.is_empty() {
        writer
            .write_all(format!("{tag} BAD FETCH expects attributes\r\n").as_bytes())
            .await?;
        return Ok(());
    }
    match mailbox_snapshot(state, auth, principal, mailbox) {
        Ok(snapshot) => {
            let selected = select_messages(&snapshot, &parts[0], by_uid);
            for (seq, message) in selected {
                let mut items = Vec::new();
                if fetch_attributes.uid {
                    items.push(format!("UID {}", snapshot.imap_uid(&message.uid)));
                }
                if fetch_attributes.flags {
                    let flags = match mail_flags(state, auth, principal, mailbox, &message.uid) {
                        Ok(flags) => flags,
                        Err(err) => {
                            write_no(writer, tag, err).await?;
                            return Ok(());
                        }
                    };
                    items.push(format!("FLAGS ({})", flags.join(" ")));
                }
                if fetch_attributes.internaldate {
                    items.push(format!("INTERNALDATE {}", imap_internaldate(&message)));
                }
                if fetch_attributes.rfc822_size {
                    items.push(format!("RFC822.SIZE {}", message.size));
                }
                let envelope = imap_envelope(&message);
                let bodystructure = "(\"TEXT\" \"PLAIN\" NIL NIL NIL \"7BIT\" 0 0 NIL NIL NIL NIL)";
                if fetch_attributes.envelope {
                    items.push(format!("ENVELOPE {envelope}"));
                }
                if fetch_attributes.bodystructure {
                    items.push(format!("BODYSTRUCTURE {bodystructure}"));
                }
                if let Some(fields) = fetch_attributes.header_fields.as_ref() {
                    let raw = match mail_raw(state, auth, principal, mailbox, &message.uid) {
                        Ok(raw) => raw,
                        Err(err) => {
                            write_no(writer, tag, err).await?;
                            return Ok(());
                        }
                    };
                    let header = imap_header_fields(&raw, fields);
                    let label = format!("BODY[HEADER.FIELDS ({})]", fields.join(" "));
                    items.push(format!("{label} {{{}}}", header.len()));
                    writer
                        .write_all(format!("* {seq} FETCH ({})\r\n", items.join(" ")).as_bytes())
                        .await?;
                    writer.write_all(&header).await?;
                    writer.write_all(b")\r\n").await?;
                    continue;
                }
                if let Some(body_request) = fetch_attributes.body.as_ref() {
                    let raw = match mail_raw(state, auth, principal, mailbox, &message.uid) {
                        Ok(raw) => raw,
                        Err(err) => {
                            write_no(writer, tag, err).await?;
                            return Ok(());
                        }
                    };
                    let (label, body) = imap_body_fetch_response(body_request, &raw);
                    items.push(format!("{label} {{{}}}", body.len()));
                    writer
                        .write_all(format!("* {seq} FETCH ({})\r\n", items.join(" ")).as_bytes())
                        .await?;
                    writer.write_all(&body).await?;
                    writer.write_all(b")\r\n").await?;
                } else {
                    writer
                        .write_all(format!("* {seq} FETCH ({})\r\n", items.join(" ")).as_bytes())
                        .await?;
                }
            }
            writer
                .write_all(format!("{tag} OK FETCH completed\r\n").as_bytes())
                .await?;
        }
        Err(err) => write_no(writer, tag, err).await?,
    }
    Ok(())
}

async fn store_flags<W>(
    state: &MailImapState,
    session: &MailSession,
    writer: &mut W,
    tag: &str,
    parts: &[String],
    by_uid: bool,
) -> std::io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    let Some((auth, principal)) = session_auth(session) else {
        writer
            .write_all(format!("{tag} NO authenticate first\r\n").as_bytes())
            .await?;
        return Ok(());
    };
    let Some(mailbox) = session.selected_mailbox.as_deref() else {
        writer
            .write_all(format!("{tag} NO select a mailbox first\r\n").as_bytes())
            .await?;
        return Ok(());
    };
    if session.read_only {
        writer
            .write_all(format!("{tag} NO selected mailbox is read-only\r\n").as_bytes())
            .await?;
        return Ok(());
    }
    if parts.len() < 3 {
        writer
            .write_all(format!("{tag} BAD STORE expects set, operation, and flags\r\n").as_bytes())
            .await?;
        return Ok(());
    }
    match mailbox_snapshot(state, auth, principal, mailbox) {
        Ok(snapshot) => {
            let selected = select_messages(&snapshot, &parts[0], by_uid);
            let op = parts[1].to_ascii_uppercase();
            let silent = op.ends_with(".SILENT");
            let input_flags = parse_flags(&parts[2..].join(" "));
            for (seq, message) in &selected {
                let current = match mail_flags(state, auth, principal, mailbox, &message.uid) {
                    Ok(flags) => flags,
                    Err(err) => {
                        write_no(writer, tag, err).await?;
                        return Ok(());
                    }
                };
                let flags = apply_flags(&current, &op, &input_flags);
                match set_mail_flags(state, auth, principal, mailbox, &message.uid, &flags) {
                    Ok(()) => {
                        if !silent {
                            writer
                                .write_all(
                                    format!(
                                        "* {seq} FETCH (UID {} FLAGS ({}))\r\n",
                                        snapshot.imap_uid(&message.uid),
                                        flags.join(" ")
                                    )
                                    .as_bytes(),
                                )
                                .await?;
                        }
                    }
                    Err(err) => {
                        write_no(writer, tag, err).await?;
                        return Ok(());
                    }
                }
            }
            writer
                .write_all(format!("{tag} OK STORE completed\r\n").as_bytes())
                .await?;
        }
        Err(err) => write_no(writer, tag, err).await?,
    }
    Ok(())
}

async fn append_message<R, W>(
    state: &MailImapState,
    session: &MailSession,
    reader: &mut R,
    writer: &mut W,
    tag: &str,
    parts: &[String],
) -> std::io::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let Some((auth, principal)) = session_auth(session) else {
        writer
            .write_all(format!("{tag} NO authenticate first\r\n").as_bytes())
            .await?;
        return Ok(());
    };
    if parts.len() < 2 {
        writer
            .write_all(format!("{tag} BAD APPEND expects mailbox and literal\r\n").as_bytes())
            .await?;
        return Ok(());
    }
    let Some(literal_size) = parts.last().and_then(|part| parse_literal_size(part)) else {
        writer
            .write_all(format!("{tag} BAD APPEND expects a synchronizing literal\r\n").as_bytes())
            .await?;
        return Ok(());
    };
    let mailbox = normalize_mailbox(&unquote(&parts[0]));
    let flags = parts
        .iter()
        .skip(1)
        .take(parts.len().saturating_sub(2))
        .find(|part| part.starts_with('('))
        .map(|part| parse_flags(part))
        .unwrap_or_default();
    writer.write_all(b"+ Ready for literal\r\n").await?;
    let mut raw = vec![0u8; literal_size];
    reader.read_exact(&mut raw).await?;
    let mut eol = [0u8; 2];
    reader.read_exact(&mut eol).await?;
    if eol != *b"\r\n" {
        writer
            .write_all(format!("{tag} BAD APPEND literal must end with CRLF\r\n").as_bytes())
            .await?;
        return Ok(());
    }
    match append_mail_message(state, auth, principal, &mailbox, &raw, &flags) {
        Ok(uid) => {
            writer
                .write_all(format!("{tag} OK APPEND completed UID {uid}\r\n").as_bytes())
                .await?;
        }
        Err(err) => write_no(writer, tag, err).await?,
    }
    Ok(())
}

async fn search_messages<W>(
    state: &MailImapState,
    session: &MailSession,
    writer: &mut W,
    tag: &str,
    parts: &[String],
    by_uid: bool,
) -> std::io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    let Some((auth, principal)) = session_auth(session) else {
        writer
            .write_all(format!("{tag} NO authenticate first\r\n").as_bytes())
            .await?;
        return Ok(());
    };
    let Some(mailbox) = session.selected_mailbox.as_deref() else {
        writer
            .write_all(format!("{tag} NO select a mailbox first\r\n").as_bytes())
            .await?;
        return Ok(());
    };
    if parts.is_empty() {
        writer
            .write_all(format!("{tag} BAD SEARCH expects criteria\r\n").as_bytes())
            .await?;
        return Ok(());
    }
    let criteria = match SearchCriteria::parse(parts) {
        Ok(criteria) => criteria,
        Err(err) => {
            writer
                .write_all(format!("{tag} BAD {err}\r\n").as_bytes())
                .await?;
            return Ok(());
        }
    };
    match search_mailbox(state, auth, principal, mailbox, &criteria, by_uid) {
        Ok(values) => {
            let values = values
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(" ");
            writer
                .write_all(format!("* SEARCH {values}\r\n{tag} OK SEARCH completed\r\n").as_bytes())
                .await?;
        }
        Err(err) => write_no(writer, tag, err).await?,
    }
    Ok(())
}

async fn copy_or_move_messages<W>(
    state: &MailImapState,
    session: &MailSession,
    writer: &mut W,
    tag: &str,
    parts: &[String],
    by_uid: bool,
    move_messages: bool,
) -> std::io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    let Some((auth, principal)) = session_auth(session) else {
        writer
            .write_all(format!("{tag} NO authenticate first\r\n").as_bytes())
            .await?;
        return Ok(());
    };
    let Some(source_mailbox) = session.selected_mailbox.as_deref() else {
        writer
            .write_all(format!("{tag} NO select a mailbox first\r\n").as_bytes())
            .await?;
        return Ok(());
    };
    if session.read_only && move_messages {
        writer
            .write_all(format!("{tag} NO selected mailbox is read-only\r\n").as_bytes())
            .await?;
        return Ok(());
    }
    if parts.len() < 2 {
        let command = if move_messages { "MOVE" } else { "COPY" };
        writer
            .write_all(format!("{tag} BAD {command} expects set and mailbox\r\n").as_bytes())
            .await?;
        return Ok(());
    }
    let target_mailbox = normalize_mailbox(&unquote(&parts[1]));
    match copy_or_move_selected(
        state,
        auth,
        CopyMoveSelection {
            principal,
            source_mailbox,
            target_mailbox: &target_mailbox,
            set: &parts[0],
            by_uid,
            move_messages,
        },
    ) {
        Ok(results) => {
            if move_messages {
                for result in &results {
                    writer
                        .write_all(format!("* {} EXPUNGE\r\n", result.sequence).as_bytes())
                        .await?;
                }
                writer
                    .write_all(format!("{tag} OK MOVE completed\r\n").as_bytes())
                    .await?;
            } else {
                writer
                    .write_all(format!("{tag} OK COPY completed\r\n").as_bytes())
                    .await?;
            }
        }
        Err(err) => write_no(writer, tag, err).await?,
    }
    Ok(())
}

fn login(
    state: &MailImapState,
    username: &str,
    password: &str,
) -> Result<(HostedAuth, String), LoomError> {
    let (auth, mail_principal) = state.kernel.read(&HostedAuth::unauthenticated(), |loom| {
        let Some(identity) = loom.identity_store() else {
            if state.auth_policy == HostedAuthPolicy::OwnerOrPassphrase {
                return Ok((HostedAuth::unauthenticated(), username.to_string()));
            }
            return Err(LoomError::new(
                Code::AuthenticationFailed,
                "IMAP login requires an authenticated principal",
            ));
        };
        if !identity.authenticated_mode()
            && state.auth_policy == HostedAuthPolicy::OwnerOrPassphrase
        {
            return Ok((HostedAuth::unauthenticated(), username.to_string()));
        }
        let (principal, mail_principal) = resolve_login_principal(identity, username)?;
        let session_id = format!("imap-{principal}");
        Ok((
            HostedAuth::passphrase(principal, password, session_id),
            mail_principal,
        ))
    })?;
    state.kernel.read(&auth, |_| Ok(()))?;
    let Some(principal) = auth.principal else {
        return Ok((auth, mail_principal));
    };
    Ok((
        HostedAuth::preauthenticated(principal, auth.session_id.clone()),
        mail_principal,
    ))
}

fn authenticate_plain(
    state: &MailImapState,
    initial_response: &str,
) -> Result<(HostedAuth, String), LoomError> {
    let decoded = decode_base64(initial_response)?;
    let fields = decoded.split(|byte| *byte == 0).collect::<Vec<_>>();
    if fields.len() != 3 {
        return Err(LoomError::invalid(
            "IMAP AUTHENTICATE PLAIN expects three fields",
        ));
    }
    if fields[1].is_empty() || fields[2].is_empty() {
        return Err(LoomError::invalid(
            "IMAP AUTHENTICATE PLAIN missing credentials",
        ));
    }
    let username = std::str::from_utf8(fields[1])
        .map_err(|_| LoomError::invalid("IMAP AUTHENTICATE PLAIN username is not UTF-8"))?;
    let password = std::str::from_utf8(fields[2])
        .map_err(|_| LoomError::invalid("IMAP AUTHENTICATE PLAIN password is not UTF-8"))?;
    login(state, username, password)
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
        .ok_or_else(|| LoomError::new(Code::AuthenticationFailed, "unknown IMAP principal"))
}

fn session_auth(session: &MailSession) -> Option<(&HostedAuth, &str)> {
    Some((session.auth.as_ref()?, session.principal.as_deref()?))
}

fn list_mailboxes(
    state: &MailImapState,
    auth: &HostedAuth,
    principal: &str,
) -> Result<Vec<String>, LoomError> {
    state.kernel.read(auth, |loom| {
        let ns = resolve_mail_workspace(loom, &state.workspace)?;
        mail::list_mailboxes(loom, ns, principal)
    })
}

fn list_subscribed_mailboxes(
    state: &MailImapState,
    auth: &HostedAuth,
    principal: &str,
) -> Result<Vec<String>, LoomError> {
    state.kernel.read(auth, |loom| {
        let ns = resolve_mail_workspace(loom, &state.workspace)?;
        mail::list_imap_subscriptions(loom, ns, principal)
    })
}

fn set_mailbox_subscription(
    state: &MailImapState,
    auth: &HostedAuth,
    principal: &str,
    mailbox: &str,
    subscribe: bool,
) -> Result<(), LoomError> {
    state.kernel.write(auth, |loom| {
        let ns = resolve_mail_workspace(loom, &state.workspace)?;
        if subscribe {
            mail::subscribe_imap_mailbox(loom, ns, principal, mailbox)?;
        } else {
            mail::unsubscribe_imap_mailbox(loom, ns, principal, mailbox)?;
        }
        Ok(())
    })
}

fn mailbox_snapshot(
    state: &MailImapState,
    auth: &HostedAuth,
    principal: &str,
    mailbox: &str,
) -> Result<ImapMailboxSnapshot, LoomError> {
    state.kernel.write(auth, |loom| {
        let ns = resolve_mail_workspace(loom, &state.workspace)?;
        let uid_state = mail::ensure_imap_uid_state(loom, ns, principal, mailbox)?;
        let messages = mail::list_messages(loom, ns, principal, mailbox)?;
        Ok(ImapMailboxSnapshot::new(messages, uid_state))
    })
}

fn mail_flags(
    state: &MailImapState,
    auth: &HostedAuth,
    principal: &str,
    mailbox: &str,
    uid: &str,
) -> Result<Vec<String>, LoomError> {
    state.kernel.read(auth, |loom| {
        let ns = resolve_mail_workspace(loom, &state.workspace)?;
        mail::get_flags(loom, ns, principal, mailbox, uid).map(|flags| {
            flags
                .into_iter()
                .map(|flag| {
                    if flag.starts_with('\\') {
                        flag
                    } else {
                        format!("\\{flag}")
                    }
                })
                .collect()
        })
    })
}

fn set_mail_flags(
    state: &MailImapState,
    auth: &HostedAuth,
    principal: &str,
    mailbox: &str,
    uid: &str,
    flags: &[String],
) -> Result<(), LoomError> {
    let normalized = flags
        .iter()
        .map(|flag| flag.trim_start_matches('\\').to_string())
        .collect::<Vec<_>>();
    state.kernel.write(auth, |loom| {
        let ns = resolve_mail_workspace(loom, &state.workspace)?;
        mail::set_flags(loom, ns, principal, mailbox, uid, &normalized)
    })
}

fn mail_raw(
    state: &MailImapState,
    auth: &HostedAuth,
    principal: &str,
    mailbox: &str,
    uid: &str,
) -> Result<Vec<u8>, LoomError> {
    state.kernel.read(auth, |loom| {
        let ns = resolve_mail_workspace(loom, &state.workspace)?;
        mail::to_eml(loom, ns, principal, mailbox, uid)?
            .ok_or_else(|| LoomError::not_found(format!("mail message {uid}")))
    })
}

fn expunge_deleted(
    state: &MailImapState,
    auth: &HostedAuth,
    principal: &str,
    mailbox: &str,
) -> Result<Vec<usize>, LoomError> {
    state.kernel.write(auth, |loom| {
        let ns = resolve_mail_workspace(loom, &state.workspace)?;
        let messages = mail::list_messages(loom, ns, principal, mailbox)?;
        let mut removed = 0usize;
        let mut sequences = Vec::new();
        for (idx, message) in messages.iter().enumerate() {
            let flags = mail::get_flags(loom, ns, principal, mailbox, &message.uid)?;
            if flags.iter().any(|flag| flag_is_deleted(flag)) {
                let sequence = idx + 1 - removed;
                if mail::delete_message(loom, ns, principal, mailbox, &message.uid)? {
                    sequences.push(sequence);
                    removed += 1;
                }
            }
        }
        Ok(sequences)
    })
}

fn create_mailbox_record(
    state: &MailImapState,
    auth: &HostedAuth,
    principal: &str,
    mailbox: &str,
) -> Result<(), LoomError> {
    state.kernel.write(auth, |loom| {
        let ns = resolve_mail_workspace(loom, &state.workspace)?;
        mail::create_mailbox(
            loom,
            ns,
            principal,
            mailbox,
            &mail::MailboxMeta {
                display_name: imap_mailbox_name(mailbox),
            },
        )
    })
}

fn delete_mailbox_record(
    state: &MailImapState,
    auth: &HostedAuth,
    principal: &str,
    mailbox: &str,
) -> Result<bool, LoomError> {
    state.kernel.write(auth, |loom| {
        let ns = resolve_mail_workspace(loom, &state.workspace)?;
        mail::delete_mailbox(loom, ns, principal, mailbox)
    })
}

fn rename_mailbox_record(
    state: &MailImapState,
    auth: &HostedAuth,
    principal: &str,
    source_mailbox: &str,
    target_mailbox: &str,
) -> Result<(), LoomError> {
    state.kernel.write(auth, |loom| {
        let ns = resolve_mail_workspace(loom, &state.workspace)?;
        mail::rename_mailbox(loom, ns, principal, source_mailbox, target_mailbox)
    })
}

fn append_mail_message(
    state: &MailImapState,
    auth: &HostedAuth,
    principal: &str,
    mailbox: &str,
    raw: &[u8],
    flags: &[String],
) -> Result<u32, LoomError> {
    state.kernel.write(auth, |loom| {
        let ns = resolve_mail_workspace(loom, &state.workspace)?;
        let apple_note = apple_notes_append_target(loom, ns, principal, mailbox, raw)?;
        let (target_mailbox, existing_uid) = apple_note
            .map(|note| (note.target_mailbox, note.existing_uid))
            .unwrap_or_else(|| (mailbox.to_string(), None));
        let replacing_existing = existing_uid.is_some();
        let uid_state = mail::ensure_imap_uid_state(loom, ns, principal, &target_mailbox)?;
        let uid_text = next_mailbox_message_uid(loom, ns, principal, &target_mailbox, &uid_state)?;
        if let Some(existing_uid) = existing_uid.as_deref() {
            mail::delete_message(loom, ns, principal, &target_mailbox, existing_uid)?;
        }
        mail::ingest_message(loom, ns, principal, &target_mailbox, &uid_text, raw)?;
        if !flags.is_empty() {
            let normalized = flags
                .iter()
                .map(|flag| flag.trim_start_matches('\\').to_string())
                .collect::<Vec<_>>();
            mail::set_flags(loom, ns, principal, &target_mailbox, &uid_text, &normalized)?;
        }
        let uid_state = if replacing_existing {
            mail::reset_imap_uid_state(loom, ns, principal, &target_mailbox)?
        } else {
            mail::ensure_imap_uid_state(loom, ns, principal, &target_mailbox)?
        };
        uid_state
            .mappings
            .iter()
            .find(|mapping| mapping.uid == uid_text)
            .map(|mapping| mapping.imap_uid)
            .ok_or_else(|| LoomError::corrupt("mail: missing IMAP UID mapping after APPEND"))
    })
}

fn next_mailbox_message_uid<S: loom_core::provider::ObjectStore>(
    loom: &loom_core::vcs::Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    mailbox: &str,
    uid_state: &mail::ImapUidState,
) -> Result<String, LoomError> {
    let mut next_uid = uid_state.uid_next.max(1);
    for message in mail::list_messages(loom, ns, principal, mailbox)? {
        if let Ok(uid) = message.uid.parse::<u32>() {
            next_uid = next_uid.max(uid.saturating_add(1));
        }
    }
    Ok(next_uid.to_string())
}

struct AppleNotesAppendTarget {
    target_mailbox: String,
    existing_uid: Option<String>,
}

fn apple_notes_append_target<S: loom_core::provider::ObjectStore>(
    loom: &loom_core::vcs::Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    mailbox: &str,
    raw: &[u8],
) -> Result<Option<AppleNotesAppendTarget>, LoomError> {
    if !mailbox.eq_ignore_ascii_case("Notes") {
        return Ok(None);
    }
    let candidate = MailMessage::from_rfc5322("", "", raw)?;
    let Some(note_uuid) = mail_header_value(&candidate, "X-Universally-Unique-Identifier") else {
        return Ok(None);
    };
    if mail_header_value(&candidate, "X-Uniform-Type-Identifier")
        .is_some_and(|value| !value.eq_ignore_ascii_case("com.apple.mail-note"))
    {
        return Ok(None);
    }
    let mut mailboxes = mail::list_mailboxes(loom, ns, principal)?;
    mailboxes.sort_by_key(|candidate| {
        if candidate.eq_ignore_ascii_case(mailbox) {
            0
        } else if candidate.eq_ignore_ascii_case("Trash") {
            1
        } else {
            2
        }
    });
    for candidate_mailbox in mailboxes {
        for message in mail::list_messages(loom, ns, principal, &candidate_mailbox)? {
            if mail_header_value(&message, "X-Universally-Unique-Identifier")
                .is_some_and(|value| value.eq_ignore_ascii_case(note_uuid))
            {
                return Ok(Some(AppleNotesAppendTarget {
                    target_mailbox: candidate_mailbox,
                    existing_uid: Some(message.uid),
                }));
            }
        }
    }
    Ok(Some(AppleNotesAppendTarget {
        target_mailbox: mailbox.to_string(),
        existing_uid: None,
    }))
}

fn mail_header_value<'a>(message: &'a MailMessage, name: &str) -> Option<&'a str> {
    message
        .headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.trim())
}

fn search_mailbox(
    state: &MailImapState,
    auth: &HostedAuth,
    principal: &str,
    mailbox: &str,
    criteria: &SearchCriteria,
    by_uid: bool,
) -> Result<Vec<u32>, LoomError> {
    state.kernel.write(auth, |loom| {
        let ns = resolve_mail_workspace(loom, &state.workspace)?;
        let uid_state = mail::ensure_imap_uid_state(loom, ns, principal, mailbox)?;
        let messages = mail::list_messages(loom, ns, principal, mailbox)?;
        let snapshot = ImapMailboxSnapshot::new(messages, uid_state);
        let max_uid = snapshot.max_imap_uid();
        let mut out = Vec::new();
        for (idx, message) in snapshot.messages.iter().enumerate() {
            let imap_uid = snapshot.imap_uid(&message.uid);
            let flags = mail::get_flags(loom, ns, principal, mailbox, &message.uid)?;
            let raw = if criteria.needs_raw() {
                mail::to_eml(loom, ns, principal, mailbox, &message.uid)?.unwrap_or_default()
            } else {
                Vec::new()
            };
            if criteria.matches(message, &flags, &raw, max_uid, imap_uid) {
                let value = if by_uid {
                    imap_uid
                } else {
                    idx.saturating_add(1) as u32
                };
                out.push(value);
            }
        }
        Ok(out)
    })
}

struct CopyMoveSelection<'a> {
    principal: &'a str,
    source_mailbox: &'a str,
    target_mailbox: &'a str,
    set: &'a str,
    by_uid: bool,
    move_messages: bool,
}

struct CopyMoveResult {
    sequence: usize,
}

fn copy_or_move_selected(
    state: &MailImapState,
    auth: &HostedAuth,
    selection: CopyMoveSelection<'_>,
) -> Result<Vec<CopyMoveResult>, LoomError> {
    state.kernel.write(auth, |loom| {
        let ns = resolve_mail_workspace(loom, &state.workspace)?;
        let source_uid_state =
            mail::ensure_imap_uid_state(loom, ns, selection.principal, selection.source_mailbox)?;
        let source_messages =
            mail::list_messages(loom, ns, selection.principal, selection.source_mailbox)?;
        let source_snapshot = ImapMailboxSnapshot::new(source_messages, source_uid_state);
        let selected = select_messages(&source_snapshot, selection.set, selection.by_uid);
        let target_uid_state =
            mail::ensure_imap_uid_state(loom, ns, selection.principal, selection.target_mailbox)?;
        let mut next_uid = target_uid_state.uid_next;
        let mut moved = 0usize;
        let mut results = Vec::new();
        for (sequence, message) in selected {
            let target_uid = next_uid.to_string();
            next_uid = next_uid.saturating_add(1);
            if selection.move_messages {
                if mail::move_message(
                    loom,
                    ns,
                    selection.principal,
                    selection.source_mailbox,
                    &message.uid,
                    selection.target_mailbox,
                    &target_uid,
                )? {
                    results.push(CopyMoveResult {
                        sequence: sequence - moved,
                    });
                    moved += 1;
                }
            } else {
                let raw = mail::to_eml(
                    loom,
                    ns,
                    selection.principal,
                    selection.source_mailbox,
                    &message.uid,
                )?
                .ok_or_else(|| LoomError::not_found(format!("mail message {}", message.uid)))?;
                let flags = mail::get_flags(
                    loom,
                    ns,
                    selection.principal,
                    selection.source_mailbox,
                    &message.uid,
                )?;
                mail::ingest_message(
                    loom,
                    ns,
                    selection.principal,
                    selection.target_mailbox,
                    &target_uid,
                    &raw,
                )?;
                if !flags.is_empty() {
                    mail::set_flags(
                        loom,
                        ns,
                        selection.principal,
                        selection.target_mailbox,
                        &target_uid,
                        &flags,
                    )?;
                }
                results.push(CopyMoveResult { sequence });
            }
        }
        mail::ensure_imap_uid_state(loom, ns, selection.principal, selection.target_mailbox)?;
        Ok(results)
    })
}

fn resolve_mail_workspace<S: loom_core::provider::ObjectStore>(
    loom: &loom_core::Loom<S>,
    workspace: &str,
) -> Result<WorkspaceId, LoomError> {
    let selector = match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Typed {
            ty: FacetKind::Mail,
            name: workspace.to_string(),
        },
    };
    loom.registry().open(&selector)
}

fn unseen_count(
    state: &MailImapState,
    auth: &HostedAuth,
    principal: &str,
    mailbox: &str,
    messages: &[MailMessage],
) -> std::io::Result<usize> {
    let mut unseen = 0usize;
    for message in messages {
        let flags = mail_flags(state, auth, principal, mailbox, &message.uid)
            .map_err(|err| std::io::Error::other(err.message))?;
        if !flags.iter().any(|flag| flag.eq_ignore_ascii_case("\\Seen")) {
            unseen += 1;
        }
    }
    Ok(unseen)
}

fn first_unseen_sequence(
    state: &MailImapState,
    auth: &HostedAuth,
    principal: &str,
    mailbox: &str,
    messages: &[MailMessage],
) -> Result<Option<usize>, LoomError> {
    for (index, message) in messages.iter().enumerate() {
        let flags = mail_flags(state, auth, principal, mailbox, &message.uid)?;
        if !flags.iter().any(|flag| flag.eq_ignore_ascii_case("\\Seen")) {
            return Ok(Some(index + 1));
        }
    }
    Ok(None)
}

fn parse_status_items(parts: &[String]) -> Result<Vec<String>, String> {
    if parts.is_empty() {
        return Err("STATUS expects an item list".to_string());
    }
    let input = parts.join(" ");
    let trimmed = input.trim().trim_start_matches('(').trim_end_matches(')');
    if trimmed.is_empty() {
        return Err("STATUS expects at least one item".to_string());
    }
    let mut out = Vec::new();
    for item in trimmed.split_whitespace() {
        let item = item.to_ascii_uppercase();
        match item.as_str() {
            "MESSAGES" | "RECENT" | "UNSEEN" | "UIDNEXT" | "UIDVALIDITY" | "HIGHESTMODSEQ" => {
                out.push(item)
            }
            _ => return Err(format!("unsupported STATUS item {item}")),
        }
    }
    Ok(out)
}

fn status_values(snapshot: &ImapMailboxSnapshot, unseen: usize, items: &[String]) -> String {
    items
        .iter()
        .map(|item| match item.as_str() {
            "MESSAGES" => format!("MESSAGES {}", snapshot.messages.len()),
            "RECENT" => "RECENT 0".to_string(),
            "UNSEEN" => format!("UNSEEN {unseen}"),
            "UIDNEXT" => format!("UIDNEXT {}", snapshot.uid_state.uid_next),
            "UIDVALIDITY" => format!("UIDVALIDITY {}", snapshot.uid_state.uid_validity),
            "HIGHESTMODSEQ" => format!("HIGHESTMODSEQ {}", snapshot.uid_state.uid_next.max(1)),
            _ => unreachable!(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Clone, Debug, Default)]
struct FetchAttributes {
    uid: bool,
    flags: bool,
    internaldate: bool,
    rfc822_size: bool,
    envelope: bool,
    bodystructure: bool,
    body: Option<FetchBodyRequest>,
    header_fields: Option<Vec<String>>,
}

#[derive(Clone, Debug)]
struct FetchBodyRequest {
    label: String,
    section: FetchBodySection,
    partial: Option<(usize, usize)>,
}

#[derive(Clone, Debug)]
enum FetchBodySection {
    Full,
    Header,
    Text,
}

impl FetchAttributes {
    fn is_empty(&self) -> bool {
        !self.uid
            && !self.flags
            && !self.internaldate
            && !self.rfc822_size
            && !self.envelope
            && !self.bodystructure
            && self.body.is_none()
            && self.header_fields.is_none()
    }

    fn all(&mut self) {
        self.flags = true;
        self.internaldate = true;
        self.rfc822_size = true;
        self.envelope = true;
    }

    fn fast(&mut self) {
        self.flags = true;
        self.internaldate = true;
        self.rfc822_size = true;
    }

    fn full(&mut self) {
        self.all();
        self.bodystructure = true;
    }
}

fn parse_fetch_attributes(input: &str) -> Result<FetchAttributes, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("FETCH expects attributes".to_string());
    }
    let normalized = trimmed.to_ascii_uppercase();
    let mut out = FetchAttributes::default();
    if normalized.contains("BODY.PEEK[HEADER.FIELDS") || normalized.contains("BODY[HEADER.FIELDS") {
        if normalized.contains("BINARY") {
            return Err("unsupported FETCH attribute BINARY[]".to_string());
        }
        out.uid = fetch_contains_atom(&normalized, "UID");
        out.flags = fetch_contains_atom(&normalized, "FLAGS");
        out.internaldate = fetch_contains_atom(&normalized, "INTERNALDATE");
        out.rfc822_size = fetch_contains_atom(&normalized, "RFC822.SIZE");
        out.envelope = fetch_contains_atom(&normalized, "ENVELOPE");
        out.bodystructure = fetch_contains_atom(&normalized, "BODYSTRUCTURE");
        out.header_fields = requested_header_fields(trimmed);
        return Ok(out);
    }
    let attrs = trimmed.trim_start_matches('(').trim_end_matches(')');
    for attr in attrs.split_whitespace() {
        let normalized = attr.to_ascii_uppercase();
        match normalized.as_str() {
            "ALL" => out.all(),
            "FAST" => out.fast(),
            "FULL" => out.full(),
            "FLAGS" => out.flags = true,
            "UID" => out.uid = true,
            "RFC822" => {
                out.body = Some(FetchBodyRequest {
                    label: "RFC822".to_string(),
                    section: FetchBodySection::Full,
                    partial: None,
                });
            }
            "RFC822.SIZE" => out.rfc822_size = true,
            "ENVELOPE" => out.envelope = true,
            "BODYSTRUCTURE" => out.bodystructure = true,
            "INTERNALDATE" => out.internaldate = true,
            _ => {
                if let Some(body) = parse_body_fetch_attribute(&normalized) {
                    out.body = Some(body);
                } else {
                    return Err(format!("unsupported FETCH attribute {attr}"));
                }
            }
        }
    }
    Ok(out)
}

fn parse_body_fetch_attribute(value: &str) -> Option<FetchBodyRequest> {
    let body = value
        .strip_prefix("BODY.PEEK")
        .or_else(|| value.strip_prefix("BODY"))?;
    let (section, rest) = parse_body_fetch_section(body)?;
    let (partial_label, partial) = if rest.is_empty() {
        (String::new(), None)
    } else {
        let partial = rest.strip_prefix('<')?.strip_suffix('>')?;
        let (start, length) = partial.split_once('.')?;
        let start = start.parse::<usize>().ok()?;
        let length = length.parse::<usize>().ok()?;
        (format!("<{start}>"), Some((start, length)))
    };
    let section_label = match section {
        FetchBodySection::Full => "[]",
        FetchBodySection::Header => "[HEADER]",
        FetchBodySection::Text => "[TEXT]",
    };
    Some(FetchBodyRequest {
        label: format!("BODY{section_label}{partial_label}"),
        section,
        partial,
    })
}

fn parse_body_fetch_section(value: &str) -> Option<(FetchBodySection, &str)> {
    if let Some(rest) = value.strip_prefix("[]") {
        return Some((FetchBodySection::Full, rest));
    }
    if let Some(rest) = value.strip_prefix("[HEADER]") {
        return Some((FetchBodySection::Header, rest));
    }
    if let Some(rest) = value.strip_prefix("[TEXT]") {
        return Some((FetchBodySection::Text, rest));
    }
    None
}

fn fetch_contains_atom(input: &str, atom: &str) -> bool {
    input
        .split(|ch: char| ch.is_ascii_whitespace() || matches!(ch, '(' | ')'))
        .any(|part| part == atom)
}

fn requested_header_fields(input: &str) -> Option<Vec<String>> {
    let normalized = input.to_ascii_uppercase();
    let marker = if let Some(index) = normalized.find("BODY.PEEK[HEADER.FIELDS") {
        index
    } else {
        normalized.find("BODY[HEADER.FIELDS")?
    };
    let after_marker = &normalized[marker..];
    let start = after_marker.find('(')? + marker + 1;
    let end = normalized[start..].find(')')? + start;
    let fields = normalized[start..end]
        .split_whitespace()
        .filter(|field| !field.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    if fields.is_empty() {
        None
    } else {
        Some(fields)
    }
}

fn imap_header_fields(raw: &[u8], fields: &[String]) -> Vec<u8> {
    let wanted = fields
        .iter()
        .map(|field| field.to_ascii_lowercase())
        .collect::<std::collections::BTreeSet<_>>();
    let text = String::from_utf8_lossy(raw);
    let header = text
        .split_once("\r\n\r\n")
        .or_else(|| text.split_once("\n\n"))
        .map_or(text.as_ref(), |(header, _)| header);
    let mut out = String::new();
    let mut include_continuation = false;
    for line in header.lines() {
        if line.starts_with(' ') || line.starts_with('\t') {
            if include_continuation {
                out.push_str(line);
                out.push_str("\r\n");
            }
            continue;
        }
        include_continuation = line
            .split_once(':')
            .is_some_and(|(name, _)| wanted.contains(&name.to_ascii_lowercase()));
        if include_continuation {
            out.push_str(line);
            out.push_str("\r\n");
        }
    }
    out.push_str("\r\n");
    out.into_bytes()
}

fn imap_body_fetch_response(request: &FetchBodyRequest, raw: &[u8]) -> (String, Vec<u8>) {
    let body = match request.section {
        FetchBodySection::Full => raw.to_vec(),
        FetchBodySection::Header => imap_header_bytes(raw),
        FetchBodySection::Text => imap_text_bytes(raw),
    };
    let body = if let Some((start, length)) = request.partial {
        let start = start.min(body.len());
        let end = start.saturating_add(length).min(body.len());
        body[start..end].to_vec()
    } else {
        body
    };
    (request.label.clone(), body)
}

fn imap_header_bytes(raw: &[u8]) -> Vec<u8> {
    if let Some(index) = raw.windows(4).position(|window| window == b"\r\n\r\n") {
        return raw[..index + 4].to_vec();
    }
    if let Some(index) = raw.windows(2).position(|window| window == b"\n\n") {
        return raw[..index + 2].to_vec();
    }
    raw.to_vec()
}

fn imap_text_bytes(raw: &[u8]) -> Vec<u8> {
    if let Some(index) = raw.windows(4).position(|window| window == b"\r\n\r\n") {
        return raw[index + 4..].to_vec();
    }
    if let Some(index) = raw.windows(2).position(|window| window == b"\n\n") {
        return raw[index + 2..].to_vec();
    }
    Vec::new()
}

fn select_messages(
    snapshot: &ImapMailboxSnapshot,
    set: &str,
    by_uid: bool,
) -> Vec<(usize, MailMessage)> {
    snapshot
        .messages
        .iter()
        .enumerate()
        .filter(|(idx, message)| {
            let value = if by_uid {
                snapshot.imap_uid(&message.uid)
            } else {
                idx.saturating_add(1) as u32
            };
            let max = if by_uid {
                snapshot.max_imap_uid()
            } else {
                snapshot.messages.len() as u32
            };
            sequence_set_contains(set, value, max)
        })
        .map(|(idx, message)| (idx + 1, message.clone()))
        .collect()
}

fn sequence_set_contains(set: &str, value: u32, max: u32) -> bool {
    set.split(',').any(|part| {
        if part == "*" {
            return max != 0 && value == max;
        }
        if let Some((start, end)) = part.split_once(':') {
            if sequence_bound_exceeds(start, max) || sequence_bound_exceeds(end, max) {
                return false;
            }
            let Some(start) = sequence_bound(start, max) else {
                return false;
            };
            let Some(end) = sequence_bound(end, max) else {
                return false;
            };
            let lo = start.min(end);
            let hi = start.max(end);
            return value >= lo && value <= hi;
        }
        if sequence_bound_exceeds(part, max) {
            return false;
        }
        sequence_bound(part, max) == Some(value)
    })
}

fn sequence_bound(value: &str, max: u32) -> Option<u32> {
    if value == "*" {
        (max != 0).then_some(max)
    } else {
        value
            .parse::<u32>()
            .ok()
            .and_then(|value| (value != 0).then_some(value))
    }
}

fn sequence_bound_exceeds(value: &str, max: u32) -> bool {
    value != "*"
        && value
            .parse::<u32>()
            .is_ok_and(|value| max != 0 && value > max)
}

fn fallback_imap_uid(uid: &str) -> u32 {
    if let Ok(value) = uid.parse::<u32>()
        && value > 0
    {
        return value;
    }
    let mut hash = 2_166_136_261u32;
    for byte in uid.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(16_777_619);
    }
    hash.max(1)
}

struct ListRequest {
    reference: String,
    mailbox_pattern: String,
    return_status_items: Vec<String>,
}

fn parse_list_request(parts: &[String]) -> Option<ListRequest> {
    if parts.len() < 2 {
        return None;
    }
    let mut index = 0usize;
    if parts[index].starts_with('(') {
        index += 1;
    }
    let reference = parts.get(index).map(|part| unquote(part))?;
    let mailbox_pattern = parts.get(index + 1).map(|part| unquote(part))?;
    let return_status_items = parse_list_return_status_items(&parts[index + 2..]);
    Some(ListRequest {
        reference,
        mailbox_pattern,
        return_status_items,
    })
}

fn parse_list_return_status_items(parts: &[String]) -> Vec<String> {
    let joined = parts.join(" ");
    let upper = joined.to_ascii_uppercase();
    let Some(status_index) = upper.find("STATUS") else {
        return Vec::new();
    };
    let Some(open_relative) = upper[status_index..].find('(') else {
        return Vec::new();
    };
    let open = status_index + open_relative;
    let Some(close_relative) = upper[open + 1..].find(')') else {
        return Vec::new();
    };
    joined[open + 1..open + 1 + close_relative]
        .split_whitespace()
        .map(|item| {
            item.trim_matches(|ch| ch == '(' || ch == ')')
                .to_ascii_uppercase()
        })
        .filter(|item| !item.is_empty())
        .collect()
}

fn normalize_mailbox(mailbox: &str) -> String {
    if mailbox.eq_ignore_ascii_case("INBOX") {
        "inbox".to_string()
    } else {
        mailbox.to_string()
    }
}

fn imap_mailbox_name(mailbox: &str) -> String {
    if mailbox.eq_ignore_ascii_case("inbox") {
        "INBOX".to_string()
    } else {
        escape_quoted(mailbox)
    }
}

fn imap_mailbox_attributes(mailbox: &str) -> String {
    let mut attributes = vec!["\\HasNoChildren"];
    if let Some(special_use) = imap_special_use_attribute(mailbox) {
        attributes.push(special_use);
    }
    attributes.join(" ")
}

fn imap_special_use_attribute(mailbox: &str) -> Option<&'static str> {
    match mailbox.to_ascii_lowercase().as_str() {
        "archive" => Some("\\Archive"),
        "drafts" => Some("\\Drafts"),
        "junk" => Some("\\Junk"),
        "sent" => Some("\\Sent"),
        "trash" => Some("\\Trash"),
        _ => None,
    }
}

fn imap_list_pattern(reference: &str, mailbox_pattern: &str) -> String {
    if reference.is_empty() {
        mailbox_pattern.to_string()
    } else if reference.ends_with('/') || mailbox_pattern.starts_with('/') {
        format!("{reference}{mailbox_pattern}")
    } else {
        format!("{reference}/{mailbox_pattern}")
    }
}

fn imap_mailbox_matches_pattern(mailbox: &str, pattern: &str) -> bool {
    imap_pattern_matches(&imap_mailbox_name(mailbox), pattern)
}

fn imap_pattern_matches(value: &str, pattern: &str) -> bool {
    fn matches_inner(value: &[char], pattern: &[char]) -> bool {
        match pattern.split_first() {
            None => value.is_empty(),
            Some(('*', rest)) => {
                matches_inner(value, rest)
                    || (!value.is_empty() && matches_inner(&value[1..], pattern))
            }
            Some(('%', rest)) => {
                matches_inner(value, rest)
                    || value
                        .split_first()
                        .is_some_and(|(ch, tail)| *ch != '/' && matches_inner(tail, pattern))
            }
            Some((expected, rest)) => value
                .split_first()
                .is_some_and(|(actual, tail)| actual == expected && matches_inner(tail, rest)),
        }
    }

    let value = value.chars().collect::<Vec<_>>();
    let pattern = pattern.chars().collect::<Vec<_>>();
    matches_inner(&value, &pattern)
}

fn imap_envelope(message: &MailMessage) -> String {
    format!(
        "({} {} {} {} NIL NIL NIL {})",
        quoted_or_nil(&message.date),
        quoted_or_nil(&message.subject),
        address_list(std::slice::from_ref(&message.from)),
        address_list(&message.to),
        message
            .message_id
            .as_deref()
            .map(quoted)
            .unwrap_or_else(|| "NIL".to_string())
    )
}

fn imap_internaldate(message: &MailMessage) -> String {
    quoted(
        imap_internaldate_value(&message.date)
            .as_deref()
            .unwrap_or("01-Jan-1970 00:00:00 +0000"),
    )
}

fn imap_internaldate_value(value: &str) -> Option<String> {
    let (date, time) = value.split_once('T')?;
    let mut date_parts = date.split('-');
    let year = date_parts.next()?;
    let month = date_parts.next()?.parse::<usize>().ok()?;
    let day = date_parts.next()?;
    if date_parts.next().is_some() || !(1..=12).contains(&month) {
        return None;
    }
    let time = time.strip_suffix('Z')?;
    let mut time_parts = time.split(':');
    let hour = time_parts.next()?;
    let minute = time_parts.next()?;
    let second = time_parts.next()?.split('.').next().unwrap_or("00");
    Some(format!(
        "{}-{}-{} {}:{}:{} +0000",
        day,
        [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"
        ][month - 1],
        year,
        hour,
        minute,
        second
    ))
}

fn address_list(addresses: &[String]) -> String {
    if addresses.is_empty() {
        return "NIL".to_string();
    }
    let values = addresses
        .iter()
        .map(|address| format!("(NIL NIL {} NIL)", quoted(address)))
        .collect::<Vec<_>>()
        .join(" ");
    format!("({values})")
}

fn quoted_or_nil(value: &str) -> String {
    if value.is_empty() {
        "NIL".to_string()
    } else {
        quoted(value)
    }
}

fn quoted(value: &str) -> String {
    format!("\"{}\"", escape_quoted(value))
}

fn escape_quoted(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[derive(Default)]
struct SearchCriteria {
    all: bool,
    seen: Option<bool>,
    answered: Option<bool>,
    deleted: Option<bool>,
    flagged: Option<bool>,
    draft: Option<bool>,
    uid_set: Option<String>,
    larger: Option<u64>,
    smaller: Option<u64>,
    from: Vec<String>,
    to: Vec<String>,
    subject: Vec<String>,
    header: Vec<(String, String)>,
    body: Vec<String>,
    text: Vec<String>,
    keyword: Vec<String>,
    unkeyword: Vec<String>,
}

impl SearchCriteria {
    fn parse(parts: &[String]) -> Result<Self, String> {
        let mut criteria = Self::default();
        let mut idx = 0usize;
        if parts
            .get(idx)
            .is_some_and(|part| part.eq_ignore_ascii_case("CHARSET"))
        {
            let Some(charset) = parts.get(idx + 1) else {
                return Err("SEARCH CHARSET expects a charset".to_string());
            };
            if !charset.eq_ignore_ascii_case("UTF-8") && !charset.eq_ignore_ascii_case("US-ASCII") {
                return Err("SEARCH supports UTF-8 or US-ASCII charset".to_string());
            }
            idx += 2;
        }
        while idx < parts.len() {
            let key = parts[idx].to_ascii_uppercase();
            match key.as_str() {
                "ALL" => {
                    criteria.all = true;
                    idx += 1;
                }
                "SEEN" => {
                    criteria.seen = Some(true);
                    idx += 1;
                }
                "UNSEEN" => {
                    criteria.seen = Some(false);
                    idx += 1;
                }
                "ANSWERED" => {
                    criteria.answered = Some(true);
                    idx += 1;
                }
                "UNANSWERED" => {
                    criteria.answered = Some(false);
                    idx += 1;
                }
                "DELETED" => {
                    criteria.deleted = Some(true);
                    idx += 1;
                }
                "UNDELETED" => {
                    criteria.deleted = Some(false);
                    idx += 1;
                }
                "FLAGGED" => {
                    criteria.flagged = Some(true);
                    idx += 1;
                }
                "UNFLAGGED" => {
                    criteria.flagged = Some(false);
                    idx += 1;
                }
                "DRAFT" => {
                    criteria.draft = Some(true);
                    idx += 1;
                }
                "UNDRAFT" => {
                    criteria.draft = Some(false);
                    idx += 1;
                }
                "UID" => {
                    let Some(set) = parts.get(idx + 1) else {
                        return Err("SEARCH UID expects a sequence set".to_string());
                    };
                    criteria.uid_set = Some(set.clone());
                    idx += 2;
                }
                "LARGER" | "SMALLER" => {
                    let Some(value) = parts.get(idx + 1) else {
                        return Err(format!("SEARCH {key} expects an octet count"));
                    };
                    let size = value
                        .parse::<u64>()
                        .map_err(|_| format!("SEARCH {key} expects an octet count"))?;
                    if key == "LARGER" {
                        criteria.larger = Some(size);
                    } else {
                        criteria.smaller = Some(size);
                    }
                    idx += 2;
                }
                "FROM" | "TO" | "SUBJECT" | "BODY" | "TEXT" => {
                    let Some(value) = parts.get(idx + 1) else {
                        return Err(format!("SEARCH {key} expects text"));
                    };
                    let value = unquote(value);
                    match key.as_str() {
                        "FROM" => criteria.from.push(value),
                        "TO" => criteria.to.push(value),
                        "SUBJECT" => criteria.subject.push(value),
                        "BODY" => criteria.body.push(value),
                        "TEXT" => criteria.text.push(value),
                        _ => unreachable!(),
                    }
                    idx += 2;
                }
                "HEADER" => {
                    let (Some(name), Some(value)) = (parts.get(idx + 1), parts.get(idx + 2)) else {
                        return Err("SEARCH HEADER expects a field name and text".to_string());
                    };
                    criteria.header.push((unquote(name), unquote(value)));
                    idx += 3;
                }
                "KEYWORD" | "UNKEYWORD" => {
                    let Some(value) = parts.get(idx + 1) else {
                        return Err(format!("SEARCH {key} expects a keyword"));
                    };
                    if key == "KEYWORD" {
                        criteria.keyword.push(unquote(value));
                    } else {
                        criteria.unkeyword.push(unquote(value));
                    }
                    idx += 2;
                }
                _ => return Err(format!("unsupported SEARCH criterion {}", parts[idx])),
            }
        }
        if !criteria.all
            && criteria.seen.is_none()
            && criteria.answered.is_none()
            && criteria.deleted.is_none()
            && criteria.flagged.is_none()
            && criteria.draft.is_none()
            && criteria.uid_set.is_none()
            && criteria.larger.is_none()
            && criteria.smaller.is_none()
            && criteria.from.is_empty()
            && criteria.to.is_empty()
            && criteria.subject.is_empty()
            && criteria.header.is_empty()
            && criteria.body.is_empty()
            && criteria.text.is_empty()
            && criteria.keyword.is_empty()
            && criteria.unkeyword.is_empty()
        {
            criteria.all = true;
        }
        Ok(criteria)
    }

    fn needs_raw(&self) -> bool {
        !self.body.is_empty() || !self.text.is_empty()
    }

    fn matches(
        &self,
        message: &MailMessage,
        flags: &[String],
        raw: &[u8],
        max_uid: u32,
        imap_uid: u32,
    ) -> bool {
        if let Some(seen) = self.seen {
            let has_seen = flags.iter().any(|flag| flag_is_seen(flag));
            if has_seen != seen {
                return false;
            }
        }
        if let Some(answered) = self.answered {
            let has_answered = flags.iter().any(|flag| flag_matches(flag, "Answered"));
            if has_answered != answered {
                return false;
            }
        }
        if let Some(deleted) = self.deleted {
            let has_deleted = flags.iter().any(|flag| flag_is_deleted(flag));
            if has_deleted != deleted {
                return false;
            }
        }
        if let Some(flagged) = self.flagged {
            let has_flagged = flags.iter().any(|flag| flag_matches(flag, "Flagged"));
            if has_flagged != flagged {
                return false;
            }
        }
        if let Some(draft) = self.draft {
            let has_draft = flags.iter().any(|flag| flag_matches(flag, "Draft"));
            if has_draft != draft {
                return false;
            }
        }
        if let Some(uid_set) = self.uid_set.as_deref()
            && !sequence_set_contains(uid_set, imap_uid, max_uid)
        {
            return false;
        }
        if let Some(size) = self.larger
            && message.size <= size
        {
            return false;
        }
        if let Some(size) = self.smaller
            && message.size >= size
        {
            return false;
        }
        for value in &self.keyword {
            if !flags.iter().any(|flag| flag_matches(flag, value)) {
                return false;
            }
        }
        for value in &self.unkeyword {
            if flags.iter().any(|flag| flag_matches(flag, value)) {
                return false;
            }
        }
        for value in &self.from {
            if !contains_case_insensitive(&message.from, value) {
                return false;
            }
        }
        for value in &self.to {
            if !message
                .to
                .iter()
                .any(|to| contains_case_insensitive(to, value))
            {
                return false;
            }
        }
        for value in &self.subject {
            if !contains_case_insensitive(&message.subject, value) {
                return false;
            }
        }
        for (name, value) in &self.header {
            let matched = message.headers.iter().any(|(header_name, header_value)| {
                header_name.eq_ignore_ascii_case(name)
                    && contains_case_insensitive(header_value, value)
            });
            if !matched {
                return false;
            }
        }
        if !self.body.is_empty() {
            let raw = String::from_utf8_lossy(raw);
            for value in &self.body {
                if !contains_case_insensitive(&raw, value) {
                    return false;
                }
            }
        }
        if !self.text.is_empty() {
            let raw = String::from_utf8_lossy(raw);
            for value in &self.text {
                if !contains_case_insensitive(&raw, value) {
                    return false;
                }
            }
        }
        true
    }
}

fn parse_flags(input: &str) -> Vec<String> {
    input
        .trim()
        .trim_start_matches('(')
        .trim_end_matches(')')
        .split_whitespace()
        .map(|flag| flag.to_string())
        .collect()
}

fn parse_literal_size(input: &str) -> Option<usize> {
    let value = input.strip_prefix('{')?.strip_suffix('}')?;
    value.parse().ok()
}

fn apply_flags(current: &[String], op: &str, input: &[String]) -> Vec<String> {
    let mut out = current.to_vec();
    if op.starts_with("FLAGS") {
        out = input.to_vec();
    } else if op.starts_with("+FLAGS") {
        for flag in input {
            if !out
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(flag))
            {
                out.push(flag.clone());
            }
        }
    } else if op.starts_with("-FLAGS") {
        out.retain(|existing| !input.iter().any(|flag| existing.eq_ignore_ascii_case(flag)));
    }
    out.sort_by_key(|flag| flag.to_ascii_lowercase());
    out.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    out
}

fn flag_is_deleted(flag: &str) -> bool {
    flag.trim_start_matches('\\')
        .eq_ignore_ascii_case("Deleted")
}

fn flag_is_seen(flag: &str) -> bool {
    flag_matches(flag, "Seen")
}

fn flag_matches(flag: &str, expected: &str) -> bool {
    flag.trim_start_matches('\\').eq_ignore_ascii_case(expected)
}

fn contains_case_insensitive(haystack: &str, needle: &str) -> bool {
    haystack.to_lowercase().contains(&needle.to_lowercase())
}

fn decode_base64(input: &str) -> Result<Vec<u8>, LoomError> {
    let clean = input.trim();
    if clean.is_empty() || !clean.len().is_multiple_of(4) {
        return Err(LoomError::invalid("invalid base64"));
    }
    let mut out = Vec::with_capacity(clean.len() / 4 * 3);
    let bytes = clean.as_bytes();
    for chunk in bytes.chunks_exact(4) {
        let mut values = [0u8; 4];
        let mut padding = 0usize;
        for (idx, byte) in chunk.iter().copied().enumerate() {
            if byte == b'=' {
                padding += 1;
                values[idx] = 0;
            } else if padding > 0 {
                return Err(LoomError::invalid("invalid base64 padding"));
            } else {
                values[idx] = base64_value(byte)?;
            }
        }
        if padding > 2 {
            return Err(LoomError::invalid("invalid base64 padding"));
        }
        out.push((values[0] << 2) | (values[1] >> 4));
        if padding < 2 {
            out.push((values[1] << 4) | (values[2] >> 2));
        }
        if padding == 0 {
            out.push((values[2] << 6) | values[3]);
        }
    }
    Ok(out)
}

fn base64_value(byte: u8) -> Result<u8, LoomError> {
    match byte {
        b'A'..=b'Z' => Ok(byte - b'A'),
        b'a'..=b'z' => Ok(byte - b'a' + 26),
        b'0'..=b'9' => Ok(byte - b'0' + 52),
        b'+' => Ok(62),
        b'/' => Ok(63),
        _ => Err(LoomError::invalid("invalid base64 character")),
    }
}

fn split_imap_words(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut escaped = false;
    for ch in line.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
        } else if quoted && ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            quoted = !quoted;
            current.push(ch);
        } else if !quoted && ch.is_whitespace() {
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
        } else {
            current.push(ch);
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

fn unquote(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2 {
        trimmed[1..trimmed.len() - 1]
            .replace("\\\"", "\"")
            .replace("\\\\", "\\")
    } else {
        trimmed.to_string()
    }
}

fn trim_eol(line: &mut String) {
    while line.ends_with('\n') || line.ends_with('\r') {
        line.pop();
    }
}

async fn write_no<W>(writer: &mut W, tag: &str, err: LoomError) -> std::io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    writer
        .write_all(format!("{tag} NO [{}] {}\r\n", err.code.as_str(), err.message).as_bytes())
        .await
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use loom_hosted_core::HostedKernel;
    use loom_hosted_core::test_support::{init, nid, temp_path};

    fn setup_imap_store(
        name: &str,
        label: &str,
        messages: &[(&str, &[u8])],
    ) -> (
        PathBuf,
        WorkspaceId,
        HostedKernel,
        HostedAuth,
        MailImapState,
    ) {
        let path = temp_path(name);
        let ns = init(&path, None);
        let kernel = HostedKernel::new(&path);
        let auth = HostedAuth::passphrase(nid(1), "root-pass", label);
        kernel
            .pim()
            .mail_create_mailbox(
                &auth,
                ns,
                "root",
                "inbox",
                &mail::MailboxMeta {
                    display_name: "Inbox".to_string(),
                },
            )
            .unwrap();
        for (uid, raw) in messages {
            kernel
                .pim()
                .mail_ingest_message(&auth, ns, "root", "inbox", uid, raw)
                .unwrap();
        }
        let state = MailImapState {
            kernel: kernel.clone(),
            workspace: "main".to_string(),
            auth_policy: HostedAuthPolicy::Passphrase,
        };
        (path, ns, kernel, auth, state)
    }

    async fn run_imap_script(state: MailImapState, commands: &[&[u8]]) -> String {
        let (mut client, server) = tokio::io::duplex(8192);
        let server = tokio::spawn(async move { handle_imap_connection(server, state).await });
        let mut buf = vec![0u8; 8192];
        let read = client.read(&mut buf).await.unwrap();
        let greeting = String::from_utf8_lossy(&buf[..read]);
        assert!(greeting.contains("Loom IMAP ready"), "{greeting}");

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
    fn imap_sequence_sets_cover_ranges_and_star() {
        assert!(sequence_set_contains("1:*", 2, 3));
        assert!(sequence_set_contains("2,4", 4, 5));
        assert!(!sequence_set_contains("2:3", 4, 5));
        assert!(!sequence_set_contains("5:*", 4, 4));
        assert!(!sequence_set_contains("*:5", 4, 4));
        assert!(!sequence_set_contains("*", 0, 0));
    }

    #[test]
    fn imap_flag_ops_are_case_insensitive() {
        let current = vec!["\\Seen".to_string()];
        let added = apply_flags(&current, "+FLAGS", &["\\Flagged".to_string()]);
        assert_eq!(added, vec!["\\Flagged", "\\Seen"]);
        let removed = apply_flags(&added, "-FLAGS.SILENT", &["\\seen".to_string()]);
        assert_eq!(removed, vec!["\\Flagged"]);
    }

    #[test]
    fn imap_idle_update_lines_cover_expunge_and_exists() {
        let mut current = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let latest = vec!["a".to_string(), "c".to_string(), "d".to_string()];

        let lines = idle_mailbox_update_lines(&mut current, &latest);

        assert_eq!(lines, vec!["* 2 EXPUNGE\r\n", "* 3 EXISTS\r\n"]);
        assert_eq!(current, vec!["a", "c"]);
    }

    #[test]
    fn imap_list_pattern_matching_covers_basic_wildcards() {
        assert!(imap_mailbox_matches_pattern("inbox", "INBOX"));
        assert!(imap_mailbox_matches_pattern("inbox", "IN*"));
        assert!(imap_mailbox_matches_pattern("archive/2026", "archive/%"));
        assert!(!imap_mailbox_matches_pattern(
            "archive/2026/july",
            "archive/%"
        ));
        assert!(imap_mailbox_matches_pattern(
            "archive/2026/july",
            "archive/*"
        ));
        assert_eq!(imap_list_pattern("archive", "%"), "archive/%");
        assert_eq!(imap_list_pattern("archive/", "%"), "archive/%");
    }

    #[test]
    fn imap_base64_decoder_is_strict() {
        assert_eq!(
            decode_base64("AHJvb3QAcm9vdC1wYXNz").unwrap(),
            b"\0root\0root-pass"
        );
        assert!(decode_base64("abc").is_err());
        assert!(decode_base64("ab=c").is_err());
        assert!(decode_base64("!!!!").is_err());
    }

    #[test]
    fn imap_login_fetch_and_store_flags() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let path = temp_path("imap-round-trip");
            let ns = init(&path, None);
            let kernel = HostedKernel::new(&path);
            let auth = HostedAuth::passphrase(nid(1), "root-pass", "imap-setup");
            kernel
                .pim()
                .mail_create_mailbox(
                    &auth,
                    ns,
                    "root",
                    "inbox",
                    &mail::MailboxMeta {
                        display_name: "Inbox".to_string(),
                    },
                )
                .unwrap();
            kernel
                .pim()
                .mail_ingest_message(
                    &auth,
                    ns,
                    "root",
                    "inbox",
                    "1",
                    b"From: a@example.com\r\nTo: root@example.com\r\nSubject: Hi\r\n\r\nBody",
                )
                .unwrap();

            let state = MailImapState {
                kernel: kernel.clone(),
                workspace: "main".to_string(),
                auth_policy: HostedAuthPolicy::Passphrase,
            };
            let (mut client, server) = tokio::io::duplex(8192);
            let server = tokio::spawn(async move { handle_imap_connection(server, state).await });
            let mut buf = vec![0u8; 8192];
            let read = client.read(&mut buf).await.unwrap();
            let greeting = String::from_utf8_lossy(&buf[..read]);
            assert!(greeting.contains("Loom IMAP ready"), "{greeting}");

            client
                .write_all(b"a1 LOGIN root root-pass\r\n")
                .await
                .unwrap();
            client.write_all(b"a2 LIST \"\" *\r\n").await.unwrap();
            client.write_all(b"a3 SELECT INBOX\r\n").await.unwrap();
            client
                .write_all(b"a4 FETCH 1:* (FLAGS UID RFC822.SIZE BODY[])\r\n")
                .await
                .unwrap();
            client
                .write_all(b"a5 STORE 1 +FLAGS (\\Seen)\r\n")
                .await
                .unwrap();
            client.write_all(b"a6 LOGOUT\r\n").await.unwrap();
            client.shutdown().await.unwrap();

            let mut out = Vec::new();
            client.read_to_end(&mut out).await.unwrap();
            let response = String::from_utf8_lossy(&out);
            assert!(response.contains("a1 OK LOGIN completed"), "{response}");
            assert!(response.contains("* LIST"), "{response}");
            assert!(
                response.contains("a3 OK [READ-WRITE] SELECT completed"),
                "{response}"
            );
            assert!(response.contains("Subject: Hi"), "{response}");
            assert!(
                response.contains("* 1 FETCH (UID 1 FLAGS (\\Seen))"),
                "{response}"
            );
            assert!(response.contains("a6 OK LOGOUT completed"), "{response}");
            server.await.unwrap().unwrap();

            let flags = kernel
                .read(&auth, |loom| {
                    mail::get_flags(loom, ns, "root", "inbox", "1")
                })
                .unwrap();
            assert_eq!(flags, vec!["Seen"]);
            let _ = std::fs::remove_file(path);
        });
    }

    #[test]
    fn imap_select_and_status_empty_mailbox_name_return_nonexistent() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let path = temp_path("imap-empty-mailbox");
            let _ns = init(&path, None);
            let kernel = HostedKernel::new(&path);
            let state = MailImapState {
                kernel,
                workspace: "main".to_string(),
                auth_policy: HostedAuthPolicy::Passphrase,
            };
            let (mut client, server) = tokio::io::duplex(8192);
            let server = tokio::spawn(async move { handle_imap_connection(server, state).await });
            let mut buf = vec![0u8; 8192];
            let read = client.read(&mut buf).await.unwrap();
            let greeting = String::from_utf8_lossy(&buf[..read]);
            assert!(greeting.contains("Loom IMAP ready"), "{greeting}");

            client
                .write_all(b"a1 LOGIN root root-pass\r\n")
                .await
                .unwrap();
            client.write_all(b"a2 SELECT \"\"\r\n").await.unwrap();
            client
                .write_all(b"a3 STATUS \"\" (MESSAGES UIDNEXT)\r\n")
                .await
                .unwrap();
            client.write_all(b"a4 LOGOUT\r\n").await.unwrap();
            client.shutdown().await.unwrap();

            let mut out = Vec::new();
            client.read_to_end(&mut out).await.unwrap();
            let response = String::from_utf8_lossy(&out);
            assert!(response.contains("a1 OK LOGIN completed"), "{response}");
            assert!(
                response.contains("a2 NO [NONEXISTENT] mailbox does not exist"),
                "{response}"
            );
            assert!(
                response.contains("a3 NO [NONEXISTENT] mailbox does not exist"),
                "{response}"
            );
            assert!(!response.contains(".loom/facets/mail"), "{response}");
            server.await.unwrap().unwrap();
            let _ = std::fs::remove_file(path);
        });
    }

    #[test]
    fn imap_list_empty_mailbox_name_returns_hierarchy_root() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let (path, _ns, _kernel, _auth, state) =
                setup_imap_store("imap-list-root", "imap-list-root-setup", &[]);

            let response = run_imap_script(
                state,
                &[
                    b"a1 LOGIN root root-pass\r\n",
                    b"a2 LIST \"\" \"\"\r\n",
                    b"a3 LSUB \"\" \"\"\r\n",
                    b"a4 LOGOUT\r\n",
                ],
            )
            .await;

            assert!(
                response.contains("* LIST (\\Noselect) \"/\" \"\""),
                "{response}"
            );
            assert!(response.contains("a2 OK LIST completed"), "{response}");
            assert!(
                response.contains("* LSUB (\\Noselect) \"/\" \"\""),
                "{response}"
            );
            assert!(response.contains("a3 OK LSUB completed"), "{response}");
            assert!(!response.contains("* LIST (\\HasNoChildren)"), "{response}");
            assert!(!response.contains("* LSUB (\\HasNoChildren)"), "{response}");
            let _ = std::fs::remove_file(path);
        });
    }

    #[test]
    fn imap_list_filters_by_mailbox_pattern() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let (path, ns, kernel, auth, state) =
                setup_imap_store("imap-list-pattern", "imap-list-pattern-setup", &[]);
            kernel
                .pim()
                .mail_create_mailbox(
                    &auth,
                    ns,
                    "root",
                    "archive",
                    &mail::MailboxMeta {
                        display_name: "Archive".to_string(),
                    },
                )
                .unwrap();

            let response = run_imap_script(
                state,
                &[
                    b"a1 LOGIN root root-pass\r\n",
                    b"a2 LIST \"\" \"IN*\"\r\n",
                    b"a3 LIST \"\" \"archive\"\r\n",
                    b"a4 LOGOUT\r\n",
                ],
            )
            .await;

            let inbox_rows = response
                .matches("* LIST (\\HasNoChildren) \"/\" \"INBOX\"")
                .count();
            let archive_rows = response
                .matches("* LIST (\\HasNoChildren \\Archive) \"/\" \"archive\"")
                .count();
            assert_eq!(inbox_rows, 1, "{response}");
            assert_eq!(archive_rows, 1, "{response}");
            assert!(response.contains("a2 OK LIST completed"), "{response}");
            assert!(response.contains("a3 OK LIST completed"), "{response}");
            let _ = std::fs::remove_file(path);
        });
    }

    #[test]
    fn imap_list_supports_special_use_roles_and_extended_return() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let (path, ns, kernel, auth, state) =
                setup_imap_store("imap-special-use", "imap-special-use-setup", &[]);
            for mailbox in ["Archive", "Drafts", "Junk", "Sent", "Trash"] {
                kernel
                    .pim()
                    .mail_create_mailbox(
                        &auth,
                        ns,
                        "root",
                        mailbox,
                        &mail::MailboxMeta {
                            display_name: mailbox.to_string(),
                        },
                    )
                    .unwrap();
            }

            let response = run_imap_script(
                state,
                &[
                    b"a1 LOGIN root root-pass\r\n",
                    b"a2 LIST \"\" * RETURN (SPECIAL-USE)\r\n",
                    b"a3 LIST (SPECIAL-USE) \"\" \"*\"\r\n",
                    b"a4 LOGOUT\r\n",
                ],
            )
            .await;

            assert!(
                response.contains("* LIST (\\HasNoChildren \\Archive) \"/\" \"Archive\""),
                "{response}"
            );
            assert!(
                response.contains("* LIST (\\HasNoChildren \\Drafts) \"/\" \"Drafts\""),
                "{response}"
            );
            assert!(
                response.contains("* LIST (\\HasNoChildren \\Junk) \"/\" \"Junk\""),
                "{response}"
            );
            assert!(
                response.contains("* LIST (\\HasNoChildren \\Sent) \"/\" \"Sent\""),
                "{response}"
            );
            assert!(
                response.contains("* LIST (\\HasNoChildren \\Trash) \"/\" \"Trash\""),
                "{response}"
            );
            assert!(response.contains("a2 OK LIST completed"), "{response}");
            assert!(response.contains("a3 OK LIST completed"), "{response}");
            let _ = std::fs::remove_file(path);
        });
    }

    #[test]
    fn imap_list_extended_return_status_reports_source_backed_status() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let (path, ns, kernel, auth, state) =
                setup_imap_store("imap-list-status", "imap-list-status-setup", &[]);
            kernel
                .write(&auth, |loom| {
                    mail::create_mailbox(
                        loom,
                        ns,
                        "root",
                        "Archive",
                        &mail::MailboxMeta {
                            display_name: "Archive".to_string(),
                        },
                    )?;
                    mail::ingest_message(
                        loom,
                        ns,
                        "root",
                        "Archive",
                        "1",
                        b"From: root@example.com\r\nSubject: A\r\n\r\nbody",
                    )?;
                    mail::ensure_imap_uid_state(loom, ns, "root", "Archive")?;
                    Ok(())
                })
                .unwrap();

            let response = run_imap_script(
                state,
                &[
                    b"a1 CAPABILITY\r\n",
                    b"a2 LOGIN root root-pass\r\n",
                    b"a3 LIST \"\" \"Archive\" RETURN (STATUS (MESSAGES UIDNEXT UIDVALIDITY))\r\n",
                    b"a4 LOGOUT\r\n",
                ],
            )
            .await;

            assert!(response.contains("LIST-EXTENDED"), "{response}");
            assert!(
                response.contains("* LIST (\\HasNoChildren \\Archive) \"/\" \"Archive\""),
                "{response}"
            );
            assert!(
                response.contains("* STATUS \"Archive\" (MESSAGES 1 UIDNEXT 2 UIDVALIDITY "),
                "{response}"
            );
            assert!(response.contains("a3 OK LIST completed"), "{response}");
            let _ = std::fs::remove_file(path);
        });
    }

    #[test]
    fn imap_check_is_noop_for_selected_mailbox() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let (path, _ns, _kernel, _auth, state) =
                setup_imap_store("imap-check", "imap-check-setup", &[]);

            let response = run_imap_script(
                state,
                &[
                    b"a1 LOGIN root root-pass\r\n",
                    b"a2 CHECK\r\n",
                    b"a3 SELECT INBOX\r\n",
                    b"a4 CHECK\r\n",
                    b"a5 CHECK unexpected\r\n",
                    b"a6 LOGOUT\r\n",
                ],
            )
            .await;

            assert!(
                response.contains("a2 BAD CHECK expects a selected mailbox and no arguments"),
                "{response}"
            );
            assert!(
                response.contains("a3 OK [READ-WRITE] SELECT completed"),
                "{response}"
            );
            assert!(response.contains("a4 OK CHECK completed"), "{response}");
            assert!(
                response.contains("a5 BAD CHECK expects a selected mailbox and no arguments"),
                "{response}"
            );
            let _ = std::fs::remove_file(path);
        });
    }

    #[test]
    fn imap_fetch_only_returns_requested_metadata_without_body_literal() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let (path, _ns, _kernel, _auth, state) = setup_imap_store(
                "imap-fetch-metadata",
                "imap-fetch-metadata-setup",
                &[(
                    "1",
                    b"From: sender@example.com\r\nTo: root@example.com\r\nSubject: Metadata Only\r\nDate: Tue, 07 Jul 2026 16:00:00 +0000\r\n\r\nBody should not be sent",
                )],
            );

            let response = run_imap_script(
                state,
                &[
                    b"a1 LOGIN root root-pass\r\n",
                    b"a2 SELECT INBOX\r\n",
                    b"a3 FETCH 1:* (FLAGS UID RFC822.SIZE ENVELOPE)\r\n",
                    b"a4 LOGOUT\r\n",
                ],
            )
            .await;

            assert!(
                response.contains("* 1 FETCH (UID 1 FLAGS () RFC822.SIZE"),
                "{response}"
            );
            assert!(response.contains("ENVELOPE"), "{response}");
            assert!(!response.contains("BODY[]"), "{response}");
            assert!(!response.contains("Body should not be sent"), "{response}");
            let _ = std::fs::remove_file(path);
        });
    }

    #[test]
    fn imap_fetch_supports_partial_and_section_body_requests() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let (path, _ns, _kernel, _auth, state) = setup_imap_store(
                "imap-fetch-body-sections",
                "imap-fetch-body-sections-setup",
                &[(
                    "1",
                    b"From: sender@example.com\r\nTo: root@example.com\r\nSubject: Section Fetch\r\nDate: Tue, 07 Jul 2026 16:00:00 +0000\r\n\r\nPlain message body",
                )],
            );

            let response = run_imap_script(
                state,
                &[
                    b"a1 LOGIN root root-pass\r\n",
                    b"a2 SELECT INBOX\r\n",
                    b"a3 FETCH 1 (BODY.PEEK[]<0.16>)\r\n",
                    b"a4 FETCH 1 (BODY.PEEK[HEADER])\r\n",
                    b"a5 FETCH 1 (BODY.PEEK[TEXT])\r\n",
                    b"a6 LOGOUT\r\n",
                ],
            )
            .await;

            assert!(!response.contains(" BAD "), "{response}");
            assert!(response.contains("BODY[]<0> {16}"), "{response}");
            assert!(response.contains("BODY[HEADER]"), "{response}");
            assert!(response.contains("Subject: Section Fetch"), "{response}");
            assert!(response.contains("BODY[TEXT]"), "{response}");
            assert!(response.contains("Plain message body"), "{response}");
            let _ = std::fs::remove_file(path);
        });
    }

    #[test]
    fn imap_expunge_removes_deleted_messages_and_emits_sequence() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let (path, ns, kernel, auth, state) = setup_imap_store(
                "imap-expunge",
                "imap-expunge-setup",
                &[
                    (
                        "1",
                        b"From: delete@example.com\r\nTo: root@example.com\r\nSubject: Delete Me\r\n\r\nBody",
                    ),
                    (
                        "2",
                        b"From: keep@example.com\r\nTo: root@example.com\r\nSubject: Keep Me\r\n\r\nBody",
                    ),
                ],
            );

            let response = run_imap_script(
                state,
                &[
                    b"a1 LOGIN root root-pass\r\n",
                    b"a2 SELECT INBOX\r\n",
                    b"a3 STORE 1 +FLAGS (\\Deleted)\r\n",
                    b"a4 EXPUNGE\r\n",
                    b"a5 FETCH 1:* (FLAGS UID RFC822.SIZE BODY[])\r\n",
                    b"a6 LOGOUT\r\n",
                ],
            )
            .await;

            assert!(
                response.contains("* 1 FETCH (UID 1 FLAGS (\\Deleted))"),
                "{response}"
            );
            assert!(response.contains("* 1 EXPUNGE"), "{response}");
            assert!(response.contains("a4 OK EXPUNGE completed"), "{response}");
            assert!(response.contains("Subject: Keep Me"), "{response}");
            assert!(!response.contains("Subject: Delete Me"), "{response}");

            let messages = kernel
                .read(&auth, |loom| {
                    mail::list_messages(loom, ns, "root", "inbox")
                })
                .unwrap();
            assert_eq!(messages.len(), 1);
            assert_eq!(messages[0].uid, "2");
            let _ = std::fs::remove_file(path);
        });
    }

    #[test]
    fn imap_close_expunge_suppresses_untagged_and_unselects_mailbox() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let (path, ns, kernel, auth, state) = setup_imap_store(
                "imap-close",
                "imap-close-setup",
                &[
                    (
                        "1",
                        b"From: close-delete@example.com\r\nTo: root@example.com\r\nSubject: Close Delete\r\n\r\nBody",
                    ),
                    (
                        "2",
                        b"From: close-keep@example.com\r\nTo: root@example.com\r\nSubject: Close Keep\r\n\r\nBody",
                    ),
                ],
            );

            let response = run_imap_script(
                state,
                &[
                    b"a1 LOGIN root root-pass\r\n",
                    b"a2 SELECT INBOX\r\n",
                    b"a3 STORE 1 +FLAGS (\\Deleted)\r\n",
                    b"a4 CLOSE\r\n",
                    b"a5 FETCH 1:* (FLAGS UID RFC822.SIZE BODY[])\r\n",
                    b"a6 LOGOUT\r\n",
                ],
            )
            .await;

            assert!(
                response.contains("* 1 FETCH (UID 1 FLAGS (\\Deleted))"),
                "{response}"
            );
            assert!(!response.contains("* 1 EXPUNGE"), "{response}");
            assert!(response.contains("a4 OK CLOSE completed"), "{response}");
            assert!(response.contains("a5 NO select a mailbox first"), "{response}");

            let messages = kernel
                .read(&auth, |loom| {
                    mail::list_messages(loom, ns, "root", "inbox")
                })
                .unwrap();
            assert_eq!(messages.len(), 1);
            assert_eq!(messages[0].uid, "2");
            let _ = std::fs::remove_file(path);
        });
    }

    #[test]
    fn imap_search_filters_messages() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let (path, ns, kernel, auth, state) = setup_imap_store(
                "imap-search",
                "imap-search-setup",
                &[
                    (
                        "1",
                        b"From: alice@example.com\r\nTo: root@example.com\r\nSubject: Project Alpha\r\n\r\nAlpha body",
                    ),
                    (
                        "2",
                        b"From: bob@example.com\r\nTo: root@example.com\r\nSubject: Dinner\r\n\r\nBeta body",
                    ),
                ],
            );

            let response = run_imap_script(
                state,
                &[
                    b"a1 LOGIN root root-pass\r\n",
                    b"a2 SELECT INBOX\r\n",
                    b"a3 STORE 2 +FLAGS (\\Seen)\r\n",
                    b"a4 STORE 1 +FLAGS (\\Flagged Custom)\r\n",
                    b"a5 SEARCH UNSEEN SUBJECT Alpha\r\n",
                    b"a6 SEARCH SEEN FROM bob\r\n",
                    b"a7 UID SEARCH UID 2:*\r\n",
                    b"a8 SEARCH TEXT Beta\r\n",
                    b"a9 SEARCH FLAGGED KEYWORD Custom\r\n",
                    b"a10 SEARCH UNKEYWORD Custom\r\n",
                    b"a11 SEARCH HEADER Subject Dinner\r\n",
                    b"a12 SEARCH BODY Alpha\r\n",
                    b"a13 SEARCH LARGER 10 SMALLER 200\r\n",
                    b"a14 LOGOUT\r\n",
                ],
            )
            .await;

            assert!(
                response.contains("* SEARCH 1\r\na5 OK SEARCH completed"),
                "{response}"
            );
            assert!(
                response.contains("* SEARCH 2\r\na6 OK SEARCH completed"),
                "{response}"
            );
            assert!(
                response.contains("* SEARCH 2\r\na7 OK SEARCH completed"),
                "{response}"
            );
            assert!(
                response.contains("* SEARCH 2\r\na8 OK SEARCH completed"),
                "{response}"
            );
            assert!(
                response.contains("* SEARCH 1\r\na9 OK SEARCH completed"),
                "{response}"
            );
            assert!(
                response.contains("* SEARCH 2\r\na10 OK SEARCH completed"),
                "{response}"
            );
            assert!(
                response.contains("* SEARCH 2\r\na11 OK SEARCH completed"),
                "{response}"
            );
            assert!(
                response.contains("* SEARCH 1\r\na12 OK SEARCH completed"),
                "{response}"
            );
            assert!(
                response.contains("* SEARCH 1 2\r\na13 OK SEARCH completed"),
                "{response}"
            );
            let flags = kernel
                .read(&auth, |loom| {
                    mail::get_flags(loom, ns, "root", "inbox", "2")
                })
                .unwrap();
            assert_eq!(flags, vec!["Seen"]);
            let _ = std::fs::remove_file(path);
        });
    }

    #[test]
    fn imap_status_fetch_and_store_edge_semantics() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let (path, _ns, _kernel, _auth, state) = setup_imap_store(
                "imap-edge-semantics",
                "imap-edge-semantics-setup",
                &[(
                    "1",
                    b"From: edge@example.com\r\nTo: root@example.com\r\nSubject: Edge\r\n\r\nBody",
                )],
            );

            let response = run_imap_script(
                state,
                &[
                    b"a1 LOGIN root root-pass\r\n",
                    b"a2 SELECT INBOX\r\n",
                    b"a3 STATUS INBOX (MESSAGES UIDNEXT HIGHESTMODSEQ)\r\n",
                    b"a4 STORE 1 +FLAGS.SILENT (\\Seen)\r\n",
                    b"a5 FETCH 1 (FLAGS UID)\r\n",
                    b"a6 FETCH 1 (BINARY[])\r\n",
                    b"a7 STATUS INBOX (RECENT)\r\n",
                    b"a8 LOGOUT\r\n",
                ],
            )
            .await;

            assert!(
                response.contains("* STATUS \"INBOX\" (MESSAGES 1 UIDNEXT 2 HIGHESTMODSEQ 2)"),
                "{response}"
            );
            assert!(response.contains("* 0 RECENT"), "{response}");
            assert!(
                response.contains(
                    "* OK [PERMANENTFLAGS (\\Seen \\Answered \\Flagged \\Deleted \\Draft \\*)]"
                ),
                "{response}"
            );
            assert!(response.contains("* OK [UNSEEN 1]"), "{response}");
            assert!(response.contains("a4 OK STORE completed"), "{response}");
            assert!(
                !response.contains("* 1 FETCH (UID 1 FLAGS (\\Seen))\r\na4 OK STORE completed"),
                "{response}"
            );
            assert!(
                response.contains("* 1 FETCH (UID 1 FLAGS (\\Seen)"),
                "{response}"
            );
            assert!(
                response.contains("a6 BAD unsupported FETCH attribute BINARY[]"),
                "{response}"
            );
            assert!(
                response.contains("* STATUS \"INBOX\" (RECENT 0)"),
                "{response}"
            );
            let _ = std::fs::remove_file(path);
        });
    }

    #[test]
    fn imap_uid_state_is_stable_for_nonnumeric_message_ids() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let (path, _ns, _kernel, _auth, state) = setup_imap_store(
                "imap-durable-uids",
                "imap-durable-uids-setup",
                &[
                    (
                        "alpha",
                        b"From: alpha@example.com\r\nTo: root@example.com\r\nSubject: Alpha\r\n\r\nBody",
                    ),
                    (
                        "zeta",
                        b"From: zeta@example.com\r\nTo: root@example.com\r\nSubject: Zeta\r\n\r\nBody",
                    ),
                ],
            );
            let raw =
                b"From: append@example.com\r\nTo: root@example.com\r\nSubject: Appended\r\n\r\nBody";
            let append = format!(
                "a6 APPEND INBOX (\\Seen) {{{}}}\r\n{}\r\n",
                raw.len(),
                String::from_utf8_lossy(raw)
            );

            let response = run_imap_script(
                state,
                &[
                    b"a1 LOGIN root root-pass\r\n",
                    b"a2 SELECT INBOX\r\n",
                    b"a3 FETCH 1:* (FLAGS UID RFC822.SIZE BODY[])\r\n",
                    b"a4 UID FETCH 2 (FLAGS UID RFC822.SIZE BODY[])\r\n",
                    b"a5 STATUS INBOX (MESSAGES UIDNEXT UIDVALIDITY)\r\n",
                    append.as_bytes(),
                    b"a7 SELECT INBOX\r\n",
                    b"a8 FETCH 3 (FLAGS UID RFC822.SIZE BODY[])\r\n",
                    b"a9 LOGOUT\r\n",
                ],
            )
            .await;

            assert!(response.contains("* OK [UIDNEXT 3] Predicted next UID"), "{response}");
            assert!(response.contains("* 1 FETCH (UID 1"), "{response}");
            assert!(response.contains("* 2 FETCH (UID 2"), "{response}");
            assert!(response.contains("a6 OK APPEND completed UID 3"), "{response}");
            assert!(response.contains("* OK [UIDNEXT 4] Predicted next UID"), "{response}");
            assert!(response.contains("* 3 FETCH (UID 3 FLAGS (\\Seen)"), "{response}");
            let _ = std::fs::remove_file(path);
        });
    }

    #[test]
    fn imap_copy_and_move_messages_between_mailboxes() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let (path, ns, kernel, auth, state) = setup_imap_store(
                "imap-copy-move",
                "imap-copy-move-setup",
                &[
                    (
                        "1",
                        b"From: copy@example.com\r\nTo: root@example.com\r\nSubject: Copy Source\r\n\r\nBody",
                    ),
                    (
                        "2",
                        b"From: move@example.com\r\nTo: root@example.com\r\nSubject: Move Source\r\n\r\nBody",
                    ),
                ],
            );
            kernel
                .pim()
                .mail_create_mailbox(
                    &auth,
                    ns,
                    "root",
                    "archive",
                    &mail::MailboxMeta {
                        display_name: "Archive".to_string(),
                    },
                )
                .unwrap();

            let response = run_imap_script(
                state,
                &[
                    b"a1 LOGIN root root-pass\r\n",
                    b"a2 SELECT INBOX\r\n",
                    b"a3 COPY 1 archive\r\n",
                    b"a4 MOVE 2 archive\r\n",
                    b"a5 SELECT archive\r\n",
                    b"a6 FETCH 1:* (FLAGS UID RFC822.SIZE BODY[])\r\n",
                    b"a7 SELECT INBOX\r\n",
                    b"a8 FETCH 1:* (FLAGS UID RFC822.SIZE BODY[])\r\n",
                    b"a9 LOGOUT\r\n",
                ],
            )
            .await;

            assert!(response.contains("a3 OK COPY completed"), "{response}");
            assert!(response.contains("* 2 EXPUNGE"), "{response}");
            assert!(response.contains("a4 OK MOVE completed"), "{response}");
            assert!(response.contains("Subject: Copy Source"), "{response}");
            assert!(response.contains("Subject: Move Source"), "{response}");

            let inbox = kernel
                .read(&auth, |loom| {
                    mail::list_messages(loom, ns, "root", "inbox")
                })
                .unwrap();
            let archive = kernel
                .read(&auth, |loom| {
                    mail::list_messages(loom, ns, "root", "archive")
                })
                .unwrap();
            assert_eq!(inbox.iter().map(|message| message.uid.as_str()).collect::<Vec<_>>(), ["1"]);
            assert_eq!(
                archive
                    .iter()
                    .map(|message| message.subject.as_str())
                    .collect::<Vec<_>>(),
                ["Copy Source", "Move Source"]
            );
            let _ = std::fs::remove_file(path);
        });
    }

    #[test]
    fn imap_create_append_and_delete_mailbox() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let (path, ns, kernel, auth, state) =
                setup_imap_store("imap-append", "imap-append-setup", &[]);
            let raw =
                b"From: append@example.com\r\nTo: root@example.com\r\nSubject: Appended\r\n\r\nBody";
            let append = format!(
                "a3 APPEND Archive (\\Seen) {{{}}}\r\n{}\r\n",
                raw.len(),
                String::from_utf8_lossy(raw)
            );

            let response = run_imap_script(
                state,
                &[
                    b"a1 LOGIN root root-pass\r\n",
                    b"a2 CREATE Archive\r\n",
                    append.as_bytes(),
                    b"a4 SELECT Archive\r\n",
                    b"a5 FETCH 1:* (FLAGS UID RFC822.SIZE BODY[])\r\n",
                    b"a6 DELETE Archive\r\n",
                    b"a7 LIST \"\" *\r\n",
                    b"a8 LOGOUT\r\n",
                ],
            )
            .await;

            assert!(response.contains("a2 OK CREATE completed"), "{response}");
            assert!(response.contains("+ Ready for literal"), "{response}");
            assert!(response.contains("a3 OK APPEND completed UID 1"), "{response}");
            assert!(response.contains("Subject: Appended"), "{response}");
            assert!(
                response.contains("* 1 FETCH (UID 1 FLAGS (\\Seen)"),
                "{response}"
            );
            assert!(response.contains("a6 OK DELETE completed"), "{response}");

            let archive = kernel
                .read(&auth, |loom| {
                    mail::list_messages(loom, ns, "root", "archive")
                })
                .unwrap();
            assert!(archive.is_empty());
            let _ = std::fs::remove_file(path);
        });
    }

    #[test]
    fn imap_apple_notes_append_replaces_same_note_uuid() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let (path, ns, kernel, auth, state) =
                setup_imap_store("imap-apple-notes-upsert", "imap-apple-notes-upsert-setup", &[]);
            let raw1 = b"From: root@example.com\r\nX-Uniform-Type-Identifier: com.apple.mail-note\r\nSubject: Note one\r\nX-Universally-Unique-Identifier: B31FB018-AA45-4BEC-BC1F-1C1F36F3C85B\r\nMessage-Id: <note-1@example.com>\r\n\r\n<html>first</html>";
            let raw2 = b"From: root@example.com\r\nX-Uniform-Type-Identifier: com.apple.mail-note\r\nSubject: Note edited\r\nX-Universally-Unique-Identifier: B31FB018-AA45-4BEC-BC1F-1C1F36F3C85B\r\nMessage-Id: <note-2@example.com>\r\n\r\n<html>edited</html>";
            let append1 = format!(
                "a3 APPEND Notes (\\Seen) {{{}}}\r\n{}\r\n",
                raw1.len(),
                String::from_utf8_lossy(raw1)
            );
            let append2 = format!(
                "a4 APPEND Notes (\\Seen) {{{}}}\r\n{}\r\n",
                raw2.len(),
                String::from_utf8_lossy(raw2)
            );

            let response = run_imap_script(
                state,
                &[
                    b"a1 LOGIN root root-pass\r\n",
                    b"a2 CREATE Notes\r\n",
                    append1.as_bytes(),
                    append2.as_bytes(),
                    b"a5 SELECT Notes\r\n",
                    b"a6 FETCH 1:* (UID RFC822.SIZE BODY[])\r\n",
                    b"a7 LOGOUT\r\n",
                ],
            )
            .await;

            assert!(response.contains("a3 OK APPEND completed UID 1"), "{response}");
            assert!(response.contains("a4 OK APPEND completed UID 1"), "{response}");
            assert!(response.contains("* 1 EXISTS"), "{response}");
            assert!(response.contains("Subject: Note edited"), "{response}");
            assert!(!response.contains("Subject: Note one"), "{response}");

            let notes = kernel
                .read(&auth, |loom| {
                    mail::list_messages(loom, ns, "root", "Notes")
                })
                .unwrap();
            assert_eq!(notes.len(), 1);
            assert_eq!(notes[0].uid, "2");
            assert_eq!(notes[0].subject, "Note edited");
            let raw = kernel
                .read(&auth, |loom| {
                    mail::to_eml(loom, ns, "root", "Notes", "2")
                })
                .unwrap()
                .unwrap();
            assert!(String::from_utf8_lossy(&raw).contains("<html>edited</html>"));
            let uid_state = kernel
                .read(&auth, |loom| {
                    mail::get_imap_uid_state(loom, ns, "root", "Notes")
                })
                .unwrap()
                .unwrap();
            assert_eq!(uid_state.mappings.len(), 1);
            assert_eq!(uid_state.mappings[0].uid, "2");
            let _ = std::fs::remove_file(path);
        });
    }

    #[test]
    fn imap_apple_notes_append_does_not_resurrect_trashed_note() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let (path, ns, kernel, auth, state) =
                setup_imap_store("imap-apple-notes-trash", "imap-apple-notes-trash-setup", &[]);
            let raw1 = b"From: root@example.com\r\nX-Uniform-Type-Identifier: com.apple.mail-note\r\nSubject: Note one\r\nX-Universally-Unique-Identifier: 7F58023D-85AA-41E4-8D21-49EB5D78E37D\r\nMessage-Id: <note-trash-1@example.com>\r\n\r\n<html>first</html>";
            let raw2 = b"From: root@example.com\r\nX-Uniform-Type-Identifier: com.apple.mail-note\r\nSubject: Note edited\r\nX-Universally-Unique-Identifier: 7F58023D-85AA-41E4-8D21-49EB5D78E37D\r\nMessage-Id: <note-trash-2@example.com>\r\n\r\n<html>edited</html>";
            let append1 = format!(
                "a4 APPEND Notes (\\Seen) {{{}}}\r\n{}\r\n",
                raw1.len(),
                String::from_utf8_lossy(raw1)
            );
            let append2 = format!(
                "a7 APPEND Notes (\\Seen) {{{}}}\r\n{}\r\n",
                raw2.len(),
                String::from_utf8_lossy(raw2)
            );

            let response = run_imap_script(
                state,
                &[
                    b"a1 LOGIN root root-pass\r\n",
                    b"a2 CREATE Notes\r\n",
                    b"a3 CREATE Trash\r\n",
                    append1.as_bytes(),
                    b"a5 SELECT Notes\r\n",
                    b"a6 UID MOVE 1 Trash\r\n",
                    append2.as_bytes(),
                    b"a8 LOGOUT\r\n",
                ],
            )
            .await;

            assert!(response.contains("a6 OK MOVE completed"), "{response}");
            assert!(response.contains("a7 OK APPEND completed UID 1"), "{response}");
            let notes = kernel
                .read(&auth, |loom| {
                    mail::list_messages(loom, ns, "root", "Notes")
                })
                .unwrap();
            let trash = kernel
                .read(&auth, |loom| {
                    mail::list_messages(loom, ns, "root", "Trash")
                })
                .unwrap();
            assert!(notes.is_empty());
            assert_eq!(trash.len(), 1);
            assert_eq!(trash[0].uid, "2");
            assert_eq!(trash[0].subject, "Note edited");
            let _ = std::fs::remove_file(path);
        });
    }

    #[test]
    fn imap_apple_notes_thunderbird_delete_lifecycle_keeps_notes_deleted() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let (path, ns, kernel, auth, state) = setup_imap_store(
                "imap-apple-notes-delete-lifecycle",
                "imap-apple-notes-delete-lifecycle-setup",
                &[],
            );
            let raw1 = b"From: root@example.com\r\nX-Uniform-Type-Identifier: com.apple.mail-note\r\nSubject: A 1\r\nX-Universally-Unique-Identifier: 2A10DE81-15B0-4201-8C44-A924E44242C1\r\nMessage-Id: <note-lifecycle-1@example.com>\r\n\r\n<html>A 1</html>";
            let raw2 = b"From: root@example.com\r\nX-Uniform-Type-Identifier: com.apple.mail-note\r\nSubject: B 2\r\nX-Universally-Unique-Identifier: 0614302C-8814-4B1D-9E33-46FD0C1790A3\r\nMessage-Id: <note-lifecycle-2@example.com>\r\n\r\n<html>B 2</html>";
            let raw3 = b"From: root@example.com\r\nX-Uniform-Type-Identifier: com.apple.mail-note\r\nSubject: C 3\r\nX-Universally-Unique-Identifier: 6E91F48D-8C89-46E1-97D4-A09706DB79FE\r\nMessage-Id: <note-lifecycle-3@example.com>\r\n\r\n<html>C 3</html>";
            let raw4 = b"From: root@example.com\r\nX-Uniform-Type-Identifier: com.apple.mail-note\r\nSubject: D 4\r\nX-Universally-Unique-Identifier: D2A2C8F6-A5B7-49B6-BDC8-4DF7D5EC1C53\r\nMessage-Id: <note-lifecycle-4@example.com>\r\n\r\n<html>D 4</html>";
            let append1 = format!(
                "a3 APPEND Notes (\\Seen $NotJunk NotJunk) \"07-Jul-2026 19:59:10 -0700\" {{{}}}\r\n{}\r\n",
                raw1.len(),
                String::from_utf8_lossy(raw1)
            );
            let append2 = format!(
                "a4 APPEND Notes (\\Seen $NotJunk NotJunk) \"07-Jul-2026 19:59:14 -0700\" {{{}}}\r\n{}\r\n",
                raw2.len(),
                String::from_utf8_lossy(raw2)
            );
            let append3 = format!(
                "a5 APPEND Notes (\\Seen $NotJunk NotJunk) \"07-Jul-2026 19:59:19 -0700\" {{{}}}\r\n{}\r\n",
                raw3.len(),
                String::from_utf8_lossy(raw3)
            );
            let append4 = format!(
                "a13 APPEND Notes (\\Seen $NotJunk NotJunk) \"07-Jul-2026 20:21:53 -0700\" {{{}}}\r\n{}\r\n",
                raw4.len(),
                String::from_utf8_lossy(raw4)
            );

            let response = run_imap_script(
                state,
                &[
                    b"a1 LOGIN root root-pass\r\n",
                    b"a2 CREATE Notes\r\n",
                    b"a2b CREATE Trash\r\n",
                    append1.as_bytes(),
                    append2.as_bytes(),
                    append3.as_bytes(),
                    b"a6 SELECT Notes\r\n",
                    b"a7 UID FETCH 1:3 (UID RFC822.SIZE BODY.PEEK[])\r\n",
                    b"a8 UID move 1:3 \"Trash\"\r\n",
                    b"a9 UID FETCH 1:* (FLAGS)\r\n",
                    b"a10 SELECT Trash\r\n",
                    b"a11 UID FETCH 1:* (UID RFC822.SIZE BODY.PEEK[])\r\n",
                    b"a12 SELECT Notes\r\n",
                    append4.as_bytes(),
                    b"a14 UID FETCH 1:* (FLAGS)\r\n",
                    b"a15 UID FETCH 4 (UID RFC822.SIZE BODY.PEEK[])\r\n",
                    b"a16 FETCH 1:* (INTERNALDATE UID RFC822.SIZE BODY.PEEK[HEADER.FIELDS (date subject x-universally-unique-identifier)])\r\n",
                    b"a17 LOGOUT\r\n",
                ],
            )
            .await;

            assert!(response.contains("a8 OK MOVE completed"), "{response}");
            assert!(response.contains("a9 OK FETCH completed"), "{response}");
            assert!(response.contains("a13 OK APPEND completed UID 4"), "{response}");
            assert!(response.contains("Subject: D 4"), "{response}");

            let (notes, trash) = kernel
                .read(&auth, |loom| {
                    Ok::<_, LoomError>((
                        mail::list_messages(loom, ns, "root", "Notes")?,
                        mail::list_messages(loom, ns, "root", "Trash")?,
                    ))
                })
                .unwrap();
            assert_eq!(notes.len(), 1);
            assert_eq!(trash.len(), 3);
            assert_eq!(notes[0].subject, "D 4");
            assert_eq!(notes[0].uid, "4");

            let note_uuids = notes
                .iter()
                .filter_map(|message| mail_header_value(message, "X-Universally-Unique-Identifier"))
                .collect::<Vec<_>>();
            let trash_uuids = trash
                .iter()
                .filter_map(|message| mail_header_value(message, "X-Universally-Unique-Identifier"))
                .collect::<Vec<_>>();
            assert_eq!(note_uuids, vec!["D2A2C8F6-A5B7-49B6-BDC8-4DF7D5EC1C53"]);
            assert_eq!(
                trash_uuids,
                vec![
                    "2A10DE81-15B0-4201-8C44-A924E44242C1",
                    "0614302C-8814-4B1D-9E33-46FD0C1790A3",
                    "6E91F48D-8C89-46E1-97D4-A09706DB79FE"
                ]
            );
            let _ = std::fs::remove_file(path);
        });
    }

    #[test]
    fn imap_append_after_uidvalidity_reset_uses_highest_stored_uid() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let (path, ns, kernel, auth, state) = setup_imap_store(
                "imap-append-after-reset",
                "imap-append-after-reset-setup",
                &[],
            );
            kernel
                .write(&auth, |loom| {
                    mail::create_mailbox(
                        loom,
                        ns,
                        "root",
                        "Notes",
                        &mail::MailboxMeta {
                            display_name: "Notes".to_string(),
                        },
                    )?;
                    mail::ingest_message(
                        loom,
                        ns,
                        "root",
                        "Notes",
                        "12",
                        b"From: root@example.com\r\nSubject: Existing\r\n\r\nexisting",
                    )?;
                    mail::reset_imap_uid_state(loom, ns, "root", "Notes")?;
                    Ok(())
                })
                .unwrap();
            let raw = b"From: root@example.com\r\nSubject: New\r\n\r\nnew";
            let append = format!(
                "a3 APPEND Notes (\\Seen) {{{}}}\r\n{}\r\n",
                raw.len(),
                String::from_utf8_lossy(raw)
            );

            let response = run_imap_script(
                state,
                &[
                    b"a1 LOGIN root root-pass\r\n",
                    append.as_bytes(),
                    b"a4 LOGOUT\r\n",
                ],
            )
            .await;

            assert!(
                response.contains("a3 OK APPEND completed UID 2"),
                "{response}"
            );
            let notes = kernel
                .read(&auth, |loom| mail::list_messages(loom, ns, "root", "Notes"))
                .unwrap();
            let uids = notes
                .iter()
                .map(|message| message.uid.as_str())
                .collect::<Vec<_>>();
            assert_eq!(uids, vec!["12", "13"]);
            let _ = std::fs::remove_file(path);
        });
    }

    #[test]
    fn imap_workspace_and_idle_complete() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let (path, _ns, _kernel, _auth, state) =
                setup_imap_store("imap-workspace-idle", "imap-workspace-idle-setup", &[]);

            let response = run_imap_script(
                state,
                &[
                    b"a1 CAPABILITY\r\n",
                    b"a2 LOGIN root root-pass\r\n",
                    b"a3 WORKSPACE\r\n",
                    b"a4 IDLE\r\n",
                    b"DONE\r\n",
                    b"a5 LOGOUT\r\n",
                ],
            )
            .await;

            assert!(
                response.contains("AUTH=PLAIN IDLE MOVE WORKSPACE"),
                "{response}"
            );
            let capability = response
                .lines()
                .find(|line| line.starts_with("* CAPABILITY "))
                .unwrap_or("");
            assert!(!capability.contains("IMAP4rev2"), "{response}");
            assert!(!capability.contains("STARTTLS"), "{response}");
            assert!(capability.contains("SPECIAL-USE"), "{response}");
            assert!(
                response.contains("* WORKSPACE ((\"\" \"/\")) NIL NIL"),
                "{response}"
            );
            assert!(response.contains("+ idling"), "{response}");
            assert!(response.contains("a4 OK IDLE completed"), "{response}");
            let _ = std::fs::remove_file(path);
        });
    }

    #[test]
    fn imap_rfc9051_bounded_profile_does_not_overclaim_rev2() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let (path, _ns, _kernel, _auth, state) =
                setup_imap_store("imap-rfc9051-bounded", "imap-rfc9051-bounded-setup", &[]);

            let response = run_imap_script(
                state,
                &[
                    b"a1 CAPABILITY\r\n",
                    b"a2 LOGIN root root-pass\r\n",
                    b"a3 ENABLE IMAP4rev2 CONDSTORE\r\n",
                    b"a4 SELECT INBOX\r\n",
                    b"a5 UNSELECT\r\n",
                    b"a6 FETCH 1:* (FLAGS UID)\r\n",
                    b"a7 RENAME INBOX Archive\r\n",
                    b"a8 STARTTLS\r\n",
                    b"a9 LOGOUT\r\n",
                ],
            )
            .await;

            let capability = response
                .lines()
                .find(|line| line.starts_with("* CAPABILITY "))
                .unwrap_or("");
            assert!(!capability.contains("IMAP4rev2"), "{response}");
            assert!(!capability.contains("STARTTLS"), "{response}");
            assert!(capability.contains("SPECIAL-USE"), "{response}");
            assert!(capability.contains("LIST-EXTENDED"), "{response}");
            assert!(
                response.contains("* ENABLED\r\na3 OK ENABLE completed"),
                "{response}"
            );
            assert!(response.contains("a5 OK UNSELECT completed"), "{response}");
            assert!(
                response.contains("a6 NO select a mailbox first"),
                "{response}"
            );
            assert!(
                response.contains("a7 BAD RENAME of INBOX is not supported"),
                "{response}"
            );
            assert!(
                response.contains("a8 BAD unsupported command STARTTLS"),
                "{response}"
            );
            let _ = std::fs::remove_file(path);
        });
    }

    #[test]
    fn imap_subscription_commands_are_durable() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let (path, ns, kernel, auth, state) =
                setup_imap_store("imap-subscriptions", "imap-subscriptions-setup", &[]);

            let response = run_imap_script(
                state,
                &[
                    b"a1 LOGIN root root-pass\r\n",
                    b"a2 SUBSCRIBE INBOX\r\n",
                    b"a3 LSUB \"\" *\r\n",
                    b"a4 UNSUBSCRIBE INBOX\r\n",
                    b"a5 LSUB \"\" *\r\n",
                    b"a6 LOGOUT\r\n",
                ],
            )
            .await;

            assert!(response.contains("a2 OK SUBSCRIBE completed"), "{response}");
            assert!(
                response.contains("* LSUB (\\HasNoChildren) \"/\" \"INBOX\""),
                "{response}"
            );
            assert!(response.contains("a3 OK LSUB completed"), "{response}");
            assert!(
                response.contains("a4 OK UNSUBSCRIBE completed"),
                "{response}"
            );
            assert!(response.contains("a5 OK LSUB completed"), "{response}");
            assert_eq!(response.matches("* LSUB").count(), 1, "{response}");
            assert!(response.contains("a6 OK LOGOUT completed"), "{response}");
            let subscriptions = kernel
                .read(&auth, |loom| {
                    mail::list_imap_subscriptions(loom, ns, "root")
                })
                .unwrap();
            assert!(subscriptions.is_empty());
            let _ = std::fs::remove_file(path);
        });
    }

    #[test]
    fn imap_append_rejects_non_synchronizing_literals() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let (path, ns, kernel, auth, state) =
                setup_imap_store("imap-nonsync-literal", "imap-nonsync-literal-setup", &[]);
            let response = run_imap_script(
                state,
                &[
                    b"a1 LOGIN root root-pass\r\n",
                    b"a2 APPEND INBOX {5+}\r\nhello\r\n",
                    b"a3 LOGOUT\r\n",
                ],
            )
            .await;

            assert!(
                response.contains("a2 BAD APPEND expects a synchronizing literal"),
                "{response}"
            );
            assert!(response.contains("a3 OK LOGOUT completed"), "{response}");
            let messages = kernel
                .read(&auth, |loom| mail::list_messages(loom, ns, "root", "inbox"))
                .unwrap();
            assert!(messages.is_empty());
            let _ = std::fs::remove_file(path);
        });
    }

    #[test]
    fn imap_rename_preserves_uid_state_and_subscription() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let (path, ns, kernel, auth, state) =
                setup_imap_store("imap-rename", "imap-rename-setup", &[]);
            let before = kernel
                .write(&auth, |loom| {
                    mail::create_mailbox(
                        loom,
                        ns,
                        "root",
                        "Archive",
                        &mail::MailboxMeta {
                            display_name: "Archive".to_string(),
                        },
                    )?;
                    mail::ingest_message(
                        loom,
                        ns,
                        "root",
                        "Archive",
                        "1",
                        b"From: root@example.com\r\nSubject: Archived\r\n\r\nbody",
                    )?;
                    mail::subscribe_imap_mailbox(loom, ns, "root", "Archive")?;
                    mail::ensure_imap_uid_state(loom, ns, "root", "Archive")
                })
                .unwrap();

            let response = run_imap_script(
                state,
                &[
                    b"a1 LOGIN root root-pass\r\n",
                    b"a2 RENAME Archive Renamed\r\n",
                    b"a3 LSUB \"\" *\r\n",
                    b"a4 STATUS Renamed (MESSAGES UIDNEXT UIDVALIDITY)\r\n",
                    b"a5 SELECT Renamed\r\n",
                    b"a6 LOGOUT\r\n",
                ],
            )
            .await;

            assert!(response.contains("a2 OK RENAME completed"), "{response}");
            assert!(
                response.contains("* LSUB (\\HasNoChildren) \"/\" \"Renamed\""),
                "{response}"
            );
            assert!(
                response.contains(&format!(
                    "* STATUS \"Renamed\" (MESSAGES 1 UIDNEXT {} UIDVALIDITY {})",
                    before.uid_next, before.uid_validity
                )),
                "{response}"
            );
            assert!(
                response.contains(&format!(
                    "* OK [UIDVALIDITY {}] UIDs valid",
                    before.uid_validity
                )),
                "{response}"
            );
            let (old_mailbox, new_mailbox, subscriptions, after) = kernel
                .read(&auth, |loom| {
                    Ok::<_, LoomError>((
                        mail::get_mailbox(loom, ns, "root", "Archive")?,
                        mail::get_mailbox(loom, ns, "root", "Renamed")?,
                        mail::list_imap_subscriptions(loom, ns, "root")?,
                        mail::get_imap_uid_state(loom, ns, "root", "Renamed")?,
                    ))
                })
                .unwrap();
            assert!(old_mailbox.is_none());
            assert!(new_mailbox.is_some());
            assert_eq!(subscriptions, vec!["Renamed"]);
            assert_eq!(after, Some(before));
            let _ = std::fs::remove_file(path);
        });
    }

    #[test]
    fn imap_authenticate_plain_fetches_mailbox() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let path = temp_path("imap-auth-plain");
            let ns = init(&path, None);
            let kernel = HostedKernel::new(&path);
            let auth = HostedAuth::passphrase(nid(1), "root-pass", "imap-auth-setup");
            kernel
                .pim()
                .mail_create_mailbox(
                    &auth,
                    ns,
                    "root",
                    "inbox",
                    &mail::MailboxMeta {
                        display_name: "Inbox".to_string(),
                    },
                )
                .unwrap();
            kernel
                .pim()
                .mail_ingest_message(
                    &auth,
                    ns,
                    "root",
                    "inbox",
                    "1",
                    b"From: auth@example.com\r\nTo: root@example.com\r\nSubject: Auth Plain\r\n\r\nBody",
                )
                .unwrap();

            let state = MailImapState {
                kernel,
                workspace: "main".to_string(),
                auth_policy: HostedAuthPolicy::Passphrase,
            };
            let (mut client, server) = tokio::io::duplex(8192);
            let server = tokio::spawn(async move { handle_imap_connection(server, state).await });
            let mut buf = vec![0u8; 8192];
            let read = client.read(&mut buf).await.unwrap();
            let greeting = String::from_utf8_lossy(&buf[..read]);
            assert!(greeting.contains("Loom IMAP ready"), "{greeting}");

            client
                .write_all(b"a1 AUTHENTICATE PLAIN AHJvb3QAcm9vdC1wYXNz\r\n")
                .await
                .unwrap();
            client.write_all(b"a2 SELECT INBOX\r\n").await.unwrap();
            client
                .write_all(b"a3 FETCH 1:* (FLAGS UID RFC822.SIZE BODY[])\r\n")
                .await
                .unwrap();
            client.write_all(b"a4 LOGOUT\r\n").await.unwrap();
            client.shutdown().await.unwrap();

            let mut out = Vec::new();
            client.read_to_end(&mut out).await.unwrap();
            let response = String::from_utf8_lossy(&out);
            assert!(response.contains("a1 OK AUTHENTICATE completed"), "{response}");
            assert!(response.contains("Subject: Auth Plain"), "{response}");
            assert!(response.contains("a4 OK LOGOUT completed"), "{response}");
            server.await.unwrap().unwrap();
            let _ = std::fs::remove_file(path);
        });
    }

    #[test]
    fn imap_authenticate_plain_continuation_fetches_mailbox() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let path = temp_path("imap-auth-plain-continuation");
            let ns = init(&path, None);
            let kernel = HostedKernel::new(&path);
            let auth = HostedAuth::passphrase(nid(1), "root-pass", "imap-auth-continuation-setup");
            kernel
                .pim()
                .mail_create_mailbox(
                    &auth,
                    ns,
                    "root",
                    "inbox",
                    &mail::MailboxMeta {
                        display_name: "Inbox".to_string(),
                    },
                )
                .unwrap();
            kernel
                .pim()
                .mail_ingest_message(
                    &auth,
                    ns,
                    "root",
                    "inbox",
                    "1",
                    b"From: cont@example.com\r\nTo: root@example.com\r\nSubject: Auth Continuation\r\n\r\nBody",
                )
                .unwrap();

            let state = MailImapState {
                kernel,
                workspace: "main".to_string(),
                auth_policy: HostedAuthPolicy::Passphrase,
            };
            let (mut client, server) = tokio::io::duplex(8192);
            let server = tokio::spawn(async move { handle_imap_connection(server, state).await });
            let mut buf = vec![0u8; 8192];
            let read = client.read(&mut buf).await.unwrap();
            let greeting = String::from_utf8_lossy(&buf[..read]);
            assert!(greeting.contains("Loom IMAP ready"), "{greeting}");

            client.write_all(b"a1 AUTHENTICATE PLAIN\r\n").await.unwrap();
            client.write_all(b"AHJvb3QAcm9vdC1wYXNz\r\n").await.unwrap();
            client.write_all(b"a2 SELECT INBOX\r\n").await.unwrap();
            client
                .write_all(b"a3 FETCH 1:* (FLAGS UID RFC822.SIZE BODY[])\r\n")
                .await
                .unwrap();
            client.write_all(b"a4 LOGOUT\r\n").await.unwrap();
            client.shutdown().await.unwrap();

            let mut out = Vec::new();
            client.read_to_end(&mut out).await.unwrap();
            let response = String::from_utf8_lossy(&out);
            assert!(response.contains("+ \r\n"), "{response}");
            assert!(response.contains("a1 OK AUTHENTICATE completed"), "{response}");
            assert!(response.contains("Subject: Auth Continuation"), "{response}");
            assert!(response.contains("a4 OK LOGOUT completed"), "{response}");
            server.await.unwrap().unwrap();
            let _ = std::fs::remove_file(path);
        });
    }
}
