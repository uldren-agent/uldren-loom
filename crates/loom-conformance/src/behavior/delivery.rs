//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

pub fn run_delivery_behavior<S: ObjectStore>(loom: &mut Loom<S>) -> Result<()> {
    let ns =
        loom.registry_mut()
            .create(FacetKind::Queue, None, WorkspaceId::from_bytes([71; 16]))?;
    let first = delivery_produce(
        loom,
        ns,
        DeliveryProduceRequest {
            stream_id: "events",
            producer: "watch",
            subject: "client-a",
            payload: b"payload-1",
            created_at_ms: 10,
            expires_at_ms: None,
            source_cursor: Some(b"watch-cursor-1"),
        },
    )?;
    let second = delivery_produce(
        loom,
        ns,
        DeliveryProduceRequest {
            stream_id: "events",
            producer: "trigger",
            subject: "client-a",
            payload: b"payload-2",
            created_at_ms: 11,
            expires_at_ms: Some(100),
            source_cursor: None,
        },
    )?;

    let replay = delivery_replay(loom, ns, "events", "client-a", None, true, 10)?;
    assert_eq!(replay.stream_id, "events");
    assert_eq!(replay.subscriber_id, "client-a");
    assert_eq!(replay.next_seq, 2);
    assert_eq!(replay.messages.len(), 2);
    assert_eq!(replay.messages[0].envelope.id, first.id);
    assert_eq!(replay.messages[0].envelope.seq, 0);
    assert_eq!(
        replay.messages[0].envelope.source_cursor,
        Some(b"watch-cursor-1".to_vec())
    );
    assert_eq!(replay.messages[0].payload, b"payload-1");
    assert_eq!(replay.messages[1].envelope.id, second.id);
    assert_eq!(replay.messages[1].envelope.expires_at_ms, Some(100));
    assert_eq!(replay.messages[1].payload, b"payload-2");

    let redelivery = delivery_replay(loom, ns, "events", "client-a", None, true, 10)?;
    assert_eq!(
        redelivery.messages[0].envelope.id, first.id,
        "unacked delivery redelivers with the same message id"
    );

    assert_eq!(delivery_ack(loom, ns, "events", "client-a", first.seq)?, 1);
    assert_eq!(delivery_ack_position(loom, ns, "events", "client-a")?, 1);
    let after_ack = delivery_replay(loom, ns, "events", "client-a", None, true, 10)?;
    assert_eq!(after_ack.messages.len(), 1);
    assert_eq!(after_ack.messages[0].envelope.id, second.id);
    delivery_set_retained_low_water_mark(loom, ns, "events", 1)?;
    assert_eq!(
        delivery_replay(loom, ns, "events", "client-a", Some(0), false, 10)
            .unwrap_err()
            .code,
        Code::RetainedGap,
        "delivery replay reports RETAINED_GAP before the retained mark"
    );
    let changes = delivery_change_set(loom, ns, "events", "client-a", Some(1), false, 10)?;
    assert_eq!(changes.retained_low_water_mark, Some(1));
    assert_eq!(changes.items.len(), 1);
    assert_eq!(changes.items[0].sequence, Some(second.seq));

    let root = WorkspaceId::from_bytes([72; 16]);
    let mut identity = IdentityStore::new(root);
    identity.set_passphrase(root, "root", b"12345678")?;
    let session = identity.authenticate_passphrase(root, "root", "delivery")?;
    loom.set_identity_store(identity);
    loom.set_session(session.id);

    assert_eq!(
        delivery_produce(
            loom,
            ns,
            DeliveryProduceRequest {
                stream_id: "secure",
                producer: "watch",
                subject: "client-a",
                payload: b"payload",
                created_at_ms: 12,
                expires_at_ms: None,
                source_cursor: None,
            },
        )
        .unwrap_err()
        .code,
        Code::PermissionDenied,
        "delivery produce fails closed before enqueue"
    );

    loom.acl_store_mut().allow(
        AclSubject::Principal(root),
        Some(ns),
        Some(FacetKind::Queue),
        [AclRight::Read, AclRight::Write, AclRight::Advance],
    )?;
    let secured = delivery_produce(
        loom,
        ns,
        DeliveryProduceRequest {
            stream_id: "secure",
            producer: "watch",
            subject: "client-a",
            payload: b"payload",
            created_at_ms: 13,
            expires_at_ms: None,
            source_cursor: None,
        },
    )?;
    let secured_replay = delivery_replay(loom, ns, "secure", "client-a", None, true, 10)?;
    assert_eq!(secured_replay.messages[0].envelope.id, secured.id);
    assert_eq!(
        delivery_ack(loom, ns, "secure", "client-a", secured.seq)?,
        1
    );

    loom.acl_store_mut().deny(
        AclSubject::Principal(root),
        Some(ns),
        Some(FacetKind::Queue),
        [AclRight::Read],
    )?;
    assert_eq!(
        delivery_replay(loom, ns, "secure", "client-a", None, true, 10)
            .unwrap_err()
            .code,
        Code::PermissionDenied,
        "delivery replay fails closed before payload egress"
    );
    Ok(())
}
