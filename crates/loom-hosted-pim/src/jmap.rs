use std::future::Future;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::{DefaultBodyLimit, Path, Request, State};
use axum::http::header::CONTENT_TYPE;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use base64::Engine;
use loom_core::mail::{self, MailMessage, MailboxMeta};
use loom_core::{Code, Digest, FacetKind, LoomError, WorkspaceId, WsSelector};
use serde_json::{Value, json};
use tokio::net::TcpListener;

use loom_hosted_core::{HostedAuth, HostedAuthPolicy, HostedError, HostedHttpLimits, HostedKernel};

const JMAP_BLOB_MAX_SIZE: usize = 16 * 1024 * 1024;
const JMAP_BLOB_MAX_DATA_SOURCES: usize = 64;
const JMAP_QUOTA_CAPABILITY: &str = "urn:ietf:params:jmap:quota";
const JMAP_QUOTA_ACCOUNT_OCTETS_ID: &str = "mail-octets";

#[derive(Clone)]
struct MailJmapState {
    kernel: HostedKernel,
    workspace: String,
    auth_policy: HostedAuthPolicy,
}

struct JmapHttpError(Box<Response>);

type JmapHttpResult<T> = std::result::Result<T, JmapHttpError>;

impl JmapHttpError {
    fn into_response(self) -> Response {
        *self.0
    }
}

impl From<Response> for JmapHttpError {
    fn from(response: Response) -> Self {
        Self(Box::new(response))
    }
}

pub fn mail_jmap_router(kernel: HostedKernel, workspace: impl Into<String>) -> Router {
    mail_jmap_router_with_limit(kernel, workspace, 16 * 1024 * 1024)
}

pub fn mail_jmap_router_with_limit(
    kernel: HostedKernel,
    workspace: impl Into<String>,
    request_size_limit: usize,
) -> Router {
    mail_jmap_router_with_policy(
        kernel,
        workspace,
        request_size_limit,
        HostedAuthPolicy::OwnerOrPassphrase,
    )
}

pub fn mail_jmap_router_with_policy(
    kernel: HostedKernel,
    workspace: impl Into<String>,
    request_size_limit: usize,
    auth_policy: HostedAuthPolicy,
) -> Router {
    let state = MailJmapState {
        kernel,
        workspace: workspace.into(),
        auth_policy,
    };
    Router::new()
        .route("/.well-known/jmap", get(jmap_well_known))
        .route("/jmap/session", get(jmap_session))
        .route("/jmap/upload/{account_id}", post(jmap_upload))
        .route(
            "/jmap/download/{account_id}/{blob_id}/{name}",
            get(jmap_download),
        )
        .route("/jmap", post(jmap_api))
        .route("/jmap/api", post(jmap_api))
        .layer(DefaultBodyLimit::max(request_size_limit))
        .with_state(state)
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
    loom_hosted_core::http::serve_router(
        listener,
        mail_jmap_router_with_policy(kernel, workspace, limits.request_size_limit, auth_policy),
        limits,
        shutdown,
    )
    .await
}

async fn jmap_well_known() -> Response {
    (
        StatusCode::MOVED_PERMANENTLY,
        [("location", "/jmap/session")],
        Body::empty(),
    )
        .into_response()
}

async fn jmap_session(State(state): State<MailJmapState>, headers: HeaderMap) -> Response {
    let auth = match hosted_auth(&headers, state.auth_policy) {
        Ok(auth) => auth,
        Err(err) => return err.into_response(),
    };
    let account_name = match hosted_principal_name(&state.kernel, &auth, &headers) {
        Ok(principal) => principal,
        Err(err) => return err.into_response(),
    };
    let account_state = match state.kernel.read(&auth, |loom| {
        let ns = resolve_mail_workspace(loom, &state.workspace)?;
        jmap_account_state(loom, ns, &account_name)
    }) {
        Ok(state) => state,
        Err(err) => return loom_error_response(StatusCode::FORBIDDEN, err),
    };
    jmap_json(json!({
        "capabilities": jmap_capabilities(),
        "accounts": {
            "mail": {
                "name": account_name,
                "isPersonal": true,
                "isReadOnly": false,
                "accountCapabilities": {
                    "urn:ietf:params:jmap:mail": {
                        "maxMailboxesPerEmail": null,
                        "maxMailboxDepth": 1,
                        "maxSizeMailboxName": 255,
                        "mayCreateTopLevelMailbox": true
                    },
                    "urn:ietf:params:jmap:blob": {
                        "maxSizeBlobSet": JMAP_BLOB_MAX_SIZE,
                        "maxDataSources": JMAP_BLOB_MAX_DATA_SOURCES,
                        "supportedTypeNames": ["Mailbox", "Thread", "Email"],
                        "supportedDigestAlgorithms": []
                    },
                    JMAP_QUOTA_CAPABILITY: {
                        "maxObjectsInGet": 1
                    }
                }
            }
        },
        "primaryAccounts": {
            "urn:ietf:params:jmap:mail": "mail",
            "urn:ietf:params:jmap:blob": "mail",
            JMAP_QUOTA_CAPABILITY: "mail"
        },
        "username": account_name,
        "apiUrl": "/jmap/api",
        "downloadUrl": "/jmap/download/{accountId}/{blobId}/{name}?accept={type}",
        "uploadUrl": "/jmap/upload/{accountId}",
        "eventSourceUrl": null,
        "state": account_state
    }))
}

async fn jmap_api(
    State(state): State<MailJmapState>,
    headers: HeaderMap,
    req: Request,
) -> Response {
    let auth = match hosted_auth(&headers, state.auth_policy) {
        Ok(auth) => auth,
        Err(err) => return err.into_response(),
    };
    let principal = match hosted_principal_name(&state.kernel, &auth, &headers) {
        Ok(principal) => principal,
        Err(err) => return err.into_response(),
    };
    let body = match to_bytes(req.into_body(), 16 * 1024 * 1024).await {
        Ok(body) => body,
        Err(_) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                Code::InvalidArgument,
                "invalid JMAP request body",
            );
        }
    };
    let value = match serde_json::from_slice::<Value>(&body) {
        Ok(value) => value,
        Err(err) => {
            return jmap_request_error(
                StatusCode::BAD_REQUEST,
                "invalidArguments",
                &format!("invalid JMAP JSON: {err}"),
            );
        }
    };
    if let Err(err) = validate_jmap_using(&value) {
        return jmap_json_status(StatusCode::BAD_REQUEST, err);
    }
    let Some(calls) = value.get("methodCalls").and_then(Value::as_array) else {
        return jmap_request_error(
            StatusCode::BAD_REQUEST,
            "invalidArguments",
            "JMAP request missing methodCalls",
        );
    };
    let mut responses = Vec::new();
    for call in calls {
        let Some(items) = call.as_array() else {
            return jmap_request_error(
                StatusCode::BAD_REQUEST,
                "invalidArguments",
                "JMAP methodCall must be an array",
            );
        };
        if items.len() != 3 {
            return jmap_request_error(
                StatusCode::BAD_REQUEST,
                "invalidArguments",
                "JMAP methodCall must have three items",
            );
        }
        let Some(name) = items[0].as_str() else {
            return jmap_request_error(
                StatusCode::BAD_REQUEST,
                "invalidArguments",
                "JMAP method name must be a string",
            );
        };
        if !items[1].is_object() {
            return jmap_request_error(
                StatusCode::BAD_REQUEST,
                "invalidArguments",
                "JMAP method arguments must be an object",
            );
        }
        let Some(call_id) = items[2].as_str() else {
            return jmap_request_error(
                StatusCode::BAD_REQUEST,
                "invalidArguments",
                "JMAP method call id must be a string",
            );
        };
        let args = match resolve_result_references(&items[1], &responses) {
            Ok(args) => args,
            Err(err) => {
                responses.push(json!(["error", err, call_id]));
                continue;
            }
        };
        let response = match dispatch_jmap(&state, &auth, &principal, name, &args) {
            Ok(value) => json!([name, value, call_id]),
            Err(err) => json!(["error", err, call_id]),
        };
        responses.push(response);
    }
    let session_state = state
        .kernel
        .read(&auth, |loom| {
            let ns = resolve_mail_workspace(loom, &state.workspace)?;
            jmap_account_state(loom, ns, &principal)
        })
        .unwrap_or_else(|_| "0".to_string());
    jmap_json(json!({
        "methodResponses": responses,
        "sessionState": session_state
    }))
}

