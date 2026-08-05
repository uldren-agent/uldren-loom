use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use loom_core::{Algo, FacetKind, Loom, WorkspaceId, document_put_text};
use loom_lanes::{Lane, LaneTicket};
use loom_pages::PageCreateRequest;
use loom_store::FileStore;
use loom_tickets::{TicketCommentRequest, TicketCreateRequest};

struct StoreFixture {
    path: String,
}

impl StoreFixture {
    fn new(tag: &str) -> Self {
        let mut path = std::path::PathBuf::from("/private/tmp");
        path.push(format!(
            "loom-offline-cli-reads-{tag}-{}-{}.loom",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = path.to_string_lossy().into_owned();
        let _ = std::fs::remove_file(&path);
        seed_store(&path);
        Self { path }
    }
}

impl Drop for StoreFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn seed_store(path: &str) {
    let fs = FileStore::create_with_profile(path, Algo::Blake3).unwrap();
    let workspace = WorkspaceId::v4_from_bytes([96; 16]);
    let profile_id = workspace.to_string();
    let mut loom = Loom::new(fs);
    loom.registry_mut()
        .create(FacetKind::Vcs, Some("main"), workspace)
        .unwrap();
    loom.registry_mut()
        .add_facet(workspace, FacetKind::Document)
        .unwrap();
    loom.registry_mut()
        .add_facet(workspace, FacetKind::Queue)
        .unwrap();
    loom.registry_mut()
        .add_facet(workspace, FacetKind::Graph)
        .unwrap();
    document_put_text(&mut loom, workspace, "notes", "intro", "hello", None).unwrap();
    loom_tickets::create_project(
        &mut loom,
        workspace,
        &profile_id,
        "core",
        "CORE",
        "Core",
        None,
    )
    .unwrap();
    let fields = serde_json::json!({
        "title": "Read-only CLI",
        "status": "planned"
    });
    let ticket = loom_tickets::create_ticket(
        &mut loom,
        workspace,
        TicketCreateRequest {
            workspace_id: &profile_id,
            project_id: "core",
            ticket_type: "task",
            external_source: None,
            external_id: None,
            fields: &fields,
            policy_labels: &[],
            expected_root: None,
        },
    )
    .unwrap();
    loom_tickets::add_ticket_comment(
        &mut loom,
        workspace,
        TicketCommentRequest {
            workspace_id: &profile_id,
            ticket_id: &ticket.ticket_id,
            comment_id: Some("progress"),
            comment_type: Some("progress"),
            body: "Seeded for offline reads.",
            evidence: None,
            expected_root: None,
        },
    )
    .unwrap();
    loom_lanes::put_lane(
        &mut loom,
        workspace,
        Lane {
            lane_id: "agent-1".to_string(),
            lane_key: "agent-1".to_string(),
            title: "Agent 1".to_string(),
            description: String::new(),
            lane_kind: "assignment".to_string(),
            owner_principal: Some("agent:1".to_string()),
            lane_status: "ready".to_string(),
            lane_tickets: vec![LaneTicket {
                ticket_id: ticket.primary_key.clone(),
                order_key: "a".to_string(),
            }],
            active_ticket_id: Some(ticket.primary_key.clone()),
            status_report: "ready".to_string(),
            reviewer_feedback: String::new(),
            updated_at: 1,
            updated_by: "seed".to_string(),
        },
    )
    .unwrap();
    loom_pages::create_space(&mut loom, workspace, &profile_id, "docs", "Docs", None).unwrap();
    loom_pages::create_page(
        &mut loom,
        workspace,
        PageCreateRequest {
            workspace_id: &profile_id,
            page_id: "intro",
            space_id: "docs",
            parent_page_id: None,
            title: "Intro",
            expected_root: None,
        },
    )
    .unwrap();
    loom_pages::update_page_text(
        &mut loom,
        workspace,
        &profile_id,
        "intro",
        "Welcome.",
        2,
        None,
    )
    .unwrap();
    loom_pages::publish_page(&mut loom, workspace, &profile_id, "intro", 3, None).unwrap();
}

fn loom(args: &[&str]) -> Result<String, String> {
    let output = Command::new(env!("CARGO_BIN_EXE_loom"))
        .args(args)
        .output()
        .map_err(|error| format!("spawn loom: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "loom {} failed with {}\nstdout:\n{}\nstderr:\n{}",
            args.join(" "),
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[test]
fn offline_cli_reads_are_concurrent_and_do_not_mutate_store() {
    let fixture = StoreFixture::new("concurrent");
    let before_bytes = std::fs::read(&fixture.path).unwrap();
    let before_meta = std::fs::metadata(&fixture.path).unwrap();
    let before_generation = FileStore::open_read(&fixture.path)
        .unwrap()
        .mutable_overlay_generation()
        .unwrap()
        .as_u64();
    let commands: Vec<Vec<String>> = vec![
        vec!["lanes", "list", &fixture.path, "main", "--format", "json"],
        vec![
            "lanes",
            "get",
            &fixture.path,
            "main",
            "agent-1",
            "--format",
            "json",
        ],
        vec![
            "tickets",
            "project-settings-get",
            &fixture.path,
            "main",
            "core",
            "--format",
            "json",
        ],
        vec!["tickets", "list", &fixture.path, "main", "--format", "json"],
        vec![
            "tickets",
            "get",
            &fixture.path,
            "main",
            "CORE-1",
            "--format",
            "json",
        ],
        vec![
            "tickets",
            "comments",
            &fixture.path,
            "main",
            "CORE-1",
            "--format",
            "json",
        ],
        vec![
            "document",
            "get-text",
            &fixture.path,
            "main",
            "notes",
            "intro",
        ],
        vec!["document", "list-binary", &fixture.path, "main", "notes"],
        vec![
            "pages",
            "space-list",
            &fixture.path,
            "main",
            "--format",
            "json",
        ],
        vec![
            "pages",
            "space-get",
            &fixture.path,
            "main",
            "docs",
            "--format",
            "json",
        ],
        vec![
            "pages",
            "get",
            &fixture.path,
            "main",
            "intro",
            "--format",
            "json",
        ],
        vec![
            "pages",
            "history",
            &fixture.path,
            "main",
            "intro",
            "--format",
            "json",
        ],
        vec!["doctor", "store", &fixture.path],
    ]
    .into_iter()
    .map(|args| args.into_iter().map(str::to_string).collect())
    .collect();

    let mut children = Vec::new();
    for args in &commands {
        children.push((
            args.clone(),
            Command::new(env!("CARGO_BIN_EXE_loom"))
                .args(args)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        ));
    }
    for (args, child) in children {
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "loom {} failed with {}\nstdout:\n{}\nstderr:\n{}",
            args.join(" "),
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let after_generation = FileStore::open_read(&fixture.path)
        .unwrap()
        .mutable_overlay_generation()
        .unwrap()
        .as_u64();
    let after_meta = std::fs::metadata(&fixture.path).unwrap();
    assert_eq!(std::fs::read(&fixture.path).unwrap(), before_bytes);
    assert_eq!(after_meta.len(), before_meta.len());
    assert_eq!(after_generation, before_generation);
    assert_eq!(
        after_meta.modified().unwrap(),
        before_meta.modified().unwrap()
    );

    for args in commands {
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        loom(&refs).unwrap();
    }
    assert_eq!(std::fs::read(&fixture.path).unwrap(), before_bytes);
}
