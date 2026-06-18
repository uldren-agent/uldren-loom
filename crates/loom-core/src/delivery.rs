//! Generic durable delivery over structured append-log streams.

use crate::AclRight;
use crate::cas::{cas_get_unchecked, cas_put_unchecked};
use crate::change_set::{ChangeCursor, ChangeGapState, ChangeItem, ChangeSet};
use crate::error::{Code, LoomError, Result};
use crate::log;
use crate::provider::ObjectStore;
use crate::vcs::Loom;
use crate::workspace::{FacetKind, WorkspaceId};
pub use loom_delivery::{
    DeliveryEnvelope, DeliveryMessage, DeliveryProduceRequest, DeliveryReplay, decode_envelope,
    encode_envelope,
};

pub fn delivery_produce<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    request: DeliveryProduceRequest<'_>,
) -> Result<DeliveryEnvelope> {
    let DeliveryProduceRequest {
        stream_id,
        producer,
        subject,
        payload,
        created_at_ms,
        expires_at_ms,
        source_cursor,
    } = request;
    loom.authorize_collection(ns, FacetKind::Queue, stream_id, AclRight::Read)?;
    let seq = match log::len(loom, ns, stream_id) {
        Ok(len) => len as u64,
        Err(err) if err.code == Code::NotFound => 0,
        Err(err) => return Err(err),
    };
    let payload_digest = cas_put_unchecked(loom, ns, payload)?;
    let payload_len = payload.len() as u64;
    let envelope = DeliveryEnvelope::new(
        loom.store().digest_algo(),
        stream_id,
        seq,
        producer,
        subject,
        payload_digest,
        payload_len,
        created_at_ms,
        expires_at_ms,
        source_cursor,
    )?;
    let written = log::append(loom, ns, stream_id, &encode_envelope(&envelope)?)?;
    if written as u64 != seq {
        return Err(LoomError::new(
            Code::Conflict,
            "delivery stream advanced while producing envelope",
        ));
    }
    Ok(envelope)
}

pub fn delivery_ack<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    stream_id: &str,
    subscriber_id: &str,
    ack_seq: u64,
) -> Result<u64> {
    let next_seq = ack_seq
        .checked_add(1)
        .ok_or_else(|| LoomError::invalid("delivery ack sequence overflow"))?;
    log::consumer_advance(loom, ns, stream_id, subscriber_id, next_seq)?;
    Ok(next_seq)
}

pub fn delivery_ack_position<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    stream_id: &str,
    subscriber_id: &str,
) -> Result<u64> {
    log::consumer_position(loom, ns, stream_id, subscriber_id)
}

pub fn delivery_replay<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    stream_id: &str,
    subscriber_id: &str,
    from_seq: Option<u64>,
    resume_from_ack: bool,
    limit: usize,
) -> Result<DeliveryReplay> {
    if limit == 0 {
        return Err(LoomError::invalid(
            "delivery replay limit must be greater than zero",
        ));
    }
    let start = if resume_from_ack {
        log::consumer_position(loom, ns, stream_id, subscriber_id)?
    } else {
        from_seq.unwrap_or(0)
    };
    let low_water = log::retained_low_water_mark(loom, ns, stream_id)?;
    if start < low_water {
        return Err(LoomError::retained_gap(format!(
            "delivery stream {stream_id:?} cursor {start} predates retained low-water mark {low_water}"
        )));
    }
    let lo = usize::try_from(start).map_err(|_| LoomError::invalid("delivery cursor too large"))?;
    let hi = lo.saturating_add(limit);
    let mut messages = Vec::new();
    for bytes in log::range(loom, ns, stream_id, lo, hi)? {
        let envelope = decode_envelope(&bytes)?;
        if envelope.stream_id != stream_id || envelope.seq < start {
            return Err(LoomError::corrupt(
                "delivery envelope does not match stream",
            ));
        }
        let payload = cas_get_unchecked(loom, ns, &envelope.payload_digest)?.ok_or_else(|| {
            LoomError::corrupt("delivery payload digest is not present in content storage")
        })?;
        if payload.len() as u64 != envelope.payload_len {
            return Err(LoomError::integrity_failure(
                "delivery payload length does not match envelope",
            ));
        }
        messages.push(DeliveryMessage { envelope, payload });
    }
    let next_seq = messages
        .last()
        .map_or(start, |message| message.envelope.seq.saturating_add(1));
    Ok(DeliveryReplay {
        stream_id: stream_id.to_string(),
        subscriber_id: subscriber_id.to_string(),
        next_seq,
        messages,
    })
}

