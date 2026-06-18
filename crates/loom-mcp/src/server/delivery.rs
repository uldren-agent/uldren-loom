use std::collections::{HashMap, VecDeque};

use loom_core::{Code, Digest, LoomError, Result};
use serde::Serialize;
use serde_json::{Value, json};

pub const DEFAULT_RETENTION_MAX_AGE_MS: u64 = 24 * 60 * 60 * 1000;
pub const DEFAULT_RETENTION_MAX_EVENTS: usize = 10_000;
pub const DEFAULT_RETENTION_MAX_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DeliveryRetention {
    pub max_age_ms: u64,
    pub max_events_per_stream: usize,
    pub max_bytes_per_stream: u64,
}

impl Default for DeliveryRetention {
    fn default() -> Self {
        Self {
            max_age_ms: DEFAULT_RETENTION_MAX_AGE_MS,
            max_events_per_stream: DEFAULT_RETENTION_MAX_EVENTS,
            max_bytes_per_stream: DEFAULT_RETENTION_MAX_BYTES,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct DeliveryEnvelope {
    pub stream_id: String,
    pub seq: u64,
    pub id: String,
    pub producer: String,
    pub subject: String,
    pub payload_digest: String,
    pub payload_len: u64,
    pub created_at_ms: u64,
    pub expires_at_ms: Option<u64>,
    pub source_cursor: Option<String>,
    pub payload: Value,
}

#[derive(Clone, Debug, Serialize)]
pub struct DeliveryReplay {
    pub stream_id: String,
    pub subscriber_id: String,
    pub from_seq: u64,
    pub next_seq: u64,
    pub ack_seq: u64,
    pub events: Vec<DeliveryEnvelope>,
}

#[derive(Debug)]
pub struct DeliveryState {
    policy: DeliveryRetention,
    streams: HashMap<String, DeliveryStream>,
    acks: HashMap<(String, String), u64>,
}

impl Default for DeliveryState {
    fn default() -> Self {
        Self::new(DeliveryRetention::default())
    }
}

impl DeliveryState {
    pub fn new(policy: DeliveryRetention) -> Self {
        Self {
            policy,
            streams: HashMap::new(),
            acks: HashMap::new(),
        }
    }

    pub fn policy(&self) -> DeliveryRetention {
        self.policy.clone()
    }

    pub fn app_stream_id(uri: &str) -> String {
        format!("mcp-app:{uri}")
    }

    pub fn produce_app_update(
        &mut self,
        uri: &str,
        version: Option<String>,
        source_cursor: Option<String>,
        now_ms: u64,
    ) -> Result<DeliveryEnvelope> {
        let stream_id = Self::app_stream_id(uri);
        let payload = json!({
            "type": "resource.updated",
            "uri": uri,
            "version": version,
        });
        self.produce(
            stream_id,
            "loom-mcp".to_string(),
            uri.to_string(),
            payload,
            source_cursor,
            now_ms,
        )
    }

    pub fn ack(&mut self, stream_id: &str, subscriber_id: &str, seq: u64) -> u64 {
        let key = (stream_id.to_string(), subscriber_id.to_string());
        let entry = self.acks.entry(key).or_insert(0);
        *entry = (*entry).max(seq);
        *entry
    }

    pub fn replay(
        &mut self,
        stream_id: &str,
        subscriber_id: &str,
        from_seq: Option<u64>,
        resume_from_ack: bool,
        limit: usize,
        now_ms: u64,
    ) -> Result<DeliveryReplay> {
        self.enforce_stream(stream_id, now_ms);
        let ack_seq = self.ack_seq(stream_id, subscriber_id);
        let start = if resume_from_ack {
            ack_seq.saturating_add(1)
        } else {
            from_seq.unwrap_or(1)
        };
        let Some(stream) = self.streams.get(stream_id) else {
            return Ok(DeliveryReplay {
                stream_id: stream_id.to_string(),
                subscriber_id: subscriber_id.to_string(),
                from_seq: start,
                next_seq: start,
                ack_seq,
                events: Vec::new(),
            });
        };
        if start < stream.floor_seq {
            return Err(LoomError::new(
                Code::CursorInvalid,
                format!(
                    "delivery retention expired before sequence {} on stream {}",
                    stream.floor_seq, stream_id
                ),
            ));
        }
        let events = stream
            .envelopes
            .iter()
            .filter(|event| event.seq >= start)
            .take(limit)
            .cloned()
            .collect::<Vec<_>>();
        let next_seq = events
            .last()
            .map_or(start, |event| event.seq.saturating_add(1));
        Ok(DeliveryReplay {
            stream_id: stream_id.to_string(),
            subscriber_id: subscriber_id.to_string(),
            from_seq: start,
            next_seq,
            ack_seq,
            events,
        })
    }

    fn produce(
        &mut self,
        stream_id: String,
        producer: String,
        subject: String,
        payload: Value,
        source_cursor: Option<String>,
        now_ms: u64,
    ) -> Result<DeliveryEnvelope> {
        self.enforce_stream(&stream_id, now_ms);
        let payload_bytes = serde_json::to_vec(&payload)
            .map_err(|e| LoomError::invalid(format!("delivery payload encoding failed: {e}")))?;
        let payload_digest = Digest::blake3(&payload_bytes).to_string();
        let payload_len = payload_bytes.len() as u64;
        let expires_at_ms = now_ms.checked_add(self.policy.max_age_ms);
        let stream = self.streams.entry(stream_id.clone()).or_default();
        let seq = stream.next_seq;
        stream.next_seq = stream.next_seq.saturating_add(1);
        let id_input = DeliveryIdInput {
            stream_id: &stream_id,
            seq,
            producer: &producer,
            subject: &subject,
            payload_digest: &payload_digest,
            created_at_ms: now_ms,
            expires_at_ms,
            source_cursor: source_cursor.as_deref(),
        };
        let id_bytes = serde_json::to_vec(&id_input).map_err(|e| {
            LoomError::invalid(format!("delivery envelope id encoding failed: {e}"))
        })?;
        let envelope = DeliveryEnvelope {
            stream_id,
            seq,
            id: Digest::blake3(&id_bytes).to_string(),
            producer,
            subject,
            payload_digest,
            payload_len,
            created_at_ms: now_ms,
            expires_at_ms,
            source_cursor,
            payload,
        };
        stream.retained_bytes = stream.retained_bytes.saturating_add(envelope.payload_len);
        stream.envelopes.push_back(envelope.clone());
        Self::enforce_retention(stream, &self.policy, now_ms);
        Ok(envelope)
    }

    fn ack_seq(&self, stream_id: &str, subscriber_id: &str) -> u64 {
        self.acks
            .get(&(stream_id.to_string(), subscriber_id.to_string()))
            .copied()
            .unwrap_or(0)
    }

    fn enforce_stream(&mut self, stream_id: &str, now_ms: u64) {
        if let Some(stream) = self.streams.get_mut(stream_id) {
            Self::enforce_retention(stream, &self.policy, now_ms);
        }
    }

    fn enforce_retention(stream: &mut DeliveryStream, policy: &DeliveryRetention, now_ms: u64) {
        while stream
            .envelopes
            .front()
            .and_then(|event| event.expires_at_ms.map(|expires| expires <= now_ms))
            .unwrap_or(false)
        {
            stream.pop_front();
        }
        while stream.envelopes.len() > policy.max_events_per_stream {
            stream.pop_front();
        }
        while stream.retained_bytes > policy.max_bytes_per_stream {
            stream.pop_front();
        }
    }
}

#[derive(Debug)]
struct DeliveryStream {
    next_seq: u64,
    floor_seq: u64,
    retained_bytes: u64,
    envelopes: VecDeque<DeliveryEnvelope>,
}

impl Default for DeliveryStream {
    fn default() -> Self {
        Self {
            next_seq: 1,
            floor_seq: 1,
            retained_bytes: 0,
            envelopes: VecDeque::new(),
        }
    }
}

impl DeliveryStream {
    fn pop_front(&mut self) {
        if let Some(event) = self.envelopes.pop_front() {
            self.retained_bytes = self.retained_bytes.saturating_sub(event.payload_len);
            self.floor_seq = event.seq.saturating_add(1);
        }
    }
}

#[derive(Serialize)]
struct DeliveryIdInput<'a> {
    stream_id: &'a str,
    seq: u64,
    producer: &'a str,
    subject: &'a str,
    payload_digest: &'a str,
    created_at_ms: u64,
    expires_at_ms: Option<u64>,
    source_cursor: Option<&'a str>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policy_uses_conservative_limits() {
        assert_eq!(
            DeliveryRetention::default(),
            DeliveryRetention {
                max_age_ms: DEFAULT_RETENTION_MAX_AGE_MS,
                max_events_per_stream: DEFAULT_RETENTION_MAX_EVENTS,
                max_bytes_per_stream: DEFAULT_RETENTION_MAX_BYTES,
            }
        );
    }

    #[test]
    fn replay_redelivers_until_ack_advances() {
        let mut state = DeliveryState::default();
        let stream_id = DeliveryState::app_stream_id("ui://repo/mcp/apps/panel");
        let event = state
            .produce_app_update(
                "ui://repo/mcp/apps/panel",
                Some("v1".to_string()),
                Some("watch:1".to_string()),
                1,
            )
            .unwrap();
        let replay = state
            .replay(&stream_id, "client", None, true, 10, 2)
            .unwrap();
        assert_eq!(replay.events.len(), 1);
        assert_eq!(replay.events[0].id, event.id);

        let redelivery = state
            .replay(&stream_id, "client", None, true, 10, 3)
            .unwrap();
        assert_eq!(redelivery.events.len(), 1);
        assert_eq!(redelivery.events[0].id, event.id);

        assert_eq!(state.ack(&stream_id, "client", event.seq), event.seq);
        let replay = state
            .replay(&stream_id, "client", None, true, 10, 4)
            .unwrap();
        assert!(replay.events.is_empty());
        assert_eq!(replay.from_seq, 2);
    }

    #[test]
    fn retention_overflow_invalidates_old_sequence() {
        let mut state = DeliveryState::new(DeliveryRetention {
            max_age_ms: DEFAULT_RETENTION_MAX_AGE_MS,
            max_events_per_stream: 1,
            max_bytes_per_stream: DEFAULT_RETENTION_MAX_BYTES,
        });
        let stream_id = DeliveryState::app_stream_id("ui://repo/mcp/apps/panel");
        state
            .produce_app_update("ui://repo/mcp/apps/panel", Some("v1".to_string()), None, 1)
            .unwrap();
        state
            .produce_app_update("ui://repo/mcp/apps/panel", Some("v2".to_string()), None, 2)
            .unwrap();
        let err = state
            .replay(&stream_id, "client", Some(1), false, 10, 3)
            .unwrap_err();
        assert_eq!(err.code, Code::CursorInvalid);
    }
}