fn dispatch_jmap(
    state: &MailJmapState,
    auth: &HostedAuth,
    principal: &str,
    name: &str,
    args: &Value,
) -> std::result::Result<Value, Value> {
    match name {
        "Core/echo" => Ok(args.clone()),
        "Mailbox/get" => jmap_mailbox_get(state, auth, principal, args),
        "Mailbox/changes" => jmap_mailbox_changes(state, auth, principal, args),
        "Mailbox/query" => jmap_mailbox_query(state, auth, principal, args),
        "Mailbox/queryChanges" => jmap_mailbox_query_changes(state, auth, principal, args),
        "Mailbox/set" => jmap_mailbox_set(state, auth, principal, args),
        "Thread/get" => jmap_thread_get(state, auth, principal, args),
        "Thread/changes" => jmap_email_changes(state, auth, principal, args),
        "Identity/get" => jmap_identity_get(principal, args),
        "Identity/changes" => jmap_identity_changes(args),
        "Identity/set" => jmap_identity_set(args),
        "Email/query" => jmap_email_query(state, auth, principal, args),
        "Email/get" => jmap_email_get(state, auth, principal, args),
        "Email/set" => jmap_email_set(state, auth, principal, args),
        "Email/copy" => jmap_email_copy(state, auth, principal, args),
        "Email/import" => jmap_email_import(state, auth, principal, args),
        "Email/parse" => jmap_email_parse(state, auth, principal, args),
        "Email/changes" => jmap_email_changes(state, auth, principal, args),
        "Email/queryChanges" => jmap_email_query_changes(state, auth, principal, args),
        "Blob/upload" => jmap_blob_upload(state, auth, principal, args),
        "Blob/get" => jmap_blob_get(state, auth, principal, args),
        "Blob/lookup" => jmap_blob_lookup(state, auth, principal, args),
        "Quota/get" => jmap_quota_get(state, auth, principal, args),
        "SearchSnippet/get" => jmap_search_snippet_get(args),
        _ => Err(jmap_error(
            "unknownMethod",
            format!("unsupported JMAP method {name}"),
        )),
    }
}

fn validate_jmap_using(value: &Value) -> std::result::Result<(), Value> {
    let Some(using) = value.get("using").and_then(Value::as_array) else {
        return Err(jmap_error("invalidArguments", "JMAP request missing using"));
    };
    for capability in using {
        let Some(capability) = capability.as_str() else {
            return Err(jmap_error(
                "invalidArguments",
                "JMAP using entries must be strings",
            ));
        };
        if !matches!(
            capability,
            "urn:ietf:params:jmap:core"
                | "urn:ietf:params:jmap:mail"
                | "urn:ietf:params:jmap:blob"
                | JMAP_QUOTA_CAPABILITY
        ) {
            return Err(jmap_error(
                "unknownCapability",
                format!("unsupported JMAP capability {capability}"),
            ));
        }
    }
    Ok(())
}

fn resolve_result_references(
    args: &Value,
    responses: &[Value],
) -> std::result::Result<Value, Value> {
    let mut resolved = args.clone();
    let Some(object) = args.as_object() else {
        return Err(jmap_error(
            "invalidArguments",
            "JMAP method arguments must be an object",
        ));
    };
    for (key, reference) in object {
        let Some(target_key) = key.strip_prefix('#') else {
            continue;
        };
        let value = resolve_result_reference(reference, responses)?;
        let Some(target) = resolved.as_object_mut() else {
            return Err(jmap_error(
                "invalidArguments",
                "JMAP method arguments must be an object",
            ));
        };
        target.remove(key);
        target.insert(target_key.to_string(), value);
    }
    Ok(resolved)
}

fn resolve_result_reference(
    reference: &Value,
    responses: &[Value],
) -> std::result::Result<Value, Value> {
    let Some(result_of) = reference.get("resultOf").and_then(Value::as_str) else {
        return Err(jmap_error(
            "invalidResultReference",
            "result reference missing resultOf",
        ));
    };
    let Some(name) = reference.get("name").and_then(Value::as_str) else {
        return Err(jmap_error(
            "invalidResultReference",
            "result reference missing name",
        ));
    };
    let Some(path) = reference.get("path").and_then(Value::as_str) else {
        return Err(jmap_error(
            "invalidResultReference",
            "result reference missing path",
        ));
    };
    let Some(response) = responses.iter().find(|response| {
        response.get(0).and_then(Value::as_str) == Some(name)
            && response.get(2).and_then(Value::as_str) == Some(result_of)
    }) else {
        return Err(jmap_error(
            "invalidResultReference",
            "referenced method response was not found",
        ));
    };
    response
        .get(1)
        .and_then(|arguments| arguments.pointer(path))
        .cloned()
        .ok_or_else(|| jmap_error("invalidResultReference", "referenced path was not found"))
}

async fn jmap_upload(
    State(state): State<MailJmapState>,
    Path(account_id): Path<String>,
    headers: HeaderMap,
    req: Request,
) -> Response {
    if account_id != "mail" {
        return error_response(
            StatusCode::NOT_FOUND,
            Code::NotFound,
            "JMAP account not found",
        );
    }
    let auth = match hosted_auth(&headers, state.auth_policy) {
        Ok(auth) => auth,
        Err(err) => return err.into_response(),
    };
    let principal = match hosted_principal_name(&state.kernel, &auth, &headers) {
        Ok(principal) => principal,
        Err(err) => return err.into_response(),
    };
    let body = match to_bytes(req.into_body(), 16 * 1024 * 1024).await {
        Ok(body) => body,
        Err(_) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                Code::InvalidArgument,
                "invalid JMAP upload body",
            );
        }
    };
    match state.kernel.write(&auth, |loom| {
        let ns = resolve_mail_workspace(loom, &state.workspace)?;
        mail::put_blob(loom, ns, &principal, &body)
    }) {
        Ok(digest) => jmap_json(json!({
            "accountId": "mail",
            "blobId": digest.to_hex(),
            "type": content_type(&headers),
            "size": body.len()
        })),
        Err(err) => loom_error_response(StatusCode::FORBIDDEN, err),
    }
}

