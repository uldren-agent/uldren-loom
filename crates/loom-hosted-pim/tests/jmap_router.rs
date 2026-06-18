use axum::body::{Body, to_bytes};
use axum::http::header::CONTENT_TYPE;
use axum::http::{Request, StatusCode};
use loom_core::mail::{self, MailboxMeta};
use loom_hosted_core::test_support::{init, nid, temp_path};
use loom_hosted_core::{HostedAuth, HostedAuthPolicy, HostedKernel, HostedWriteGuard};
use loom_hosted_pim::mail_jmap_router_with_policy;
use serde_json::{Value, json};
use tower::ServiceExt as _;

#[test]
fn jmap_router_serves_mailbox_query_get_and_set() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let path = temp_path("jmap-router");
        let ns = init(&path, None);
        let kernel = HostedKernel::new(&path).with_write_guard(HostedWriteGuard::DirectFileLock);
        kernel
            .write(
                &HostedAuth::passphrase(nid(1), "root-pass", "jmap-setup"),
                |loom| {
                    mail::create_mailbox(
                        loom,
                        ns,
                        "root",
                        "inbox",
                        &MailboxMeta {
                            display_name: "Inbox".into(),
                        },
                    )?;
                    mail::ingest_message(
                        loom,
                        ns,
                        "root",
                        "inbox",
                        "1",
                        b"From: bob@example.com\r\nTo: root@example.com\r\nSubject: Hello\r\nMessage-ID: <m1@example.com>\r\n\r\nBody",
                    )?;
                    mail::set_account_hard_limit(loom, ns, "root", Some(500))?;
                    Ok(())
                },
            )
            .unwrap();
        let router = mail_jmap_router_with_policy(
            kernel.clone(),
            "main",
            16 * 1024 * 1024,
            HostedAuthPolicy::OwnerOrPassphrase,
        );
        let session = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/jmap/session")
                    .header("x-loom-principal", nid(1).to_string())
                    .header("x-loom-passphrase", "root-pass")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(session.status(), StatusCode::OK);
        let session_body = to_bytes(session.into_body(), 16 * 1024 * 1024)
            .await
            .unwrap();
        let session_json: Value = serde_json::from_slice(&session_body).unwrap();
        assert_eq!(session_json.get("eventSourceUrl"), Some(&Value::Null));
        assert_ne!(
            session_json.get("state").and_then(Value::as_str),
            Some("0")
        );
        assert_eq!(
            session_json
                .get("capabilities")
                .and_then(|value| value.get("urn:ietf:params:jmap:core"))
                .and_then(|value| value.get("maxSizeUpload"))
                .and_then(Value::as_u64),
            Some(16_777_216)
        );
        assert!(
            session_json
                .get("capabilities")
                .and_then(|value| value.get("urn:ietf:params:jmap:quota"))
                .is_some()
        );
        assert!(
            session_json
                .get("accounts")
                .and_then(|value| value.get("mail"))
                .and_then(|value| value.get("accountCapabilities"))
                .and_then(|value| value.get("urn:ietf:params:jmap:quota"))
                .is_some()
        );
        let uploaded_raw = b"From: upload@example.com\r\nTo: root@example.com\r\nSubject: Uploaded\r\nMessage-ID: <uploaded@example.com>\r\n\r\nUploaded body";
        let upload = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/jmap/upload/mail")
                    .header("x-loom-principal", nid(1).to_string())
                    .header("x-loom-passphrase", "root-pass")
                    .header(CONTENT_TYPE, "message/rfc822")
                    .body(Body::from(uploaded_raw.as_slice()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(upload.status(), StatusCode::OK);
        let upload_body = to_bytes(upload.into_body(), 16 * 1024 * 1024)
            .await
            .unwrap();
        let upload_json: Value = serde_json::from_slice(&upload_body).unwrap();
        let blob_id = upload_json
            .get("blobId")
            .and_then(Value::as_str)
            .unwrap()
            .to_string();
        assert_eq!(
            upload_json.get("type").and_then(Value::as_str),
            Some("message/rfc822")
        );
        assert_eq!(
            upload_json.get("size").and_then(Value::as_u64),
            Some(uploaded_raw.len() as u64)
        );
        let download = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/jmap/download/mail/{blob_id}/message.eml"))
                    .header("x-loom-principal", nid(1).to_string())
                    .header("x-loom-passphrase", "root-pass")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(download.status(), StatusCode::OK);
        let downloaded = to_bytes(download.into_body(), 16 * 1024 * 1024)
            .await
            .unwrap();
        assert_eq!(downloaded.as_ref(), uploaded_raw);

        let body = json!({
            "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail", "urn:ietf:params:jmap:quota"],
            "methodCalls": [
                ["Core/echo", {"hello": "world"}, "echo"],
                ["Mailbox/get", {}, "a"],
                ["Mailbox/query", {}, "mailbox-query"],
                ["Mailbox/changes", {"sinceState": "0"}, "mailbox-changes"],
                ["Mailbox/queryChanges", {"sinceQueryState": "0"}, "mailbox-query-changes"],
                ["Email/query", {"filter": {"inMailbox": "inbox", "text": "hello"}}, "b"],
                ["Email/get", {"ids": ["inbox/1"]}, "c"],
                ["Email/get", {"#ids": {"resultOf": "b", "name": "Email/query", "path": "/ids"}}, "ref"],
                ["Quota/get", {"ids": ["mail-octets", "missing"]}, "quota"],
                ["Email/set", {"update": {"inbox/1": {"keywords": {"$seen": true}}}}, "d"],
                ["Email/import", {"emails": {"imported": {"blobId": blob_id, "mailboxIds": {"inbox": true}, "keywords": {"$seen": true}}}}, "e"],
                ["Email/set", {"create": {"created": {"blobId": blob_id, "mailboxIds": {"inbox": true}, "keywords": {"$flagged": true}}}}, "f"],
                ["Email/copy", {"fromAccountId": "mail", "accountId": "mail", "create": {"copied": {"id": "inbox/1", "mailboxIds": {"inbox": true}, "keywords": {"$answered": true}}}}, "copy"],
                ["Email/parse", {"blobIds": [blob_id]}, "parse"],
                ["Email/get", {"ids": ["inbox/imported", "inbox/created"]}, "g"],
                ["Thread/get", {"ids": ["inbox/1"]}, "thread"],
                ["Thread/changes", {"sinceState": "0"}, "thread-changes"],
                ["Identity/get", {}, "h"],
                ["Identity/changes", {"sinceState": "x"}, "identity-changes"],
                ["Identity/set", {"update": {"default": {"name": "Root"}}}, "identity-set"],
                ["Email/changes", {"sinceState": "0"}, "i"],
                ["Email/queryChanges", {"sinceQueryState": "0", "filter": {"inMailbox": "inbox"}}, "j"],
                ["SearchSnippet/get", {"emailIds": ["inbox/1"], "filter": {"text": "hello"}}, "snippet"],
                ["EmailSubmission/get", {}, "submission"],
                ["VacationResponse/get", {}, "vacation"]
            ]
        });
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/jmap/api")
                    .header("x-loom-principal", nid(1).to_string())
                    .header("x-loom-passphrase", "root-pass")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 16 * 1024 * 1024)
            .await
            .unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        assert!(body.contains("\"Core/echo\""), "{body}");
        assert!(body.contains("\"hello\":\"world\""), "{body}");
        assert!(body.contains("\"Mailbox/get\""), "{body}");
        assert!(body.contains("\"Mailbox/query\""), "{body}");
        assert!(body.contains("\"Mailbox/changes\""), "{body}");
        assert!(body.contains("\"Mailbox/queryChanges\""), "{body}");
        assert!(body.contains("\"Email/query\""), "{body}");
        assert!(body.contains("\"inbox/1\""), "{body}");
        assert!(body.contains("\"Email/import\""), "{body}");
        assert!(body.contains("\"Email/copy\""), "{body}");
        assert!(body.contains("\"inbox/copied\""), "{body}");
        assert!(body.contains("\"Quota/get\""), "{body}");
        assert!(body.contains("\"Email/parse\""), "{body}");
        assert!(body.contains("\"inbox/imported\""), "{body}");
        assert!(body.contains("\"inbox/created\""), "{body}");
        assert!(body.contains("\"Thread/get\""), "{body}");
        assert!(body.contains("\"Identity/get\""), "{body}");
        assert!(body.contains("\"Identity/changes\""), "{body}");
        assert!(body.contains("\"Identity/set\""), "{body}");
        assert!(body.contains("\"id\":\"default\""), "{body}");
        assert!(body.contains("\"Email/changes\""), "{body}");
        assert!(body.contains("\"Email/queryChanges\""), "{body}");
        assert!(body.contains("\"SearchSnippet/get\""), "{body}");
        assert!(body.contains("unsupported JMAP method EmailSubmission/get"), "{body}");
        assert!(body.contains("unsupported JMAP method VacationResponse/get"), "{body}");
        let parsed: Value = serde_json::from_str(&body).unwrap();
        assert_ne!(parsed.get("sessionState").and_then(Value::as_str), Some("0"));
        let responses = parsed
            .get("methodResponses")
            .and_then(Value::as_array)
            .unwrap();
        let result_ref = responses
            .iter()
            .find(|response| response.get(2).and_then(Value::as_str) == Some("ref"))
            .unwrap()
            .get(1)
            .unwrap();
        assert!(
            result_ref
                .get("list")
                .and_then(Value::as_array)
                .is_some_and(|emails| emails
                    .iter()
                    .any(|email| email.get("id") == Some(&json!("inbox/1")))),
            "{body}"
        );
        let mailbox_query = responses
            .iter()
            .find(|response| response.get(2).and_then(Value::as_str) == Some("mailbox-query"))
            .unwrap()
            .get(1)
            .unwrap();
        assert!(
            mailbox_query
                .get("ids")
                .and_then(Value::as_array)
                .is_some_and(|ids| ids.iter().any(|id| id == "inbox")),
            "{body}"
        );
        let thread = responses
            .iter()
            .find(|response| response.get(2).and_then(Value::as_str) == Some("thread"))
            .unwrap()
            .get(1)
            .unwrap();
        assert!(
            thread
                .get("list")
                .and_then(Value::as_array)
                .is_some_and(|threads| threads
                    .iter()
                    .any(|thread| thread.get("id") == Some(&json!("inbox/1")))),
            "{body}"
        );
        let parsed_email = responses
            .iter()
            .find(|response| response.get(2).and_then(Value::as_str) == Some("parse"))
            .unwrap()
            .get(1)
            .unwrap();
        assert!(
            parsed_email
                .get("parsed")
                .and_then(Value::as_object)
                .is_some_and(|parsed| parsed.contains_key(&blob_id)),
            "{body}"
        );
        let quota = responses
            .iter()
            .find(|response| response.get(2).and_then(Value::as_str) == Some("quota"))
            .unwrap()
            .get(1)
            .unwrap();
        assert_eq!(
            quota
                .get("list")
                .and_then(Value::as_array)
                .and_then(|list| list.first())
                .and_then(|quota| quota.get("id")),
            Some(&json!("mail-octets"))
        );
        assert_eq!(
            quota
                .get("list")
                .and_then(Value::as_array)
                .and_then(|list| list.first())
                .and_then(|quota| quota.get("used"))
                .and_then(Value::as_u64),
            Some(
                b"From: bob@example.com\r\nTo: root@example.com\r\nSubject: Hello\r\nMessage-ID: <m1@example.com>\r\n\r\nBody"
                    .len() as u64
            )
        );
        assert_eq!(
            quota
                .get("list")
                .and_then(Value::as_array)
                .and_then(|list| list.first())
                .and_then(|quota| quota.get("hardLimit"))
                .and_then(Value::as_u64),
            Some(500)
        );
        assert_eq!(
            quota.get("notFound").and_then(Value::as_array),
            Some(&vec![json!("missing")])
        );
        for call_id in ["d", "e", "f", "copy"] {
            let response = responses
                .iter()
                .find(|response| response.get(2).and_then(Value::as_str) == Some(call_id))
                .unwrap();
            let args = response.get(1).unwrap();
            assert_ne!(
                args.get("oldState").and_then(Value::as_str),
                args.get("newState").and_then(Value::as_str),
                "{body}"
            );
        }
        let changes = responses
            .iter()
            .find(|response| response.get(2).and_then(Value::as_str) == Some("i"))
            .unwrap()
            .get(1)
            .unwrap();
        assert!(
            changes
                .get("created")
                .and_then(Value::as_array)
                .is_some_and(|ids| ids.iter().any(|id| id == "inbox/imported")),
            "{body}"
        );
        let query_changes = responses
            .iter()
            .find(|response| response.get(2).and_then(Value::as_str) == Some("j"))
            .unwrap()
            .get(1)
            .unwrap();
        assert!(
            query_changes
                .get("added")
                .and_then(Value::as_array)
                .is_some_and(|ids| ids
                    .iter()
                    .any(|entry| entry.get("id") == Some(&json!("inbox/created")))),
            "{body}"
        );

        let flags = kernel
            .read(
                &HostedAuth::passphrase(nid(1), "root-pass", "jmap-check"),
                |loom| mail::get_flags(loom, ns, "root", "inbox", "1"),
            )
            .unwrap();
        assert_eq!(flags, vec!["Seen"]);
        let imported = kernel
            .read(
                &HostedAuth::passphrase(nid(1), "root-pass", "jmap-import-check"),
                |loom| mail::get_flags(loom, ns, "root", "inbox", "imported"),
            )
            .unwrap();
        assert_eq!(imported, vec!["Seen"]);
        let created = kernel
            .read(
                &HostedAuth::passphrase(nid(1), "root-pass", "jmap-create-check"),
                |loom| mail::get_flags(loom, ns, "root", "inbox", "created"),
            )
            .unwrap();
        assert_eq!(created, vec!["Flagged"]);
        let copied = kernel
            .read(
                &HostedAuth::passphrase(nid(1), "root-pass", "jmap-copy-check"),
                |loom| mail::get_flags(loom, ns, "root", "inbox", "copied"),
            )
            .unwrap();
        assert_eq!(copied, vec!["Answered"]);

        let unknown_capability = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/jmap/api")
                    .header("x-loom-principal", nid(1).to_string())
                    .header("x-loom-passphrase", "root-pass")
                    .body(Body::from(
                        json!({
                            "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:unknown"],
                            "methodCalls": []
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unknown_capability.status(), StatusCode::BAD_REQUEST);
        let unknown_body = to_bytes(unknown_capability.into_body(), 16 * 1024 * 1024)
            .await
            .unwrap();
        let unknown_json: Value = serde_json::from_slice(&unknown_body).unwrap();
        assert_eq!(
            unknown_json.get("type").and_then(Value::as_str),
            Some("unknownCapability")
        );
    });
}
