use super::*;
use loom_core::{
    AclDomain, AclEffect, AclGrant, AclRight, AclScope, AclSubject, FacetKind, IdentityStore,
    LoomError,
};
use loom_store::{FileStore, MemoryBacking};
use loom_tickets::{
    TicketCommentDeleteRequest, TicketCommentRequest, TicketCommentUpdateRequest,
    TicketCreateRequest, TicketUpdateCommentRequest, TicketUpdateRequest, add_ticket_comment,
    create_project, create_ticket, delete_ticket_comment, get_ticket, history,
    list_ticket_comments, update_ticket, update_ticket_comment,
};

fn ticket_pid(seed: u8) -> WorkspaceId {
    WorkspaceId::from_bytes([seed; 16])
}

fn ticket_conformance_change_stream(workspace_id: &str) -> String {
    format!("tickets:{workspace_id}:changes")
}

fn ticket_conformance_loom() -> Result<(Loom<FileStore>, WorkspaceId, WorkspaceId)> {
    let namespace = ticket_pid(41);
    let admin = ticket_pid(42);
    let mut loom = Loom::new(FileStore::with_backing(
        Box::new(MemoryBacking::new()),
        true,
    )?);
    let mut identity = IdentityStore::new(admin);
    identity.set_passphrase(admin, "admin", b"12345678")?;
    let admin_session = identity.authenticate_passphrase(admin, "admin", "admin-session")?;
    loom.set_identity_store(identity);
    loom.set_session(admin_session.id);
    loom.acl_store_mut().allow(
        AclSubject::Principal(admin),
        Some(namespace),
        None,
        [AclRight::Admin],
    )?;
    loom.acl_store_mut().grant(AclGrant {
        subject: AclSubject::Principal(admin),
        workspace: Some(namespace),
        domain: Some(AclDomain::Tickets),
        ref_glob: None,
        scopes: vec![AclScope::All],
        rights: [AclRight::Read, AclRight::Write, AclRight::Admin]
            .into_iter()
            .collect(),
        effect: AclEffect::Allow,
        predicate: None,
    })?;
    loom.acl_store_mut().allow(
        AclSubject::Principal(admin),
        Some(namespace),
        Some(FacetKind::Vcs),
        [AclRight::Admin],
    )?;
    loom.acl_store_mut().allow(
        AclSubject::Principal(admin),
        Some(namespace),
        Some(FacetKind::Graph),
        [AclRight::Admin],
    )?;
    loom.acl_store_mut().allow(
        AclSubject::Principal(admin),
        Some(namespace),
        Some(FacetKind::Queue),
        [AclRight::Read, AclRight::Write, AclRight::Advance],
    )?;
    Ok((loom, namespace, admin))
}