async fn jmap_download(
    State(state): State<MailJmapState>,
    Path((account_id, blob_id, _name)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> Response {
    if account_id != "mail" {
        return error_response(
            StatusCode::NOT_FOUND,
            Code::NotFound,
            "JMAP account not found",
        );
    }
    let auth = match hosted_auth(&headers, state.auth_policy) {
        Ok(auth) => auth,
        Err(err) => return err.into_response(),
    };
    let principal = match hosted_principal_name(&state.kernel, &auth, &headers) {
        Ok(principal) => principal,
        Err(err) => return err.into_response(),
    };
    match state.kernel.read(&auth, |loom| {
        let ns = resolve_mail_workspace(loom, &state.workspace)?;
        let digest = jmap_blob_digest(loom, &blob_id)?;
        mail::get_blob(loom, ns, &principal, &digest)
    }) {
        Ok(Some(bytes)) => (
            StatusCode::OK,
            [(CONTENT_TYPE, "message/rfc822")],
            Body::from(bytes),
        )
            .into_response(),
        Ok(None) => error_response(StatusCode::NOT_FOUND, Code::NotFound, "JMAP blob not found"),
        Err(err) => loom_error_response(StatusCode::BAD_REQUEST, err),
    }
}

fn jmap_mailbox_get(
    state: &MailJmapState,
    auth: &HostedAuth,
    principal: &str,
    args: &Value,
) -> std::result::Result<Value, Value> {
    let ids = optional_string_array(args, "ids")?;
    let (mailboxes, state_token) = state
        .kernel
        .read(auth, |loom| {
            let ns = resolve_mail_workspace(loom, &state.workspace)?;
            let names = mail::list_mailboxes(loom, ns, principal)?;
            let mut out = Vec::new();
            for name in names {
                if ids
                    .as_ref()
                    .is_none_or(|ids| ids.iter().any(|id| id == &name))
                {
                    let messages = mail::list_messages(loom, ns, principal, &name)?;
                    let unread = messages
                        .iter()
                        .filter(|message| {
                            mail::get_flags(loom, ns, principal, &name, &message.uid)
                                .map(|flags| !flags.iter().any(|flag| flag_eq(flag, "Seen")))
                                .unwrap_or(false)
                        })
                        .count();
                    out.push(jmap_mailbox_json(&name, messages.len(), unread));
                }
            }
            let state_token = jmap_account_state(loom, ns, principal)?;
            Ok((out, state_token))
        })
        .map_err(jmap_loom_error)?;
    let not_found = ids
        .unwrap_or_default()
        .into_iter()
        .filter(|id| {
            !mailboxes
                .iter()
                .any(|mailbox| mailbox.get("id") == Some(&json!(id)))
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "accountId": "mail",
        "state": state_token,
        "list": mailboxes,
        "notFound": not_found
    }))
}

fn jmap_mailbox_changes(
    state: &MailJmapState,
    auth: &HostedAuth,
    principal: &str,
    args: &Value,
) -> std::result::Result<Value, Value> {
    let Some(since_state) = args.get("sinceState").and_then(Value::as_str) else {
        return Err(jmap_error("invalidArguments", "sinceState is required"));
    };
    let (current_state, ids) = state
        .kernel
        .read(auth, |loom| {
            let ns = resolve_mail_workspace(loom, &state.workspace)?;
            Ok((
                jmap_account_state(loom, ns, principal)?,
                mail::list_mailboxes(loom, ns, principal)?,
            ))
        })
        .map_err(jmap_loom_error)?;
    Ok(json!({
        "accountId": "mail",
        "oldState": since_state,
        "newState": current_state,
        "hasMoreChanges": false,
        "created": if since_state == current_state { Vec::<String>::new() } else { ids },
        "updated": [],
        "destroyed": []
    }))
}

fn jmap_mailbox_query(
    state: &MailJmapState,
    auth: &HostedAuth,
    principal: &str,
    _args: &Value,
) -> std::result::Result<Value, Value> {
    let (ids, state_token) = state
        .kernel
        .read(auth, |loom| {
            let ns = resolve_mail_workspace(loom, &state.workspace)?;
            Ok((
                mail::list_mailboxes(loom, ns, principal)?,
                jmap_account_state(loom, ns, principal)?,
            ))
        })
        .map_err(jmap_loom_error)?;
    Ok(json!({
        "accountId": "mail",
        "queryState": state_token,
        "canCalculateChanges": false,
        "position": 0,
        "ids": ids,
        "total": ids.len()
    }))
}

fn jmap_mailbox_query_changes(
    state: &MailJmapState,
    auth: &HostedAuth,
    principal: &str,
    args: &Value,
) -> std::result::Result<Value, Value> {
    let Some(since_state) = args.get("sinceQueryState").and_then(Value::as_str) else {
        return Err(jmap_error(
            "invalidArguments",
            "sinceQueryState is required",
        ));
    };
    let (current_state, ids) = state
        .kernel
        .read(auth, |loom| {
            let ns = resolve_mail_workspace(loom, &state.workspace)?;
            Ok((
                jmap_account_state(loom, ns, principal)?,
                mail::list_mailboxes(loom, ns, principal)?,
            ))
        })
        .map_err(jmap_loom_error)?;
    let added = if since_state == current_state {
        Vec::new()
    } else {
        ids.into_iter()
            .enumerate()
            .map(|(index, id)| json!({ "id": id, "index": index }))
            .collect::<Vec<_>>()
    };
    Ok(json!({
        "accountId": "mail",
        "oldQueryState": since_state,
        "newQueryState": current_state,
        "hasMoreChanges": false,
        "removed": [],
        "added": added
    }))
}

fn jmap_mailbox_set(
    state: &MailJmapState,
    auth: &HostedAuth,
    principal: &str,
    args: &Value,
) -> std::result::Result<Value, Value> {
    let mut created = serde_json::Map::new();
    let mut updated = Vec::new();
    let mut destroyed = Vec::new();
    let mut not_created = serde_json::Map::new();
    let mut not_updated = serde_json::Map::new();
    let mut not_destroyed = serde_json::Map::new();
    let (old_state, new_state) = state
        .kernel
        .write(auth, |loom| {
            let ns = resolve_mail_workspace(loom, &state.workspace)?;
            let old_state = jmap_account_state(loom, ns, principal)?;
            if let Some(create) = args.get("create").and_then(Value::as_object) {
                for (creation_id, value) in create {
                    let Some(name) = value.get("name").and_then(Value::as_str) else {
                        not_created.insert(
                            creation_id.clone(),
                            jmap_error("invalidProperties", "mailbox name is required"),
                        );
                        continue;
                    };
                    match mail::create_mailbox(
                        loom,
                        ns,
                        principal,
                        name,
                        &MailboxMeta {
                            display_name: name.to_string(),
                        },
                    ) {
                        Ok(()) => {
                            created.insert(creation_id.clone(), jmap_mailbox_json(name, 0, 0));
                        }
                        Err(err) => {
                            not_created.insert(creation_id.clone(), jmap_loom_error(err));
                        }
                    }
                }
            }
            if let Some(update) = args.get("update").and_then(Value::as_object) {
                for (id, value) in update {
                    let name = value.get("name").and_then(Value::as_str).unwrap_or(id);
                    match mail::create_mailbox(
                        loom,
                        ns,
                        principal,
                        id,
                        &MailboxMeta {
                            display_name: name.to_string(),
                        },
                    ) {
                        Ok(()) => updated.push(id.clone()),
                        Err(err) => {
                            not_updated.insert(id.clone(), jmap_loom_error(err));
                        }
                    }
                }
            }
            if let Some(destroy) = args.get("destroy").and_then(Value::as_array) {
                for id in destroy {
                    let Some(id) = id.as_str() else {
                        continue;
                    };
                    match mail::delete_mailbox(loom, ns, principal, id) {
                        Ok(true) => destroyed.push(id.to_string()),
                        Ok(false) => {
                            not_destroyed.insert(
                                id.to_string(),
                                jmap_error("notFound", "mailbox not found"),
                            );
                        }
                        Err(err) => {
                            not_destroyed.insert(id.to_string(), jmap_loom_error(err));
                        }
                    }
                }
            }
            let new_state = jmap_account_state(loom, ns, principal)?;
            Ok((old_state, new_state))
        })
        .map_err(jmap_loom_error)?;
    Ok(json!({
        "accountId": "mail",
        "oldState": old_state,
        "newState": new_state,
        "created": created,
        "updated": updated,
        "destroyed": destroyed,
        "notCreated": not_created,
        "notUpdated": not_updated,
        "notDestroyed": not_destroyed
    }))
}

fn jmap_identity_get(principal: &str, args: &Value) -> std::result::Result<Value, Value> {
    let ids = optional_string_array(args, "ids")?;
    let identity = json!({
        "id": "default",
        "name": principal,
        "email": principal,
        "replyTo": null,
        "bcc": null,
        "textSignature": "",
        "htmlSignature": "",
        "mayDelete": false
    });
    let include_default = ids
        .as_ref()
        .is_none_or(|ids| ids.iter().any(|id| id == "default"));
    let list = if include_default {
        vec![identity]
    } else {
        Vec::new()
    };
    let not_found = ids
        .unwrap_or_default()
        .into_iter()
        .filter(|id| id != "default")
        .collect::<Vec<_>>();
    Ok(json!({
        "accountId": "mail",
        "state": "0",
        "list": list,
        "notFound": not_found
    }))
}

fn jmap_identity_changes(args: &Value) -> std::result::Result<Value, Value> {
    let Some(since_state) = args.get("sinceState").and_then(Value::as_str) else {
        return Err(jmap_error("invalidArguments", "sinceState is required"));
    };
    Ok(json!({
        "accountId": "mail",
        "oldState": since_state,
        "newState": "0",
        "hasMoreChanges": false,
        "created": if since_state == "0" { Vec::<String>::new() } else { vec!["default".to_string()] },
        "updated": [],
        "destroyed": []
    }))
}

fn jmap_quota_get(
    state: &MailJmapState,
    auth: &HostedAuth,
    principal: &str,
    args: &Value,
) -> std::result::Result<Value, Value> {
    require_mail_account(args)?;
    let ids = optional_string_array(args, "ids")?
        .unwrap_or_else(|| vec![JMAP_QUOTA_ACCOUNT_OCTETS_ID.to_string()]);
    let usage = state
        .kernel
        .read(auth, |loom| {
            let ns = resolve_mail_workspace(loom, &state.workspace)?;
            mail::account_usage(loom, ns, principal)
        })
        .map_err(jmap_loom_error)?;
    let quota = jmap_quota_object(&usage);
    let list = ids
        .iter()
        .filter(|id| id.as_str() == JMAP_QUOTA_ACCOUNT_OCTETS_ID)
        .map(|_| quota.clone())
        .collect::<Vec<_>>();
    let not_found = ids
        .into_iter()
        .filter(|id| id.as_str() != JMAP_QUOTA_ACCOUNT_OCTETS_ID)
        .collect::<Vec<_>>();
    Ok(json!({
        "accountId": "mail",
        "state": jmap_account_quota_state(&usage),
        "list": list,
        "notFound": not_found
    }))
}

fn jmap_quota_object(usage: &mail::MailAccountUsage) -> Value {
    json!({
        "id": JMAP_QUOTA_ACCOUNT_OCTETS_ID,
        "resourceType": "octets",
        "used": usage.used_octets,
        "hardLimit": usage.hard_limit_octets,
        "scope": "account",
        "types": ["Mail"]
    })
}

fn jmap_account_quota_state(usage: &mail::MailAccountUsage) -> String {
    let mut bytes = b"loom-jmap-quota-state-v1\0".to_vec();
    bytes.extend_from_slice(&usage.used_octets.to_be_bytes());
    match usage.hard_limit_octets {
        Some(limit) => {
            bytes.push(1);
            bytes.extend_from_slice(&limit.to_be_bytes());
        }
        None => bytes.push(0),
    }
    Digest::blake3(&bytes).to_hex()
}

fn jmap_identity_set(args: &Value) -> std::result::Result<Value, Value> {
    let mut not_created = serde_json::Map::new();
    let mut not_updated = serde_json::Map::new();
    let mut not_destroyed = serde_json::Map::new();
    if let Some(create) = args.get("create").and_then(Value::as_object) {
        for creation_id in create.keys() {
            not_created.insert(
                creation_id.clone(),
                jmap_error("forbidden", "identity creation is not supported"),
            );
        }
    }
    if let Some(update) = args.get("update").and_then(Value::as_object) {
        for id in update.keys() {
            not_updated.insert(
                id.clone(),
                jmap_error("forbidden", "identity updates are not supported"),
            );
        }
    }
    if let Some(destroy) = args.get("destroy").and_then(Value::as_array) {
        for id in destroy {
            if let Some(id) = id.as_str() {
                not_destroyed.insert(
                    id.to_string(),
                    jmap_error("forbidden", "identity deletion is not supported"),
                );
            }
        }
    }
    Ok(json!({
        "accountId": "mail",
        "oldState": "0",
        "newState": "0",
        "created": {},
        "updated": [],
        "destroyed": [],
        "notCreated": not_created,
        "notUpdated": not_updated,
        "notDestroyed": not_destroyed
    }))
}

fn jmap_thread_get(
    state: &MailJmapState,
    auth: &HostedAuth,
    principal: &str,
    args: &Value,
) -> std::result::Result<Value, Value> {
    let ids = optional_string_array(args, "ids")?;
    let (available, state_token) = state
        .kernel
        .read(auth, |loom| {
            let ns = resolve_mail_workspace(loom, &state.workspace)?;
            Ok((
                jmap_all_message_ids(loom, ns, principal)?,
                jmap_account_state(loom, ns, principal)?,
            ))
        })
        .map_err(jmap_loom_error)?;
    let requested = ids.clone().unwrap_or_else(|| available.clone());
    let mut list = Vec::new();
    let mut not_found = Vec::new();
    for id in requested {
        if available.iter().any(|available_id| available_id == &id) {
            list.push(json!({
                "id": id,
                "emailIds": [id]
            }));
        } else {
            not_found.push(id);
        }
    }
    Ok(json!({
        "accountId": "mail",
        "state": state_token,
        "list": list,
        "notFound": not_found
    }))
}

fn jmap_email_query(
    state: &MailJmapState,
    auth: &HostedAuth,
    principal: &str,
    args: &Value,
) -> std::result::Result<Value, Value> {
    let filter = args.get("filter").unwrap_or(&Value::Null);
    let in_mailbox = filter.get("inMailbox").and_then(Value::as_str);
    let text = filter.get("text").and_then(Value::as_str);
    let from = filter.get("from").and_then(Value::as_str);
    let subject = filter.get("subject").and_then(Value::as_str);
    let (ids, state_token) = state
        .kernel
        .read(auth, |loom| {
            let ns = resolve_mail_workspace(loom, &state.workspace)?;
            let mailboxes = match in_mailbox {
                Some(mailbox) => vec![mailbox.to_string()],
                None => mail::list_mailboxes(loom, ns, principal)?,
            };
            let mut ids = Vec::new();
            for mailbox in mailboxes {
                for message in mail::list_messages(loom, ns, principal, &mailbox)? {
                    if email_matches(&message, text, from, subject) {
                        ids.push(jmap_message_id(&mailbox, &message.uid));
                    }
                }
            }
            let state_token = jmap_account_state(loom, ns, principal)?;
            Ok((ids, state_token))
        })
        .map_err(jmap_loom_error)?;
    Ok(json!({
        "accountId": "mail",
        "queryState": state_token,
        "canCalculateChanges": false,
        "position": 0,
        "ids": ids,
        "total": ids.len()
    }))
}

fn jmap_email_get(
    state: &MailJmapState,
    auth: &HostedAuth,
    principal: &str,
    args: &Value,
) -> std::result::Result<Value, Value> {
    let ids = optional_string_array(args, "ids")?;
    let (list, not_found, state_token) = state
        .kernel
        .read(auth, |loom| {
            let ns = resolve_mail_workspace(loom, &state.workspace)?;
            let requested = match &ids {
                Some(ids) => ids.clone(),
                None => mail::list_mailboxes(loom, ns, principal)?
                    .into_iter()
                    .flat_map(|mailbox| {
                        mail::list_messages(loom, ns, principal, &mailbox)
                            .unwrap_or_default()
                            .into_iter()
                            .map(move |message| jmap_message_id(&mailbox, &message.uid))
                    })
                    .collect(),
            };
            let mut list = Vec::new();
            let mut not_found = Vec::new();
            for id in requested {
                let Some((mailbox, uid)) = parse_jmap_message_id(&id) else {
                    not_found.push(id);
                    continue;
                };
                match mail::get_message(loom, ns, principal, &mailbox, &uid)? {
                    Some(message) => {
                        let flags = mail::get_flags(loom, ns, principal, &mailbox, &uid)?;
                        list.push(jmap_email_json(&mailbox, message, flags));
                    }
                    None => not_found.push(id),
                }
            }
            let state_token = jmap_account_state(loom, ns, principal)?;
            Ok((list, not_found, state_token))
        })
        .map_err(jmap_loom_error)?;
    Ok(json!({
        "accountId": "mail",
        "state": state_token,
        "list": list,
        "notFound": not_found
    }))
}

fn jmap_email_changes(
    state: &MailJmapState,
    auth: &HostedAuth,
    principal: &str,
    args: &Value,
) -> std::result::Result<Value, Value> {
    let Some(since_state) = args.get("sinceState").and_then(Value::as_str) else {
        return Err(jmap_error("invalidArguments", "sinceState is required"));
    };
    let (current_state, ids) = state
        .kernel
        .read(auth, |loom| {
            let ns = resolve_mail_workspace(loom, &state.workspace)?;
            Ok((
                jmap_account_state(loom, ns, principal)?,
                jmap_all_message_ids(loom, ns, principal)?,
            ))
        })
        .map_err(jmap_loom_error)?;
    let created = if since_state == current_state {
        Vec::new()
    } else {
        ids
    };
    Ok(json!({
        "accountId": "mail",
        "oldState": since_state,
        "newState": current_state,
        "hasMoreChanges": false,
        "created": created,
        "updated": [],
        "destroyed": []
    }))
}

fn jmap_email_query_changes(
    state: &MailJmapState,
    auth: &HostedAuth,
    principal: &str,
    args: &Value,
) -> std::result::Result<Value, Value> {
    let Some(since_state) = args.get("sinceQueryState").and_then(Value::as_str) else {
        return Err(jmap_error(
            "invalidArguments",
            "sinceQueryState is required",
        ));
    };
    let filter = args.get("filter").unwrap_or(&Value::Null);
    let in_mailbox = filter.get("inMailbox").and_then(Value::as_str);
    let text = filter.get("text").and_then(Value::as_str);
    let from = filter.get("from").and_then(Value::as_str);
    let subject = filter.get("subject").and_then(Value::as_str);
    let (current_state, ids) = state
        .kernel
        .read(auth, |loom| {
            let ns = resolve_mail_workspace(loom, &state.workspace)?;
            Ok((
                jmap_account_state(loom, ns, principal)?,
                jmap_query_message_ids(loom, ns, principal, in_mailbox, text, from, subject)?,
            ))
        })
        .map_err(jmap_loom_error)?;
    let added = if since_state == current_state {
        Vec::new()
    } else {
        ids.into_iter()
            .enumerate()
            .map(|(index, id)| json!({ "id": id, "index": index }))
            .collect::<Vec<_>>()
    };
    Ok(json!({
        "accountId": "mail",
        "oldQueryState": since_state,
        "newQueryState": current_state,
        "hasMoreChanges": false,
        "removed": [],
        "added": added
    }))
}

fn jmap_email_set(
    state: &MailJmapState,
    auth: &HostedAuth,
    principal: &str,
    args: &Value,
) -> std::result::Result<Value, Value> {
    let mut created = serde_json::Map::new();
    let mut not_created = serde_json::Map::new();
    let mut updated = Vec::new();
    let mut destroyed = Vec::new();
    let mut not_updated = serde_json::Map::new();
    let mut not_destroyed = serde_json::Map::new();
    let (old_state, new_state) = state
        .kernel
        .write(auth, |loom| {
            let ns = resolve_mail_workspace(loom, &state.workspace)?;
            let old_state = jmap_account_state(loom, ns, principal)?;
            if let Some(create) = args.get("create").and_then(Value::as_object) {
                for (creation_id, value) in create {
                    match create_email_from_blob(loom, ns, principal, creation_id, value) {
                        Ok(email) => {
                            created.insert(creation_id.clone(), email);
                        }
                        Err(err) => {
                            not_created.insert(creation_id.clone(), err);
                        }
                    }
                }
            }
            if let Some(update) = args.get("update").and_then(Value::as_object) {
                for (id, value) in update {
                    let Some((mailbox, uid)) = parse_jmap_message_id(id) else {
                        not_updated.insert(id.clone(), jmap_error("notFound", "email not found"));
                        continue;
                    };
                    let Some(keywords) = value.get("keywords").and_then(Value::as_object) else {
                        not_updated.insert(
                            id.clone(),
                            jmap_error("invalidProperties", "only keywords updates are supported"),
                        );
                        continue;
                    };
                    let flags = keywords
                        .iter()
                        .filter_map(|(name, enabled)| {
                            enabled.as_bool().unwrap_or(false).then_some(name)
                        })
                        .map(|name| core_flag_from_jmap(name).to_string())
                        .collect::<Vec<_>>();
                    match mail::set_flags(loom, ns, principal, &mailbox, &uid, &flags) {
                        Ok(()) => updated.push(id.clone()),
                        Err(err) => {
                            not_updated.insert(id.clone(), jmap_loom_error(err));
                        }
                    }
                }
            }
            if let Some(destroy) = args.get("destroy").and_then(Value::as_array) {
                for id in destroy {
                    let Some(id) = id.as_str() else {
                        continue;
                    };
                    let Some((mailbox, uid)) = parse_jmap_message_id(id) else {
                        not_destroyed
                            .insert(id.to_string(), jmap_error("notFound", "email not found"));
                        continue;
                    };
                    match mail::delete_message(loom, ns, principal, &mailbox, &uid) {
                        Ok(true) => destroyed.push(id.to_string()),
                        Ok(false) => {
                            not_destroyed
                                .insert(id.to_string(), jmap_error("notFound", "email not found"));
                        }
                        Err(err) => {
                            not_destroyed.insert(id.to_string(), jmap_loom_error(err));
                        }
                    }
                }
            }
            let new_state = jmap_account_state(loom, ns, principal)?;
            Ok((old_state, new_state))
        })
        .map_err(jmap_loom_error)?;
    Ok(json!({
        "accountId": "mail",
        "oldState": old_state,
        "newState": new_state,
        "created": created,
        "updated": updated,
        "destroyed": destroyed,
        "notCreated": not_created,
        "notUpdated": not_updated,
        "notDestroyed": not_destroyed
    }))
}

fn jmap_email_copy(
    state: &MailJmapState,
    auth: &HostedAuth,
    principal: &str,
    args: &Value,
) -> std::result::Result<Value, Value> {
    if args.get("fromAccountId").and_then(Value::as_str) != Some("mail")
        || args.get("accountId").and_then(Value::as_str) != Some("mail")
    {
        return Err(jmap_error("accountNotFound", "JMAP account not found"));
    }
    let mut created = serde_json::Map::new();
    let mut not_created = serde_json::Map::new();
    let (old_state, new_state) = state
        .kernel
        .write(auth, |loom| {
            let ns = resolve_mail_workspace(loom, &state.workspace)?;
            let old_state = jmap_account_state(loom, ns, principal)?;
            let Some(create) = args.get("create").and_then(Value::as_object) else {
                return Err(LoomError::invalid("Email/copy expects create"));
            };
            for (creation_id, value) in create {
                let Some(id) = value.get("id").and_then(Value::as_str) else {
                    not_created.insert(
                        creation_id.clone(),
                        jmap_error("invalidProperties", "id is required"),
                    );
                    continue;
                };
                let Some((source_mailbox, source_uid)) = parse_jmap_message_id(id) else {
                    not_created.insert(
                        creation_id.clone(),
                        jmap_error("notFound", "email not found"),
                    );
                    continue;
                };
                let mailbox = match value.get("mailboxIds") {
                    Some(_) => match email_create_mailbox(value) {
                        Ok(mailbox) => mailbox,
                        Err(err) => {
                            not_created.insert(creation_id.clone(), err);
                            continue;
                        }
                    },
                    None => source_mailbox.clone(),
                };
                let raw = match mail::to_eml(loom, ns, principal, &source_mailbox, &source_uid)? {
                    Some(raw) => raw,
                    None => {
                        not_created.insert(
                            creation_id.clone(),
                            jmap_error("notFound", "email not found"),
                        );
                        continue;
                    }
                };
                if mail::get_message(loom, ns, principal, &mailbox, creation_id)?.is_some() {
                    not_created.insert(
                        creation_id.clone(),
                        jmap_error("alreadyExists", "email creation id already exists"),
                    );
                    continue;
                }
                mail::ingest_message(loom, ns, principal, &mailbox, creation_id, &raw)?;
                let flags = email_create_flags(value);
                if !flags.is_empty() {
                    mail::set_flags(loom, ns, principal, &mailbox, creation_id, &flags)?;
                }
                let message = mail::get_message(loom, ns, principal, &mailbox, creation_id)?
                    .ok_or_else(|| LoomError::corrupt("mail: copied email missing"))?;
                created.insert(
                    creation_id.clone(),
                    jmap_email_json(&mailbox, message, flags),
                );
            }
            let new_state = jmap_account_state(loom, ns, principal)?;
            Ok((old_state, new_state))
        })
        .map_err(jmap_loom_error)?;
    Ok(json!({
        "accountId": "mail",
        "oldState": old_state,
        "newState": new_state,
        "created": created,
        "notCreated": not_created
    }))
}

fn jmap_email_import(
    state: &MailJmapState,
    auth: &HostedAuth,
    principal: &str,
    args: &Value,
) -> std::result::Result<Value, Value> {
    let mut created = serde_json::Map::new();
    let mut not_created = serde_json::Map::new();
    let (old_state, new_state) = state
        .kernel
        .write(auth, |loom| {
            let ns = resolve_mail_workspace(loom, &state.workspace)?;
            let old_state = jmap_account_state(loom, ns, principal)?;
            let Some(emails) = args.get("emails").and_then(Value::as_object) else {
                return Err(LoomError::invalid("Email/import expects emails"));
            };
            for (creation_id, value) in emails {
                match create_email_from_blob(loom, ns, principal, creation_id, value) {
                    Ok(email) => {
                        created.insert(creation_id.clone(), email);
                    }
                    Err(err) => {
                        not_created.insert(creation_id.clone(), err);
                    }
                }
            }
            let new_state = jmap_account_state(loom, ns, principal)?;
            Ok((old_state, new_state))
        })
        .map_err(jmap_loom_error)?;
    Ok(json!({
        "accountId": "mail",
        "oldState": old_state,
        "newState": new_state,
        "created": created,
        "notCreated": not_created
    }))
}

fn jmap_email_parse(
    state: &MailJmapState,
    auth: &HostedAuth,
    principal: &str,
    args: &Value,
) -> std::result::Result<Value, Value> {
    let blob_ids = optional_string_array(args, "blobIds")?
        .ok_or_else(|| jmap_error("invalidArguments", "blobIds is required"))?;
    let (list, not_parsable, not_found) = state
        .kernel
        .read(auth, |loom| {
            let ns = resolve_mail_workspace(loom, &state.workspace)?;
            let mut list = serde_json::Map::new();
            let mut not_parsable = Vec::new();
            let mut not_found = Vec::new();
            for blob_id in blob_ids {
                let digest = match jmap_blob_digest(loom, &blob_id) {
                    Ok(digest) => digest,
                    Err(_) => {
                        not_found.push(blob_id);
                        continue;
                    }
                };
                let Some(raw) = mail::get_blob(loom, ns, principal, &digest)? else {
                    not_found.push(blob_id);
                    continue;
                };
                match MailMessage::from_rfc5322("parsed", digest.to_string(), &raw) {
                    Ok(message) => {
                        list.insert(blob_id.clone(), jmap_parsed_email_json(message));
                    }
                    Err(_) => not_parsable.push(blob_id),
                }
            }
            Ok((list, not_parsable, not_found))
        })
        .map_err(jmap_loom_error)?;
    Ok(json!({
        "accountId": "mail",
        "parsed": list,
        "notParsable": not_parsable,
        "notFound": not_found
    }))
}

fn jmap_search_snippet_get(args: &Value) -> std::result::Result<Value, Value> {
    let ids = optional_string_array(args, "emailIds")?
        .or_else(|| optional_string_array(args, "ids").ok().flatten())
        .unwrap_or_default();
    let list = ids
        .into_iter()
        .map(|email_id| {
            json!({
                "emailId": email_id,
                "subject": null,
                "preview": null
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "accountId": "mail",
        "list": list,
        "notFound": []
    }))
}

fn jmap_blob_upload(
    state: &MailJmapState,
    auth: &HostedAuth,
    principal: &str,
    args: &Value,
) -> std::result::Result<Value, Value> {
    require_mail_account(args)?;
    let Some(create) = args.get("create").and_then(Value::as_object) else {
        return Err(jmap_error("invalidArguments", "Blob/upload expects create"));
    };
    let mut created = serde_json::Map::new();
    let mut not_created = serde_json::Map::new();
    state
        .kernel
        .write(auth, |loom| {
            let ns = resolve_mail_workspace(loom, &state.workspace)?;
            for (creation_id, value) in create {
                match build_blob_upload_bytes(loom, ns, principal, value, &created) {
                    Ok(bytes) => {
                        let digest = mail::put_blob(loom, ns, principal, &bytes)?;
                        created.insert(
                            creation_id.clone(),
                            json!({
                                "id": digest.to_hex(),
                                "type": value.get("type").and_then(Value::as_str).unwrap_or("application/octet-stream"),
                                "size": bytes.len()
                            }),
                        );
                    }
                    Err(err) => {
                        not_created.insert(creation_id.clone(), err);
                    }
                }
            }
            Ok(())
        })
        .map_err(jmap_loom_error)?;
    Ok(json!({
        "accountId": "mail",
        "created": created,
        "notCreated": not_created
    }))
}

fn jmap_blob_get(
    state: &MailJmapState,
    auth: &HostedAuth,
    principal: &str,
    args: &Value,
) -> std::result::Result<Value, Value> {
    require_mail_account(args)?;
    let ids = optional_string_array(args, "ids")?
        .ok_or_else(|| jmap_error("invalidArguments", "ids is required"))?;
    let properties = optional_string_array(args, "properties")?
        .unwrap_or_else(|| vec!["data".to_string(), "size".to_string()]);
    validate_blob_get_properties(&properties)?;
    let offset = optional_usize(args, "offset")?.unwrap_or(0);
    let length = optional_usize(args, "length")?;
    let (list, not_found) = state
        .kernel
        .read(auth, |loom| {
            let ns = resolve_mail_workspace(loom, &state.workspace)?;
            let mut list = Vec::new();
            let mut not_found = Vec::new();
            for id in ids {
                let digest = match jmap_blob_digest(loom, &id) {
                    Ok(digest) => digest,
                    Err(_) => {
                        not_found.push(id);
                        continue;
                    }
                };
                let Some(bytes) = mail::get_blob(loom, ns, principal, &digest)? else {
                    not_found.push(id);
                    continue;
                };
                list.push(blob_get_item(&id, &bytes, offset, length, &properties));
            }
            Ok((list, not_found))
        })
        .map_err(jmap_loom_error)?;
    Ok(json!({
        "accountId": "mail",
        "list": list,
        "notFound": not_found
    }))
}

fn jmap_blob_lookup(
    state: &MailJmapState,
    auth: &HostedAuth,
    principal: &str,
    args: &Value,
) -> std::result::Result<Value, Value> {
    require_mail_account(args)?;
    let type_names = optional_string_array(args, "typeNames")?
        .ok_or_else(|| jmap_error("invalidArguments", "typeNames is required"))?;
    for name in &type_names {
        if !matches!(name.as_str(), "Mailbox" | "Thread" | "Email") {
            return Err(jmap_error(
                "unknownDataType",
                format!("unsupported Blob/lookup data type {name}"),
            ));
        }
    }
    let ids = optional_string_array(args, "ids")?
        .ok_or_else(|| jmap_error("invalidArguments", "ids is required"))?;
    let list = state
        .kernel
        .read(auth, |loom| {
            let ns = resolve_mail_workspace(loom, &state.workspace)?;
            let mut out = Vec::new();
            for id in ids {
                out.push(blob_lookup_item(loom, ns, principal, &id, &type_names)?);
            }
            Ok(out)
        })
        .map_err(jmap_loom_error)?;
    Ok(json!({
        "accountId": "mail",
        "list": list,
        "notFound": []
    }))
}

fn require_mail_account(args: &Value) -> std::result::Result<(), Value> {
    match args.get("accountId").and_then(Value::as_str) {
        Some("mail") | None => Ok(()),
        _ => Err(jmap_error("accountNotFound", "JMAP account not found")),
    }
}

fn build_blob_upload_bytes<S: loom_core::provider::ObjectStore>(
    loom: &loom_core::Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    value: &Value,
    created: &serde_json::Map<String, Value>,
) -> std::result::Result<Vec<u8>, Value> {
    let sources = value
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| jmap_error("invalidProperties", "Blob/upload data is required"))?;
    if sources.len() > JMAP_BLOB_MAX_DATA_SOURCES {
        return Err(jmap_error(
            "tooLarge",
            "Blob/upload has too many data sources",
        ));
    }
    let mut out = Vec::new();
    for source in sources {
        append_blob_source(loom, ns, principal, source, created, &mut out)?;
        if out.len() > JMAP_BLOB_MAX_SIZE {
            return Err(jmap_error("tooLarge", "Blob/upload exceeds maxSizeBlobSet"));
        }
    }
    Ok(out)
}

fn append_blob_source<S: loom_core::provider::ObjectStore>(
    loom: &loom_core::Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    source: &Value,
    created: &serde_json::Map<String, Value>,
    out: &mut Vec<u8>,
) -> std::result::Result<(), Value> {
    let text = source.get("data:asText");
    let base64 = source.get("data:asBase64");
    let blob_id = source.get("blobId");
    let selected = [text, base64, blob_id]
        .into_iter()
        .filter(|value| value.is_some_and(|value| !value.is_null()))
        .count();
    if selected != 1 {
        return Err(jmap_error(
            "invalidProperties",
            "Blob/upload data source must set exactly one data property",
        ));
    }
    if let Some(text) = text.and_then(Value::as_str) {
        out.extend_from_slice(text.as_bytes());
        return Ok(());
    }
    if let Some(encoded) = base64.and_then(Value::as_str) {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|_| jmap_error("invalidProperties", "invalid data:asBase64"))?;
        out.extend_from_slice(&bytes);
        return Ok(());
    }
    let Some(blob_id) = blob_id.and_then(Value::as_str) else {
        return Err(jmap_error("invalidProperties", "invalid blobId source"));
    };
    let blob_id = resolve_created_blob_id(blob_id, created)?;
    let digest = jmap_blob_digest(loom, &blob_id).map_err(jmap_loom_error)?;
    let bytes = mail::get_blob(loom, ns, principal, &digest)
        .map_err(jmap_loom_error)?
        .ok_or_else(|| jmap_error("notFound", "blob source not found"))?;
    let offset = optional_usize(source, "offset")
        .map_err(|_| jmap_error("invalidProperties", "invalid blob source offset"))?
        .unwrap_or(0);
    let length = optional_usize(source, "length")
        .map_err(|_| jmap_error("invalidProperties", "invalid blob source length"))?;
    let end = match length {
        Some(length) => offset.checked_add(length),
        None => Some(bytes.len()),
    }
    .ok_or_else(|| jmap_error("invalidProperties", "invalid blob source range"))?;
    if offset > bytes.len() || end > bytes.len() {
        return Err(jmap_error(
            "invalidProperties",
            "blob source range is not satisfiable",
        ));
    }
    out.extend_from_slice(&bytes[offset..end]);
    Ok(())
}

