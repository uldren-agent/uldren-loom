//! Canonical native traces contracts.

use loom_codec::Value;
use loom_types::{Digest, LoomError, Result};
use std::collections::BTreeMap;

pub const SPAN_RECORD_SCHEMA: &str = "loom.traces.span.v1";
pub const MAX_TRACE_ATTRIBUTES: usize = 128;
pub const MAX_TRACE_EVENTS: usize = 128;
pub const MAX_TRACE_LINKS: usize = 128;
pub const MAX_TRACE_KEY_BYTES: usize = 128;
pub const MAX_TRACE_TEXT_BYTES: usize = 4096;
pub const MAX_TRACE_VALUE_DEPTH: usize = 16;

#[derive(Debug, Clone, PartialEq)]
pub enum TraceValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Bytes(Vec<u8>),
    Array(Vec<TraceValue>),
    Map(BTreeMap<String, TraceValue>),
}

impl TraceValue {
    pub fn validate(&self) -> Result<()> {
        self.validate_depth(0)
    }

    fn validate_depth(&self, depth: usize) -> Result<()> {
        if depth > MAX_TRACE_VALUE_DEPTH {
            return Err(LoomError::invalid("trace value nesting is too deep"));
        }
        match self {
            Self::Null | Self::Bool(_) | Self::Int(_) => Ok(()),
            Self::Float(value) if value.is_finite() => Ok(()),
            Self::Float(_) => Err(LoomError::invalid("trace float value is invalid")),
            Self::String(value) if value.len() <= MAX_TRACE_TEXT_BYTES => Ok(()),
            Self::String(_) => Err(LoomError::invalid("trace text value is too large")),
            Self::Bytes(value) if value.len() <= MAX_TRACE_TEXT_BYTES => Ok(()),
            Self::Bytes(_) => Err(LoomError::invalid("trace bytes value is too large")),
            Self::Array(values) => values
                .iter()
                .try_for_each(|value| value.validate_depth(depth.saturating_add(1))),
            Self::Map(values) => validate_value_map(values, depth.saturating_add(1)),
        }
    }

    fn encode_value(&self) -> Value {
        match self {
            Self::Null => Value::Array(vec![Value::Text("null".into()), Value::Null]),
            Self::Bool(value) => {
                Value::Array(vec![Value::Text("bool".into()), Value::Bool(*value)])
            }
            Self::Int(value) => Value::Array(vec![Value::Text("int".into()), Value::int(*value)]),
            Self::Float(value) => {
                Value::Array(vec![Value::Text("float".into()), Value::Float(*value)])
            }
            Self::String(value) => Value::Array(vec![
                Value::Text("string".into()),
                Value::Text(value.clone()),
            ]),
            Self::Bytes(value) => Value::Array(vec![
                Value::Text("bytes".into()),
                Value::Bytes(value.clone()),
            ]),
            Self::Array(values) => Value::Array(vec![
                Value::Text("array".into()),
                Value::Array(values.iter().map(Self::encode_value).collect()),
            ]),
            Self::Map(values) => Value::Array(vec![
                Value::Text("map".into()),
                Value::Map(
                    values
                        .iter()
                        .map(|(key, value)| (Value::Text(key.clone()), value.encode_value()))
                        .collect(),
                ),
            ]),
        }
    }