pub fn delivery_set_retained_low_water_mark<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    stream_id: &str,
    mark: u64,
) -> Result<()> {
    log::set_retained_low_water_mark(loom, ns, stream_id, mark)
}

pub fn delivery_change_set<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    stream_id: &str,
    subscriber_id: &str,
    from_seq: Option<u64>,
    resume_from_ack: bool,
    limit: usize,
) -> Result<ChangeSet> {
    let replay = delivery_replay(
        loom,
        ns,
        stream_id,
        subscriber_id,
        from_seq,
        resume_from_ack,
        limit,
    )?;
    let scope = delivery_change_scope(ns, stream_id, subscriber_id);
    let items = replay
        .messages
        .into_iter()
        .map(|message| {
            Ok(ChangeItem::sequence_record(
                message.envelope.seq,
                encode_envelope(&message.envelope)?,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    ChangeSet::new(
        scope.clone(),
        ChangeGapState::Retained,
        Some(log::retained_low_water_mark(loom, ns, stream_id)?),
        ChangeCursor::sequence(scope, replay.next_seq),
        items,
    )
}

pub fn delivery_change_scope(ns: WorkspaceId, stream_id: &str, subscriber_id: &str) -> String {
    format!(
        "delivery:{}:{stream_id}:{subscriber_id}",
        hex::encode(ns.as_bytes())
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AclRight, AclSubject, FacetKind, IdentityStore, MemoryStore, PrincipalKind,
        ROLE_SERVICE_ID, WorkspaceId,
    };

    fn nid(seed: u8) -> WorkspaceId {
        WorkspaceId::from_bytes([seed; 16])
    }

    fn queue_ns(loom: &mut Loom<MemoryStore>) -> WorkspaceId {
        loom.registry_mut()
            .create(FacetKind::Queue, None, nid(7))
            .unwrap()
    }

    #[test]
    fn delivery_replays_until_ack_advances() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = queue_ns(&mut loom);
        let first = delivery_produce(
            &mut loom,
            ns,
            DeliveryProduceRequest {
                stream_id: "apps",
                producer: "watch",
                subject: "client",
                payload: b"payload-1",
                created_at_ms: 10,
                expires_at_ms: None,
                source_cursor: Some(b"cursor-1"),
            },
        )
        .unwrap();
        let second = delivery_produce(
            &mut loom,
            ns,
            DeliveryProduceRequest {
                stream_id: "apps",
                producer: "watch",
                subject: "client",
                payload: b"payload-2",
                created_at_ms: 11,
                expires_at_ms: None,
                source_cursor: None,
            },
        )
        .unwrap();

        let replay = delivery_replay(&loom, ns, "apps", "client", None, true, 10).unwrap();
        assert_eq!(replay.messages.len(), 2);
        assert_eq!(replay.messages[0].envelope.id, first.id);
        assert_eq!(replay.messages[0].payload, b"payload-1");
        assert_eq!(replay.messages[1].envelope.id, second.id);

        let redelivery = delivery_replay(&loom, ns, "apps", "client", None, true, 10).unwrap();
        assert_eq!(redelivery.messages[0].envelope.id, first.id);

        assert_eq!(
            delivery_ack(&mut loom, ns, "apps", "client", first.seq).unwrap(),
            1
        );
        let after_ack = delivery_replay(&loom, ns, "apps", "client", None, true, 10).unwrap();
        assert_eq!(after_ack.messages.len(), 1);
        assert_eq!(after_ack.messages[0].envelope.id, second.id);
        assert_eq!(
            delivery_ack_position(&loom, ns, "apps", "client").unwrap(),
            1
        );
    }

    #[test]
    fn delivery_retained_low_water_mark_reports_gap_and_changeset() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = queue_ns(&mut loom);
        delivery_produce(
            &mut loom,
            ns,
            DeliveryProduceRequest {
                stream_id: "apps",
                producer: "watch",
                subject: "client",
                payload: b"payload-1",
                created_at_ms: 10,
                expires_at_ms: None,
                source_cursor: None,
            },
        )
        .unwrap();
        let second = delivery_produce(
            &mut loom,
            ns,
            DeliveryProduceRequest {
                stream_id: "apps",
                producer: "watch",
                subject: "client",
                payload: b"payload-2",
                created_at_ms: 11,
                expires_at_ms: None,
                source_cursor: None,
            },
        )
        .unwrap();

        delivery_set_retained_low_water_mark(&mut loom, ns, "apps", 1).unwrap();

        assert_eq!(
            delivery_replay(&loom, ns, "apps", "client", Some(0), false, 10)
                .unwrap_err()
                .code,
            Code::RetainedGap
        );
        let changes = delivery_change_set(&loom, ns, "apps", "client", Some(1), false, 10).unwrap();
        assert_eq!(changes.gap_state, ChangeGapState::Retained);
        assert_eq!(changes.retained_low_water_mark, Some(1));
        assert_eq!(changes.items.len(), 1);
        assert_eq!(changes.items[0].sequence, Some(second.seq));
    }

    #[test]
    fn delivery_authorization_is_checked_before_enqueue_replay_and_ack() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = queue_ns(&mut loom);
        let root = nid(1);
        let service = nid(2);
        let mut identity = IdentityStore::new(root);
        identity.set_passphrase(root, "root", b"12345678").unwrap();
        let session = identity
            .authenticate_passphrase(root, "root", "root-session")
            .unwrap();
        identity
            .add_principal(service, "svc", PrincipalKind::Service)
            .unwrap();
        identity.assign_role(service, ROLE_SERVICE_ID).unwrap();
        loom.set_identity_store(identity);
        loom.set_session(session.id);

        let denied = delivery_produce(
            &mut loom,
            ns,
            DeliveryProduceRequest {
                stream_id: "apps",
                producer: "watch",
                subject: "client",
                payload: b"p",
                created_at_ms: 10,
                expires_at_ms: None,
                source_cursor: None,
            },
        )
        .unwrap_err();
        assert_eq!(denied.code, Code::PermissionDenied);

        loom.acl_store_mut()
            .allow(
                AclSubject::Principal(root),
                Some(ns),
                Some(FacetKind::Queue),
                [AclRight::Write, AclRight::Read, AclRight::Advance],
            )
            .unwrap();
        delivery_produce(
            &mut loom,
            ns,
            DeliveryProduceRequest {
                stream_id: "apps",
                producer: "watch",
                subject: "client",
                payload: b"p",
                created_at_ms: 10,
                expires_at_ms: None,
                source_cursor: None,
            },
        )
        .unwrap();
        delivery_replay(&loom, ns, "apps", "client", None, true, 1).unwrap();
        delivery_ack(&mut loom, ns, "apps", "client", 0).unwrap();

        loom.acl_store_mut()
            .deny(
                AclSubject::Principal(root),
                Some(ns),
                Some(FacetKind::Queue),
                [AclRight::Read],
            )
            .unwrap();
        assert_eq!(
            delivery_replay(&loom, ns, "apps", "client", None, true, 1)
                .unwrap_err()
                .code,
            Code::PermissionDenied
        );
    }
}