pub fn run_ticket_comment_behavior() -> Result<()> {
    let (mut loom, namespace, admin) = ticket_conformance_loom()?;
    let workspace_id = namespace.to_string();
    create_project(
        &mut loom,
        namespace,
        &workspace_id,
        "matrix",
        "MX",
        "Matrix",
        None,
    )?;
    let ticket = create_ticket(
        &mut loom,
        namespace,
        TicketCreateRequest {
            workspace_id: &workspace_id,
            project_id: "matrix",
            ticket_type: "task",
            external_source: None,
            external_id: None,
            fields: &serde_json::json!({"status": "open", "title": "Conformance"}),
            policy_labels: &[],
            expected_root: None,
        },
    )?;

    let added = add_ticket_comment(
        &mut loom,
        namespace,
        TicketCommentRequest {
            workspace_id: &workspace_id,
            ticket_id: &ticket.ticket_id,
            comment_id: Some("review"),
            comment_type: Some("review_request"),
            body: "Ready for review",
            evidence: None,
            expected_root: Some(&ticket.profile_root),
        },
    )?;
    assert_eq!(added.comments.len(), 1);
    assert_eq!(added.comments[0].comment_id, "review");
    assert_eq!(added.comments[0].comment_type, "review_request");
    assert_eq!(added.comments[0].author_principal, admin.to_string());
    assert!(!added.comments[0].redacted);

    let listed = list_ticket_comments(&loom, namespace, &workspace_id, &ticket.ticket_id)?;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].body, "Ready for review");
    assert_eq!(listed[0].created_at_ms, added.comments[0].created_at_ms);
    assert!(listed[0].updated_at_ms.is_none());

    let summary = get_ticket(&loom, namespace, &workspace_id, &ticket.ticket_id)?
        .ok_or_else(|| LoomError::not_found("ticket missing after comment add"))?;
    assert_eq!(summary.comments.len(), 1);
    assert_eq!(summary.comments[0].comment_id, "review");
    assert_eq!(summary.comments[0].comment_type, "review_request");
    assert_eq!(summary.comments[0].author_principal, admin.to_string());
    assert!(!summary.comments[0].redacted);

    update_ticket_comment(
        &mut loom,
        namespace,
        TicketCommentUpdateRequest {
            workspace_id: &workspace_id,
            ticket_id: &ticket.ticket_id,
            comment_id: "review",
            comment_type: Some("review_feedback"),
            body: Some("Needs evidence"),
            evidence: None,
            expected_root: Some(&added.profile_root),
        },
    )?;
    let updated = list_ticket_comments(&loom, namespace, &workspace_id, &ticket.ticket_id)?;
    assert_eq!(updated.len(), 1);
    assert_eq!(updated[0].comment_type, "review_feedback");
    assert_eq!(updated[0].body, "Needs evidence");
    assert_eq!(updated[0].author_principal, admin.to_string());
    assert_eq!(updated[0].created_at_ms, listed[0].created_at_ms);
    assert!(updated[0].updated_at_ms.is_some());

    delete_ticket_comment(
        &mut loom,
        namespace,
        TicketCommentDeleteRequest {
            workspace_id: &workspace_id,
            ticket_id: &ticket.ticket_id,
            comment_id: "review",
            expected_root: None,
        },
    )?;
    let deleted = list_ticket_comments(&loom, namespace, &workspace_id, &ticket.ticket_id)?;
    assert_eq!(deleted.len(), 1);
    assert!(deleted[0].redacted);
    assert!(deleted[0].body.is_empty());
    assert_eq!(deleted[0].comment_type, "review_feedback");

    let history = history(&loom, namespace, &workspace_id, Some(&ticket.ticket_id))?;
    assert!(
        history
            .iter()
            .any(|record| record.operation_kind == "ticket.comment_added")
    );
    assert!(
        history
            .iter()
            .any(|record| record.operation_kind == "ticket.comment_updated")
    );
    assert!(
        history
            .iter()
            .any(|record| record.operation_kind == "ticket.comment_deleted")
    );

    let atomic_ticket = create_ticket(
        &mut loom,
        namespace,
        TicketCreateRequest {
            workspace_id: &workspace_id,
            project_id: "matrix",
            ticket_type: "task",
            external_source: None,
            external_id: None,
            fields: &serde_json::json!({"status": "planned", "title": "Atomic"}),
            policy_labels: &[],
            expected_root: None,
        },
    )?;
    let atomic = update_ticket(
        &mut loom,
        namespace,
        TicketUpdateRequest {
            workspace_id: &workspace_id,
            ticket_id: &atomic_ticket.ticket_id,
            set_fields: None,
            delete_fields: &[],
            action: None,
            target_status: Some("in_progress"),
            observed_source_status: Some("planned"),
            observed_workflow_version: None,
            assignee: None,
            expected_root: Some(&atomic_ticket.profile_root),
            comment: Some(TicketUpdateCommentRequest {
                comment_id: Some("atomic-progress"),
                comment_type: Some("progress"),
                body: "Status and evidence moved together",
                evidence: None,
            }),
            comments: &[],
            relation_sets: &[],
            relation_removes: &[],
        },
    )?;
    assert_eq!(atomic.fields["status"], "in_progress");
    assert_eq!(atomic.comments.len(), 1);
    assert_eq!(atomic.comments[0].comment_id, "atomic-progress");
    let atomic_comments =
        list_ticket_comments(&loom, namespace, &workspace_id, &atomic_ticket.ticket_id)?;
    assert_eq!(atomic_comments.len(), 1);
    assert_eq!(
        atomic_comments[0].body,
        "Status and evidence moved together"
    );
    assert_eq!(atomic_comments[0].comment_type, "progress");

    let replay = delivery_replay(
        &loom,
        namespace,
        &ticket_conformance_change_stream(&workspace_id),
        "ticket-comment-conformance",
        None,
        false,
        8,
    )?;
    assert!(replay.messages.iter().any(|message| {
        message.envelope.subject == format!("ticket:{}", ticket.ticket_id)
            && serde_json::from_slice::<serde_json::Value>(&message.payload)
                .expect("ticket event payload must be JSON")["operation_kind"]
                == "ticket.comment_added"
    }));
    Ok(())
}