    fn decode_value(value: Value) -> Result<Self> {
        let fields = array(value)?;
        if fields.len() != 2 {
            return Err(LoomError::corrupt("trace value is invalid"));
        }
        let value = match text(&fields[0])? {
            "null" if fields[1] == Value::Null => Self::Null,
            "bool" => Self::Bool(bool_value(&fields[1])?),
            "int" => Self::Int(i64_value(&fields[1])?),
            "float" => Self::Float(f64_value(fields[1].clone())?),
            "string" => Self::String(text(&fields[1])?.to_owned()),
            "bytes" => Self::Bytes(bytes(&fields[1])?.to_vec()),
            "array" => Self::Array(
                array(fields[1].clone())?
                    .into_iter()
                    .map(Self::decode_value)
                    .collect::<Result<Vec<_>>>()?,
            ),
            "map" => Self::Map(decode_value_map(fields[1].clone())?),
            _ => return Err(LoomError::corrupt("trace value kind is invalid")),
        };
        value.validate()?;
        Ok(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanKind {
    Internal,
    Server,
    Client,
    Producer,
    Consumer,
}

impl SpanKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Internal => "internal",
            Self::Server => "server",
            Self::Client => "client",
            Self::Producer => "producer",
            Self::Consumer => "consumer",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "internal" => Ok(Self::Internal),
            "server" => Ok(Self::Server),
            "client" => Ok(Self::Client),
            "producer" => Ok(Self::Producer),
            "consumer" => Ok(Self::Consumer),
            _ => Err(LoomError::corrupt("unknown span kind")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SpanStatusCode {
    #[default]
    Unset,
    Ok,
    Error,
}

impl SpanStatusCode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Unset => "unset",
            Self::Ok => "ok",
            Self::Error => "error",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "unset" => Ok(Self::Unset),
            "ok" => Ok(Self::Ok),
            "error" => Ok(Self::Error),
            _ => Err(LoomError::corrupt("unknown span status")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpanContext {
    pub trace_id: [u8; 16],
    pub span_id: [u8; 8],
    pub trace_flags: u8,
}

impl SpanContext {
    pub fn new(trace_id: [u8; 16], span_id: [u8; 8], trace_flags: u8) -> Result<Self> {
        if trace_id == [0; 16] || span_id == [0; 8] || trace_flags > 1 {
            return Err(LoomError::invalid("span context is invalid"));
        }
        Ok(Self {
            trace_id,
            span_id,
            trace_flags,
        })
    }

    fn encode_value(&self) -> Value {
        Value::Array(vec![
            Value::Bytes(self.trace_id.to_vec()),
            Value::Bytes(self.span_id.to_vec()),
            Value::Uint(self.trace_flags.into()),
        ])
    }

    fn decode_value(value: Value) -> Result<Self> {
        let fields = array(value)?;
        if fields.len() != 3 {
            return Err(LoomError::corrupt("span context is invalid"));
        }
        let trace_id = fixed_bytes::<16>(&fields[0], "trace id is invalid")?;
        let span_id = fixed_bytes::<8>(&fields[1], "span id is invalid")?;
        let trace_flags = u64_value(&fields[2])?
            .try_into()
            .map_err(|_| LoomError::corrupt("trace flags are invalid"))?;
        Self::new(trace_id, span_id, trace_flags)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpanEvent {
    pub timestamp_ns: u64,
    pub name: String,
    pub attributes: BTreeMap<String, TraceValue>,
}

impl SpanEvent {
    pub fn new(
        timestamp_ns: u64,
        name: String,
        attributes: BTreeMap<String, TraceValue>,
    ) -> Result<Self> {
        let event = Self {
            timestamp_ns,
            name,
            attributes,
        };
        event.validate()?;
        Ok(event)
    }

    fn validate(&self) -> Result<()> {
        if self.timestamp_ns == 0 || self.name.is_empty() || self.name.len() > MAX_TRACE_KEY_BYTES {
            return Err(LoomError::invalid("span event is invalid"));
        }
        validate_value_map(&self.attributes, 0)
    }

    fn encode_value(&self) -> Value {
        Value::Array(vec![
            Value::Uint(self.timestamp_ns),
            Value::Text(self.name.clone()),
            encode_value_map(&self.attributes),
        ])
    }

    fn decode_value(value: Value) -> Result<Self> {
        let fields = array(value)?;
        if fields.len() != 3 {
            return Err(LoomError::corrupt("span event is invalid"));
        }
        Self::new(
            u64_value(&fields[0])?,
            text(&fields[1])?.to_owned(),
            decode_value_map(fields[2].clone())?,
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpanLink {
    pub context: SpanContext,
    pub attributes: BTreeMap<String, TraceValue>,
}

impl SpanLink {
    pub fn new(context: SpanContext, attributes: BTreeMap<String, TraceValue>) -> Result<Self> {
        let link = Self {
            context,
            attributes,
        };
        link.validate()?;
        Ok(link)
    }

    fn validate(&self) -> Result<()> {
        SpanContext::new(
            self.context.trace_id,
            self.context.span_id,
            self.context.trace_flags,
        )?;
        validate_value_map(&self.attributes, 0)
    }

    fn encode_value(&self) -> Value {
        Value::Array(vec![
            self.context.encode_value(),
            encode_value_map(&self.attributes),
        ])
    }

    fn decode_value(value: Value) -> Result<Self> {
        let fields = array(value)?;
        if fields.len() != 2 {
            return Err(LoomError::corrupt("span link is invalid"));
        }
        Self::new(
            SpanContext::decode_value(fields[0].clone())?,
            decode_value_map(fields[1].clone())?,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SpanDetails {
    pub parent_span_id: Option<[u8; 8]>,
    pub observed_time_ns: Option<u64>,
    pub status_code: SpanStatusCode,
    pub status_message: String,
    pub attributes: BTreeMap<String, TraceValue>,
    pub resource: BTreeMap<String, TraceValue>,
    pub scope: BTreeMap<String, TraceValue>,
    pub events: Vec<SpanEvent>,
    pub links: Vec<SpanLink>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpanRecord {
    pub context: SpanContext,
    pub parent_span_id: Option<[u8; 8]>,
    pub name: String,
    pub kind: SpanKind,
    pub start_time_ns: u64,
    pub end_time_ns: u64,
    pub observed_time_ns: Option<u64>,
    pub status_code: SpanStatusCode,
    pub status_message: String,
    pub attributes: BTreeMap<String, TraceValue>,
    pub resource: BTreeMap<String, TraceValue>,
    pub scope: BTreeMap<String, TraceValue>,
    pub events: Vec<SpanEvent>,
    pub links: Vec<SpanLink>,
}

impl SpanRecord {
    pub fn new(
        context: SpanContext,
        name: String,
        kind: SpanKind,
        start_time_ns: u64,
        end_time_ns: u64,
    ) -> Result<Self> {
        let span = Self {
            context,
            parent_span_id: None,
            name,
            kind,
            start_time_ns,
            end_time_ns,
            observed_time_ns: None,
            status_code: SpanStatusCode::Unset,
            status_message: String::new(),
            attributes: BTreeMap::new(),
            resource: BTreeMap::new(),
            scope: BTreeMap::new(),
            events: Vec::new(),
            links: Vec::new(),
        };
        span.validate()?;
        Ok(span)
    }

    pub fn with_details(mut self, details: SpanDetails) -> Result<Self> {
        self.parent_span_id = details.parent_span_id;
        self.observed_time_ns = details.observed_time_ns;
        self.status_code = details.status_code;
        self.status_message = details.status_message;
        self.attributes = details.attributes;
        self.resource = details.resource;
        self.scope = details.scope;
        self.events = details.events;
        self.links = details.links;
        self.validate()?;
        Ok(self)
    }

    pub fn validate(&self) -> Result<()> {
        SpanContext::new(
            self.context.trace_id,
            self.context.span_id,
            self.context.trace_flags,
        )?;
        if self.parent_span_id == Some(self.context.span_id)
            || self.parent_span_id == Some([0; 8])
            || self.name.is_empty()
            || self.name.len() > MAX_TRACE_TEXT_BYTES
            || self.start_time_ns == 0
            || self.end_time_ns < self.start_time_ns
            || self.status_message.len() > MAX_TRACE_TEXT_BYTES
            || self.events.len() > MAX_TRACE_EVENTS
            || self.links.len() > MAX_TRACE_LINKS
        {
            return Err(LoomError::invalid("span record is invalid"));
        }
        if let Some(observed_time_ns) = self.observed_time_ns
            && (observed_time_ns == 0 || observed_time_ns < self.start_time_ns)
        {
            return Err(LoomError::invalid("span observed time is invalid"));
        }
        validate_value_map(&self.attributes, 0)?;
        validate_value_map(&self.resource, 0)?;
        validate_value_map(&self.scope, 0)?;
        self.events.iter().try_for_each(SpanEvent::validate)?;
        self.links.iter().try_for_each(SpanLink::validate)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        loom_codec::encode(&Value::Array(vec![
            Value::Text(SPAN_RECORD_SCHEMA.into()),
            self.context.encode_value(),
            optional_span_id(&self.parent_span_id),
            Value::Text(self.name.clone()),
            Value::Text(self.kind.as_str().into()),
            Value::Uint(self.start_time_ns),
            Value::Uint(self.end_time_ns),
            optional_u64(self.observed_time_ns),
            Value::Text(self.status_code.as_str().into()),
            Value::Text(self.status_message.clone()),
            encode_value_map(&self.attributes),
            encode_value_map(&self.resource),
            encode_value_map(&self.scope),
            Value::Array(self.events.iter().map(SpanEvent::encode_value).collect()),
            Value::Array(self.links.iter().map(SpanLink::encode_value).collect()),
        ]))
        .map_err(|err| LoomError::invalid(format!("span record encoding failed: {err}")))
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let fields = array(
            loom_codec::decode(bytes)
                .map_err(|err| LoomError::corrupt(format!("invalid span CBOR: {err}")))?,
        )?;
        if fields.len() != 15 || text(&fields[0])? != SPAN_RECORD_SCHEMA {
            return Err(LoomError::corrupt("invalid span schema"));
        }
        let span = Self {
            context: SpanContext::decode_value(fields[1].clone())?,
            parent_span_id: optional_span_id_value(&fields[2])?,
            name: text(&fields[3])?.to_owned(),
            kind: SpanKind::parse(text(&fields[4])?)?,
            start_time_ns: u64_value(&fields[5])?,
            end_time_ns: u64_value(&fields[6])?,
            observed_time_ns: optional_u64_value(&fields[7])?,
            status_code: SpanStatusCode::parse(text(&fields[8])?)?,
            status_message: text(&fields[9])?.to_owned(),
            attributes: decode_value_map(fields[10].clone())?,
            resource: decode_value_map(fields[11].clone())?,
            scope: decode_value_map(fields[12].clone())?,
            events: array(fields[13].clone())?
                .into_iter()
                .map(SpanEvent::decode_value)
                .collect::<Result<Vec<_>>>()?,
            links: array(fields[14].clone())?
                .into_iter()
                .map(SpanLink::decode_value)
                .collect::<Result<Vec<_>>>()?,
        };
        span.validate()?;
        Ok(span)
    }

    pub fn record_id(&self) -> Result<String> {
        Ok(Digest::blake3(&self.encode()?).to_hex())
    }

    pub fn trace_id_hex(&self) -> String {
        hex_lower(&self.context.trace_id)
    }

    pub fn span_id_hex(&self) -> String {
        hex_lower(&self.context.span_id)
    }
}

fn validate_value_map(values: &BTreeMap<String, TraceValue>, depth: usize) -> Result<()> {
    if values.len() > MAX_TRACE_ATTRIBUTES {
        return Err(LoomError::invalid("trace attribute count is too large"));
    }
    for (key, value) in values {
        if key.is_empty() || key.len() > MAX_TRACE_KEY_BYTES || key.chars().any(char::is_control) {
            return Err(LoomError::invalid("trace attribute key is invalid"));
        }
        value.validate_depth(depth)?;
    }
    Ok(())
}

fn encode_value_map(values: &BTreeMap<String, TraceValue>) -> Value {
    Value::Map(
        values
            .iter()
            .map(|(key, value)| (Value::Text(key.clone()), value.encode_value()))
            .collect(),
    )
}

fn decode_value_map(value: Value) -> Result<BTreeMap<String, TraceValue>> {
    let mut values = BTreeMap::new();
    for (key, value) in map(value)? {
        let key = text(&key)?.to_owned();
        if values
            .insert(key, TraceValue::decode_value(value)?)
            .is_some()
        {
            return Err(LoomError::corrupt("duplicate trace attribute key"));
        }
    }
    validate_value_map(&values, 0)?;
    Ok(values)
}

fn optional_span_id(value: &Option<[u8; 8]>) -> Value {
    value.map_or(Value::Null, |value| Value::Bytes(value.to_vec()))
}

fn optional_span_id_value(value: &Value) -> Result<Option<[u8; 8]>> {
    if *value == Value::Null {
        Ok(None)
    } else {
        fixed_bytes::<8>(value, "parent span id is invalid").map(Some)
    }
}

fn optional_u64(value: Option<u64>) -> Value {
    value.map_or(Value::Null, Value::Uint)
}

fn optional_u64_value(value: &Value) -> Result<Option<u64>> {
    if *value == Value::Null {
        Ok(None)
    } else {
        Ok(Some(u64_value(value)?))
    }
}

fn fixed_bytes<const N: usize>(value: &Value, message: &'static str) -> Result<[u8; N]> {
    bytes(value)?
        .try_into()
        .map_err(|_| LoomError::corrupt(message))
}

fn hex_lower(bytes: &[u8]) -> String {
    const CHARS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(CHARS[(byte >> 4) as usize] as char);
        out.push(CHARS[(byte & 0x0f) as usize] as char);
    }
    out
}

fn array(value: Value) -> Result<Vec<Value>> {
    match value {
        Value::Array(values) => Ok(values),
        _ => Err(LoomError::corrupt("expected array")),
    }
}

fn map(value: Value) -> Result<Vec<(Value, Value)>> {
    match value {
        Value::Map(values) => Ok(values),
        _ => Err(LoomError::corrupt("expected map")),
    }
}

fn text(value: &Value) -> Result<&str> {
    match value {
        Value::Text(value) => Ok(value),
        _ => Err(LoomError::corrupt("expected text")),
    }
}

fn bytes(value: &Value) -> Result<&[u8]> {
    match value {
        Value::Bytes(value) => Ok(value),
        _ => Err(LoomError::corrupt("expected bytes")),
    }
}

fn bool_value(value: &Value) -> Result<bool> {
    match value {
        Value::Bool(value) => Ok(*value),
        _ => Err(LoomError::corrupt("expected bool")),
    }
}

fn u64_value(value: &Value) -> Result<u64> {
    match value {
        Value::Uint(value) => Ok(*value),
        _ => Err(LoomError::corrupt("expected unsigned integer")),
    }
}

fn i64_value(value: &Value) -> Result<i64> {
    match value {
        Value::Uint(value) => (*value)
            .try_into()
            .map_err(|_| LoomError::corrupt("integer is out of range")),
        Value::Nint(value) => {
            let positive: i64 = (*value)
                .try_into()
                .map_err(|_| LoomError::corrupt("integer is out of range"))?;
            Ok(-1 - positive)
        }
        _ => Err(LoomError::corrupt("expected signed integer")),
    }
}

fn f64_value(value: Value) -> Result<f64> {
    match value {
        Value::Float(value) if value.is_finite() => Ok(value),
        _ => Err(LoomError::corrupt("expected finite float")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn span_round_trips() {
        let context = SpanContext::new([1; 16], [2; 8], 1).unwrap();
        let span = SpanRecord::new(context, "GET /items".into(), SpanKind::Server, 10, 20)
            .unwrap()
            .with_details(SpanDetails {
                parent_span_id: Some([3; 8]),
                observed_time_ns: Some(30),
                status_code: SpanStatusCode::Ok,
                attributes: BTreeMap::from([(
                    "http.method".into(),
                    TraceValue::String("GET".into()),
                )]),
                resource: BTreeMap::from([(
                    "service.name".into(),
                    TraceValue::String("api".into()),
                )]),
                scope: BTreeMap::from([("name".into(), TraceValue::String("loom".into()))]),
                events: vec![
                    SpanEvent::new(
                        12,
                        "db.query".into(),
                        BTreeMap::from([("rows".into(), TraceValue::Int(3))]),
                    )
                    .unwrap(),
                ],
                links: vec![
                    SpanLink::new(
                        SpanContext::new([4; 16], [5; 8], 0).unwrap(),
                        BTreeMap::new(),
                    )
                    .unwrap(),
                ],
                ..SpanDetails::default()
            })
            .unwrap();
        assert_eq!(SpanRecord::decode(&span.encode().unwrap()).unwrap(), span);
    }
}
