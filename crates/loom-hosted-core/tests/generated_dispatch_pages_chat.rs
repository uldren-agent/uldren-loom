use loom_client::{LocalLoomClient, generated_api::Chat, types::LoomSession};
use loom_codec::Value;
use loom_core::{Algo, Digest, FacetKind, WorkspaceId};
use loom_hosted_core::generated_dispatch::{Dispatched, dispatch};
use loom_pages::PageCreateRequest;

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let dir =
        std::env::temp_dir().join(format!("loom-hosted-dispatch-{}-{tag}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn text(value: &str) -> Value {
    Value::Text(value.to_string())
}

fn unary_text(out: Dispatched) -> String {
    match out {
        Dispatched::Unary(Value::Text(value)) => value,
        _ => panic!("expected unary text result"),
    }
}

fn block<T>(
    fut: impl ::core::future::Future<Output = Result<T, loom_core::LoomError>>,
) -> Result<T, loom_core::LoomError> {
    let mut fut = ::std::pin::pin!(fut);
    match fut.as_mut().poll(&mut ::core::task::Context::from_waker(
        ::std::task::Waker::noop(),
    )) {
        ::core::task::Poll::Ready(output) => output,
        ::core::task::Poll::Pending => Err(loom_core::LoomError::new(
            loom_core::Code::Internal,
            "in-process future returned Pending",
        )),
    }
}

fn meetings_import_input() -> Vec<u8> {
    let source_digest = Digest::hash(Algo::Blake3, b"dispatch-meetings").to_string();
    format!(
        r#"{{"snapshot_version":1,"profile":"generic","source_system":"generic","source_scope":"dispatch","observed_at":500,"coverage":"complete","items":[{{"source_entity_id":"source-a","source_digest":"{source_digest}","title":"Dispatch","summary_text":"Dispatch summary","transcript_spans":[{{"text":"Route through hosted dispatch."}}]}}]}}"#
    )
    .into_bytes()
}

fn exec_apply_request(workspace: &str, base: &str, fork: &str) -> Vec<u8> {
    loom_codec::encode(&Value::Map(vec![
        (text("workspace"), text(workspace)),
        (text("base"), text(base)),
        (text("fork"), text(fork)),
        (text("author"), text("alice")),
        (text("timestamp_ms"), Value::Uint(3_000)),
    ]))
    .expect("encode exec apply request")
}

fn seed_exec_apply_fixture(client: &LocalLoomClient, session: &LoomSession) {
    client
        .write_file(session, "repo", "a.txt", b"base", 0)
        .expect("write base");
    client.stage_all(session, "repo").expect("stage base");
    client
        .commit(session, "repo", "alice", "base", 1_000)
        .expect("commit base");
    client
        .branch(session, "repo", "feature")
        .expect("create feature branch");
    client
        .checkout(session, "repo", "feature")
        .expect("checkout feature");
    client
        .write_file(session, "repo", "b.txt", b"feature", 0)
        .expect("write feature");
    client.stage_all(session, "repo").expect("stage feature");
    client
        .commit(session, "repo", "alice", "feature", 2_000)
        .expect("commit feature");
}

#[test]
fn generated_dispatch_calls_local_exec_apply_cbor() {
    let dir = temp_dir("exec-apply");
    let client = LocalLoomClient::new(dir.join("t.loom"));
    client.create().expect("create store");
    let session = client.open().expect("open");

    let err = match dispatch(
        &client,
        &session,
        "Exec",
        "apply_cbor",
        &[Value::Null, Value::Bytes(b"not cbor".to_vec())],
    ) {
        Ok(_) => panic!("dispatch must reject malformed apply request"),
        Err(err) => err,
    };
    assert_eq!(err.code, loom_core::Code::InvalidArgument);

    client.close(&session);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn generated_dispatch_exec_apply_matches_local_output() {
    let direct_dir = temp_dir("exec-apply-direct");
    let direct = LocalLoomClient::new(direct_dir.join("t.loom"));
    direct.create().expect("create direct store");
    let direct_session = direct.open().expect("open direct");
    seed_exec_apply_fixture(&direct, &direct_session);
    let request = exec_apply_request("repo", "main", "feature");
    let direct_output = direct
        .apply_cbor(&direct_session, &request)
        .expect("direct apply");
    direct.close(&direct_session);

    let hosted_dir = temp_dir("exec-apply-hosted");
    let hosted = LocalLoomClient::new(hosted_dir.join("t.loom"));
    hosted.create().expect("create hosted store");
    let hosted_session = hosted.open().expect("open hosted");
    seed_exec_apply_fixture(&hosted, &hosted_session);
    let hosted_output = match dispatch(
        &hosted,
        &hosted_session,
        "Exec",
        "apply_cbor",
        &[Value::Null, Value::Bytes(request)],
    )
    .expect("dispatch apply")
    {
        Dispatched::Unary(Value::Bytes(bytes)) => bytes,
        _ => panic!("expected bytes output"),
    };
    assert_eq!(hosted_output, direct_output);
    hosted.close(&hosted_session);

    std::fs::remove_dir_all(&direct_dir).ok();
    std::fs::remove_dir_all(&hosted_dir).ok();
}

#[test]
fn generated_dispatch_calls_local_meetings_import_snapshot() {
    let dir = temp_dir("meetings-import");
    let client = LocalLoomClient::new(dir.join("t.loom"));
    client.create().expect("create store");
    let session = client.open().expect("open");

    let out = unary_text(
        dispatch(
            &client,
            &session,
            "Meetings",
            "meetings_import_snapshot",
            &[
                Value::Null,
                text("studio"),
                text("generic"),
                Value::Bytes(meetings_import_input()),
                Value::Bool(false),
            ],
        )
        .expect("dispatch meetings import"),
    );
    assert!(out.contains(r#""profile":"meetings""#));
    assert!(out.contains(r#""source_scope":"dispatch""#));
    assert!(out.contains(r#""rows_imported":1"#));
    assert!(out.contains(r#""operations_planned":4"#));
    assert!(out.contains(r#""operations_applied":4"#));

    client.close(&session);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn generated_dispatch_meetings_dry_run_matches_local_output_and_rejects_malformed_input() {
    let direct_dir = temp_dir("meetings-dry-run-direct");
    let direct = LocalLoomClient::new(direct_dir.join("t.loom"));
    direct.create().expect("create direct store");
    let direct_session = direct.open().expect("open direct");
    let input = meetings_import_input();
    let direct_output = direct
        .meetings_import_snapshot(&direct_session, "studio", "generic", &input, true)
        .expect("direct meetings dry-run");
    direct.close(&direct_session);

    let hosted_dir = temp_dir("meetings-dry-run-hosted");
    let hosted = LocalLoomClient::new(hosted_dir.join("t.loom"));
    hosted.create().expect("create hosted store");
    let hosted_session = hosted.open().expect("open hosted");
    let hosted_output = unary_text(
        dispatch(
            &hosted,
            &hosted_session,
            "Meetings",
            "meetings_import_snapshot",
            &[
                Value::Null,
                text("studio"),
                text("generic"),
                Value::Bytes(input),
                Value::Bool(true),
            ],
        )
        .expect("dispatch meetings dry-run"),
    );
    assert_eq!(hosted_output, direct_output);
    let err = match dispatch(
        &hosted,
        &hosted_session,
        "Meetings",
        "meetings_import_snapshot",
        &[
            Value::Null,
            text("studio"),
            text("generic"),
            Value::Bytes(b"not json".to_vec()),
            Value::Bool(true),
        ],
    ) {
        Ok(_) => panic!("malformed meetings input must fail"),
        Err(err) => err,
    };
    assert_eq!(err.code, loom_core::Code::InvalidArgument);
    hosted.close(&hosted_session);

    std::fs::remove_dir_all(&direct_dir).ok();
    std::fs::remove_dir_all(&hosted_dir).ok();
}

#[test]
fn generated_dispatch_calls_local_pages_update_json() {
    let dir = temp_dir("pages");
    let client = LocalLoomClient::new(dir.join("t.loom"));
    client.create().expect("create store");
    let session = client.open().expect("open");
    let workspace = client
        .workspace_create(&session, Some("repo"), Some(FacetKind::Document))
        .expect("workspace");
    client
        .with_session(&session, |loom| {
            let space = loom_pages::create_space(loom, workspace, "studio", "eng", "Eng", None)?;
            loom_pages::create_page(
                loom,
                workspace,
                PageCreateRequest {
                    workspace_id: "studio",
                    page_id: "page-1",
                    space_id: "eng",
                    parent_page_id: None,
                    title: "Roadmap",
                    expected_root: Some(&space.profile_root),
                },
            )?;
            Ok(())
        })
        .expect("seed page");
    client.save(&session).expect("save seed");

    let out = unary_text(
        dispatch(
            &client,
            &session,
            "Pages",
            "pages_update_json",
            &[
                Value::Null,
                text("repo"),
                text("studio"),
                text("page-1"),
                text("dispatch body"),
                Value::Null,
            ],
        )
        .expect("dispatch update"),
    );
    assert!(out.contains("\"page_id\":\"page-1\""));
    client
        .with_session(&session, |loom| {
            let page = loom_pages::get_page(loom, workspace, "studio", "page-1")?.expect("page");
            assert_eq!(page.draft_body_text.as_deref(), Some("dispatch body\n"));
            Ok(())
        })
        .expect("read page");
    client.close(&session);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn generated_dispatch_calls_local_chat_string_json_methods() {
    let dir = temp_dir("chat");
    let client = LocalLoomClient::new(dir.join("t.loom"));
    client.create().expect("create store");
    let session = client.open().expect("open");
    let workspace = client
        .workspace_create(&session, Some("repo"), Some(FacetKind::Document))
        .expect("workspace");
    let channel_id = WorkspaceId::from_bytes([9; 16]);
    client
        .with_session(&session, |loom| {
            loom_chat::ensure_channel(
                loom, workspace, "studio", channel_id, "general", "General", None,
            )?;
            Ok(())
        })
        .expect("seed channel");
    client.save(&session).expect("save seed");

    dispatch(
        &client,
        &session,
        "Chat",
        "chat_post_message_json",
        &[
            Value::Null,
            text("repo"),
            text("studio"),
            text("general"),
            text("m1"),
            Value::Null,
            text("hello"),
            Value::Null,
        ],
    )
    .expect("dispatch post");
    dispatch(
        &client,
        &session,
        "Chat",
        "chat_edit_message_json",
        &[
            Value::Null,
            text("repo"),
            text("studio"),
            text("general"),
            text("m1"),
            text("edited"),
            Value::Null,
        ],
    )
    .expect("dispatch edit");
    dispatch(
        &client,
        &session,
        "Chat",
        "chat_invoke_agent_json",
        &[
            Value::Null,
            text("repo"),
            text("studio"),
            text("general"),
            text("inv-1"),
            text(&WorkspaceId::from_bytes([7; 16]).to_string()),
            text("[\"m1\"]"),
            text("summarize"),
            Value::Null,
        ],
    )
    .expect("dispatch invoke");
    dispatch(
        &client,
        &session,
        "Chat",
        "chat_emoji_register_json",
        &[
            Value::Null,
            text("repo"),
            text("studio"),
            text("ship"),
            Value::Null,
        ],
    )
    .expect("dispatch emoji register");
    dispatch(
        &client,
        &session,
        "Chat",
        "chat_add_reaction_json",
        &[
            Value::Null,
            text("repo"),
            text("studio"),
            text("general"),
            text("m1"),
            text("ship"),
            Value::Null,
        ],
    )
    .expect("dispatch add reaction");
    dispatch(
        &client,
        &session,
        "Chat",
        "chat_remove_reaction_json",
        &[
            Value::Null,
            text("repo"),
            text("studio"),
            text("general"),
            text("m1"),
            text("ship"),
            Value::Null,
        ],
    )
    .expect("dispatch remove reaction");
    dispatch(
        &client,
        &session,
        "Chat",
        "chat_update_cursor_json",
        &[
            Value::Null,
            text("repo"),
            text("studio"),
            text("general"),
            Value::Uint(1),
            Value::Null,
        ],
    )
    .expect("dispatch update cursor");
    dispatch(
        &client,
        &session,
        "Chat",
        "chat_emoji_unregister_json",
        &[
            Value::Null,
            text("repo"),
            text("studio"),
            text("ship"),
            Value::Null,
        ],
    )
    .expect("dispatch emoji unregister");

    client
        .with_session(&session, |loom| {
            let channel = loom_chat::channel_projection(loom, workspace, "studio", "general")?;
            assert_eq!(channel.messages[0].body, b"edited");
            assert!(channel.messages[0].reactions.is_empty());
            assert_eq!(channel.agent_invocations[0].source_message_ids, ["m1"]);
            assert_eq!(channel.agent_invocations[0].prompt, b"summarize");
            let registry = loom_chat::emoji_registry(loom, workspace, "studio")?;
            assert!(registry.custom.is_empty());
            let cursor = loom_chat::read_cursor(loom, workspace, "studio", "general")?;
            assert_eq!(cursor.next_sequence, 1);
            Ok(())
        })
        .expect("read channel");
    client.close(&session);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn mu_6h_k_e_generated_dispatch_chat_byte_body_methods_preserve_invalid_utf8() {
    let dir = temp_dir("chat-byte-dispatch");
    let client = LocalLoomClient::new(dir.join("t.loom"));
    client.create().expect("create store");
    let session = client.open().expect("open");
    let workspace = client
        .workspace_create(&session, Some("repo"), Some(FacetKind::Document))
        .expect("workspace");
    let channel_id = WorkspaceId::from_bytes([135; 16]);
    let agent = WorkspaceId::from_bytes([136; 16]).to_string();
    let post_body = vec![0, 0xff, b'h'];
    let edit_body = vec![0xfe, b'e', 0x80];
    let prompt = vec![b'p', 0xff, 0];
    client
        .with_session(&session, |loom| {
            loom_chat::ensure_channel(
                loom, workspace, "studio", channel_id, "general", "General", None,
            )?;
            Ok(())
        })
        .expect("seed channel");
    client.save(&session).expect("save seed");

    dispatch(
        &client,
        &session,
        "Chat",
        "chat_post_message_bytes_json",
        &[
            Value::Null,
            text("repo"),
            text("studio"),
            text("general"),
            text("m1"),
            Value::Null,
            Value::Bytes(post_body),
            Value::Null,
        ],
    )
    .expect("dispatch post bytes");
    dispatch(
        &client,
        &session,
        "Chat",
        "chat_edit_message_bytes_json",
        &[
            Value::Null,
            text("repo"),
            text("studio"),
            text("general"),
            text("m1"),
            Value::Bytes(edit_body.clone()),
            Value::Null,
        ],
    )
    .expect("dispatch edit bytes");
    dispatch(
        &client,
        &session,
        "Chat",
        "chat_invoke_agent_bytes_json",
        &[
            Value::Null,
            text("repo"),
            text("studio"),
            text("general"),
            text("inv-1"),
            text(&agent),
            text("[\"m1\"]"),
            Value::Bytes(prompt.clone()),
            Value::Null,
        ],
    )
    .expect("dispatch invoke bytes");

    client
        .with_session(&session, |loom| {
            let channel = loom_chat::channel_projection(loom, workspace, "studio", "general")?;
            assert_eq!(channel.messages[0].body, edit_body);
            assert_eq!(channel.agent_invocations[0].prompt, prompt);
            assert_eq!(channel.agent_invocations[0].source_message_ids, ["m1"]);
            Ok(())
        })
        .expect("read byte projection");
    client.close(&session);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn mu_6h_k_d_b_generated_dispatch_chat_reads_match_local_owner() {
    let dir = temp_dir("chat-generated-read-dispatch");
    let client = LocalLoomClient::new(dir.join("t.loom"));
    client.create().expect("create store");
    let session = client.open().expect("open");
    let workspace = client
        .workspace_create(&session, Some("repo"), Some(FacetKind::Document))
        .expect("workspace");
    let channel_id = WorkspaceId::from_bytes([111; 16]);
    let channel_id_text = channel_id.to_string();

    block(<LocalLoomClient as Chat>::chat_create_channel_json(
        &client,
        session.clone(),
        "repo".to_string(),
        "studio".to_string(),
        channel_id_text.clone(),
        "general".to_string(),
        "General".to_string(),
        None,
    ))
    .expect("create channel");
    block(<LocalLoomClient as Chat>::chat_post_message_json(
        &client,
        session.clone(),
        "repo".to_string(),
        "studio".to_string(),
        "general".to_string(),
        "m1".to_string(),
        None,
        "hello".to_string(),
        None,
    ))
    .expect("post message");
    client
        .with_session(&session, |loom| {
            loom_chat::register_emoji(loom, workspace, "studio", "shipit", None)?;
            loom_chat::update_cursor(loom, workspace, "studio", "general", 1, None)?;
            Ok(())
        })
        .expect("seed emoji and cursor");
    client.save(&session).expect("save seed");

    let cases = [
        (
            block(<LocalLoomClient as Chat>::chat_list_channels_json(
                &client,
                session.clone(),
                "repo".to_string(),
                "studio".to_string(),
            ))
            .expect("direct list"),
            unary_text(
                dispatch(
                    &client,
                    &session,
                    "Chat",
                    "chat_list_channels_json",
                    &[Value::Null, text("repo"), text("studio")],
                )
                .expect("dispatch list"),
            ),
        ),
        (
            block(<LocalLoomClient as Chat>::chat_emoji_list_json(
                &client,
                session.clone(),
                "repo".to_string(),
                "studio".to_string(),
            ))
            .expect("direct emoji"),
            unary_text(
                dispatch(
                    &client,
                    &session,
                    "Chat",
                    "chat_emoji_list_json",
                    &[Value::Null, text("repo"), text("studio")],
                )
                .expect("dispatch emoji"),
            ),
        ),
        (
            block(<LocalLoomClient as Chat>::chat_messages_json(
                &client,
                session.clone(),
                "repo".to_string(),
                "studio".to_string(),
                "general".to_string(),
            ))
            .expect("direct messages"),
            unary_text(
                dispatch(
                    &client,
                    &session,
                    "Chat",
                    "chat_messages_json",
                    &[Value::Null, text("repo"), text("studio"), text("general")],
                )
                .expect("dispatch messages"),
            ),
        ),
        (
            block(<LocalLoomClient as Chat>::chat_cursor_json(
                &client,
                session.clone(),
                "repo".to_string(),
                "studio".to_string(),
                "general".to_string(),
            ))
            .expect("direct cursor"),
            unary_text(
                dispatch(
                    &client,
                    &session,
                    "Chat",
                    "chat_cursor_json",
                    &[Value::Null, text("repo"), text("studio"), text("general")],
                )
                .expect("dispatch cursor"),
            ),
        ),
        (
            block(<LocalLoomClient as Chat>::chat_fetch_events_json(
                &client,
                session.clone(),
                "repo".to_string(),
                "studio".to_string(),
                "general".to_string(),
                1,
                1,
            ))
            .expect("direct events"),
            unary_text(
                dispatch(
                    &client,
                    &session,
                    "Chat",
                    "chat_fetch_events_json",
                    &[
                        Value::Null,
                        text("repo"),
                        text("studio"),
                        text("general"),
                        Value::Uint(1),
                        Value::Uint(1),
                    ],
                )
                .expect("dispatch events"),
            ),
        ),
    ];
    for (direct, dispatched) in cases {
        assert_eq!(dispatched, direct);
    }

    let direct_error = block(<LocalLoomClient as Chat>::chat_fetch_events_json(
        &client,
        session.clone(),
        "repo".to_string(),
        "studio".to_string(),
        "general".to_string(),
        0,
        1,
    ))
    .expect_err("direct invalid event cursor");
    let dispatched_error = match dispatch(
        &client,
        &session,
        "Chat",
        "chat_fetch_events_json",
        &[
            Value::Null,
            text("repo"),
            text("studio"),
            text("general"),
            Value::Uint(0),
            Value::Uint(1),
        ],
    ) {
        Ok(_) => panic!("dispatch invalid event cursor must fail"),
        Err(error) => error,
    };
    assert_eq!(dispatched_error.code, direct_error.code);
    assert_eq!(dispatched_error.message, direct_error.message);
    assert_eq!(dispatched_error.details, direct_error.details);

    client.close(&session);
    std::fs::remove_dir_all(&dir).ok();
}
