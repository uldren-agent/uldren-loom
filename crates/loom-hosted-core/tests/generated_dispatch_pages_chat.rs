use loom_client::LocalLoomClient;
use loom_codec::Value;
use loom_core::{FacetKind, WorkspaceId};
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
            loom_chat::ensure_channel(loom, workspace, "studio", channel_id, "general", "General")?;
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
        ],
    )
    .expect("dispatch invoke");

    client
        .with_session(&session, |loom| {
            let channel = loom_chat::channel_projection(loom, workspace, "studio", "general")?;
            assert_eq!(channel.messages[0].body, b"edited");
            assert_eq!(channel.agent_invocations[0].source_message_ids, ["m1"]);
            assert_eq!(channel.agent_invocations[0].prompt, b"summarize");
            Ok(())
        })
        .expect("read channel");
    client.close(&session);
    std::fs::remove_dir_all(&dir).ok();
}