fn resolve_created_blob_id(
    blob_id: &str,
    created: &serde_json::Map<String, Value>,
) -> std::result::Result<String, Value> {
    let Some(creation_id) = blob_id.strip_prefix('#') else {
        return Ok(blob_id.to_string());
    };
    created
        .get(creation_id)
        .and_then(|value| value.get("id"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| jmap_error("invalidProperties", "blob source creation id not found"))
}

fn validate_blob_get_properties(properties: &[String]) -> std::result::Result<(), Value> {
    for property in properties {
        if property == "data"
            || property == "data:asText"
            || property == "data:asBase64"
            || property == "size"
        {
            continue;
        }
        if property.starts_with("digest:") {
            return Err(jmap_error(
                "invalidArguments",
                "Blob/get digest properties are not supported",
            ));
        }
        return Err(jmap_error(
            "invalidArguments",
            format!("unsupported Blob/get property {property}"),
        ));
    }
    Ok(())
}

fn blob_get_item(
    id: &str,
    bytes: &[u8],
    offset: usize,
    length: Option<usize>,
    properties: &[String],
) -> Value {
    let start = offset.min(bytes.len());
    let requested_end = length
        .and_then(|length| offset.checked_add(length))
        .unwrap_or(bytes.len());
    let end = requested_end.min(bytes.len());
    let selected = &bytes[start..end];
    let truncated = offset > bytes.len() || requested_end > bytes.len();
    let mut item = serde_json::Map::new();
    item.insert("id".to_string(), json!(id));
    for property in properties {
        match property.as_str() {
            "size" => {
                item.insert("size".to_string(), json!(bytes.len()));
            }
            "data" => {
                insert_blob_data(&mut item, selected, false);
            }
            "data:asText" => {
                insert_blob_data(&mut item, selected, true);
            }
            "data:asBase64" => {
                item.insert(
                    "data:asBase64".to_string(),
                    json!(base64::engine::general_purpose::STANDARD.encode(selected)),
                );
            }
            _ => {}
        }
    }
    if truncated {
        item.insert("isTruncated".to_string(), json!(true));
    }
    Value::Object(item)
}

fn insert_blob_data(item: &mut serde_json::Map<String, Value>, selected: &[u8], text_only: bool) {
    match std::str::from_utf8(selected) {
        Ok(text) => {
            item.insert("data:asText".to_string(), json!(text));
        }
        Err(_) => {
            item.insert("isEncodingProblem".to_string(), json!(true));
            if text_only {
                item.insert("data:asText".to_string(), Value::Null);
            } else {
                item.insert(
                    "data:asBase64".to_string(),
                    json!(base64::engine::general_purpose::STANDARD.encode(selected)),
                );
            }
        }
    }
}

fn blob_lookup_item<S: loom_core::provider::ObjectStore>(
    loom: &loom_core::Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    id: &str,
    type_names: &[String],
) -> Result<Value, LoomError> {
    let digest = match jmap_blob_digest(loom, id) {
        Ok(digest) => digest,
        Err(_) => {
            return Ok(empty_blob_lookup_item(id, type_names));
        }
    };
    if mail::get_blob(loom, ns, principal, &digest)?.is_none() {
        return Ok(empty_blob_lookup_item(id, type_names));
    }
    let digest_hex = digest.to_hex();
    let mut emails = Vec::new();
    let mut mailboxes = Vec::new();
    for mailbox in mail::list_mailboxes(loom, ns, principal)? {
        let mut mailbox_matched = false;
        for message in mail::list_messages(loom, ns, principal, &mailbox)? {
            if message.body == digest_hex {
                emails.push(jmap_message_id(&mailbox, &message.uid));
                mailbox_matched = true;
            }
        }
        if mailbox_matched {
            mailboxes.push(mailbox);
        }
    }
    let mut matched = serde_json::Map::new();
    for type_name in type_names {
        let values = match type_name.as_str() {
            "Email" | "Thread" => emails.clone(),
            "Mailbox" => mailboxes.clone(),
            _ => Vec::new(),
        };
        matched.insert(type_name.clone(), json!(values));
    }
    Ok(json!({
        "id": id,
        "matchedIds": matched
    }))
}

fn empty_blob_lookup_item(id: &str, type_names: &[String]) -> Value {
    let matched = type_names
        .iter()
        .map(|type_name| (type_name.clone(), json!([])))
        .collect::<serde_json::Map<_, _>>();
    json!({
        "id": id,
        "matchedIds": matched
    })
}

fn jmap_all_message_ids<S: loom_core::provider::ObjectStore>(
    loom: &loom_core::Loom<S>,
    ns: WorkspaceId,
    principal: &str,
) -> Result<Vec<String>, LoomError> {
    jmap_query_message_ids(loom, ns, principal, None, None, None, None)
}

fn jmap_query_message_ids<S: loom_core::provider::ObjectStore>(
    loom: &loom_core::Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    in_mailbox: Option<&str>,
    text: Option<&str>,
    from: Option<&str>,
    subject: Option<&str>,
) -> Result<Vec<String>, LoomError> {
    let mailboxes = match in_mailbox {
        Some(mailbox) => vec![mailbox.to_string()],
        None => mail::list_mailboxes(loom, ns, principal)?,
    };
    let mut ids = Vec::new();
    for mailbox in mailboxes {
        for message in mail::list_messages(loom, ns, principal, &mailbox)? {
            if email_matches(&message, text, from, subject) {
                ids.push(jmap_message_id(&mailbox, &message.uid));
            }
        }
    }
    Ok(ids)
}

fn create_email_from_blob<S: loom_core::provider::ObjectStore>(
    loom: &mut loom_core::Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    creation_id: &str,
    value: &Value,
) -> std::result::Result<Value, Value> {
    let Some(blob_id) = value.get("blobId").and_then(Value::as_str) else {
        return Err(jmap_error("invalidProperties", "blobId is required"));
    };
    let mailbox = email_create_mailbox(value)?;
    let digest = jmap_blob_digest(loom, blob_id).map_err(jmap_loom_error)?;
    let raw = mail::get_blob(loom, ns, principal, &digest)
        .map_err(jmap_loom_error)?
        .ok_or_else(|| jmap_error("notFound", "blob not found"))?;
    let uid = creation_id.to_string();
    if mail::get_message(loom, ns, principal, &mailbox, &uid)
        .map_err(jmap_loom_error)?
        .is_some()
    {
        return Err(jmap_error(
            "alreadyExists",
            "email creation id already exists",
        ));
    }
    mail::ingest_message(loom, ns, principal, &mailbox, &uid, &raw).map_err(jmap_loom_error)?;
    let flags = email_create_flags(value);
    if !flags.is_empty() {
        mail::set_flags(loom, ns, principal, &mailbox, &uid, &flags).map_err(jmap_loom_error)?;
    }
    let message = mail::get_message(loom, ns, principal, &mailbox, &uid)
        .map_err(jmap_loom_error)?
        .ok_or_else(|| jmap_error("notFound", "created email not found"))?;
    Ok(jmap_email_json(&mailbox, message, flags))
}

fn email_create_mailbox(value: &Value) -> std::result::Result<String, Value> {
    let Some(mailbox_ids) = value.get("mailboxIds").and_then(Value::as_object) else {
        return Err(jmap_error("invalidProperties", "mailboxIds is required"));
    };
    mailbox_ids
        .iter()
        .find_map(|(mailbox, enabled)| enabled.as_bool().unwrap_or(false).then(|| mailbox.clone()))
        .ok_or_else(|| jmap_error("invalidProperties", "mailboxIds must select one mailbox"))
}

fn email_create_flags(value: &Value) -> Vec<String> {
    value
        .get("keywords")
        .and_then(Value::as_object)
        .map(|keywords| {
            keywords
                .iter()
                .filter_map(|(name, enabled)| enabled.as_bool().unwrap_or(false).then_some(name))
                .map(|name| core_flag_from_jmap(name).to_string())
                .collect()
        })
        .unwrap_or_default()
}

fn email_matches(
    message: &MailMessage,
    text: Option<&str>,
    from: Option<&str>,
    subject: Option<&str>,
) -> bool {
    text.is_none_or(|needle| {
        contains_ci(&message.subject, needle) || contains_ci(&message.from, needle)
    }) && from.is_none_or(|needle| contains_ci(&message.from, needle))
        && subject.is_none_or(|needle| contains_ci(&message.subject, needle))
}

fn contains_ci(haystack: &str, needle: &str) -> bool {
    haystack.to_lowercase().contains(&needle.to_lowercase())
}

fn optional_string_array(
    args: &Value,
    name: &str,
) -> std::result::Result<Option<Vec<String>>, Value> {
    let Some(value) = args.get(name) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    let Some(values) = value.as_array() else {
        return Err(jmap_error(
            "invalidArguments",
            format!("{name} must be an array"),
        ));
    };
    let mut out = Vec::new();
    for value in values {
        let Some(value) = value.as_str() else {
            return Err(jmap_error(
                "invalidArguments",
                format!("{name} entries must be strings"),
            ));
        };
        out.push(value.to_string());
    }
    Ok(Some(out))
}

fn optional_usize(args: &Value, name: &str) -> std::result::Result<Option<usize>, Value> {
    let Some(value) = args.get(name) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    let Some(value) = value.as_u64() else {
        return Err(jmap_error(
            "invalidArguments",
            format!("{name} must be an unsigned integer"),
        ));
    };
    usize::try_from(value).map(Some).map_err(|_| {
        jmap_error(
            "invalidArguments",
            format!("{name} exceeds supported integer range"),
        )
    })
}

fn jmap_mailbox_json(id: &str, total: usize, unread: usize) -> Value {
    json!({
        "id": id,
        "name": id,
        "parentId": null,
        "role": if id.eq_ignore_ascii_case("inbox") { "inbox" } else { "" },
        "sortOrder": 0,
        "totalEmails": total,
        "unreadEmails": unread,
        "totalThreads": total,
        "unreadThreads": unread,
        "myRights": {
            "mayReadItems": true,
            "mayAddItems": true,
            "mayRemoveItems": true,
            "maySetSeen": true,
            "maySetKeywords": true,
            "mayCreateChild": false,
            "mayRename": true,
            "mayDelete": true,
            "maySubmit": false
        }
    })
}

fn jmap_email_json(mailbox: &str, message: MailMessage, flags: Vec<String>) -> Value {
    let id = jmap_message_id(mailbox, &message.uid);
    json!({
        "id": id,
        "blobId": message.body,
        "threadId": id,
        "mailboxIds": { mailbox: true },
        "keywords": jmap_keywords(flags),
        "size": message.size,
        "receivedAt": message.date,
        "messageId": message.message_id.into_iter().collect::<Vec<_>>(),
        "from": email_address_list([message.from]),
        "to": email_address_list(message.to),
        "subject": message.subject,
        "preview": "",
        "bodyStructure": {
            "partId": "raw",
            "blobId": message.body,
            "type": "message/rfc822",
            "size": message.size
        },
        "textBody": [],
        "htmlBody": [],
        "attachments": []
    })
}

fn jmap_parsed_email_json(message: MailMessage) -> Value {
    json!({
        "blobId": message.body,
        "threadId": null,
        "keywords": {},
        "size": message.size,
        "receivedAt": message.date,
        "messageId": message.message_id.into_iter().collect::<Vec<_>>(),
        "from": email_address_list([message.from]),
        "to": email_address_list(message.to),
        "subject": message.subject,
        "preview": "",
        "bodyStructure": {
            "partId": "raw",
            "blobId": message.body,
            "type": "message/rfc822",
            "size": message.size
        },
        "textBody": [],
        "htmlBody": [],
        "attachments": []
    })
}

fn email_address_list(values: impl IntoIterator<Item = String>) -> Vec<Value> {
    values
        .into_iter()
        .filter(|email| !email.is_empty())
        .map(|email| json!({ "name": null, "email": email }))
        .collect()
}

fn jmap_keywords(flags: Vec<String>) -> Value {
    Value::Object(
        flags
            .into_iter()
            .map(|flag| (jmap_keyword_from_core(&flag).to_string(), Value::Bool(true)))
            .collect(),
    )
}

fn jmap_keyword_from_core(flag: &str) -> &str {
    match flag.trim_start_matches('\\').to_ascii_lowercase().as_str() {
        "seen" => "$seen",
        "answered" => "$answered",
        "flagged" => "$flagged",
        "draft" => "$draft",
        "deleted" => "$deleted",
        _ => flag,
    }
}

fn core_flag_from_jmap(keyword: &str) -> &str {
    match keyword {
        "$seen" => "Seen",
        "$answered" => "Answered",
        "$flagged" => "Flagged",
        "$draft" => "Draft",
        "$deleted" => "Deleted",
        other => other,
    }
}

fn flag_eq(flag: &str, expected: &str) -> bool {
    flag.trim_start_matches('\\').eq_ignore_ascii_case(expected)
}

fn jmap_message_id(mailbox: &str, uid: &str) -> String {
    format!("{mailbox}/{uid}")
}

fn parse_jmap_message_id(id: &str) -> Option<(String, String)> {
    let (mailbox, uid) = id.split_once('/')?;
    (!mailbox.is_empty() && !uid.is_empty()).then(|| (mailbox.to_string(), uid.to_string()))
}

fn jmap_blob_digest<S: loom_core::provider::ObjectStore>(
    loom: &loom_core::Loom<S>,
    blob_id: &str,
) -> Result<Digest, LoomError> {
    if blob_id.contains(':') {
        return Digest::parse(blob_id);
    }
    let bytes = decode_hex_32(blob_id).ok_or_else(|| LoomError::invalid("invalid JMAP blobId"))?;
    Ok(Digest::of(loom.store().digest_algo(), bytes))
}

fn jmap_account_state<S: loom_core::provider::ObjectStore>(
    loom: &loom_core::Loom<S>,
    ns: WorkspaceId,
    principal: &str,
) -> Result<String, LoomError> {
    let mut bytes = b"loom-jmap-account-state-v1\0".to_vec();
    bytes.extend_from_slice(principal.as_bytes());
    bytes.push(0);
    for mailbox in mail::list_mailboxes(loom, ns, principal)? {
        bytes.extend_from_slice(mailbox.as_bytes());
        bytes.push(0);
        for message in mail::list_messages(loom, ns, principal, &mailbox)? {
            bytes.extend_from_slice(message.uid.as_bytes());
            bytes.push(0);
            bytes.extend_from_slice(message.body.as_bytes());
            bytes.push(0);
            bytes.extend_from_slice(message.size.to_string().as_bytes());
            bytes.push(0);
            for flag in mail::get_flags(loom, ns, principal, &mailbox, &message.uid)? {
                bytes.extend_from_slice(flag.as_bytes());
                bytes.push(0);
            }
            bytes.push(0);
        }
    }
    Ok(Digest::hash(loom.store().digest_algo(), &bytes).to_string())
}

fn decode_hex_32(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (idx, chunk) in value.as_bytes().chunks_exact(2).enumerate() {
        out[idx] = (hex_nibble(chunk[0])? << 4) | hex_nibble(chunk[1])?;
    }
    Some(out)
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn jmap_capabilities() -> Value {
    json!({
        "urn:ietf:params:jmap:core": {
            "maxSizeUpload": 16777216,
            "maxConcurrentUpload": 4,
            "maxSizeRequest": 16777216,
            "maxConcurrentRequests": 4,
            "maxCallsInRequest": 16,
            "maxObjectsInGet": 1024,
            "maxObjectsInSet": 1024,
            "collationAlgorithms": ["i;unicode-casemap"]
        },
        "urn:ietf:params:jmap:mail": {
            "maxMailboxesPerEmail": null,
            "maxMailboxDepth": 1,
            "maxSizeMailboxName": 255,
            "emailQuerySortOptions": ["receivedAt", "subject", "from"],
            "mayCreateTopLevelMailbox": true
        },
        JMAP_QUOTA_CAPABILITY: {
            "maxObjectsInGet": 1
        }
    })
}

fn content_type(headers: &HeaderMap) -> String {
    headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string()
}

fn hosted_auth(headers: &HeaderMap, policy: HostedAuthPolicy) -> JmapHttpResult<HostedAuth> {
    let principal = header(headers, "x-loom-principal")?;
    let passphrase = header(headers, "x-loom-passphrase")?;
    match (principal, passphrase) {
        (Some(principal), Some(passphrase)) => {
            let principal = WorkspaceId::parse(&principal).map_err(|err| {
                JmapHttpError::from(loom_error_response(StatusCode::BAD_REQUEST, err))
            })?;
            Ok(HostedAuth::passphrase(
                principal,
                passphrase,
                format!("jmap-{principal}"),
            ))
        }
        (None, None) if policy == HostedAuthPolicy::OwnerOrPassphrase => {
            Ok(HostedAuth::unauthenticated())
        }
        (None, None) => Err(error_response(
            StatusCode::UNAUTHORIZED,
            Code::AuthenticationFailed,
            "hosted JMAP requires x-loom-principal and x-loom-passphrase",
        )
        .into()),
        _ => Err(error_response(
            StatusCode::BAD_REQUEST,
            Code::InvalidArgument,
            "x-loom-principal and x-loom-passphrase must be provided together",
        )
        .into()),
    }
}

fn hosted_principal_name(
    kernel: &HostedKernel,
    auth: &HostedAuth,
    headers: &HeaderMap,
) -> JmapHttpResult<String> {
    let principal = match header(headers, "x-loom-principal")? {
        Some(principal) => principal,
        None => return Ok("root".to_string()),
    };
    let principal_id = WorkspaceId::parse(&principal)
        .map_err(|err| JmapHttpError::from(loom_error_response(StatusCode::BAD_REQUEST, err)))?;
    kernel
        .read(auth, |loom| {
            let Some(identity) = loom.identity_store() else {
                return Ok(principal);
            };
            identity
                .principal(principal_id)
                .map(|principal| principal.name.clone())
        })
        .map_err(|err| JmapHttpError::from(loom_error_response(StatusCode::FORBIDDEN, err)))
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

fn header(headers: &HeaderMap, name: &str) -> JmapHttpResult<Option<String>> {
    match headers.get(name) {
        Some(value) => value
            .to_str()
            .map(|value| Some(value.to_string()))
            .map_err(|_| {
                error_response(
                    StatusCode::BAD_REQUEST,
                    Code::InvalidArgument,
                    &format!("{name} header must be UTF-8"),
                )
                .into()
            }),
        None => Ok(None),
    }
}

fn jmap_loom_error(err: LoomError) -> Value {
    jmap_error(HostedError::from_error(err).code_name, "operation failed")
}

fn jmap_error(kind: impl Into<String>, description: impl Into<String>) -> Value {
    json!({
        "type": kind.into(),
        "description": description.into()
    })
}

fn loom_error_response(status: StatusCode, err: LoomError) -> Response {
    let err = HostedError::from_error(err);
    jmap_json_status(
        status,
        json!({
            "error": err.code_name,
            "message": err.message
        }),
    )
}

fn error_response(status: StatusCode, code: Code, message: &str) -> Response {
    jmap_json_status(
        status,
        json!({
            "error": code.as_str(),
            "message": message
        }),
    )
}

fn jmap_request_error(status: StatusCode, kind: impl Into<String>, description: &str) -> Response {
    jmap_json_status(status, jmap_error(kind, description))
}

fn jmap_json(value: Value) -> Response {
    jmap_json_status(StatusCode::OK, value)
}

fn jmap_json_status(status: StatusCode, value: Value) -> Response {
    (
        status,
        [(CONTENT_TYPE, "application/json; charset=utf-8")],
        Body::from(value.to_string()),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blob_get_defaults_to_text_data_and_size() {
        let item = blob_get_item(
            "blob-1",
            b"hello world",
            6,
            Some(5),
            &["data".to_string(), "size".to_string()],
        );

        assert_eq!(item["id"], "blob-1");
        assert_eq!(item["size"], 11);
        assert_eq!(item["data:asText"], "world");
        assert!(item.get("isTruncated").is_none());
    }

    #[test]
    fn blob_get_marks_truncated_and_encoding_problem() {
        let item = blob_get_item("blob-1", &[0xff, 0x00], 0, Some(8), &["data".to_string()]);

        assert_eq!(item["isTruncated"], true);
        assert_eq!(item["isEncodingProblem"], true);
        assert_eq!(item["data:asBase64"], "/wA=");
    }

    #[test]
    fn blob_get_rejects_unadvertised_digest_properties() {
        let err = validate_blob_get_properties(&["digest:sha-256".to_string()]).unwrap_err();

        assert_eq!(err["type"], "invalidArguments");
    }

    #[test]
    fn quota_object_projects_account_octets_only() {
        let usage = mail::MailAccountUsage {
            used_octets: 42,
            hard_limit_octets: Some(100),
        };
        let quota = jmap_quota_object(&usage);

        assert_eq!(quota["id"], JMAP_QUOTA_ACCOUNT_OCTETS_ID);
        assert_eq!(quota["resourceType"], "octets");
        assert_eq!(quota["used"], 42);
        assert_eq!(quota["hardLimit"], 100);
        assert_eq!(quota["scope"], "account");
        assert_eq!(quota["types"], json!(["Mail"]));
    }

    #[test]
    fn quota_capability_is_advertised_and_accepted_in_using() {
        let capabilities = jmap_capabilities();
        assert!(capabilities.get(JMAP_QUOTA_CAPABILITY).is_some());
        validate_jmap_using(&json!({
            "using": [
                "urn:ietf:params:jmap:core",
                "urn:ietf:params:jmap:mail",
                JMAP_QUOTA_CAPABILITY
            ]
        }))
        .unwrap();
    }

    #[test]
    fn created_blob_references_resolve_with_hash_prefix() {
        let mut created = serde_json::Map::new();
        created.insert("a".to_string(), json!({ "id": "abc" }));

        assert_eq!(resolve_created_blob_id("#a", &created).unwrap(), "abc");
        assert!(resolve_created_blob_id("#missing", &created).is_err());
    }
}
