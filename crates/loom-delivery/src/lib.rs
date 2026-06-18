//! Reusable durable delivery contracts.

use loom_codec::Value;
use loom_types::{Algo, Digest, LoomError, Result};

pub const DELIVERY_ENVELOPE_SCHEMA: &str = "loom.delivery.envelope.v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveryEnvelope {
    pub stream_id: String,
    pub seq: u64,
    pub id: Digest,
    pub producer: String,
    pub subject: String,
    pub payload_digest: Digest,
    pub payload_len: u64,
    pub created_at_ms: u64,
    pub expires_at_ms: Option<u64>,
    pub source_cursor: Option<Vec<u8>>,
}

impl DeliveryEnvelope {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        algo: Algo,
        stream_id: &str,
        seq: u64,
        producer: &str,
        subject: &str,
        payload_digest: Digest,
        payload_len: u64,
        created_at_ms: u64,
        expires_at_ms: Option<u64>,
        source_cursor: Option<&[u8]>,
    ) -> Result<Self> {
        validate_delivery_text("stream_id", stream_id)?;
        validate_delivery_text("producer", producer)?;
        validate_delivery_text("subject", subject)?;
        let id = delivery_id(
            algo,
            DeliveryIdInput {
                stream_id,
                seq,
                producer,
                subject,
                payload_digest,
                payload_len,
                created_at_ms,
                expires_at_ms,
                source_cursor,
            },
        );
        Ok(Self {
            stream_id: stream_id.to_string(),
            seq,
            id,
            producer: producer.to_string(),
            subject: subject.to_string(),
            payload_digest,
            payload_len,
            created_at_ms,
            expires_at_ms,
            source_cursor: source_cursor.map(<[u8]>::to_vec),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveryMessage {
    pub envelope: DeliveryEnvelope,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveryReplay {
    pub stream_id: String,
    pub subscriber_id: String,
    pub next_seq: u64,
    pub messages: Vec<DeliveryMessage>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeliveryProduceRequest<'a> {
    pub stream_id: &'a str,
    pub producer: &'a str,
    pub subject: &'a str,
    pub payload: &'a [u8],
    pub created_at_ms: u64,
    pub expires_at_ms: Option<u64>,
    pub source_cursor: Option<&'a [u8]>,
}

struct DeliveryIdInput<'a> {
    stream_id: &'a str,
    seq: u64,
    producer: &'a str,
    subject: &'a str,
    payload_digest: Digest,
    payload_len: u64,
    created_at_ms: u64,
    expires_at_ms: Option<u64>,
    source_cursor: Option<&'a [u8]>,
}

fn delivery_id(algo: Algo, input: DeliveryIdInput<'_>) -> Digest {
    Digest::hash(algo, &encode_value(&delivery_id_value(input)))
}

fn delivery_id_value(input: DeliveryIdInput<'_>) -> Value {
    Value::Array(vec![
        Value::Text(DELIVERY_ENVELOPE_SCHEMA.to_string()),
        Value::Text(input.stream_id.to_string()),
        Value::Uint(input.seq),
        Value::Text(input.producer.to_string()),
        Value::Text(input.subject.to_string()),
        Value::Text(input.payload_digest.to_string()),
        Value::Uint(input.payload_len),
        Value::Uint(input.created_at_ms),
        optional_u64(input.expires_at_ms),
        optional_bytes(input.source_cursor),
    ])
}

pub fn encode_envelope(envelope: &DeliveryEnvelope) -> Result<Vec<u8>> {
    Ok(encode_value(&Value::Array(vec![
        Value::Text(DELIVERY_ENVELOPE_SCHEMA.to_string()),
        Value::Text(envelope.stream_id.clone()),
        Value::Uint(envelope.seq),
        Value::Text(envelope.id.to_string()),
        Value::Text(envelope.producer.clone()),
        Value::Text(envelope.subject.clone()),
        Value::Text(envelope.payload_digest.to_string()),
        Value::Uint(envelope.payload_len),
        Value::Uint(envelope.created_at_ms),
        optional_u64(envelope.expires_at_ms),
        optional_bytes(envelope.source_cursor.as_deref()),
    ])))
}

pub fn decode_envelope(bytes: &[u8]) -> Result<DeliveryEnvelope> {
    let mut fields =
        take_array(loom_codec::decode(bytes).map_err(|err| {
            LoomError::corrupt(format!("invalid delivery envelope CBOR: {err}"))
        })?)?
        .into_iter();
    let schema = take_text(next_field(&mut fields)?)?;
    if schema != DELIVERY_ENVELOPE_SCHEMA {
        return Err(LoomError::corrupt("unknown delivery envelope schema"));
    }
    let stream_id = take_text(next_field(&mut fields)?)?;
    let seq = take_u64(next_field(&mut fields)?)?;
    let id = parse_digest(&take_text(next_field(&mut fields)?)?)?;
    let producer = take_text(next_field(&mut fields)?)?;
    let subject = take_text(next_field(&mut fields)?)?;
    let payload_digest = parse_digest(&take_text(next_field(&mut fields)?)?)?;
    let payload_len = take_u64(next_field(&mut fields)?)?;
    let created_at_ms = take_u64(next_field(&mut fields)?)?;
    let expires_at_ms = take_optional_u64(next_field(&mut fields)?)?;
    let source_cursor = take_optional_bytes(next_field(&mut fields)?)?;
    if fields.next().is_some() {
        return Err(LoomError::corrupt("delivery envelope has extra fields"));
    }
    Ok(DeliveryEnvelope {
        stream_id,
        seq,
        id,
        producer,
        subject,
        payload_digest,
        payload_len,
        created_at_ms,
        expires_at_ms,
        source_cursor,
    })
}

pub fn validate_delivery_text(field: &str, value: &str) -> Result<()> {
    if value.is_empty() {
        return Err(LoomError::invalid(format!(
            "delivery {field} must not be empty"
        )));
    }
    Ok(())
}

fn encode_value(value: &Value) -> Vec<u8> {
    loom_codec::encode(value).expect("delivery canonical CBOR is encodable")
}

fn parse_digest(value: &str) -> Result<Digest> {
    Digest::parse(value).map_err(|_| LoomError::corrupt("invalid delivery digest"))
}

fn optional_u64(value: Option<u64>) -> Value {
    value.map_or(Value::Null, Value::Uint)
}

fn optional_bytes(value: Option<&[u8]>) -> Value {
    value.map_or(Value::Null, |bytes| Value::Bytes(bytes.to_vec()))
}

fn take_array(value: Value) -> Result<Vec<Value>> {
    match value {
        Value::Array(fields) => Ok(fields),
        _ => Err(LoomError::corrupt("delivery envelope must be an array")),
    }
}

fn next_field(fields: &mut impl Iterator<Item = Value>) -> Result<Value> {
    fields
        .next()
        .ok_or_else(|| LoomError::corrupt("delivery envelope is missing a field"))
}

fn take_text(value: Value) -> Result<String> {
    match value {
        Value::Text(value) => Ok(value),
        _ => Err(LoomError::corrupt("delivery text field is invalid")),
    }
}

fn take_u64(value: Value) -> Result<u64> {
    match value {
        Value::Uint(value) => Ok(value),
        _ => Err(LoomError::corrupt("delivery u64 field is invalid")),
    }
}

fn take_optional_u64(value: Value) -> Result<Option<u64>> {
    match value {
        Value::Null => Ok(None),
        Value::Uint(value) => Ok(Some(value)),
        _ => Err(LoomError::corrupt("delivery optional u64 field is invalid")),
    }
}

fn take_optional_bytes(value: Value) -> Result<Option<Vec<u8>>> {
    match value {
        Value::Null => Ok(None),
        Value::Bytes(value) => Ok(Some(value)),
        _ => Err(LoomError::corrupt(
            "delivery optional bytes field is invalid",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_id_and_codec_round_trip() {
        let payload_digest = Digest::blake3(b"payload");
        let envelope = DeliveryEnvelope::new(
            Algo::Blake3,
            "events",
            3,
            "watch",
            "changed",
            payload_digest,
            7,
            10,
            Some(99),
            Some(b"source-cursor"),
        )
        .unwrap();

        let decoded = decode_envelope(&encode_envelope(&envelope).unwrap()).unwrap();

        assert_eq!(decoded, envelope);
        assert_eq!(
            decoded.source_cursor.as_deref(),
            Some(&b"source-cursor"[..])
        );
    }

    #[test]
    fn envelope_rejects_empty_identity_fields() {
        assert!(
            DeliveryEnvelope::new(
                Algo::Blake3,
                "",
                0,
                "watch",
                "changed",
                Digest::blake3(b"payload"),
                7,
                10,
                None,
                None,
            )
            .is_err()
        );
    }
}
